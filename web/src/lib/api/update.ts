import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { bearerJson } from "../session";

export type UpdatePhase = "idle" | "waiting" | "installing" | "restarting" | "failed";
export type UpdateStep = "checking" | "downloading" | "installing";

/** Rust `yogurt_server::update::Status`. */
export interface UpdateStatus {
  current: string;
  latest: string | null;
  /** `latest` is newer than `current`. */
  available: boolean;
  /** Unix ms of the last check that reached GitHub. */
  checked_at: number | null;
  check_error: string | null;
  auto_check: boolean;
  /** Only a Homebrew install updates itself. */
  installable: boolean;
  phase: UpdatePhase;
  /** Homebrew already installed it, so only the restart is left. */
  installed: boolean;
  error: string | null;
  /** What Homebrew is doing. Null outside an install, and during one the server cannot split into steps. */
  step: UpdateStep | null;
  /** Bytes of the release archive downloaded so far, and its size when GitHub reported one. */
  downloaded: number | null;
  total: number | null;
}

export const UPDATE_ENDS_PRO = "This update is past your Pro updates date, so Pro features turn off in it";
export const BREW_UPGRADE = "brew upgrade jarvisrchen/yogurt/yogurt";
export const releaseNotesUrl = (v: string) => `https://github.com/jarvisrchen/yogurt-releases/releases/tag/v${v}`;
/** The app socket writes each `update_status` frame here, so the bar moves between polls. */
export const UPDATE_KEY = ["update"];

export function updateReady(s: UpdateStatus | undefined): s is UpdateStatus & { latest: string } {
  return !!s?.installable && s.available && !!s.latest;
}

const BUSY: UpdatePhase[] = ["waiting", "installing", "restarting"];

export function useUpdateStatus() {
  return useQuery({
    queryKey: UPDATE_KEY,
    queryFn: () => bearerJson<UpdateStatus>("/api/update"),
    // Fast while an install runs, so every tab follows the restart, even one that did not click Update.
    refetchInterval: ({ state: { data } }) =>
      data && BUSY.includes(data.phase) ? 1_500 : updateReady(data) ? 15_000 : 10 * 60_000,
    retry: false,
  });
}

function useUpdateAction(path: string, method = "POST") {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body?: object) =>
      bearerJson<UpdateStatus>(path, { method, body: body ? JSON.stringify(body) : undefined }),
    onSuccess: (s) => qc.setQueryData(UPDATE_KEY, s),
  });
}

export const useCheckForUpdate = () => useUpdateAction("/api/update/check");
export const useInstallUpdate = () => useUpdateAction("/api/update/install");
export const useCancelUpdate = () => useUpdateAction("/api/update/cancel");
export const useSetAutoCheck = () => useUpdateAction("/api/update", "PATCH");
