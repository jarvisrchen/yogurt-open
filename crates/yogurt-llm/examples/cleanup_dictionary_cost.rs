//! Measures what dictionary words in Voice cleanup's prompt add to the time to the first token and to the whole answer.
//!
//! ```sh
//! cargo run --release -p yogurt-llm --features local --example cleanup_dictionary_cost -- \
//!     <model.gguf | openai> <names file> <runs> <line counts, e.g. 0,10,100> [system | user]
//! ```
//!
//! `openai` reads `CLEANUP_BASE_URL`, `CLEANUP_MODEL`, and `CLEANUP_KEY` from the environment.
//! `system`, the default, puts the lines after the instructions, and `user` puts them in the dictation's own turn, after the examples.
//! Each run gets a different dictation and a different slice of names, so the built-in model's KV cache keeps only what a real next dictation would share.

use std::sync::Arc;
use std::time::Instant;

use futures_util::StreamExt;
use yogurt_llm::{ChatMessage, ChatRequest, LlmClient, LocalLlamaClient, OpenAiCompatClient};
use yogurt_prompts::{Mode, Prompts, VoiceCleanup};

/// About 45 words each, the length `voice_cleanup::total` budgets 2.6 s for.
const DICTATIONS: [&str; 5] = [
    "Um so I talked to Quillane this morning and uh she thinks the launch can move to Thursday, but only if Marivex signs off on the pricing page first, so can you, like, ping him today and let me know what he says before five?",
    "Hey just a heads up, the Zorbal sync is moved to, no wait, it's still Tuesday at ten, but Tavimek can't make it, so I was thinking we could, you know, record it and send him the notes after, does that work for you?",
    "Okay so for the offsite we need, um, twelve rooms for two nights, and Brexnor said the hotel near the river is cheaper, uh, by about forty dollars a night, so let's book that one unless someone has a strong reason not to.",
    "Can you remind Telian that the report is due Friday and, um, that the numbers from last quarter need to be in there too, I mean the revised numbers, not the ones from the first draft, because those were wrong in a couple of places?",
    "So I think the main thing, uh, the main thing is that Quillane and Zorbal both need access to the staging server before Monday, and I'd rather we, like, sort that out today than have them blocked first thing next week.",
];

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [target, names, runs, counts, rest @ ..] = args.as_slice() else {
        panic!("usage: cleanup_dictionary_cost <model.gguf | openai> <names file> <runs> <counts> [system | user]");
    };
    let in_user_turn = rest.first().is_some_and(|p| p == "user");
    let runs: usize = runs.parse().expect("runs is a number");
    let names: Vec<String> = std::fs::read_to_string(names)
        .expect("read names")
        .lines()
        .map(str::to_string)
        .collect();
    let prompt = Prompts::load(Mode::Release)
        .and_then(|p| p.voice_cleanup())
        .expect("load the cleanup prompt");
    let env = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set"));
    let client: Arc<dyn LlmClient> = if target == "openai" {
        Arc::new(OpenAiCompatClient::new(
            env("CLEANUP_BASE_URL"),
            env("CLEANUP_KEY"),
            env("CLEANUP_MODEL"),
        ))
    } else {
        Arc::new(LocalLlamaClient::new(target.into(), "built-in"))
    };
    client.warm().await;

    let mut call = 0;
    for count in counts.split(',') {
        let n: usize = count.parse().expect("count is a number");
        let (mut first, mut total) = (Vec::new(), Vec::new());
        let mut text = String::new();
        // The first call is untimed, so every case starts from a loaded model and a cache holding the previous call.
        for i in 0..=runs {
            let lines: Vec<&str> = (0..n)
                .map(|j| names[(call * 7 + j) % names.len()].as_str())
                .collect();
            let raw = DICTATIONS[call % DICTATIONS.len()];
            call += 1;
            let start = Instant::now();
            let mut chunks = client
                .stream(ChatRequest {
                    messages: messages(&prompt, &lines, raw, in_user_turn),
                    stream: true,
                })
                .await
                .expect("open the stream");
            let mut first_ms = None;
            text.clear();
            while let Some(chunk) = chunks.next().await {
                let chunk = chunk.expect("stream chunk");
                if first_ms.is_none() && !chunk.delta.is_empty() {
                    first_ms = Some(ms(start));
                }
                text.push_str(&chunk.delta);
                if chunk.done {
                    break;
                }
            }
            if i > 0 {
                first.push(first_ms.expect("the model answered with no text"));
                total.push(ms(start));
            }
        }
        let place = if in_user_turn { "user" } else { "system" };
        println!(
            "{}\t{n} lines, {place}\t{:.0}\t{:.0}\t{:.0}\t{:.0}\t{}",
            client.model_name(),
            median(&mut first),
            median(&mut total),
            total.iter().copied().fold(f64::MAX, f64::min),
            total.iter().copied().fold(0.0, f64::max),
            text.trim().replace('\n', " ")
        );
    }
}

/// Must mirror `voice_cleanup::messages`, which ends the instructions with the dictionary line.
/// `in_user_turn` moves the line into the dictation's turn instead.
fn messages(
    prompt: &VoiceCleanup,
    lines: &[&str],
    raw: &str,
    in_user_turn: bool,
) -> Vec<ChatMessage> {
    let dictionary = match &prompt.dictionary {
        Some(line) if !lines.is_empty() => line.replace("{words}", &lines.join(", ")),
        _ => String::new(),
    };
    let system = if in_user_turn || dictionary.is_empty() {
        prompt.system.clone()
    } else {
        format!("{}\n\n{dictionary}", prompt.system)
    };
    let tag = |d: &str| ChatMessage::user(format!("<dictation>{d}</dictation>"));
    let mut out = vec![ChatMessage::system(system)];
    for (dictation, cleaned) in &prompt.examples {
        out.push(tag(dictation));
        out.push(ChatMessage::assistant(cleaned));
    }
    out.push(if in_user_turn && !dictionary.is_empty() {
        ChatMessage::user(format!("{dictionary}\n\n<dictation>{raw}</dictation>"))
    } else {
        tag(raw)
    });
    out
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}
