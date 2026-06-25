/** Unlike `LabelPicker`, no checkmark: a bulk selection has no single current label set. Fans one add/remove over every selected meeting via `useBulkSetLabel` (no bulk server route). */

import { useEffect, useRef, useState } from "react";
import { isArchivedLabel, pickerLabels, useCreateLabel, useLabels } from "../../lib/api/labels";
import { useBulkSetLabel, type Meeting } from "../../lib/api/meetings";
import { LabelCap, useLabelCap } from "./LabelCap";
import { labelTone } from "../../lib/labelColors";
import { INLINE_INPUT } from "../field";

interface Props {
  meetings: Pick<Meeting, "id" | "labels">[];
  mode: "add" | "remove";
  /** The selection is archived meetings, so Add lists archived labels too. */
  archived?: boolean;
  open: boolean;
  onClose: () => void;
}

export function BulkLabelPicker({ meetings, mode, archived = false, open, onClose }: Props) {
  const [query, setQuery] = useState("");
  const wrapperRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const labels = useLabels();
  const createLabel = useCreateLabel();
  const cap = useLabelCap();
  const bulkSetLabel = useBulkSetLabel();

  const trimmed = query.trim();
  const all = labels.data ?? [];
  const showArchived = archived || mode === "remove";
  const { shown: filtered, exactMatch: hasExactMatch } = pickerLabels(all, trimmed, { showArchived });
  // Only "Add label" can mint a brand-new label - "Remove label" only
  // makes sense against labels that already exist.
  const showCreateRow = mode === "add" && trimmed.length > 0 && !hasExactMatch;

  useEffect(() => {
    if (!open) return;
    setQuery("");
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

  function apply(labelId: string) {
    bulkSetLabel.mutate({ meetings, labelId, mode });
    onClose();
  }

  async function createAndApply() {
    if (!trimmed || cap !== null) return;
    const created = await createLabel.mutateAsync({ name: trimmed });
    bulkSetLabel.mutate({ meetings, labelId: created.id, mode: "add" });
    setQuery("");
    onClose();
  }

  return (
    <div
      ref={wrapperRef}
      className="absolute bottom-full left-0 mb-1 bg-card border border-line rounded-card shadow-pop py-1 min-w-55 z-20 text-ui"
    >
      <div className="px-2 pb-1">
        <input
          ref={inputRef}
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.stopPropagation();
              onClose();
            } else if (e.key === "Enter" && showCreateRow) {
              void createAndApply();
            }
          }}
          placeholder={mode === "add" ? "Search or create label" : "Search label to remove"}
          className={INLINE_INPUT}
        />
      </div>
      {filtered.length === 0 && !showCreateRow && (
        <p className="px-3 py-2 text-meta text-mut">
          {trimmed ? "No matches." : all.length === 0 ? "No labels yet." : "Type to find an archived label."}
        </p>
      )}
      {filtered.map((l) => {
        const tone = labelTone(l.color);
        return (
          <button
            key={l.id}
            type="button"
            role="option"
            onClick={() => apply(l.id)}
            className="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-paper"
          >
            <span
              className="w-2.25 h-2.25 rounded-[3px] shrink-0"
              style={{ background: tone.fg }}
              aria-hidden
            />
            <span className="flex-1 truncate text-ink">{l.name}</span>
            {!showArchived && isArchivedLabel(l) && <span className="text-caption text-mut shrink-0">Archived</span>}
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
