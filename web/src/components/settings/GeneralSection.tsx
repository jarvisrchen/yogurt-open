// Appearance is browser-local (lib/theme.ts), so it applies instantly and survives reload without a flash.
import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { settingsApi, type General } from "../../lib/api/settings";
import { getThemePref, setThemePref, type ThemePref } from "../../lib/theme";
import { Segmented } from "../Segmented";
import { LABEL } from "./labelClass";
import { fieldClassName } from "../field";
import { Slot, slots } from "../../slots";

const THEME_OPTIONS: { value: ThemePref; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

interface GeneralSectionProps {
  general: General;
}

export function GeneralSection({ general }: GeneralSectionProps) {
  const qc = useQueryClient();
  const patch = useMutation({
    mutationFn: (p: Partial<General>) => settingsApi.patch(p),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["settings"] }),
  });

  const [theme, setTheme] = useState<ThemePref>(getThemePref);

  return (
    <section className="space-y-4">
      <h2 className="heading-section">General</h2>

      <div className="rounded-card border border-line bg-card p-5 space-y-5">
        <div className="space-y-1.5">
          <label className={`block ${LABEL}`}>
            Appearance
          </label>
          <Segmented
            label="Appearance"
            options={THEME_OPTIONS}
            value={theme}
            onChange={(v) => {
              setThemePref(v);
              setTheme(v);
            }}
          />
        </div>

        <div className="border-t border-line pt-4 mt-4 space-y-1.5">
          <label className={LABEL}>
            Port
          </label>
          <input
            type="number"
            min={1024}
            max={65535}
            defaultValue={general.port}
            onBlur={(e) => {
              const next = Number.parseInt(e.target.value, 10);
              if (
                Number.isFinite(next) &&
                next >= 1024 &&
                next <= 65535 &&
                next !== general.port
              ) {
                patch.mutate({ port: next });
              }
            }}
            className={fieldClassName("md", "block w-32")}
          />
          <p className="text-meta text-mut">
            Port change applies on next <code className="font-mono">yogurt start</code>.
          </p>
        </div>

        <div className="border-t border-line pt-4 mt-4 space-y-3">
          <label className="flex items-center gap-2 text-ui text-ink">
            <input
              type="checkbox"
              defaultChecked={general.open_browser_on_start}
              onChange={(e) =>
                patch.mutate({ open_browser_on_start: e.target.checked })
              }
              className="h-4 w-4 accent-blue"
            />
            <span>Open browser on start</span>
          </label>

          <label className="flex items-center gap-2 text-ui text-ink">
            <input
              type="checkbox"
              defaultChecked={general.meeting_detection}
              onChange={(e) => patch.mutate({ meeting_detection: e.target.checked })}
              className="h-4 w-4 accent-blue"
            />
            <span>Offer to record when a meeting is detected</span>
          </label>

          <label className="flex items-center gap-2 text-ui text-ink">
            <input
              type="checkbox"
              defaultChecked={general.meeting_detection_focus}
              onChange={(e) =>
                patch.mutate({ meeting_detection_focus: e.target.checked })
              }
              className="h-4 w-4 accent-blue"
            />
            <span>Bring the installed yogurt app to the front when a meeting is detected</span>
          </label>

          <label className="flex items-center gap-2 text-ui text-ink">
            <input
              type="checkbox"
              defaultChecked={general.meeting_auto_stop}
              onChange={(e) =>
                patch.mutate({ meeting_auto_stop: e.target.checked })
              }
              className="h-4 w-4 accent-blue"
            />
            <span>Stop recording automatically when the meeting ends</span>
          </label>

          <p className="text-meta text-mut">
            Detection reads on-screen window titles for known meeting apps
            (Zoom, Google Meet, Teams, Slack huddles). It never starts a
            recording on its own - it offers, you click. Nothing leaves your
            machine, and titles are never saved. Automatic stop ends a recording when the call app releases the
            microphone, or when the detected meeting window closes on systems
            that cannot report microphone use (needs detection on), or when
            neither your mic nor the meeting audio has made a sound for 5
            minutes. A minute before the silence stop, the meeting page warns
            you and offers Keep recording.
          </p>

          <NotificationPermissionControl />

          {!isStandalone() && (
            <p className="text-meta text-mut">
              Install yogurt as an app (Chrome menu &gt; Cast, save, and
              share &gt; Install page as app) so it can be brought to the
              front, alerts carry yogurt's name, and they keep arriving while
              the tab is in the background.
            </p>
          )}
        </div>

        <Slot of={slots.settingsRows.general} />
      </div>
    </section>
  );
}

/** Guarded because jsdom has no `matchMedia`, same as `lib/theme.ts`. */
function isStandalone(): boolean {
  return (
    typeof window.matchMedia === "function" &&
    window.matchMedia("(display-mode: standalone)").matches
  );
}

function NotificationPermissionControl() {
  const supported = "Notification" in window;
  const [permission, setPermission] = useState<NotificationPermission | null>(
    supported ? Notification.permission : null,
  );

  if (!supported || permission === null) return null;

  if (permission === "granted") {
    return <p className="text-meta text-mut">System notifications are on.</p>;
  }

  if (permission === "denied") {
    return (
      <p className="text-meta text-mut">
        Notifications are blocked for this site in Chrome. Allow them in the
        site settings to get alerts.
      </p>
    );
  }

  return (
    <button
      type="button"
      onClick={async () => {
        const result = await Notification.requestPermission();
        setPermission(result);
      }}
      className="px-3 py-1.5 rounded-button border border-line text-ui font-medium hover:bg-line/40 transition-colors"
    >
      Enable system notifications
    </button>
  );
}
