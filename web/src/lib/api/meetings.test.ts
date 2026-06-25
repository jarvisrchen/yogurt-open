import { describe, expect, it } from "vitest";
import { QueryClient } from "@tanstack/react-query";
import { listedMeeting, meetingKey, type MeetingSummary } from "./meetings";

const row = (id: string): MeetingSummary => ({
  id,
  title: `Meeting ${id}`,
  started_at: 1,
  ended_at: 2,
  starred: false,
  stt_engine: null,
  llm_model: null,
  has_enriched: true,
  labels: [],
});

describe("listedMeeting", () => {
  it("finds a meeting on any cached Library page and skips other meeting queries", () => {
    const qc = new QueryClient();
    qc.setQueryData(["meetings", "active"], null);
    qc.setQueryData(meetingKey("a"), { id: "a", title: "full" });
    qc.setQueryData(["meetings", { page: 2 }], { meetings: [row("b")], total: 1 });
    expect(listedMeeting(qc, "b")?.title).toBe("Meeting b");
    expect(listedMeeting(qc, "a")).toBeUndefined();
  });
});
