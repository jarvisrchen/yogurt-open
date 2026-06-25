import { LABEL } from "../settings/labelClass";
import { Shimmer } from "../Shimmer";

const TITLE_WIDTHS = ["46%", "58%", "38%", "52%", "44%", "61%", "40%", "55%"];

export function LibrarySkeleton({ rows }: { rows: number }) {
  return (
    <div className="flex flex-col gap-8" aria-hidden data-testid="library-skeleton">
      <section>
        <h2 className={`${LABEL} mb-2`}>
          <Shimmer className="h-2.25 w-16" inline />
        </h2>
        <ul className="flex flex-col gap-2">
          {Array.from({ length: rows }, (_, i) => (
            <li key={i}>
              <div className="flex items-start justify-between gap-4 px-4.5 py-4 bg-card border border-line rounded-card shadow-card">
                {/* The hidden select checkbox's slot. */}
                <div className="shrink-0 w-4.5 h-5.5" />
                <div className="flex-1 min-w-0">
                  <div className="text-lead h-[1.5em] flex items-center">
                    <Shimmer className="h-3.25" width={TITLE_WIDTHS[i % TITLE_WIDTHS.length]} />
                  </div>
                  <div className="mt-1 h-4.5 flex items-center">
                    <Shimmer className="h-2.25 w-[34%]" />
                  </div>
                </div>
                <div className="flex items-center gap-1.5 shrink-0 h-5.5">
                  <Shimmer className="h-5 w-23" radius="rounded-pill" />
                  <Shimmer className="h-5 w-19.5" radius="rounded-pill" />
                  <div className="w-7" />
                </div>
              </div>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
