//! Deepgram streaming adapter for `wss://api.deepgram.com/v1/listen`.
//!
//! Audio goes in as binary frames of 16 kHz mono linear16 PCM, events come back as JSON `Results` frames.
//!
//! Each `Stt::start` opens one WS per [`Channel`], so a "Me" line is never tagged "Them".
//! That costs 2x Deepgram seconds but preserves the channel label without diarization.
//!
//! Reliability:
//!
//! - Backpressure: the per-channel mpsc uses `try_send`, so a stalled WS never blocks the other channel.
//!   `MAX_CONSECUTIVE_DROPS` drops in a row emit a synthetic overload event.
//! - Reconnect: transient errors retry up to `MAX_RECONNECT_ATTEMPTS` times with 1s, 2s, 4s backoff and a synthetic status event.
//! - Auth: HTTP 401/403 on the upgrade is terminal, with an "[stt auth failed]" event so the user can fix the key.

use crate::{AudioChunk, AudioRx, Channel, Stt, TranscriptEvent, TranscriptTx};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

/// Consecutive `try_send` failures before a channel session is declared unrecoverable (about 1 s of audio at ~50 fps).
const MAX_CONSECUTIVE_DROPS: usize = 50;

const MAX_RECONNECT_ATTEMPTS: usize = 3;

/// Deepgram rejects a request whose keyterms pass 500 tokens.
/// A token covers at least one byte of a term, so terms whose bytes add up to this always fit.
/// Each term also counts one byte more, in case Deepgram spends a token between terms.
const KEYTERM_BYTES: usize = 500;

pub struct DeepgramStt {
    pub api_key: String,
    /// Overrides the WS base URL (used by tests).
    pub base_url: String,
    pub model: String,
}

impl DeepgramStt {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "wss://api.deepgram.com".into(),
            // nova-3 handles far-field and noisy audio better: nova-2 silently drops clauses below its confidence floor.
            model: std::env::var("YOGURT_DEEPGRAM_MODEL").unwrap_or_else(|_| "nova-3".into()),
        }
    }

    /// Build the connect URL with all query params baked in.
    ///
    /// `endpointing` is the silence in ms that closes an utterance into one transcript line.
    /// At 300 ms every breath started a new line, while 1000 ms tracks sentence boundaries and interim partials still stream live.
    fn connect_url(&self, words: &[String]) -> String {
        self.listen_url("&interim_results=true&endpointing=1000", words)
    }

    fn listen_url(&self, extra: &str, words: &[String]) -> String {
        format!(
            "{base}/v1/listen?model={model}\
             &encoding=linear16&sample_rate=16000&channels=1&smart_format=true{extra}{keyterms}",
            base = self.base_url,
            model = self.model,
            keyterms = self.keyterms(words),
        )
    }

    /// A `keyterm` param for each of the first `words` that fit [`KEYTERM_BYTES`].
    /// Only Nova-3 takes keyterms, so another model set through `YOGURT_DEEPGRAM_MODEL` gets none.
    fn keyterms(&self, words: &[String]) -> String {
        if !self.model.starts_with("nova-3") {
            return String::new();
        }
        let mut left = KEYTERM_BYTES;
        words
            .iter()
            .take_while(|w| match left.checked_sub(w.len() + 1) {
                Some(rest) => {
                    left = rest;
                    true
                }
                None => false,
            })
            .map(|w| format!("&keyterm={}", percent_encode(w)))
            .collect()
    }

    /// Transcribes one dictation: streams `audio` as it arrives, sends `CloseStream` once it closes, and returns every final joined.
    /// Frames sent before the socket opens wait in the channel and go out first.
    /// Dropping the future closes the socket.
    pub async fn dictate(
        &self,
        mut audio: tokio::sync::mpsc::UnboundedReceiver<Vec<i16>>,
        words: &[String],
    ) -> anyhow::Result<String> {
        // No interim results, so every `Results` frame is a final.
        let ws = connect_once(self, self.listen_url("", words), Channel::Mic)
            .await
            .map_err(|e| match e {
                ConnectError::Auth => anyhow::anyhow!("Deepgram rejected the key"),
                ConnectError::Transient(msg) => anyhow::anyhow!("Deepgram connect failed: {msg}"),
            })?;
        let (mut write, mut read) = ws.split();
        let mut text = String::new();
        let mut recording = true;
        let mut flushed = false;
        loop {
            tokio::select! {
                msg = read.next() => match msg {
                    Some(Ok(Message::Text(t))) => {
                        let v: serde_json::Value = serde_json::from_str(&t).unwrap_or_default();
                        match v.get("type").and_then(|t| t.as_str()) {
                            Some("Results") => {
                                let part = v.pointer("/channel/alternatives/0/transcript").and_then(|t| t.as_str()).unwrap_or("").trim();
                                text = join_words(&text, part);
                            }
                            // Deepgram's last frame after `CloseStream`, so there is no need to wait for the close.
                            Some("Metadata") if !recording => {
                                flushed = true;
                                break;
                            }
                            _ => {}
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(e)) => anyhow::bail!("Deepgram socket error: {e}"),
                },
                chunk = audio.recv(), if recording => match chunk {
                    Some(samples) => write.send(Message::Binary(i16_slice_to_le_bytes(&samples))).await?,
                    None => {
                        recording = false;
                        write.send(Message::Text(serde_json::json!({ "type": "CloseStream" }).to_string())).await?;
                    }
                },
            }
        }
        anyhow::ensure!(!recording, "Deepgram closed the socket during the clip");
        anyhow::ensure!(
            flushed,
            "Deepgram closed the socket before its final result"
        );
        Ok(text)
    }
}

#[derive(Debug)]
enum ConnectError {
    /// Authentication failed (401/403). Do NOT retry — user must fix key.
    Auth,
    /// Transient: network blip, 5xx, TLS hiccup. Retry with backoff.
    Transient(String),
}

#[async_trait]
impl Stt for DeepgramStt {
    async fn start(
        &self,
        mut audio_rx: AudioRx,
        txn: TranscriptTx,
        words: &[String],
    ) -> anyhow::Result<()> {
        let url = self.connect_url(words);
        let mic = spawn_supervised_session(self, &url, Channel::Mic, txn.clone()).await?;
        let sys = spawn_supervised_session(self, &url, Channel::System, txn.clone()).await?;

        // `try_send` so a stalled WS doesn't block the other channel.
        let mut mic_drops: usize = 0;
        let mut sys_drops: usize = 0;
        loop {
            let chunk = match audio_rx.recv().await {
                Ok(c) => c,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(n, "deepgram pump: audio receiver lagged");
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    tracing::info!("deepgram pump: audio stream closed");
                    break;
                }
            };
            // Taken before `chunk` moves, so a synthetic overload event lands at the right point on the timeline.
            let ts_ms = chunk.ts_ms;

            let (dest, drops, channel) = match chunk.channel {
                Channel::Mic => (&mic, &mut mic_drops, Channel::Mic),
                Channel::System => (&sys, &mut sys_drops, Channel::System),
            };

            match dest.try_send(chunk) {
                Ok(()) => {
                    *drops = 0;
                }
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    *drops += 1;
                    tracing::warn!(
                        ?channel,
                        consecutive = *drops,
                        "deepgram pump: backpressure — dropping chunk"
                    );
                    if *drops >= MAX_CONSECUTIVE_DROPS {
                        tracing::error!(
                            ?channel,
                            consecutive = *drops,
                            "deepgram pump: sustained backpressure — channel unrecoverable"
                        );
                        emit_status(
                            &txn,
                            channel,
                            ts_ms,
                            "[stt overloaded, transcript may be lossy]",
                        );
                        // Reset to avoid spamming; the supervisor's reconnect path signals recovery.
                        *drops = 0;
                    }
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    tracing::warn!(
                        ?channel,
                        "deepgram pump: session task gone (channel closed)"
                    );
                    // Keep going: the other channel may still be healthy.
                }
            }
        }

        drop(mic);
        drop(sys);
        Ok(())
    }
}

/// Emit a synthetic status event, final so the dock renders it as a locked line.
///
/// `ts_ms` is the latest real audio timestamp the caller has seen, so the line sorts near where the audio was. Pass `0` before any audio has flowed.
fn emit_status(txn: &TranscriptTx, channel: Channel, ts_ms: u64, text: &str) {
    let _ = txn.send(TranscriptEvent {
        ts_ms,
        channel,
        text: text.to_string(),
        is_final: true,
        confidence: None,
    });
}

/// Open a self-healing Deepgram session for one channel and return the sender the audio pump feeds.
///
/// The initial connect error is returned to the caller. A supervisor task handles reconnects and status events afterwards.
async fn spawn_supervised_session(
    cfg: &DeepgramStt,
    url: &str,
    channel: Channel,
    txn: TranscriptTx,
) -> anyhow::Result<tokio::sync::mpsc::Sender<AudioChunk>> {
    // Bounded so backpressure surfaces at the pump's `try_send` instead of growing memory.
    let (audio_tx, mut audio_rx) = tokio::sync::mpsc::channel::<AudioChunk>(64);

    let initial = connect_once(cfg, url.to_string(), channel)
        .await
        .map_err(|e| match e {
            ConnectError::Auth => anyhow::anyhow!(
                "deepgram auth failed (channel={channel:?}): \
             check YOGURT_DEEPGRAM_API_KEY"
            ),
            ConnectError::Transient(msg) => {
                anyhow::anyhow!("deepgram connect failed (channel={channel:?}): {msg}")
            }
        })?;

    let api_key = cfg.api_key.clone();
    let base_url = cfg.base_url.clone();
    let model = cfg.model.clone();
    let url = url.to_string();

    tokio::spawn(async move {
        let mut current_ws = Some(initial);
        let mut attempt: usize = 0;
        // Audio timestamp of the last forwarded chunk, kept across reconnects so status events land near the audio.
        let mut last_ts_ms: u64 = 0;

        loop {
            let ws = match current_ws.take() {
                Some(w) => w,
                None => {
                    if attempt >= MAX_RECONNECT_ATTEMPTS {
                        tracing::error!(
                            ?channel,
                            attempts = attempt,
                            "deepgram supervisor: exhausted reconnects — giving up"
                        );
                        emit_status(
                            &txn,
                            channel,
                            last_ts_ms,
                            "[stt disconnected, reconnect failed]",
                        );
                        return;
                    }
                    let backoff_ms = 1000u64 << attempt;
                    tracing::info!(
                        ?channel,
                        attempt = attempt + 1,
                        backoff_ms,
                        "deepgram supervisor: reconnecting"
                    );
                    emit_status(&txn, channel, last_ts_ms, "[stt reconnecting]");
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;

                    let cfg_local = DeepgramStt {
                        api_key: api_key.clone(),
                        base_url: base_url.clone(),
                        model: model.clone(),
                    };
                    match connect_once(&cfg_local, url.clone(), channel).await {
                        Ok(w) => {
                            attempt = 0;
                            emit_status(&txn, channel, last_ts_ms, "[stt reconnected]");
                            w
                        }
                        Err(ConnectError::Auth) => {
                            tracing::error!(
                                ?channel,
                                "deepgram supervisor: auth failed on reconnect — terminal"
                            );
                            emit_status(&txn, channel, last_ts_ms, "[stt auth failed]");
                            return;
                        }
                        Err(ConnectError::Transient(msg)) => {
                            tracing::warn!(
                                ?channel,
                                ?msg,
                                "deepgram supervisor: reconnect attempt failed"
                            );
                            attempt += 1;
                            continue;
                        }
                    }
                }
            };

            let (mut write, mut read) = ws.split();

            let mut clean_shutdown = false;
            let mut needs_reconnect = false;
            let mut utterance = UtteranceState::default();
            loop {
                tokio::select! {
                    // Prefer the reader so a server-initiated close is handled before audio is pushed into the void.
                    biased;
                    msg = read.next() => {
                        match msg {
                            Some(Ok(Message::Text(t))) => {
                                if let Some(ev) = fold_deepgram_event(&mut utterance, &t, channel) {
                                    let _ = txn.send(ev);
                                }
                            }
                            Some(Ok(Message::Close(_))) => {
                                tracing::info!(?channel, "deepgram reader: ws closed by server");
                                needs_reconnect = true;
                                break;
                            }
                            Some(Ok(_)) => {
                            }
                            Some(Err(e)) => {
                                tracing::warn!(?channel, ?e, "deepgram reader: ws error");
                                needs_reconnect = true;
                                break;
                            }
                            None => {
                                tracing::info!(?channel, "deepgram reader: ws stream ended");
                                needs_reconnect = true;
                                break;
                            }
                        }
                    }
                    audio = audio_rx.recv() => {
                        match audio {
                            Some(chunk) => {
                                last_ts_ms = chunk.ts_ms;
                                let bytes = i16_slice_to_le_bytes(&chunk.samples);
                                if write.send(Message::Binary(bytes)).await.is_err() {
                                    tracing::warn!(
                                        ?channel,
                                        "deepgram writer: ws send failed — will reconnect"
                                    );
                                    needs_reconnect = true;
                                    break;
                                }
                            }
                            None => {
                                tracing::info!(
                                    ?channel,
                                    "deepgram writer: audio channel closed — clean shutdown"
                                );
                                let close = serde_json::json!({ "type": "CloseStream" }).to_string();
                                let _ = write.send(Message::Text(close)).await;
                                let _ = write.close().await;
                                clean_shutdown = true;
                                break;
                            }
                        }
                    }
                }
            }

            if clean_shutdown {
                return;
            }

            if needs_reconnect {
                emit_status(&txn, channel, last_ts_ms, "[stt disconnected, retrying]");
            }
        }
    });

    Ok(audio_tx)
}

/// Escapes all but RFC 3986's unreserved characters, enough for a query value.
fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Open one Deepgram WS connection, classifying failures as retryable or terminal.
async fn connect_once(
    cfg: &DeepgramStt,
    url: String,
    channel: Channel,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    ConnectError,
> {
    tracing::info!(?channel, %url, "deepgram: connecting");

    let mut req = url
        .into_client_request()
        .map_err(|e| ConnectError::Transient(format!("bad request: {e}")))?;
    let auth_header = format!("Token {}", cfg.api_key)
        .parse()
        .map_err(|e| ConnectError::Transient(format!("bad auth header: {e}")))?;
    req.headers_mut().insert("Authorization", auth_header);

    match tokio_tungstenite::connect_async(req).await {
        Ok((ws, _resp)) => Ok(ws),
        Err(tokio_tungstenite::tungstenite::Error::Http(resp))
            if resp.status() == 401 || resp.status() == 403 =>
        {
            tracing::error!(?channel, status=%resp.status(), "deepgram: auth rejected");
            Err(ConnectError::Auth)
        }
        Err(e) => {
            tracing::warn!(?channel, ?e, "deepgram: transient connect error");
            Err(ConnectError::Transient(format!("{e}")))
        }
    }
}

fn i16_slice_to_le_bytes(samples: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Per-connection utterance accumulator.
///
/// Deepgram delivers one utterance as several result windows: interim frames, then window-finals (`is_final: true, speech_final: false`)
/// that carry only their own window's words, and finally `speech_final: true`.
/// Treating frames statelessly loses every earlier window, so the persisted final held only the utterance's tail.
#[derive(Default)]
pub struct UtteranceState {
    /// Text of window-finalized chunks of the in-progress utterance.
    buf: String,
    /// Timestamp of the utterance's first audio, carried onto every event.
    start_ms: Option<u64>,
    /// Text of the last final emitted on this connection.
    /// Deepgram can resend the terminal `speech_final` frame on reconnect, and by then `buf` and `start_ms` have been reset,
    /// so the duplicate would re-emit as a new utterance. Comparing against this drops the repeat.
    last_final: Option<String>,
}

fn join_words(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_string(),
        (_, true) => a.to_string(),
        _ => format!("{a} {b}"),
    }
}

/// Fold one Deepgram JSON frame into `st`, returning the event to emit:
/// - interim -> partial with buffered plus live window text
/// - window-final -> partial with the full buffered text
/// - speech_final -> final with the complete joined utterance, then state resets
pub fn fold_deepgram_event(
    st: &mut UtteranceState,
    text: &str,
    channel: Channel,
) -> Option<TranscriptEvent> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    if v.get("type")?.as_str()? != "Results" {
        return None;
    }
    let alt = v.get("channel")?.get("alternatives")?.get(0)?;
    let transcript = alt.get("transcript")?.as_str()?.trim().to_string();
    let confidence = alt
        .get("confidence")
        .and_then(|x| x.as_f64())
        .map(|c| c as f32);
    let window_final = v.get("is_final").and_then(|x| x.as_bool()).unwrap_or(false);
    let speech_final = v
        .get("speech_final")
        .and_then(|x| x.as_bool())
        .unwrap_or(window_final);
    let ts_now = (v.get("start").and_then(|x| x.as_f64()).unwrap_or(0.0) * 1000.0) as u64;

    if transcript.is_empty() && !speech_final {
        return None;
    }
    if st.start_ms.is_none() && !transcript.is_empty() {
        st.start_ms = Some(ts_now);
    }
    let ts_ms = st.start_ms.unwrap_or(ts_now);

    if speech_final {
        let full = join_words(&st.buf, &transcript);
        st.buf.clear();
        st.start_ms = None;
        if full.is_empty() {
            return None;
        }
        if st.last_final.as_deref() == Some(full.as_str()) {
            return None;
        }
        st.last_final = Some(full.clone());
        Some(TranscriptEvent {
            ts_ms,
            channel,
            text: full,
            is_final: true,
            confidence,
        })
    } else if window_final {
        st.buf = join_words(&st.buf, &transcript);
        Some(TranscriptEvent {
            ts_ms,
            channel,
            text: st.buf.clone(),
            is_final: false,
            confidence,
        })
    } else {
        Some(TranscriptEvent {
            ts_ms,
            channel,
            text: join_words(&st.buf, &transcript),
            is_final: false,
            confidence,
        })
    }
}

/// Parse one Deepgram JSON frame into a [`TranscriptEvent`], or `None` for non-`Results` frames and empty transcripts.
///
/// `speech_final` marks end-of-utterance and is what locks a line.
/// `is_final` fires per ~6 s window mid-sentence, so reading it alone made every window render as a new locked line.
/// It is used only when `speech_final` is absent.
pub fn parse_deepgram_event(text: &str, channel: Channel) -> Option<TranscriptEvent> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    if v.get("type")?.as_str()? != "Results" {
        return None;
    }
    let alt = v.get("channel")?.get("alternatives")?.get(0)?;
    let transcript = alt.get("transcript")?.as_str()?.trim();
    if transcript.is_empty() {
        return None;
    }
    let confidence = alt
        .get("confidence")
        .and_then(|x| x.as_f64())
        .map(|c| c as f32);
    let start_s = v.get("start").and_then(|x| x.as_f64()).unwrap_or(0.0);
    let is_final = match v.get("speech_final").and_then(|x| x.as_bool()) {
        Some(b) => b,
        None => v.get("is_final").and_then(|x| x.as_bool()).unwrap_or(false),
    };
    Some(TranscriptEvent {
        ts_ms: (start_s * 1000.0) as u64,
        channel,
        text: transcript.to_string(),
        is_final,
        confidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyterms_are_escaped_and_stop_at_the_budget() {
        let dg = |model: &str| DeepgramStt {
            api_key: String::new(),
            base_url: String::new(),
            model: model.into(),
        };
        let words = ["Zorbal".to_string(), "Niamh Ó Brien".into()];
        assert_eq!(
            dg("nova-3").keyterms(&words),
            "&keyterm=Zorbal&keyterm=Niamh%20%C3%93%20Brien"
        );
        assert_eq!(dg("nova-2").keyterms(&words), "");
        let long: Vec<String> = (0..11).map(|i| format!("{i:0>49}")).collect();
        assert_eq!(
            dg("nova-3").keyterms(&long).matches("&keyterm=").count(),
            10
        );
    }

    #[test]
    fn it_parses_a_speech_final_results_frame() {
        let frame = r#"{
          "type": "Results",
          "channel": {"alternatives": [{"transcript": "hello world", "confidence": 0.99}]},
          "is_final": true,
          "speech_final": true,
          "start": 1.42,
          "duration": 0.7
        }"#;
        let ev = parse_deepgram_event(frame, Channel::Mic).expect("should parse");
        assert_eq!(ev.ts_ms, 1420);
        assert_eq!(ev.text, "hello world");
        assert!(ev.is_final);
        assert_eq!(ev.channel, Channel::Mic);
        assert_eq!(ev.confidence, Some(0.99));
    }

    #[test]
    fn it_treats_interim_final_as_partial() {
        let frame = r#"{
          "type": "Results",
          "channel": {"alternatives": [{"transcript": "hello world how are", "confidence": 0.94}]},
          "is_final": true,
          "speech_final": false,
          "start": 0.0,
          "duration": 2.1
        }"#;
        let ev = parse_deepgram_event(frame, Channel::Mic).expect("should parse");
        assert_eq!(ev.text, "hello world how are");
        assert!(
            !ev.is_final,
            "interim final (is_final=true, speech_final=false) must NOT lock the line"
        );
    }

    #[test]
    fn it_falls_back_to_is_final_when_speech_final_absent() {
        let frame = r#"{
          "type": "Results",
          "channel": {"alternatives": [{"transcript": "fallback", "confidence": 0.9}]},
          "is_final": true,
          "start": 0.0
        }"#;
        let ev = parse_deepgram_event(frame, Channel::Mic).expect("should parse");
        assert!(ev.is_final);
    }

    #[test]
    fn it_ignores_empty_transcripts() {
        let frame = r#"{
          "type": "Results",
          "channel": {"alternatives": [{"transcript": "", "confidence": 0.0}]},
          "is_final": false,
          "start": 0.0
        }"#;
        assert!(parse_deepgram_event(frame, Channel::Mic).is_none());
    }

    #[test]
    fn it_ignores_metadata_frames() {
        let frame = r#"{"type": "Metadata", "request_id": "abc"}"#;
        assert!(parse_deepgram_event(frame, Channel::System).is_none());
    }

    #[test]
    fn it_converts_i16_to_le_bytes_correctly() {
        let bytes = i16_slice_to_le_bytes(&[0, 1, -1, 256]);
        assert_eq!(bytes, vec![0x00, 0x00, 0x01, 0x00, 0xff, 0xff, 0x00, 0x01]);
    }

    #[test]
    fn emit_status_carries_the_caller_supplied_timestamp() {
        let (tx, mut rx) = tokio::sync::broadcast::channel(4);
        emit_status(&tx, Channel::Mic, 42_000, "[stt reconnecting]");
        let ev = rx.try_recv().expect("event was sent");
        assert_eq!(ev.ts_ms, 42_000);
        assert_eq!(ev.channel, Channel::Mic);
        assert!(ev.is_final, "status events must be final/locked lines");
        assert_eq!(ev.text, "[stt reconnecting]");
    }
}

#[cfg(test)]
mod utterance_tests {
    use super::*;

    fn frame(text: &str, start: f64, window_final: bool, speech_final: bool) -> String {
        format!(
            r#"{{"type":"Results","channel":{{"alternatives":[{{"transcript":"{text}"}}]}},"is_final":{window_final},"speech_final":{speech_final},"start":{start}}}"#
        )
    }

    #[test]
    fn multi_window_utterance_joins_all_text() {
        let mut st = UtteranceState::default();
        let p1 = fold_deepgram_event(
            &mut st,
            &frame("we wanna isolate", 4.0, false, false),
            Channel::Mic,
        )
        .unwrap();
        assert!(!p1.is_final);
        let w1 = fold_deepgram_event(
            &mut st,
            &frame("we wanna isolate them longer", 4.0, true, false),
            Channel::Mic,
        )
        .unwrap();
        assert!(!w1.is_final, "window-final is not utterance-final");
        assert_eq!(w1.text, "we wanna isolate them longer");
        let p2 = fold_deepgram_event(
            &mut st,
            &frame("from the changes", 9.5, false, false),
            Channel::Mic,
        )
        .unwrap();
        assert_eq!(p2.text, "we wanna isolate them longer from the changes");
        assert_eq!(p2.ts_ms, 4000, "anchored at utterance start");
        let f = fold_deepgram_event(
            &mut st,
            &frame("from the changes we introduce", 9.5, true, true),
            Channel::Mic,
        )
        .unwrap();
        assert!(f.is_final);
        assert_eq!(
            f.text,
            "we wanna isolate them longer from the changes we introduce"
        );
        assert_eq!(f.ts_ms, 4000);
        let n = fold_deepgram_event(
            &mut st,
            &frame("next thought", 12.0, false, false),
            Channel::Mic,
        )
        .unwrap();
        assert_eq!(n.text, "next thought");
        assert_eq!(n.ts_ms, 12000);
    }

    #[test]
    fn empty_speech_final_flushes_buffer() {
        let mut st = UtteranceState::default();
        fold_deepgram_event(
            &mut st,
            &frame("hello there", 1.0, true, false),
            Channel::System,
        )
        .unwrap();
        let f = fold_deepgram_event(&mut st, &frame("", 2.0, true, true), Channel::System).unwrap();
        assert!(f.is_final);
        assert_eq!(f.text, "hello there");
        assert_eq!(f.ts_ms, 1000);
    }

    #[test]
    fn duplicate_speech_final_frame_is_suppressed() {
        let mut st = UtteranceState::default();
        let f1 = fold_deepgram_event(
            &mut st,
            &frame("hello there", 4.0, true, true),
            Channel::Mic,
        )
        .unwrap();
        assert!(f1.is_final);
        assert_eq!(f1.text, "hello there");
        assert_eq!(f1.ts_ms, 4000);

        let f2 = fold_deepgram_event(
            &mut st,
            &frame("hello there", 4.0, true, true),
            Channel::Mic,
        );
        assert!(
            f2.is_none(),
            "resent speech_final frame must not emit a second final"
        );
    }

    #[test]
    fn duplicate_speech_final_with_zero_start_after_reset_is_suppressed() {
        let mut st = UtteranceState::default();
        fold_deepgram_event(
            &mut st,
            &frame("hello there", 4.0, true, true),
            Channel::Mic,
        )
        .unwrap();

        let f2 = fold_deepgram_event(
            &mut st,
            &frame("hello there", 0.0, true, true),
            Channel::Mic,
        );
        assert!(
            f2.is_none(),
            "late duplicate final with no start info must not re-emit at ts_ms 0"
        );
    }

    #[test]
    fn distinct_final_after_a_final_still_emits() {
        let mut st = UtteranceState::default();
        fold_deepgram_event(
            &mut st,
            &frame("hello there", 4.0, true, true),
            Channel::Mic,
        )
        .unwrap();

        let f2 = fold_deepgram_event(
            &mut st,
            &frame("a different sentence", 6.0, true, true),
            Channel::Mic,
        )
        .unwrap();
        assert!(f2.is_final);
        assert_eq!(f2.text, "a different sentence");
        assert_eq!(f2.ts_ms, 6000);
    }

    #[test]
    fn single_window_utterance_is_unchanged() {
        let mut st = UtteranceState::default();
        let f = fold_deepgram_event(
            &mut st,
            &frame("quick reply", 3.0, true, true),
            Channel::Mic,
        )
        .unwrap();
        assert!(f.is_final);
        assert_eq!(f.text, "quick reply");
        assert!(
            fold_deepgram_event(&mut st, &frame("", 4.0, false, false), Channel::Mic).is_none()
        );
    }
}
