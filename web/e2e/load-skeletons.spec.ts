import { test, expect, type Page, type Route } from "@playwright/test";

// Set to a folder to also save light and dark screenshots of each state, as for a PR.
const SHOTS = process.env.YOGURT_E2E_SHOTS;

const now = Date.now();
const meeting = (i: number, title: string, labels: { id: string; name: string; color: string }[] = []) => ({
  id: `019f0000-0000-7000-8000-00000000000${i}`,
  title,
  started_at: now - i * 3_600_000,
  ended_at: now - i * 3_600_000 + 1_800_000,
  starred: false,
  stt_engine: "local · parakeet-v3",
  llm_model: "MiniMax-M2",
  has_enriched: true,
  labels,
});
const ROWS = [
  meeting(1, "Roadmap sync", [{ id: "l1", name: "Planning", color: "blue" }]),
  meeting(2, "Design review: onboarding", [{ id: "l2", name: "Design", color: "straw" }]),
  meeting(3, "Candidate debrief"),
];
const FULL = {
  ...ROWS[0],
  notes_md: "- ship it",
  enriched_md: "## Decisions\n\n- Ship onboarding v2 behind a flag\n- Keep the import step optional\n",
  transcript_json: "[]",
  template: null,
  created_at: "2026-11-07T00:00:00.000Z",
  updated_at: "2026-11-07T00:00:00.000Z",
};
const SETTINGS = {
  general: { port: 7878, open_browser_on_start: true, audio_input_device: "", first_run_completed: true, stt_provider: "local", stt_model: "parakeet-v3" },
  providers: [{ id: "p1", name: "Mock", base_url: "http://mock/v1", model: "m", is_active: true, created_at: 1, api_key_masked: "••••abcd" }],
  presets: [],
};

function gate() {
  let open!: () => void;
  const opened = new Promise<void>((r) => (open = r));
  return { open, wait: () => opened };
}

async function mockBackend(page: Page, holds: { list?: Promise<void>; meeting?: Promise<void> } = {}) {
  const gets: string[] = [];
  await page.routeWebSocket(/\/ws(\/|\?|$)/, () => {});
  await page.route("**/api/session-token", (r) => r.fulfill({ json: { token: "e2e-token" } }));
  await page.route(/\/api\/settings(\?|$)/, (r) => r.fulfill({ json: SETTINGS }));
  await page.route("**/api/audio/permission", (r) => r.fulfill({ json: { screen_recording: "granted", microphone: "granted" } }));
  await page.route(/\/api\/meetings\/[0-9a-f-]+(\?|$)/, async (r: Route) => {
    gets.push(r.request().url());
    await holds.meeting;
    await r.fulfill({ json: FULL });
  });
  await page.route(/\/api\/meetings(\?|$)/, async (r) => {
    await holds.list;
    await r.fulfill({ json: { meetings: ROWS, total: ROWS.length } });
  });
  return gets;
}

async function shoot(page: Page, name: string) {
  if (!SHOTS) return;
  // The dev server's query devtools button sits over the sidebar's Settings link.
  await page.addStyleTag({ content: ".tsqd-parent-container { display: none !important; }" });
  for (const scheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme: scheme, reducedMotion: "reduce" });
    await page.evaluate((t) => (document.documentElement.dataset.theme = t), scheme);
    // Cards ease their colors over 150ms after the theme flips.
    await page.waitForTimeout(400);
    await page.screenshot({ path: `${SHOTS}/${name}-${scheme}.png` });
  }
}

test.use({ viewport: { width: 1280, height: 800 } });

test("the Library skeleton holds the cards' place, so the rows fill in without moving", async ({ page }) => {
  const list = gate();
  await mockBackend(page, { list: list.wait() });
  await page.goto("/");

  const skeleton = page.getByTestId("library-skeleton");
  await expect(skeleton).toBeVisible();
  const greeting = page.locator("header p").first();
  const before = { card: await skeleton.locator("li > div").first().boundingBox(), line: await greeting.boundingBox() };
  await shoot(page, "library-skeleton");

  list.open();
  const card = page.getByRole("link", { name: /Roadmap sync/ });
  await expect(card).toBeVisible();
  await expect(skeleton).toHaveCount(0);
  const after = { card: await card.boundingBox(), line: await greeting.boundingBox() };
  await shoot(page, "library-loaded");

  expect(after.card).toEqual(before.card);
  expect(after.line?.height).toBe(before.line?.height);
});

test("a meeting draws its header from the Library row while its notes load, and opens with one request", async ({ page }) => {
  const body = gate();
  const gets = await mockBackend(page, { meeting: body.wait() });
  await page.addInitScript(() => {
    const w = window as unknown as { vtCount: number };
    w.vtCount = 0;
    const start = document.startViewTransition?.bind(document);
    if (start)
      document.startViewTransition = ((cb: () => void) => {
        w.vtCount++;
        return start(cb);
      }) as typeof document.startViewTransition;
  });
  await page.goto("/");
  await page.getByRole("link", { name: /Roadmap sync/ }).click();

  await expect(page).toHaveURL(/\/meeting\/[0-9a-f-]+\/post$/);
  await expect(page.getByTestId("notes-skeleton")).toBeVisible();
  await expect(page.locator("[data-testid=meeting-post-route]").getByText("Roadmap sync")).toBeVisible();
  await expect(page.locator("[data-testid=meeting-post-route]").getByText("Planning")).toBeVisible();
  await shoot(page, "meeting-skeleton");

  body.open();
  await expect(page.getByText("Ship onboarding v2 behind a flag")).toBeVisible();
  await expect(page.getByTestId("notes-skeleton")).toHaveCount(0);
  await shoot(page, "meeting-loaded");
  expect(gets).toHaveLength(1);
  expect(await page.evaluate(() => (window as unknown as { vtCount: number }).vtCount)).toBe(1);

  await page.getByRole("link", { name: "Library" }).first().click();
  await expect(page.getByRole("link", { name: /Roadmap sync/ })).toBeVisible();
  expect(await page.evaluate(() => (window as unknown as { vtCount: number }).vtCount)).toBe(2);
});

test("Settings opens in its own frame, with shimmer only where its values go", async ({ page }) => {
  const settings = gate();
  await mockBackend(page);
  await page.route(/\/api\/settings(\?|$)/, async (r) => {
    await settings.wait();
    await r.fulfill({ json: SETTINGS });
  });
  await page.goto("/settings/model");
  await expect(page.getByTestId("settings-loading")).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Settings sections" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Model" })).toBeVisible();
  await shoot(page, "settings-skeleton");
  settings.open();
  await expect(page.getByTestId("settings-loading")).toHaveCount(0);
});

test("+ New meeting glides into the live page's recording control", async ({ page }) => {
  await mockBackend(page);
  const created = { ...FULL, id: "019f0000-0000-7000-8000-0000000000aa", title: "New meeting", ended_at: null, enriched_md: null, notes_md: "" };
  await page.route(/\/api\/meetings(\?|$)/, async (r) =>
    r.request().method() === "POST" ? r.fulfill({ json: created }) : r.fulfill({ json: { meetings: ROWS, total: ROWS.length } }),
  );
  await page.route(/\/api\/meetings\/[0-9a-f-]+\/start$/, (r) => r.fulfill({ status: 500, body: "no capture in e2e" }));
  await page.addInitScript(() => {
    const w = window as unknown as { morph: { old: string[]; next: string[] }[] };
    w.morph = [];
    const names = () =>
      [...document.querySelectorAll<HTMLElement>("*")]
        .filter((e) => e.style.viewTransitionName === "record-control")
        .map((e) => e.textContent?.trim() ?? "");
    const start = document.startViewTransition.bind(document);
    document.startViewTransition = ((cb: () => Promise<void>) => {
      const old = names();
      return start(async () => {
        await cb();
        w.morph.push({ old, next: names() });
      });
    }) as typeof document.startViewTransition;
  });
  await page.goto("/");
  await page.getByRole("button", { name: "New meeting" }).click();
  await expect(page).toHaveURL(/\/meeting\/019f0000-0000-7000-8000-0000000000aa$/);
  await expect.poll(() => page.evaluate(() => (window as unknown as { morph: unknown[] }).morph.length)).toBe(1);
  const [morph] = await page.evaluate(() => (window as unknown as { morph: { old: string[]; next: string[] }[] }).morph);
  expect(morph.old).toEqual(["New meeting"]);
  expect(morph.next).toHaveLength(1);
  expect(morph.next[0]).toMatch(/recording/i);
});
