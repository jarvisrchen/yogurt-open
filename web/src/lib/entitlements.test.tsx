import { describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { updateEndsPro, useAddLicense, useEntitlements, type License } from "./entitlements";

const fetchMock = vi.hoisted(() => vi.fn<(path: string, init?: RequestInit) => Promise<Response>>());
vi.mock("./session", () => ({
  bearerFetch: fetchMock,
  bearerJson: async (path: string, init?: RequestInit) => (await fetchMock(path, init)).json(),
}));

const pro = (active: boolean) => ({
  license: {
    id: "l",
    email: "a@example.com",
    issued: "2026-11-13",
    founder: null,
    build_date: "2026-11-13",
    entitlements: [{ name: "pro", until: "lifetime", expires: null, active }],
  },
});

function wrapper({ children }: { children: ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
}

function both() {
  return { ent: useEntitlements(), add: useAddLicense() };
}

describe("useEntitlements", () => {
  it("is pro only while the stored license's pro entitlement is active", async () => {
    fetchMock.mockResolvedValue(Response.json(pro(false)));
    const { result } = renderHook(useEntitlements, { wrapper });
    expect(result.current).toEqual({ pro: false, loading: true });
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.pro).toBe(false);
  });

  it("reads a build without the license route as free", async () => {
    fetchMock.mockResolvedValue(new Response("", { status: 404 }));
    const { result } = renderHook(useEntitlements, { wrapper });
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.pro).toBe(false);
  });

  it("refetches after a license is added", async () => {
    fetchMock.mockResolvedValue(Response.json({ license: null }));
    const { result } = renderHook(both, { wrapper });
    await waitFor(() => expect(result.current.ent.loading).toBe(false));
    expect(result.current.ent.pro).toBe(false);

    fetchMock.mockImplementation(async () => Response.json(pro(true)));
    await act(() => result.current.add.mutateAsync("yogurt1.a.b"));
    await waitFor(() => expect(result.current.ent.pro).toBe(true));
    expect(fetchMock).toHaveBeenCalledWith("/api/license", {
      method: "POST",
      body: JSON.stringify({ license: "yogurt1.a.b" }),
    });
  });
});

describe("updateEndsPro", () => {
  const held = (until: string | null, active = true) =>
    ({ entitlements: [{ name: "pro", until, expires: until ? null : "2027-01-01", active }] }) as License;

  it("holds only for an active dated Pro whose date has passed", () => {
    expect(updateEndsPro(held("2027-11-13"), "2027-11-14")).toBe(true);
    expect(updateEndsPro(held("2027-11-13"), "2027-11-13")).toBe(false);
    expect(updateEndsPro(held("lifetime"), "2099-01-01")).toBe(false);
    expect(updateEndsPro(held(null), "2099-01-01")).toBe(false);
    expect(updateEndsPro(held("2027-11-13", false), "2027-11-14")).toBe(false);
    expect(updateEndsPro(null, "2027-11-14")).toBe(false);
  });
});
