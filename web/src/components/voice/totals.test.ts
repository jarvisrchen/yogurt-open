import { describe, expect, it } from "vitest";
import { allDevicesStats, zeroDay, type VoiceDayRow } from "./totals";

const NOW = new Date(2026, 10, 9, 15, 0).getTime();
const row = (o: Partial<VoiceDayRow> & Pick<VoiceDayRow, "device_id" | "day">): VoiceDayRow => ({
  ...zeroDay(o.day),
  device_name: o.device_id,
  device_kind: "mac",
  count: 1,
  words: 10,
  updated_at: 1,
  ...o,
});

describe("allDevicesStats", () => {
  const rows = [
    row({ device_id: "a", day: "2026-11-09", words: 100, count: 3, audio_ms: 60_000, wait_ms: 900, cleaned_count: 2, usd: 0.02, updated_at: 5 }),
    row({ device_id: "b", day: "2026-11-09", words: 40, snippet_count: 1, snippet_words: 30, too_slow_count: 1, unpriced_count: 2, updated_at: 7 }),
    row({ device_id: "a", day: "2026-10-11", words: 5, updated_at: 9 }),
    row({ device_id: "a", day: "2026-10-10", words: 1_000, usd: 1 }),
    // Delete all on device c sent this day as zeros, so c dictated nothing in either range.
    row({ device_id: "c", day: "2026-11-01", count: 0, words: 0 }),
  ];

  it("sums the last 30 days, charts each day across devices, and names each device that dictated", () => {
    const all = allDevicesStats(rows, "30d", NOW);
    expect(all.stats.chart.unit).toBe("day");
    expect(all.stats.chart.buckets).toHaveLength(30);
    expect(all.stats.chart.buckets[0]).toEqual({ key: "2026-10-11", words: 5, count: 1 });
    expect(all.stats.chart.buckets.at(-1)).toEqual({ key: "2026-11-09", words: 140, count: 4 });
    expect(all.stats).toMatchObject({ count: 5, words: 145, audio_ms: 60_000, wait_ms: 900, first_day: "2026-10-11", cleaned_count: 2, too_slow_count: 1, median_wait_ms: null });
    expect(all.stats.snippets).toEqual({ count: 1, words: 30, audio_ms: 0, wait_ms: 0 });
    expect(all.usd).toBeCloseTo(0.02);
    expect(all.unpriced_count).toBe(2);
    expect(all.devices).toEqual([
      { id: "a", name: "a", words: 105, count: 4, usd: 0.02, cleaned: true, updated_at: 9 },
      { id: "b", name: "b", words: 40, count: 1, usd: 0, cleaned: true, updated_at: 7 },
    ]);
  });

  it("charts all time per week from the first day's Monday", () => {
    const all = allDevicesStats(rows, "all", NOW);
    expect(all.stats.words).toBe(1_145);
    expect(all.stats.first_day).toBe("2026-10-10");
    expect(all.usd).toBeCloseTo(1.02);
    expect(all.stats.chart).toEqual({
      unit: "week",
      buckets: [
        { key: "2026-10-05", words: 1_005, count: 2 },
        ...["2026-10-12", "2026-10-19", "2026-10-26", "2026-11-02"].map((key) => ({ key, words: 0, count: 0 })),
        { key: "2026-11-09", words: 140, count: 4 },
      ],
    });
    expect(all.devices.map((d) => d.id)).toEqual(["a", "b"]);
  });

  it("charts a history under two weeks old by the day", () => {
    const { chart } = allDevicesStats([row({ device_id: "a", day: "2026-11-05" }), row({ device_id: "a", day: "2026-11-09" })], "all", NOW).stats;
    expect(chart.unit).toBe("day");
    expect(chart.buckets.map((b) => b.count)).toEqual([1, 0, 0, 0, 1]);
  });

  it("charts all time per month past a year", () => {
    const { chart } = allDevicesStats([...rows, row({ device_id: "a", day: "2025-01-15", words: 7 })], "all", NOW).stats;
    expect(chart.unit).toBe("month");
    expect(chart.buckets).toHaveLength(23);
    expect(chart.buckets[0]).toEqual({ key: "2025-01", words: 7, count: 1 });
    expect(chart.buckets.at(-1)).toEqual({ key: "2026-11", words: 140, count: 4 });
  });

  it("charts 7 days per day, and today as its one day, since the rows hold no hours", () => {
    const week = allDevicesStats(rows, "7d", NOW);
    expect(week.stats.chart.buckets.map((b) => b.key)).toEqual(["2026-11-03", "2026-11-04", "2026-11-05", "2026-11-06", "2026-11-07", "2026-11-08", "2026-11-09"]);
    expect(week.stats.words).toBe(140);
    const today = allDevicesStats(rows, "today", NOW);
    expect(today.stats.chart).toEqual({ unit: "day", buckets: [{ key: "2026-11-09", words: 140, count: 4 }] });
    expect(today.stats.first_day).toBe("2026-11-09");
  });

  it("is empty with no rows", () => {
    const all = allDevicesStats([], "30d", NOW);
    expect(all.stats.count).toBe(0);
    expect(all.stats.first_day).toBeNull();
    expect(all.devices).toEqual([]);
  });
});
