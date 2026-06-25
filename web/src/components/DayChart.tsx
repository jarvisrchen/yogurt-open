import { useState } from "react";
import { BAR_STAGGER_MS, GROW_MS } from "./voice/motion";
import { LABEL } from "./settings/labelClass";

export interface ChartBar {
  key: string;
  value: number;
  /** The whole tooltip, which is also the bar's accessible name. */
  tip: string;
}

/**
 * A card of bars under a title. `caption` sits on the title's right.
 * `step` is one of the three gridlines, in the units of `value`.
 */
export function DayChart({ title, caption = "Last 30 days", ...chart }: { title: string; caption?: string } & BarChartProps) {
  return (
    <div className="rounded-card border border-line bg-card shadow-card p-5 mt-3">
      <div className="flex items-baseline justify-between gap-3">
        <h2 className="heading-sm">{title}</h2>
        <p className="text-meta text-mut">{caption}</p>
      </div>
      <BarChart {...chart} />
    </div>
  );
}

/** The chart without its card, under a small caps label, for a chart inside another panel. */
export function InlineChart({ title, ...chart }: { title: string } & BarChartProps) {
  return (
    <>
      <p className={LABEL}>{title}</p>
      <BarChart {...chart} />
    </>
  );
}

interface BarChartProps {
  bars: ChartBar[];
  step: number;
  format: (value: number) => string;
  /** One label per bar, centred under it, or fewer spread from the first bar to the last. */
  ticks: string[];
  /** Room left of the plot for the gridline labels, in px. */
  gutter?: number;
  height?: number;
}

// Bars stop growing at 56px and spread out, so a range of 7 does not turn into blocks.
const COLUMN = "flex-1 max-w-14";

function BarChart({ bars, step, format, ticks, gutter = 36, height = 150 }: BarChartProps) {
  const [hover, setHover] = useState<number | null>(null);
  const [mountedAt] = useState(() => performance.now());
  const top = step * 3;
  const peak = bars.reduce((best, d, i) => (d.value > (bars[best]?.value ?? 0) ? i : best), -1);
  const pct = (value: number) => `${(value / top) * 100}%`;
  const cells = ticks.length === bars.length;

  return (
    <>
      <div className="relative mt-4" style={{ height, paddingLeft: gutter }}>
        {[1, 2, 3].map((k) => (
          <div key={k} className="absolute right-0 border-t border-line" style={{ left: gutter, bottom: pct(step * k) }}>
            <span className="absolute right-[calc(100%+6px)] -top-1.75 whitespace-nowrap text-caption text-mut tabular-nums">
              {format(step * k)}
            </span>
          </div>
        ))}
        <div
          className="absolute right-0 inset-y-0 flex justify-around gap-0.75 items-end border-b border-grey"
          style={{ left: gutter }}
          onMouseLeave={() => setHover(null)}
        >
          {bars.map((d, i) => (
            <div
              key={d.key}
              role="img"
              aria-label={d.tip}
              className={`group relative ${COLUMN} h-full flex items-end`}
              onMouseEnter={() => setHover(i)}
            >
              {i === peak && (
                <PeakLabel
                  bottom={`calc(${pct(d.value)} + 4px)`}
                  wait={Math.max(GROW_MS, i * BAR_STAGGER_MS + GROW_MS - (performance.now() - mountedAt))}
                >
                  {format(d.value)}
                </PeakLabel>
              )}
              {/* A bar with nothing keeps no height, so it grows from the baseline when one lands. */}
              <i
                className="block w-full rounded-t-[4px] bg-blue group-hover:brightness-115 origin-bottom motion-safe:animate-grow motion-safe:transition-[height] motion-safe:duration-520 motion-safe:ease-(--ease-slide)"
                style={{ height: pct(d.value), animationDelay: `${i * BAR_STAGGER_MS}ms` }}
              />
              {hover === i && (
                <div
                  role="tooltip"
                  className="absolute z-10 left-1/2 pointer-events-none -translate-x-1/2 whitespace-nowrap rounded-[8px] bg-ink text-paper px-2.25 py-1.5 text-meta font-medium leading-[1.4] shadow-[0_8px_18px_-6px_rgba(0,0,0,0.35)]"
                  style={{ bottom: `calc(${pct(d.value)} + 22px)` }}
                >
                  {d.tip}
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
      <div
        className={`flex ${cells ? "justify-around gap-0.75" : "justify-between"} mt-1.5 text-caption text-mut tabular-nums`}
        style={{ paddingLeft: gutter }}
      >
        {ticks.map((t, i) => (
          <span key={i} className={cells ? `${COLUMN} text-center whitespace-nowrap` : undefined}>
            {t}
          </span>
        ))}
      </div>
    </>
  );
}

/**
 * Rises in once its bar has grown: on first show after the bar's place in the stagger, and on a moved peak after the bars resize.
 * `wait` is kept from mount, since a changed `animation-delay` would move the rise while it waits.
 */
function PeakLabel({ bottom, wait, children }: { bottom: string; wait: number; children: string }) {
  const [delay] = useState(wait);
  return (
    <span
      className="absolute left-1/2 -translate-x-1/2 text-caption text-mut tabular-nums whitespace-nowrap motion-safe:animate-rise motion-safe:transition-[bottom] motion-safe:duration-520 motion-safe:ease-(--ease-slide)"
      style={{ bottom, animationDelay: `${delay}ms` }}
    >
      {children}
    </span>
  );
}
