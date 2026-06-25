import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { CleanupField } from "./CleanupField";
import { settingsApi, type General, type ProviderView } from "../../lib/api/settings";
import { testCleanup } from "../../lib/api/voice";
import { fetchModel, startDownload, sttKeys, type ModelView } from "../../lib/api/stt";
import { applyDownloadEvent, dismissDownload } from "../../hooks/useModelDownload";

vi.mock("../../lib/api/settings", async () => {
  const actual = await vi.importActual<typeof import("../../lib/api/settings")>(
    "../../lib/api/settings",
  );
  return { ...actual, settingsApi: { ...actual.settingsApi, patch: vi.fn() } };
});
vi.mock("../../lib/api/voice", async () => {
  const actual = await vi.importActual<typeof import("../../lib/api/voice")>(
    "../../lib/api/voice",
  );
  return {
    ...actual,
    testCleanup: vi.fn(),
    useVoiceHistory: () => ({ history: { cleanup_avg_ms: averages } }),
  };
});

let averages: Record<string, number> = {};

vi.mock("../../lib/api/stt", async () => {
  const actual = await vi.importActual<typeof import("../../lib/api/stt")>("../../lib/api/stt");
  const fetchModel = vi.fn();
  // `useModel` calls the module's own `fetchModel`, which the mock cannot reach, so it is rebuilt over the mocked one.
  const useModel = (name: string) =>
    useQuery({ queryKey: [...actual.sttKeys.models, name], queryFn: () => fetchModel(name) });
  return { ...actual, fetchModel, startDownload: vi.fn(), useModel };
});

function qwen(v: Partial<ModelView> = {}): ModelView {
  return {
    name: "qwen3-4b-instruct",
    size_mb: 2_497,
    downloaded: false,
    downloading: false,
    intel_supported: true,
    managed_by_homebrew: false,
    ...v,
  } as ModelView;
}

const RAW = "so um I think we should we should move the launch";

function provider(p: Partial<ProviderView>): ProviderView {
  return {
    id: "p1",
    name: "Gemini",
    base_url: "https://example.test/v1",
    model: "gemini-3.5-flash-lite",
    is_active: false,
    created_at: 0,
    api_key_masked: "••••abcd",
    adapter: "http",
    cli_model: "",
    has_model_catalog: true,
    ...p,
  };
}

const PROVIDERS = [
  provider({}),
  provider({ id: "p3", name: "Acme", model: "deepseek-r9" }),
  provider({ id: "p2", name: "Claude Code", model: "claude", adapter: "cli", api_key_masked: null }),
];

function renderField(overrides: Partial<General> = {}) {
  const general = {
    voice_cleanup: true,
    voice_cleanup_provider: "p1",
    ...overrides,
  } as General;
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={qc}>
      <CleanupField general={general} providers={PROVIDERS} />
    </QueryClientProvider>,
  );
  return qc;
}

describe("CleanupField", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    averages = {};
    vi.mocked(settingsApi.patch).mockResolvedValue({} as General);
    vi.mocked(fetchModel).mockResolvedValue(qwen());
    vi.mocked(startDownload).mockResolvedValue();
    dismissDownload();
  });

  it("hides the picker while cleanup is off and turns it on through the patch", async () => {
    renderField({ voice_cleanup: false });
    expect(screen.queryByRole("radiogroup", { name: "Cleanup model" })).toBeNull();
    fireEvent.click(screen.getByLabelText("Clean up the text with an LLM"));
    await waitFor(() =>
      expect(vi.mocked(settingsApi.patch).mock.calls[0][0]).toEqual({ voice_cleanup: true }),
    );
  });

  it("lists the built-in model first, then each provider, with a CLI one unpickable", async () => {
    vi.mocked(fetchModel).mockResolvedValue(qwen({ downloaded: true }));
    renderField();
    const group = screen.getByRole("radiogroup", { name: "Cleanup model" });
    const radios = within(group).getAllByRole("radio");
    await waitFor(() => expect(radios[0]).toBeEnabled());
    expect(radios).toHaveLength(4);
    [/^Qwen3 4B Instruct/, /^gemini-3\.5-flash-lite/, /^deepseek-r9/, /^Claude Code/].forEach((name, i) =>
      expect(radios[i]).toHaveAccessibleName(name),
    );
    expect(radios.map((r) => (r as HTMLInputElement).disabled)).toEqual([false, false, false, true]);
    expect(within(group).getByText("Runs on this Mac")).toBeInTheDocument();
    expect(within(group).getByText("Fastest on short dictations, slower on long ones")).toBeInTheDocument();
    expect(within(group).getByText("2.5 GB download")).toBeInTheDocument();
    expect(within(group).getByText("3.0 GB RAM")).toBeInTheDocument();
    expect(within(group).getByText("Sends the text to Gemini")).toBeInTheDocument();
    expect(within(group).getByText("About 0.85 s at any length")).toBeInTheDocument();
    expect(within(group).getByText("Not measured. Test shows its time.")).toBeInTheDocument();
  });

  it("shows each model's own average, and none for a model with no saved dictations", async () => {
    averages = { "qwen3-4b-instruct": 423, "gemini-3.5-flash-lite": 861 };
    vi.mocked(fetchModel).mockResolvedValue(qwen({ downloaded: true }));
    renderField();
    expect(screen.getByText("On your Mac: 0.42 s average")).toBeInTheDocument();
    expect(screen.getByText("Your average: 0.86 s")).toBeInTheDocument();
    expect(screen.getAllByText(/average/)).toHaveLength(2);
  });

  it("puts Test on the picked row only", async () => {
    vi.mocked(fetchModel).mockResolvedValue(qwen({ downloaded: true }));
    renderField();
    const test = screen.getByRole("button", { name: "Test" });
    expect(test.closest("div")).toHaveTextContent("gemini-3.5-flash-lite");
    expect(screen.getAllByRole("button", { name: "Test" })).toHaveLength(1);
  });

  it("offers the built-in model's download in its row, which cannot be picked until it is on disk", async () => {
    renderField({ voice_cleanup_provider: "builtin" });
    expect(await screen.findByRole("button", { name: "Download Qwen3 4B Instruct" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /Qwen3 4B Instruct/ })).toBeDisabled();
    expect(screen.getByText("Until the download finishes, dictation pastes the raw text.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Test" })).toBeNull();
  });

  it("says when it cannot read the built-in model's status, and offers no Test", async () => {
    vi.mocked(fetchModel).mockRejectedValue(new Error("500 Internal Server Error: boom"));
    renderField({ voice_cleanup_provider: "builtin" });
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not read the built-in model's status: 500 Internal Server Error: boom",
    );
    expect(screen.queryByRole("button", { name: "Test" })).toBeNull();
  });

  it("shows the download's progress in the row and offers Test once the model is on disk", async () => {
    const qc = renderField({ voice_cleanup_provider: "builtin" });
    fireEvent.click(await screen.findByRole("button", { name: "Download Qwen3 4B Instruct" }));
    expect(startDownload).toHaveBeenCalledWith("qwen3-4b-instruct");
    expect(
      await screen.findByRole("button", { name: "Show download progress for qwen3-4b-instruct" }),
    ).toBeInTheDocument();

    vi.mocked(fetchModel).mockResolvedValue(qwen({ downloaded: true }));
    act(() =>
      applyDownloadEvent({ type: "stt_model_download_complete", model: "qwen3-4b-instruct" }, () =>
        qc.invalidateQueries({ queryKey: sttKeys.models }),
      ),
    );
    await waitFor(() => expect(screen.getByRole("button", { name: "Test" })).toBeEnabled());
    expect(screen.getByRole("radio", { name: /Qwen3 4B Instruct/ })).toBeEnabled();
  });

  it("saves a picked provider", async () => {
    renderField({ voice_cleanup_provider: "builtin" });
    fireEvent.click(screen.getByRole("radio", { name: /gemini-3\.5-flash-lite/ }));
    await waitFor(() =>
      expect(vi.mocked(settingsApi.patch).mock.calls[0][0]).toEqual({
        voice_cleanup_provider: "p1",
      }),
    );
  });

  it("says when the picked provider no longer exists", async () => {
    vi.mocked(fetchModel).mockResolvedValue(qwen({ downloaded: true }));
    renderField({ voice_cleanup_provider: "deleted" });
    expect(screen.getByRole("alert")).toHaveTextContent("no longer in Settings › Model");
    expect(screen.getAllByRole("radio").filter((r) => (r as HTMLInputElement).checked)).toHaveLength(0);
  });

  it("shows no download notice while the built-in model's status is loading", () => {
    vi.mocked(fetchModel).mockReturnValue(new Promise(() => {}));
    renderField({ voice_cleanup_provider: "builtin" });
    expect(screen.queryByText(/Until the download finishes/)).toBeNull();
  });

  it("shows the time and the before and after of a passing test", async () => {
    vi.mocked(testCleanup).mockResolvedValue({
      outcome: "done",
      raw: RAW,
      words: 45,
      total_ms: 2_625,
      ms: 846,
      model: "gemini-flash-lite",
      text: "So I think we should move the launch.",
    });
    renderField();
    fireEvent.click(screen.getByRole("button", { name: "Test" }));
    expect(await screen.findByText("846 ms")).toBeInTheDocument();
    expect(screen.getByText("on a 45-word sample")).toBeInTheDocument();
    expect(screen.getByText(RAW)).toBeInTheDocument();
    expect(screen.getByText("So I think we should move the launch.")).toBeInTheDocument();
  });

  it("names the broken limit when the model is too slow", async () => {
    vi.mocked(testCleanup).mockResolvedValue({
      outcome: "too_slow",
      raw: RAW,
      words: 45,
      total_ms: 2_625,
      ms: 3_875,
      model: "MiniMax-M3",
      limit: "first_token",
    });
    renderField();
    fireEvent.click(screen.getByRole("button", { name: "Test" }));
    expect(await screen.findByText("3.9 s")).toBeInTheDocument();
    expect(
      screen.getByText(
        "MiniMax-M3 had not started answering after 1.5 s, so most dictations would paste the raw text. Pick a faster model.",
      ),
    ).toBeInTheDocument();
  });

  it("states the total limit for the sample's length", async () => {
    vi.mocked(testCleanup).mockResolvedValue({
      outcome: "too_slow",
      raw: RAW,
      words: 45,
      total_ms: 2_625,
      ms: 3_100,
      model: "slow",
      limit: "total",
    });
    renderField();
    fireEvent.click(screen.getByRole("button", { name: "Test" }));
    expect(
      await screen.findByText(/slow took longer than the 2\.6 s a 45-word dictation gets/),
    ).toBeInTheDocument();
  });

  it("shows a provider error as a sentence", async () => {
    vi.mocked(testCleanup).mockResolvedValue({
      outcome: "failed",
      raw: RAW,
      words: 45,
      total_ms: 2_625,
      ms: 120,
      error: "LLM stream open failed: 401 Unauthorized",
    });
    renderField();
    fireEvent.click(screen.getByRole("button", { name: "Test" }));
    expect(await screen.findByText("Failed")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "LLM stream open failed: 401 Unauthorized.",
    );
  });
});
