import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { PauseToggle } from "./PauseToggle";
import { setMeetingPaused } from "../lib/api/meetings";

vi.mock("../lib/api/meetings", async () => {
  const actual =
    await vi.importActual<typeof import("../lib/api/meetings")>("../lib/api/meetings");
  return { ...actual, setMeetingPaused: vi.fn() };
});

function renderToggle(paused: boolean, onError = vi.fn()) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={qc}>
      <PauseToggle meetingId="meeting-1" paused={paused} onError={onError} />
    </QueryClientProvider>,
  );
  return { onError };
}

describe("PauseToggle", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("offers Pause while the recording runs and pauses on click", async () => {
    vi.mocked(setMeetingPaused).mockResolvedValue();
    renderToggle(false);
    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    await waitFor(() => expect(setMeetingPaused).toHaveBeenCalledWith("meeting-1", true));
  });

  it("offers Resume while paused and resumes on click", async () => {
    vi.mocked(setMeetingPaused).mockResolvedValue();
    renderToggle(true);
    fireEvent.click(screen.getByRole("button", { name: "Resume" }));
    await waitFor(() => expect(setMeetingPaused).toHaveBeenCalledWith("meeting-1", false));
  });

  it("reports a failed pause to the page", async () => {
    vi.mocked(setMeetingPaused).mockRejectedValue(new Error("meeting is not currently recording"));
    const { onError } = renderToggle(false);
    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    await waitFor(() =>
      expect(onError).toHaveBeenCalledWith("meeting is not currently recording"),
    );
  });
});
