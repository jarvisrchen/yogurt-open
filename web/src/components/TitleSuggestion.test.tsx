import { describe, it, expect, afterEach, vi } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { createRef } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TitleSuggestion } from "./TitleSuggestion";
import { meetingsApi, type Meeting } from "../lib/api/meetings";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const OCT6 = new Date("2026-10-06T12:00:00").getTime();
const NAMES = ["Q3 pricing review", "Pro tier launch", "Pricing sync"];

function renderChip(names = NAMES) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <TitleSuggestion meetingId="m1" title="Meeting 10/6/26" startedAt={OCT6} names={names} titleRef={createRef()} />
    </QueryClientProvider>,
  );
}

describe("TitleSuggestion", () => {
  it("renames to the first name and keeps the date", async () => {
    const patch = vi.spyOn(meetingsApi, "patch").mockResolvedValue({ id: "m1" } as Meeting);
    renderChip();
    expect(screen.getByText("Q3 pricing review")).toBeInTheDocument();
    const use = screen.getByRole("button", { name: "Use" });
    fireEvent.click(use);
    fireEvent.click(use);
    await waitFor(() => expect(patch).toHaveBeenCalledWith("m1", { title: "Q3 pricing review 10/6/26" }));
    expect(patch).toHaveBeenCalledTimes(1);
  });

  it("offers every name in the menu", async () => {
    const patch = vi.spyOn(meetingsApi, "patch").mockResolvedValue({ id: "m1" } as Meeting);
    renderChip();
    fireEvent.click(screen.getByRole("button", { name: "Other titles" }));
    const items = screen.getAllByRole("menuitem");
    expect(items.map((i) => i.textContent)).toEqual(NAMES);
    fireEvent.click(items[2]);
    await waitFor(() => expect(patch).toHaveBeenCalledWith("m1", { title: "Pricing sync 10/6/26" }));
  });

  it("dismisses for good", async () => {
    const dismiss = vi.spyOn(meetingsApi, "dismissTitleSuggestions").mockResolvedValue(undefined);
    renderChip();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss suggestion" }));
    await waitFor(() => expect(dismiss).toHaveBeenCalledWith("m1"));
  });

  it("has no menu for a single name", () => {
    renderChip(["Q3 pricing review"]);
    expect(screen.queryByRole("button", { name: "Other titles" })).toBeNull();
  });

  it("brings the chip back with the error when the rename fails", async () => {
    vi.spyOn(meetingsApi, "patch").mockRejectedValue(new Error("500: boom"));
    renderChip();
    fireEvent.click(screen.getByRole("button", { name: "Use" }));
    expect(await screen.findByText("500: boom")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Use" })).toBeInTheDocument();
  });
});
