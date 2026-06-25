import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import type { Dictation } from "../../lib/api/voiceTypes";
import { DictationCard } from "./DictationCard";

const dictation: Dictation = {
  id: "d1", created_at: 0, app_bundle_id: null, app_name: "Notes", hands_free: false,
  audio_ms: 5_000, raw_text: "um send the release checklist tomorrow", text: "Send the release checklist tomorrow.", raw_words: 6, words: 5,
  stt_engine: "local:parakeet-v3", stt_ms: 200, cleanup: "done", cleanup_model: "celeris-1",
  cleanup_ms: 300, input_tokens: null, output_tokens: null, cost_usd: null,
};

afterEach(() => {
  cleanup();
  window.getSelection()!.removeAllRanges();
});

const addWord = { mutate: vi.fn(), reset: vi.fn(), isPending: false, error: null };

function select(el: HTMLElement, from: number, to: number) {
  const range = document.createRange();
  range.setStart(el.firstChild!, from);
  range.setEnd(el.firstChild!, to);
  window.getSelection()!.removeAllRanges();
  window.getSelection()!.addRange(range);
  fireEvent(document, new Event("selectionchange"));
}

describe("dictation model history", () => {
  it.each([
    ["local:parakeet-v3", "parakeet-v3"],
    ["deepgram:nova-3", "Deepgram nova-3"],
    ["local", "Local (model not recorded)"],
  ])("shows the recorded transcription engine %s", (stt_engine, label) => {
    render(<DictationCard d={{ ...dictation, stt_engine }} />);
    expect(screen.getByText(`Transcription: ${label}`)).toBeVisible();
    expect(screen.getByText("Cleanup: celeris-1")).toBeVisible();
  });

  it.each([
    ["off", "Not applied"], ["rules", "Rules only"], ["snippet", "Snippet"], ["done", "Model not recorded"],
  ] as const)("does not invent a cleanup model for %s", (cleanup, label) => {
    render(<DictationCard d={{ ...dictation, cleanup, cleanup_model: null }} />);
    expect(screen.getByText(`Cleanup: ${label}`)).toBeVisible();
  });

  it("marks a failed model as attempted even when cleanup was saved as off", () => {
    render(<DictationCard d={{ ...dictation, cleanup: "off" }} />);
    expect(screen.getByText("Cleanup attempted: celeris-1")).toBeVisible();
  });

  it("marks a model that was too slow as attempted", () => {
    render(<DictationCard d={{ ...dictation, cleanup: "too_slow" }} />);
    expect(screen.getByText("Cleanup attempted: celeris-1")).toBeVisible();
    expect(screen.getByText("Pasted raw, cleanup too slow")).toBeVisible();
  });
});

describe("add to dictionary", () => {
  it("shows the raw text, which is what the dictionary is matched against, and fills Sounds like from it", () => {
    render(<DictationCard d={dictation} addWord={addWord} />);
    fireEvent.click(screen.getByRole("button", { name: "Add to dictionary" }));
    expect(screen.getByText("What you said")).toBeVisible();
    expect(screen.getByText(/^Select the misheard word in What you said/)).toBeVisible();
    select(screen.getByText(dictation.raw_text), 3, 7);
    expect(screen.getByRole("textbox", { name: "Sounds like" })).toHaveValue("send");
    expect(screen.getByRole("textbox", { name: "Word" })).toHaveValue("");
  });

  it("fills Sounds like from a raw selection made before the form opens", () => {
    render(<DictationCard d={dictation} addWord={addWord} />);
    fireEvent.click(screen.getByRole("button", { name: "Raw" }));
    select(screen.getByText(dictation.raw_text), 12, 19);
    fireEvent.click(screen.getByRole("button", { name: "Add to dictionary" }));
    expect(screen.getByRole("textbox", { name: "Sounds like" })).toHaveValue("release");
  });
});
