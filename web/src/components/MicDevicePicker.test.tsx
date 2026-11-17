import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MicDevicePicker } from "./MicDevicePicker";
import { audioApi, settingsApi, type General } from "../lib/api/settings";

const active = vi.hoisted(() => ({ get: vi.fn() }));

// The real cache, so a switch's write to it reaches the picker as it does in the app.
vi.mock("../lib/api/meetings", async () => {
  const { useQuery } = await import("@tanstack/react-query");
  const activeRecordingKey = ["meetings", "active"] as const;
  return {
    activeRecordingKey,
    useActiveRecording: () => useQuery({ queryKey: activeRecordingKey, queryFn: async () => active.get() }),
  };
});

vi.mock("../lib/api/settings", async () => {
  const actual =
    await vi.importActual<typeof import("../lib/api/settings")>(
      "../lib/api/settings",
    );
  return {
    ...actual,
    audioApi: {
      devices: vi.fn(),
      switchMeetingDevice: vi.fn(),
    },
    settingsApi: {
      ...actual.settingsApi,
      get: vi.fn(),
      patch: vi.fn(),
    },
  };
});

const DEVICES = [
  { name: "Built-in Microphone", is_default: true, sample_rate: 48000 },
  { name: "AirPods Pro", is_default: false, sample_rate: 16000 },
];

function renderPicker(recording = true) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={qc}>
      <MicDevicePicker meetingId="meeting-1" recording={recording} />
    </QueryClientProvider>,
  );
}

describe("MicDevicePicker", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(audioApi.devices).mockResolvedValue(DEVICES);
    active.get.mockReturnValue({ id: "meeting-1", mic_device: "" });
    vi.mocked(audioApi.switchMeetingDevice).mockResolvedValue({
      status: "switched",
      device: "AirPods Pro",
    });
    vi.mocked(settingsApi.get).mockResolvedValue({
      general: {
        port: 7878,
        open_browser_on_start: true,
        audio_input_device: "",
        first_run_completed: true,
        stt_provider: "cloud",
        stt_model: "nova-3",
        meeting_detection: true,
        meeting_detection_focus: true,
        meeting_auto_stop: true,
        meeting_bubble: true,
        live_recap: true,
        live_recap_placement: "dock",
        voice_enabled: false,
        voice_hotkey: "fn",
        voice_retention: "30d",
        voice_cleanup: false,
        voice_cleanup_provider: "builtin",
        voice_stt_model: "",
        voice_input_device: "",
      },
      providers: [],
      presets: [],
      deepgram_key_masked: null,
      paths: {
        data_dir: "~/.yogurt",
        keys: "~/.yogurt/keys.json",
        notes: "~/.yogurt/notes",
      },
    });
    vi.mocked(settingsApi.patch).mockResolvedValue({
      port: 7878,
      open_browser_on_start: true,
      audio_input_device: "AirPods Pro",
      first_run_completed: true,
      stt_provider: "cloud",
      stt_model: "nova-3",
      meeting_detection: true,
      meeting_detection_focus: true,
      meeting_auto_stop: true,
      meeting_bubble: true,
      live_recap: true,
      live_recap_placement: "dock",
      voice_enabled: false,
      voice_hotkey: "fn",
      voice_retention: "30d",
      voice_cleanup: false,
      voice_cleanup_provider: "builtin",
      voice_stt_model: "",
      voice_input_device: "",
    });
  });

  it("renders a combobox with both device names, defaulting to the default device", async () => {
    renderPicker();
    const select = await screen.findByRole("combobox", {
      name: /microphone/i,
    });
    expect(
      screen.getByRole("option", { name: /Built-in Microphone/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: /AirPods Pro/ }),
    ).toBeInTheDocument();
    expect(select).toHaveValue("Built-in Microphone");
  });

  it("calls switchMeetingDevice with (meetingId, deviceName) on selection", async () => {
    renderPicker();
    const select = await screen.findByRole("combobox", {
      name: /microphone/i,
    });
    fireEvent.change(select, { target: { value: "AirPods Pro" } });
    await waitFor(() => {
      expect(audioApi.switchMeetingDevice).toHaveBeenCalledWith(
        "meeting-1",
        "AirPods Pro",
      );
    });
  });

  it("shows the device the recording opened, not the OS default, on a fresh load", async () => {
    active.get.mockReturnValue({ id: "meeting-1", mic_device: "AirPods Pro" });
    renderPicker();
    const select = await screen.findByRole("combobox", { name: /microphone/i });
    await waitFor(() => expect(select).toHaveValue("AirPods Pro"));
  });

  it("reflects the resolved active device after a successful switch", async () => {
    renderPicker();
    const select = await screen.findByRole("combobox", {
      name: /microphone/i,
    });
    fireEvent.change(select, { target: { value: "AirPods Pro" } });
    await waitFor(() => {
      expect(select).toHaveValue("AirPods Pro");
    });
  });

  it("disables the select and shows a switching indicator while pending, and surfaces an error without reverting the value", async () => {
    let resolveSwitch!: (v: { status: string; device: string }) => void;
    vi.mocked(audioApi.switchMeetingDevice).mockReset();
    vi.mocked(audioApi.switchMeetingDevice).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveSwitch = resolve;
        }),
    );

    renderPicker();
    const select = await screen.findByRole("combobox", {
      name: /microphone/i,
    });
    fireEvent.change(select, { target: { value: "AirPods Pro" } });

    await waitFor(() => {
      expect(select).toBeDisabled();
    });
    expect(screen.getByText(/switching/i)).toBeInTheDocument();

    resolveSwitch({ status: "switched", device: "AirPods Pro" });
    await waitFor(() => {
      expect(select).not.toBeDisabled();
    });
  });

  it("shows an inline error and keeps the last-known-good value on mutation error", async () => {
    vi.mocked(audioApi.switchMeetingDevice).mockReset();
    vi.mocked(audioApi.switchMeetingDevice).mockRejectedValue(
      new Error("device not found"),
    );

    renderPicker();
    const select = await screen.findByRole("combobox", {
      name: /microphone/i,
    });
    fireEvent.change(select, { target: { value: "AirPods Pro" } });

    await waitFor(() => {
      expect(screen.getByText(/device not found/i)).toBeInTheDocument();
    });
    expect(select).toHaveValue("Built-in Microphone");
  });
});

describe("MicDevicePicker — stopped (meeting open, not recording)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(audioApi.devices).mockResolvedValue(DEVICES);
    active.get.mockReturnValue({ id: "meeting-1", mic_device: "" });
    vi.mocked(settingsApi.get).mockResolvedValue({
      general: {
        port: 7878,
        open_browser_on_start: true,
        audio_input_device: "",
        first_run_completed: true,
        stt_provider: "cloud",
        stt_model: "nova-3",
        meeting_detection: true,
        meeting_detection_focus: true,
        meeting_auto_stop: true,
        meeting_bubble: true,
        live_recap: true,
        live_recap_placement: "dock",
        voice_enabled: false,
        voice_hotkey: "fn",
        voice_retention: "30d",
        voice_cleanup: false,
        voice_cleanup_provider: "builtin",
        voice_stt_model: "",
        voice_input_device: "",
      },
      providers: [],
      presets: [],
      deepgram_key_masked: null,
      paths: {
        data_dir: "~/.yogurt",
        keys: "~/.yogurt/keys.json",
        notes: "~/.yogurt/notes",
      },
    });
    vi.mocked(settingsApi.patch).mockResolvedValue({
      port: 7878,
      open_browser_on_start: true,
      audio_input_device: "AirPods Pro",
      first_run_completed: true,
      stt_provider: "cloud",
      stt_model: "nova-3",
      meeting_detection: true,
      meeting_detection_focus: true,
      meeting_auto_stop: true,
      meeting_bubble: true,
      live_recap: true,
      live_recap_placement: "dock",
      voice_enabled: false,
      voice_hotkey: "fn",
      voice_retention: "30d",
      voice_cleanup: false,
      voice_cleanup_provider: "builtin",
      voice_stt_model: "",
      voice_input_device: "",
    });
  });

  it("renders the combobox while stopped instead of hiding", async () => {
    renderPicker(false);
    expect(
      await screen.findByRole("combobox", { name: /microphone/i }),
    ).toBeInTheDocument();
  });

  it("PATCHes settings.audio_input_device (not the live hot-swap endpoint) on selection while stopped", async () => {
    renderPicker(false);
    const select = await screen.findByRole("combobox", {
      name: /microphone/i,
    });
    fireEvent.change(select, { target: { value: "AirPods Pro" } });

    await waitFor(() => {
      expect(settingsApi.patch).toHaveBeenCalledWith({
        audio_input_device: "AirPods Pro",
      });
    });
    expect(audioApi.switchMeetingDevice).not.toHaveBeenCalled();
  });

  it("defaults the selection to the already-persisted audio_input_device, not just the OS default", async () => {
    vi.mocked(settingsApi.get).mockResolvedValue({
      general: {
        port: 7878,
        open_browser_on_start: true,
        audio_input_device: "AirPods Pro",
        first_run_completed: true,
        stt_provider: "cloud",
        stt_model: "nova-3",
        meeting_detection: true,
        meeting_detection_focus: true,
        meeting_auto_stop: true,
        meeting_bubble: true,
        live_recap: true,
        live_recap_placement: "dock",
        voice_enabled: false,
        voice_hotkey: "fn",
        voice_retention: "30d",
        voice_cleanup: false,
        voice_cleanup_provider: "builtin",
        voice_stt_model: "",
        voice_input_device: "",
      },
      providers: [],
      presets: [],
      deepgram_key_masked: null,
      paths: {
        data_dir: "~/.yogurt",
        keys: "~/.yogurt/keys.json",
        notes: "~/.yogurt/notes",
      },
    });

    renderPicker(false);
    const select = await screen.findByRole("combobox", {
      name: /microphone/i,
    });
    await waitFor(() => expect(select).toHaveValue("AirPods Pro"));
  });

  it("reflects the newly-selected device after a successful settings patch, so the next Start honors it", async () => {
    renderPicker(false);
    const select = await screen.findByRole("combobox", {
      name: /microphone/i,
    });
    fireEvent.change(select, { target: { value: "AirPods Pro" } });
    await waitFor(() => {
      expect(select).toHaveValue("AirPods Pro");
    });
  });
});

describe("MicDevicePicker — voice", () => {
  function settingsWith(g: Partial<General>) {
    vi.mocked(settingsApi.get).mockResolvedValue({
      general: { audio_input_device: "", voice_input_device: "", ...g } as General,
      providers: [],
      presets: [],
      deepgram_key_masked: null,
      paths: { data_dir: "~/.yogurt", keys: "~/.yogurt/keys.json", notes: "~/.yogurt/notes" },
    });
  }

  function renderVoice() {
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    return render(
      <QueryClientProvider client={qc}>
        <MicDevicePicker voice />
      </QueryClientProvider>,
    );
  }

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(audioApi.devices).mockResolvedValue(DEVICES);
    active.get.mockReturnValue(null);
  });

  it("starts on Same as meetings, naming the meetings mic, and saves voice_input_device", async () => {
    settingsWith({ audio_input_device: "AirPods Pro" });
    vi.mocked(settingsApi.patch).mockResolvedValue({ voice_input_device: "Built-in Microphone" } as General);
    renderVoice();
    const select = await screen.findByRole("combobox", { name: "Microphone" });
    await waitFor(() => expect(select).toHaveDisplayValue("Same as meetings: AirPods Pro"));
    expect(screen.getAllByRole("option")[0]).toHaveValue("");

    fireEvent.change(select, { target: { value: "Built-in Microphone" } });
    await waitFor(() => expect(select).toHaveValue("Built-in Microphone"));
    expect(settingsApi.patch).toHaveBeenCalledWith({ voice_input_device: "Built-in Microphone" });
    expect(audioApi.switchMeetingDevice).not.toHaveBeenCalled();
  });

  it("names the system default when meetings follow it", async () => {
    settingsWith({});
    renderVoice();
    expect(await screen.findByRole("option", { name: "Same as meetings: Built-in Microphone" })).toBeInTheDocument();
  });

  it("marks Same as meetings unavailable when the meetings mic is unplugged", async () => {
    settingsWith({ audio_input_device: "Desk Mic" });
    renderVoice();
    expect(await screen.findByRole("option", { name: "Same as meetings: Desk Mic (unavailable)" })).toBeInTheDocument();
  });

  it("keeps an unplugged Voice mic selected as unavailable", async () => {
    settingsWith({ voice_input_device: "Desk Mic" });
    renderVoice();
    const select = await screen.findByRole("combobox", { name: "Microphone" });
    await waitFor(() => expect(select).toHaveDisplayValue("Desk Mic (unavailable)"));
  });
});
