import {
  keepPreviousData,
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { bearerFetch } from "../session";
import { json } from "./meetings";
import type { Dictation, HistoryCounts, StatsRange, StyleKind, VoiceStats, VoiceTried } from "./voiceTypes";

export type { Cleanup, Dictation, HistoryCounts, StatsRange, StyleKind, VoiceStats, VoiceStyleChoice, VoiceTried, VoiceTry } from "./voiceTypes";

export interface DictationPage {
  dictations: Dictation[];
  total: number;
  /** Over every dictation the plan shows, whatever the filter. */
  history: HistoryCounts;
  /** How far back the history reaches without Pro, `null` for all of it. */
  window_hours: number | null;
}

export interface DictationFilter {
  q: string;
  app: string | null;
  handsFree: boolean;
}

/** Every history query sits under this key, so a save or a delete refreshes them all. */
export const voiceKey = ["voice", "history"] as const;

export const voiceApi = {
  stats: (range: StatsRange) => json<VoiceStats>(`/api/voice/stats?range=${range}`),
  list: ({ q, app, handsFree }: DictationFilter, page: number) => {
    const qs = new URLSearchParams({ page: String(page) });
    if (q.trim()) qs.set("q", q.trim());
    if (app) qs.set("app", app);
    if (handsFree) qs.set("hands_free", "true");
    return json<DictationPage>(`/api/voice/dictations?${qs}`);
  },
  deleteAll: () => json<{ deleted: number }>("/api/voice/dictations", { method: "DELETE" }),
};

export function useVoiceStats(range: StatsRange = "30d") {
  return useQuery({
    queryKey: [...voiceKey, "stats", range],
    queryFn: () => voiceApi.stats(range),
    placeholderData: keepPreviousData,
  });
}

export function useDictations(filter: DictationFilter) {
  return useInfiniteQuery({
    queryKey: [...voiceKey, "dictations", filter],
    queryFn: ({ pageParam }) => voiceApi.list(filter, pageParam),
    initialPageParam: 0,
    getNextPageParam: (last, pages) =>
      pages.reduce((n, p) => n + p.dictations.length, 0) < last.total ? pages.length : undefined,
    placeholderData: keepPreviousData,
  });
}

const UNFILTERED: DictationFilter = { q: "", app: null, handsFree: false };

/** The unfiltered first page, which the Voice page shares, for its `history` and `window_hours`. */
export function useVoiceHistory() {
  return useDictations(UNFILTERED).data?.pages[0];
}

export function useDeleteAllDictations() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: voiceApi.deleteAll,
    onSuccess: () => qc.invalidateQueries({ queryKey: voiceKey }),
  });
}

export interface VoiceStatus {
  accessibility: "granted" | "missing";
  /** The hotkey is armed. */
  running: boolean;
  /** The voice task's last status line, which says why Voice is not running. */
  status: string;
}

export const voiceStatusKey = ["voice", "status"] as const;

export async function fetchVoiceStatus(): Promise<VoiceStatus> {
  const res = await bearerFetch("/api/voice/status");
  if (!res.ok) {
    throw new Error(`${res.status} ${res.statusText}`);
  }
  return (await res.json()) as VoiceStatus;
}

/** Settings' shortcut recording, which the event tap runs: Fn never reaches a browser page on macOS. */
export type RecorderStep =
  | { kind: "pending"; held: string[] }
  | { kind: "done"; hotkey: string }
  | { kind: "cancel" };

/** Starts the tap recording and keeps the hotkey off for 10 s, so a recorder must renew it while it stays open. */
export async function recordShortcut(recording: boolean): Promise<void> {
  const res = await bearerFetch("/api/voice/record", {
    method: "POST",
    // The stop sent as the tab closes must outlive the page.
    keepalive: true,
    body: JSON.stringify({ recording }),
  });
  if (!res.ok) {
    throw new Error(`${res.status} ${res.statusText}`);
  }
}

export async function fetchRecorded(): Promise<RecorderStep> {
  const res = await bearerFetch("/api/voice/record");
  if (!res.ok) {
    throw new Error(`${res.status} ${res.statusText}`);
  }
  return (await res.json()) as RecorderStep;
}

/** Which part of the cleanup time limit a model broke first. */
export type CleanupLimit = "first_token" | "stall" | "total";

export interface CleanupTest {
  outcome: "done" | "too_slow" | "failed" | "unavailable";
  /** The sample that was sent. */
  raw: string;
  words: number;
  /** The whole-answer limit for `words`: 1.5 s plus 25 ms a word. */
  total_ms: number;
  /** The model's real time, past the limit too. Absent when `unavailable`. */
  ms?: number;
  model?: string;
  /** `done` only. */
  text?: string;
  /** `too_slow` only. */
  limit?: CleanupLimit;
  /** `failed` and `unavailable` only. */
  error?: string;
}

/** Runs a 45-word sample through the saved cleanup model. */
export async function testCleanup(): Promise<CleanupTest> {
  const res = await bearerFetch("/api/voice/cleanup/test", { method: "POST" });
  if (!res.ok) {
    throw new Error(`${res.status} ${res.statusText}`);
  }
  return (await res.json()) as CleanupTest;
}


/** The last clip that ended in the Try Voice field, set by the app-wide socket's `voice_try` frame. */
export const voiceTryKey = ["voice", "try"] as const;

/** A clip that ends while armed goes to the page instead of being pasted. The server drops the arm 5 s after the last renewal, and `null` drops it now. */
export async function armVoiceTry(kind: StyleKind | null): Promise<void> {
  const res = await bearerFetch("/api/voice/try", { method: "POST", body: JSON.stringify({ kind }) });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
}

/** The last try's raw text, cleaned again as for `kind`. */
export async function cleanVoiceTry(raw: string, kind: StyleKind): Promise<VoiceTried> {
  const res = await bearerFetch("/api/voice/try/clean", { method: "POST", body: JSON.stringify({ raw, kind }) });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return (await res.json()) as VoiceTried;
}
