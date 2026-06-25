import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { LabelPicker } from "./LabelPicker";
import { useCreateLabel, useLabels, type Label } from "../../lib/api/labels";
import { useSetMeetingLabels } from "../../lib/api/meetings";

vi.mock("../../lib/api/labels", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api/labels")>()),
  useLabels: vi.fn(),
  useCreateLabel: vi.fn(),
}));

const entitlements = { pro: false, loading: false };
vi.mock("../../lib/entitlements", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/entitlements")>()),
  useEntitlements: () => entitlements,
}));

vi.mock("../../lib/api/meetings", () => ({
  useSetMeetingLabels: vi.fn(),
}));

const LABELS = [
  { id: "l1", name: "Sales", color: "blue" as const, meeting_count: 2, archived_count: 1 },
  { id: "l2", name: "Support", color: "matcha" as const, meeting_count: 0, archived_count: 0 },
  { id: "l3", name: "GEICO", color: "honey" as const, meeting_count: 3, archived_count: 3 },
];

const setMutate = vi.fn();
const createMutateAsync = vi.fn();

function renderPicker(selected: Label[] = [{ id: "l1", name: "Sales", color: "blue" }]) {
  return render(
    <LabelPicker
      meetingId="meeting-1"
      selected={selected}
      open
      onClose={() => {}}
    />,
  );
}

afterEach(() => cleanup());

describe("LabelPicker", () => {
  beforeEach(() => {
    entitlements.pro = false;
    vi.mocked(useLabels).mockReturnValue({ data: LABELS } as ReturnType<typeof useLabels>);
    vi.mocked(useCreateLabel).mockReturnValue({
      mutateAsync: createMutateAsync,
    } as unknown as ReturnType<typeof useCreateLabel>);
    vi.mocked(useSetMeetingLabels).mockReturnValue({
      mutate: setMutate,
    } as unknown as ReturnType<typeof useSetMeetingLabels>);
    setMutate.mockClear();
    createMutateAsync.mockReset();
    createMutateAsync.mockResolvedValue({ id: "new-1", name: "Marketing", color: "honey" });
  });

  describe("with five labels", () => {
    const FIVE = [1, 2, 3, 4, 5].map((n) => ({
      id: `f${n}`,
      name: `Label ${n}`,
      color: "blue" as const,
      meeting_count: 1,
      archived_count: 0,
    }));
    beforeEach(() => {
      vi.mocked(useLabels).mockReturnValue({ data: FIVE } as ReturnType<typeof useLabels>);
    });

    it("without Pro, a new name shows the count and a Pro link, and Enter creates nothing", () => {
      const open = vi.spyOn(window, "open").mockReturnValue(null);
      renderPicker([]);
      const input = screen.getByPlaceholderText("Search or create label");
      fireEvent.change(input, { target: { value: "Marketing" } });
      expect(screen.getByTestId("label-cap")).toHaveTextContent("5 of 5 labels");
      expect(screen.queryByText('Create "Marketing"')).toBeNull();
      fireEvent.keyDown(input, { key: "Enter" });
      expect(createMutateAsync).not.toHaveBeenCalled();
      fireEvent.click(screen.getByRole("link", { name: "Get Pro for more" }));
      expect(open).toHaveBeenCalledWith("https://yogurt-ai.dev", "_blank", "noreferrer");
      open.mockRestore();
    });

    it("without Pro, every label still lists and applies", () => {
      renderPicker([]);
      expect(screen.getAllByRole("option")).toHaveLength(5);
      fireEvent.click(screen.getByRole("option", { name: /Label 5/ }));
      expect(setMutate).toHaveBeenCalledWith({ id: "meeting-1", label_ids: ["f5"] });
    });

    it("words the count differently once Pro labels put it past five", () => {
      const seven = [...FIVE, { ...FIVE[0], id: "f6", name: "Label 6" }, { ...FIVE[0], id: "f7", name: "Label 7" }];
      vi.mocked(useLabels).mockReturnValue({ data: seven } as ReturnType<typeof useLabels>);
      renderPicker([]);
      fireEvent.change(screen.getByPlaceholderText("Search or create label"), { target: { value: "Marketing" } });
      expect(screen.getByTestId("label-cap")).toHaveTextContent("7 labels, over the free 5.");
    });

    it("with Pro, a new name still offers Create", () => {
      entitlements.pro = true;
      renderPicker([]);
      fireEvent.change(screen.getByPlaceholderText("Search or create label"), { target: { value: "Marketing" } });
      expect(screen.getByText('Create "Marketing"')).toBeInTheDocument();
      expect(screen.queryByTestId("label-cap")).toBeNull();
    });
  });

  it("renders existing labels and marks the selected one checked", () => {
    renderPicker();
    const sales = screen.getByRole("option", { name: /Sales/ });
    const support = screen.getByRole("option", { name: /Support/ });
    expect(sales).toHaveAttribute("aria-selected", "true");
    expect(support).toHaveAttribute("aria-selected", "false");
  });

  it('shows Create "…" for a query with no match, and Enter creates then applies it', async () => {
    renderPicker();
    const input = screen.getByPlaceholderText("Search or create label");
    fireEvent.change(input, { target: { value: "Marketing" } });
    const createRow = await screen.findByText('Create "Marketing"');
    expect(createRow).toBeInTheDocument();

    fireEvent.keyDown(input, { key: "Enter" });

    expect(createMutateAsync).toHaveBeenCalledWith({ name: "Marketing" });
    await waitFor(() =>
      expect(setMutate).toHaveBeenCalledWith({
        id: "meeting-1",
        label_ids: ["l1", "new-1"],
      }),
    );
  });

  it("adds an unselected label and removes a selected one on click", () => {
    renderPicker();
    fireEvent.click(screen.getByRole("option", { name: /Support/ }));
    expect(setMutate).toHaveBeenCalledWith({
      id: "meeting-1",
      label_ids: ["l1", "l2"],
    });

    fireEvent.click(screen.getByRole("option", { name: /Sales/ }));
    expect(setMutate).toHaveBeenCalledWith({
      id: "meeting-1",
      label_ids: [],
    });
  });

  it("leaves out an archived label until the query finds it, and picking it applies that label", () => {
    renderPicker();
    expect(screen.queryByRole("option", { name: /GEICO/ })).not.toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText("Search or create label"), { target: { value: "gei" } });
    const geico = screen.getByRole("option", { name: /GEICO/ });
    expect(geico).toHaveTextContent("Archived");

    fireEvent.change(screen.getByPlaceholderText("Search or create label"), { target: { value: "geico" } });
    expect(screen.queryByText(/^Create/)).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("option", { name: /GEICO/ }));
    expect(setMutate).toHaveBeenCalledWith({ id: "meeting-1", label_ids: ["l1", "l3"] });
  });

  it("keeps an archived label's row after unchecking it, so the click can be undone", () => {
    const { rerender } = renderPicker([{ id: "l3", name: "GEICO", color: "honey" as const }]);
    fireEvent.click(screen.getByRole("option", { name: /GEICO/ }));
    expect(setMutate).toHaveBeenCalledWith({ id: "meeting-1", label_ids: [] });

    rerender(<LabelPicker meetingId="meeting-1" selected={[]} open onClose={() => {}} />);
    expect(screen.getByRole("option", { name: /GEICO/ })).toHaveAttribute("aria-selected", "false");
  });

  it("lists archived labels for an archived meeting", () => {
    render(<LabelPicker meetingId="meeting-1" selected={[]} archived open onClose={() => {}} />);
    const geico = screen.getByRole("option", { name: /GEICO/ });
    expect(geico).not.toHaveTextContent("Archived");
  });
});
