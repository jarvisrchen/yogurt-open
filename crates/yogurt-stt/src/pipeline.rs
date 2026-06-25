//! The local meeting pipeline both engines run: a VAD segmenter per channel, a final decode per utterance, and a partial ticker.
//!
//! The engine only supplies [`Decode`]. Timestamps come from the segmenter, never the engine.
//!
//! ## Runtime safety invariant
//!
//! Both engines' decodes are synchronous C++ that block for hundreds of ms to seconds, which would starve tokio's worker threads.
//! Every decode site (mic final, system final, partial ticker) must stay inside `tokio::task::spawn_blocking`.
//! Removing one deadlocks the WS broadcast pump under load.
//!
//! ## Partial ticker reads the segmenter
//!
//! The ticker has no audio buffer of its own: `Segmenter::buffer` already is the in-flight utterance, which only grows in speech and is bounded by `MAX_SEGMENT_SAMPLES`.
//! A separate rolling window would make partials shrink once an utterance outran it, and would see only the mic channel.
//!
//! The segmenters therefore live behind `Arc<Mutex<_>>`, shared with the pump.
//! It is a `std::sync::Mutex` so that holding it across an await is a compile error (the future stops being `Send`) rather than a stall of the audio pump.

use crate::vad::{Segmenter, SegmenterEvent};
use crate::{AudioChunk, AudioRx, Channel, TranscriptEvent, TranscriptTx};
use anyhow::Result;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// Decode 16 kHz mono PCM into text and an optional confidence. `fast` asks for a cheaper preview decode where the engine has one.
///
/// **Blocks the calling thread.** The pipeline calls it only from `spawn_blocking`.
pub(crate) type Decode = Arc<dyn Fn(&[i16], bool) -> Result<(String, Option<f32>)> + Send + Sync>;

/// Run the pipeline for the lifetime of the audio stream, returning when the upstream audio broadcast closes.
///
/// Spawns a final decoder per channel, a partial ticker that decodes one channel per second round-robin, and the audio pump.
pub(crate) async fn run(decode: Decode, mut audio_rx: AudioRx, txn: TranscriptTx) -> Result<()> {
    let decode_mic = decode.clone();
    let decode_sys = decode.clone();
    let decode_partial = decode;

    // Shared with the partial ticker, which reads each segmenter's in-flight utterance.
    let mic_seg = Arc::new(Mutex::new(Segmenter::new(16_000)));
    let sys_seg = Arc::new(Mutex::new(Segmenter::new(16_000)));

    // Small queue: under sustained overload `try_send` drops rather than blocking the audio pump, like the deepgram adapter.
    let (mic_seg_tx, mut mic_seg_rx) = mpsc::channel::<(Vec<i16>, u64, u64)>(8);
    let (sys_seg_tx, mut sys_seg_rx) = mpsc::channel::<(Vec<i16>, u64, u64)>(8);

    let txn_mic = txn.clone();
    tokio::spawn(async move {
        while let Some((pcm, start_ms, _end_ms)) = mic_seg_rx.recv().await {
            let decode = decode_mic.clone();
            // Must run on a blocking thread.
            let (text, confidence) = tokio::task::spawn_blocking(move || {
                decode(&pcm, /* fast */ false)
            })
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or_default();
            if text.is_empty() {
                continue;
            }
            let _ = txn_mic.send(TranscriptEvent {
                ts_ms: start_ms,
                channel: Channel::Mic,
                text,
                is_final: true,
                confidence,
            });
        }
    });

    let txn_sys = txn.clone();
    tokio::spawn(async move {
        while let Some((pcm, start_ms, _end_ms)) = sys_seg_rx.recv().await {
            let decode = decode_sys.clone();
            // Must run on a blocking thread.
            let (text, confidence) = tokio::task::spawn_blocking(move || {
                decode(&pcm, /* fast */ false)
            })
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or_default();
            if text.is_empty() {
                continue;
            }
            let _ = txn_sys.send(TranscriptEvent {
                ts_ms: start_ms,
                channel: Channel::System,
                text,
                is_final: true,
                confidence,
            });
        }
    });

    // Decoding both channels per tick does not fit: a full 25 s buffer decodes in ~585 ms on large-v3-turbo,
    // so two would exceed the 1 s tick and `interval`'s default `Burst` behavior would spin catching up.
    // `examples/partial_decode_cost.rs` re-measures this. If a model pushes the 25 s row past ~1 s, raise the interval, not shrink the buffer.
    // Two simultaneous speakers each refresh every 2 s. A single speaker still refreshes every 1 s because an idle channel's `pending()` is `None`.
    // The ticker loops forever, so tie its lifetime to this `run()` future.
    // Dropping a plain JoinHandle detaches, which would keep the multi-GB model alive and burn a decode per second after the meeting ended.
    struct AbortOnDrop(tokio::task::JoinHandle<()>);
    impl Drop for AbortOnDrop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    let sources = [
        (mic_seg.clone(), Channel::Mic),
        (sys_seg.clone(), Channel::System),
    ];
    let txn_partial = txn.clone();
    let _ticker_guard = AbortOnDrop(tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_millis(1000));
        // A decode longer than the tick (Parakeet on the CPU of a slower Mac) must not fire the missed ticks back to back.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Skip the immediate first tick so the first decode fires after 1 s of audio.
        ticker.tick().await;
        let mut next_source = 0usize;
        loop {
            ticker.tick().await;

            let mut picked = None;
            for _ in 0..sources.len() {
                let (seg, channel) = &sources[next_source];
                next_source = (next_source + 1) % sources.len();
                // Scoped so the std guard is released before the decode.
                let snapshot = {
                    let seg = seg.lock().unwrap_or_else(|e| e.into_inner());
                    seg.pending()
                        .filter(|(pcm, _)| pcm.len() >= 16_000)
                        .map(|(pcm, start_ms)| (pcm.to_vec(), start_ms))
                };
                if let Some((pcm, start_ms)) = snapshot {
                    picked = Some((pcm, start_ms, *channel));
                    break;
                }
            }
            let Some((pcm, start_ms, channel)) = picked else {
                continue;
            };

            let decode = decode_partial.clone();
            // Must run on a blocking thread.
            let (text, confidence) = tokio::task::spawn_blocking(move || {
                decode(&pcm, /* fast */ true)
            })
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or_default();
            if text.is_empty() {
                continue;
            }
            let _ = txn_partial.send(TranscriptEvent {
                // The utterance's own start, so the timestamp does not jump when the final replaces the partial.
                ts_ms: start_ms,
                channel,
                text,
                is_final: false,
                confidence,
            });
        }
    }));

    loop {
        match audio_rx.recv().await {
            Ok(chunk) => {
                let AudioChunk {
                    channel,
                    samples,
                    ts_ms: _,
                } = chunk;
                match channel {
                    Channel::Mic => {
                        let tx_seg = mic_seg_tx.clone();
                        let mut seg = mic_seg.lock().unwrap_or_else(|e| e.into_inner());
                        seg.push(&samples, |e| {
                            // An exhaustive match makes a new segmenter event a compile error.
                            match e {
                                SegmenterEvent::Segment {
                                    pcm,
                                    start_ms,
                                    end_ms,
                                } => {
                                    let _ = tx_seg.try_send((pcm, start_ms, end_ms));
                                }
                                SegmenterEvent::SpeechStart { .. } => {}
                            }
                        });
                    }
                    Channel::System => {
                        let tx_seg = sys_seg_tx.clone();
                        let mut seg = sys_seg.lock().unwrap_or_else(|e| e.into_inner());
                        seg.push(&samples, |e| match e {
                            SegmenterEvent::Segment {
                                pcm,
                                start_ms,
                                end_ms,
                            } => {
                                let _ = tx_seg.try_send((pcm, start_ms, end_ms));
                            }
                            SegmenterEvent::SpeechStart { .. } => {}
                        });
                    }
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                // Debug level: a burst while the model loads is not actionable.
                tracing::debug!(?n, "local stt audio rx lagged; dropping");
                continue;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                tracing::info!("audio channel closed; local stt exiting");
                break;
            }
        }
    }

    drop(mic_seg_tx);
    drop(sys_seg_tx);
    Ok(())
}
