import { test, expect } from "@playwright/test";
import { DICTATIONS, STATS } from "./fixtures/voice-history";

for (const colorScheme of ["light", "dark"] as const) {
  test(`dictation history shows the saved models with cleanup enabled (${colorScheme})`, async ({ page }) => {
    await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
    await page.setViewportSize({ width: 1280, height: 1000 });
    await page.routeWebSocket(/\/ws(\/|\?|$)/, () => {});
    const responses: Record<string, unknown> = {
      "/api/session-token": { token: "e2e-token" },
      "/api/settings": { general: { first_run_completed: true, voice_enabled: true, voice_cleanup: true,
        voice_hotkey: "option+a", voice_retention: "30d" }, providers: [], presets: [] },
      "/api/voice/stats": STATS,
      "/api/voice/dictations": { dictations: DICTATIONS, total: DICTATIONS.length, history: STATS.history, window_hours: null },
      "/api/cloud/voice-totals": { share: false },
      "/api/meetings": { meetings: [], total: 0 },
      "/api/meetings/active": null,
      "/api/meetings/detected": null,
    };
    await page.route((url) => url.pathname.startsWith("/api/"), (route) => {
      const path = new URL(route.request().url()).pathname;
      return route.fulfill({ json: path in responses ? responses[path] : [] });
    });
    await page.goto("/voice");
    for (const model of ["celeris-1", "gemini-3.5-flash-lite", "qwen3-4b-instruct", "Rules only"]) {
      await expect(page.getByText(`Cleanup: ${model}`, { exact: true })).toBeVisible();
    }
    await expect(page.getByText("Transcription: parakeet-v3", { exact: true })).toHaveCount(4);
    await expect(page.getByText(/model not recorded|Cleanup: Not applied/i)).toHaveCount(0);
    await page.getByText("Cleanup: Rules only", { exact: true }).scrollIntoViewIfNeeded();
    if (process.env.YOGURT_SHOTS_DIR) {
      await page.addStyleTag({ content: ".tsqd-parent-container { display: none !important; }" });
      await page.screenshot({ path: `${process.env.YOGURT_SHOTS_DIR}/voice-models-${colorScheme}.png` });
    }
    const card = page.locator("li").filter({ hasText: "Cleanup: gemini-3.5-flash-lite" });
    await card.evaluate((el) => { el.style.width = "320px"; });
    expect(await card.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
  });
}
