import { test, expect, type Page } from "@playwright/test";

const SETTINGS = {
  general: { port: 7878, first_run_completed: true, stt_provider: "local", stt_model: "large-v3", live_recap: true, live_recap_placement: "main" },
  providers: [],
  presets: [],
};

// Each radiogroup whose thumb starts a transition, recorded from page load, so no check races the 260ms slide.
async function recordSlides(page: Page) {
  await page.addInitScript(() => {
    const w = window as unknown as { slid: string[] };
    w.slid = [];
    document.addEventListener("transitionrun", (e) => {
      const t = e.target as HTMLElement;
      if (t.matches('[role="radiogroup"] > span[aria-hidden]') && e.propertyName === "transform") w.slid.push(t.parentElement!.getAttribute("aria-label")!);
    });
  });
}
const slides = (page: Page) => page.evaluate(() => (window as unknown as { slid: string[] }).slid);

async function open(page: Page, path = "/settings/general") {
  await recordSlides(page);
  await page.routeWebSocket(/\/ws(\/|\?|$)/, () => {});
  await page.route("**/api/session-token", (r) => r.fulfill({ json: { token: "e2e-token" } }));
  await page.route(/\/api\/settings(\?|$)/, (r) => r.fulfill({ json: SETTINGS }));
  // Left to the dev proxy this fails late, and the rows it decides then mount while a thumb is being watched.
  await page.route("**/api/license", (r) => r.fulfill({ json: { license: null } }));
  await page.goto(path);
  // A web font that lands late resizes the options. A read just then finds the thumb at its old width, and a click in that frame gets re-placed without the slide a test waits for.
  await page.getByRole("radiogroup").first().waitFor();
  await page.evaluate(async () => void (await document.fonts.ready));
}

const appearance = (page: Page) => page.getByRole("radiogroup", { name: "Appearance" });

const onPicked = (group: ReturnType<Page["getByRole"]>) =>
  group.evaluate((g) => {
    const t = g.querySelector<HTMLElement>(":scope > span[aria-hidden]")!.getBoundingClientRect();
    const r = g.querySelector('[aria-checked="true"]')!.getBoundingClientRect();
    return Math.abs(t.left - r.left) < 0.5 && Math.abs(t.width - r.width) < 0.5;
  });

test("the Segmented thumb starts on the picked option and slides to a new pick", async ({ page }) => {
  await open(page);
  await expect(appearance(page).getByRole("radio", { name: "System" })).toHaveAttribute("aria-checked", "true");
  expect(await onPicked(appearance(page))).toBe(true);
  expect(await slides(page)).toEqual([]);

  await appearance(page).getByRole("radio", { name: "Dark" }).click();
  await expect(appearance(page).getByRole("radio", { name: "Dark" })).toHaveAttribute("aria-checked", "true");
  await expect.poll(() => slides(page)).toEqual(["Appearance"]);
});

test("the Segmented thumb follows a resize in place, without sliding", async ({ page }) => {
  await open(page);
  const group = appearance(page);
  await expect(group.getByRole("radio", { name: "System" })).toHaveAttribute("aria-checked", "true");
  await group.evaluate((g) => {
    for (const b of g.querySelectorAll<HTMLElement>('[role="radio"]')) b.style.paddingInline = "40.5px";
    // The ResizeObserver reports within the next frame.
    return new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
  });
  expect(await onPicked(group)).toBe(true);
  expect(await slides(page)).toEqual([]);
});

test("under reduced motion the Segmented thumb jumps straight to a new pick", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await open(page);
  await appearance(page).getByRole("radio", { name: "Dark" }).click();
  await expect(appearance(page).getByRole("radio", { name: "Dark" })).toHaveAttribute("aria-checked", "true");
  expect(await onPicked(appearance(page))).toBe(true);
  expect(await slides(page)).toEqual([]);
});

test("the Library switch's thumb slides from Meetings to Voice, though each page mounts its own switch", async ({ page }) => {
  await open(page, "/");
  await page.getByRole("radiogroup", { name: "Library" }).getByRole("radio", { name: "Voice" }).click();
  await expect(page.getByRole("radiogroup", { name: "Library" }).getByRole("radio", { name: "Voice" })).toHaveAttribute("aria-checked", "true");
  await expect.poll(() => slides(page)).toEqual(["Library"]);
});
