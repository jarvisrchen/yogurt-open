import { describe, it, expect, vi, beforeEach } from "vitest";
import { cleanup, render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { VoiceSection } from "./VoiceSection";
import { audioApi, settingsApi, type General, type SettingsView } from "../../lib/api/settings";
import { fetchRecorded, fetchVoiceStatus, recordShortcut } from "../../lib/api/voice";
import { startDownload } from "../../lib/api/stt";
import type { ModelView } from "./modelTypes";

vi.mock("../../lib/api/settings", async () => {
  const actual = await vi.importActual<typeof import("../../lib/api/settings")>(
    "../../lib/api/settings",
  );
  return {
    ...actual,
    audioApi: { ...actual.audioApi, devices: vi.fn() },
    settingsApi: { ...actual.settingsApi, get: vi.fn(), patch: vi.fn() },
  };
});
let voiceHistory = { history: { count: 3, stt_avg_ms: {} }, window_hours: null as number | null };
vi.mock("../../lib/api/voice", async () => {
  const actual = await vi.importActual<typeof import("../../lib/api/voice")>(
    "../../lib/api/voice",
  );
  return {
    ...actual,
    fetchVoiceStatus: vi.fn(),
    recordShortcut: vi.fn(),
    fetchRecorded: vi.fn(),
    useVoiceHistory: () => voiceHistory,
    useDeleteAllDictations: () => deleteAll,
  };
});
let models: ModelView[] = [];
vi.mock("../../lib/api/stt", async () => {
  const actual = await vi.importActual<typeof import("../../lib/api/stt")>("../../lib/api/stt");
  return { ...actual, useModels: () => ({ data: models }), startDownload: vi.fn() };
});
const model = (name: ModelView["name"], downloaded: boolean): ModelView => ({
  name,
  size_mb: 487,
  downloaded,
  downloading: false,
  intel_supported: true,
  managed_by_homebrew: false,
});
const deleteAll = {
  mutate: vi.fn((_: undefined, o: { onSettled: () => void }) => o.onSettled()),
  reset: vi.fn(),
  isPending: false,
  error: null,
  data: undefined,
};

const RUNNING = { accessibility: "granted", running: true, status: "on, hotkey fn, model parakeet-v3" } as const;
vi.mock("../../lib/api/audio", async () => {
  const actual = await vi.importActual<typeof import("../../lib/api/audio")>(
    "../../lib/api/audio",
  );
  return {
    ...actual,
    fetchPermissions: vi.fn().mockResolvedValue({
      screen_recording: "granted",
      microphone: "granted",
      owner: "Ghostty",
    }),
  };
});

function general(overrides: Partial<General> = {}): General {
  return {
    port: 7878,
    open_browser_on_start: true,
    audio_input_device: "",
    first_run_completed: true,
    stt_provider: "local",
    stt_model: "small.en",
    meeting_detection: true,
    meeting_detection_focus: true,
    meeting_auto_stop: true,
    live_recap: true,
    live_recap_placement: "dock",
    voice_enabled: true,
    voice_hotkey: "fn",
    voice_retention: "30d",
    voice_cleanup: false,
    voice_cleanup_provider: "builtin",
    voice_stt_model: "",
    voice_input_device: "",
    ...overrides,
  };
}

function renderSection(g: General) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <MemoryRouter>
      <QueryClientProvider client={qc}>
        <VoiceSection general={g} providers={[]} deepgramKey />
      </QueryClientProvider>
    </MemoryRouter>,
  );
}

describe("VoiceSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    voiceHistory = { history: { count: 3, stt_avg_ms: {} }, window_hours: null };
    vi.mocked(settingsApi.patch).mockResolvedValue(general());
    vi.mocked(settingsApi.get).mockResolvedValue({ general: general() } as SettingsView);
    vi.mocked(audioApi.devices).mockResolvedValue([
      { name: "Built-in Microphone", is_default: true, sample_rate: 48000 },
      { name: "USB Headset", is_default: false, sample_rate: 16000 },
    ]);
    vi.mocked(fetchVoiceStatus).mockResolvedValue(RUNNING);
    vi.mocked(recordShortcut).mockResolvedValue();
    vi.mocked(fetchRecorded).mockResolvedValue({ kind: "pending", held: [] });
    vi.mocked(startDownload).mockResolvedValue();
    models = [model("tiny.en", true), model("parakeet-v3", true), model("large-v3-turbo", true)];
  });

  it("shows the engine and both grants once Accessibility is granted", async () => {
    renderSection(general());
    expect(await screen.findByText("On · local parakeet-v3")).toBeInTheDocument();
    // The pill sits outside the label, so the checkbox keeps a clean name and a click on the pill does nothing.
    expect(screen.getByRole("checkbox", { name: "Turn on Yogurt Voice" })).toBeChecked();
    expect(screen.getByText("Granted")).toBeInTheDocument();
    await waitFor(() => expect(screen.getAllByText("Microphone")).toHaveLength(2));
    expect(screen.queryByText("Open System Settings")).toBeNull();
  });

  it("saves Voice's own mic from the Microphone field, under Same as meetings", async () => {
    renderSection(general());
    const mic = await screen.findByRole("combobox", { name: "Microphone" });
    expect(within(mic).getByRole("option", { name: "Same as meetings: Built-in Microphone" })).toHaveValue("");
    fireEvent.change(mic, { target: { value: "USB Headset" } });
    await waitFor(() => expect(settingsApi.patch).toHaveBeenCalledWith({ voice_input_device: "USB Headset" }));
  });

  it("names the Voice model in the status pill once one is picked", async () => {
    renderSection(general({ voice_stt_model: "tiny.en" }));
    expect(await screen.findByText("On · local tiny.en")).toBeInTheDocument();
  });

  it("names Deepgram in the status pill when Voice is on it", async () => {
    renderSection(general({ voice_stt_model: "deepgram" }));
    expect(await screen.findByText("On · Deepgram")).toBeInTheDocument();
    expect(screen.queryByText("Model missing")).toBeNull();
  });

  it("links to the Accessibility pane while the grant is missing", async () => {
    vi.mocked(fetchVoiceStatus).mockResolvedValue({
      accessibility: "missing",
      running: false,
      status: "waiting for Accessibility",
    });
    renderSection(general());
    expect(await screen.findByText("Waiting for Accessibility")).toBeInTheDocument();
    expect(screen.getByText("Missing")).toBeInTheDocument();
    expect(screen.getByText("Open System Settings").getAttribute("href")).toBe(
      "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
    );
  });

  it("shows no status pill while Voice is off", async () => {
    renderSection(general({ voice_enabled: false }));
    await screen.findByText("Granted");
    expect(screen.queryByText(/^On ·/)).toBeNull();
  });

  it("turns Voice on through the settings patch", async () => {
    renderSection(general({ voice_enabled: false }));
    fireEvent.click(screen.getByLabelText("Turn on Yogurt Voice"));
    await waitFor(() =>
      expect(vi.mocked(settingsApi.patch).mock.calls[0][0]).toEqual({ voice_enabled: true }),
    );
  });

  it("shows the Globe hint only for Fn", () => {
    const { unmount } = renderSection(general());
    expect(screen.getByText("Press 🌐 key to")).toBeInTheDocument();
    unmount();
    renderSection(general({ voice_hotkey: "option+e" }));
    expect(screen.queryByText("Press 🌐 key to")).toBeNull();
    expect(screen.getByTestId("shortcut")).toHaveTextContent("⌥+E");
  });

  it("shows the keys the tap sees held, then saves the shortcut it recorded", async () => {
    renderSection(general({ voice_hotkey: "option+e" }));
    await screen.findByText("Granted");
    fireEvent.click(screen.getByRole("button", { name: "Change" }));
    expect(screen.getByText("Press the keys you want. Esc cancels.")).toBeInTheDocument();
    vi.mocked(fetchRecorded).mockResolvedValue({ kind: "pending", held: ["fn"] });
    await waitFor(() => expect(screen.getByTestId("shortcut-recorder")).toHaveTextContent("fn"));
    vi.mocked(fetchRecorded).mockResolvedValue({ kind: "done", hotkey: "fn" });
    await waitFor(() =>
      expect(vi.mocked(settingsApi.patch).mock.calls[0][0]).toEqual({ voice_hotkey: "fn" }),
    );
    expect(screen.queryByTestId("shortcut-recorder")).toBeNull();
    expect(vi.mocked(recordShortcut).mock.calls).toEqual([[true], [false]]);
  });

  it("draws a held modifier without its side", async () => {
    renderSection(general());
    fireEvent.click(screen.getByRole("button", { name: "Change" }));
    vi.mocked(fetchRecorded).mockResolvedValue({ kind: "pending", held: ["right_option", "left_shift"] });
    await waitFor(() => expect(screen.getByTestId("shortcut-recorder")).toHaveTextContent("⌥⇧"));
  });

  it("ends the recording without saving when the tap sees Esc", async () => {
    renderSection(general());
    fireEvent.click(screen.getByRole("button", { name: "Change" }));
    vi.mocked(fetchRecorded).mockResolvedValue({ kind: "cancel" });
    await waitFor(() => expect(screen.queryByTestId("shortcut-recorder")).toBeNull());
    expect(settingsApi.patch).not.toHaveBeenCalled();
  });

  it("cannot record without Accessibility, since the tap does the recording", async () => {
    vi.mocked(fetchVoiceStatus).mockResolvedValue({ accessibility: "missing", running: false, status: "x" });
    renderSection(general());
    expect(await screen.findByText("Changing it needs Accessibility, below.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Change" })).toBeDisabled();
  });

  it("says why Voice is not running", async () => {
    vi.mocked(fetchVoiceStatus).mockResolvedValue({
      accessibility: "granted",
      running: false,
      status: "could not load /tmp/m.bin: bad magic",
    });
    renderSection(general());
    expect(await screen.findByText("Not running")).toBeInTheDocument();
    expect(screen.getByText("Could not load /tmp/m.bin: bad magic.")).toBeInTheDocument();
  });

  it("offers the picker's Download when the Voice model is missing", async () => {
    models = [model("parakeet-v3", false)];
    vi.mocked(fetchVoiceStatus).mockResolvedValue({
      accessibility: "granted",
      running: false,
      status: "the local model parakeet-v3 is not downloaded; download it in Settings > Voice",
    });
    renderSection(general({ stt_provider: "cloud" }));
    expect(await screen.findByText("Model missing")).toBeInTheDocument();
    expect(
      screen.getByText("The local model parakeet-v3 is not downloaded; download it in Settings > Voice."),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download parakeet-v3" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Download the Voice model, parakeet-v3" }));
    await waitFor(() => expect(vi.mocked(startDownload)).toHaveBeenCalledWith("parakeet-v3"));
  });

  it("flags the model Same as meetings follows when it is missing", async () => {
    models = [model("small.en", true), model("large-v3-turbo", false)];
    vi.mocked(fetchVoiceStatus).mockResolvedValue({ accessibility: "granted", running: false, status: "x" });
    renderSection(general({ voice_stt_model: "meetings", stt_model: "large-v3-turbo" }));
    expect(await screen.findByText("Model missing")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download the Voice model, large-v3-turbo" })).toBeInTheDocument();
  });

  it("trusts the running voice task over a stale models list", async () => {
    models = [model("parakeet-v3", false)];
    renderSection(general());
    expect(await screen.findByText("On · local parakeet-v3")).toBeInTheDocument();
    expect(screen.queryByText("Model missing")).toBeNull();
    expect(screen.queryByRole("button", { name: /Download the Voice model/ })).toBeNull();
  });

  it("stops recording when the window loses focus", () => {
    renderSection(general());
    fireEvent.click(screen.getByRole("button", { name: "Change" }));
    fireEvent.blur(window);
    expect(screen.queryByTestId("shortcut-recorder")).toBeNull();
  });

  it("shows a toggle failure by the toggle, not under the shortcut", async () => {
    vi.mocked(settingsApi.patch).mockRejectedValue(new Error("500 boom"));
    renderSection(general({ voice_enabled: false }));
    fireEvent.click(screen.getByLabelText("Turn on Yogurt Voice"));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("500 boom");
    const shortcut = screen.getByText("Shortcut");
    expect(alert.compareDocumentPosition(shortcut) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("offers four retentions and saves the one picked", async () => {
    renderSection(general());
    const select = screen.getByRole("combobox", { name: "History" });
    expect(select).toHaveValue("30d");
    expect(within(select).getAllByRole("option").map((o) => o.textContent)).toEqual([
      "30 days",
      "7 days",
      "24 hours",
      "Never save",
    ]);
    expect(screen.getByText(/A shorter time deletes older text right away/)).toBeInTheDocument();
    fireEvent.change(select, { target: { value: "never" } });
    await waitFor(() =>
      expect(vi.mocked(settingsApi.patch).mock.calls[0][0]).toEqual({ voice_retention: "never" }),
    );
  });

  it("says Never save keeps stats but no history", () => {
    renderSection(general({ voice_retention: "never" }));
    expect(screen.getByRole("combobox", { name: "History" })).toHaveValue("never");
    expect(screen.getByText(/No text is saved, so the Voice page shows stats but no history/)).toBeInTheDocument();
  });

  it("asks before deleting every dictation", () => {
    renderSection(general());
    fireEvent.click(screen.getByRole("button", { name: "Delete all dictations" }));
    expect(deleteAll.mutate).not.toHaveBeenCalled();
    expect(screen.getByText("Delete all dictations and reset the stats? This cannot be undone.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(deleteAll.mutate).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Delete all dictations" })).toBeInTheDocument();
  });

  it("without Pro, deletes only the window and keeps the stats", () => {
    voiceHistory = { history: { count: 2, stt_avg_ms: {} }, window_hours: 24 };
    renderSection(general());
    fireEvent.click(screen.getByRole("button", { name: "Delete all dictations" }));
    expect(
      screen.getByText("Delete the dictations from the last 24 hours? Older ones and the stats are kept. This cannot be undone."),
    ).toBeInTheDocument();
    cleanup();
    voiceHistory = { history: { count: 0, stt_avg_ms: {} }, window_hours: 24 };
    renderSection(general());
    expect(screen.getByRole("button", { name: "Delete all dictations" })).toBeDisabled();
  });
});
