import { useMutation, useQueryClient } from "@tanstack/react-query";
import { settingsApi, type General, type ProviderView } from "../../lib/api/settings";
import { testCleanup, useVoiceHistory, type CleanupTest } from "../../lib/api/voice";
import { Button } from "../Button";
import { Pill } from "../Pill";
import { LABEL } from "./labelClass";
import { CLEANUP_MODEL, useModel } from "../../lib/api/stt";
import { sizeLabel } from "./ModelPicker";
import { ModelDownloadButton, seconds } from "./TranscriptionField";
import { ModelRow } from "./ModelRow";

const CAPTION = "text-meta text-mut";
const BUILTIN = "builtin";
const GROUP = "voice-cleanup-model";

// From the design doc's "How fast it can be". A provider measured nowhere gets no line rather than a guess.
const SPEED: Record<string, string> = {
  "gemini-3.5-flash-lite": "About 0.85 s at any length",
  "deepseek-v4-flash": "About 1.3 to 1.8 s, near the 1.5 s first-word limit",
  "MiniMax-M3": "Thinks before it answers, so it misses the 1.5 s limit",
};

export function CleanupField({
  general,
  providers,
}: {
  general: General;
  providers: ProviderView[];
}) {
  const qc = useQueryClient();
  const test = useMutation({ mutationFn: testCleanup });
  const save = useMutation({
    mutationFn: (patch: Partial<General>) => settingsApi.patch(patch),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["settings"] }),
  });
  // Keyed by `cleanup_model`, so two providers serving the same model share one average.
  const averages = useVoiceHistory()?.history.cleanup_avg_ms ?? {};
  const on = general.voice_cleanup;
  const picked = general.voice_cleanup_provider;
  const builtin = useModel(CLEANUP_MODEL);
  // Unknown counts as missing, so Test never runs a model that is still loading its status.
  const downloaded = builtin.data?.downloaded ?? false;
  const missing = picked === BUILTIN && builtin.data?.downloaded === false;
  // A provider deleted in Settings > Model leaves the saved ID pointing at nothing, so no row is checked.
  const gone = picked !== BUILTIN && !providers.some((p) => p.id === picked);
  const pick = (value: string) => {
    test.reset();
    save.mutate({ voice_cleanup_provider: value });
  };
  const testButton = (
    <Button
      size="sm"
      variant="secondary"
      className="shrink-0"
      disabled={test.isPending || save.isPending}
      onClick={() => test.mutate()}
    >
      {test.isPending ? "Testing…" : "Test"}
    </Button>
  );

  return (
    <div className="border-t border-line pt-4 mt-4 space-y-2">
      <label className="flex items-center gap-2 text-ui text-ink">
        <input
          type="checkbox"
          checked={on}
          onChange={(e) => save.mutate({ voice_cleanup: e.target.checked })}
          className="h-4 w-4 accent-blue"
        />
        <span>Clean up the text with an LLM</span>
      </label>
      <p className={CAPTION}>
        Removes ums and repeats, keeps only the corrected version when you
        correct yourself, and fixes punctuation. The text goes to the model you
        pick below.
      </p>
      {save.error && (
        <p className="text-meta text-straw" role="alert">
          {save.error.message}
        </p>
      )}
      {on && (
        <div className="space-y-2 pt-1">
          <span id="cleanup-model-label" className={`block ${LABEL}`}>
            Cleanup model
          </span>
          {builtin.error && (
            <p className="text-meta text-straw" role="alert">
              Could not read the built-in model&apos;s status: {builtin.error.message}
            </p>
          )}
          <div
            role="radiogroup"
            aria-labelledby="cleanup-model-label"
            className="rounded-button border border-line bg-paper divide-y divide-line"
          >
            <ModelRow
              group={GROUP}
              name="Qwen3 4B Instruct"
              recommended
              label="Runs on this Mac"
              checked={picked === BUILTIN}
              disabled={!downloaded || save.isPending}
              onPick={() => pick(BUILTIN)}
              action={
                !downloaded
                  ? builtin.data && <ModelDownloadButton model={builtin.data} label="Download Qwen3 4B Instruct" />
                  : picked === BUILTIN && testButton
              }
            >
              <span className="block">Fastest on short dictations, slower on long ones</span>
              {builtin.data && (
                <>
                  <span className="whitespace-nowrap">{sizeLabel(builtin.data.size_mb)} download</span>
                  {" · "}
                </>
              )}
              {/* Measured as yogurt going from 0.2 GB to 3.2 GB with the model loaded. */}
              <span className="whitespace-nowrap">3.0 GB RAM</span>
              {averages[CLEANUP_MODEL] != null && (
                <span className="block text-ink font-medium">
                  On your Mac: {seconds(averages[CLEANUP_MODEL])} s average
                </span>
              )}
            </ModelRow>
            {providers.map((p) =>
              p.adapter === "cli" ? (
                <ModelRow
                  key={p.id}
                  group={GROUP}
                  name={p.name}
                  label="CLI, cannot stream"
                  checked={picked === p.id}
                  disabled
                  dim
                  onPick={() => undefined}
                >
                  A CLI answers only once it has finished, so it misses the 1.5 s limit for the first word.
                </ModelRow>
              ) : (
                <ModelRow
                  key={p.id}
                  group={GROUP}
                  name={p.model}
                  label={`Sends the text to ${p.name}`}
                  checked={picked === p.id}
                  disabled={save.isPending}
                  onPick={() => pick(p.id)}
                  action={picked === p.id && testButton}
                >
                  {SPEED[p.model] ?? "Not measured. Test shows its time."}
                  {averages[p.model] != null && (
                    <span className="block text-ink font-medium">Your average: {seconds(averages[p.model])} s</span>
                  )}
                </ModelRow>
              ),
            )}
          </div>
          {gone && (
            <p className="text-meta text-straw" role="alert">
              The model picked for cleanup is no longer in Settings › Model, so dictation pastes the raw text. Pick
              another.
            </p>
          )}
          {missing ? (
            <p className="text-meta text-ink">Until the download finishes, dictation pastes the raw text.</p>
          ) : test.error ? (
            <p className="text-meta text-straw" role="alert">
              Could not run the test: {test.error.message}
            </p>
          ) : (
            test.data && <TestResult result={test.data} />
          )}
          <p className={CAPTION}>
            Your providers from Settings › Model are listed too, Ollama
            included. If the model has not started answering after 1.5 s, or
            stalls, the raw text is pasted. Longer dictations get more time to
            finish.
          </p>
        </div>
      )}
    </div>
  );
}

function TestResult({ result: r }: { result: CleanupTest }) {
  if (r.outcome === "done") {
    return (
      <>
        <div className="flex flex-wrap items-center gap-2 pt-1">
          <Pill tone="matcha">{duration(r.ms)}</Pill>
          <span className={CAPTION}>on a {r.words}-word sample</span>
        </div>
        <div className="rounded-button border border-line bg-paper p-3 text-meta space-y-1.5">
          <p className="text-mut line-through decoration-grey/60">{r.raw}</p>
          <p className="text-ink">{r.text}</p>
        </div>
      </>
    );
  }
  if (r.outcome === "too_slow") {
    return (
      <div className="flex flex-wrap items-center gap-2 pt-1">
        <Pill tone="honey">{duration(r.ms)}</Pill>
        <span className="text-meta text-ink">
          {tooSlow(r)} so most dictations would paste the raw text. Pick a
          faster model.
        </span>
      </div>
    );
  }
  return (
    <div className="flex flex-wrap items-center gap-2 pt-1">
      <Pill tone="straw">{r.outcome === "failed" ? "Failed" : "Unavailable"}</Pill>
      <span className="text-meta text-ink" role="alert">
        {sentence(r.error ?? "")}
      </span>
    </div>
  );
}

function tooSlow(r: CleanupTest): string {
  const model = r.model ?? "This model";
  switch (r.limit) {
    case "stall":
      return `${model} paused for more than 0.5 s partway through,`;
    case "total":
      return `${model} took longer than the ${duration(r.total_ms)} a ${r.words}-word dictation gets,`;
    default:
      return `${model} had not started answering after 1.5 s,`;
  }
}

function duration(ms = 0): string {
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`;
}

function sentence(s: string): string {
  const t = `${s.charAt(0).toUpperCase()}${s.slice(1)}`;
  return /[.!?]$/.test(t) ? t : `${t}.`;
}
