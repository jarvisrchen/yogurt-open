import clsx from "clsx";
import type { UpdateStatus } from "../lib/api/update";
import { formatBytes } from "./dialogs/ModelDownloadDialog";

const STEPS = ["Checking Homebrew", "Downloading", "Installing", "Restarting"];

/**
 * Where an install stands: the step it is on, and a percentage only while the download's size is known.
 * A step of null is an install the server cannot split into steps, which gets one bar that only moves.
 */
export function updateProgress(s: UpdateStatus, restarting: boolean) {
  if (restarting || s.phase === "restarting") return { at: 3, label: STEPS[3], pct: null, detail: null };
  if (s.step === "downloading" && s.total && s.downloaded != null) {
    const pct = Math.min(100, Math.floor((s.downloaded / s.total) * 100));
    return { at: 1, label: STEPS[1], pct, detail: `${formatBytes(s.downloaded)} of ${formatBytes(s.total)}` };
  }
  if (!s.step) return { at: null, label: "Downloading and installing", pct: null, detail: null };
  const at = ["checking", "downloading", "installing"].indexOf(s.step);
  return { at, label: STEPS[at], pct: null, detail: null };
}

export function UpdateProgress({ s, restarting = false, className }: { s: UpdateStatus; restarting?: boolean; className?: string }) {
  const { at, label, pct, detail } = updateProgress(s, restarting);
  const text = pct == null ? label : `${label} ${pct}%`;
  return (
    <div className={className} role="progressbar" aria-label="Update" aria-valuetext={text} aria-valuenow={pct ?? undefined}>
      <div className="flex gap-1" aria-hidden>
        {(at == null ? [0] : STEPS.map((_, i) => i)).map((i) => {
          const active = at == null || i === at;
          return (
            <span key={i} className="h-1 flex-1 rounded-pill bg-line overflow-hidden">
              <span
                className={clsx(
                  "block h-full rounded-pill bg-blue",
                  active && pct == null && "w-[45%] motion-safe:animate-[toast-slide_1.4s_ease-in-out_infinite]",
                  active && pct != null && "transition-[width] duration-200",
                )}
                style={active ? (pct == null ? undefined : { width: `${pct}%` }) : { width: at != null && i < at ? "100%" : 0 }}
              />
            </span>
          );
        })}
      </div>
      <p className="mt-1.5 flex justify-between gap-3 text-caption text-mut tabular-nums">
        <span>{text}</span>
        {detail && <span>{detail}</span>}
      </p>
    </div>
  );
}
