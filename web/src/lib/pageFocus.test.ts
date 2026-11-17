import { afterEach, describe, expect, it, vi } from "vitest";
import { flashTitle, reportFocus } from "./pageFocus";

function fakeSocket() {
  return { readyState: WebSocket.OPEN, send: vi.fn() } as unknown as WebSocket & {
    send: ReturnType<typeof vi.fn>;
  };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("reportFocus", () => {
  it("sends the page's focus on start and on each change, and stops after cleanup", () => {
    const hasFocus = vi.spyOn(document, "hasFocus").mockReturnValue(true);
    const ws = fakeSocket();
    const stop = reportFocus(ws);
    expect(ws.send).toHaveBeenLastCalledWith('{"type":"page_focus","focused":true}');

    window.dispatchEvent(new Event("focus"));
    expect(ws.send).toHaveBeenCalledTimes(1);

    hasFocus.mockReturnValue(false);
    window.dispatchEvent(new Event("blur"));
    expect(ws.send).toHaveBeenLastCalledWith('{"type":"page_focus","focused":false}');

    stop();
    hasFocus.mockReturnValue(true);
    window.dispatchEvent(new Event("focus"));
    expect(ws.send).toHaveBeenCalledTimes(2);
  });
});

describe("flashTitle", () => {
  it("alternates the title while the page is in the background and restores it on focus", () => {
    vi.useFakeTimers();
    const hasFocus = vi.spyOn(document, "hasFocus").mockReturnValue(false);
    document.title = "Weekly sync - yogurt";
    flashTitle();
    vi.advanceTimersByTime(900);
    expect(document.title).toBe("● Back to yogurt");
    vi.advanceTimersByTime(900);
    expect(document.title).toBe("Weekly sync - yogurt");
    vi.advanceTimersByTime(900);
    hasFocus.mockReturnValue(true);
    vi.advanceTimersByTime(900);
    expect(document.title).toBe("Weekly sync - yogurt");
  });

  it("does nothing on a page that already has focus", () => {
    vi.useFakeTimers();
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    document.title = "Library - yogurt";
    flashTitle();
    vi.advanceTimersByTime(5_000);
    expect(document.title).toBe("Library - yogurt");
  });
});
