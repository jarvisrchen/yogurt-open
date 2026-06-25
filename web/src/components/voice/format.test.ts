import { describe, expect, it } from "vitest";
import { appShares, clipLength, costRow, gridStep, pace, savedLabel, savedMs, sinceLabel, usd } from "./format";

describe("voice formatting", () => {
  it("writes clip lengths in seconds, then minutes", () => {
    expect(clipLength(200)).toBe("1 s");
    expect(clipLength(9_400)).toBe("9 s");
    expect(clipLength(91_000)).toBe("1 min 31 s");
    expect(clipLength(120_000)).toBe("2 min");
  });

  it("names each range for the first tile", () => {
    expect(sinceLabel("2026-08-12", "today")).toBe("today");
    expect(sinceLabel("2026-08-12", "7d")).toBe("in the last 7 days");
    expect(sinceLabel("2026-08-12", "30d")).toBe("in the last 30 days");
    expect(sinceLabel("2020-08-12", "all")).toBe("since Aug 12, 2020");
    expect(sinceLabel(null, "7d")).toBe("none yet");
  });

  it("computes pace and time saved against typing", () => {
    const none = { count: 0, words: 0, audio_ms: 0, wait_ms: 0 };
    expect(pace({ words: 141, audio_ms: 60_000, wait_ms: 0, snippets: none })).toBe(141);
    expect(pace({ words: 10, audio_ms: 0, wait_ms: 0, snippets: none })).toBe(0);
    // 400 words take 10 min to type, less 3 min talking and 1 min waiting.
    expect(savedMs({ words: 400, audio_ms: 180_000, wait_ms: 60_000, snippets: none })).toBe(360_000);
    expect(savedMs({ words: 1, audio_ms: 60_000, wait_ms: 0, snippets: none })).toBe(0);
    // A 2 s cue that pasted 200 words leaves both as they were without it.
    const snippets = { count: 1, words: 200, audio_ms: 2_000, wait_ms: 300 };
    expect(pace({ words: 341, audio_ms: 62_000, wait_ms: 0, snippets })).toBe(141);
    expect(savedMs({ words: 600, audio_ms: 182_000, wait_ms: 60_300, snippets })).toBe(360_000);
    expect(savedLabel(360_000)).toBe("6 min");
    expect(savedLabel(50_760_000)).toBe("14.1 h");
    // A count keeps the unit of the value it counts toward.
    expect(savedLabel(1_800_000, 7_200_000)).toBe("0.5 h");
    expect(savedLabel(7_200_000, 1_800_000)).toBe("120 min");
  });

  it("writes dollars in cents, or one digit below a cent", () => {
    expect(usd(0)).toBe("$0");
    expect(usd(0.05)).toBe("$0.05");
    expect(usd(12.3)).toBe("$12.30");
    expect(usd(0.0034)).toBe("$0.003");
    expect(usd(0.00012)).toBe("$0.0001");
    expect(usd(0.0099)).toBe("$0.01");
    expect(usd(0.00096)).toBe("$0.001");
  });

  it("lists each model's cost when there are several, and what could not be priced", () => {
    const cost = (model: string, usd: number, count = 10, unpriced_count = 0) => ({
      model,
      count,
      input_tokens: 6_000,
      output_tokens: 400,
      usd,
      unpriced_count,
    });
    expect(costRow([])).toEqual({ usd: null, unpriced: 0, parts: [] });
    expect(costRow([cost("gemini-3.5-flash-lite", 0.03)])).toEqual({ usd: 0.03, unpriced: 0, parts: [] });
    expect(costRow([cost("qwen3-4b-instruct", 0)])).toEqual({ usd: 0, unpriced: 0, parts: [] });
    expect(costRow([cost("gemini-3.5-flash-lite", 0.03), cost("deepseek-v4-flash", 0.02)])).toEqual({
      usd: 0.05,
      unpriced: 0,
      parts: ["gemini-3.5-flash-lite ≈ $0.030", "deepseek-v4-flash ≈ $0.020"],
    });
    expect(costRow([cost("gemini-3.5-flash-lite", 0.03), cost("llama3.2:3b", 0, 3, 3)])).toEqual({
      usd: 0.03,
      unpriced: 3,
      parts: ["gemini-3.5-flash-lite ≈ $0.030", "llama3.2:3b 6,400 tokens", "3 dictations not priced"],
    });
    expect(costRow([cost("llama3.2:3b", 0, 1, 1)])).toEqual({ usd: null, unpriced: 1, parts: ["llama3.2:3b 6,400 tokens", "1 dictation not priced"] });
  });

  it("picks round gridlines that clear the tallest bar", () => {
    expect(gridStep(1_020)).toBe(400);
    expect(gridStep(1_200)).toBe(400);
    expect(gridStep(1_201)).toBe(500);
    expect(gridStep(0)).toBe(10);
  });

  it("keeps five apps and folds the rest into Other", () => {
    const apps = ["A", "B", "C", "D", "E", "F", "G"].map((name, i) => ({ name, words: 70 - i * 10, count: 1 }));
    const shares = appShares(apps);
    expect(shares.map((a) => a.name)).toEqual(["A", "B", "C", "D", "E", "Other"]);
    expect(shares[5]).toEqual({ name: "Other", words: 30, pct: 11 });
    expect(appShares([])).toEqual([]);
  });
});
