/** Every click stops propagation and prevents default so the picker can live inside a card `<Link>` without navigating. */

import { useEffect, useRef, useState } from "react";
import { Check } from "lucide-react";
import {
  isArchivedLabel,
  pickerLabels,
  useCreateLabel,
  useLabels,
  type Label,
} from "../../lib/api/labels";
import { useSetMeetingLabels } from "../../lib/api/meetings";
import { LabelCap, useLabelCap } from "./LabelCap";
import { labelTone } from "../../lib/labelColors";
import { INLINE_INPUT } from "../field";

interface Props {
  meetingId: string;
  selected: Label[];
  /** The meeting is archived, so its picker lists archived labels too. */
  archived?: boolean;
  open: boolean;
  onClose: () => void;
  anchorClassName?: string;
}

export function LabelPicker({ meetingId, selected, archived = false, open, onClose, anchorClassName }: Props) {
  const [query, setQuery] = useState("");
  const [touched, setTouched] = useState<ReadonlySet<string>>(new Set());
  const wrapperRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const labels = useLabels();
  const createLabel = useCreateLabel();
  const cap = useLabelCap();
  const setLabels = useSetMeetingLabels();

  const selectedIds = new Set(selected.map((l) => l.id));
  const trimmed = query.trim();
  const all = labels.data ?? [];
  // Unchecking a meeting's last live use archives a label, and `touched` keeps its row so the click can be undone.
  const { shown: filtered, exactMatch } = pickerLabels(all, trimmed, {
    showArchived: archived,
    keep: new Set([...selectedIds, ...touched]),
  });
  const showCreateRow = trimmed.length > 0 && !exactMatch;

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setTouched(new Set());
    inputRef.current?.focus();
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (wrapperRef.current && !wrapperRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open, onClose]);

  if (!open) return null;

  const stop = (e: React.SyntheticEvent) => {
    e.preventDefault();
    e.stopPropagation();
  };

  function toggle(labelId: string) {
    setTouched((t) => new Set(t).add(labelId));
    const next = selectedIds.has(labelId)
      ? selected.filter((l) => l.id !== labelId).map((l) => l.id)
      : [...selected.map((l) => l.id), labelId];
    setLabels.mutate({ id: meetingId, label_ids: next });
  }

  async function createAndApply() {
    if (!trimmed || cap !== null) return;
    const created = await createLabel.mutateAsync({ name: trimmed });
    setLabels.mutate({
      id: meetingId,
      label_ids: [...selected.map((l) => l.id), created.id],
    });
    setQuery("");
  }

  return (
    <div
      ref={wrapperRef}
      onClick={stop}
      // stopPropagation only (no preventDefault) so the input still gets
      // focus / text selection on mousedown; Link navigation fires on click,
      // which `stop` above already cancels.
      onMouseDown={(e) => e.stopPropagation()}
      className={
        anchorClassName ??
        "absolute left-0 top-full mt-1 bg-card border border-line rounded-card shadow-pop py-1 min-w-55 z-20 text-ui"
      }
    >
      <div className="px-2 pb-1">
        <input
          ref={inputRef}
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              onClose();
            } else if (e.key === "Enter" && showCreateRow) {
              void createAndApply();
            }
          }}
          // ponytail: no up/down arrow-key navigation between rows — mouse
          // + Enter-to-create covers the common path; add arrow nav if
          // keyboard-only users report friction.
          placeholder="Search or create label"
          className={INLINE_INPUT}
        />
      </div>
      {filtered.length === 0 && !showCreateRow && (
        <p className="px-3 py-2 text-meta text-mut">
          {trimmed
            ? "No matches."
            : all.length === 0
              ? "No labels yet. Type to create one."
              : "Type to find an archived label."}
        </p>
      )}
      {filtered.map((l) => {
        const tone = labelTone(l.color);
        const isSelected = selectedIds.has(l.id);
        return (
          <button
            key={l.id}
            type="button"
            role="option"
            aria-selected={isSelected}
            onClick={() => toggle(l.id)}
            className="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-paper"
          >
            <span
              className="w-2.25 h-2.25 rounded-[3px] shrink-0"
              style={{ background: tone.fg }}
              aria-hidden
            />
            <span className="flex-1 truncate text-ink">{l.name}</span>
            {!archived && isArchivedLabel(l) && <span className="text-caption text-mut shrink-0">Archived</span>}
            {isSelected && <Check size={14} className="text-blue shrink-0" aria-hidden />}
          </button>
        );
      })}
      {showCreateRow &&
        (cap !== null ? (
          <LabelCap count={cap} className="px-3 py-1.5" />
        ) : (
          <button
            type="button"
            onClick={() => void createAndApply()}
            className="w-full text-left px-3 py-1.5 text-blue hover:bg-paper"
          >
            Create &quot;{trimmed}&quot;
          </button>
        ))}
    </div>
  );
}
