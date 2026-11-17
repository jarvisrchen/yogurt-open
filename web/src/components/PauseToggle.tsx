/** Pauses the whole recording (`POST /:id/paused`): both channels go silent and nothing is transcribed until it resumes. */
import type { CSSProperties } from "react";
import { Pause, Play } from "lucide-react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Button } from "./Button";
import { activeRecordingKey, setMeetingPaused } from "../lib/api/meetings";

interface PauseToggleProps {
  meetingId: string;
  /** From the shared active-recording query, so a pause made from another tab or `yogurt ctl` shows on the next poll. */
  paused: boolean;
  onError: (message: string) => void;
  style?: CSSProperties;
}

export function PauseToggle({ meetingId, paused, onError, style }: PauseToggleProps) {
  const qc = useQueryClient();
  const setPaused = useMutation({
    mutationFn: (next: boolean) => setMeetingPaused(meetingId, next),
    // Returned, so the button stays disabled until `paused` reflects the click and a quick second click cannot resend it.
    onSuccess: () => qc.invalidateQueries({ queryKey: activeRecordingKey }),
    onError: (e) => onError(e instanceof Error ? e.message : "Couldn't pause the recording"),
  });

  return (
    // Secondary's 1px border makes it 2px taller than the primary buttons beside it, so pin the height to keep the row still when this toggles.
    <Button
      variant={paused ? "warn" : "secondary"}
      className="h-8.5"
      style={style}
      disabled={setPaused.isPending}
      onClick={() => setPaused.mutate(!paused)}
      title={
        paused
          ? "Paused. Nothing is transcribed until you resume."
          : "Pause the recording. Both channels go silent until you resume."
      }
    >
      {paused ? <Play size={14} aria-hidden /> : <Pause size={14} aria-hidden />}
      {paused ? "Resume" : "Pause"}
    </Button>
  );
}
