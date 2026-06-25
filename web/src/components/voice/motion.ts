import { useEffect, useRef, useState } from "react";

const COUNT_MS = 600;
const RISE_ROWS = 8;
export const RISE_STAGGER_MS = 40;
export const BAR_STAGGER_MS = 14;
/** Matches `--animate-grow` in `web/src/index.css`. */
export const GROW_MS = 520;

// Guarded because jsdom has no `matchMedia`.
const reduced = () => typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;

/** `to`, counted up from 0 on mount and from wherever it stands on each change. */
export function useCount(to: number): number {
  const [shown, setShown] = useState(0);
  const at = useRef(shown);
  useEffect(() => {
    if (reduced()) {
      at.current = to;
      setShown(to);
      return;
    }
    const from = at.current;
    const t0 = performance.now();
    let frame = 0;
    const step = (t: number) => {
      const p = Math.min(1, (t - t0) / COUNT_MS);
      at.current = from + (to - from) * (1 - (1 - p) ** 3);
      setShown(at.current);
      if (p < 1) frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
    return () => cancelAnimationFrame(frame);
  }, [to]);
  // The state catches up only after the effect, so a change would show the old value for a frame.
  return reduced() ? to : shown;
}

/**
 * Each of the first eight ids the list showed, by its place, for a rise in on first show.
 * Rows that come later, from a new dictation or Show more, are not in it, so they just appear.
 */
export function useRiseOrder(ids: string[]): ReadonlyMap<string, number> {
  const [order, setOrder] = useState<Map<string, number> | null>(null);
  if (!order && ids.length) setOrder(new Map(ids.slice(0, RISE_ROWS).map((id, i) => [id, i])));
  return order ?? NONE;
}
const NONE: ReadonlyMap<string, number> = new Map();
