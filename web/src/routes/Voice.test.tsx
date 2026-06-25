import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { MemoryRouter, Route, Routes, useLocation } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { voiceTryKey, type Dictation, type DictationFilter, type DictationPage, type HistoryCounts, type VoiceTry } from "../lib/api/voice";
import type { VoiceRetention } from "../lib/api/settings";
import { Voice } from "./Voice";
import { WithSidebar } from "../components/library/Sidebar";

const history: HistoryCounts = { count: 3, hands_free_count: 1, apps: [{ name: "Slack", words: 200, count: 2 }], stt_avg_ms: {}, cleanup_avg_ms: {} };

const dictation: Dictation = {
  id: "d1",
  created_at: Date.now(),
  app_bundle_id: "com.tinyspeck.slackmacgap",
  app_name: "Slack",
  hands_free: true,
  audio_ms: 9_000,
  raw_text: "send the deck",
  text: "send the deck",
  raw_words: 3,
  words: 3,
  stt_engine: "local:parakeet-v3",
  stt_ms: 300,
  cleanup: "off",
  cleanup_model: null,
  cleanup_ms: null,
  input_tokens: null,
  output_tokens: null,
  cost_usd: null,
};

let page: DictationPage = { dictations: [dictation], total: 3, history, window_hours: null };

const armVoiceTry = vi.fn(async (_kind: string | null) => {});
const cleanVoiceTry = vi.fn();
const filters: DictationFilter[] = [];
let retention: VoiceRetention = "30d";
vi.mock("../lib/api/voice", async () => ({
  ...(await vi.importActual<typeof import("../lib/api/voice")>("../lib/api/voice")),
  useDictations: (f: DictationFilter) => {
    filters.push(f);
    return { data: { pages: [{ ...page, total: f.handsFree ? 1 : f.app ? 2 : 3 }] }, hasNextPage: false };
  },
  useVoiceHistory: () => page,
  armVoiceTry: (kind: string | null) => armVoiceTry(kind),
  cleanVoiceTry: (raw: string, kind: string) => cleanVoiceTry(raw, kind),
}));
vi.mock("../components/MicDevicePicker", () => ({
  MicDevicePicker: () => <select aria-label="Microphone" />,
}));
vi.mock("../lib/api/settings", async () => ({
  ...(await vi.importActual<typeof import("../lib/api/settings")>("../lib/api/settings")),
  useSettings: () => ({
    data: { general: { voice_enabled: true, voice_hotkey: "option+e", voice_retention: retention, voice_cleanup: false }, providers: [] },
  }),
}));
vi.mock("../lib/api/labels", async () => ({
  ...(await vi.importActual<typeof import("../lib/api/labels")>("../lib/api/labels")),
  useLabels: () => ({ data: [] }),
  useCreateLabel: () => ({ mutate: vi.fn(), isPending: false }),
  useReorderLabels: () => ({ mutate: vi.fn() }),
}));
vi.mock("../lib/api/meetings", async () => ({
  ...(await vi.importActual<typeof import("../lib/api/meetings")>("../lib/api/meetings")),
  useCreateMeeting: () => ({ mutateAsync: vi.fn(), isPending: false }),
}));
vi.mock("../lib/entitlements", async () => ({
  ...(await vi.importActual<typeof import("../lib/entitlements")>("../lib/entitlements")),
  useEntitlements: () => ({ pro: false, loading: false }),
}));


function Where() {
  const l = useLocation();
  return <span data-testid="where">{l.pathname + l.search}</span>;
}

function renderAt(url: string, client = new QueryClient()) {
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[url]}>
        <Routes>
          <Route element={<WithSidebar />}>
            <Route path="/voice" element={<Voice />} />
            <Route path="/voice/hands-free" element={<Voice />} />
          </Route>
          <Route path="*" element={null} />
        </Routes>
        <Where />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

// The tiles count up frame by frame, which jsdom reports as updates outside act(), so the page runs under reduced motion.
beforeEach(() => {
  window.matchMedia = vi.fn((q: string) => ({ matches: q.includes("reduce") })) as unknown as typeof window.matchMedia;
});

afterEach(() => {
  cleanup();
  filters.length = 0;
  retention = "30d";
  page = { dictations: [dictation], total: 3, history, window_hours: null };
  armVoiceTry.mockClear();
  cleanVoiceTry.mockReset();
  vi.useRealTimers();
});

const tried: VoiceTry = {
  raw: "um so can you ask shao yoo to send the deck",
  stt_engine: "local:parakeet-v3",
  stt_ms: 210,
  text: "So can you ask Xiaoyu to send the deck?",
  cleanup: "done",
  cleanup_model: "fast-1",
  cleanup_ms: 620,
  style: "normal",
  words: [],
  fillers: ["um"],
};

describe("Try Voice", () => {
  it("sits on Home only", () => {
    renderAt("/voice");
    const card = screen.getByRole("region", { name: "Try Voice" });
    expect(within(card).getByRole("textbox", { name: "Try Voice text" })).toHaveAttribute(
      "placeholder",
      "Click here, hold ⌥+E, and talk.",
    );
    expect(within(card).getByRole("combobox", { name: "Microphone" })).toBeInTheDocument();
    cleanup();
    renderAt("/voice/hands-free");
    expect(screen.queryByRole("region", { name: "Try Voice" })).not.toBeInTheDocument();
    cleanup();
    renderAt("/voice?app=Slack");
    expect(screen.queryByRole("region", { name: "Try Voice" })).not.toBeInTheDocument();
  });

  it("arms the server only while the field has focus, one request at a time", async () => {
    vi.useFakeTimers();
    let release = () => {};
    armVoiceTry.mockImplementationOnce(() => new Promise<void>((r) => (release = r)));
    renderAt("/voice");
    const field = screen.getByRole("textbox", { name: "Try Voice text" });
    fireEvent.focus(field);
    await vi.advanceTimersByTimeAsync(2_100);
    fireEvent.blur(field);
    await vi.advanceTimersByTimeAsync(0);
    // The renewal and the disarm wait behind the first request, so the disarm always lands last.
    expect(armVoiceTry.mock.calls.map((c) => c[0])).toEqual(["other"]);
    release();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(armVoiceTry.mock.calls.map((c) => c[0])).toEqual(["other", "other", null]);
  });

  it("disarms and forgets the try when the page goes away with the field focused", async () => {
    const client = new QueryClient();
    const { unmount } = renderAt("/voice", client);
    fireEvent.focus(screen.getByRole("textbox", { name: "Try Voice text" }));
    client.setQueryData(voiceTryKey, tried);
    unmount();
    await waitFor(() => expect(armVoiceTry).toHaveBeenLastCalledWith(null));
    expect(client.getQueryData(voiceTryKey)).toBeUndefined();
  });

  it("shows a try's result, with no style to pick without Pro", async () => {
    const client = new QueryClient();
    renderAt("/voice", client);
    act(() => void client.setQueryData(voiceTryKey, tried));
    const card = screen.getByRole("region", { name: "Try Voice" });
    await waitFor(() => expect(within(card).getByRole("textbox", { name: "Try Voice text" })).toHaveValue(tried.text));
    expect(within(card).getByText(tried.raw)).toBeInTheDocument();
    expect(within(card).getByText("parakeet-v3 · 0.21 s")).toBeInTheDocument();
    expect(within(card).getByText("fast-1 · 0.62 s")).toBeInTheDocument();
    expect(within(card).getByText("Filler removed: um")).toBeInTheDocument();
    expect(within(card).queryByText("Normal")).not.toBeInTheDocument();
    expect(within(card).queryByRole("radiogroup", { name: "Clean up as" })).not.toBeInTheDocument();
    expect(cleanVoiceTry).not.toHaveBeenCalled();

    fireEvent.click(within(card).getByRole("button", { name: "Clear" }));
    expect(within(card).getByRole("textbox", { name: "Try Voice text" })).toHaveValue("");
  });
});

describe("Voice page", () => {
  it("selects Voice in the switch, and Meetings goes back to the Library", () => {
    renderAt("/voice");
    expect(screen.getByRole("radio", { name: "Voice" })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(screen.getByRole("radio", { name: "Meetings" }));
    expect(screen.getByTestId("where")).toHaveTextContent(/^\/$/);
  });

  it("lists apps with counts that filter the history by app", async () => {
    renderAt("/voice?app=Slack");
    const slack = screen.getByRole("link", { name: /Slack/ });
    expect(slack).toHaveAttribute("href", "/voice?app=Slack");
    expect(slack).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: /Home/ })).not.toHaveAttribute("aria-current");
    expect(filters.at(-1)).toEqual({ q: "", app: "Slack", handsFree: false });
    expect(await screen.findByText("⌥")).toBeInTheDocument();
  });

  it("counts the kept history in the sidebar, apart from the stats", () => {
    renderAt("/voice");
    expect(screen.getByRole("link", { name: /Home/ })).toHaveTextContent("3");
    expect(screen.queryByRole("link", { name: /Style|Dictionary|Snippets/ })).not.toBeInTheDocument();
    expect(screen.getByText("History")).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /Notes/ })).not.toBeInTheDocument();
  });

  it("says how long text is kept at the end of the history", () => {
    renderAt("/voice");
    const line = screen.getByText(/Text is kept on this Mac for 30 days/);
    expect(within(line).getByRole("link", { name: "Settings › Voice" })).toHaveAttribute("href", "/settings/voice");
    cleanup();
    retention = "never";
    renderAt("/voice");
    expect(screen.getByText(/Text is not saved, so dictations count only in the stats/)).toBeInTheDocument();
  });

  it("says how far back the history reaches without Pro", () => {
    page = { ...page, window_hours: 24 };
    renderAt("/voice?app=Slack");
    expect(screen.getByText("2 dictations in the last 24 hours")).toBeInTheDocument();
  });

  it("filters to hands-free on its own path", () => {
    renderAt("/voice/hands-free");
    expect(screen.getByRole("link", { name: /Hands-free/ })).toHaveAttribute("aria-current", "page");
    expect(filters.at(-1)).toEqual({ q: "", app: null, handsFree: true });
  });

  it("shows only the filtered history, titled by the filter, without the stats", () => {
    for (const [url, title, count] of [
      ["/voice/hands-free", "Hands-free", "1 dictation"],
      ["/voice?app=Slack", "Slack", "2 dictations"],
    ]) {
      renderAt(url);
      expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(title);
      expect(screen.getByText(count)).toBeInTheDocument();
      expect(screen.queryByRole("radio", { name: "30 days" })).not.toBeInTheDocument();
      expect(screen.queryByRole("heading", { name: "Cleanup" })).not.toBeInTheDocument();
      expect(screen.queryByText(/Couldn't load/)).not.toBeInTheDocument();
      cleanup();
    }
  });

  it("has no Add to dictionary without Pro", () => {
    renderAt("/voice");
    expect(screen.getByRole("button", { name: "Copy" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add to dictionary" })).not.toBeInTheDocument();
  });
});
