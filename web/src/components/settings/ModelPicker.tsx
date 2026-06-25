// A Homebrew-managed model shows a "brew" chip instead of the trash: the server
// refuses to delete out of another tool's prefix.
import { useEffect, useState } from "react";
import clsx from "clsx";
import { Trash2 } from "lucide-react";
import type { DownloadState, ModelView } from "./modelTypes";

interface ModelPickerProps {
  models: ModelView[];
  selected: string | null;
  onSelect: (name: string) => void;
  // Also called for the in-flight model, where the parent reopens the existing dialog.
  onRequestDownload: (name: string) => void;
  activeDownloadName?: string | null;
  activeDownloadProgress?: DownloadState | null;
  // Omit to hide the trash affordance.
  onDelete?: (name: string) => void;
  activeModelName?: string | null;
  recommended?: string | null;
}

// Best-effort: a false positive only over-warns.
function isIntelMac(): boolean {
  if (typeof navigator === "undefined") return false;
  const ua = navigator.userAgent;
  // Safari/Chrome on Apple Silicon report "Intel Mac OS X", so check for an ARM signal first.
  if (/Mac/.test(ua) && /arm64|Apple Silicon/i.test(ua)) return false;
  // `userAgentData` is more reliable on Chromium.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const uaData = (navigator as any).userAgentData;
  if (uaData?.platform && /macOS/i.test(uaData.platform)) {
    // The high-entropy architecture hint is async, so accept false positives.
    return /Intel/i.test(ua);
  }
  return /Mac/.test(ua) && /Intel/.test(ua);
}

export function downloadPct(p: DownloadState | null): number | null {
  return p && p.totalBytes > 0
    ? Math.min(100, Math.round((p.bytesDownloaded / p.totalBytes) * 100))
    : null;
}

export function DownloadBar({ pct }: { pct: number | null }) {
  return (
    <>
      <span aria-hidden className="h-1.5 flex-1 rounded-pill bg-line overflow-hidden">
        <span className="block h-full rounded-pill bg-blue" style={{ width: `${pct ?? 0}%` }} />
      </span>
      <span aria-hidden className="text-caption text-blue font-semibold">
        {pct != null ? `${pct}%` : "…"}
      </span>
    </>
  );
}

export function sizeLabel(mb: number): string {
  return mb < 1000 ? `${mb} MB` : `${(mb / 1000).toFixed(1)} GB`;
}

export function ModelPicker({
  models,
  selected,
  onSelect,
  onRequestDownload,
  activeDownloadName = null,
  activeDownloadProgress = null,
  onDelete,
  activeModelName = null,
  recommended = null,
}: ModelPickerProps) {
  const intel = isIntelMac();
  const [confirming, setConfirming] = useState<string | null>(null);

  useEffect(() => {
    if (!confirming) return;
    const t = setTimeout(() => setConfirming(null), 3000);
    return () => clearTimeout(t);
  }, [confirming]);

  return (
    <div className="flex flex-wrap gap-2">
      {models.map((m) => {
        const isSelected = selected === m.name;
        // The browser cannot tell Apple Silicon from Intel, so trust the server when it says.
        const showSlow = m.runs_here === undefined ? intel && !m.intel_supported : !m.runs_here;
        const isDownloading =
          (activeDownloadName === m.name || m.downloading) && !m.downloaded;
        const pct =
          isDownloading && activeDownloadName === m.name
            ? downloadPct(activeDownloadProgress)
            : null;
        const glyph = m.downloaded ? "✓" : isDownloading ? "⟳" : "↓";
        const title = m.retired
          ? `${m.name} is no longer offered · delete it to free the space`
          : m.downloaded
          ? `${m.name}${isSelected ? " · selected" : ""}`
          : isDownloading
            ? pct != null
              ? `downloading… ${pct}%`
              : "downloading…"
            : `download (${m.size_mb} MB)`;
        return (
          <span
            key={m.name}
            className={clsx(
              "inline-flex gap-1.5",
              isDownloading ? "flex-col items-start" : "items-center",
            )}
          >
            <span className="inline-flex items-center gap-1.5">
            <button
              type="button"
              title={title}
              aria-pressed={isSelected}
              aria-disabled={m.retired || undefined}
              aria-label={
                m.retired
                  ? `${m.name}, no longer offered`
                  : m.downloaded
                  ? `Select ${m.name}`
                  : isDownloading
                    ? `Show download progress for ${m.name}`
                    : `Download ${m.name} (${m.size_mb} MB)`
              }
              onClick={() => {
                if (m.retired) return;
                if (m.downloaded) onSelect(m.name);
                else onRequestDownload(m.name);
              }}
              className={clsx(
                "text-ui font-medium px-2.5 py-1 rounded-pill transition-colors",
                // The pill wraps to the next row as a unit when the delete confirm appears.
                "inline-flex items-center gap-1.5 whitespace-nowrap shrink-0",
                isSelected
                  ? "border-[1.5px] border-matcha bg-mtsoft text-matcha"
                  : m.retired
                    ? "border border-line bg-card text-mut cursor-default"
                    : m.downloaded
                    ? "border border-line bg-card text-ink hover:border-matcha"
                    : isDownloading
                      ? "border border-matcha bg-mtsoft text-matcha hover:opacity-90"
                      : "border border-dashed border-line bg-card text-mut hover:border-matcha hover:text-matcha",
              )}
            >
              <span aria-hidden>{m.name}</span>
              <span aria-hidden className={isDownloading ? "animate-spin" : ""}>
                {glyph}
              </span>
              <span
                aria-hidden
                className="text-meta text-mut"
                title={
                  m.managed_by_homebrew
                    ? `${m.size_mb} MB installed by Homebrew`
                    : `${m.size_mb} MB on huggingface.co`
                }
              >
                {sizeLabel(m.size_mb)}
              </span>
            </button>
            {m.name === recommended && (
              <span className="text-caption px-1.5 py-0.5 rounded bg-mtsoft text-matcha border border-matcha/40">
                Recommended
              </span>
            )}
            {m.retired && (
              <span
                title="No longer offered - delete it to free the space"
                className="text-caption px-1.5 py-0.5 rounded bg-line/40 text-mut border border-line"
              >
                retired
              </span>
            )}
            {showSlow && (
              <span
                title="Slower than real-time on Intel"
                className="text-caption px-1.5 py-0.5 rounded bg-strsoft text-ink border border-straw/40"
              >
                slow
              </span>
            )}
            {m.downloaded && m.managed_by_homebrew ? (
              <span
                title="Installed by Homebrew - remove it with brew uninstall"
                className="text-caption px-1.5 py-0.5 rounded bg-line/40 text-mut border border-line"
              >
                brew
              </span>
            ) : m.downloaded && onDelete && m.name !== activeModelName ? (
              confirming === m.name ? (
                <span className="inline-flex items-center gap-1">
                  <button
                    type="button"
                    autoFocus
                    aria-label={`Confirm delete ${m.name}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      setConfirming(null);
                      onDelete(m.name);
                    }}
                    className="text-caption px-2 py-0.5 rounded-full bg-strsoft text-ink border border-straw/40 font-semibold whitespace-nowrap hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-straw/50"
                  >
                    Delete?
                  </button>
                  <button
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      setConfirming(null);
                    }}
                    className="text-caption text-mut hover:text-ink"
                  >
                    Cancel
                  </button>
                </span>
              ) : (
                <button
                  type="button"
                  aria-label={`Delete ${m.name}`}
                  title="Delete downloaded model"
                  onClick={(e) => {
                    e.stopPropagation();
                    setConfirming(m.name);
                  }}
                  className="p-1 rounded-full text-mut hover:text-straw hover:bg-line/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-straw/40"
                >
                  <Trash2 size={13} aria-hidden />
                </button>
              )
            ) : null}
            </span>
            {isDownloading && (
              <span
                className="inline-flex items-center gap-1.5 w-24"
                title="click pill to reopen progress dialog"
              >
                <DownloadBar pct={pct} />
              </span>
            )}
          </span>
        );
      })}
    </div>
  );
}
