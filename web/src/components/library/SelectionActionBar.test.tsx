import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { SelectionActionBar } from "./SelectionActionBar";
import { useLabels, useCreateLabel } from "../../lib/api/labels";
import {
  useBulkDeleteMeetings,
  useBulkSetLabel,
  type Meeting,
} from "../../lib/api/meetings";

vi.mock("../../lib/entitlements", async (original) => ({
  ...(await original<typeof import("../../lib/entitlements")>()),
  useEntitlements: () => ({ pro: false, loading: false }),
}));

vi.mock("../../lib/api/labels", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api/labels")>()),
  useLabels: vi.fn(),
  useCreateLabel: vi.fn(),
}));

vi.mock("../../lib/api/meetings", () => ({
  useBulkDeleteMeetings: vi.fn(),
  useBulkSetLabel: vi.fn(),
}));

vi.mock("../../lib/api/settings", () => ({
  useDataPaths: () => ({
    data_dir: "/tmp/scratch",
    keys: "/tmp/scratch/keys.json",
    notes: "/tmp/scratch/notes",
  }),
}));

function meetings(n: number): Pick<Meeting, "id" | "title" | "labels">[] {
  return Array.from({ length: n }, (_, i) => ({ id: `m${i}`, title: `Meeting ${i}`, labels: [] }));
}

const bulkDeleteMutateAsync = vi.fn();

function renderBar(count: number, archived = false, activeId: string | null = null) {
  const onSelectAll = vi.fn();
  const onClear = vi.fn();
  render(
    <SelectionActionBar
      meetings={meetings(count)}
      onSelectAll={onSelectAll}
      onClear={onClear}
      archived={archived}
      activeId={activeId}
    />,
  );
  return { onSelectAll, onClear };
}

afterEach(() => cleanup());

describe("SelectionActionBar", () => {
  beforeEach(() => {
    vi.mocked(useLabels).mockReturnValue({ data: [] } as unknown as ReturnType<typeof useLabels>);
    vi.mocked(useCreateLabel).mockReturnValue({
      mutateAsync: vi.fn(),
    } as unknown as ReturnType<typeof useCreateLabel>);
    vi.mocked(useBulkSetLabel).mockReturnValue({
      mutate: vi.fn(),
    } as unknown as ReturnType<typeof useBulkSetLabel>);
    bulkDeleteMutateAsync.mockReset();
    bulkDeleteMutateAsync.mockResolvedValue(undefined);
    vi.mocked(useBulkDeleteMeetings).mockReturnValue({
      mutateAsync: bulkDeleteMutateAsync,
      isPending: false,
    } as unknown as ReturnType<typeof useBulkDeleteMeetings>);
  });

  it("shows the selected count", () => {
    renderBar(3);
    expect(screen.getByText("3 selected")).toBeInTheDocument();
  });

  it("Clear invokes onClear", () => {
    const { onClear } = renderBar(2);
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it("Select all invokes onSelectAll", () => {
    const { onSelectAll } = renderBar(2);
    fireEvent.click(screen.getByRole("button", { name: "Select all" }));
    expect(onSelectAll).toHaveBeenCalledTimes(1);
  });

  it("Delete requires a confirm step before the bulk mutation fires", () => {
    renderBar(2);
    expect(screen.queryByTestId("bulk-delete-confirm-popover")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.getByTestId("bulk-delete-confirm-popover")).toBeInTheDocument();
    expect(bulkDeleteMutateAsync).not.toHaveBeenCalled();
  });

  it("confirming Delete? fires the bulk mutation with every selected id", async () => {
    renderBar(2);
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    // ConfirmPanel buttons carry `role="menuitem"`.
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete?" }));
    expect(bulkDeleteMutateAsync).toHaveBeenCalledWith({
      ids: ["m0", "m1"],
      deleteFile: true,
    });
  });

  it("Cancel on the confirm step reverts to the plain Delete button", () => {
    renderBar(1);
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Cancel" }));
    expect(screen.queryByTestId("bulk-delete-confirm-popover")).toBeNull();
    expect(screen.getByRole("button", { name: "Delete" })).toBeInTheDocument();
  });

  it("Add label opens the bulk label picker", () => {
    renderBar(2);
    fireEvent.click(screen.getByRole("button", { name: "Add label" }));
    expect(screen.getByPlaceholderText("Search or create label")).toBeInTheDocument();
  });

  it("Add hides archived labels unless the selection is archived, and Remove lists them", () => {
    vi.mocked(useLabels).mockReturnValue({
      data: [
        { id: "l1", name: "Sales", color: "blue", meeting_count: 2, archived_count: 0 },
        { id: "l2", name: "GEICO", color: "honey", meeting_count: 3, archived_count: 3 },
      ],
    } as unknown as ReturnType<typeof useLabels>);
    renderBar(2);
    fireEvent.click(screen.getByRole("button", { name: "Add label" }));
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["Sales"]);

    fireEvent.click(screen.getByRole("button", { name: "Remove label" }));
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["Sales", "GEICO"]);
    cleanup();

    renderBar(2, true);
    fireEvent.click(screen.getByRole("button", { name: "Add label" }));
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["Sales", "GEICO"]);
  });
});
