//! Transcribes a list of WAV files with one local model through the app's own load and decode code, for WER and speed evals.
//!
//! ```sh
//! cargo run --release -p yogurt-stt --features local-stt --example stt_eval -- \
//!     parakeet-ultra <model dir> manifest.tsv [--vad] > hyp.tsv
//! ```
//!
//! `manifest.tsv` holds `id<TAB>path` rows of 16 kHz mono 16-bit WAVs.
//! `--vad` cuts each file with the meeting pipeline's `Segmenter` and decodes each utterance as a final, joined by spaces.
//! Without it each file decodes whole, the way a Voice clip does.
//! Prints `id<TAB>audio_s<TAB>decode_s<TAB>text` per file, and the load time and real-time factor to stderr.
//! Run it under `/usr/bin/time -l` for peak memory.

use std::time::Instant;
use yogurt_stt::vad::{Segmenter, SegmenterEvent, SAMPLE_RATE_HZ};
use yogurt_stt::LocalModel;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [name, dir, manifest, rest @ ..] = args.as_slice() else {
        anyhow::bail!("usage: stt_eval <model> <model dir> <manifest.tsv> [--vad]");
    };
    let vad = rest.iter().any(|a| a == "--vad");
    let spec = yogurt_stt::models::find_any(name)
        .ok_or_else(|| anyhow::anyhow!("unknown model {name}"))?;

    let t = Instant::now();
    let model = LocalModel::load(spec, std::path::Path::new(dir).join(spec.files[0].filename))?;
    let load_s = t.elapsed().as_secs_f64();

    let (mut audio_total, mut decode_total) = (0.0, 0.0);
    for line in std::fs::read_to_string(manifest)?.lines() {
        let Some((id, path)) = line.split_once('\t') else {
            continue;
        };
        let pcm = read_wav(path)?;
        let audio_s = pcm.len() as f64 / SAMPLE_RATE_HZ as f64;
        let t = Instant::now();
        let text = if vad {
            let mut seg = Segmenter::new(SAMPLE_RATE_HZ);
            let mut parts = Vec::new();
            let mut on_event = |e| {
                if let SegmenterEvent::Segment { pcm, .. } = e {
                    parts.push(pcm);
                }
            };
            seg.push(&pcm, &mut on_event);
            // A second of silence flushes the last utterance, which a live meeting flushes with the pause after it.
            seg.push(&[0; SAMPLE_RATE_HZ], &mut on_event);
            parts
                .iter()
                .map(|p| model.transcribe(p, &[]))
                .collect::<anyhow::Result<Vec<_>>>()?
                .join(" ")
        } else {
            model.transcribe(&pcm, &[])?
        };
        let decode_s = t.elapsed().as_secs_f64();
        audio_total += audio_s;
        decode_total += decode_s;
        println!(
            "{id}\t{audio_s:.3}\t{decode_s:.3}\t{}",
            text.replace(['\t', '\n'], " ")
        );
    }
    eprintln!(
        "model={name} load_s={load_s:.2} audio_s={audio_total:.1} decode_s={decode_total:.1} rtf={:.4}",
        decode_total / audio_total.max(f64::EPSILON)
    );
    Ok(())
}

/// Walks the RIFF chunks for `fmt ` and `data`, checks the format is 16 kHz mono 16-bit PCM, and returns the data chunk's samples.
/// A float WAV read as i16 decodes to empty text without an error, which would score as every word missed.
fn read_wav(path: &str) -> anyhow::Result<Vec<i16>> {
    let bytes = std::fs::read(path)?;
    anyhow::ensure!(
        bytes.get(..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WAVE"),
        "{path} is not a WAV"
    );
    let (mut fmt, mut data) = (None, None);
    let mut at = 12;
    while let Some(head) = bytes.get(at..at + 8) {
        let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        let body = bytes
            .get(at + 8..at + 8 + len)
            .ok_or_else(|| anyhow::anyhow!("{path}: chunk runs past the end"))?;
        match &head[..4] {
            b"fmt " => fmt = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        // Chunks are word-aligned, so an odd length has a pad byte.
        at += 8 + len + (len & 1);
    }
    let fmt = fmt.ok_or_else(|| anyhow::anyhow!("no fmt chunk in {path}"))?;
    let data = data.ok_or_else(|| anyhow::anyhow!("no data chunk in {path}"))?;
    anyhow::ensure!(fmt.len() >= 16, "{path}: short fmt chunk");
    let u16_at = |i: usize| u16::from_le_bytes([fmt[i], fmt[i + 1]]);
    let rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
    let (format, channels, bits) = (u16_at(0), u16_at(2), u16_at(14));
    anyhow::ensure!(
        format == 1 && channels == 1 && rate == 16_000 && bits == 16,
        "{path} is not 16 kHz mono 16-bit PCM (format {format}, {channels} channels, {rate} Hz, {bits} bits)"
    );
    Ok(data
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect())
}
