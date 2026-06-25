import { Shimmer } from "./Shimmer";

// Sized in em, so it follows the text size of the page it stands in for.
const SECTIONS = [
  { heading: "38%", lines: ["72%", "58%", "66%"] },
  { heading: "30%", lines: ["61%", "47%"] },
];

export function NotesSkeleton({ className = "" }: { className?: string }) {
  return (
    <div role="status" aria-label="Loading notes" data-testid="notes-skeleton" className={className}>
      {SECTIONS.map((s, i) => (
        <div key={i} className={i ? "mt-[1em]" : undefined}>
          <div className="h-[1.6em] flex items-center mb-[0.35em]">
            <Shimmer className="h-[0.8em]" width={s.heading} />
          </div>
          {s.lines.map((w, j) => (
            <div key={j} className="h-[1.5em] my-[0.2em] flex items-center gap-[0.6em] pl-[0.5em]">
              <span aria-hidden className="w-[0.3em] h-[0.3em] rounded-pill bg-line shrink-0" />
              <Shimmer className="h-[0.6em]" width={w} />
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}
