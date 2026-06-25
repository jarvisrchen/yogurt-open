import { afterEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { AboutSection } from "./AboutSection";

function renderWithHealth(body: object, status = 200) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response(JSON.stringify(body), { status })),
  );
  return render(
    <MemoryRouter>
      <QueryClientProvider client={new QueryClient()}>
        <AboutSection />
      </QueryClientProvider>
    </MemoryRouter>,
  );
}

afterEach(() => vi.unstubAllGlobals());

const health = { status: "ok", service: "yogurt-server", version: "3.2.2", mode: "release" };

function renderWithUpdate(update: object) {
  const calls: string[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push(`${init?.method ?? "GET"} ${url}`);
      if (url === "/api/session-token") return Response.json({ token: "t" });
      if (url === "/api/health") return Response.json(health);
      return Response.json({
        current: "3.2.2", latest: "3.2.2", available: false, checked_at: Date.now(), check_error: null,
        auto_check: true, installable: true, phase: "idle", installed: false, error: null,
        ...update,
      });
    }),
  );
  render(
    <MemoryRouter>
      <QueryClientProvider client={new QueryClient()}>
        <AboutSection />
      </QueryClientProvider>
    </MemoryRouter>,
  );
  return calls;
}

describe("AboutSection", () => {
  it("shows the server version and links its release notes", async () => {
    renderWithHealth({ status: "ok", service: "yogurt-server", version: "2.2.0", mode: "release" });
    expect(await screen.findByTestId("about-version")).toHaveTextContent("v2.2.0");
    expect(screen.getByRole("link", { name: "Release notes" })).toHaveAttribute(
      "href",
      "https://github.com/jarvisrchen/yogurt-releases/releases/tag/v2.2.0",
    );
    expect(screen.queryByText("dev build")).toBeNull();
  });

  it("marks a dev build and skips its release notes link", async () => {
    renderWithHealth({ status: "ok", service: "yogurt-server", version: "2.2.0", mode: "dev" });
    expect(await screen.findByText("dev build")).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Release notes" })).toBeNull();
  });

  it("says Unavailable when the health check fails", async () => {
    renderWithHealth({}, 500);
    expect(await screen.findByText("Unavailable")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Website" })).toHaveAttribute(
      "href",
      "https://yogurt-ai.dev",
    );
  });

  it("checks for updates on demand and turns the daily check off", async () => {
    const calls = renderWithUpdate({});
    expect(await screen.findByText("Up to date")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    await waitFor(() => expect(calls).toContain("POST /api/update/check"));
    fireEvent.click(screen.getByRole("checkbox", { name: "Check for updates once a day" }));
    await waitFor(() => expect(calls).toContain("PATCH /api/update"));
  });

  it("offers a ready update beside the version", async () => {
    renderWithUpdate({ latest: "3.3.0", available: true });
    expect(await screen.findByText("v3.3.0 is ready")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Update and restart" })).toBeInTheDocument();
  });

  it("explains how to update a copy brew does not own", async () => {
    renderWithUpdate({ installable: false });
    expect(await screen.findByText(/not installed with Homebrew/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Check for updates" })).toBeNull();
    expect(screen.queryByRole("checkbox")).toBeNull();
  });
});
