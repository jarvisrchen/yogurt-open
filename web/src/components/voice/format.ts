import type { ModelCost, StatsRange, VoiceStats } from "../../lib/api/voiceTypes";
import { formatCount } from "../library/pagination";

export const TYPING_WPM = 40;

export function clipLength(ms: number): string {
  const s = Math.max(1, Math.round(ms / 1000));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  return s % 60 ? `${m} min ${s % 60} s` : `${m} min`;
}

/** A `YYYY-MM-DD` day from the stats, at local midnight. */
export function parseDay(day: string): Date {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y, m - 1, d);
}

/** "Nov 8", with the year when it is not this year's. */
export function monthDay(d: Date): string {
  const sameYear = d.getFullYear() === new Date().getFullYear();
  return d.toLocaleDateString("en-US", { month: "short", day: "numeric", ...(sameYear ? {} : { year: "numeric" }) });
}

export function shortDay(d: Date): string {
  return d.toLocaleDateString("en-US", { weekday: "short", month: "short", day: "numeric" });
}

/** `firstDay` is the range's first day with anything in it. */
export function sinceLabel(firstDay: string | null, range: StatsRange): string {
  if (firstDay == null) return "none yet";
  return range === "all" ? `since ${monthDay(parseDay(firstDay))}` : RANGE_WORDS[range];
}

export const RANGE_WORDS: Record<Exclude<StatsRange, "all">, string> = { today: "today", "7d": "in the last 7 days", "30d": "in the last 30 days" };

type Spoken = Pick<VoiceStats, "words" | "audio_ms" | "wait_ms" | "snippets">;

// A snippet pastes many words for a short cue, so counting it would read as hundreds of words a minute.
const spoken = (s: Spoken) => ({
  words: s.words - s.snippets.words,
  audio_ms: s.audio_ms - s.snippets.audio_ms,
  wait_ms: s.wait_ms - s.snippets.wait_ms,
});

export function pace(s: Spoken): number {
  const { words, audio_ms } = spoken(s);
  return audio_ms > 0 ? Math.round(words / (audio_ms / 60_000)) : 0;
}

export function savedMs(s: Spoken): number {
  const { words, audio_ms, wait_ms } = spoken(s);
  return Math.max(0, (words / TYPING_WPM) * 60_000 - audio_ms - wait_ms);
}

/** Cents from a cent up, and one significant digit below it, where a single dictation's cost sits. */
export function usd(x: number): string {
  if (x === 0) return "$0";
  if (x >= 0.005) return `$${x.toFixed(2)}`;
  const r = Number(x.toPrecision(1));
  return `$${r.toFixed(-Math.floor(Math.log10(r)))}`;
}

/** An estimate. Under ten cents it keeps a third decimal, since one meeting's calls cost a fraction of a cent. */
export function approxUsd(x: number): string {
  if (x === 0) return "$0";
  if (x < 0.0005) return "< $0.001";
  return x < 0.0995 ? `≈ $${x.toFixed(3)}` : `≈ ${usd(x)}`;
}

/** A cleanup cost: `usd` is null with nothing priced, and `parts` splits it by model when there is more than one, or names what the price table could not price. */
export interface Cost {
  usd: number | null;
  unpriced: number;
  parts: string[];
}

export function costRow(costs: ModelCost[]): Cost {
  const priced = costs.filter((c) => c.count > c.unpriced_count);
  const total = priced.reduce((n, c) => n + c.usd, 0);
  const unpriced = costs.reduce((n, c) => n + c.unpriced_count, 0);
  const parts =
    costs.length > 1 || unpriced > 0
      ? costs.map((c) =>
          c.count > c.unpriced_count
            ? // One significant digit under a tenth of a cent, where each model's share of a few dictations sits, so the rows differ.
              `${c.model} ${c.usd > 0 && c.usd < 0.001 ? `≈ ${usd(c.usd)}` : approxUsd(c.usd)}`
            : `${c.model} ${formatCount(c.input_tokens + c.output_tokens)} tokens`,
        )
      : [];
  if (unpriced > 0) parts.push(`${formatCount(unpriced)} ${unpriced === 1 ? "dictation" : "dictations"} not priced`);
  return { usd: priced.length === 0 ? null : total, unpriced, parts };
}

/** `unit` picks minutes or hours, so a count toward it keeps one unit the whole way. */
export function savedLabel(ms: number, unit = ms): string {
  const min = ms / 60_000;
  if (unit / 60_000 < 60) return `${Math.round(min)} min`;
  return `${(min / 60).toFixed(1)} h`;
}

export function gridStep(max: number): number {
  const raw = Math.max(max, 30) / 3;
  const pow = 10 ** Math.floor(Math.log10(raw));
  return [1, 2, 2.5, 4, 5, 10].map((f) => f * pow).find((v) => v >= raw)!;
}

export function appShares(apps: VoiceStats["apps"]): { name: string; words: number; pct: number }[] {
  const total = apps.reduce((n, a) => n + a.words, 0);
  if (total === 0) return [];
  const sorted = [...apps].sort((a, b) => b.words - a.words);
  const top = sorted.slice(0, 5).map((a) => ({ name: a.name, words: a.words }));
  const rest = sorted.slice(5).reduce((n, a) => n + a.words, 0);
  if (rest > 0) top.push({ name: "Other", words: rest });
  return top.map((a) => ({ ...a, pct: Math.round((a.words / total) * 100) }));
}
