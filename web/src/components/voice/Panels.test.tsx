import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import type { ModelCost, VoiceStats } from "../../lib/api/voiceTypes";
import { CleanupPanel, Tiles } from "./Panels";
import type { AllDevices } from "./totals";

const stats: VoiceStats = {
  count: 3,
  hands_free_count: 0,
  words: 12_480,
  audio_ms: 120_000,
  wait_ms: 1_200,
  first_day: "2026-08-12",
  chart: { unit: "day", buckets: [] },
  apps: [],
  cleaned_count: 0,
  cleaned_words: 0,
  fillers_removed: 0,
  too_slow_count: 0,
  median_wait_ms: null,
  cleanup_model: null,
  costs: [],
  snippets: { count: 0, words: 0, audio_ms: 0, wait_ms: 0 },
  history: { count: 3, hands_free_count: 0, apps: [], stt_avg_ms: {}, cleanup_avg_ms: {} },
};

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame", "performance"] });
  window.matchMedia = vi.fn().mockReturnValue({ matches: false }) as unknown as typeof window.matchMedia;
});
afterEach(() => vi.useRealTimers());

it("counts a tile on a hidden layer over the final value, which keeps the width and is what a screen reader reads", () => {
  render(<Tiles s={stats} range="all" cleanup />);
  const final = screen.getByText("12,480");
  expect(final).toHaveClass("text-transparent");
  expect(final.nextElementSibling).toHaveAttribute("aria-hidden");
  expect(final.nextElementSibling).toHaveTextContent(/^0$/);

  act(() => void vi.advanceTimersByTime(700));
  expect(screen.getByText("12,480")).not.toHaveClass("text-transparent");
  expect(screen.getByText("12,480").nextElementSibling).toBeNull();
});

const cost = (model: string, usd: number, unpriced_count = 0): ModelCost => ({ model, count: 4, input_tokens: 2_000, output_tokens: 100, usd, unpriced_count });
const costTile = () => screen.getByText("Estimated cost").parentElement!;

it("reads the cost tile as none, a total, or not priced", () => {
  render(<Tiles s={stats} range="30d" cleanup />);
  expect(costTile()).toHaveTextContent(/^Estimated costNonecleanup, at list price$/);
  expect(screen.getByText("None")).toHaveClass("text-mut");
  cleanup();
  render(<Tiles s={stats} range="30d" cleanup={false} />);
  expect(costTile()).toHaveTextContent(/^Estimated costOffcleanup, at list price$/);
  cleanup();
  render(<Tiles s={{ ...stats, costs: [cost("gemini-3.5-flash-lite", 0.0042), cost("deepseek-v4-flash", 0.31)] }} range="30d" cleanup />);
  expect(costTile()).toHaveTextContent("≈ $0.31");
  cleanup();
  render(<Tiles s={{ ...stats, costs: [cost("gemini-3.5-flash-lite", 0.0042)] }} range="30d" cleanup />);
  expect(costTile()).toHaveTextContent("≈ $0.004");
  cleanup();
  render(<Tiles s={{ ...stats, costs: [cost("llama3.2:3b", 0, 4)] }} range="30d" cleanup />);
  expect(screen.getByText("Not priced")).toHaveClass("text-mut");
});

it("reads All devices' cost tile from the devices' own dollars", () => {
  const all = (usd: number, unpriced_count: number): AllDevices => ({ stats, devices: [], usd, unpriced_count });
  render(<Tiles s={stats} range="30d" all={all(0.31, 0)} cleanup />);
  expect(costTile()).toHaveTextContent("≈ $0.31");
  cleanup();
  render(<Tiles s={stats} range="30d" all={all(0, 2)} cleanup />);
  expect(costTile()).toHaveTextContent("Not priced");
  cleanup();
  // Cleanup on a model that costs nothing is a cost of $0, not None.
  render(<Tiles s={{ ...stats, cleaned_count: 4 }} range="30d" all={{ ...all(0, 0), stats: { ...stats, cleaned_count: 4 } }} cleanup />);
  expect(costTile()).toHaveTextContent("$0");
  expect(costTile()).not.toHaveTextContent("None");
});

describe("Cleanup panel's cost row", () => {
  const cleaned = { ...stats, cleaned_count: 4, cleaned_words: 20 };
  const row = () => screen.queryByText(/^Cost/);

  it("is left out with one priced model, since the tile has the total", () => {
    render(<CleanupPanel s={{ ...cleaned, costs: [cost("gemini-3.5-flash-lite", 0.03)] }} on turnOn={null} />);
    expect(row()).toBeNull();
    expect(screen.queryByText(/\$0\.03/)).toBeNull();
  });

  it("splits several models without repeating the total", () => {
    render(<CleanupPanel s={{ ...cleaned, costs: [cost("gemini-3.5-flash-lite", 0.03), cost("deepseek-v4-flash", 0.02)] }} on turnOn={null} />);
    expect(row()).toHaveTextContent(/^Cost by modelgemini-3.5-flash-lite ≈ \$0\.030 · deepseek-v4-flash ≈ \$0\.020$/);
    expect(screen.queryByText("≈ $0.05")).toBeNull();
  });

  it("names what was not priced, and is left out with cleanup off", () => {
    render(<CleanupPanel s={{ ...cleaned, costs: [cost("llama3.2:3b", 0, 4)] }} on turnOn={null} />);
    expect(row()).toHaveTextContent("Cost by modelllama3.2:3b 2,100 tokens · 4 dictations not priced");
    cleanup();
    render(<CleanupPanel s={stats} on={false} turnOn={null} />);
    expect(row()).toBeNull();
  });
});
