import { useEffect, useState } from "react";
import { Link } from "react-router";
import type { MeetingSummary } from "../../lib/api/meetings";
import { LabelChip } from "../labels/LabelChip";
import { EnginePill, LlmPill, MetaPill } from "../MeetingMetaPills";
import { InlineTitle } from "./InlineTitle";
import { shortDate } from "./titles";
import { TitleHint } from "./TitleHint";
import { MeetingCardActions } from "./MeetingCardActions";
import { useTransitionTouches } from "../../lib/viewTransition";
import { Slot, slots } from "../../slots";

// The "not enhanced" warn pill marks only an ended meeting with no enrichment
// pass. Flagging the normal enhanced case would be noise.
export function metaParts(m: MeetingSummary): { text: string; tone: "neutral" | "warn" }[] {
  const parts: { text: string; tone: "neutral" | "warn" }[] = [
    {
      text: shortDate(m.started_at),
      tone: "neutral",
    },
    {
      text: new Date(m.started_at).toLocaleTimeString(undefined, {
        hour: "numeric",
        minute: "2-digit",
      }),
      tone: "neutral",
    },
  ];
  if (m.ended_at != null && m.ended_at > m.started_at) {
    const minutes = Math.max(1, Math.round((m.ended_at - m.started_at) / 60_000));
    parts.push({ text: `${minutes} min`, tone: "neutral" });
  }
  if (m.ended_at != null && !m.has_enriched) {
    parts.push({ text: "not enhanced", tone: "warn" });
  }
  return parts;
}

interface Props {
  meeting: MeetingSummary;
  // Passed down from the Library route so the list shares one poll.
  activeId?: string | null;
  selectionActive?: boolean;
  selected?: boolean;
  onToggleSelect?: (id: string, shiftKey: boolean) => void;
  archived?: boolean;
}

export function MeetingCard({
  meeting,
  activeId,
  archived = false,
  selectionActive = false,
  selected = false,
  onToggleSelect,
}: Props) {
  const isLive = activeId != null && activeId === meeting.id;
  const parts = metaParts(meeting);
  const caption = parts
    .filter((p) => p.tone !== "warn")
    .map((p) => p.text)
    .join(" · ");
  const warnPart = parts.find((p) => p.tone === "warn");
  // A live meeting opens the capture surface. The post view has no controls or live transcript.
  const to = isLive ? `/meeting/${meeting.id}` : `/meeting/${meeting.id}/post`;
  const morphing = useTransitionTouches(to);
  // Mounted from the first suggestion on, so a card without one carries none of its hooks, and one that had one can fold it shut.
  const [hinted, setHinted] = useState(false);
  if (!hinted && meeting.title_suggestions?.length) setHinted(true);
  // The title it replaced, crossfading out over the new one.
  const [renamedFrom, setRenamedFrom] = useState<string | null>(null);
  const swapped = renamedFrom != null && renamedFrom !== meeting.title;
  useEffect(() => {
    if (!swapped) return;
    // As long as the card's flash, the longest piece of the swap.
    const t = setTimeout(() => setRenamedFrom(null), 1100);
    return () => clearTimeout(t);
  }, [swapped]);
  return (
    <Link
      to={to}
      className={`group flex items-start justify-between gap-4 px-4.5 py-4 bg-card border ${
        isLive ? "border-straw" : "border-line"
      } rounded-card shadow-card hover:border-grey/60 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue/40 ${
        swapped ? "motion-safe:animate-flash" : ""
      }`}
    >
      <label
        className={`shrink-0 flex items-center h-5.5 pr-0.5 ${
          selectionActive
            ? "opacity-100"
            : "opacity-0 group-hover:opacity-100 group-focus-within:opacity-100"
        }`}
        // No preventDefault, so the label still toggles its checkbox natively.
        onClick={(e) => e.stopPropagation()}
      >
        <input
          type="checkbox"
          checked={selected}
          // onChange rather than onClick+preventDefault: suppressing the native
          // toggle from a click handler leaves the DOM `checked` out of sync with
          // `selected` (a shift-range-selected box stayed unchecked).
          onChange={(e) =>
            onToggleSelect?.(meeting.id, (e.nativeEvent as MouseEvent).shiftKey)
          }
          aria-label={selected ? `Deselect ${meeting.title}` : `Select ${meeting.title}`}
          className="w-4 h-4 rounded-[4px] border border-line accent-blue cursor-pointer"
        />
      </label>
      <div className="flex-1 min-w-0">
        <div className="relative flex items-center gap-1.5 min-w-0">
          <InlineTitle
            key={meeting.title}
            id={meeting.id}
            title={meeting.title}
            startedAt={meeting.started_at}
            className={`block min-w-0 text-lead font-semibold text-ink truncate ${
              swapped ? "motion-safe:animate-[fade-up_260ms_var(--ease-pop)_100ms_both]" : ""
            }`}
            viewTransitionName={morphing ? "meeting-title" : undefined}
          />
          {swapped && (
            <span
              aria-hidden
              className="absolute left-0 top-0 max-w-full truncate text-lead font-semibold text-ink motion-safe:animate-old-out motion-reduce:hidden"
            >
              {renamedFrom}
            </span>
          )}
        </div>
        {hinted && <TitleHint meeting={meeting} hidden={isLive} onRenamed={setRenamedFrom} />}
        {isLive ? (
          <div className="mt-1">
            <MetaPill tone="warn">
              <span
                aria-hidden="true"
                className="inline-block w-1.5 h-1.5 rounded-pill bg-straw animate-recpulse"
              />
              Recording
            </MetaPill>
          </div>
        ) : (
          <div className="mt-1 flex flex-wrap items-center gap-1.5">
            {caption && <span className="text-meta text-mut">{caption}</span>}
            {warnPart && <MetaPill tone="warn">{warnPart.text}</MetaPill>}
          </div>
        )}
      </div>
      <div className="flex items-center gap-1.5 shrink-0">
        {meeting.labels.map((l) => (
          <LabelChip key={l.id} label={l} size="sm" />
        ))}
        <Slot of={slots.meetingCard} meeting={meeting} />
        {/* Pre-column rows render nothing rather than a guessed "Local". */}
        <EnginePill sttEngine={meeting.stt_engine} />
        <LlmPill llmModel={meeting.llm_model} />
        <MeetingCardActions
          id={meeting.id}
          title={meeting.title}
          starred={meeting.starred}
          labels={meeting.labels}
          archived={archived}
          isLive={isLive}
        />
      </div>
    </Link>
  );
}
