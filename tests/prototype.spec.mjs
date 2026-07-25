import { expect, test } from "@playwright/test";

test("Dioxus WASM joins and leaves a LiveKit Room Session", async ({ page }) => {
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));

  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Dioxus meets LiveKit" })).toBeVisible();
  const sdkScript = page.locator('script[src$="/livekit-client.umd.min.js"]');
  const sdkUrl = new URL(await sdkScript.getAttribute("src"), page.url());
  expect(sdkUrl.origin).toBe(new URL(page.url()).origin);
  expect(await page.evaluate(() => window.LivekitClient?.version)).toBe("2.20.1");
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await expect(page.locator("#camera-state")).toHaveText("off");
  await expect(page.locator("#microphone-state")).toHaveText("off");
  await expect(page.locator("#participant-count")).toHaveText("1");
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");

  await page.getByRole("button", { name: "Leave room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("disconnected");
  expect(pageErrors).toEqual([]);
});

test("controls fit a phone-sized browser", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");

  await expect(page.getByRole("button", { name: "Join room" })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});
