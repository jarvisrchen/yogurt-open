import { DeviceOptions } from "../DeviceOptions";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { audioApi, settingsApi, type General } from "../../lib/api/settings";
import { RefreshDevicesButton } from "../RefreshDevicesButton";
import { LABEL } from "./labelClass";
import { Select } from "../Select";
import { Slot, slots } from "../../slots";

interface AudioSectionProps {
  general: General;
}

export function AudioSection({ general }: AudioSectionProps) {
  const qc = useQueryClient();
  const devices = useQuery({
    queryKey: ["audio-devices"],
    queryFn: audioApi.devices,
  });

  const patch = useMutation({
    mutationFn: (audio_input_device: string) =>
      settingsApi.patch({ audio_input_device }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["settings"] }),
  });

  return (
    <section className="space-y-4">
      <h2 className="heading-section">Audio</h2>

      <div className="rounded-card border border-line bg-card p-5 space-y-4">
        <div className="space-y-1.5">
          <div className="flex items-center justify-between">
            <label className={LABEL}>
              Input device
            </label>
            <RefreshDevicesButton />
          </div>
          <Select
            className="block w-full"
            value={general.audio_input_device}
            onChange={(e) => patch.mutate(e.target.value)}
            disabled={devices.isLoading || patch.isPending}
          >
            <option value="">System default</option>
            <DeviceOptions devices={devices.data} selected={general.audio_input_device} />
          </Select>
        </div>

        <Slot of={slots.settingsRows.audio} />
        <p className="text-meta text-mut">
          System audio is captured via ScreenCaptureKit, with no extra setup.
        </p>
      </div>
    </section>
  );
}
