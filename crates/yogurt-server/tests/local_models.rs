//! whisper.cpp links its own ggml statically and llama.cpp loads another from its dylibs, two copies in one process.
//! A static llama.cpp beside whisper once aborted on its first call, so this runs both, whisper on either side of
//! the LLM, and gates every bump of either crate.
//! Parakeet brings a third native runtime, sherpa-onnx with onnxruntime, so it gets the same check against whisper.
//!
//! The models come from `YOGURT_TEST_WHISPER`, `YOGURT_TEST_GGUF`, `YOGURT_TEST_PARAKEET`, and `YOGURT_TEST_PARAKEET_ULTRA` (each Parakeet's encoder, with the other files beside it),
//! or else the downloaded small.en, the built-in cleanup model, Parakeet v3, and Parakeet Ultra:
//!
//! ```sh
//! cargo test -p yogurt-server --test local_models -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::time::Instant;

use futures_util::StreamExt;

use yogurt_llm::{ChatMessage, ChatRequest, LlmClient, LocalLlamaClient};
use yogurt_stt::models;
use yogurt_stt::{LocalModel, WhisperLocal};

fn model(var: &str, spec: &models::ModelSpec) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .or_else(|| models::resolve_model(spec))
        .unwrap_or_else(|| panic!("set {var}, or download {} first", spec.name))
}

/// "Um, can you send me the deck before the meeting on Wednesday?", spoken by macOS `say`.
fn dictation() -> Vec<i16> {
    wav("dictation.wav")
}

/// "Hey, can you send the Quillane deck over to Marivex before the Zorbal sync tomorrow morning, please?", spoken by macOS `say`.
/// The names are made up, from `scripts/eval/made-up-names.txt`.
fn names() -> Vec<i16> {
    wav("names.wav")
}

fn wav(name: &str) -> Vec<i16> {
    let wav = std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let data = wav.windows(4).position(|w| w == b"data").unwrap() + 8;
    wav[data..]
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a whisper model and a GGUF on disk"]
async fn whisper_and_llama_run_in_one_process() {
    let whisper = WhisperLocal::load(model(
        "YOGURT_TEST_WHISPER",
        models::lookup("small.en").unwrap(),
    ))
    .unwrap();
    let pcm = dictation();
    let before = whisper.transcribe(&pcm, &[]).unwrap();
    eprintln!("whisper: {before}");
    let words = before.to_lowercase();
    assert!(
        words.contains("deck") && words.contains("wednesday"),
        "{before}"
    );

    let llm = LocalLlamaClient::new(model("YOGURT_TEST_GGUF", &models::CLEANUP_MODEL), "test");
    let system = yogurt_prompts::Prompts::load(yogurt_prompts::Mode::Release)
        .unwrap()
        .voice_cleanup()
        .unwrap()
        .system;
    let ask = |raw: &str| ChatRequest {
        messages: vec![
            ChatMessage::system(system.clone()),
            ChatMessage::user(format!("<dictation>{raw}</dictation>")),
        ],
        stream: true,
    };
    let mut answers = Vec::new();
    // The first call loads the model and fills the KV cache. The next ones reuse the system prompt's KV state.
    // The compared calls both decode the dictation alone after the same cached prefix: llama.cpp rounds differently
    // for a different batch size, so a cold call can pick another token at a near tie.
    let other = "um so the the launch moves to uh Friday";
    for raw in [other, before.as_str(), other, before.as_str()] {
        let started = Instant::now();
        // At most 16 chunks: CI's stand-in model thinks before it answers, and a slow runner took over 39 min
        // to finish that. Dropping the stream stops the worker, as a cleanup past its time limit does.
        let mut chunks = llm.stream(ask(raw)).await.unwrap().take(16);
        let mut answer = String::new();
        while let Some(chunk) = chunks.next().await {
            answer.push_str(&chunk.unwrap().delta);
        }
        eprintln!("llama: {} ms: {answer}", started.elapsed().as_millis());
        assert!(!answer.trim().is_empty());
        answers.push(answer);
    }
    assert_eq!(
        answers[1], answers[3],
        "the same dictation must clean up the same way after another one changed the cached KV state"
    );

    // Waits for the worker to free the model, so the process never exits mid-free.
    drop(llm);
    let after = whisper.transcribe(&pcm, &[]).unwrap();
    assert_eq!(
        before, after,
        "whisper must transcribe the same after llama.cpp ran"
    );
}

#[test]
#[ignore = "needs a whisper model and Parakeet on disk"]
fn whisper_and_parakeet_run_in_one_process() {
    let whisper = WhisperLocal::load(model(
        "YOGURT_TEST_WHISPER",
        models::lookup("small.en").unwrap(),
    ))
    .unwrap();
    let pcm = dictation();
    let before = whisper.transcribe(&pcm, &[]).unwrap();

    let spec = models::lookup("parakeet-v3").unwrap();
    let parakeet = LocalModel::load(spec, model("YOGURT_TEST_PARAKEET", spec)).unwrap();
    let started = Instant::now();
    let text = parakeet.transcribe(&pcm, &[]).unwrap();
    eprintln!("parakeet: {} ms: {text}", started.elapsed().as_millis());
    let words = text.to_lowercase();
    assert!(
        words.contains("deck") && words.contains("wednesday"),
        "{text}"
    );

    assert_eq!(
        before,
        whisper.transcribe(&pcm, &[]).unwrap(),
        "whisper must transcribe the same after Parakeet ran"
    );
}

/// The dictionary as the engines get it, the clip's names among made-up ones it never says.
fn dictionary() -> Vec<String> {
    [
        "Brexnor", "Quillane", "Telian", "Marivex", "Tavimek", "Zorbal",
    ]
    .map(String::from)
    .to_vec()
}

#[test]
#[ignore = "needs a whisper model on disk"]
fn whisper_spells_a_dictation_the_dictionarys_way() {
    let spec = models::lookup("small.en").unwrap();
    let whisper = LocalModel::load(spec, model("YOGURT_TEST_WHISPER", spec)).unwrap();
    let pcm = names();
    let before = whisper.transcribe(&pcm, &[]).unwrap();
    let after = whisper.transcribe(&pcm, &dictionary()).unwrap();
    eprintln!("whisper without the dictionary: {before}\nwhisper with it: {after}");
    assert!(!before.contains("Marivex"), "{before}");
    assert!(
        after.contains("Marivex") && after.contains("Zorbal"),
        "{after}"
    );
    assert!(
        !after.contains("Telian") && !after.contains("Brexnor"),
        "{after}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a whisper model on disk"]
async fn whisper_spells_a_meeting_the_dictionarys_way() {
    use yogurt_stt::{AudioChunk, Channel, Stt};
    let spec = models::lookup("small.en").unwrap();
    let whisper = LocalModel::load(spec, model("YOGURT_TEST_WHISPER", spec)).unwrap();
    let (audio_tx, audio_rx) = tokio::sync::broadcast::channel(16);
    let (txn, mut events) = tokio::sync::broadcast::channel(64);
    let session = tokio::spawn(async move { whisper.start(audio_rx, txn, &dictionary()).await });
    // A second of silence after the clip ends the utterance, as a pause in a meeting would.
    let mut pcm = names();
    pcm.extend([0i16; 16_000]);
    audio_tx
        .send(AudioChunk {
            channel: Channel::Mic,
            samples: pcm,
            ts_ms: 0,
        })
        .unwrap();
    drop(audio_tx);
    session.await.unwrap().unwrap();
    // The final decoders outlive the session and close the channel once they finish.
    let mut finals = String::new();
    while let Ok(ev) = events.recv().await {
        if ev.is_final {
            finals.push_str(&ev.text);
        }
    }
    eprintln!("whisper in a meeting: {finals}");
    assert!(
        finals.contains("Marivex") && finals.contains("Zorbal"),
        "{finals}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Parakeet v3 on disk"]
async fn parakeet_v3_transcribes_a_meeting() {
    parakeet_transcribes_a_meeting("parakeet-v3", "YOGURT_TEST_PARAKEET").await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Parakeet Ultra on disk"]
async fn parakeet_ultra_transcribes_a_meeting() {
    parakeet_transcribes_a_meeting("parakeet-ultra", "YOGURT_TEST_PARAKEET_ULTRA").await;
}

/// The dictation through the meeting pipeline's VAD and final decode, as system audio.
async fn parakeet_transcribes_a_meeting(name: &str, var: &str) {
    use yogurt_stt::{AudioChunk, Channel, Stt};
    let spec = models::lookup(name).unwrap();
    let parakeet = LocalModel::load(spec, model(var, spec)).unwrap();
    let (audio_tx, audio_rx) = tokio::sync::broadcast::channel(16);
    let (txn, mut events) = tokio::sync::broadcast::channel(64);
    let session = tokio::spawn(async move { parakeet.start(audio_rx, txn, &[]).await });
    let mut pcm = dictation();
    pcm.extend([0i16; 16_000]);
    audio_tx
        .send(AudioChunk {
            channel: Channel::System,
            samples: pcm,
            ts_ms: 0,
        })
        .unwrap();
    drop(audio_tx);
    session.await.unwrap().unwrap();
    let mut finals = String::new();
    while let Ok(ev) = events.recv().await {
        if ev.is_final {
            finals.push_str(&ev.text);
        }
    }
    eprintln!("{name} in a meeting: {finals}");
    let words = finals.to_lowercase();
    assert!(
        words.contains("deck") && words.contains("wednesday"),
        "{name}: {finals}"
    );
}
