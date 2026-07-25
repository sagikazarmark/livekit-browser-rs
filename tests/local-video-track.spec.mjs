import { expect, test } from "@playwright/test";

async function installWebKitCamera(page, browserName) {
  if (browserName !== "webkit") return;
  await page.evaluate(() => {
    const mediaDevices = navigator.mediaDevices;
    Object.defineProperty(navigator, "mediaDevices", {
      configurable: true,
      value: mediaDevices,
    });
    const canvas = document.createElement("canvas");
    canvas.width = 640;
    canvas.height = 360;
    const context = canvas.getContext("2d");
    context.fillStyle = "#d9ff43";
    context.fillRect(0, 0, canvas.width, canvas.height);
    const stream = canvas.captureStream(30);
    const original = mediaDevices.getUserMedia.bind(mediaDevices);
    Object.defineProperty(mediaDevices, "getUserMedia", {
      configurable: true,
      writable: true,
      value: async (constraints) => {
        if (!constraints?.video) return original(constraints);
        if (window.__localVideoCalls) window.__localVideoCalls.userMedia += 1;
        return stream.clone();
      },
    });
    window.__localVideoWebKitCamera = { canvas, stream };
  });
}

async function instrumentTrackStops(page) {
  await page.addInitScript(() => {
    window.__localVideoStopCalls = 0;
    const original = MediaStreamTrack.prototype.stop;
    MediaStreamTrack.prototype.stop = function (...args) {
      window.__localVideoStopCalls += 1;
      return original.apply(this, args);
    };
  });
}

test("captures and previews camera before joining without publication", async ({
  page,
  browserName,
}) => {
  await page.addInitScript(() => {
    const original = navigator.mediaDevices.getUserMedia;
    window.__localVideoCalls = { userMedia: 0 };
    navigator.mediaDevices.getUserMedia = function (...args) {
      window.__localVideoCalls.userMedia += 1;
      return original.apply(this, args);
    };
  });
  await page.goto("/");
  await installWebKitCamera(page, browserName);

  await page.getByRole("button", { name: "Start camera" }).click();

  await expect(page.locator("#camera-state")).toHaveText("on");
  await expect(page.locator("#local-video-track")).toHaveAttribute("data-stopped", "false");
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");
  await expect
    .poll(() => page.locator("#local-video").evaluate((video) => video.srcObject?.active === true))
    .toBe(true);
  expect(await page.evaluate(() => window.__localVideoCalls.userMedia)).toBe(1);
});

test("captures and previews camera after joining without implicit publication", async ({
  page,
  browserName,
}) => {
  await page.goto("/");
  await installWebKitCamera(page, browserName);
  await page.getByLabel("Room name").fill(`post-join-camera-${Date.now()}`);
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");

  await page.getByRole("button", { name: "Start camera" }).click();

  await expect(page.locator("#camera-state")).toHaveText("on");
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");
  await expect
    .poll(() => page.locator("#local-video").evaluate((video) => video.srcObject?.active === true))
    .toBe(true);
  await page.getByRole("button", { name: "Leave room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("disconnected");
});

test("publishes typed camera options while mute and unpublish keep independent lifecycles", async ({
  page,
  browserName,
}) => {
  await page.goto("/");
  await installWebKitCamera(page, browserName);
  await page.getByLabel("Room name").fill(`camera-publication-${Date.now()}`);
  await page.evaluate(() => {
    const calls = { publishOptions: null, unpublishStopOnUnpublish: null };
    window.__localVideoPublicationCalls = calls;
    const prototype = window.LivekitClient.LocalParticipant.prototype;
    const originalPublish = prototype.publishTrack;
    const originalUnpublish = prototype.unpublishTrack;
    prototype.publishTrack = function (track, options) {
      calls.publishOptions = structuredClone(options);
      return originalPublish.call(this, track, options);
    };
    prototype.unpublishTrack = function (track, stopOnUnpublish) {
      calls.unpublishStopOnUnpublish = stopOnUnpublish;
      return originalUnpublish.call(this, track, stopOnUnpublish);
    };
  });

  await page.getByRole("button", { name: "Start camera" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });

  await page.getByRole("button", { name: "Publish camera" }).click();

  const publication = page.locator(".local-track-publication");
  await expect(publication).toHaveCount(1);
  await expect(publication).toHaveAttribute("data-source", "camera");
  await expect(publication).toHaveAttribute("data-name", "camera-preview");
  await expect(publication).toHaveAttribute("data-muted", "false");
  expect(await page.evaluate(() => window.__localVideoPublicationCalls.publishOptions)).toEqual({
    source: "camera",
    name: "camera-preview",
    videoCodec: "vp8",
    simulcast: true,
    videoEncoding: { maxBitrate: 800_000, maxFramerate: 30 },
  });

  await page.getByRole("button", { name: "Mute camera publication" }).click();
  await expect(publication).toHaveAttribute("data-muted", "true");
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "1");
  await expect(page.locator("#local-video-track")).toHaveAttribute(
    "data-preview-attached",
    "true",
  );

  await page.getByRole("button", { name: "Unpublish camera" }).click();
  await expect(publication).toHaveCount(0);
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");
  await expect(page.locator("#camera-state")).toHaveText("on");
  await expect(page.locator("#local-video-track")).toHaveAttribute(
    "data-preview-attached",
    "true",
  );
  expect(
    await page.evaluate(() => window.__localVideoPublicationCalls.unpublishStopOnUnpublish),
  ).toBe(false);
  await page.getByRole("button", { name: "Leave room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("disconnected");
});

test("explicit stop is terminal and releases capture and preview", async ({ page, browserName }) => {
  await instrumentTrackStops(page);
  await page.goto("/");
  await installWebKitCamera(page, browserName);
  await page.getByRole("button", { name: "Start camera" }).click();
  await expect(page.locator("#local-video-track")).toHaveAttribute(
    "data-preview-attached",
    "true",
  );

  await page.getByRole("button", { name: "Stop camera" }).click();

  await expect(page.locator("#camera-state")).toHaveText("off");
  await expect(page.locator("#local-video-track")).toHaveAttribute("data-stopped", "true");
  await expect(page.locator("#local-video-track")).toHaveAttribute(
    "data-preview-attached",
    "false",
  );
  await expect.poll(() => page.locator("#local-video").evaluate((video) => video.srcObject)).toBeNull();
  expect(await page.evaluate(() => window.__localVideoStopCalls)).toBeGreaterThan(0);
});

test("explicit stop can be retried after browser cleanup fails", async ({ page, browserName }) => {
  await page.addInitScript(() => {
    const original = MediaStreamTrack.prototype.stop;
    window.__localVideoStopAttempts = 0;
    MediaStreamTrack.prototype.stop = function (...args) {
      window.__localVideoStopAttempts += 1;
      if (window.__localVideoStopAttempts === 1) throw new Error("synthetic stop failure");
      return original.apply(this, args);
    };
  });
  await page.goto("/");
  await installWebKitCamera(page, browserName);
  await page.getByRole("button", { name: "Start camera" }).click();

  await page.getByRole("button", { name: "Stop camera" }).click();
  await expect(page.locator(".notice.error")).toContainText("synthetic stop failure");
  await expect(page.locator("#local-video-track")).toHaveAttribute("data-stopped", "true");

  await page.locator("#retry-local-video-stop").dispatchEvent("click");

  await expect.poll(() => page.evaluate(() => window.__localVideoStopAttempts)).toBe(2);
  await expect(page.locator(".notice.error")).toHaveCount(0);
});

test("dropping the final Local Video Track owner stops capture as a fallback", async ({
  page,
  browserName,
}) => {
  await instrumentTrackStops(page);
  await page.goto("/");
  await installWebKitCamera(page, browserName);
  await page.getByRole("button", { name: "Start camera" }).click();

  await page.locator("#drop-local-video-track").dispatchEvent("click");

  await expect(page.locator("#local-video-track")).toHaveAttribute("data-present", "false");
  await expect.poll(() => page.evaluate(() => window.__localVideoStopCalls)).toBeGreaterThan(0);
  await expect.poll(() => page.locator("#local-video").evaluate((video) => video.srcObject)).toBeNull();
});

test("final-owner cleanup keeps retrying transient browser stop failures", async ({
  page,
  browserName,
}) => {
  await page.addInitScript(() => {
    const original = MediaStreamTrack.prototype.stop;
    window.__localVideoStopAttempts = 0;
    MediaStreamTrack.prototype.stop = function (...args) {
      window.__localVideoStopAttempts += 1;
      if (window.__localVideoStopAttempts < 5) throw new Error("transient stop failure");
      return original.apply(this, args);
    };
  });
  await page.goto("/");
  await installWebKitCamera(page, browserName);
  await page.getByRole("button", { name: "Start camera" }).click();

  await page.locator("#drop-local-video-track").dispatchEvent("click");

  await expect.poll(() => page.evaluate(() => window.__localVideoStopAttempts)).toBe(5);
  await expect(page.locator("#local-video-track")).toHaveAttribute("data-present", "false");
  await expect.poll(() => page.locator("#local-video").evaluate((video) => video.srcObject)).toBeNull();
});

test("dropping the preview attachment does not stop or publish capture", async ({
  page,
  browserName,
}) => {
  await instrumentTrackStops(page);
  await page.goto("/");
  await installWebKitCamera(page, browserName);
  await page.getByRole("button", { name: "Start camera" }).click();

  await page.locator("#drop-local-video-preview").dispatchEvent("click");

  await expect(page.locator("#local-video-track")).toHaveAttribute(
    "data-preview-attached",
    "false",
  );
  await expect(page.locator("#local-video-track")).toHaveAttribute("data-stopped", "false");
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");
  await expect(page.locator("#camera-state")).toHaveText("on");
  await expect.poll(() => page.locator("#local-video").evaluate((video) => video.srcObject)).toBeNull();
  expect(await page.evaluate(() => window.__localVideoStopCalls)).toBe(0);
});

test("explicitly closing the preview attachment leaves capture private and active", async ({
  page,
  browserName,
}) => {
  await instrumentTrackStops(page);
  await page.goto("/");
  await installWebKitCamera(page, browserName);
  await page.getByRole("button", { name: "Start camera" }).click();

  await page.locator("#close-local-video-preview").dispatchEvent("click");

  await expect(page.locator("#local-video-track")).toHaveAttribute(
    "data-preview-attached",
    "false",
  );
  await expect(page.locator("#local-video-track")).toHaveAttribute("data-stopped", "false");
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");
  await expect(page.locator("#camera-state")).toHaveText("on");
  await expect.poll(() => page.locator("#local-video").evaluate((video) => video.srcObject)).toBeNull();
  expect(await page.evaluate(() => window.__localVideoStopCalls)).toBe(0);
});

test("a stopped published track cannot reacquire camera through unmute", async ({
  page,
  browserName,
}) => {
  await page.addInitScript(() => {
    const original = navigator.mediaDevices.getUserMedia;
    window.__localVideoCalls = { userMedia: 0 };
    navigator.mediaDevices.getUserMedia = function (...args) {
      window.__localVideoCalls.userMedia += 1;
      return original.apply(this, args);
    };
  });
  await page.goto("/");
  await installWebKitCamera(page, browserName);
  await page.getByLabel("Room name").fill(`stopped-publication-${Date.now()}`);
  await page.getByRole("button", { name: "Start camera" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish camera" }).click();
  await expect(page.locator(".local-track-publication")).toHaveCount(1);
  await page.getByRole("button", { name: "Mute camera publication" }).click();
  await expect(page.locator(".local-track-publication")).toHaveAttribute("data-muted", "true");

  await page.getByRole("button", { name: "Stop camera" }).click();
  await page.locator("#unmute-local-video-publication").dispatchEvent("click");

  await expect(page.locator(".notice.error")).toContainText("capture has stopped");
  await expect(page.locator("#local-video-track")).toHaveAttribute("data-stopped", "true");
  expect(await page.evaluate(() => window.__localVideoCalls.userMedia)).toBe(1);
  await page.getByRole("button", { name: "Leave room" }).click();
});

test("abandoning an in-flight capture future releases the eventual camera", async ({
  page,
  browserName,
}) => {
  await instrumentTrackStops(page);
  await page.goto("/");
  await installWebKitCamera(page, browserName);
  await page.evaluate(() => {
    const original = window.LivekitClient.createLocalVideoTrack;
    window.LivekitClient.createLocalVideoTrack = async (...args) => {
      const track = await original(...args);
      await new Promise((resolve) => setTimeout(resolve, 500));
      return track;
    };
  });

  await page.locator("#abandon-camera-capture").dispatchEvent("click");

  await expect.poll(() => page.evaluate(() => window.__localVideoStopCalls)).toBeGreaterThan(0);
  await expect(page.locator("#local-video-track")).toHaveAttribute("data-present", "false");
  await expect(page.locator("#camera-state")).toHaveText("off");
});

test("stop is serialized after an in-flight publication", async ({ page, browserName }) => {
  await instrumentTrackStops(page);
  await page.goto("/");
  await installWebKitCamera(page, browserName);
  await page.getByLabel("Room name").fill(`serialized-stop-${Date.now()}`);
  await page.getByRole("button", { name: "Start camera" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.evaluate(() => {
    const participant = window.LivekitClient.LocalParticipant.prototype;
    const original = participant.publishTrack;
    participant.publishTrack = async function (...args) {
      await new Promise((resolve) => setTimeout(resolve, 500));
      return original.apply(this, args);
    };
  });

  await page.getByRole("button", { name: "Publish camera" }).click();
  await page.getByRole("button", { name: "Stop camera" }).click();

  await expect(page.locator("#local-video-track")).toHaveAttribute("data-stopped", "true");
  await expect(page.locator(".local-track-publication")).toHaveCount(1);
  await expect.poll(() => page.evaluate(() => window.__localVideoStopCalls)).toBeGreaterThan(0);
  await expect(page.locator(".notice.error")).toHaveCount(0);
  await page.getByRole("button", { name: "Leave room" }).click();
});

test("overlapping camera mute and unpublish mutations run in order", async ({
  page,
  browserName,
}) => {
  await page.goto("/");
  await installWebKitCamera(page, browserName);
  await page.getByLabel("Room name").fill(`serialized-video-controls-${Date.now()}`);
  await page.evaluate(() => {
    window.__localVideoMutationEvents = [];
    const track = window.LivekitClient.LocalVideoTrack.prototype;
    const participant = window.LivekitClient.LocalParticipant.prototype;
    const originalMute = track.mute;
    const originalUnpublish = participant.unpublishTrack;
    track.mute = async function (...args) {
      window.__localVideoMutationEvents.push("mute-start");
      await new Promise((resolve) => setTimeout(resolve, 300));
      const result = await originalMute.apply(this, args);
      window.__localVideoMutationEvents.push("mute-end");
      return result;
    };
    participant.unpublishTrack = async function (...args) {
      if (args[0].kind === "video") window.__localVideoMutationEvents.push("unpublish");
      return originalUnpublish.apply(this, args);
    };
  });
  await page.getByRole("button", { name: "Start camera" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish camera" }).click();
  await expect(page.locator('.local-track-publication[data-kind="video"]')).toHaveCount(1);

  await page.getByRole("button", { name: "Mute camera publication" }).click();
  await page.getByRole("button", { name: "Unpublish camera" }).click();

  await expect(page.locator('.local-track-publication[data-kind="video"]')).toHaveCount(0);
  expect(await page.evaluate(() => window.__localVideoMutationEvents)).toEqual([
    "mute-start",
    "mute-end",
    "unpublish",
  ]);
  await expect(page.locator(".notice.error")).toHaveCount(0);
});

test("camera controls fit a phone-sized browser", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");

  await expect(page.getByRole("button", { name: "Start camera" })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
});
