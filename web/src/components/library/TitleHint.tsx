import { Sparkles, X } from "lucide-react";
import { useState } from "react";
import { useDismissTitleSuggestions, useUpdateMeetingTitle, type MeetingSummary } from "../../lib/api/meetings";
import { Fold } from "../Fold";
import { withName } from "./titles";

interface Props {
  meeting: MeetingSummary;
  /** Hides the hint while the meeting is live. */
  hidden: boolean;
  /** After a Use, with the title it replaced. */
  onRenamed: (from: string) => void;
}

/** A Library card's offer of enhance's first name, under its title. */
export function TitleHint({ meeting, hidden, onRenamed }: Props) {
  const suggested = hidden ? undefined : meeting.title_suggestions?.[0];
  // Kept while the hint folds shut, after the list has dropped it.
  const [hint, setHint] = useState(suggested);
  // Stays set through a slow request, so the hint never unfolds again before the list catches up.
  const [acted, setActed] = useState(false);
  if (suggested && suggested !== hint) {
    setHint(suggested);
    setActed(false);
  }
  const [error, setError] = useState<string | null>(null);
  const rename = useUpdateMeetingTitle();
  const dismissal = useDismissTitleSuggestions();

  const act = (e: React.MouseEvent, run: () => Promise<unknown>) => {
    // The hint sits inside the card's link.
    e.preventDefault();
    e.stopPropagation();
    if (acted) return;
    setActed(true);
    setError(null);
    run().catch((err: unknown) => {
      setActed(false);
      setError(err instanceof Error ? err.message : String(err));
    });
  };

  return (
    <Fold open={!!suggested && !acted}>
      {hint && (
        <span className="mt-1.5 inline-flex max-w-full items-center gap-1.5 text-meta text-mut">
          <Sparkles size={12} aria-hidden className="shrink-0 text-blue" />
          <b className="min-w-0 truncate font-semibold text-ink">{hint}</b>
          <button
            type="button"
            onClick={(e) =>
              act(e, async () => {
                const from = meeting.title;
                await rename.mutateAsync({ id: meeting.id, title: withName(from, meeting.started_at, hint) });
                onRenamed(from);
              })
            }
            className="px-0.5 font-bold text-blue hover:underline"
          >
            Use
          </button>
          <button
            type="button"
            aria-label="Dismiss suggestion"
            onClick={(e) => act(e, () => dismissal.mutateAsync(meeting.id))}
            className="grid place-items-center rounded-pill p-0.5 text-mut hover:text-ink"
          >
            <X size={12} aria-hidden />
          </button>
        </span>
      )}
      {error && <p className="mt-1 text-meta text-straw">{error}</p>}
    </Fold>
  );
}
