import type { Page } from "@playwright/test";
import { STATS } from "./voice-history";

const APPS = [{ name: "Notes", words: 12, count: 2 }, { name: "Slack", words: 8, count: 1 }];

const SETTINGS = {
  general: { port: 7878, first_run_completed: true, stt_provider: "local", stt_model: "large-v3", voice_enabled: true, voice_hotkey: "fn" },
  // An active provider keeps the first-run redirect from sending `/` to Welcome.
  providers: [{ id: "p1", name: "Local", base_url: "http://localhost:11434/v1", model: "m", is_active: true, created_at: 1, api_key_masked: "" }],
  presets: [],
};

// Every slide a sidebar's nav pill starts, recorded from page load, so no check races the 300ms slide.
export async function open(page: Page, path: string, labels: unknown[] = []) {
  await page.addInitScript(() => {
    const w = window as unknown as { slid: number };
    w.slid = 0;
    document.addEventListener("transitionrun", (e) => {
      if ((e.target as HTMLElement).matches(".nav-pill") && e.propertyName === "transform") w.slid++;
    });
  });
  await page.routeWebSocket(/\/ws(\/|\?|$)/, () => {});
  await page.route("**/api/session-token", (r) => r.fulfill({ json: { token: "e2e-token" } }));
  await page.route(/\/api\/settings(\?|$)/, (r) => r.fulfill({ json: SETTINGS }));
  await page.route("**/api/audio/permission", (r) => r.fulfill({ json: { screen_recording: "granted", microphone: "granted" } }));
  await page.route("**/api/labels", (r) => r.fulfill({ json: labels }));
  await page.route(/\/api\/meetings(\?|$)/, (r) => r.fulfill({ json: { meetings: [], total: 0 } }));
  const history = { ...STATS.history, hands_free_count: 1, apps: APPS };
  await page.route(/\/api\/voice\/stats/, (r) => r.fulfill({ json: { ...STATS, history } }));
  await page.route(/\/api\/voice\/dictations/, (r) => r.fulfill({ json: { dictations: [], total: 0, history, window_hours: null } }));
  await page.route("**/api/templates", (r) => r.fulfill({ json: [] }));
  await page.goto(path);
}

export const slides = (page: Page) => page.evaluate(() => (window as unknown as { slid: number }).slid);

// The pill is placed on, or headed for, the current row: its set transform, rather than its rect, which moves through a slide.
export const onCurrent = (page: Page) =>
  page.evaluate(() => {
    const p = document.querySelector<HTMLElement>(".nav-pill")!;
    const r = p.parentElement!.querySelector('[aria-current="page"]')!.getBoundingClientRect();
    const top = p.parentElement!.getBoundingClientRect().top + new DOMMatrix(p.style.transform).m42;
    return Math.abs(top - r.top) < 0.5 && Math.abs(parseFloat(p.style.height) - r.height) < 0.5;
  });
