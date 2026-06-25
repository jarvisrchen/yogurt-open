import { useLayoutEffect, useRef } from "react";
import { reduced } from "../components/Fold";

/**
 * Replays the page rise on the returned ref's element when `key` turns to one the user picked with `pick`, so a tab panel enters the way a page does.
 * A key that changes on its own, like a load correcting a guessed tab or a recap arriving, swaps without it.
 * It runs before paint, so the old content never shows through.
 */
export function useRise<K, T extends HTMLElement = HTMLDivElement>(key: K) {
  const ref = useRef<T>(null);
  const picked = useRef<{ key: K } | null>(null);
  useLayoutEffect(() => {
    const mine = picked.current?.key === key;
    picked.current = null;
    const el = ref.current;
    if (!mine || !el || reduced()) return;
    el.style.animation = "none";
    // The restart's forced layout lays out the new content, which this frame does anyway.
    void el.offsetWidth;
    el.style.animation = "var(--animate-page-rise)";
  }, [key]);
  return { ref, pick: (next: K) => void (picked.current = { key: next }) };
}
