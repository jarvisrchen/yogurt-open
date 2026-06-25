import { describe, expect, it } from "vitest";
import { bucketName, chartTicks, visibleBuckets } from "./statsChart";

const hours = (busy: number[]) => Array.from({ length: 24 }, (_, h) => ({ key: String(h).padStart(2, "0"), n: busy.includes(h) ? 1 : 0 }));
const span = (bs: { key: string }[]) => [bs[0].key, bs.at(-1)!.key];

describe("visibleBuckets", () => {
  it("shows 8 AM to 6 PM, widened to any busy hour", () => {
    expect(span(visibleBuckets("hour", hours([10]), (b) => b.n > 0))).toEqual(["08", "17"]);
    expect(span(visibleBuckets("hour", hours([6, 20]), (b) => b.n > 0))).toEqual(["06", "20"]);
  });

  it("keeps every bucket of a longer unit", () => {
    const days = [{ key: "2026-11-12" }, { key: "2026-11-13" }];
    expect(visibleBuckets("day", days, () => false)).toBe(days);
  });
});

describe("chartTicks", () => {
  it("names every second hour and each of 7 days", () => {
    expect(chartTicks("hour", ["08", "09", "10"])).toEqual(["8 AM", "", "10 AM"]);
    const week = Array.from({ length: 7 }, (_, i) => `2026-11-${String(i + 7).padStart(2, "0")}`);
    expect(chartTicks("day", week)).toEqual(["Sat", "Sun", "Mon", "Tue", "Wed", "Thu", "Today"]);
  });

  it("gives a few weeks a slot each and many days three labels", () => {
    expect(chartTicks("week", ["2026-08-03", "2026-08-10", "2026-08-17", "2026-08-24", "2026-08-31"])).toEqual(["Aug 3", "", "Aug 17", "", "This week"]);
    const month = Array.from({ length: 30 }, (_, i) => `2026-10-${String(i + 2).padStart(2, "0")}`);
    expect(chartTicks("day", month)).toEqual(["Fri, Oct 2", "Sat, Oct 17", "Today"]);
  });
});

describe("bucketName", () => {
  it("names each unit for the tooltip", () => {
    expect(bucketName("hour", "23")).toBe("11 PM to 12 AM");
    expect(bucketName("week", "2026-11-09")).toBe("Week of Nov 9");
    expect(bucketName("month", "2026-10")).toBe("Oct 2026");
  });
});
