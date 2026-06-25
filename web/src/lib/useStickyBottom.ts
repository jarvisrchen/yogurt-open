import { useEffect, useRef } from "react";

/** Sticks a scroll container to the bottom as `deps` change, until the user scrolls more than 24px up. */
export function useStickyBottom(deps: unknown[]) {
  const ref = useRef<HTMLDivElement | null>(null);
  const sticky = useRef(true);
  useEffect(() => {
    if (sticky.current && ref.current) ref.current.scrollTop = ref.current.scrollHeight;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  const onScroll = (e: React.UIEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    sticky.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  };
  return { ref, sticky, onScroll };
}
