import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { useCount, useRiseOrder } from "./motion";

const setReduced = (reduce: boolean) =>
  (window.matchMedia = vi.fn().mockReturnValue({ matches: reduce }) as unknown as typeof window.matchMedia);
const after = (ms: number) => act(() => void vi.advanceTimersByTime(ms));

describe("useCount", () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame", "performance"] });
    setReduced(false);
  });
  afterEach(() => vi.useRealTimers());

  it("counts up from 0 on mount, then from where it stands to a new value", () => {
    const { result, rerender } = renderHook(({ to }) => useCount(to), { initialProps: { to: 1000 } });
    expect(result.current).toBe(0);
    after(300);
    expect(result.current).toBeGreaterThan(500);
    expect(result.current).toBeLessThan(1000);
    after(320);
    expect(result.current).toBe(1000);

    rerender({ to: 400 });
    after(50);
    expect(result.current).toBeLessThan(1000);
    expect(result.current).toBeGreaterThan(400);
    after(600);
    expect(result.current).toBe(400);
  });

  it("shows the value at once under reduced motion", () => {
    setReduced(true);
    const { result, rerender } = renderHook(({ to }) => useCount(to), { initialProps: { to: 1000 } });
    expect(result.current).toBe(1000);
    rerender({ to: 400 });
    expect(result.current).toBe(400);
  });
});

describe("useRiseOrder", () => {
  it("keeps the first eight ids it was given, so later rows never rise", () => {
    const ids = (n: number, from = 0) => Array.from({ length: n }, (_, i) => `d${from + i}`);
    const { result, rerender } = renderHook(({ list }) => useRiseOrder(list), { initialProps: { list: [] as string[] } });
    expect(result.current.size).toBe(0);
    rerender({ list: ids(12) });
    expect([...result.current]).toEqual(ids(8).map((id, i) => [id, i]));
    rerender({ list: ["new", ...ids(20)] });
    expect(result.current.get("new")).toBeUndefined();
    expect(result.current.get("d0")).toBe(0);
  });
});
