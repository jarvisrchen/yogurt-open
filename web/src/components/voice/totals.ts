import type { StatsRange, VoiceStats } from "../../lib/api/voiceTypes";
import { parseDay } from "./format";

// No react-query or react-router, so the Cloud webapp's phone Home shares it with the Mac's Voice page.

/** One local day's counts on one device, what Share daily totals sends. Counts only, never text or app names. */
export interface DayCounts {
  /** `YYYY-MM-DD`. */
  day: string;
  count: number;
  words: number;
  audio_ms: number;
  wait_ms: number;
  hands_free_count: number;
  cleaned_count: number;
  cleaned_words: number;
  fillers_removed: number;
  too_slow_count: number;
  snippet_count: number;
  snippet_words: number;
  snippet_audio_ms: number;
  snippet_wait_ms: number;
  /** What the device priced its own cleanups at. */
  usd: number;
  /** Cleanups whose model the device had no price for. */
  unpriced_count: number;
}

/** All zeros: the start of a sum, and what a day Delete all emptied sends. */
export const zeroDay = (day: string): DayCounts => ({
  day,
  count: 0,
  words: 0,
  audio_ms: 0,
  wait_ms: 0,
  hands_free_count: 0,
  cleaned_count: 0,
  cleaned_words: 0,
  fillers_removed: 0,
  too_slow_count: 0,
  snippet_count: 0,
  snippet_words: 0,
  snippet_audio_ms: 0,
  snippet_wait_ms: 0,
  usd: 0,
  unpriced_count: 0,
});

/** A sharing device's day, as `yogurt_voice_days` returns it. */
export interface VoiceDayRow extends DayCounts {
  device_id: string;
  device_name: string;
  device_kind: "mac" | "phone";
  /** Unix ms when the device last changed this day. */
  updated_at: number;
}

export interface DeviceTotal {
  id: string;
  name: string;
  words: number;
  count: number;
  usd: number;
  /** Whether any cleanup ran, so a $0 reads as a cost rather than as cleanup being off. */
  cleaned: boolean;
  /** Unix ms of the device's newest change. */
  updated_at: number;
}

export interface AllDevices {
  stats: VoiceStats;
  /** The devices that dictated in the range, most words first. */
  devices: DeviceTotal[];
  usd: number;
  unpriced_count: number;
}

export const dayKey = (d: Date) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;

const RANGE_DAYS: Record<Exclude<StatsRange, "all">, number> = { today: 1, "7d": 7, "30d": 30 };

/** The range's first local day, `YYYY-MM-DD`, or null for all time. */
export function rangeStart(range: StatsRange, now = new Date()): string | null {
  if (range === "all") return null;
  return dayKey(new Date(now.getFullYear(), now.getMonth(), now.getDate() - RANGE_DAYS[range] + 1));
}

type Chart = VoiceStats["chart"];
type Unit = Chart["unit"];

const monday = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate() - ((d.getDay() + 6) % 7));

/** The bucket a local time falls in. */
export function chartKey(unit: Unit, d: Date): string {
  if (unit === "hour") return String(d.getHours()).padStart(2, "0");
  if (unit === "day") return dayKey(d);
  if (unit === "week") return dayKey(monday(d));
  return dayKey(d).slice(0, 7);
}

/**
 * The range's empty buckets, oldest first, as the Mac's `/api/voice/stats` lays them out.
 * `first` is the first day with a dictation, where all time starts. `hourly` false charts today as its one day, for counts kept per day.
 */
export function emptyChart(range: StatsRange, now: number, first: string | null, hourly = true): Chart {
  const t = new Date(now);
  const today = new Date(t.getFullYear(), t.getMonth(), t.getDate());
  if (range === "today" && hourly)
    return { unit: "hour", buckets: Array.from({ length: 24 }, (_, h) => ({ key: String(h).padStart(2, "0"), words: 0, count: 0 })) };
  const start = rangeStart(range, t);
  const from = parseDay(start ?? first ?? dayKey(today));
  const span = Math.round((today.getTime() - from.getTime()) / 86_400_000);
  // Two weeks of history would be one or two weekly bars.
  const unit: Unit = start || span < 14 ? "day" : span <= 366 ? "week" : "month";
  const keys: string[] = [];
  for (let d = unit === "week" ? monday(from) : from; d <= today; d = new Date(d.getFullYear(), d.getMonth(), d.getDate() + (unit === "week" ? 7 : 1))) {
    const k = chartKey(unit, d);
    if (keys.at(-1) !== k) keys.push(k);
  }
  return { unit, buckets: keys.map((key) => ({ key, words: 0, count: 0 })) };
}

/** Adds each dictation's words to its bucket. `at` is a local time in the bucket, and anything outside the chart is skipped. */
export function fillChart<T>(chart: Chart, items: T[], at: (x: T) => Date, words: (x: T) => number, count: (x: T) => number): Chart {
  const by = new Map(chart.buckets.map((b) => [b.key, b]));
  for (const x of items) {
    const b = by.get(chartKey(chart.unit, at(x)));
    if (b) {
      b.words += words(x);
      b.count += count(x);
    }
  }
  return chart;
}

/**
 * Sums every sharing device's day rows into the stats the Voice page draws.
 * The rows carry no app names, no text, and no single waits, so `apps` and `history` are empty and there is no median.
 */
export function allDevicesStats(rows: VoiceDayRow[], range: StatsRange, now = Date.now()): AllDevices {
  const start = rangeStart(range, new Date(now));
  // A zero row is a day the device's Delete all emptied.
  const inRange = rows.filter((r) => r.count > 0 && (start == null || r.day >= start));
  const first_day = inRange.reduce<string | null>((min, r) => (min == null || r.day < min ? r.day : min), null);
  // The rows hold whole days, so today is one bucket, which the chart leaves out.
  const chart = fillChart(emptyChart(range, now, first_day, false), inRange, (r) => parseDay(r.day), (r) => r.words, (r) => r.count);
  const sum = (f: (r: VoiceDayRow) => number) => inRange.reduce((n, r) => n + f(r), 0);

  const devices = new Map<string, DeviceTotal>();
  for (const r of rows) {
    const d = devices.get(r.device_id) ?? { id: r.device_id, name: r.device_name, words: 0, count: 0, usd: 0, cleaned: false, updated_at: 0 };
    d.updated_at = Math.max(d.updated_at, r.updated_at);
    devices.set(r.device_id, d);
  }
  for (const r of inRange) {
    const d = devices.get(r.device_id)!;
    d.words += r.words;
    d.count += r.count;
    d.usd += r.usd;
    d.cleaned ||= r.cleaned_count + r.too_slow_count > 0;
  }

  return {
    stats: {
      count: sum((r) => r.count),
      hands_free_count: sum((r) => r.hands_free_count),
      words: sum((r) => r.words),
      audio_ms: sum((r) => r.audio_ms),
      wait_ms: sum((r) => r.wait_ms),
      first_day,
      chart,
      apps: [],
      cleaned_count: sum((r) => r.cleaned_count),
      cleaned_words: sum((r) => r.cleaned_words),
      fillers_removed: sum((r) => r.fillers_removed),
      too_slow_count: sum((r) => r.too_slow_count),
      median_wait_ms: null,
      cleanup_model: null,
      costs: [],
      snippets: {
        count: sum((r) => r.snippet_count),
        words: sum((r) => r.snippet_words),
        audio_ms: sum((r) => r.snippet_audio_ms),
        wait_ms: sum((r) => r.snippet_wait_ms),
      },
      history: { count: 0, hands_free_count: 0, apps: [], stt_avg_ms: {}, cleanup_avg_ms: {} },
    },
    devices: [...devices.values()].filter((d) => d.count > 0).sort((a, b) => b.words - a.words || a.name.localeCompare(b.name)),
    usd: sum((r) => r.usd),
    unpriced_count: sum((r) => r.unpriced_count),
  };
}
