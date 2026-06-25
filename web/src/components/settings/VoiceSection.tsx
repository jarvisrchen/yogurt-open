import { Fragment, useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  settingsApi,
  VOICE_RETENTIONS,
  VOICE_STT_DEEPGRAM,
  voiceModel,
  type General,
  type ProviderView,
  type VoiceRetention,
} from "../../lib/api/settings";
import {
  fetchRecorded,
  fetchVoiceStatus,
  recordShortcut,
  useDeleteAllDictations,
  useVoiceHistory,
  voiceKey,
  voiceStatusKey,
  type VoiceStatus,
} from "../../lib/api/voice";
import { useModels } from "../../lib/api/stt";
import { heldKey, hotkeyKeys, isFnHotkey } from "../../lib/hotkey";
import { useMicrophoneStatus } from "../../hooks/useMicrophoneStatus";
import { Button, buttonClassName } from "../Button";
import { Pill } from "../Pill";
import { LABEL } from "./labelClass";
import { MicDevicePicker } from "../MicDevicePicker";
import { Select } from "../Select";
import { CleanupField } from "./CleanupField";
import { ModelDownloadButton, TranscriptionField } from "./TranscriptionField";
import { Slot, slots } from "../../slots";

const CAPTION = "text-meta text-mut";
const KBD =
  "inline-flex items-center justify-center min-w-6.5 h-6 px-1.5 rounded-[6px] border border-line bg-card text-meta font-semibold font-mono text-ink shadow-[0_1px_0_var(--color-line)]";
const ACCESSIBILITY_PANE =
  "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

export function VoiceSection({
  general,
  providers,
  deepgramKey,
}: {
  general: General;
  providers: ProviderView[];
  deepgramKey: boolean;
}) {
  const qc = useQueryClient();
  const onSaved = () => qc.invalidateQueries({ queryKey: ["settings"] });
  const toggle = useMutation({
    mutationFn: (voice_enabled: boolean) => settingsApi.patch({ voice_enabled }),
    onSuccess: onSaved,
  });
  const saveRetention = useMutation({
    mutationFn: (voice_retention: VoiceRetention) => settingsApi.patch({ voice_retention }),
    // A shorter retention deletes text at once, so the Voice page refreshes too.
    onSuccess: () => {
      onSaved();
      qc.invalidateQueries({ queryKey: voiceKey });
    },
  });
  const saveHotkey = useMutation({
    mutationFn: (voice_hotkey: string) => settingsApi.patch({ voice_hotkey }),
    onSuccess: onSaved,
  });
  const [recording, setRecording] = useState(false);
  // 2 s matches how often the voice task checks for the grant, and nothing changes while Voice is off and granted.
  const status = useQuery({
    queryKey: voiceStatusKey,
    queryFn: fetchVoiceStatus,
    refetchInterval: (q) =>
      general.voice_enabled || q.state.data?.accessibility === "missing" ? 2_000 : false,
  });
  const v = status.data;
  const model = voiceModel(general);
  // The models list does not poll, so a model fetched elsewhere would read missing while Voice already runs on it.
  const models = useModels();
  const missingModel = v?.running ? undefined : models.data?.find((m) => m.name === model && !m.downloaded);
  // "off" means the voice task has not yet seen Voice turned on.
  const stalled =
    general.voice_enabled &&
    v?.accessibility === "granted" &&
    !v.running &&
    !recording &&
    v.status !== "off";

  return (
    <section className="space-y-4">
      <header className="space-y-1">
        <h2 className="heading-section">Voice</h2>
        <p className="text-ui text-mut">
          Dictate into any app. Hold the shortcut, talk, and let go. Audio is
          never saved.
        </p>
      </header>

      <div className="rounded-card border border-line bg-card p-5 space-y-4">
        <div className="space-y-1.5">
          <div className="flex items-center gap-2 text-ui text-ink font-semibold">
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={general.voice_enabled}
                onChange={(e) => toggle.mutate(e.target.checked)}
                className="h-4 w-4 accent-blue"
              />
              <span>Turn on Yogurt Voice</span>
            </label>
            {general.voice_enabled && v && (
              <StatePill status={v} stalled={stalled} missing={!!missingModel} sttModel={model} />
            )}
            {general.voice_enabled && v?.accessibility === "granted" && missingModel && (
              <span className="ml-auto font-normal">
                <ModelDownloadButton model={missingModel} label={`Download the Voice model, ${missingModel.name}`} />
              </span>
            )}
          </div>
          {stalled && <p className={CAPTION}>{sentence(v.status)}</p>}
          {toggle.error && <ErrorLine error={toggle.error} />}
        </div>

        <ShortcutField
          hotkey={general.voice_hotkey}
          canRecord={v?.accessibility !== "missing"}
          recording={recording}
          setRecording={setRecording}
          onRecorded={(hotkey) => saveHotkey.mutate(hotkey)}
          error={saveHotkey.error}
        />

        <div className="border-t border-line pt-4 mt-4 space-y-1.5">
          <label className={`block ${LABEL}`}>Microphone</label>
          <MicDevicePicker voice />
          <p className={CAPTION}>
            Meetings record from the input in Settings › Audio. Pick another to dictate with a different mic. Try it on
            the Voice page.
          </p>
        </div>

        {(v || status.isError) && (
          <div className="border-t border-line pt-4 mt-4 space-y-1.5">
            <label className={`block ${LABEL}`}>Permissions</label>
            {!v ? (
              <ErrorLine error={status.error} prefix="Could not read the Accessibility status: " />
            ) : v.accessibility === "missing" ? (
              <>
                <div className="flex items-center justify-between gap-3">
                  <div className="flex items-center gap-2 text-ui text-ink">
                    Accessibility <Grant tone="straw">Missing</Grant>
                  </div>
                  <a
                    href={ACCESSIBILITY_PANE}
                    className={buttonClassName("primary", "whitespace-nowrap", "sm")}
                  >
                    Open System Settings
                  </a>
                </div>
                <p className={CAPTION}>
                  Voice needs it to see the shortcut and to paste. macOS lists
                  the grant under the terminal that runs yogurt, so turn that
                  terminal on. Voice starts by itself once it is granted.
                </p>
              </>
            ) : (
              <>
                <div className="flex items-center gap-2 text-ui text-ink">
                  Accessibility <Grant tone="matcha">Granted</Grant>
                  <MicrophoneGrant />
                </div>
                <p className={CAPTION}>
                  Both grants live under the terminal that runs yogurt.
                </p>
              </>
            )}
          </div>
        )}

        <TranscriptionField general={general} deepgramKey={deepgramKey} />

        <CleanupField general={general} providers={providers} />

        <Slot of={slots.settingsRows.voice} />

        <div className="border-t border-line pt-4 mt-4 space-y-2">
          <label htmlFor="voice-retention" className={`block ${LABEL}`}>
            History
          </label>
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
            <span className="text-ui text-ink">Keep the text of each dictation for</span>
            <Select
              id="voice-retention"
              className="block w-auto"
              value={general.voice_retention}
              disabled={saveRetention.isPending}
              onChange={(e) => saveRetention.mutate(e.target.value as VoiceRetention)}
            >
              {VOICE_RETENTIONS.map((r) => (
                <option key={r.value} value={r.value}>
                  {r.label}
                </option>
              ))}
            </Select>
          </div>
          {saveRetention.error && <ErrorLine error={saveRetention.error} />}
          <p className={CAPTION}>
            {general.voice_retention === "never"
              ? "No text is saved, so the Voice page shows stats but no history. Audio is never saved, and the saved text never leaves this Mac."
              : "Saved on this Mac for the Voice page in the Library, then deleted. A shorter time deletes older text right away. The stats keep counting either way, audio is never saved, and the saved text never leaves this Mac."}
          </p>
          <DeleteAllDictations />
        </div>
      </div>
    </section>
  );
}

/** Two clicks, since what it deletes cannot be brought back. The ask lapses after 3 s, like a meeting delete. */
function DeleteAllDictations() {
  const page = useVoiceHistory();
  const hours = page?.window_hours ?? null;
  const within = hours == null ? "" : ` from the last ${hours} hours`;
  const del = useDeleteAllDictations();
  const [confirming, setConfirming] = useState(false);

  useEffect(() => {
    if (!confirming || del.isPending) return;
    const t = window.setTimeout(() => setConfirming(false), 3000);
    return () => window.clearTimeout(t);
  }, [confirming, del.isPending]);

  if (confirming) {
    return (
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-ui text-ink">
          {hours == null
            ? "Delete all dictations and reset the stats? This cannot be undone."
            : `Delete the dictations${within}? Older ones and the stats are kept. This cannot be undone.`}
        </span>
        <Button
          size="sm"
          variant="warn"
          disabled={del.isPending}
          onClick={() => del.mutate(undefined, { onSettled: () => setConfirming(false) })}
        >
          Delete
        </Button>
        <Button size="sm" variant="secondary" disabled={del.isPending} onClick={() => setConfirming(false)}>
          Cancel
        </Button>
      </div>
    );
  }
  const n = del.data?.deleted;
  return (
    <div className="space-y-1.5">
      <Button
        size="sm"
        variant="secondary"
        disabled={!page || (hours != null && page.history.count === 0)}
        onClick={() => {
          del.reset();
          setConfirming(true);
        }}
      >
        Delete all dictations
      </Button>
      {del.error ? (
        <ErrorLine error={del.error} />
      ) : (
        n != null && (
          <p className={CAPTION}>
            {hours != null
              ? `Deleted ${n.toLocaleString("en-US")} ${n === 1 ? "dictation" : "dictations"}${within}.`
              : n === 0
                ? "Reset the stats."
                : `Deleted ${n.toLocaleString("en-US")} ${n === 1 ? "dictation" : "dictations"} and reset the stats.`}
          </p>
        )
      )}
    </div>
  );
}

function StatePill({
  status,
  stalled,
  missing,
  sttModel,
}: {
  status: VoiceStatus;
  stalled: boolean;
  missing: boolean;
  sttModel: string;
}) {
  if (status.accessibility === "missing") {
    return (
      <Pill tone="honey" className="ml-1">
        Waiting for Accessibility
      </Pill>
    );
  }
  if (missing) {
    return (
      <Pill tone="honey" className="ml-1">
        Model missing
      </Pill>
    );
  }
  if (stalled) {
    return (
      <Pill tone="honey" className="ml-1">
        Not running
      </Pill>
    );
  }
  // Voice picks its own engine, whatever `stt_provider` says.
  return (
    <Pill tone="blue" className="ml-1">
      {sttModel === VOICE_STT_DEEPGRAM ? "On · Deepgram" : `On · local ${sttModel}`}
    </Pill>
  );
}

function sentence(s: string): string {
  return `${s.charAt(0).toUpperCase()}${s.slice(1)}.`;
}

function ErrorLine({ error, prefix = "" }: { error: Error | null; prefix?: string }) {
  return (
    <p className="text-meta text-straw" role="alert">
      {prefix}
      {error?.message}
    </p>
  );
}

function Grant({ tone, children }: { tone: "matcha" | "straw"; children: string }) {
  return (
    <Pill tone={tone}>
      <span
        className={`w-1.5 h-1.5 rounded-pill ${tone === "matcha" ? "bg-matcha" : "bg-straw"}`}
      />
      {children}
    </Pill>
  );
}

function MicrophoneGrant() {
  const mic = useMicrophoneStatus({ refetchIntervalMs: 10_000 });
  if (mic.status === undefined) return null;
  return <Grant tone={mic.granted ? "matcha" : "straw"}>Microphone</Grant>;
}

function ShortcutField({
  hotkey,
  canRecord,
  recording,
  setRecording,
  onRecorded,
  error,
}: {
  hotkey: string;
  /** The event tap records the shortcut, and it needs Accessibility. */
  canRecord: boolean;
  recording: boolean;
  setRecording: (recording: boolean) => void;
  onRecorded: (hotkey: string) => void;
  error: Error | null;
}) {
  const qc = useQueryClient();
  const [held, setHeld] = useState<string[]>([]);
  // A ref, so a new callback each render does not restart the recording.
  const recorded = useRef(onRecorded);
  recorded.current = onRecorded;
  // One queue across recordings, so a slow renewal cannot land after the stop and leave the tap swallowing every key.
  const sent = useRef(Promise.resolve());

  useEffect(() => {
    if (!recording) return;
    let live = true;
    const stop = () => {
      live = false;
      setRecording(false);
      setHeld([]);
    };
    // A failed start shows as a recorder that never sees a key, which Cancel still ends.
    const send = (on: boolean) =>
      (sent.current = sent.current.then(() => recordShortcut(on)).catch(() => undefined));
    const start = () => void send(true);
    start();
    const renew = window.setInterval(start, 4_000);
    const poll = window.setInterval(() => {
      void fetchRecorded()
        .then((step) => {
          if (!live) return;
          if (step.kind === "pending") {
            setHeld(step.held.map(heldKey));
            return;
          }
          if (step.kind === "done") recorded.current(step.hotkey);
          stop();
        })
        .catch(() => undefined);
    }, 100);
    // The tap swallows every key while it records, so a user who clicks away must not lose their typing to it.
    window.addEventListener("blur", stop);
    // An unloading page never runs the cleanup below.
    const leave = () => void recordShortcut(false).catch(() => undefined);
    window.addEventListener("pagehide", leave);
    return () => {
      window.removeEventListener("pagehide", leave);
      live = false;
      window.clearInterval(renew);
      window.clearInterval(poll);
      window.removeEventListener("blur", stop);
      void send(false).then(() => qc.invalidateQueries({ queryKey: voiceStatusKey }));
    };
  }, [recording, setRecording, qc]);

  return (
    <div className="border-t border-line pt-4 mt-4 space-y-1.5">
      <label className={`block ${LABEL}`}>Shortcut</label>
      {recording ? (
        <>
          <div className="flex items-center gap-2">
            <div
              data-testid="shortcut-recorder"
              className="flex-1 flex items-center gap-2 min-h-10.5 rounded-button border border-blue bg-paper px-3 py-2 text-ui ring-2 ring-blue/20"
            >
              <span className="w-1.5 h-1.5 rounded-pill bg-straw animate-pulse" />
              <span className="text-mut" role="status">
                Press the keys you want. Esc cancels.
              </span>
              {held.length > 0 && (
                <span className="ml-auto flex items-center gap-1.5">
                  {held.map((k, i) => (
                    <span key={i} className={KBD}>
                      {k}
                    </span>
                  ))}
                </span>
              )}
            </div>
            <Button size="sm" variant="secondary" onClick={() => setRecording(false)}>
              Cancel
            </Button>
          </div>
          <p className={CAPTION}>
            Fn alone, a single modifier like Right Option, or a combo like
            Option+E. Holding Option shows here as you press it.
          </p>
        </>
      ) : (
        <>
          <div className="flex items-center gap-2">
            <div
              data-testid="shortcut"
              className="flex-1 flex items-center gap-1.5 min-h-10.5 rounded-button border border-line bg-paper px-3 py-2 text-ui"
            >
              {hotkeyKeys(hotkey).map((k, i) => (
                <Fragment key={i}>
                  {i > 0 && <span className="text-mut text-meta">+</span>}
                  <span className={KBD}>{k}</span>
                </Fragment>
              ))}
            </div>
            <Button size="sm" variant="secondary" disabled={!canRecord} onClick={() => setRecording(true)}>
              Change
            </Button>
          </div>
          {error && <ErrorLine error={error} />}
          {!canRecord && <p className={CAPTION}>Changing it needs Accessibility, below.</p>}
          <p className={CAPTION}>
            Hold it while you talk and let go to paste. Or tap it once to talk
            hands-free, and tap again to paste.
          </p>
          {isFnHotkey(hotkey) && (
            <div className="flex gap-2 rounded-button bg-honeysoft px-3 py-2.5 text-meta text-honeyink">
              <span aria-hidden="true">🌐</span>
              <span>
                Set System Settings › Keyboard › <b>Press 🌐 key to</b> ›{" "}
                <b>Do Nothing</b>, or macOS opens the emoji picker on the same
                press.
              </span>
            </div>
          )}
        </>
      )}
    </div>
  );
}
