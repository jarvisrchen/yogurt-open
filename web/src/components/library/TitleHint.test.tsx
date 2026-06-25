import { describe, it, expect, afterEach, vi } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TitleHint } from "./TitleHint";
import { meetingsApi, type Meeting, type MeetingSummary } from "../../lib/api/meetings";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const OCT6 = new Date("2026-10-06T12:00:00").getTime();
const MEETING = {
  id: "m1",
  title: "Meeting 10/6/26",
  started_at: OCT6,
  title_suggestions: ["Q3 pricing review", "Pricing sync"],
} as MeetingSummary;

function renderHint(onRenamed = vi.fn()) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <TitleHint meeting={MEETING} hidden={false} onRenamed={onRenamed} />
    </QueryClientProvider>,
  );
  return onRenamed;
}

describe("TitleHint", () => {
  it("renames once however often Use is clicked, and reports the old title", async () => {
    const patch = vi.spyOn(meetingsApi, "patch").mockResolvedValue({ id: "m1" } as Meeting);
    const onRenamed = renderHint();
    const use = screen.getByRole("button", { name: "Use" });
    fireEvent.click(use);
    fireEvent.click(use);
    await waitFor(() => expect(onRenamed).toHaveBeenCalledWith("Meeting 10/6/26"));
    expect(patch).toHaveBeenCalledTimes(1);
    expect(patch).toHaveBeenCalledWith("m1", { title: "Q3 pricing review 10/6/26" });
  });

  it("shows why a dismiss failed", async () => {
    vi.spyOn(meetingsApi, "dismissTitleSuggestions").mockRejectedValue(new Error("offline"));
    renderHint();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss suggestion" }));
    expect(await screen.findByText("offline")).toBeInTheDocument();
  });
});
