# Model evals

How to feed two recordings the exact same audio, run them through different STT engines or LLMs, and get a graded comparison of the AI summaries.

The pieces live in `scripts/eval/`:

| File | Role |
|---|---|
| `conversation.txt` | A scripted two-person meeting (~5 min spoken). It is both the audio source and the ground truth the judge grades against. |
| `play.sh` | Speaks the script through the speaker with two macOS `say` voices, honoring `PAUSE` lines. |
| `compare.sh` | Pulls two meetings from SQLite, bundles them with the script, and asks headless `claude -p` for a scorecard. |

## Run one trial

1. Pick the STT engine and LLM under test in Settings (or start the backend with `YOGURT_LLM_*` env vars).
2. Put on headphones. System audio is captured through ScreenCaptureKit whatever the output device, so the "them" channel stays clean, but an open mic next to speakers would double-capture the audio as "me".
3. Start a new meeting in the UI.
4. In a terminal: `just eval-play`. It prints each line as it is spoken and ends with `done - stop the meeting now`.
5. Stop the meeting and let enhance finish.
6. Copy the meeting URL from the address bar.

Repeat with a different engine or model.
Each meeting row records what produced it: `stt_engine` is stamped at start, `llm_model` is stamped by enhance, so runs cannot be mixed up later.

## Skipping the recording (fixtures)

`just eval-play` is no longer the only way to get `conversation.txt` into a meeting.
`yogurt ctl meeting new --from-script scripts/eval/conversation.txt` converts the same `A:`/`B:` lines into a finished meeting directly (no speaker, no microphone, no TCC grants), with synthetic timestamps standing in for the real ones `play.sh` would have produced.
This is enough to compare LLMs (the transcript is identical either way, so `stt_engine` is irrelevant), but not STT engines, since no audio was ever transcribed - use `just eval-play` when the STT engine itself is what's under test.
Run `yogurt ctl meeting enhance <id>` against the fixture to produce `enriched_md` under whichever LLM is configured, then `just eval-compare` on two such fixtures exactly as with two recordings.

## Compare two trials

```bash
just eval-compare http://localhost:7878/meeting/<id-a>/post http://localhost:7878/meeting/<id-b>/post > report.md
```

Bare ids work too.
The judge grades transcript accuracy (numbers, names, dates), summary faithfulness (hallucinations), coverage (decision, disagreement, self-correction, every action item with owner and date), and usefulness, then names the better run and the most damaging error in each.

Knobs:

- `PROMPT_ONLY=1` prints the assembled prompt instead of calling Claude, useful for pasting into another model.
- `JUDGE_MODEL=opus` is passed through to `claude --model`.
- `SCRIPT=path/to/other.txt` swaps the ground truth, for both `play.sh` (first argument) and `compare.sh`.
- `A_VOICE`, `B_VOICE`, `RATE`, `GAP` change the voices, words per minute, and the silence between turns in `play.sh`.

## Better voices

The voices that ship with macOS (Samantha, Daniel) are the compact variants and sound robotic.
Apple's Premium and Enhanced variants are free downloads that `say` can use: System Settings > Accessibility > Spoken Content > System Voice > Manage Voices..., then tick Ava (Premium), Zoe (Premium), Samantha (Enhanced), Daniel (Enhanced), Tom (Enhanced).
`play.sh` picks them up automatically once installed, in that order of preference, or set `A_VOICE="Zoe (Premium)"` explicitly.
Siri voices are not exposed to `say`.
`say -v '?'` lists what is installed.

## Writing another script

Keep the format: `A:` and `B:` lines are spoken, `PAUSE <seconds>` is silence, `#` is a comment.
Spell numbers and dates out the way a person would say them, because that is what the STT hears and what the judge checks against.
Include things worth grading: at least one decision, one disagreement, one correction, and action items with owners and dates.

## Voice dictionary speed

`scripts/eval/voice-speed.sh` measures what a Voice dictionary adds to a dictation's latency, and the phase 3 design's speed budget holds its numbers.
It speaks two clips with `say`, about 5 s and 30 s, that name people from `scripts/eval/made-up-names.txt`, and prints one tab-separated row per case with the median of `RUNS` warm runs (11 by default).

- `parakeet`: Parakeet greedy against modified beam search with 0, 20, and 100 hotwords on the Mac, through `scripts/eval/parakeet_hotwords.c` on sherpa-onnx's C API, linked against the dylibs the app's own build checks.
- `phone`: the same cases on an adb device, through `scripts/eval/ParakeetHotwords.java` on the sherpa-onnx AAR the Android app ships, run with `app_process`.
  It needs a JDK and the Android SDK's build tools for `d8`, and is left out of a run with no sections named, since the yogurt emulator is shared and needs its lock taken first.
- `whisper`: whisper with a 0- and 50-entry initial prompt, on base.en and on small.en and large-v3-turbo when installed (`WHISPER_MODELS` picks), through `crates/yogurt-stt/examples/initial_prompt_cost.rs`.
- `cleanup`: time to the first token and in all with 0, 10, and 100 dictionary words, on the built-in Qwen3 4B, with the `dictionary:` line after the instructions as the app sends it and in the dictation's turn, through `crates/yogurt-llm/examples/cleanup_dictionary_cost.rs`.
  `CLEANUP_PROVIDER=<provider id>` adds that provider in both placements, read from the data directory's database with its key from `keys.json`.

```bash
just eval-voice-speed
CLEANUP_PROVIDER=<provider id> RUNS=21 just eval-voice-speed cleanup
```

Downloads and builds land in `~/.cache/yogurt-voice-speed`, and every download is checked against a pinned SHA-256.
The phone section copies about 700 MB to `/data/local/tmp/voice-speed` on the device.

## Voice cleanup prompt

`just test-voice-cleanup` runs the voice cleanup prompt's regression cases in `crates/yogurt-server/src/voice_cleanup.rs`, so a prompt edit that breaks one shows before it ships.
On its own it runs them against the built-in Qwen3 4B, whose greedy answers repeat, and every case must pass, as `just test-hw` asks.
`CLEANUP_PROVIDER=<provider id>` runs them against that provider from Settings instead, with its key read from `keys.json`, and adds the cases the 4B model misses.
A provider's answers vary, so `RUNS` repeats each case, and the summary counts the cases right on every run.

Real dictations make the best cases but are private, so `CLEANUP_CASES` adds a JSON Lines file kept outside the repo.
Each line is one dictation with phrases its cleanup must keep and must drop, since a long dictation has many right cleanups:

```json
{"raw": "um ask zorbec to check our just land", "keep": ["Zorbex Lab", "our just land"], "drop": ["um"], "dictionary": [["Zorbex Lab", "zorb ex"]]}
```

```bash
CLEANUP_PROVIDER=<provider id> RUNS=5 CLEANUP_CASES=~/.cache/yogurt-voice-eval/cleanup-cases.jsonl just test-voice-cleanup
```

Compare an edit with the prompt before it, on the built-in model and every provider in use, since a wording that helps one model can cost another a case.
