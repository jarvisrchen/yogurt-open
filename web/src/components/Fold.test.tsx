import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";
import { Fold } from "./Fold";

const fold = () => screen.getByText("row").parentElement!;
const setReduced = (reduce: boolean) =>
  (window.matchMedia = vi.fn().mockReturnValue({ matches: reduce }) as unknown as typeof window.matchMedia);
const after = (ms: number) => act(() => void vi.advanceTimersByTime(ms));

describe("Fold", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    setReduced(false);
  });
  afterEach(() => vi.useRealTimers());

  it("opens a row added with appear, clipping it only until the fold ends", () => {
    render(<Fold open appear>row</Fold>);
    expect(fold()).not.toHaveAttribute("data-folded");
    expect(fold()).toHaveAttribute("data-folding");
    after(220);
    expect(fold()).not.toHaveAttribute("data-folding");
  });

  it("folds shut, then unmounts and reports it once the fold ends", () => {
    const onClosed = vi.fn();
    const { rerender } = render(<Fold open onClosed={onClosed}>row</Fold>);
    rerender(<Fold open={false} onClosed={onClosed}>row</Fold>);
    expect(fold()).toHaveAttribute("data-folded");
    after(219);
    expect(onClosed).not.toHaveBeenCalled();
    after(1);
    expect(screen.queryByText("row")).toBeNull();
    expect(onClosed).toHaveBeenCalledTimes(1);
  });

  it("stays mounted when reopened before the fold ends", () => {
    const onClosed = vi.fn();
    const { rerender } = render(<Fold open onClosed={onClosed}>row</Fold>);
    rerender(<Fold open={false} onClosed={onClosed}>row</Fold>);
    after(100);
    rerender(<Fold open onClosed={onClosed}>row</Fold>);
    after(500);
    expect(fold()).not.toHaveAttribute("data-folded");
    expect(onClosed).not.toHaveBeenCalled();
  });

  it("unmounts with no wait under reduced motion", () => {
    setReduced(true);
    const onClosed = vi.fn();
    const { rerender } = render(<Fold open onClosed={onClosed}>row</Fold>);
    rerender(<Fold open={false} onClosed={onClosed}>row</Fold>);
    after(0);
    expect(screen.queryByText("row")).toBeNull();
    expect(onClosed).toHaveBeenCalledTimes(1);
  });

  it("renders nothing and reports nothing when mounted shut", () => {
    const onClosed = vi.fn();
    render(<Fold open={false} onClosed={onClosed}>row</Fold>);
    after(500);
    expect(screen.queryByText("row")).toBeNull();
    expect(onClosed).not.toHaveBeenCalled();
  });
});
