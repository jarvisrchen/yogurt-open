use crate::models::{Engine, ModelSpec};
use crate::{AudioRx, ParakeetLocal, Stt, TranscriptTx, WhisperLocal};
use anyhow::{bail, Result};
use async_trait::async_trait;
use std::path::PathBuf;

pub enum LocalModel {
    Whisper(WhisperLocal),
    Parakeet(ParakeetLocal),
}

impl LocalModel {
    /// Load `spec` from `path`, its first file as [`crate::models::resolve_model`] returns it, with the other files beside it.
    ///
    /// **Blocking:** async callers wrap it in `tokio::task::spawn_blocking`.
    pub fn load(spec: &ModelSpec, path: PathBuf) -> Result<Self> {
        match spec.engine {
            Engine::Whisper => WhisperLocal::load(path).map(Self::Whisper),
            Engine::Parakeet => {
                let file = |i: usize| path.with_file_name(spec.files[i].filename);
                ParakeetLocal::load(&file(0), &file(1), &file(2), &file(3)).map(Self::Parakeet)
            }
            Engine::Llama => bail!("{} is not a transcription model", spec.name),
        }
    }

    /// Settings "Test": load the model and transcribe a 1 s silent clip, which proves inference works on this Mac
    /// before a meeting finds out. An empty transcript is the expected result.
    ///
    /// **Blocking:** async callers must use `tokio::task::spawn_blocking`.
    pub fn self_test(spec: &ModelSpec, path: PathBuf) -> Result<()> {
        Self::load(spec, path)?.transcribe(&[0i16; 16_000], &[])?;
        Ok(())
    }

    /// whisper takes the dictionary `words` in its initial prompt. Parakeet ignores them, since its hotwords need a beam search decoder too slow for a long dictation.
    ///
    /// **Blocks the calling thread.** Call it from `tokio::task::spawn_blocking`.
    pub fn transcribe(&self, pcm_i16: &[i16], words: &[String]) -> Result<String> {
        match self {
            Self::Whisper(w) => w.transcribe(pcm_i16, words),
            Self::Parakeet(p) => p.transcribe(pcm_i16),
        }
    }
}

#[async_trait]
impl Stt for LocalModel {
    async fn start(&self, audio_rx: AudioRx, txn: TranscriptTx, words: &[String]) -> Result<()> {
        match self {
            Self::Whisper(w) => w.start(audio_rx, txn, words).await,
            Self::Parakeet(p) => p.start(audio_rx, txn, words).await,
        }
    }
}
