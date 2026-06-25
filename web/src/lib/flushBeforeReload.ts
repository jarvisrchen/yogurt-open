import { useEffect } from "react";

const flushers = new Set<() => Promise<void>>();

/** A reload skips React's unmount effects, so a page with unsaved edits registers how to save them first. */
export function useFlushBeforeReload(flush: () => Promise<void>) {
  useEffect(() => {
    flushers.add(flush);
    return () => {
      flushers.delete(flush);
    };
  }, [flush]);
}

/** Saves what every mounted page still holds, for an action that reads it back from the server. */
export async function flushPending() {
  await Promise.allSettled([...flushers].map((f) => f()));
}

export async function flushThenReload() {
  await flushPending();
  window.location.reload();
}
