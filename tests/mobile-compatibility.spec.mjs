import { expect, test } from "@playwright/test";

test("mobile-sized consumers reflect browser capabilities and Room Session state", async ({
  page,
}) => {
  await page.goto("/");

  const runtime = page.locator("#browser-runtime");
  await expect(runtime).toHaveAttribute("data-state", "ready");
  const browserCapabilities = await page.evaluate(() => ({
    displayCapture: typeof navigator.mediaDevices?.getDisplayMedia === "function",
    localClipRecording:
      typeof MediaRecorder === "function" &&
      typeof MediaRecorder.isTypeSupported === "function",
    userMedia: typeof navigator.mediaDevices?.getUserMedia === "function",
  }));
  await expect(runtime).toHaveAttribute("data-user-media", String(browserCapabilities.userMedia));
  await expect(runtime).toHaveAttribute(
    "data-display-capture",
    String(browserCapabilities.displayCapture),
  );
  await expect(runtime).toHaveAttribute(
    "data-local-clip-recording",
    String(browserCapabilities.localClipRecording),
  );
  await expect(page.getByRole("button", { name: "Start screen share" })).toBeEnabled();

  await page.getByLabel("Room name").fill(`mobile-${Date.now()}`);
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await expect(page.locator("#dioxus-room-session")).toHaveAttribute("data-connected", "true");
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
  ).toBe(true);
  await page.getByRole("button", { name: "Leave room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("disconnected");
});

test("mobile-sized consumers disable unsupported optional workflows", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator.mediaDevices, "getDisplayMedia", {
      configurable: true,
      value: undefined,
    });
    Object.defineProperty(globalThis, "MediaRecorder", {
      configurable: true,
      value: undefined,
    });
  });

  await page.goto("/");

  const runtime = page.locator("#browser-runtime");
  await expect(runtime).toHaveAttribute("data-state", "ready");
  await expect(runtime).toHaveAttribute("data-display-capture", "false");
  await expect(runtime).toHaveAttribute("data-local-clip-recording", "false");
  await page.getByRole("button", { name: "Start screen share" }).click();
  await expect(page.locator("#media-operation-error")).toHaveAttribute(
    "data-kind",
    "unsupportedBrowser",
  );
  await expect(page.locator("#start-local-clip")).toBeDisabled();
  await expect(page.locator("#screen-capture")).toHaveAttribute("data-present", "false");
  await expect(page.locator("#local-clip-recording")).toHaveAttribute("data-state", "idle");
});
