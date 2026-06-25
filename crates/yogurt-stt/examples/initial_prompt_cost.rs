//! Measures what a dictionary in whisper's initial prompt adds to a Voice dictation's decode.
//!
//! ```sh
//! cargo run --release -p yogurt-stt --features local-stt --example initial_prompt_cost -- \
//!     <ggml model> <names file> <runs> <entry counts, e.g. 0,50> <16 kHz mono wav>...
//! ```

use std::time::Instant;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [model, names, runs, counts, wavs @ ..] = args.as_slice() else {
        panic!("usage: initial_prompt_cost <model> <names file> <runs> <counts> <wav>...");
    };
    let runs: usize = runs.parse().expect("runs is a number");
    let names: Vec<String> = std::fs::read_to_string(names)
        .expect("read names")
        .lines()
        .map(str::to_string)
        .collect();
    whisper_rs::install_logging_hooks();
    let ctx = WhisperContext::new_with_params(model, WhisperContextParameters::default())
        .expect("load model");

    for wav in wavs {
        let pcm = read_wav_mono_16k(wav);
        let file = wav.rsplit('/').next().unwrap_or(wav);
        for count in counts.split(',') {
            let n: usize = count.parse().expect("count is a number");
            let prompt = names[..n].join(", ");
            let mut ms = Vec::new();
            let mut text = String::new();
            // The first decode is untimed, like the app's warm model.
            for i in 0..=runs {
                let t = Instant::now();
                text = decode(&ctx, &pcm, &prompt);
                if i > 0 {
                    ms.push(t.elapsed().as_secs_f64() * 1000.0);
                }
            }
            ms.sort_by(f64::total_cmp);
            let median = if runs % 2 == 1 {
                ms[runs / 2]
            } else {
                (ms[runs / 2 - 1] + ms[runs / 2]) / 2.0
            };
            println!(
                "{file}\t{n} entries\t{median:.0}\t{:.0}\t{:.0}\t{text}",
                ms[0],
                ms[runs - 1]
            );
        }
    }
}

/// Must mirror `WhisperLocal::decode(.., fast = false)`, the path a Voice dictation takes, plus the prompt.
fn decode(ctx: &WhisperContext, pcm: &[f32], prompt: &str) -> String {
    let mut state = ctx.create_state().expect("create state");
    let mut params = FullParams::new(SamplingStrategy::BeamSearch {
        beam_size: 5,
        patience: 1.0,
    });
    params.set_language(Some("en"));
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_special(false);
    params.set_print_timestamps(false);
    params.set_no_context(false);
    if !prompt.is_empty() {
        params.set_initial_prompt(prompt);
    }
    state.full(params, pcm).expect("decode");
    (0..state.full_n_segments())
        .filter_map(|i| state.get_segment(i))
        .filter_map(|seg| seg.to_str_lossy().ok().map(|s| s.to_string()))
        .collect::<String>()
        .trim()
        .to_string()
}

/// Seeks the `data` chunk and reads the rest as little-endian i16, avoiding a `hound` dependency.
fn read_wav_mono_16k(path: &str) -> Vec<f32> {
    let bytes = std::fs::read(path).expect("read wav");
    let data = bytes
        .windows(4)
        .position(|w| w == b"data")
        .expect("no `data` chunk, is this a WAV?")
        + 8;
    bytes[data..]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32_768.0)
        .collect()
}
