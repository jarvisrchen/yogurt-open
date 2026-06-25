import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Sidebar } from "./Sidebar";
import {
  useCreateLabel,
  useDeleteLabel,
  useLabels,
  useReorderLabels,
  useUpdateLabel,
} from "../../lib/api/labels";
import { useCreateMeeting } from "../../lib/api/meetings";
import { settingsApi } from "../../lib/api/settings";
import { LABEL_SORT_KEY } from "../../lib/labelSort";

vi.mock("../../lib/api/labels", async (importOriginal) => ({
  isArchivedLabel: (await importOriginal<typeof import("../../lib/api/labels")>()).isArchivedLabel,
  useLabels: vi.fn(),
  useUpdateLabel: vi.fn(),
  useDeleteLabel: vi.fn(),
  useCreateLabel: vi.fn(),
  useReorderLabels: vi.fn(),
}));

vi.mock("../../lib/api/meetings", () => ({
  useCreateMeeting: vi.fn(),
}));

vi.mock("../../lib/entitlements", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/entitlements")>()),
  useEntitlements: () => ({ pro: false, loading: false }),
}));

vi.mock("../../lib/api/settings", () => ({
  settingsApi: { get: vi.fn() },
}));

afterEach(() => cleanup());

function renderSidebar(path = "/") {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={[path]}>
        <Sidebar />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("Sidebar — Labels section", () => {
  it("renders labels with counts and links to /label/:id", () => {
    vi.mocked(useLabels).mockReturnValue({
      data: [
        { id: "l1", name: "Sales", color: "blue", meeting_count: 3, archived_count: 0 },
        { id: "l2", name: "Support", color: "matcha", meeting_count: 0, archived_count: 0 },
      ],
    } as unknown as ReturnType<typeof useLabels>);
    vi.mocked(useUpdateLabel).mockReturnValue({
      mutate: vi.fn(),
    } as unknown as ReturnType<typeof useUpdateLabel>);
    vi.mocked(useDeleteLabel).mockReturnValue({
      mutate: vi.fn(),
      mutateAsync: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useDeleteLabel>);
    vi.mocked(useCreateMeeting).mockReturnValue({
      mutateAsync: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useCreateMeeting>);
    vi.mocked(useCreateLabel).mockReturnValue({
      mutate: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useCreateLabel>);
    vi.mocked(useReorderLabels).mockReturnValue({
      mutate: vi.fn(),
    } as unknown as ReturnType<typeof useReorderLabels>);
    vi.mocked(settingsApi.get).mockResolvedValue({ providers: [] } as never);

    renderSidebar();

    expect(screen.getByText("Sales")).toBeInTheDocument();
    expect(screen.getByText("3")).toBeInTheDocument();
    expect(screen.getByText("Support")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Sales/ })).toHaveAttribute(
      "href",
      "/label/l1",
    );
  });

  it("opens the row menu from the kebab without navigating", () => {
    vi.mocked(useLabels).mockReturnValue({
      data: [{ id: "l1", name: "Sales", color: "blue", meeting_count: 3, archived_count: 0 }],
    } as unknown as ReturnType<typeof useLabels>);
    vi.mocked(useUpdateLabel).mockReturnValue({
      mutate: vi.fn(),
    } as unknown as ReturnType<typeof useUpdateLabel>);
    vi.mocked(useDeleteLabel).mockReturnValue({
      mutate: vi.fn(),
      mutateAsync: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useDeleteLabel>);
    vi.mocked(useCreateMeeting).mockReturnValue({
      mutateAsync: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useCreateMeeting>);
    vi.mocked(useCreateLabel).mockReturnValue({
      mutate: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useCreateLabel>);
    vi.mocked(useReorderLabels).mockReturnValue({
      mutate: vi.fn(),
    } as unknown as ReturnType<typeof useReorderLabels>);
    vi.mocked(settingsApi.get).mockResolvedValue({ providers: [] } as never);

    renderSidebar();

    fireEvent.click(screen.getByRole("button", { name: "Sales label options" }));
    expect(screen.getByRole("menuitem", { name: "Rename" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Delete" })).toBeInTheDocument();
    const menu = screen.getByRole("menu");
    expect(menu.parentElement?.parentElement?.className).not.toMatch(/overflow/);

  });

  it("commits a valid custom hex on Enter but not an invalid one on blur", () => {
    vi.mocked(useLabels).mockReturnValue({
      data: [{ id: "l1", name: "Sales", color: "blue", meeting_count: 3, archived_count: 0 }],
    } as unknown as ReturnType<typeof useLabels>);
    const mutate = vi.fn();
    vi.mocked(useUpdateLabel).mockReturnValue({
      mutate,
    } as unknown as ReturnType<typeof useUpdateLabel>);
    vi.mocked(useDeleteLabel).mockReturnValue({
      mutate: vi.fn(),
      mutateAsync: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useDeleteLabel>);
    vi.mocked(useCreateMeeting).mockReturnValue({
      mutateAsync: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useCreateMeeting>);
    vi.mocked(useCreateLabel).mockReturnValue({
      mutate: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useCreateLabel>);
    vi.mocked(settingsApi.get).mockResolvedValue({ providers: [] } as never);

    renderSidebar();

    fireEvent.click(screen.getByRole("button", { name: "Sales label options" }));
    const hexInput = screen.getByRole("textbox", { name: "Custom label color hex" });

    fireEvent.change(hexInput, { target: { value: "not-a-color" } });
    fireEvent.blur(hexInput);
    expect(mutate).not.toHaveBeenCalled();

    fireEvent.change(hexInput, { target: { value: "#A3C9FF" } });
    fireEvent.keyDown(hexInput, { key: "Enter" });
    expect(mutate).toHaveBeenCalledWith({ id: "l1", color: "#A3C9FF" });
  });

  it("shows a muted line when there are no labels", () => {
    vi.mocked(useLabels).mockReturnValue({ data: [] } as unknown as ReturnType<
      typeof useLabels
    >);
    vi.mocked(useUpdateLabel).mockReturnValue({
      mutate: vi.fn(),
    } as unknown as ReturnType<typeof useUpdateLabel>);
    vi.mocked(useDeleteLabel).mockReturnValue({
      mutate: vi.fn(),
      mutateAsync: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useDeleteLabel>);
    vi.mocked(useCreateMeeting).mockReturnValue({
      mutateAsync: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useCreateMeeting>);
    vi.mocked(useCreateLabel).mockReturnValue({
      mutate: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useCreateLabel>);
    vi.mocked(useReorderLabels).mockReturnValue({
      mutate: vi.fn(),
    } as unknown as ReturnType<typeof useReorderLabels>);
    vi.mocked(settingsApi.get).mockResolvedValue({ providers: [] } as never);

    renderSidebar();

    expect(screen.getByText("No labels yet")).toBeInTheDocument();
  });

  it("without Pro and with five labels, + shows the count and a Pro link in place of the input", () => {
    vi.mocked(useLabels).mockReturnValue({
      data: [1, 2, 3, 4, 5].map((n) => ({ id: `l${n}`, name: `Label ${n}`, color: "blue", meeting_count: 1, archived_count: 0 })),
    } as unknown as ReturnType<
      typeof useLabels
    >);
    vi.mocked(useUpdateLabel).mockReturnValue({
      mutate: vi.fn(),
    } as unknown as ReturnType<typeof useUpdateLabel>);
    vi.mocked(useDeleteLabel).mockReturnValue({
      mutate: vi.fn(),
      mutateAsync: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useDeleteLabel>);
    vi.mocked(useCreateMeeting).mockReturnValue({
      mutateAsync: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useCreateMeeting>);
    const mutate = vi.fn();
    vi.mocked(useCreateLabel).mockReturnValue({
      mutate,
      isPending: false,
    } as unknown as ReturnType<typeof useCreateLabel>);
    vi.mocked(useReorderLabels).mockReturnValue({
      mutate: vi.fn(),
    } as unknown as ReturnType<typeof useReorderLabels>);
    vi.mocked(settingsApi.get).mockResolvedValue({ providers: [] } as never);

    renderSidebar();

    fireEvent.click(screen.getByRole("button", { name: "New label" }));
    expect(screen.getByTestId("label-cap")).toHaveTextContent("5 of 5 labels");
    expect(screen.queryByPlaceholderText("Label name")).not.toBeInTheDocument();
    expect(screen.getAllByRole("link", { name: /Label \d/ })).toHaveLength(5);
    fireEvent.click(screen.getByRole("button", { name: "New label" }));
    expect(screen.queryByTestId("label-cap")).not.toBeInTheDocument();
  });

  it("creates a label via the + affordance", () => {
    vi.mocked(useLabels).mockReturnValue({ data: [] } as unknown as ReturnType<
      typeof useLabels
    >);
    vi.mocked(useUpdateLabel).mockReturnValue({
      mutate: vi.fn(),
    } as unknown as ReturnType<typeof useUpdateLabel>);
    vi.mocked(useDeleteLabel).mockReturnValue({
      mutate: vi.fn(),
      mutateAsync: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useDeleteLabel>);
    vi.mocked(useCreateMeeting).mockReturnValue({
      mutateAsync: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useCreateMeeting>);
    const mutate = vi.fn();
    vi.mocked(useCreateLabel).mockReturnValue({
      mutate,
      isPending: false,
    } as unknown as ReturnType<typeof useCreateLabel>);
    vi.mocked(useReorderLabels).mockReturnValue({
      mutate: vi.fn(),
    } as unknown as ReturnType<typeof useReorderLabels>);
    vi.mocked(settingsApi.get).mockResolvedValue({ providers: [] } as never);

    renderSidebar();

    expect(screen.queryByPlaceholderText("Label name")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "New label" }));
    const input = screen.getByPlaceholderText("Label name");

    fireEvent.change(input, { target: { value: "  Sales  " } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(mutate).toHaveBeenCalledWith({ name: "Sales" }, expect.anything());
    expect(screen.queryByPlaceholderText("Label name")).not.toBeInTheDocument();
  });

  it("closes the new-label input on Escape without creating", () => {
    vi.mocked(useLabels).mockReturnValue({ data: [] } as unknown as ReturnType<
      typeof useLabels
    >);
    vi.mocked(useUpdateLabel).mockReturnValue({
      mutate: vi.fn(),
    } as unknown as ReturnType<typeof useUpdateLabel>);
    vi.mocked(useDeleteLabel).mockReturnValue({
      mutate: vi.fn(),
      mutateAsync: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useDeleteLabel>);
    vi.mocked(useCreateMeeting).mockReturnValue({
      mutateAsync: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useCreateMeeting>);
    const mutate = vi.fn();
    vi.mocked(useCreateLabel).mockReturnValue({
      mutate,
      isPending: false,
    } as unknown as ReturnType<typeof useCreateLabel>);
    vi.mocked(useReorderLabels).mockReturnValue({
      mutate: vi.fn(),
    } as unknown as ReturnType<typeof useReorderLabels>);
    vi.mocked(settingsApi.get).mockResolvedValue({ providers: [] } as never);

    renderSidebar();

    fireEvent.click(screen.getByRole("button", { name: "New label" }));
    const input = screen.getByPlaceholderText("Label name");
    fireEvent.change(input, { target: { value: "Draft" } });
    fireEvent.keyDown(input, { key: "Escape" });

    expect(mutate).not.toHaveBeenCalled();
    expect(screen.queryByPlaceholderText("Label name")).not.toBeInTheDocument();
  });

  it("drags a label to a new position in Custom mode and calls reorder", () => {
    localStorage.setItem(LABEL_SORT_KEY, "custom");
    vi.mocked(useLabels).mockReturnValue({
      data: [
        { id: "l1", name: "Sales", color: "blue", meeting_count: 3, archived_count: 0, position: 0, updated_at: 1 },
        { id: "l2", name: "Support", color: "matcha", meeting_count: 0, archived_count: 0, position: 1, updated_at: 2 },
        { id: "l3", name: "Zeta", color: "straw", meeting_count: 0, archived_count: 0, position: 2, updated_at: 3 },
        { id: "l4", name: "Old", color: "slate", meeting_count: 2, archived_count: 2, position: 3, updated_at: 4 },
      ],
    } as unknown as ReturnType<typeof useLabels>);
    vi.mocked(useUpdateLabel).mockReturnValue({
      mutate: vi.fn(),
    } as unknown as ReturnType<typeof useUpdateLabel>);
    vi.mocked(useDeleteLabel).mockReturnValue({
      mutate: vi.fn(),
      mutateAsync: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useDeleteLabel>);
    vi.mocked(useCreateMeeting).mockReturnValue({
      mutateAsync: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useCreateMeeting>);
    vi.mocked(useCreateLabel).mockReturnValue({
      mutate: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useCreateLabel>);
    const reorder = vi.fn();
    vi.mocked(useReorderLabels).mockReturnValue({
      mutate: reorder,
    } as unknown as ReturnType<typeof useReorderLabels>);
    vi.mocked(settingsApi.get).mockResolvedValue({ providers: [] } as never);

    renderSidebar();

    const wrapperOf = (name: string) =>
      screen.getByText(name).closest('[draggable="true"]') as HTMLElement;

    fireEvent.dragStart(wrapperOf("Zeta"));
    fireEvent.dragOver(wrapperOf("Sales"));
    fireEvent.drop(wrapperOf("Sales"));

    expect(reorder).toHaveBeenCalledWith(["l3", "l1", "l2", "l4"]);

    localStorage.removeItem(LABEL_SORT_KEY);
  });
});
