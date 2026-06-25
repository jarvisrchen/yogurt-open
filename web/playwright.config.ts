import { createHash } from "node:crypto";
import { defineConfig, devices } from "@playwright/test";

// A Vite port of this checkout's own, outside the `just dev` range and derived from its path, so worktrees
// can run the suite at the same time. An existing server is never reused: it could be another worktree's code.
// Two checkouts whose paths hash alike fail with the port in use, and YOGURT_E2E_PORT moves one of them.
const E2E_PORT =
  Number(process.env.YOGURT_E2E_PORT) ||
  5200 + (createHash("sha1").update(process.cwd()).digest().readUInt16BE(0) % 800);

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? "list" : [["list"], ["html", { open: "never" }]],
  use: {
    baseURL: `http://127.0.0.1:${E2E_PORT}`,
    trace: "on-first-retry",
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
  ],
  webServer: {
    // Bind 127.0.0.1 explicitly: bare `vite` binds IPv6 localhost only, which the health check cannot reach.
    command: "pnpm dev --host 127.0.0.1",
    url: `http://127.0.0.1:${E2E_PORT}`,
    env: { YOGURT_VITE_PORT: String(E2E_PORT) },
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
