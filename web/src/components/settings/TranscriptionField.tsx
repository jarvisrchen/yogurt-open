import { Link } from "react-router";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  settingsApi,
  VOICE_STT_DEEPGRAM,
  VOICE_STT_MEETINGS,
  voiceModel,
  type General,
} from "../../lib/api/settings";
import { useModels } from "../../lib/api/stt";
import { PARAKEET_CREDIT, RECOMMENDED_MODEL, type ModelView } from "./modelTypes";
import { useVoiceHistory } from "../../lib/api/voice";
import { beginDownload, showDownloadDialog, useModelDownload } from "../../hooks/useModelDownload";
import { Button } from "../Button";
import { ModelRow } from "./ModelRow";
import { DownloadBar, downloadPct, sizeLabel } from "./ModelPicker";
import { LABEL } from "./labelClass";
import { patchErrorMessage } from "./STTPicker";

const CAPTION = "text-meta text-mut";

// Relative on purpose, since the wait depends on the Mac. Measured in the design doc's "Which models to use > Transcription".
const GUIDE: Record<string, { label: string; ram: string }> = {
  "parakeet-v3": { label: "Fastest, and stays fast during meetings", ram: "1.3 GB" },
  "small.en": { label: "Fast, catches more names", ram: "1.0 GB" },
  "tiny.en": { label: "Smallest, misses more words", ram: "0.4 GB" },
  "large-v3-turbo": { label: "Most accurate, slowest", ram: "2.2 GB" },
};

export function TranscriptionField({ general, deepgramKey }: { general: General; deepgramKey: boolean }) {
  const qc = useQueryClient();
  const models = useModels();
  const averages = useVoiceHistory()?.history.stt_avg_ms ?? {};
  const save = useMutation({
    mutationFn: (voice_stt_model: string) => settingsApi.patch({ voice_stt_model }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["settings"] }),
  });
  const meetings = general.voice_stt_model === VOICE_STT_MEETINGS;
  const resolved = voiceModel(general);
  const meetingsMissing = models.data?.some((m) => m.name === general.stt_model && !m.downloaded) ?? false;

  return (
    <div className="border-t border-line pt-4 mt-4 space-y-2">
      <span id="voice-stt-label" className={`block ${LABEL}`}>
        Transcription
      </span>
      {models.isError && <p className="text-meta text-straw">Could not list the models: {models.error.message}</p>}
      <div
        role="radiogroup"
        aria-labelledby="voice-stt-label"
        className="rounded-button border border-line bg-paper divide-y divide-line"
      >
        <ModelRow
          group="voice-stt-model"
          name="Same as meetings"
          label="Follows Settings › Transcription"
          checked={meetings}
          disabled={meetingsMissing || save.isPending}
          onPick={() => save.mutate(VOICE_STT_MEETINGS)}
        >
          Now {general.stt_model}
        </ModelRow>
        {models.data
          ?.filter((m) => !m.retired && (m.runs_here !== false || (!meetings && m.name === resolved)))
          .map((m) => {
            const avg = averages[m.name];
            return (
              <ModelRow
                key={m.name}
                group="voice-stt-model"
                name={m.name}
                recommended={m.name === RECOMMENDED_MODEL}
                label={GUIDE[m.name]?.label}
                checked={!meetings && m.name === resolved}
                disabled={!m.downloaded || save.isPending}
                onPick={() => save.mutate(m.name)}
                action={<ModelDownloadButton model={m} />}
              >
                <span className="whitespace-nowrap">{sizeLabel(m.size_mb)} download</span>
                {GUIDE[m.name] && (
                  <>
                    {" · "}
                    <span className="whitespace-nowrap">{GUIDE[m.name].ram} RAM</span>
                  </>
                )}
                {avg != null && (
                  <span className="block text-ink font-medium">On your Mac: {seconds(avg)} s average</span>
                )}
                {m.name === "parakeet-v3" && <span className="block">{PARAKEET_CREDIT}</span>}
              </ModelRow>
            );
          })}
        <ModelRow
          group="voice-stt-model"
          name="Deepgram"
          label="Sends your audio to Deepgram"
          checked={resolved === VOICE_STT_DEEPGRAM}
          disabled={!deepgramKey || save.isPending}
          onPick={() => save.mutate(VOICE_STT_DEEPGRAM)}
        >
          {deepgramKey ? (
            <>
              Uses the key in <TranscriptionLink />
              {averages[VOICE_STT_DEEPGRAM] != null && (
                <span className="block text-ink font-medium">
                  Your average: {seconds(averages[VOICE_STT_DEEPGRAM])} s
                </span>
              )}
            </>
          ) : (
            <>
              No key stored yet. Add one in <TranscriptionLink />.
            </>
          )}
        </ModelRow>
      </div>
      {save.error && (
        <p className="text-meta text-straw" role="alert">
          {patchErrorMessage(save.error)}
        </p>
      )}
      <p className={CAPTION}>
        Local models keep your audio on this Mac, and Deepgram sends it to Deepgram. The wait depends on your Mac and
        network, so each choice shows its own average from the dictations History keeps.
      </p>
    </div>
  );
}

function TranscriptionLink() {
  return (
    <Link to="/settings/transcription" className="text-blue font-semibold hover:underline">
      Settings › Transcription
    </Link>
  );
}

export function seconds(ms: number): string {
  return (ms / 1000).toFixed(2);
}

export function ModelDownloadButton({ model, label }: { model: ModelView; label?: string }) {
  const download = useModelDownload();
  if (model.downloaded) return null;
  const mine = download?.model === model.name;
  if ((mine && !download.progress.error) || model.downloading) {
    return (
      <button
        type="button"
        aria-label={`Show download progress for ${model.name}`}
        onClick={showDownloadDialog}
        className="inline-flex items-center gap-1.5 w-24 shrink-0"
      >
        <DownloadBar pct={mine ? downloadPct(download.progress) : null} />
      </button>
    );
  }
  return (
    <Button
      size="sm"
      variant="secondary"
      className="shrink-0"
      aria-label={label ?? `Download ${model.name}`}
      onClick={() => beginDownload(model.name)}
    >
      Download
    </Button>
  );
}
