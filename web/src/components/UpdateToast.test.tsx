import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { UpdateToast } from "./UpdateToast";
import type { UpdateStatus } from "../lib/api/update";

const base: UpdateStatus = {
  current: "3.2.2",
  latest: "3.3.0",
  available: true,
  checked_at: 1,
  check_error: null,
  auto_check: true,
  installable: true,
  phase: "idle",
  installed: false,
  error: null,
  step: null,
  downloaded: null,
  total: null,
};

function serve(status: Partial<UpdateStatus>, path = "/", license: object | null = null) {
  const calls: string[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push(`${init?.method ?? "GET"} ${url}`);
      if (url === "/api/session-token") return Response.json({ token: "t" });
      if (url === "/api/license") return Response.json({ license });
      return Response.json({ ...base, ...status });
    }),
  );
  render(
    <MemoryRouter initialEntries={[path]}>
      <QueryClientProvider client={new QueryClient()}>
        <UpdateToast />
      </QueryClientProvider>
    </MemoryRouter>,
  );
  return calls;
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  localStorage.clear();
});

describe("UpdateToast", () => {
  it("warns that an update past the Pro updates date turns Pro off, and still updates", async () => {
    const pro = (until: string) => ({ entitlements: [{ name: "pro", until, expires: null, active: true }] });
    const calls = serve({}, "/", pro("2020-01-31"));
    expect(await screen.findByText(/This update is past your Pro updates date, so Pro features turn off in it/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Update and restart" }));
    await waitFor(() => expect(calls).toContain("POST /api/update/install"));
    cleanup();

    serve({}, "/", pro("2099-01-31"));
    expect(await screen.findByText(/Homebrew installs it/)).toBeInTheDocument();
    expect(screen.queryByText(/past your Pro updates date/)).toBeNull();
  });

  it("shows the download's percentage only once its size is known", async () => {
    serve({ phase: "installing", step: "downloading", downloaded: 8_500_000, total: 20_300_000 });
    const bar = await screen.findByRole("progressbar");
    expect(bar).toHaveAttribute("aria-valuenow", "41");
    expect(bar).toHaveTextContent("Downloading 41%");
    expect(bar).toHaveTextContent("8.5 MB of 20.3 MB");
    cleanup();

    serve({ phase: "installing", step: "downloading", downloaded: 8_500_000 });
    const sizeless = await screen.findByRole("progressbar");
    expect(sizeless).not.toHaveAttribute("aria-valuenow");
    expect(sizeless).toHaveAttribute("aria-valuetext", "Downloading");
    cleanup();

    serve({ phase: "installing", step: "installing" });
    expect(await screen.findByRole("progressbar")).toHaveAttribute("aria-valuetext", "Installing");
    cleanup();

    serve({ phase: "installing" });
    expect(await screen.findByRole("progressbar")).toHaveAttribute("aria-valuetext", "Downloading and installing");
  });

  it("offers a newer version and installs it on Update", async () => {
    const calls = serve({});
    fireEvent.click(await screen.findByRole("button", { name: "Update and restart" }));
    await waitFor(() => expect(calls).toContain("POST /api/update/install"));
    expect(screen.getByRole("link", { name: "What's new" })).toHaveAttribute(
      "href",
      "https://github.com/jarvisrchen/yogurt-releases/releases/tag/v3.3.0",
    );
  });

  it("stays hidden for that version after Later", async () => {
    serve({});
    fireEvent.click(await screen.findByRole("button", { name: "Later" }));
    expect(screen.queryByTestId("update-toast")).toBeNull();
    expect(localStorage.getItem("yogurt-update-later")).toBe("3.3.0");
  });

  it("shows nothing for a copy brew does not own, when up to date, or during first run", async () => {
    serve({ installable: false });
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByTestId("update-toast")).toBeNull();
    cleanup();
    serve({ latest: "3.2.2", available: false });
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByTestId("update-toast")).toBeNull();
    cleanup();
    serve({}, "/welcome");
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByTestId("update-toast")).toBeNull();
  });

  it("lets a pending update be cancelled while a meeting records", async () => {
    const calls = serve({ phase: "waiting" });
    expect(await screen.findByText("Update waiting")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel update" }));
    await waitFor(() => expect(calls).toContain("POST /api/update/cancel"));
  });

  it("shows brew's error when the install fails", async () => {
    serve({ phase: "failed", error: "brew upgrade failed: Error: Download failed" });
    expect(await screen.findByText("The update didn't install")).toBeInTheDocument();
    expect(screen.getByText("brew upgrade failed: Error: Download failed")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
  });

  it("confirms the new version after the restart reloads the page", async () => {
    localStorage.setItem("yogurt-updated-from", "3.2.2");
    serve({ current: "3.3.0", latest: "3.3.0", available: false });
    expect(await screen.findByText("Yogurt is now 3.3.0")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(localStorage.getItem("yogurt-updated-from")).toBeNull();
  });
});
