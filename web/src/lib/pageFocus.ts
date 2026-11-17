/** Whether this page counts as in use for the meeting bubble, which floats over other apps only while no yogurt page does. */
function focused(): boolean {
  return document.hasFocus() && document.visibilityState === "visible";
}

/** Sends `page_focus` on `ws` now and on every change, until the returned cleanup runs. */
export function reportFocus(ws: WebSocket): () => void {
  let sent: boolean | null = null;
  const send = () => {
    const now = focused();
    if (now === sent || ws.readyState !== WebSocket.OPEN) return;
    sent = now;
    ws.send(JSON.stringify({ type: "page_focus", focused: now }));
  };
  send();
  window.addEventListener("focus", send);
  window.addEventListener("blur", send);
  document.addEventListener("visibilitychange", send);
  return () => {
    window.removeEventListener("focus", send);
    window.removeEventListener("blur", send);
    document.removeEventListener("visibilitychange", send);
  };
}

const FLASH_MS = 900;
const FLASH_FOR_MS = 10_000;

let stopFlash: (() => void) | null = null;

/**
 * The bubble brought this page's app forward, but a browser can only be raised, not switched to a tab, so a tab in the background alternates its title to be found.
 * Stops once the page has focus or after ten seconds.
 */
export function flashTitle(): void {
  if (focused() || stopFlash) return;
  const title = document.title;
  const started = Date.now();
  const id = window.setInterval(() => {
    if (focused() || Date.now() - started >= FLASH_FOR_MS) return stopFlash?.();
    document.title = document.title === title ? "● Back to yogurt" : title;
  }, FLASH_MS);
  stopFlash = () => {
    window.clearInterval(id);
    document.title = title;
    stopFlash = null;
  };
}
