import { useLayoutEffect, useRef } from "react";

interface Options {
  /**
   * Set on a switch that several screens each render, so it remounts on a pick.
   * A copy that mounts within a second of another with the same key unmounting starts on the option that one had picked and slides from there.
   */
  remember?: string;
  /** "y" sizes the thumb by `height` and slides it down a column, like a sidebar's active-row pill. */
  axis?: "x" | "y";
}

// Options are `button` and `a` children, which the ResizeObserver below also watches.
const OPTIONS = "button, a";
const lastPick = new Map<string, { index: number; at: number }>();
const HANDOFF_MS = 1000;

/**
 * Slides one thumb, sized by its `width` and `transform`, under the child of `group` that `selector` picks.
 * It moves without sliding on first paint, a resize, a font load, or a change of `labels` alone, so it never sweeps in from the left edge.
 * The thumb carries `data-sliding` from a slide until its next placement that does not slide, which index.css reads to keep a view transition from doubling it.
 * A group that is not rendered, like a tab row with one tab, is skipped until `labels` changes.
 */
export function useSlidingThumb<T extends string>(selector: string, value: T, labels: string, { remember, axis = "x" }: Options = {}) {
  const group = useRef<HTMLDivElement>(null);
  const thumb = useRef<HTMLSpanElement>(null);
  const placed = useRef<T | null>(null);
  const target = useRef("");

  const place = (slide: boolean, picked = group.current?.querySelector<HTMLElement>(selector)) => {
    const t = thumb.current;
    const g = group.current;
    if (!t || !g) return;
    // A thumb hidden while no option matched would slide in from wherever it last sat.
    if (t.hidden) slide = false;
    t.hidden = !picked;
    if (!picked) {
      delete t.dataset.sliding;
      return;
    }
    // Rects keep a half-pixel button's size, which `offsetWidth` rounds.
    // Dividing by the group's scale keeps a scaling ancestor, like a dialog popping in, from skewing them.
    const box = g.getBoundingClientRect();
    const rect = picked.getBoundingClientRect();
    const y = axis === "y";
    const scale = Math.abs(box.width - g.offsetWidth) < 1 ? 1 : box.width / g.offsetWidth;
    const size = `${(y ? rect.height : rect.width) / scale}px`;
    const transform = y ? `translateY(${(rect.top - box.top) / scale}px)` : `translateX(${(rect.left - box.left) / scale}px)`;
    // Already headed there: leave a running slide alone, as StrictMode's second effect pass or a resize would cut it.
    // Compared with what was set, since the style reads back rounded.
    if (target.current === size + transform) return;
    target.current = size + transform;
    if (slide) t.dataset.sliding = "";
    else {
      delete t.dataset.sliding;
      t.style.transition = "none";
    }
    t.style[y ? "height" : "width"] = size;
    t.style.transform = transform;
    if (!slide) {
      void t.offsetWidth;
      t.style.transition = "";
    }
  };

  useLayoutEffect(() => {
    if (!group.current) {
      placed.current = null;
      target.current = "";
      return;
    }
    let moved = placed.current !== null && placed.current !== value;
    const last = placed.current === null && remember ? lastPick.get(remember) : undefined;
    const start = last && performance.now() - last.at < HANDOFF_MS ? group.current.querySelectorAll<HTMLElement>(OPTIONS)[last.index] : undefined;
    if (start) {
      place(false, start);
      moved = true;
    }
    place(moved);
    placed.current = value;
  }, [value, labels]);

  // `place` reads only refs and the DOM, so the observer may keep the first render's copy.
  useLayoutEffect(() => {
    const g = group.current;
    if (!g) return;
    const ro = new ResizeObserver(() => place(false));
    for (const el of [g, ...g.querySelectorAll(OPTIONS)]) ro.observe(el);
    return () => ro.disconnect();
  }, [labels]);

  useLayoutEffect(() => {
    const g = group.current;
    if (!remember || !g) return;
    return () => {
      const index = [...g.querySelectorAll(OPTIONS)].findIndex((el) => el.matches(selector));
      if (index >= 0) lastPick.set(remember, { index, at: performance.now() });
      else lastPick.delete(remember);
    };
  }, [remember, selector]);

  return { group, thumb };
}
