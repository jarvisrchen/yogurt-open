import { useEffect, useState, type ReactNode } from "react";
import clsx from "clsx";
import { useLocation } from "react-router";
import { ArrowDownToLine, Check, Pause } from "lucide-react";
import { fetchHealth } from "../lib/api";
import { flushThenReload } from "../lib/flushBeforeReload";
import { readString, writeString } from "../lib/storage";
import {
  BREW_UPGRADE,
  releaseNotesUrl,
  UPDATE_ENDS_PRO,
  updateReady,
  useCancelUpdate,
  useInstallUpdate,
  useUpdateStatus,
} from "../lib/api/update";
import { updateEndsPro, useLicense } from "../lib/entitlements";
import { Button } from "./Button";
import { ProgressRing } from "./ProgressRing";
import { UpdateProgress } from "./UpdateProgress";

const LATER_KEY = "yogurt-update-later";
const FROM_KEY = "yogurt-updated-from";

/**
 * The bottom-right update notice. It follows an install through the server's restart, which keeps the
 * port and the session token, and reloads the page once `/api/health` answers with a new version.
 */
export function UpdateToast() {
  const { data: s } = useUpdateStatus();
  // First run has its own bottom button, and a new install has nothing to update yet.
  const welcome = useLocation().pathname.startsWith("/welcome");
  const install = useInstallUpdate();
  const cancel = useCancelUpdate();
  const endsPro = updateEndsPro(useLicense().data);
  const [later, setLater] = useState(() => readString(LATER_KEY));
  const [from, setFrom] = useState(() => readString(FROM_KEY));
  const [down, setDown] = useState(false);
  const [restartFrom, setRestartFrom] = useState<string | null>(null);

  const phase = s?.phase;
  const restarting = !!s && (phase === "installing" || phase === "restarting" || (phase === "waiting" && s.installed));
  useEffect(() => {
    setRestartFrom(restarting ? s.current : null);
    if (!restarting) setDown(false);
  }, [restarting, s?.current]);

  // Polls health only while an install runs, since the status endpoint goes quiet across the restart.
  useEffect(() => {
    if (!restartFrom) return;
    const t = window.setInterval(() => {
      fetchHealth().then(
        (h) => {
          setDown(false);
          if (h.version !== restartFrom) {
            writeString(FROM_KEY, restartFrom);
            void flushThenReload();
          }
        },
        () => setDown(true),
      );
    }, 1_000);
    return () => window.clearInterval(t);
  }, [restartFrom]);

  // A reload that came back on the same version is not an update.
  useEffect(() => {
    if (from && s?.current === from) {
      writeString(FROM_KEY, null);
      setFrom(null);
    }
  }, [from, s?.current]);

  if (welcome) return null;
  if (from && s && s.current !== from) {
    return (
      <Toast icon={<Check size={15} strokeWidth={2.6} />} tone="matcha" title={`Yogurt is now ${s.current}`}
        body="Everything is back where you left it."
        onClose={() => {
          writeString(FROM_KEY, null);
          setFrom(null);
        }}
        actions={<NotesLink version={s.current} />}
      />
    );
  }

  if (down || phase === "restarting") {
    return (
      <Toast icon={<ProgressRing pct={null} />} title="Restarting yogurt…" body="This page reloads by itself in a few seconds.">
        {s && <UpdateProgress s={s} restarting className="mt-2.5 ml-9.5" />}
      </Toast>
    );
  }
  if (!s || !updateReady(s)) return null;
  const v = s.latest;

  if (phase === "installing") {
    return (
      <Toast icon={<ArrowDownToLine size={15} strokeWidth={2.2} />} title={`Updating to ${v}…`}
        body="Keep working, yogurt restarts once it is done."
      >
        <UpdateProgress s={s} className="mt-2.5 ml-9.5" />
      </Toast>
    );
  }
  if (phase === "waiting") {
    return (
      <Toast icon={<Pause size={14} fill="currentColor" strokeWidth={0} />}
        title={s.installed ? "Restart waiting" : "Update waiting"}
        body={`Yogurt ${v} ${s.installed ? "is installed and restarts" : "installs and restarts"} once the current recording, summary, or dictation finishes.`}
        actions={!s.installed && (
          <Button size="sm" variant="secondary" onClick={() => cancel.mutate(undefined)} disabled={cancel.isPending}>
            Cancel update
          </Button>
        )}
      />
    );
  }
  const dismiss = () => {
    writeString(LATER_KEY, v);
    setLater(v);
  };
  if (phase === "failed") {
    return (
      <Toast icon="!" tone="straw" title="The update didn't install" body={`Yogurt is still on ${s.current} and kept running.`}
        onClose={() => {
          dismiss();
          cancel.mutate(undefined);
        }}
        actions={
          <>
            <Button size="sm" variant="secondary" onClick={() => install.mutate(undefined)} disabled={install.isPending}>
              Try again
            </Button>
            <Button size="sm" variant="ghost" onClick={() => void navigator.clipboard?.writeText(BREW_UPGRADE)}>
              Copy command
            </Button>
          </>
        }
      >
        {s.error && (
          <p className="mt-2 ml-9.5 rounded-chip border border-line bg-paper px-2 py-1.5 font-mono text-caption text-mut [overflow-wrap:anywhere]">
            {s.error}
          </p>
        )}
      </Toast>
    );
  }
  if (later === v) return null;
  return (
    <Toast icon={<ArrowDownToLine size={15} strokeWidth={2.2} />} title={`Yogurt ${v} is ready`}
      tone={endsPro ? "honey" : "blue"}
      body={
        endsPro
          ? `${UPDATE_ENDS_PRO}. Your notes and settings stay as they are.`
          : "Homebrew installs it and yogurt restarts in about a minute. Your notes and settings stay as they are."
      }
      onClose={dismiss}
      actions={
        <>
          <Button size="sm" onClick={() => install.mutate(undefined)} disabled={install.isPending}>
            Update and restart
          </Button>
          <Button size="sm" variant="ghost" onClick={dismiss}>
            Later
          </Button>
          <NotesLink version={v} />
        </>
      }
    />
  );
}

function NotesLink({ version }: { version: string }) {
  return (
    <a href={releaseNotesUrl(version)} target="_blank" rel="noopener noreferrer"
      className="ml-auto pr-1 text-ui font-medium text-blue whitespace-nowrap hover:underline"
    >
      What's new
    </a>
  );
}

interface ToastProps {
  icon: ReactNode;
  tone?: "blue" | "matcha" | "straw" | "honey";
  title: string;
  body: string;
  actions?: ReactNode;
  onClose?: () => void;
  children?: ReactNode;
}

const TONE = {
  blue: "bg-blsoft text-blue",
  matcha: "bg-mtsoft text-matcha",
  straw: "bg-strsoft text-straw font-bold",
  honey: "bg-honeysoft text-honeyink",
};

function Toast({ icon, tone = "blue", title, body, actions, onClose, children }: ToastProps) {
  return (
    <div
      role="status"
      data-testid="update-toast"
      className="fixed bottom-6 right-[calc(var(--dock-clear,0px)+24px)] z-40 w-91 max-w-[calc(100vw-32px)] rounded-card border border-line bg-card pt-3.5 pr-3.5 pb-3 pl-4 shadow-pop motion-safe:animate-pop-up"
    >
      <div className="flex items-start gap-2.5">
        <span aria-hidden className={clsx("grid h-7 w-7 shrink-0 place-items-center rounded-pill", TONE[tone])}>
          {icon}
        </span>
        <div className="min-w-0">
          <p className="text-body font-bold text-ink">{title}</p>
          <p className="mt-0.5 text-meta leading-normal text-mut">{body}</p>
        </div>
        {onClose && (
          <button type="button" onClick={onClose} aria-label="Close"
            className="-mt-0.5 ml-auto px-0.5 text-lead leading-none text-grey hover:text-ink"
          >
            ×
          </button>
        )}
      </div>
      {children}
      {actions && <div className="mt-3 flex items-center gap-1.5 pl-9.5">{actions}</div>}
    </div>
  );
}
