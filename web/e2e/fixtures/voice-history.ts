import type { Dictation, VoiceStats } from "../../src/lib/api/voiceTypes";

const NOW = Date.now();
const row = (id: string, cleanup_model: string | null, raw_text: string, text: string, cleanup_ms: number): Dictation => ({
  id, created_at: NOW - Number(id) * 60_000, app_bundle_id: "com.apple.Notes", app_name: "Notes",
  hands_free: false, audio_ms: cleanup_model ? 5_000 : 1_500,
  raw_text, text, raw_words: raw_text.split(/\s+/).length, words: text.split(/\s+/).length,
  stt_engine: "local:parakeet-v3", stt_ms: 200, cleanup: cleanup_model ? "done" : "rules",
  cleanup_model, cleanup_ms, input_tokens: null, output_tokens: null, cost_usd: null,
});

export const DICTATIONS = [
  row("1", "celeris-1", "um send the release checklist tomorrow", "Send the release checklist tomorrow.", 250),
  row("2", "gemini-3.5-flash-lite", "lets review the design after lunch", "Let's review the design after lunch.", 400),
  row("3", "qwen3-4b-instruct", "uh please share the notes with sam", "Please share the notes with Sam.", 900),
  row("4", null, "um send deck", "send deck", 0),
];
const sum = (field: "words" | "audio_ms" | "stt_ms" | "cleanup_ms") =>
  DICTATIONS.reduce((n, d) => n + (d[field] ?? 0), 0);
const removed = DICTATIONS.reduce((n, d) => n + d.raw_words - d.words, 0);
const app = { name: "Notes", words: sum("words"), count: DICTATIONS.length };
const localDay = new Date(NOW).toLocaleDateString("en-CA");

export const STATS: VoiceStats = {
  count: DICTATIONS.length, hands_free_count: 0, words: sum("words"), audio_ms: sum("audio_ms"),
  wait_ms: sum("stt_ms") + sum("cleanup_ms"), first_day: localDay,
  // The 30-day chart the server sends, with every dictation on its last day.
  chart: { unit: "day", buckets: Array.from({ length: 30 }, (_, i) => {
    const d = new Date(NOW);
    const key = new Date(d.getFullYear(), d.getMonth(), d.getDate() - 29 + i).toLocaleDateString("en-CA");
    return i === 29 ? { key, words: sum("words"), count: DICTATIONS.length } : { key, words: 0, count: 0 };
  }) }, apps: [app],
  cleaned_count: DICTATIONS.length, cleaned_words: removed, fillers_removed: removed,
  too_slow_count: 0, median_wait_ms: 525, cleanup_model: "celeris-1", costs: [],
  snippets: { count: 0, words: 0, audio_ms: 0, wait_ms: 0 },
  history: { count: DICTATIONS.length, hands_free_count: 0, apps: [app],
    stt_avg_ms: { "parakeet-v3": 200 },
    cleanup_avg_ms: { "celeris-1": 250, "gemini-3.5-flash-lite": 400, "qwen3-4b-instruct": 900 },
  },
};
