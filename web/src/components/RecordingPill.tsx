import { useEffect, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import { useActiveRecording } from "../lib/api/meetings";

/** Hidden on the live view of the same recording, where "return to recording" is noise. */
export function RecordingPill() {
  const { data } = useActiveRecording();
  const location = useLocation();
  const navigate = useNavigate();
  const [now, setNow] = useState(() => Date.now());

  // Tick the elapsed timer once a recording is known. Torn down when there's
  // nothing to time so hidden instances don't run a background interval.
  useEffect(() => {
    if (!data) return;
    setNow(Date.now());
    const id = setInterval(() => setNow(Date.now()), 1_000);
    return () => clearInterval(id);
  }, [data]);

  if (!data) return null;
  if (location.pathname === `/meeting/${data.id}`) return null;

  const paused = data.paused_since != null;
  const elapsedSec = Math.max(
    0,
    Math.floor(((data.paused_since ?? now) - data.started_at) / 1000),
  );
  const mm = String(Math.floor(elapsedSec / 60)).padStart(2, "0");
  const ss = String(elapsedSec % 60).padStart(2, "0");

  return (
    <button
      type="button"
      onClick={() => navigate(`/meeting/${data.id}`)}
      aria-label={paused ? "Return to paused recording" : "Return to recording"}
      className="flex items-center gap-2 px-3 py-1.5 bg-card border border-line rounded-full shadow-pop text-ui text-ink hover:border-straw transition-colors"
    >
      <span
        aria-hidden="true"
        className={`inline-block w-2 h-2 rounded-pill ${paused ? "bg-mut" : "bg-straw animate-recpulse"}`}
      />
      <span className="font-medium truncate max-w-55">
        {data.title || "Recording"}
      </span>
      <span className="font-mono text-meta tracking-tight text-mut">
        {mm}:{ss}
      </span>
    </button>
  );
}
