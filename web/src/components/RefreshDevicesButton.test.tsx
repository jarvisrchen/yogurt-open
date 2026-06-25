import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { RefreshDevicesButton } from "./RefreshDevicesButton";

describe("RefreshDevicesButton", () => {
  it("invalidates every device list keyed under audio-devices", () => {
    const qc = new QueryClient();
    qc.setQueryData(["audio-devices"], []);
    qc.setQueryData(["audio-devices", "another-list"], []);
    render(
      <QueryClientProvider client={qc}>
        <RefreshDevicesButton />
      </QueryClientProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: /refresh device list/i }));
    expect(qc.getQueryState(["audio-devices"])?.isInvalidated).toBe(true);
    expect(qc.getQueryState(["audio-devices", "another-list"])?.isInvalidated).toBe(true);
  });
});
