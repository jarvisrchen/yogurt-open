import { useEffect, useState } from "react";
import clsx from "clsx";
import { useModels, useDeleteModel, formatBytes } from "../../lib/api/stt";
import { settingsApi } from "../../lib/api/settings";
import {
  beginDownload,
  showDownloadDialog,
  useModelDownload,
} from "../../hooks/useModelDownload";
import { ModelPicker } from "./ModelPicker";
import { PARAKEET_CREDIT, RECOMMENDED_MODEL } from "./modelTypes";
import { TestKeyButton } from "./TestKeyButton";
import { Button } from "../Button";
import { Pill } from "../Pill";

// http() throws "<status> <statusText>: <body>". The 409 body is a plain sentence, so strip the prefix.
function deleteErrorMessage(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err);
  // `[^:]*` not `+`: HTTP/2 has no reason phrase, so the prefix can be "409 : ".
  return raw.replace(/^\d+\s+[^:]*:\s*/, "");
}

interface LocalSTTCardProps {
  active: boolean;
  selectedModel: string;
  onSelectModel: (name: string) => void;
  onActivate: () => void;
}

export function LocalSTTCard({
  active,
  selectedModel,
  onSelectModel,
  onActivate,
}: LocalSTTCardProps) {
  const q = useModels();
  const del = useDeleteModel();
  // Tracked apart from `del.isSuccess` so a later error leaves no stale "Deleted" line.
  const [showFreed, setShowFreed] = useState(false);
  useEffect(() => {
    if (!del.isSuccess) return;
    setShowFreed(true);
    const t = window.setTimeout(() => setShowFreed(false), 4000);
    return () => window.clearTimeout(t);
  }, [del.isSuccess, del.data]);
  const download = useModelDownload();

  // A second model's click replaces the tracked one, though the server still finishes the first.
  const startOrReopenDownload = (name: string) => {
    if (download?.model === name) showDownloadDialog();
    else beginDownload(name);
  };

  // The selected model must be on disk or recording fails at meeting start.
  // Deliberately not gated on `active`, so a provider already set to local
  // with a deleted model still shows the warning.
  const selectedModelDownloaded =
    q.data?.find((m) => m.name === selectedModel)?.downloaded ?? false;
  const activateBlocked = !selectedModelDownloaded;

  return (
    <article
      data-testid="local-stt-card"
      className={clsx(
        "rounded-card bg-card p-5 space-y-4 transition-colors",
        active
          ? "border-[1.5px] border-blue shadow-button-blue"
          : "border border-line",
      )}
    >
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2 min-w-0">
          <h3 className="heading-card whitespace-nowrap">Local</h3>
          <Pill tone="neutral">whisper.cpp · Parakeet</Pill>
        </div>
        {active ? (
          <Pill tone="matcha" className="shrink-0">
            <span
              className="inline-block w-1.75 h-1.75 rounded-pill bg-matcha"
              aria-hidden
            />
            In use
          </Pill>
        ) : (
          <Button
            variant="primary"
            className="px-3 py-1.5 whitespace-nowrap shrink-0"
            disabled={activateBlocked}
            // Guard in the handler too so a synthetic click on a disabled button cannot activate.
            onClick={() => {
              if (!activateBlocked) onActivate();
            }}
          >
            Use Local
          </Button>
        )}
      </header>
      {q.data && activateBlocked && (
        <span className="text-caption text-straw">
          Download the model first
        </span>
      )}

      <p className="text-meta text-mut">
        Parakeet runs on the CPU, so it stays fast while other work uses the
        GPU, and whisper.cpp runs on Metal. Audio never leaves this Mac. Local
        is free.
      </p>

      {q.isLoading && (
        <p className="text-meta text-mut">Loading models…</p>
      )}
      {q.isError && (
        <p className="text-meta text-straw">
          Failed to load models: {String(q.error)}
        </p>
      )}
      {q.data && (
        <ModelPicker
          models={q.data}
          selected={selectedModel}
          onSelect={onSelectModel}
          onRequestDownload={startOrReopenDownload}
          activeDownloadName={
            download && !download.progress.error ? download.model : null
          }
          activeDownloadProgress={download?.progress ?? null}
          onDelete={(name) => del.mutate(name)}
          activeModelName={active ? selectedModel : null}
          recommended={RECOMMENDED_MODEL}
        />
      )}

      {q.data && (
        <div className="flex flex-wrap items-center gap-2 border-t border-line pt-4 mt-4">
          <TestKeyButton
            providerName={selectedModel}
            hasStoredKey={selectedModelDownloaded}
            config={{ model: selectedModel }}
            testFn={() => settingsApi.testLocalStt(selectedModel)}
          />
        </div>
      )}

      {del.isError && (
        <p role="alert" className="text-meta text-straw">
          {deleteErrorMessage(del.error)}
        </p>
      )}
      {showFreed && del.data && (
        <p
          data-testid="model-freed"
          className="text-meta text-matcha"
        >
          Deleted {del.variables}
          {del.data.freed_bytes > 0
            ? ` - freed ${formatBytes(del.data.freed_bytes)}`
            : ""}
        </p>
      )}

      <p className="text-meta text-mut pt-1">{PARAKEET_CREDIT}</p>
      <p className="text-meta text-mut">
        Models download on first use · stored in{" "}
        <code className="font-mono text-ink">~/.yogurt/models/</code>
      </p>
    </article>
  );
}
