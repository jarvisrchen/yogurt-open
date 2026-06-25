import type { StatsRange, VoiceStats } from "../../lib/api/voiceTypes";
import { DayChart } from "../DayChart";
import { formatCount } from "../library/pagination";
import { bucketName, chartTicks, visibleBuckets } from "../statsChart";
import { gridStep } from "./format";
import { plural, STATS_RANGES } from "./Panels";

/** Draws nothing with fewer than two bars, such as All devices today, whose counts come per day. */
export function WordsChart({ chart, range }: { chart: VoiceStats["chart"]; range: StatsRange }) {
  const { unit } = chart;
  const buckets = visibleBuckets(unit, chart.buckets, (b) => b.count > 0);
  if (buckets.length < 2) return null;
  return (
    <DayChart
      title={`Words per ${unit}`}
      caption={STATS_RANGES.find((r) => r.value === range)!.caption}
      bars={buckets.map((b) => ({
        key: b.key,
        value: b.words,
        tip: `${bucketName(unit, b.key)} · ${formatCount(b.words)} words · ${plural(b.count, "dictation")}`,
      }))}
      step={gridStep(Math.max(0, ...buckets.map((b) => b.words)))}
      format={formatCount}
      ticks={chartTicks(unit, buckets.map((b) => b.key))}
    />
  );
}
