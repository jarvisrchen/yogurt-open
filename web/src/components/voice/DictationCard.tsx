import { useEffect, useRef, useState, type FormEvent } from "react";
import { BookA, Check, Copy } from "lucide-react";
import type { Dictation } from "../../lib/api/voiceTypes";
import { Button } from "../Button";
import { fieldClassName } from "../field";
import { Pill } from "../Pill";
import { LABEL } from "../settings/labelClass";
import { formatCount } from "../library/pagination";
import { clipLength } from "./format";
import { RISE_STAGGER_MS } from "./motion";

export const GHOST =
  "inline-flex items-center gap-1 px-2 py-1 rounded-button text-meta font-semibold text-mut hover:text-ink hover:bg-line/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue/40";

/** The selection inside `el`, without the punctuation around it, or "" when there is none. */
function selectedIn(el: HTMLElement | null): string {
  const sel = window.getSelection();
  if (!el || !sel || sel.isCollapsed || !el.contains(sel.anchorNode) || !el.contains(sel.focusNode)) return "";
  return sel
    .toString()
    .replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, "")
    .slice(0, 100);
}

/** The slice of the Mac's `useAddWord` mutation the card uses, typed here so the Cloud webapp can render the card without react-query. */
export interface AddWord {
  mutate: (entry: { word: string; sounds_like: string; source: "learned" }, opts: { onSuccess: () => void }) => void;
  reset: () => void;
  isPending: boolean;
  error: Error | null;
}

/** Without `addWord` the card has no Add to dictionary button. */
export function DictationCard({
  d,
  addWord,
  rise,
}: {
  d: Dictation;
  addWord?: AddWord;
  rise?: number;
}) {
  const [showRaw, setShowRaw] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [copied, setCopied] = useState(false);
  const [adding, setAdding] = useState(false);
  const [added, setAdded] = useState(false);
  const [word, setWord] = useState("");
  const [soundsLike, setSoundsLike] = useState("");
  const textRef = useRef<HTMLParagraphElement>(null);
  const rawRef = useRef<HTMLParagraphElement>(null);
  const selected = () => selectedIn(textRef.current) || selectedIn(rawRef.current);
  // With cleanup off, or too slow to finish, the raw text is what was pasted, so there is nothing else to show.
  const hasRaw = d.raw_text !== d.text && d.cleanup !== "too_slow";
  const transcription = d.stt_engine === "local"
    ? "Local (model not recorded)"
    : d.stt_engine.replace(/^local:/, "").replace(/^deepgram:/, "Deepgram ");
  const cleanup = d.cleanup_model ?? {
    off: "Not applied", done: "Model not recorded", rules: "Rules only",
    too_slow: "Model not recorded", snippet: "Snippet",
  }[d.cleanup];
  const time = new Date(d.created_at).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(d.text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch (e) {
      console.error("copy failed", e);
    }
  };

  const openAdd = () => {
    if (adding) return setAdding(false);
    setWord("");
    setSoundsLike(selected());
    // Cleanup matches the dictionary against the raw text, so its spelling of the misheard word is the one to pick.
    if (hasRaw) setShowRaw(true);
    addWord?.reset();
    setAdding(true);
  };

  // selectionchange rather than mouseup, so a long press on a phone, a keyboard selection, and a drag that ends outside the text all count.
  useEffect(() => {
    if (!adding) return;
    const pick = () => {
      const picked = selected();
      if (picked) setSoundsLike(picked);
    };
    document.addEventListener("selectionchange", pick);
    return () => document.removeEventListener("selectionchange", pick);
  }, [adding]);

  const submitWord = (e: FormEvent) => {
    e.preventDefault();
    if (!word.trim() || !addWord) return;
    addWord.mutate(
      { word, sounds_like: soundsLike.trim().toLowerCase() === word.trim().toLowerCase() ? "" : soundsLike, source: "learned" },
      {
        onSuccess: () => {
          setAdding(false);
          setAdded(true);
          window.setTimeout(() => setAdded(false), 1500);
        },
      },
    );
  };

  return (
    <li
      className={`px-4.5 py-4 bg-card border border-line rounded-card shadow-card ${rise == null ? "" : "motion-safe:animate-rise"}`}
      style={rise == null ? undefined : { animationDelay: `${rise * RISE_STAGGER_MS}ms` }}
    >
      <div className="flex items-start justify-between gap-3">
        <p className="flex flex-wrap items-center gap-1.5 text-meta text-mut min-h-6.5">
          <span className="tabular-nums">{time}</span>
          <span aria-hidden="true">·</span>
          <span>{d.app_name ?? "Unknown"}</span>
          <span aria-hidden="true">·</span>
          <span className="tabular-nums">
            {formatCount(d.words)} {d.words === 1 ? "word" : "words"}, {clipLength(d.audio_ms)}
          </span>
          {d.hands_free && <Pill>Hands-free</Pill>}
          {d.cleanup === "too_slow" && <Pill tone="honey">Pasted raw, cleanup too slow</Pill>}
        </p>
        <div className="flex items-center gap-0.5 shrink-0 -mt-0.5">
          {hasRaw && (
            <button
              type="button"
              aria-expanded={showRaw}
              onClick={() => setShowRaw(!showRaw)}
              className={`${GHOST} ${showRaw ? "bg-line/50 text-ink" : ""}`}
            >
              {showRaw ? "Hide raw" : "Raw"}
            </button>
          )}
          {addWord && (
            <button
              type="button"
              aria-label={added ? "Added to dictionary" : "Add to dictionary"}
              title="Add to dictionary"
              aria-expanded={adding}
              onClick={openAdd}
              className={`${GHOST} ${adding ? "bg-line/50 text-ink" : ""}`}
            >
              {added ? <Check size={14} className="text-matcha" aria-hidden /> : <BookA size={14} aria-hidden />}
            </button>
          )}
          <button type="button" aria-label={copied ? "Copied" : "Copy"} title="Copy" onClick={copy} className={GHOST}>
            {copied ? <Check size={14} className="text-matcha" aria-hidden /> : <Copy size={14} aria-hidden />}
          </button>
        </div>
      </div>
      {/* A click opens a long dictation in full, but not the click that ends a text selection. */}
      <p
        ref={textRef}
        onClick={() => window.getSelection()?.isCollapsed !== false && setExpanded(!expanded)}
        className={`mt-1 text-body text-ink leading-relaxed ${showRaw || expanded ? "" : "line-clamp-2"}`}
      >
        {d.text}
      </p>
      <p className="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-meta text-mut break-words">
        <span className="min-w-0">Transcription: {transcription}</span>
        <span className="min-w-0">{d.cleanup_model && d.cleanup !== "done" ? "Cleanup attempted" : "Cleanup"}: {cleanup}</span>
      </p>
      {adding && addWord && (
        <form onSubmit={submitWord} className="mt-2 rounded-button border border-line bg-paper p-3">
          <p className={LABEL}>Add to dictionary</p>
          <div className="mt-2 flex flex-wrap gap-2">
            <input
              autoFocus
              aria-label="Word"
              className={fieldClassName("md", "flex-1 min-w-35")}
              placeholder="Word or name"
              maxLength={100}
              value={word}
              onChange={(e) => setWord(e.target.value)}
            />
            <input
              aria-label="Sounds like"
              className={fieldClassName("md", "flex-1 min-w-35")}
              placeholder="Sounds like (optional)"
              maxLength={100}
              value={soundsLike}
              onChange={(e) => setSoundsLike(e.target.value)}
            />
            <Button type="submit" disabled={!word.trim() || addWord.isPending}>
              Add
            </Button>
            <Button variant="secondary" onClick={() => setAdding(false)}>
              Cancel
            </Button>
          </div>
          <p className="mt-2 text-meta text-mut">
            {addWord.error
              ? `Couldn't add it: ${addWord.error.message}`
              : `Select the misheard word in ${hasRaw ? "What you said" : "the text"} to fill in Sounds like, then type how it is spelled. It is saved as Learned.`}
          </p>
        </form>
      )}
      {showRaw && (
        <div className="mt-2 rounded-button border border-line bg-paper p-3">
          <p className={LABEL}>What you said</p>
          <p ref={rawRef} className="mt-1 text-ui text-mut leading-relaxed">{d.raw_text}</p>
        </div>
      )}
    </li>
  );
}
