import { useEffect, useRef, useState, type Ref } from "react";
import { useUpdateMeetingTitle } from "../../lib/api/meetings";
import { splitTitle, withName } from "./titles";

interface Props {
  id: string;
  title: string;
  startedAt?: number;
  className?: string;
  /** Set only during the move: two elements sharing a name cancel the transition. */
  viewTransitionName?: string;
  /** The title as shown, not while it is being edited. */
  ref?: Ref<HTMLSpanElement>;
}

export function InlineTitle({ id, title, startedAt, className, viewTransitionName, ref: shownRef }: Props) {
  const { name, date } = splitTitle(title, startedAt);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(name);
  const ref = useRef<HTMLInputElement>(null);
  const update = useUpdateMeetingTitle();

  useEffect(() => {
    setDraft(name);
  }, [name]);

  useEffect(() => {
    if (editing) ref.current?.select();
  }, [editing]);

  const commit = () => {
    const next = withName(title, startedAt, draft.trim() || (date ? "Meeting" : "Untitled meeting"));
    if (next !== title) update.mutate({ id, title: next });
    setEditing(false);
  };

  if (!editing) {
    return (
      <span
        ref={shownRef}
        className={className}
        style={viewTransitionName ? { viewTransitionName } : undefined}
        onDoubleClick={(e) => {
          e.preventDefault();
          e.stopPropagation();
          setEditing(true);
        }}
        title="Double-click to rename"
      >
        {title || "Untitled meeting"}
      </span>
    );
  }
  const input = (
    <input
      ref={ref}
      value={draft}
      // Keep the surrounding <Link> from navigating while editing.
      onClick={(e) => e.stopPropagation()}
      onMouseDown={(e) => e.stopPropagation()}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          commit();
        } else if (e.key === "Escape") {
          setDraft(name);
          setEditing(false);
        }
      }}
      className={`${className ?? ""} bg-card border border-blue rounded-button px-1 -mx-1 outline-none`}
    />
  );
  if (!date) return input;
  return (
    <span className="flex min-w-0 items-baseline gap-2">
      {input}
      <span className={`${className ?? ""} shrink-0 text-mut!`}>{date}</span>
    </span>
  );
}
