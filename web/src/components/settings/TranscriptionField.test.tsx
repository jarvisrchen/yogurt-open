import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { TranscriptionField } from "./TranscriptionField";
import { settingsApi, type General } from "../../lib/api/settings";
import { startDownload } from "../../lib/api/stt";
import type { ModelView } from "./modelTypes";

vi.mock("../../lib/api/settings", async () => {
  const actual = await vi.importActual<typeof import("../../lib/api/settings")>("../../lib/api/settings");
  return { ...actual, settingsApi: { ...actual.settingsApi, patch: vi.fn() } };
});
vi.mock("../../lib/api/stt", async () => {
  const actual = await vi.importActual<typeof import("../../lib/api/stt")>("../../lib/api/stt");
  return { ...actual, useModels: () => ({ data: models }), startDownload: vi.fn() };
});
let averages: Record<string, number> = {};
let models: ModelView[] = [];
vi.mock("../../lib/api/voice", async () => {
  const actual = await vi.importActual<typeof import("../../lib/api/voice")>("../../lib/api/voice");
  return { ...actual, useVoiceHistory: () => ({ history: { stt_avg_ms: averages } }) };
});

const model = (name: ModelView["name"], size_mb: number, downloaded: boolean): ModelView => ({
  name,
  size_mb,
  downloaded,
  downloading: false,
  intel_supported: name !== "large-v3-turbo",
  managed_by_homebrew: false,
});

function renderField(overrides: Partial<General> = {}, deepgramKey = true) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <MemoryRouter>
      <QueryClientProvider client={qc}>
        <TranscriptionField
          general={{ stt_model: "small.en", voice_stt_model: "", ...overrides } as General}
          deepgramKey={deepgramKey}
        />
      </QueryClientProvider>
    </MemoryRouter>,
  );
}

describe("TranscriptionField", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    averages = {};
    vi.mocked(settingsApi.patch).mockResolvedValue({} as General);
    vi.mocked(startDownload).mockResolvedValue();
    models = [
      model("parakeet-v3", 670, false),
      model("small.en", 487, true),
      model("tiny.en", 75, true),
      model("large-v3-turbo", 1620, true),
    ];
  });

  it("picks Parakeet by default, marks it Recommended, and labels every model without seconds", async () => {
    renderField({ stt_model: "tiny.en" });
    expect(await screen.findByRole("radio", { name: /^parakeet-v3/ })).toBeChecked();
    expect(screen.getByRole("radio", { name: /Same as meetings/ })).not.toBeChecked();
    expect(screen.getByText("Now tiny.en")).toBeInTheDocument();
    expect(screen.getAllByText("Recommended")).toHaveLength(1);
    expect(screen.getByText("Fastest, and stays fast during meetings")).toBeInTheDocument();
    expect(screen.getByText("Fast, catches more names")).toBeInTheDocument();
    expect(screen.getByText("670 MB download")).toBeInTheDocument();
    expect(screen.getByText("1.3 GB RAM")).toBeInTheDocument();
    expect(screen.getByText(/Parakeet TDT 0\.6B v3 by NVIDIA, CC BY 4\.0/)).toBeInTheDocument();
    expect(screen.queryByText(/On your Mac/)).toBeNull();
  });

  it("never lists a retired model", () => {
    models = [...models, { ...model("medium.en", 1530, true), retired: true }];
    renderField();
    expect(screen.queryByRole("radio", { name: /^medium\.en/ })).toBeNull();
  });

  it("shows the average on this Mac only for models used for dictation", async () => {
    averages = { "small.en": 341 };
    renderField({ voice_stt_model: "small.en" });
    expect(await screen.findByText("On your Mac: 0.34 s average")).toBeInTheDocument();
    expect(screen.getAllByText(/On your Mac/)).toHaveLength(1);
    expect(screen.getByRole("radio", { name: /^small\.en/ })).toBeChecked();
  });

  it("checks only Same as meetings once it is picked, even when it names the same model", () => {
    renderField({ voice_stt_model: "meetings" });
    expect(screen.getByRole("radio", { name: /Same as meetings/ })).toBeChecked();
    expect(screen.getByRole("radio", { name: /^small\.en/ })).not.toBeChecked();
  });

  it("saves a picked model, and meetings for Same as meetings", async () => {
    renderField({ voice_stt_model: "small.en" });
    fireEvent.click(await screen.findByRole("radio", { name: /^large-v3-turbo/ }));
    await waitFor(() => expect(vi.mocked(settingsApi.patch)).toHaveBeenCalledWith({ voice_stt_model: "large-v3-turbo" }));
    fireEvent.click(screen.getByRole("radio", { name: /Same as meetings/ }));
    await waitFor(() => expect(vi.mocked(settingsApi.patch)).toHaveBeenLastCalledWith({ voice_stt_model: "meetings" }));
  });

  it("lists only the models this Mac can run", async () => {
    models = [model("small.en", 487, true), { ...model("large-v3-turbo", 1620, false), runs_here: false }];
    renderField();
    expect(await screen.findByRole("radio", { name: /^small\.en/ })).toBeInTheDocument();
    expect(screen.queryByRole("radio", { name: /^large-v3-turbo/ })).toBeNull();
  });

  it("keeps the default checked while it downloads", () => {
    models = [{ ...model("parakeet-v3", 670, false), downloading: true }];
    renderField();
    expect(screen.getByRole("radio", { name: /^parakeet-v3/ })).toBeChecked();
    expect(screen.getByRole("button", { name: "Show download progress for parakeet-v3" })).toBeInTheDocument();
  });

  it("disables Same as meetings while the model it follows is missing", () => {
    renderField({ stt_model: "parakeet-v3" });
    expect(screen.getByRole("radio", { name: /Same as meetings/ })).toBeDisabled();
  });

  it("hides a model this Mac cannot run even when Same as meetings follows it", () => {
    models = [model("small.en", 487, true), { ...model("large-v3-turbo", 1620, false), runs_here: false }];
    renderField({ voice_stt_model: "meetings", stt_model: "large-v3-turbo" });
    expect(screen.queryByRole("radio", { name: /^large-v3-turbo/ })).toBeNull();
  });

  it("offers Deepgram with the stored key, saves it, and labels its average as yours", async () => {
    averages = { deepgram: 312 };
    renderField({ voice_stt_model: "small.en" });
    const deepgram = screen.getByRole("radio", { name: /^Deepgram/ });
    expect(deepgram).toBeEnabled();
    expect(screen.getByText("Sends your audio to Deepgram")).toBeInTheDocument();
    expect(screen.getByText("Your average: 0.31 s")).toBeInTheDocument();
    expect(screen.queryByText(/On your Mac/)).toBeNull();
    fireEvent.click(deepgram);
    await waitFor(() => expect(vi.mocked(settingsApi.patch)).toHaveBeenCalledWith({ voice_stt_model: "deepgram" }));
  });

  it("checks Deepgram once it is picked", () => {
    renderField({ voice_stt_model: "deepgram" });
    expect(screen.getByRole("radio", { name: /^Deepgram/ })).toBeChecked();
    expect(screen.getByRole("radio", { name: /^parakeet-v3/ })).not.toBeChecked();
  });

  it("disables Deepgram without a key and links to where the key is set", () => {
    renderField({}, false);
    expect(screen.getByRole("radio", { name: /^Deepgram/ })).toBeDisabled();
    expect(screen.getByText(/No key stored yet/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Settings › Transcription" })).toHaveAttribute(
      "href",
      "/settings/transcription",
    );
  });

  it("downloads a missing model through the shared download flow instead of picking it", async () => {
    renderField();
    const parakeet = await screen.findByRole("radio", { name: /^parakeet-v3/ });
    expect(parakeet).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Download parakeet-v3" }));
    await waitFor(() => expect(vi.mocked(startDownload)).toHaveBeenCalledWith("parakeet-v3"));
    expect(await screen.findByRole("button", { name: "Show download progress for parakeet-v3" })).toBeInTheDocument();
    expect(vi.mocked(settingsApi.patch)).not.toHaveBeenCalled();
  });
});
