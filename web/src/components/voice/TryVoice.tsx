import { useEffect, useState, type ReactNode } from "react";
import { RotateCcw } from "lucide-react";
import type { StyleKind, VoiceStyleChoice, VoiceTry } from "../../lib/api/voiceTypes";
import { Button } from "../Button";
import { fieldClassName } from "../field";
import { Pill } from "../Pill";
import { Segmented } from "../Segmented";
import { LABEL } from "../settings/labelClass";

// Shared with the phone's Voice tab in the Cloud webapp, so nothing here may import react-router or react-query.

const KINDS: { value: StyleKind; label: string }[] = [
  { value: "messages", label: "Messages" },
  { value: "work", label: "Work" },
  { value: "email", label: "Email" },
  { value: "other", label: "Other" },
];

const STYLE_LABEL: Record<VoiceStyleChoice, string> = { casual: "Casual", normal: "Normal", formal: "Formal" };

const secs = (ms: number) => `${(ms / 1000).toFixed(2)} s`;

/** `local:parakeet-v3` reads as `parakeet-v3`, and `deepgram:nova-3` as `Deepgram nova-3`. */
const engineName = (engine: string) => engine.replace(/^local:/, "").replace(/^deepgram:/, "Deepgram ");

function cleanupStep(t: VoiceTry): string {
  switch (t.cleanup) {
    case "done":
      return `${t.cleanup_model} · ${secs(t.cleanup_ms ?? 0)}`;
    case "too_slow":
      return `${t.cleanup_model}: too slow, kept what was heard`;
    case "rules":
      return "Too short for the model, fillers dropped";
    case "snippet":
      return "Snippet";
    case "off":
      return t.cleanup_model ? `${t.cleanup_model}: failed, kept what was heard` : "Cleanup off";
  }
}

interface TryVoiceProps {
  /** The last clip that ended in the field, with its cleanup for `kind`. */
  result: VoiceTry | null;
  kind: StyleKind;
  /** Picking a kind cleans the last clip's raw text again, without talking again. Without it, styles are off and the card shows none. */
  onKind?: (kind: StyleKind) => void;
  cleaning?: boolean;
  error?: string;
  /** How to dictate into the field, such as "Click here, hold fn, and talk." */
  placeholder: string;
  /** What shows under the field before the first clip, naming the pill or the bubble. */
  hint?: string;
  /** The Microphone control, which differs between the Mac and the phone. */
  mic: ReactNode;
  /** A clip goes to the field instead of another app only while the field has focus. */
  onFocusChange: (focused: boolean) => void;
}

export function TryVoice({ result, kind, onKind, cleaning, error, placeholder, hint = "The pill shows as in any app. Nothing is pasted elsewhere or saved.", mic, onFocusChange }: TryVoiceProps) {
  const [text, setText] = useState("");
  useEffect(() => setText(result?.text ?? ""), [result]);

  return (
    <section aria-label="Try Voice" className="rounded-card border border-line bg-card shadow-card p-4 space-y-3">
      <h2 className={LABEL}>Try Voice</h2>

      <div className="relative">
        <textarea
          aria-label="Try Voice text"
          rows={3}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onFocus={() => onFocusChange(true)}
          onBlur={() => onFocusChange(false)}
          placeholder={placeholder}
          className={fieldClassName("md", `block w-full resize-none ${text ? "pr-24" : ""}`)}
        />
        {text && (
          <Button variant="secondary" size="sm" className="absolute top-2 right-2" onClick={() => setText("")}>
            <RotateCcw size={13} aria-hidden />
            Clear
          </Button>
        )}
      </div>

      {result ? (
        <div className="space-y-2" aria-live="polite">
          <p className="text-meta text-mut">
            <span className="font-semibold">Heard:</span> {result.raw}
          </p>
          <div className={`flex flex-wrap items-center gap-1.5 text-meta text-mut ${cleaning ? "opacity-60" : ""}`}>
            <Pill>
              {engineName(result.stt_engine)} · {secs(result.stt_ms)}
            </Pill>
            <Pill>{cleaning ? "Cleaning up..." : cleanupStep(result)}</Pill>
            {onKind && result.style && <Pill tone="blue">{STYLE_LABEL[result.style]}</Pill>}
            {result.words.length > 0 && <Pill tone="matcha">Dictionary: {result.words.join(", ")}</Pill>}
            {result.fillers.length > 0 && (
              <span>
                {result.fillers.length === 1 ? "Filler" : "Fillers"} removed: {result.fillers.join(", ")}
              </span>
            )}
          </div>
        </div>
      ) : (
        <p className="text-meta text-mut">{hint}</p>
      )}
      {error && (
        <p className="rounded-button border border-straw/40 bg-strsoft px-3 py-2 text-meta text-ink" role="alert">
          {error}
        </p>
      )}

      <div className={`grid gap-x-4 gap-y-3 grid-cols-1 ${onKind ? "@[620px]:grid-cols-[minmax(0,1fr)_auto]" : ""}`}>
        <div className="space-y-1.5 min-w-0">
          <span className={`flex items-center h-5 ${LABEL}`}>Microphone</span>
          {mic}
        </div>
        {onKind && (
          <div className="space-y-1.5">
            <span className={`flex items-center h-5 ${LABEL}`}>Clean up as</span>
            <Segmented label="Clean up as" options={KINDS} value={kind} onChange={onKind} size="sm" />
          </div>
        )}
      </div>
    </section>
  );
}
