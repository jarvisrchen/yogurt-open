import { useQuery } from "@tanstack/react-query";
import { fetchHealth } from "../../lib/api";
import {
  releaseNotesUrl,
  UPDATE_ENDS_PRO,
  updateReady,
  useCheckForUpdate,
  useInstallUpdate,
  useSetAutoCheck,
  useUpdateStatus,
  type UpdateStatus,
} from "../../lib/api/update";
import { Button } from "../Button";
import { Pill } from "../Pill";
import { ProgressRing } from "../ProgressRing";
import { UpdateProgress } from "../UpdateProgress";
import { LABEL } from "./labelClass";
import { Shimmer } from "../Shimmer";
import { updateEndsPro, useLicense } from "../../lib/entitlements";
import { Slot, slots } from "../../slots";

export const WEBSITE = "https://yogurt-ai.dev";
const LINK = "text-ui font-medium text-blue hover:underline";

export function AboutSection() {
  const health = useQuery({
    queryKey: ["health"],
    queryFn: fetchHealth,
    staleTime: Infinity,
    retry: false,
  });
  const version = health.data?.version;
  const update = useUpdateStatus().data;
  const setAutoCheck = useSetAutoCheck();

  return (
    <section className="space-y-4">
      <header className="space-y-1">
        <h2 className="heading-section">About</h2>
        <p className="text-ui text-mut">
          A local-first meeting copilot for macOS. Yogurt records your mic and
          system audio without a meeting bot, transcribes it live, and fills in
          your notes from the transcript. Zero telemetry.
        </p>
      </header>

      <div className="rounded-card border border-line bg-card p-5">
        <dl className="grid grid-cols-[120px_minmax(0,1fr)] gap-x-4 gap-y-3 items-center">
          <dt className={`${LABEL} self-start pt-2`}>Version</dt>
          <dd className="text-ui text-ink">
            <div className="flex min-h-8 flex-wrap items-center gap-x-2.5 gap-y-2">
              {health.isError ? (
                <span className="text-mut">Unavailable</span>
              ) : version ? (
                <>
                  <code data-testid="about-version" className="font-mono">
                    v{version}
                  </code>
                  {health.data?.mode === "dev" && <Pill>dev build</Pill>}
                </>
              ) : (
                <Shimmer className="h-4 w-16" inline label="Loading version" />
              )}
              {update?.installable && <UpdateControls s={update} />}
            </div>
            {update && (update.installable || health.data?.mode === "release") && <UpdateCaption s={update} />}
          </dd>

          <dt className={`${LABEL} self-start pt-1`}>License</dt>
          <dd className="text-ui text-ink">
            {slots.settingsRows.about.length > 0 ? <Slot of={slots.settingsRows.about} /> : "MIT"}
          </dd>

          <dt className={LABEL}>Links</dt>
          <dd className="flex flex-wrap gap-x-4 gap-y-1">
            <a href={WEBSITE} target="_blank" rel="noopener noreferrer" className={LINK}>
              Website
            </a>
            {/* A dev build's version has no tag until the release is pushed. */}
            {version && health.data?.mode === "release" && (
              <a
                href={releaseNotesUrl(version)}
                target="_blank"
                rel="noopener noreferrer"
                className={LINK}
              >
                Release notes
              </a>
            )}
          </dd>
        </dl>

        {update?.installable && (
          <div className="mt-4 border-t border-line pt-4 space-y-1.5">
            <label className="flex items-center gap-2 text-ui text-ink">
              <input
                type="checkbox"
                checked={update.auto_check}
                onChange={(e) => setAutoCheck.mutate({ auto_check: e.target.checked })}
                className="h-4 w-4 accent-blue"
              />
              <span>Check for updates once a day</span>
            </label>
            <p className="text-meta text-mut">
              Asks GitHub for the newest version number. It sends nothing about you or your meetings.
            </p>
          </div>
        )}
      </div>
    </section>
  );
}

function UpdateControls({ s }: { s: UpdateStatus }) {
  const check = useCheckForUpdate();
  const install = useInstallUpdate();

  if (s.phase === "waiting") return <Pill tone="blue">Waiting to update</Pill>;
  if (s.phase !== "idle" && s.phase !== "failed") return <UpdateProgress s={s} className="w-56" />;
  if (updateReady(s)) {
    return (
      <>
        <Pill tone="blue">
          <span className="h-1.5 w-1.5 rounded-pill bg-blue" />v{s.latest} is ready
        </Pill>
        <Button size="sm" onClick={() => install.mutate(undefined)} disabled={install.isPending}>
          Update and restart
        </Button>
        <a href={releaseNotesUrl(s.latest)} target="_blank" rel="noopener noreferrer" className={LINK}>
          What's new
        </a>
      </>
    );
  }
  return (
    <>
      {check.isPending ? null : s.check_error ? (
        <Pill tone="straw">
          <span className="h-1.5 w-1.5 rounded-pill bg-straw" />
          {s.check_error}
        </Pill>
      ) : s.checked_at ? (
        <Pill tone="matcha">
          <span className="h-1.5 w-1.5 rounded-pill bg-matcha" />
          Up to date
        </Pill>
      ) : null}
      <Button size="sm" variant="secondary" onClick={() => check.mutate(undefined)} disabled={check.isPending}>
        {check.isPending && <ProgressRing pct={null} />}
        {check.isPending ? "Checking…" : s.check_error ? "Try again" : "Check for updates"}
      </Button>
    </>
  );
}

function UpdateCaption({ s }: { s: UpdateStatus }) {
  const endsPro = updateEndsPro(useLicense().data);
  if (!s.installable) {
    return (
      <p className="mt-1.5 text-meta text-mut">
        This copy was not installed with Homebrew, so it can't update itself. Install it with{" "}
        <code className="font-mono">brew install jarvisrchen/yogurt/yogurt</code>, or update a source
        checkout with <code className="font-mono">git pull && ./scripts/setup.sh</code>.
      </p>
    );
  }
  if (updateReady(s) && s.phase === "idle") {
    return (
      <p className="mt-1.5 text-meta text-mut">
        {endsPro && <span className="text-honeyink">{UPDATE_ENDS_PRO}. </span>}
        Homebrew installs it, then yogurt restarts on the same port. If you are recording, it waits until the recording and its summary finish.
      </p>
    );
  }
  if (!s.checked_at) return null;
  const when = new Date(s.checked_at).toLocaleString(undefined, {
    weekday: "short",
    hour: "numeric",
    minute: "2-digit",
  });
  return (
    <p className="mt-1.5 text-meta text-mut">
      {s.check_error ? "Last successful check" : "Last checked"} {when}.
    </p>
  );
}
