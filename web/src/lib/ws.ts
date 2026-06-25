import { useEffect, useRef, useState } from "react";

/** Must match the serialized `yogurt-stt::Channel`. */
export type Channel = "mic" | "system";

export interface TranscriptEvent {
  ts_ms: number;
  channel: Channel;
  text: string;
  is_final: boolean;
}

/** `segment_json` in yogurt-server persists channels as "me"/"them" rather than the live "mic"/"system". */
export interface StoredTranscriptSegment {
  ts_ms: number;
  channel: "me" | "them";
  text: string;
}

export function storedSegmentToEvent(
  seg: StoredTranscriptSegment,
): TranscriptEvent {
  return {
    ts_ms: seg.ts_ms,
    channel: seg.channel === "me" ? "mic" : "system",
    text: seg.text,
    is_final: true,
  };
}

/** yogurt-server forwards `Meeting.events_tx` values directly, without a payload wrapper. */
export interface EnhanceProgressEvent {
  type: "enhance_progress";
  phase: "sending" | "streaming" | "done" | "error";
  chars?: number;
  /** Full accumulated raw markdown snapshot, present on `streaming` frames. */
  text?: string;
  /** Human-readable message accompanying `phase: "error"`. */
  message?: string;
}

export type WsMessage =
  | { type: "transcript"; payload: TranscriptEvent }
  | EnhanceProgressEvent;

type ServerFrame = WsMessage;

export function mergeEvent(
  prev: TranscriptEvent[],
  next: TranscriptEvent,
): TranscriptEvent[] {
  for (let i = prev.length - 1; i >= 0; i--) {
    if (prev[i].channel === next.channel) {
      if (!prev[i].is_final) {
        const copy = prev.slice();
        copy[i] = next;
        return copy;
      }
      break;
    }
  }
  return [...prev, next];
}

export type ConnectionStatus =
  | "idle"
  | "connecting"
  | "connected"
  | "reconnecting"
  | "failed";

export interface UseTranscriptWsResult {
  events: TranscriptEvent[];

  connected: boolean;
  connectionStatus: ConnectionStatus;
}

const MAX_RECONNECT_ATTEMPTS = 3;

export function useTranscriptWs(
  meetingId: string | null,
  token: string | null,
  /** Restores persisted lines on remount, once per meeting, before merging live events. */
  seedHistory?: TranscriptEvent[],
): UseTranscriptWsResult {
  const [events, setEvents] = useState<TranscriptEvent[]>([]);
  const [connectionStatus, setConnectionStatus] =
    useState<ConnectionStatus>("idle");

  // Refetches can change seedHistory's reference without adding lines, so seed once per meeting.
  const seededMeetingIdRef = useRef<string | null>(null);
  // The server can redeliver a final between the history fetch and WebSocket subscription.
  const seededFinalsRef = useRef<Set<string>>(new Set());

  const eventsMeetingIdRef = useRef<string | null>(null);

  useEffect(() => {
    // Reset before seeding: waiting for the async token in the connect effect can erase an already-latched seed.
    if (eventsMeetingIdRef.current !== meetingId) {
      eventsMeetingIdRef.current = meetingId;
      seededMeetingIdRef.current = null;
      seededFinalsRef.current = new Set();
      setEvents([]);
    }
    if (!meetingId) return;
    if (seededMeetingIdRef.current === meetingId) return;
    if (!seedHistory || seedHistory.length === 0) return;
    seededMeetingIdRef.current = meetingId;
    seededFinalsRef.current = new Set(
      seedHistory
        .filter((e) => e.is_final)
        .map((e) => `${e.channel} ${e.text}`),
    );
    // Live finals can arrive before the persisted history fetch returns the same lines.
    setEvents((prev) => {
      const existing = new Set(
        prev
          .filter((e) => e.is_final)
          .map((e) => `${e.channel} ${e.text}`),
      );
      const newSeeds = seedHistory.filter(
        (e) => !e.is_final || !existing.has(`${e.channel} ${e.text}`),
      );
      return [...newSeeds, ...prev];
    });
  }, [meetingId, seedHistory]);

  useEffect(() => {
    // WR-06 + BL-01: the per-meeting WS handler now requires the session
    // token (Origin alone is insufficient). Wait until both meetingId AND
    // token are available before attempting to connect — otherwise the
    // first connect would 403 and burn a reconnect attempt. MTG-1: this
    // branch must not touch `events` - clearing on a missing token wiped
    // the history seeded above during the token-fetch window.
    if (!meetingId || !token) {
      setConnectionStatus("idle");
      return;
    }

    const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
    // Encode the token as a URL param (Phase 0's URL-param auth contract).
    // The server's redact_token_in_uri masks this in logs.
    const url = `${proto}//${window.location.host}/ws/meetings/${meetingId}?token=${encodeURIComponent(token)}`;

    // WR-01: reconnect state lives in refs so the lifecycle is owned by the
    // effect's cleanup, not by React reconciliation. The `cancelled` flag
    // is set in cleanup so any in-flight backoff timer or pending WS knows
    // to stand down (we DO NOT want a stale reconnect to fire after the
    // user navigates away or changes meetingId).
    let attempt = 0;
    let cancelled = false;
    let currentWs: WebSocket | null = null;
    let backoffTimer: ReturnType<typeof setTimeout> | null = null;

    const cleanupCurrentWs = () => {
      if (currentWs) {
        currentWs.onopen = null;
        currentWs.onclose = null;
        currentWs.onerror = null;
        currentWs.onmessage = null;
        if (
          currentWs.readyState === WebSocket.CONNECTING ||
          currentWs.readyState === WebSocket.OPEN
        ) {
          currentWs.close();
        }
        currentWs = null;
      }
    };

    const connect = () => {
      if (cancelled) return;
      setConnectionStatus(attempt === 0 ? "connecting" : "reconnecting");

      const ws = new WebSocket(url);
      currentWs = ws;

      ws.onopen = () => {
        if (cancelled) return;
        attempt = 0; // success — reset for any future drop
        setConnectionStatus("connected");
      };

      ws.onmessage = (event) => {
        if (cancelled) return;
        try {
          const frame = JSON.parse(event.data as string) as ServerFrame;
          if (frame.type === "transcript") {
            const ev = frame.payload;
            // Dedupe at the seam: a live FINAL whose (channel, text)
            // matches one already seeded from persisted history is the
            // same utterance landing twice (fetch raced the WS subscribe,
            // or the server redelivered it) — drop it. Partials are never
            // in the seeded set (seed history is always-final) so they're
            // unaffected.
            if (
              ev.is_final &&
              seededFinalsRef.current.has(`${ev.channel} ${ev.text}`)
            ) {
              return;
            }
            setEvents((prev) => mergeEvent(prev, ev));
          }
        } catch {
        }
      };

      const scheduleReconnect = () => {
        if (cancelled) return;
        // Detach listeners on the dead socket so its eventual onclose
        // doesn't re-enter this branch after we've already scheduled.
        cleanupCurrentWs();
        if (attempt >= MAX_RECONNECT_ATTEMPTS) {
          setConnectionStatus("failed");
          return;
        }
        const delay = 500 * (1 << attempt);
        attempt += 1;
        setConnectionStatus("reconnecting");
        backoffTimer = setTimeout(() => {
          backoffTimer = null;
          connect();
        }, delay);
      };

      ws.onclose = scheduleReconnect;
      ws.onerror = scheduleReconnect;
    };

    connect();

    return () => {
      cancelled = true;
      if (backoffTimer) {
        clearTimeout(backoffTimer);
        backoffTimer = null;
      }
      cleanupCurrentWs();
      setConnectionStatus("idle");
    };
  }, [meetingId, token]);

  return {
    events,
    connected: connectionStatus === "connected",
    connectionStatus,
  };
}

/**
 * Hook result for `useEnhanceProgress` — exposes the most recent
 * `enhance_progress` event from the per-meeting WebSocket.
 *
 *   - `phase`  is `"sending"` / `"streaming"` / `"done"` once any event
 *     has been seen, else `null`.
 *   - `chars`  is the running character count from the most recent event,
 *     or `null` if absent.
 *   - `text`   is the full accumulated raw markdown snapshot from the most
 *     recent `streaming` frame, or `null` on `sending` / `done` / `error`
 *     (and before any event) - consumers render it as a live preview and
 *     must clear it once it goes back to `null`.
 *   - `enhancing` is a convenience derived from `phase` —`true` while
 *     `sending` or `streaming`, `false` once `done` (or before any event).
 */
export interface UseEnhanceProgressResult {
  phase: EnhanceProgressEvent["phase"] | null;
  chars: number | null;
  text: string | null;
  enhancing: boolean;
  /** BL-5: last error message from a `phase: "error"` event, or null. */
  errorMessage: string | null;
}

/**
 * Subscribe to `enhance_progress` events on the per-meeting WebSocket
 * (CONTEXT D-23 / D-24).
 *
 * Opens its own connection (independent of `useTranscriptWs`) so a route
 * that only cares about enhance progress (post-meeting view) doesn't have
 * to also mount the transcript list. Both hooks can coexist on the same
 * page — the server fans out to one broadcast subscriber per WS, so two
 * sockets means two subscribers, no message is lost.
 *
 * If `meetingId` or `token` is null, the hook is dormant. Reconnect on
 * drop follows the same backoff schedule as `useTranscriptWs`
 * (500/1000/2000ms, 3 attempts then give up).
 */
export function useEnhanceProgress(
  meetingId: string | null,
  token: string | null,
): UseEnhanceProgressResult {
  const [phase, setPhase] =
    useState<EnhanceProgressEvent["phase"] | null>(null);
  const [chars, setChars] = useState<number | null>(null);
  const [text, setText] = useState<string | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  useEffect(() => {
    if (!meetingId || !token) {
      setPhase(null);
      setChars(null);
      setText(null);
      setErrorMessage(null);
      return;
    }

    return subscribeMeetingFrames(meetingId, token, (raw) => {
      const frame = raw as unknown as WsMessage;
      if (frame.type !== "enhance_progress") return;
      setPhase(frame.phase);
      if (typeof frame.chars === "number") {
        setChars(frame.chars);
      }
      // `text` carries the full accumulated raw markdown snapshot on
      // `streaming` frames. Reset it on every other phase so a stale
      // preview never lingers once streaming ends.
      if (frame.phase === "streaming" && typeof frame.text === "string") {
        setText(frame.text);
      } else {
        setText(null);
      }
      if (frame.phase === "error") {
        setErrorMessage(frame.message ?? "Enhance failed");
      } else {
        setErrorMessage(null);
      }
    });
  }, [meetingId, token]);

  const enhancing = phase === "sending" || phase === "streaming";

  return { phase, chars, text, enhancing, errorMessage };
}

/** `stt_error` WS frame — see `crates/yogurt-server/src/meetings.rs::send_stt_error`. */
interface SttErrorEvent {
  type: "stt_error";
  message: string;
}

/** `system_audio_status` WS frame — see `crates/yogurt-server/src/meetings.rs::relay_system_status`. */
interface SystemAudioStatusEvent {
  type: "system_audio_status";
  state: "stalled" | "recovered" | "failed";
}

const SYSTEM_STALLED_MSG =
  "System audio stopped. Restarting capture. Your mic is still recording.";
const SYSTEM_FAILED_MSG =
  "System audio could not be restored. Your mic is still recording. Stop and start the meeting to retry.";

export interface UseSttErrorResult {
  message: string | null;
  dismiss: () => void;
}

/**
 * Meeting.tsx task 6 — subscribe to `stt_error` (and `system_audio_status`)
 * frames on the per-meeting WS. Fired when the STT engine dies mid-recording (bad Deepgram key,
 * local model load failure); recording keeps running (audio capture is
 * independent of STT) so the user needs a visible, dismissible signal
 * that transcription itself has stopped.
 *
 * Mirrors `useEnhanceProgress`'s connect/backoff lifecycle — a third
 * independent subscription on the same per-meeting socket, alongside the
 * transcript dock's and the chat panel's. The server fans out one
 * broadcast subscriber per WS connection, so this is safe.
 */
export function useSttError(
  meetingId: string | null,
  token: string | null,
): UseSttErrorResult {
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    setMessage(null);
    if (!meetingId || !token) return;
    return subscribeMeetingFrames(meetingId, token, (frame) => {
      if (frame.type === "stt_error") {
        setMessage((frame as unknown as SttErrorEvent).message);
      } else if (frame.type === "system_audio_status") {
        // Rides the same banner. Recovery only clears its own notice.
        const { state } = frame as unknown as SystemAudioStatusEvent;
        if (state === "stalled") setMessage(SYSTEM_STALLED_MSG);
        else if (state === "failed") setMessage(SYSTEM_FAILED_MSG);
        else setMessage((m) => (m === SYSTEM_STALLED_MSG ? null : m));
      }
    });
  }, [meetingId, token]);

  return { message, dismiss: () => setMessage(null) };
}

/**
 * Opens one per-meeting socket and hands every JSON frame to `onFrame`, reconnecting on the same
 * 500/1000/2000ms schedule as the other hooks. Returns the cleanup.
 */
export function subscribeMeetingFrames(
  meetingId: string,
  token: string,
  onFrame: (frame: { type?: string } & Record<string, unknown>) => void,
): () => void {
  const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
  const url = `${proto}//${window.location.host}/ws/meetings/${meetingId}?token=${encodeURIComponent(token)}`;

  let attempt = 0;
  let cancelled = false;
  let currentWs: WebSocket | null = null;
  let backoffTimer: ReturnType<typeof setTimeout> | null = null;

  const cleanupCurrentWs = () => {
    if (currentWs) {
      currentWs.onopen = null;
      currentWs.onclose = null;
      currentWs.onerror = null;
      currentWs.onmessage = null;
      if (
        currentWs.readyState === WebSocket.CONNECTING ||
        currentWs.readyState === WebSocket.OPEN
      ) {
        currentWs.close();
      }
      currentWs = null;
    }
  };

  const connect = () => {
    if (cancelled) return;
    const ws = new WebSocket(url);
    currentWs = ws;

    ws.onmessage = (event) => {
      if (cancelled) return;
      try {
        onFrame(JSON.parse(event.data as string));
      } catch {
      }
    };

    const scheduleReconnect = () => {
      if (cancelled) return;
      cleanupCurrentWs();
      if (attempt >= MAX_RECONNECT_ATTEMPTS) return;
      const delay = 500 * (1 << attempt);
      attempt += 1;
      backoffTimer = setTimeout(() => {
        backoffTimer = null;
        connect();
      }, delay);
    };

    ws.onopen = () => {
      if (cancelled) return;
      attempt = 0;
    };
    ws.onclose = scheduleReconnect;
    ws.onerror = scheduleReconnect;
  };

  connect();

  return () => {
    cancelled = true;
    if (backoffTimer) {
      clearTimeout(backoffTimer);
      backoffTimer = null;
    }
    cleanupCurrentWs();
  };
}

/** `audio_level` WS frame — see `crates/yogurt-server/src/meetings.rs::maybe_emit_audio_level`. */
interface AudioLevelEvent {
  type: "audio_level";
  channel: Channel;
  level: number;
}

export interface AudioLevels {
  mic: number;
  system: number;
}

/** Zero out a channel's level if no `audio_level` event refreshes it within this window. */
const AUDIO_LEVEL_DECAY_MS = 600;

/**
 *
 * Each channel decays to 0 if no fresh event arrives within
 * `AUDIO_LEVEL_DECAY_MS`, so the wave visibly settles once audio stops
 * instead of freezing at its last value.
 */
export function useAudioLevels(
  meetingId: string | null,
  token: string | null,
): AudioLevels {
  const [levels, setLevels] = useState<AudioLevels>({ mic: 0, system: 0 });

  useEffect(() => {
    if (!meetingId || !token) {
      setLevels({ mic: 0, system: 0 });
      return;
    }

    const decayTimers: Record<Channel, ReturnType<typeof setTimeout> | null> = {
      mic: null,
      system: null,
    };
    const unsubscribe = subscribeMeetingFrames(meetingId, token, (frame) => {
      if (frame.type !== "audio_level") return;
      const ev = frame as unknown as AudioLevelEvent;
      setLevels((prev) => ({ ...prev, [ev.channel]: ev.level }));
      if (decayTimers[ev.channel]) clearTimeout(decayTimers[ev.channel]!);
      decayTimers[ev.channel] = setTimeout(() => {
        setLevels((prev) => ({ ...prev, [ev.channel]: 0 }));
      }, AUDIO_LEVEL_DECAY_MS);
    });

    return () => {
      unsubscribe();
      if (decayTimers.mic) clearTimeout(decayTimers.mic);
      if (decayTimers.system) clearTimeout(decayTimers.system);
      setLevels({ mic: 0, system: 0 });
    };
  }, [meetingId, token]);

  return levels;
}
