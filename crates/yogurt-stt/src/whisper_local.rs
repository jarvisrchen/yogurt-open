//! Local STT via whisper.cpp, implementing the same [`Stt`] trait as [`crate::deepgram::DeepgramStt`].
//!
//! One `WhisperContext` (the model weights) is shared by `Arc`, and a fresh `WhisperState` is created per decode because `create_state` is cheap and pooling complicates `spawn_blocking` lifetimes.
//!
//! Two sampling configurations run against it:
//!
//! - Preview (`fast=true`): greedy, fired every 1 s on the utterance in flight from `Segmenter::pending`, emitting `is_final: false`.
//!   `set_no_context(true)` keeps the partial decoder from carrying cross-segment hallucinations.
//! - Final (`fast=false`): beam search on full utterances from the VAD segmenter, emitting `is_final: true`.
//!   `set_no_context(false)` lets each segment reference prior context.
//!
//! The meeting pipeline around the decode lives in [`crate::pipeline`].

use crate::{AudioRx, Stt, TranscriptTx};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// The most dictionary words in the initial prompt, since a longer one makes whisper hear listed words that were not said.
const PROMPT_WORDS: usize = 50;

/// The first [`PROMPT_WORDS`] of `words` in reverse, so the ones that matter most sit last:
/// whisper.cpp keeps only the last 224 tokens of a prompt that runs over.
fn initial_prompt(words: &[String]) -> String {
    let mut first: Vec<&str> = words
        .iter()
        .take(PROMPT_WORDS)
        .map(String::as_str)
        .collect();
    first.reverse();
    first.join(", ")
}

/// Whisper.cpp adapter. Owns the model context and drives the preview and final decode pipeline.
pub struct WhisperLocal {
    /// Kept for diagnostics and error messages.
    #[allow(dead_code)]
    model_path: PathBuf,
    /// Heavyweight (~80 MB for tiny.en, ~3 GB for large-v3), shared across worker tasks by `Arc::clone`.
    ctx: Arc<WhisperContext>,
}

impl WhisperLocal {
    /// Load a ggml whisper model from disk.
    ///
    /// **Blocking:** it mmaps the model and runs ggml's table init on the calling thread, so async callers must wrap it in `tokio::task::spawn_blocking`.
    /// It is not wrapped here because tests call it from synchronous setup and the Settings path should not need an async context just to load a model.
    pub fn load(model_path: PathBuf) -> Result<Self> {
        if !model_path.exists() {
            return Err(anyhow!(
                "whisper model not found at {} — run the Settings → Download flow first",
                model_path.display()
            ));
        }
        let path_str = model_path
            .to_str()
            .context("model path is not utf-8")?
            .to_string();
        // ggml writes to stderr through its own log callback, which `set_print_*(false)` in `decode` does not control.
        // Left alone that is 45 lines per load and 17 per decode, and the partial ticker decodes every second.
        // This redirects it into `tracing`, where the default filter `whisper_rs=warn` still surfaces warnings.
        // Must run before the first ggml call; `Once`-guarded inside whisper-rs.
        whisper_rs::install_logging_hooks();
        let ctx = WhisperContext::new_with_params(&path_str, WhisperContextParameters::default())
            .context("loading whisper model")?;
        Ok(Self {
            model_path,
            ctx: Arc::new(ctx),
        })
    }

    /// Beam-search one whole clip, such as a Voice dictation, with the dictionary `words` in the initial prompt.
    ///
    /// **Blocks the calling thread.** Call it from `tokio::task::spawn_blocking`.
    pub fn transcribe(&self, pcm_i16: &[i16], words: &[String]) -> Result<String> {
        Self::decode(
            &self.ctx,
            pcm_i16,
            /* fast */ false,
            &initial_prompt(words),
        )
        .map(|(text, _)| text)
    }

    /// Run whisper.cpp on 16 kHz mono PCM and return the joined transcript and a confidence.
    /// `fast` selects greedy preview over beam-search final.
    ///
    /// **Blocks the calling thread.** Call it from `tokio::task::spawn_blocking`.
    fn decode(
        ctx: &WhisperContext,
        pcm_i16: &[i16],
        fast: bool,
        prompt: &str,
    ) -> Result<(String, Option<f32>)> {
        // whisper-rs wants f32 in [-1.0, 1.0].
        let mut f32_buf = vec![0.0f32; pcm_i16.len()];
        whisper_rs::convert_integer_to_float_audio(pcm_i16, &mut f32_buf)
            .context("pcm conversion")?;

        // `create_state` allocates working buffers (KV cache), not the model weights.
        let mut state = ctx.create_state().context("create whisper state")?;

        let mut params = if fast {
            FullParams::new(SamplingStrategy::Greedy { best_of: 1 })
        } else {
            FullParams::new(SamplingStrategy::BeamSearch {
                beam_size: 5,
                patience: 1.0,
            })
        };

        params.set_language(Some("en"));
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        // Partials re-decode the same overlapping audio, so they must not carry context.
        params.set_no_context(fast);
        // Finals only: a partial decodes every second, and the prompt's prefill on each was never measured.
        if !fast && !prompt.is_empty() {
            params.set_initial_prompt(prompt);
        }

        state.full(params, &f32_buf).context("whisper decode")?;

        let n_segments = state.full_n_segments();
        let mut out = String::new();
        let mut max_no_speech = 0.0f32;
        for i in 0..n_segments {
            if let Some(seg) = state.get_segment(i) {
                if let Ok(s) = seg.to_str_lossy() {
                    out.push_str(&s);
                }
                max_no_speech = max_no_speech.max(seg.no_speech_probability());
            }
        }
        let confidence = if n_segments > 0 {
            Some(1.0 - max_no_speech)
        } else {
            None
        };
        Ok((out.trim().to_string(), confidence))
    }
}

#[async_trait]
impl Stt for WhisperLocal {
    async fn start(&self, audio_rx: AudioRx, txn: TranscriptTx, words: &[String]) -> Result<()> {
        let ctx = self.ctx.clone();
        let prompt = initial_prompt(words);
        crate::pipeline::run(
            Arc::new(move |pcm: &[i16], fast| Self::decode(&ctx, pcm, fast, &prompt)),
            audio_rx,
            txn,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_keeps_the_first_words_and_puts_them_last() {
        let words: Vec<String> = (0..60).map(|i| format!("w{i}")).collect();
        let prompt = initial_prompt(&words);
        assert!(prompt.starts_with("w49, w48, "), "{prompt}");
        assert!(prompt.ends_with(", w1, w0"), "{prompt}");
        assert_eq!(prompt.split(", ").count(), PROMPT_WORDS);
        assert_eq!(initial_prompt(&[]), "");
    }

    #[test]
    fn whisper_local_is_object_safe() {
        fn _assert_dyn(_: &dyn Stt) {}
        let _f: fn(&dyn Stt) = _assert_dyn;
    }
}
