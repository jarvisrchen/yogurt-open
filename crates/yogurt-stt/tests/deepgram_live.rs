//! Release-to-text of a real Deepgram dictation, for the Voice design doc's Transcription table, and the dictionary's keyterms against real Deepgram.
//!
//! `YOGURT_DEEPGRAM_API_KEY=... YOGURT_VOICE_CLIPS=a.wav,b.wav cargo test -p yogurt-stt --features local-stt --test deepgram_live -- --ignored --nocapture`
//! The keyterm tests need only the key: `YOGURT_DEEPGRAM_API_KEY=... cargo test -p yogurt-stt --features local-stt --test deepgram_live -- --ignored --nocapture keyterms`
//! Each clip is 16 kHz mono linear16 WAV, streamed in 20 ms frames at the pace a mic records them, the way Voice streams while the hotkey is held.

use std::time::{Duration, Instant};
use yogurt_stt::deepgram::DeepgramStt;

const FRAME: usize = 320;
const RUNS: usize = 5;

#[tokio::test]
#[ignore = "needs a Deepgram key and the network"]
async fn release_to_text_by_clip_length() {
    let key = std::env::var("YOGURT_DEEPGRAM_API_KEY").expect("YOGURT_DEEPGRAM_API_KEY");
    let clips = std::env::var("YOGURT_VOICE_CLIPS").expect("YOGURT_VOICE_CLIPS");
    let dg = std::sync::Arc::new(DeepgramStt::new(key));
    for path in clips.split(',') {
        let wav = std::fs::read(path).unwrap();
        // The 44-byte header `say --data-format=LEI16@16000` writes.
        let pcm: Vec<i16> = wav[44..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        let mut times = Vec::new();
        let mut text = String::new();
        // The first run warms DNS and TLS, as an earlier dictation would.
        for _ in 0..=RUNS {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let feed = pcm.clone();
            let recorder = tokio::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_millis(20));
                for frame in feed.chunks(FRAME) {
                    tick.tick().await;
                    tx.send(frame.to_vec()).unwrap();
                }
                Instant::now()
            });
            let dg = dg.clone();
            let dictation = tokio::spawn(async move { dg.dictate(rx, &[]).await });
            let released = recorder.await.unwrap();
            text = dictation.await.unwrap().unwrap();
            times.push(released.elapsed().as_millis());
        }
        times.remove(0);
        times.sort();
        println!(
            "{path}: {:.1} s clip, median {} ms of {times:?}: {text}",
            pcm.len() as f64 / 16_000.0,
            times[RUNS / 2]
        );
        assert!(!text.is_empty());
    }
}

/// The made-up names clip `yogurt-server`'s model tests use, with a 2 s pause after it so Deepgram ends the utterance.
fn names() -> Vec<i16> {
    let wav = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../yogurt-server/tests/fixtures/names.wav"
    ))
    .unwrap();
    let data = wav.windows(4).position(|w| w == b"data").unwrap() + 8;
    let mut pcm: Vec<i16> = wav[data..]
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    pcm.extend([0i16; 32_000]);
    pcm
}

/// Every made-up name, the clip's three first, which is more than the keyterm budget holds, so Deepgram gets as many as yogurt ever sends.
fn dictionary() -> Vec<String> {
    include_str!("../../../scripts/eval/made-up-names.txt")
        .lines()
        .map(String::from)
        .collect()
}

#[tokio::test]
#[ignore = "needs a Deepgram key and the network"]
async fn keyterms_spell_a_dictation_the_dictionarys_way() {
    let key = std::env::var("YOGURT_DEEPGRAM_API_KEY").expect("YOGURT_DEEPGRAM_API_KEY");
    let dg = DeepgramStt::new(key);
    let mut heard = Vec::new();
    for words in [vec![], dictionary()] {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for frame in names().chunks(FRAME) {
            tx.send(frame.to_vec()).unwrap();
        }
        drop(tx);
        heard.push(dg.dictate(rx, &words).await.unwrap());
    }
    eprintln!(
        "Deepgram without the dictionary: {}\nDeepgram with it: {}",
        heard[0], heard[1]
    );
    for name in ["Quillane", "Marivex", "Zorbal"] {
        assert!(heard[1].contains(name), "{}", heard[1]);
    }
}

#[tokio::test]
#[ignore = "needs a Deepgram key and the network"]
async fn keyterms_spell_a_meeting_the_dictionarys_way() {
    use yogurt_stt::{AudioChunk, Channel, Stt};
    let key = std::env::var("YOGURT_DEEPGRAM_API_KEY").expect("YOGURT_DEEPGRAM_API_KEY");
    let dg = std::sync::Arc::new(DeepgramStt::new(key));
    let (audio_tx, audio_rx) = tokio::sync::broadcast::channel(1024);
    let (txn, mut events) = tokio::sync::broadcast::channel(64);
    let session = tokio::spawn(async move { dg.start(audio_rx, txn, &dictionary()).await });
    // At the mic's pace, since the session drops frames a slow socket has not taken, and held open after, since closing the audio drops what Deepgram has yet to send.
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    for (i, frame) in names().chunks(FRAME).enumerate() {
        tick.tick().await;
        audio_tx
            .send(AudioChunk {
                channel: Channel::Mic,
                samples: frame.to_vec(),
                ts_ms: i as u64 * 20,
            })
            .unwrap();
    }
    let line = loop {
        let ev = tokio::time::timeout(Duration::from_secs(20), events.recv())
            .await
            .expect("a final line within 20 s")
            .unwrap();
        if ev.is_final && !ev.text.starts_with("[stt") {
            break ev.text;
        }
    };
    drop(audio_tx);
    session.await.unwrap().unwrap();
    eprintln!("Deepgram in a meeting: {line}");
    for name in ["Quillane", "Marivex", "Zorbal"] {
        assert!(line.contains(name), "{line}");
    }
}
