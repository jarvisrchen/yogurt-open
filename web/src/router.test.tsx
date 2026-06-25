import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { RouterProvider, createMemoryRouter, type RouteObject } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { routes } from "./router";

vi.mock("./lib/api/meetings", async () => {
  const actual =
    await vi.importActual<typeof import("./lib/api/meetings")>(
      "./lib/api/meetings",
    );
  return {
    ...actual,
    useMeetings: () => ({
      data: { meetings: [], total: 0 },
      isLoading: false,
      error: null,
    }),
    useCreateMeeting: () => ({
      mutateAsync: vi.fn(),
      isPending: false,
    }),
    // RecordingPill (mounted globally by Shell) polls this on every route —
    // stub it to "nothing recording" so these route-smoke tests don't hit
    // the network or render an unrelated floating pill.
    useActiveRecording: () => ({ data: null, isLoading: false, error: null }),
  };
});

// Stub settings so the Shell's `useFirstRunRedirect` sees a set-up user.
vi.mock("./lib/api/settings", () => {
  const fixture = {
    general: {
      port: 7878,
      open_browser_on_start: true,
      audio_input_device: "",
      first_run_completed: true,
    },
    providers: [
      {
        id: "01HXXXXXXXXXXXXXXXXXXXXXXX",
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        model: "gpt-4o-mini",
        is_active: true,
        created_at: 0,
        api_key_masked: "••••ABCD",
      },
    ],
    presets: [],
  };
  return {
    settingsApi: { get: vi.fn().mockResolvedValue(fixture) },
    settingsKey: ["settings"],
    useSettings: () => ({
      data: fixture,
      isLoading: false,
      error: null,
    }),
    useDataPaths: () => undefined,
    useSetFirstRunCompleted: () => ({
      mutateAsync: vi.fn().mockResolvedValue(fixture.general),
      isPending: false,
    }),
  };
});

// Granted by default so the Shell never redirects.
const screen_ = vi.hoisted(() => ({ granted: true }));
vi.mock("./hooks/useScreenRecordingStatus", () => ({
  useScreenRecordingStatus: () => ({
    granted: screen_.granted,
    status: screen_.granted ? "granted" : "denied",
    isLoading: false,
    error: null,
  }),
}));

// jsdom's AbortSignal is not the one Node's Request accepts, and React Router builds a Request to run a lazy route, so the tests load them up front.
async function eager(rs: RouteObject[]): Promise<RouteObject[]> {
  return Promise.all(
    rs.map(async ({ lazy, children, ...rest }) => ({
      ...rest,
      ...(typeof lazy === "function" ? await lazy() : {}),
      ...(children ? { children: await eager(children) } : {}),
    })) as Promise<RouteObject>[],
  );
}

function makeQc(): QueryClient {
  return new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: 0 } },
  });
}

describe("router", () => {
  it("keeps the sidebar, and so Settings, on the Library's screen-recording denial", async () => {
    screen_.granted = false;
    try {
      const router = createMemoryRouter(await eager(routes), { initialEntries: ["/starred"] });
      render(
        <QueryClientProvider client={makeQc()}>
          <RouterProvider router={router} />
        </QueryClientProvider>,
      );
      expect(await screen.findByRole("link", { name: "Settings" })).toHaveAttribute("href", "/settings");
    } finally {
      screen_.granted = true;
    }
  });

  it("renders the Library at /", async () => {
    const qc = makeQc();
    const router = createMemoryRouter(await eager(routes), { initialEntries: ["/"] });
    render(
      <QueryClientProvider client={qc}>
        <RouterProvider router={router} />
      </QueryClientProvider>,
    );
    // The Library hero greeting always says "Good {timeOfDay}" —
    // pick a stable substring.
    expect(
      await screen.findByRole("heading", {
        name: /Good (morning|afternoon|evening)/i,
      }),
    ).toBeInTheDocument();
  });

  it("renders the StyleGuide at /style-guide", async () => {
    const qc = makeQc();
    const router = createMemoryRouter(await eager(routes), {
      initialEntries: ["/style-guide"],
    });
    render(
      <QueryClientProvider client={qc}>
        <RouterProvider router={router} />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByRole("heading", { name: /style guide/i }),
    ).toBeInTheDocument();
  });

  it("renders the Welcome route at /welcome", async () => {
    const qc = makeQc();
    const router = createMemoryRouter(await eager(routes), {
      initialEntries: ["/welcome"],
    });
    render(
      <QueryClientProvider client={qc}>
        <RouterProvider router={router} />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByRole("heading", { name: /welcome to yogurt/i }),
    ).toBeInTheDocument();
  });
});
