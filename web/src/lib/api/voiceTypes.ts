// No imports, so the Cloud webapp, which installs neither react-query nor react-router, can share these with the Mac.
/** `snippet` is a cue said as the whole dictation, which pastes its snippet without cleanup. */
export type Cleanup = "off" | "done" | "rules" | "too_slow" | "snippet";

export interface Dictation {
  id: string;
  /** Unix ms when the shortcut went down. */
  created_at: number;
  app_bundle_id: string | null;
  app_name: string | null;
  hands_free: boolean;
  audio_ms: number;
  raw_text: string;
  /** What was pasted. */
  text: string;
  raw_words: number;
  words: number;
  stt_engine: string;
  stt_ms: number;
  cleanup: Cleanup;
  cleanup_model: string | null;
  cleanup_ms: number | null;
  /** What the cleanup model used, as reported or estimated. Null when no model ran, and on dictations from before costs were saved. */
  input_tokens: number | null;
  output_tokens: number | null;
  /** At the model's list price when it ran. Null with tokens set when the price table lacked the model. */
  cost_usd: number | null;
}

/** The cleanup cost of one model over the range. */
export interface ModelCost {
  model: string;
  count: number;
  input_tokens: number;
  output_tokens: number;
  /** Over the priced dictations only. */
  usd: number;
  /** Dictations the price table did not know the model for when they ran. */
  unpriced_count: number;
}

export interface VoiceStats {
  count: number;
  hands_free_count: number;
  words: number;
  audio_ms: number;
  /** Transcription plus cleanup, summed over every dictation. */
  wait_ms: number;
  /** The first local day with a dictation, `YYYY-MM-DD`. */
  first_day: string | null;
  /**
   * Oldest first: hours `00` to `23` for today, `YYYY-MM-DD` days for 7 and 30 days, and for all time `YYYY-MM-DD` Mondays, `YYYY-MM` months past a year, or days for its first two weeks.
   * Today is one `YYYY-MM-DD` day instead of hours when no text was kept, and always in All devices, whose counts come per day.
   */
  chart: { unit: "hour" | "day" | "week" | "month"; buckets: { key: string; words: number; count: number }[] };
  /** Most-used first. A dictation with no app counts under "Unknown". */
  apps: { name: string; words: number; count: number }[];
  cleaned_count: number;
  cleaned_words: number;
  fillers_removed: number;
  too_slow_count: number;
  median_wait_ms: number | null;
  cleanup_model: string | null;
  /** Most expensive first. Dictations from before costs were saved are in none of them. */
  costs: ModelCost[];
  /** The snippet pastes within the totals, which the speaking pace and time saved leave out. */
  snippets: { count: number; words: number; audio_ms: number; wait_ms: number };
  history: HistoryCounts;
}

/** Counts of the kept text, for the sidebar rows that filter the history. */
export interface HistoryCounts {
  count: number;
  hands_free_count: number;
  apps: { name: string; words: number; count: number }[];
  /** Average transcription ms per local model, and for every Deepgram model under `deepgram`, over the kept text. */
  stt_avg_ms: Record<string, number>;
  /** Average cleanup ms per `cleanup_model`, over the kept dictations the model cleaned up. */
  cleanup_avg_ms: Record<string, number>;
}

/** Everything in [`VoiceStats`] but `history` covers this range. */
export type StatsRange = "today" | "7d" | "30d" | "all";

/** `added` was typed on the Dictionary page, `learned` was picked from a dictation. */
export type WordSource = "added" | "learned";

export interface DictionaryEntry {
  id: string;
  word: string;
  sounds_like: string | null;
  source: WordSource;
  /** Unix ms. */
  created_at: number;
  updated_at: number;
}

export interface Snippet {
  id: string;
  cue: string;
  /** Plain text, with line breaks. */
  text: string;
  /** Unix ms. */
  created_at: number;
  updated_at: number;
}

export type StyleKind = "messages" | "work" | "email" | "other";

export type VoiceStyleChoice = "casual" | "normal" | "formal";

export interface VoiceSetupWord {
  word: string;
  sounds_like: string | null;
  source: "added" | "learned";
}

/** What copying the Cloud copy into this device changes. The keep counts are entries only on this device. */
export interface VoiceSetupPlan {
  add_words: number;
  add_snippets: number;
  /** The Cloud's version of each word this device spells or hears differently. */
  replace_words: VoiceSetupWord[];
  /** The Cloud's cue for each snippet whose cue or text differs here. */
  replace_snippets: string[];
  keep_words: number;
  keep_snippets: number;
}

/** One cleanup of a Try Voice clip, which is shown and never pasted or saved. */
export interface VoiceTried {
  text: string;
  cleanup: Cleanup;
  cleanup_model: string | null;
  cleanup_ms: number | null;
  /** Absent unless the model cleaned it, since only the model applies a style. */
  style: VoiceStyleChoice | null;
  /** The dictionary words that appear in `text`. */
  words: string[];
  fillers: string[];
}

/** A clip that ended in the Try Voice field. */
export interface VoiceTry extends VoiceTried {
  raw: string;
  /** As a dictation's `stt_engine`, such as `local:parakeet-v3`. */
  stt_engine: string;
  stt_ms: number;
}
