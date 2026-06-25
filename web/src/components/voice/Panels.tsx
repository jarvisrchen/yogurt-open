import { Fragment, type ReactNode } from "react";
import type { StatsRange, VoiceStats } from "../../lib/api/voiceTypes";
import { formatCount } from "../library/pagination";
import { LABEL } from "../settings/labelClass";
import { appShares, approxUsd, costRow, monthDay, pace, RANGE_WORDS, savedLabel, savedMs, sinceLabel, TYPING_WPM, usd, type Cost } from "./format";
import { useCount } from "./motion";
import type { AllDevices } from "./totals";

// The Voice page's tiles and panels, shared with the phone's Voice tab in the Cloud webapp, so nothing here may import react-router or react-query.
export const PANEL = "rounded-card border border-line bg-card shadow-card p-5 min-w-0";
export const SETTINGS_LINK = "text-blue font-semibold hover:underline";
export const STATS_RANGES: { value: StatsRange; label: string; caption: string }[] = [
  { value: "today", label: "Today", caption: "Today" },
  { value: "7d", label: "7 days", caption: "Last 7 days" },
  { value: "30d", label: "30 days", caption: "Last 30 days" },
  { value: "all", label: "All time", caption: "All time" },
];

/**
 * A number counts to `value` on its own layer, over the final text in clear ink, which holds the width and is what a screen reader reads.
 * Any other `value` shows as it is.
 */
export function Tile({
  label,
  value,
  format = String,
  sub,
  tone = "text-ink",
}: {
  label: string;
  value: number | ReactNode;
  format?: (n: number) => string;
  sub: string;
  tone?: string;
}) {
  const counts = typeof value === "number";
  const shown = useCount(counts ? value : 0);
  return (
    <div className="rounded-card border border-line bg-card shadow-card px-4 py-3.5 min-w-0">
      <p className={LABEL}>{label}</p>
      <p className={`relative mt-2 text-display font-extrabold tracking-[-0.01em] leading-none tabular-nums ${tone}`}>
        {!counts ? (
          value
        ) : (
          <>
            <span className={shown === value ? "" : "text-transparent"}>{format(value)}</span>
            {shown !== value && (
              <span aria-hidden className="absolute left-0 top-0 whitespace-nowrap">
                {format(Math.round(shown))}
              </span>
            )}
          </>
        )}
      </p>
      <p className="mt-1.5 text-meta text-mut">{sub}</p>
    </div>
  );
}

/** The cleanup cost of the stats, or with `all`, of every sharing device, which priced its own dictations and so has no split by model. */
export function cleanupCost(s: VoiceStats, all?: AllDevices): Cost {
  if (!all) return costRow(s.costs);
  const n = all.unpriced_count;
  return { usd: all.usd > 0 || all.stats.cleaned_count > 0 ? all.usd : null, unpriced: n, parts: n > 0 ? [`${plural(n, "dictation")} not priced`] : [] };
}

/** With `all`, the tiles sum every sharing device, and the first one names how many. `cleanup` is whether Voice cleanup is on, for the cost tile's empty state. */
export function Tiles({ s, range, all, cleanup }: { s: VoiceStats; range: StatsRange; all?: AllDevices; cleanup: boolean }) {
  const since = sinceLabel(s.first_day, range);
  const saved = Math.round(savedMs(s));
  const on = all == null || s.first_day == null ? "" : `, on ${plural(all.devices.length, "device")}`;
  const cost = cleanupCost(s, all);
  return (
    <div className="grid grid-cols-2 @[820px]:grid-cols-4 gap-3">
      <Tile label="Words dictated" value={s.words} format={formatCount} sub={since + on} />
      <Tile label="Speaking pace" value={pace(s)} format={(n) => `${n} wpm`} sub={`typing is about ${TYPING_WPM}`} />
      <Tile label="Time saved" value={saved} format={(n) => savedLabel(n, saved)} sub="against typing it" tone="text-matcha" />
      <Tile
        label="Estimated cost"
        value={cost.usd != null ? approxUsd(cost.usd) : <Muted>{cost.unpriced > 0 ? "Not priced" : cleanup ? "None" : "Off"}</Muted>}
        sub="cleanup, at list price"
        tone="text-blue"
      />
    </div>
  );
}

export interface Bar {
  key: string;
  name: ReactNode;
  /** Of the track, 0 to 100. */
  width: number;
  value: ReactNode;
  title: string;
  /** A tick across the track at this percent. */
  mark?: number;
}

export function BarList({ bars }: { bars: Bar[] }) {
  return (
    // The name column fits the longest name, so a phone's text zoom does not cut it short, and every bar starts at one x.
    <ul className="mt-4 grid grid-cols-[fit-content(40%)_1fr_auto] gap-x-3 gap-y-2.5 text-ui">
      {bars.map((b) => (
        <li key={b.key} className="col-span-3 grid grid-cols-subgrid items-center" title={b.title}>
          <span className="truncate text-ink">{b.name}</span>
          <span className="relative h-2 rounded-pill bg-line/60 overflow-hidden">
            <span
              className="block h-full rounded-pill bg-blue motion-safe:animate-grow-x motion-safe:transition-[width] motion-safe:duration-520 motion-safe:ease-(--ease-slide)"
              style={{ width: `${b.width}%` }}
            />
            {b.mark != null && <span aria-hidden className="absolute inset-y-0 w-0.5 -ml-px bg-grey" style={{ left: `${b.mark}%` }} />}
          </span>
          <span className="text-right tabular-nums text-ink">{b.value}</span>
        </li>
      ))}
    </ul>
  );
}

export function AppsPanel({ apps }: { apps: VoiceStats["apps"] }) {
  const shares = appShares(apps);
  const max = Math.max(1, ...shares.map((a) => a.pct));
  return (
    <div className={PANEL}>
      <h2 className="heading-sm">Where you dictate</h2>
      <p className="text-meta text-mut mt-0.5">Share of words, by the app the text was pasted into</p>
      {shares.length === 0 ? (
        <p className="mt-4 text-ui text-mut">No dictations yet.</p>
      ) : (
        <BarList
          bars={shares.map((a) => ({
            key: a.name,
            name: a.name,
            width: (a.pct / max) * 100,
            value: `${a.pct}%`,
            title: `${a.name} · ${formatCount(a.words)} words`,
          }))}
        />
      )}
    </div>
  );
}

/**
 * `turnOn` links to where cleanup is turned on. The phone has no hands-free mode, so it leaves that row out.
 * With `all`, the card sums every sharing device: each device priced its own dictations, and a median cannot be summed, so that row goes.
 * `on` says cleanup is on now, so a range with nothing cleaned reads as none yet rather than off.
 */
export function CleanupPanel({
  s,
  turnOn,
  handsFree = true,
  all,
  on,
}: {
  s: VoiceStats;
  turnOn: ReactNode;
  handsFree?: boolean;
  all?: AllDevices;
  on: boolean;
}) {
  const off = s.cleaned_count === 0 && s.too_slow_count === 0;
  const empty = <Muted>{on ? "None yet" : "Off"}</Muted>;
  const said = s.words + s.cleaned_words;
  const rows: [string, ReactNode, ReactNode?][] = [
    [
      "Words cleaned out",
      off ? empty : formatCount(s.cleaned_words),
      off || said === 0 ? undefined : `about ${Math.round((s.cleaned_words / said) * 100)}% of what you said`,
    ],
    ["Ums and uhs removed", off ? empty : formatCount(s.fillers_removed)],
    ["Pasted raw, cleanup too slow", off ? empty : formatCount(s.too_slow_count)],
  ];
  if (!all)
    rows.push([
      "Median time to paste",
      s.median_wait_ms == null ? <Muted>None yet</Muted> : `${(s.median_wait_ms / 1000).toFixed(2)} s`,
      "after you let go",
    ]);
  if (handsFree)
    rows.push(["Hands-free dictations", s.count ? `${Math.round((s.hands_free_count / s.count) * 100)}%` : <Muted>None yet</Muted>]);
  // The total is the Estimated cost tile's, so the row is here only for what the tile cannot split out.
  const { parts } = cleanupCost(s, all);
  if (!off && parts.length)
    rows.push([
      all ? "Cost" : "Cost by model",
      null,
      // Each model stays on one line, so the card wraps between models, never inside a name like deepseek-v4-flash.
      parts.map((p, i) => (
        <Fragment key={p}>
          {i > 0 && " · "}
          <span className="whitespace-nowrap">{p}</span>
        </Fragment>
      )),
    ]);
  return (
    <div className={PANEL}>
      <h2 className="heading-sm">Cleanup</h2>
      <p className="text-meta text-mut mt-0.5">
        {off && on ? (
          "On, and nothing cleaned up yet"
        ) : off ? (
          <>
            Off, so dictations paste as transcribed. {turnOn}
          </>
        ) : all ? (
          plural(all.devices.length, "device")
        ) : (
          (s.cleanup_model ?? "The rule pass for short clips")
        )}
      </p>
      <StatRows rows={rows.map(([k, value, sub]) => ({ key: k, label: k, value, sub }))} />
    </div>
  );
}

export interface StatRow {
  key: string;
  label: ReactNode;
  value: ReactNode;
  sub?: ReactNode;
}

export function StatRows({ rows }: { rows: StatRow[] }) {
  return (
    <dl className="mt-3">
      {rows.map((r, i) => (
        <div key={r.key} className={`flex items-baseline justify-between gap-3 py-2 ${i ? "border-t border-line" : ""}`}>
          <dt className="text-ui text-ink min-w-0">
            {r.label}
            {r.sub && <span className="block text-meta text-mut">{r.sub}</span>}
          </dt>
          <dd className="text-body font-bold tabular-nums text-ink whitespace-nowrap">{r.value}</dd>
        </div>
      ))}
    </dl>
  );
}

export function plural(n: number, noun: string): string {
  return `${formatCount(n)} ${noun}${n === 1 ? "" : "s"}`;
}

/** "Nov 8, 1:02 PM" in the user's local time. */
function asOf(ms: number): string {
  const d = new Date(ms);
  return `${monthDay(d)}, ${d.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" })}`;
}

function syncedAgo(at: number, now: number): string {
  const min = Math.floor((now - at) / 60_000);
  if (min < 1) return "Synced just now";
  if (min < 60) return `Synced ${min} min ago`;
  if (min < 24 * 60) return `Synced ${Math.floor(min / 60)}h ago`;
  return `Synced ${asOf(at)}`;
}

/** Each device syncs its totals at most once a day. */
export interface TotalsSync {
  at: number;
  busy: boolean;
  error?: string | null;
  run: () => void;
}

/** All devices' stand-in for Where you dictate, since the shared rows carry no app names. `self` is this device's id. */
export function DevicesPanel({ all, range, self, selfLabel, sync }: { all: AllDevices; range: StatsRange; self: string; selfLabel: string; sync: TotalsSync }) {
  return (
    <div className={PANEL}>
      <h2 className="heading-sm">By device</h2>
      <p className="text-meta text-mut mt-0.5">
        {range === "all" ? "All words" : `Words ${RANGE_WORDS[range]}`}, from each device that shares its daily totals
      </p>
      <p className="text-meta text-mut mt-0.5">
        {sync.busy ? "Syncing…" : syncedAgo(sync.at, Date.now())}
        {" · "}
        <button type="button" onClick={sync.run} disabled={sync.busy} className={`${SETTINGS_LINK} bg-transparent border-0 p-0 cursor-pointer disabled:opacity-50 disabled:cursor-default`}>
          Sync now
        </button>
        {sync.error && <span className="block text-straw" role="alert">Couldn't sync: {sync.error}</span>}
      </p>
      <StatRows
        rows={all.devices.map((d) => ({
          key: d.id,
          label: `${d.name}${d.id === self ? ` (${selfLabel})` : ""}`,
          value: formatCount(d.words),
          sub: (
            <>
              totals as of {asOf(d.updated_at)}
              {d.cleaned && ` · ${d.usd > 0 ? `≈ ${usd(d.usd)}` : "$0"}`}
            </>
          ),
        }))}
      />
    </div>
  );
}

export function ErrorNote({ what, error }: { what: string; error: Error }) {
  return (
    <div className="inline-block mb-3 text-ui text-ink bg-strsoft border border-straw/40 rounded-button px-3 py-2">
      Couldn't load {what}: {error.message}
    </div>
  );
}

export function Muted({ children }: { children: ReactNode }) {
  return <span className="text-ui font-semibold text-mut">{children}</span>;
}
