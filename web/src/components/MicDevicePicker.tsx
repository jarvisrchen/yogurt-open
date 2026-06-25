/**
 * While recording, a selection hot-swaps the live device (`POST /:id/audio-device`).
 * Otherwise it PATCHes `settings.audio_input_device`, which the next `POST /:id/start` reads.
 * With `voice`, it sets Voice's own mic, `settings.voice_input_device`, whose empty value follows the meetings mic.
 * Shares the `["audio-devices"]` query key with `AudioSection.tsx` for one cached fetch.
 */
import { useState } from "react";
import { DeviceOptions } from "./DeviceOptions";
import { Select } from "./Select";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { audioApi, settingsApi, settingsKey } from "../lib/api/settings";
import { activeRecordingKey, useActiveRecording, type ActiveRecording } from "../lib/api/meetings";

type MicDevicePickerProps =
  | {
      meetingId: string;
      /** True while recording: hot-swap instead of persisting the setting. */
      recording: boolean;
      voice?: never;
    }
  | { voice: true; meetingId?: never; recording?: never };

export function MicDevicePicker({ meetingId, recording = false, voice }: MicDevicePickerProps) {
  const qc = useQueryClient();
  const devices = useQuery({
    queryKey: ["audio-devices"],
    queryFn: audioApi.devices,
  });

  // Seeds the default selection while stopped, so it shows what the next Start will use. Disabled while recording, where hot-swap never reads it.
  const settings = useQuery({
    queryKey: settingsKey,
    queryFn: settingsApi.get,
    enabled: !recording,
  });

  // While recording, the server's answer for what the capture opened, so a reload or another tab's switch shows the live device.
  const active = useActiveRecording();
  // Voice mode has no meetingId, and `?.` alone would match undefined === undefined with no recording.
  const liveDevice = active.data && active.data.id === meetingId ? active.data.mic_device : undefined;

  // Set only on a successful settings write, so the stopped picker keeps the chosen device instead of reverting to the OS default.
  const [savedDevice, setSavedDevice] = useState<string | null>(null);

  const setDevice = useMutation({
    mutationFn: async (deviceId: string) => {
      if (recording && meetingId) {
        const res = await audioApi.switchMeetingDevice(meetingId, deviceId);
        return res.device;
      }
      if (voice) {
        const res = await settingsApi.patch({ voice_input_device: deviceId });
        void qc.invalidateQueries({ queryKey: settingsKey });
        return res.voice_input_device;
      }
      const res = await settingsApi.patch({ audio_input_device: deviceId });
      void qc.invalidateQueries({ queryKey: settingsKey });
      return res.audio_input_device;
    },
    onSuccess: (device) => {
      if (recording) {
        qc.setQueryData<ActiveRecording | null>(activeRecordingKey, (a) => a && { ...a, mic_device: device });
      } else {
        setSavedDevice(device);
      }
    },
  });

  // Voice mode waits for its saved choice, or it would show Same as meetings until settings load.
  if (devices.isLoading || (voice && settings.isLoading)) {
    return (
      <span className="text-ui text-mut">
        Loading mics…
      </span>
    );
  }

  if (devices.isError || (voice && settings.isError)) {
    return (
      <span className="text-ui text-mut">
        Mic list unavailable
      </span>
    );
  }

  const general = settings.data?.general;
  const fallback = devices.data?.find((d) => d.is_default)?.name ?? devices.data?.[0]?.name ?? "";
  const chosen = recording
    ? liveDevice
    : (savedDevice ?? (voice ? general?.voice_input_device : general?.audio_input_device));
  const meetingsMicMissing =
    !!general?.audio_input_device && !devices.data?.some((d) => d.name === general.audio_input_device);
  // In voice mode "" is a real choice, Same as meetings.
  const effectiveValue = voice ? (chosen ?? "") : chosen || fallback;

  return (
    <div className="flex items-center gap-1.5 w-full">
      <Select
        aria-label="Microphone"
        className="w-full"
        value={effectiveValue}
        disabled={setDevice.isPending}
        onChange={(e) => setDevice.mutate(e.target.value)}
      >
        {voice && (
          <option value="">
            Same as meetings: {general?.audio_input_device || fallback}
            {meetingsMicMissing && " (unavailable)"}
          </option>
        )}
        <DeviceOptions devices={devices.data} selected={effectiveValue} />
      </Select>
      {setDevice.isPending && (
        <span className="text-ui text-mut">
          {recording ? "Switching…" : "Saving…"}
        </span>
      )}
      {setDevice.isError && (
        <span className="text-ui text-[var(--color-straw)]">
          {setDevice.error instanceof Error
            ? setDevice.error.message
            : recording
              ? "Switch failed"
              : "Save failed"}
        </span>
      )}
    </div>
  );
}
