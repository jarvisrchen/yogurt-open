import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AudioSection } from "./AudioSection";
import { audioApi, settingsApi, type General } from "../../lib/api/settings";

vi.mock("../../lib/api/settings", async () => {
  const actual =
    await vi.importActual<typeof import("../../lib/api/settings")>(
      "../../lib/api/settings",
    );
  return {
    ...actual,
    audioApi: {
      ...actual.audioApi,
      devices: vi.fn(),
    },
    settingsApi: {
      ...actual.settingsApi,
      patch: vi.fn(),
    },
  };
});

function baseGeneral(overrides: Partial<General> = {}): General {
  return {
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
    ...overrides,
  };
}

function renderSection(general: General) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={qc}>
      <AudioSection general={general} />
    </QueryClientProvider>,
  );
}

describe("AudioSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(audioApi.devices).mockResolvedValue([
      { name: "MacBook Pro Microphone", is_default: true, sample_rate: 48000 },
    ]);
    vi.mocked(settingsApi.patch).mockResolvedValue(baseGeneral());
  });

  it("saves the input device picked", async () => {
    renderSection(baseGeneral());

    await screen.findByRole("option", { name: /MacBook Pro Microphone/ });
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "MacBook Pro Microphone" } });

    await waitFor(() => {
      expect(settingsApi.patch).toHaveBeenCalledWith({ audio_input_device: "MacBook Pro Microphone" });
    });
  });
});
