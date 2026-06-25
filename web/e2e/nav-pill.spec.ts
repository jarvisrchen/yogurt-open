import { test, expect, type Page } from "@playwright/test";
import { onCurrent, open, slides } from "./fixtures/nav-pill";

const voiceNav = (page: Page, name: string) => page.getByRole("link", { name: new RegExp(`^${name}`) });

test("the Voice nav pill starts on the current page and slides to the next", async ({ page }) => {
  await open(page, "/voice/hands-free");
  await expect(voiceNav(page, "Hands-free")).toHaveAttribute("aria-current", "page");
  expect(await onCurrent(page)).toBe(true);
  expect(await slides(page)).toBe(0);

  await voiceNav(page, "Home").click();
  await expect(voiceNav(page, "Home")).toHaveAttribute("aria-current", "page");
  await expect.poll(() => slides(page)).toBe(1);
  expect(await onCurrent(page)).toBe(true);
});

test("the Voice nav pill slides to an app's history and to Hands-free", async ({ page }) => {
  await open(page, "/voice");
  await expect(voiceNav(page, "Slack")).toBeVisible();
  expect(await onCurrent(page)).toBe(true);
  expect(await slides(page)).toBe(0);

  await voiceNav(page, "Slack").click();
  await expect(voiceNav(page, "Slack")).toHaveAttribute("aria-current", "page");
  await expect.poll(() => slides(page)).toBe(1);
  expect(await onCurrent(page)).toBe(true);

  await voiceNav(page, "Hands-free").click();
  await expect(voiceNav(page, "Hands-free")).toHaveAttribute("aria-current", "page");
  await expect.poll(() => slides(page)).toBe(2);
  expect(await onCurrent(page)).toBe(true);
});

test("under reduced motion the Voice nav pill jumps to the next page", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await open(page, "/voice");
  await voiceNav(page, "Hands-free").click();
  await expect(voiceNav(page, "Hands-free")).toHaveAttribute("aria-current", "page");
  expect(await onCurrent(page)).toBe(true);
  expect(await slides(page)).toBe(0);
});

test("the Library nav pill slides from All meetings to Starred", async ({ page }) => {
  await open(page, "/");
  await expect(page.getByRole("link", { name: "All meetings" })).toHaveAttribute("aria-current", "page");
  expect(await onCurrent(page)).toBe(true);
  expect(await slides(page)).toBe(0);

  await page.getByRole("link", { name: "Starred" }).click();
  await expect(page.getByRole("link", { name: "Starred" })).toHaveAttribute("aria-current", "page");
  await expect.poll(() => slides(page)).toBe(1);
  expect(await onCurrent(page)).toBe(true);
});

test("the Settings nav pill slides from one section to the next", async ({ page }) => {
  await open(page, "/settings/transcription");
  const sections = page.getByRole("navigation", { name: "Settings sections" });
  await expect(sections.getByRole("button", { name: "Transcription" })).toHaveAttribute("aria-current", "page");
  expect(await onCurrent(page)).toBe(true);
  expect(await slides(page)).toBe(0);

  await sections.getByRole("button", { name: "General" }).click();
  await expect(sections.getByRole("button", { name: "General" })).toHaveAttribute("aria-current", "page");
  await expect.poll(() => slides(page)).toBe(1);
  expect(await onCurrent(page)).toBe(true);
});

test("after Voice and back, the Library nav pill keeps to its label row when the rows shift", async ({ page }) => {
  const label = { id: "l1", name: "Clients", color: "blue", position: 0, updated_at: 0, meeting_count: 1, archived_count: 0 };
  await open(page, "/voice", [label]);
  await page.getByRole("radio", { name: "Meetings" }).click();
  await expect(page.getByRole("link", { name: "All meetings" })).toHaveAttribute("aria-current", "page");
  await page.getByRole("link", { name: /^Clients/ }).click();
  await expect(page.getByRole("link", { name: /^Clients/ })).toHaveAttribute("aria-current", "page");
  await expect.poll(() => onCurrent(page)).toBe(true);

  // The new-label field opens above the label rows and pushes them down.
  await page.getByRole("button", { name: "New label" }).click();
  await expect(page.locator("aside").getByRole("textbox")).toBeVisible();
  await expect.poll(() => onCurrent(page)).toBe(true);
});
