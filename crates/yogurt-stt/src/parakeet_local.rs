//! Local STT with NVIDIA's Parakeet TDT 0.6B v3 through sherpa-onnx, the way the Android app runs it.
//!
//! The VAD in [`crate::pipeline`] cuts speech at pauses and each utterance goes to the offline transducer whole, with greedy search on the CPU.
//! CoreML ran slower than the CPU, and the CPU keeps Parakeet off the GPU a meeting's whisper would be using.
//! It is multilingual and picks the language itself, and it gives no confidence, so the meeting's low-confidence filter never drops its lines.

use crate::{AudioRx, Stt, TranscriptTx};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig};
use std::path::Path;
use std::sync::{Arc, Mutex, TryLockError};

/// Past 8 threads the work spills onto the efficiency cores and gets slower.
const MAX_THREADS: usize = 8;

/// One recognizer, which decodes one clip at a time.
pub struct ParakeetLocal {
    recognizer: Arc<Mutex<OfflineRecognizer>>,
}

impl ParakeetLocal {
    /// **Blocking:** it reads and initializes about 670 MB of weights, so async callers wrap it in `spawn_blocking`.
    pub fn load(encoder: &Path, decoder: &Path, joiner: &Path, tokens: &Path) -> Result<Self> {
        let path = |p: &Path| -> Result<String> {
            if !p.is_file() {
                return Err(anyhow!("Parakeet file not found at {}", p.display()));
            }
            p.to_str()
                .map(str::to_string)
                .ok_or_else(|| anyhow!("model path is not utf-8: {}", p.display()))
        };
        let mut c = OfflineRecognizerConfig::default();
        c.model_config.transducer.encoder = Some(path(encoder)?);
        c.model_config.transducer.decoder = Some(path(decoder)?);
        c.model_config.transducer.joiner = Some(path(joiner)?);
        c.model_config.tokens = Some(path(tokens)?);
        c.model_config.model_type = Some("nemo_transducer".into());
        c.model_config.provider = Some("cpu".into());
        c.model_config.num_threads = std::thread::available_parallelism()
            .map_or(MAX_THREADS, |n| n.get().min(MAX_THREADS))
            as i32;
        c.decoding_method = Some("greedy_search".into());
        // sherpa-onnx returns no reason, and logs it to stderr.
        let recognizer = OfflineRecognizer::create(&c).ok_or_else(|| {
            anyhow!(
                "sherpa-onnx could not load Parakeet from {}",
                encoder.display()
            )
        })?;
        Ok(Self {
            recognizer: Arc::new(Mutex::new(recognizer)),
        })
    }

    /// **Blocks the calling thread.** Call it from `tokio::task::spawn_blocking`.
    pub fn transcribe(&self, pcm_i16: &[i16]) -> Result<String> {
        Self::decode(&self.recognizer, pcm_i16, false).map(|(text, _)| text)
    }

    /// A preview (`fast`) skips the decode while another one runs, so partials never queue up.
    /// A final waits for at most the one decode in flight.
    fn decode(
        recognizer: &Mutex<OfflineRecognizer>,
        pcm_i16: &[i16],
        fast: bool,
    ) -> Result<(String, Option<f32>)> {
        let recognizer = match recognizer.try_lock() {
            Ok(r) => r,
            Err(TryLockError::WouldBlock) if fast => return Ok((String::new(), None)),
            Err(_) => recognizer.lock().unwrap_or_else(|e| e.into_inner()),
        };
        let samples: Vec<f32> = pcm_i16.iter().map(|&s| f32::from(s) / 32768.0).collect();
        let stream = recognizer.create_stream();
        stream.accept_waveform(16_000, &samples);
        recognizer.decode(&stream);
        let text = stream
            .get_result()
            .ok_or_else(|| anyhow!("sherpa-onnx returned no result"))?
            .text;
        Ok((text.trim().to_string(), None))
    }
}

#[async_trait]
impl Stt for ParakeetLocal {
    async fn start(&self, audio_rx: AudioRx, txn: TranscriptTx, _words: &[String]) -> Result<()> {
        let recognizer = self.recognizer.clone();
        crate::pipeline::run(
            Arc::new(move |pcm: &[i16], fast| Self::decode(&recognizer, pcm, fast)),
            audio_rx,
            txn,
        )
        .await
    }
}
