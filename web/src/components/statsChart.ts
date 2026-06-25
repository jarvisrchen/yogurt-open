import { monthDay, parseDay, shortDay } from "./voice/format";

export type ChartUnit = "hour" | "day" | "week" | "month";

export const hourName = (h: number) => new Date(2000, 0, 1, h).toLocaleTimeString("en-US", { hour: "numeric" });

/** Today's hours from 8 AM to 6 PM, widened to reach any hour `busy` says has something. Other units keep every bucket. */
export function visibleBuckets<B extends { key: string }>(unit: ChartUnit, buckets: B[], busy: (b: B) => boolean): B[] {
  if (unit !== "hour") return buckets;
  const hours = buckets.filter(busy).map((b) => Number(b.key));
  const from = Math.min(8, ...hours);
  const to = Math.max(18, ...hours.map((h) => h + 1));
  return buckets.filter((b) => Number(b.key) >= from && Number(b.key) < to);
}

const month = (key: string) => {
  const [y, m] = key.split("-").map(Number);
  return new Date(y, m - 1, 1).toLocaleDateString("en-US", { month: "short", year: "numeric" });
};

/** The bucket as a tooltip names it. */
export function bucketName(unit: ChartUnit, key: string): string {
  if (unit === "hour") return `${hourName(Number(key))} to ${hourName(Number(key) + 1)}`;
  if (unit === "day") return shortDay(parseDay(key));
  if (unit === "week") return `Week of ${monthDay(parseDay(key))}`;
  return month(key);
}

/** One tick per bar for hours (every second one named) and for up to 7 days, otherwise the first, middle, and last. */
export function chartTicks(unit: ChartUnit, keys: string[]): string[] {
  if (unit === "hour") return keys.map((k, i) => (i % 2 ? "" : hourName(Number(k))));
  const now = { day: "Today", week: "This week", month: "This month" }[unit];
  const name = (k: string) => (unit === "day" ? shortDay(parseDay(k)) : unit === "week" ? monthDay(parseDay(k)) : month(k));
  if (unit === "day" && keys.length <= 7)
    return keys.map((k, i) => (i === keys.length - 1 ? now : parseDay(k).toLocaleDateString("en-US", { weekday: "short" })));
  // Few enough bars to reach their width cap sit apart, so each label needs its own bar's slot to stay under it.
  if (keys.length <= 14) {
    const mid = Math.floor(keys.length / 2);
    return keys.map((k, i) => (i === keys.length - 1 ? now : i === 0 || i === mid ? name(k) : ""));
  }
  return [name(keys[0]), name(keys[Math.floor(keys.length / 2)]), now];
}
