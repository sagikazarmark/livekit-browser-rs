import { expect, test } from "@playwright/test";

async function installScreenFixture(page, withAudio = true) {
  await page.addInitScript(({ withAudio }) => {
    const calls = [];
    const captures = [];
    const addEventListener = MediaStreamTrack.prototype.addEventListener;
    MediaStreamTrack.prototype.addEventListener = function (type, listener, ...args) {
      if (type === "ended" && this.__screenFixture) {
        this.__screenEndedListeners ??= [];
        this.__screenEndedListeners.push(listener);
      }
      return addEventListener.call(this, type, listener, ...args);
    };
    Object.defineProperty(navigator.mediaDevices, "getDisplayMedia", {
      configurable: true,
      value: async (options) => {
        const canvas = document.createElement("canvas");
        canvas.width = 1280;
        canvas.height = 720;
        const context = canvas.getContext("2d");
        context.fillStyle = "#84b8ff";
        context.fillRect(0, 0, canvas.width, canvas.height);
        const video = canvas.captureStream(15).getVideoTracks()[0];
        video.__screenFixture = true;
        const tracks = [video];
        let audio = null;
        let audioContext = null;
        let oscillator = null;
        let destination = null;
        if (withAudio && options?.audio) {
          const AudioContext = window.AudioContext || window.webkitAudioContext;
          audioContext = new AudioContext({ sampleRate: 48_000 });
          oscillator = audioContext.createOscillator();
          destination = audioContext.createMediaStreamDestination();
          oscillator.connect(destination);
          oscillator.start();
          audio = destination.stream.getAudioTracks()[0].clone();
          audio.__screenFixture = true;
          tracks.push(audio);
        }
        calls.push(structuredClone(options));
        const capture = { audio, audioContext, canvas, destination, oscillator, video };
        captures.push(capture);
        return new MediaStream(tracks);
      },
    });
    window.__screenFixture = {
      calls,
      captures,
      end(track) {
        for (const listener of track.__screenEndedListeners || []) {
          if (typeof listener === "function") listener.call(track, new Event("ended"));
          else listener.handleEvent(new Event("ended"));
        }
      },
    };
  }, { withAudio });
}

async function instrumentScreenStops(page) {
  await page.addInitScript(() => {
    window.__screenStopCalls = { audio: 0, video: 0 };
    const original = MediaStreamTrack.prototype.stop;
    MediaStreamTrack.prototype.stop = function (...args) {
      if (this.__screenFixture) window.__screenStopCalls[this.kind] += 1;
      return original.apply(this, args);
    };
  });
}

test("captures required display video and optional system audio before joining", async ({ page }) => {
  await installScreenFixture(page);
  await page.goto("/");

  await page.getByRole("button", { name: "Start screen share" }).click();

  const capture = page.locator("#screen-capture");
  await expect(capture).toHaveAttribute("data-present", "true");
  await expect(capture).toHaveAttribute("data-video-stopped", "false");
  await expect(capture).toHaveAttribute("data-system-audio-present", "true");
  await expect(capture).toHaveAttribute("data-system-audio-stopped", "false");
  await expect(capture).toHaveAttribute("data-preview-attached", "true");
  await expect(capture).toHaveAttribute("data-audio-preview-attached", "true");
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");
  await expect
    .poll(() => page.locator("#screen-video").evaluate((video) => video.srcObject?.active === true))
    .toBe(true);
  await expect
    .poll(() => page.locator("#screen-audio").evaluate((audio) => audio.srcObject?.active === true))
    .toBe(true);
  expect(await page.evaluate(() => window.__screenFixture.calls[0])).toMatchObject({
    audio: true,
    systemAudio: "include",
  });
});

test("requested system audio remains optional when the selected source supplies none", async ({
  page,
}) => {
  await installScreenFixture(page, false);
  await page.goto("/");

  await page.getByRole("button", { name: "Start screen share" }).click();

  await expect(page.locator("#screen-capture")).toHaveAttribute(
    "data-system-audio-present",
    "false",
  );
  await expect(page.locator("#screen-capture")).toHaveAttribute(
    "data-audio-preview-attached",
    "false",
  );
  await expect(page.locator("#screen-capture")).toHaveAttribute("data-video-stopped", "false");
  await expect(page.locator(".notice.error")).toHaveCount(0);
});

test("display chooser cancellation is typed and creates no partial media state", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator.mediaDevices, "getDisplayMedia", {
      configurable: true,
      value: () => Promise.reject(new DOMException("chooser cancelled", "NotAllowedError")),
    });
  });
  await page.goto("/");

  await page.getByRole("button", { name: "Start screen share" }).click();

  await expect(page.locator("#media-operation-error")).toHaveAttribute(
    "data-kind",
    "displayCaptureNotAllowed",
  );
  await expect(page.locator("#screen-capture")).toHaveAttribute("data-present", "false");
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");
});

test("missing display capture is rejected before invoking LiveKit capture", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator.mediaDevices, "getDisplayMedia", {
      configurable: true,
      value: undefined,
    });
  });
  await page.goto("/");

  await page.getByRole("button", { name: "Start screen share" }).click();

  await expect(page.locator("#media-operation-error")).toHaveAttribute(
    "data-kind",
    "unsupportedBrowser",
  );
  await expect(page.locator("#screen-capture")).toHaveAttribute("data-present", "false");
});

test("publishes and unpublishes display video and system audio independently", async ({ page }) => {
  await installScreenFixture(page);
  await page.goto("/");
  await page.getByLabel("Room name").fill(`screen-publication-${Date.now()}`);
  await page.evaluate(() => {
    window.__screenPublishOptions = {};
    const participant = window.LivekitClient.LocalParticipant.prototype;
    const publish = participant.publishTrack;
    const unpublish = participant.unpublishTrack;
    participant.publishTrack = function (track, options) {
      window.__screenPublishOptions[track.kind] = structuredClone(options);
      return publish.call(this, track, options);
    };
    window.__screenUnpublishStop = [];
    participant.unpublishTrack = function (track, stopOnUnpublish) {
      window.__screenUnpublishStop.push({ kind: track.kind, stopOnUnpublish });
      return unpublish.call(this, track, stopOnUnpublish);
    };
  });
  await page.getByRole("button", { name: "Start screen share" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });

  await page.getByRole("button", { name: "Publish screen video" }).click();
  const video = page.locator('.local-track-publication[data-source="screenShare"]');
  await expect(video).toHaveCount(1);
  await expect(page.locator('#lifecycle-guide [data-step="capture"]')).toHaveAttribute(
    "data-complete",
    "true",
  );
  await expect(page.locator('#lifecycle-guide [data-step="publish"]')).toHaveAttribute(
    "data-complete",
    "true",
  );
  await page.getByRole("button", { name: "Publish system audio" }).click();
  const audio = page.locator('.local-track-publication[data-source="screenShareAudio"]');
  await expect(audio).toHaveCount(1);

  const options = await page.evaluate(() => window.__screenPublishOptions);
  expect(options.video).toMatchObject({
    source: "screen_share",
    name: "display-capture",
    videoCodec: "vp8",
    simulcast: true,
    screenShareEncoding: { maxBitrate: 1_500_000, maxFramerate: 15 },
  });
  expect(options.video.videoEncoding).toBeUndefined();
  expect(options.audio).toEqual({
    source: "screen_share_audio",
    name: "display-system-audio",
    dtx: false,
    red: false,
  });

  await page.getByRole("button", { name: "Unpublish screen video" }).click();
  await expect(video).toHaveCount(0);
  await expect(page.locator('#lifecycle-guide [data-step="unpublish"]')).toHaveAttribute(
    "data-complete",
    "true",
  );
  await expect(audio).toHaveCount(1);
  await expect(page.locator("#screen-capture")).toHaveAttribute("data-video-stopped", "false");
  await expect(page.locator("#screen-capture")).toHaveAttribute("data-preview-attached", "true");

  await page.getByRole("button", { name: "Unpublish system audio" }).click();
  await expect(audio).toHaveCount(0);
  expect(await page.evaluate(() => window.__screenUnpublishStop)).toEqual([
    { kind: "video", stopOnUnpublish: false },
    { kind: "audio", stopOnUnpublish: false },
  ]);
});

test("stopped published screen tracks remain available for explicit unpublish", async ({ page }) => {
  await installScreenFixture(page);
  await page.goto("/");
  await page.getByLabel("Room name").fill(`stopped-screen-publication-${Date.now()}`);
  await page.getByRole("button", { name: "Start screen share" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish screen video" }).click();
  await expect(page.locator('.local-track-publication[data-source="screenShare"]')).toHaveCount(1);
  await page.getByRole("button", { name: "Publish system audio" }).click();
  await expect(page.locator('.local-track-publication[data-source="screenShareAudio"]')).toHaveCount(
    1,
  );

  await page.getByRole("button", { name: "Stop screen share" }).click();

  await expect(page.getByRole("button", { name: "Unpublish screen video" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Unpublish system audio" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Start screen share" })).toBeDisabled();
  await page.getByRole("button", { name: "Unpublish screen video" }).click();
  await expect(page.locator('.local-track-publication[data-source="screenShare"]')).toHaveCount(0);
  await page.getByRole("button", { name: "Unpublish system audio" }).click();
  await expect(page.locator('.local-track-publication[data-source="screenShareAudio"]')).toHaveCount(
    0,
    { timeout: 15_000 },
  );
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0", {
    timeout: 15_000,
  });
});

test("browser stop-sharing automatically removes published video and paired audio", async ({ page }) => {
  await installScreenFixture(page);
  await page.goto("/");
  await page.getByLabel("Room name").fill(`ended-screen-publication-${Date.now()}`);
  await page.getByRole("button", { name: "Start screen share" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish screen video" }).click();
  await page.getByRole("button", { name: "Publish system audio" }).click();
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "2");

  await page.evaluate(() => {
    const fixture = window.__screenFixture;
    fixture.end(fixture.captures[0].video);
  });

  await expect(page.locator("#screen-capture")).toHaveAttribute("data-video-stopped", "true");
  await expect(page.locator("#screen-capture")).toHaveAttribute(
    "data-system-audio-stopped",
    "true",
  );
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");
});

test("browser-ended publication cleanup retries transient unpublish failures", async ({ page }) => {
  await installScreenFixture(page, false);
  await page.goto("/");
  await page.getByLabel("Room name").fill(`retry-ended-screen-unpublish-${Date.now()}`);
  await page.getByRole("button", { name: "Start screen share" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish screen video" }).click();
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "1");
  await page.evaluate(() => {
    window.__endedScreenUnpublishAttempts = 0;
    const participant = window.LivekitClient.LocalParticipant.prototype;
    const unpublish = participant.unpublishTrack;
    participant.unpublishTrack = function (track, ...args) {
      if (track.source === "screen_share") {
        window.__endedScreenUnpublishAttempts += 1;
        if (window.__endedScreenUnpublishAttempts <= 2) {
          return Promise.reject(new Error("transient ended-screen unpublish failure"));
        }
      }
      return unpublish.call(this, track, ...args);
    };
  });

  await page.evaluate(() => {
    const fixture = window.__screenFixture;
    fixture.end(fixture.captures[0].video);
  });

  await expect.poll(() => page.evaluate(() => window.__endedScreenUnpublishAttempts)).toBe(3);
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");
});

test("browser stop-sharing cannot install an in-flight publication on ended media", async ({
  page,
}) => {
  await installScreenFixture(page);
  await page.goto("/");
  await page.getByLabel("Room name").fill(`ended-during-screen-publish-${Date.now()}`);
  await page.getByRole("button", { name: "Start screen share" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.evaluate(() => {
    window.__screenPublishReachedAdapter = false;
    const participant = window.LivekitClient.LocalParticipant.prototype;
    const publish = participant.publishTrack;
    participant.publishTrack = async function (...args) {
      const publication = await publish.apply(this, args);
      window.__screenPublishReachedAdapter = true;
      await new Promise((resolve) => setTimeout(resolve, 500));
      return publication;
    };
  });

  await page.getByRole("button", { name: "Publish screen video" }).click();
  await expect.poll(() => page.evaluate(() => window.__screenPublishReachedAdapter)).toBe(true);
  await page.evaluate(() => {
    const fixture = window.__screenFixture;
    fixture.end(fixture.captures[0].video);
  });

  await expect(page.locator("#screen-capture")).toHaveAttribute("data-video-stopped", "true");
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");
  await expect(page.locator("#screen-capture")).toHaveAttribute("data-video-published", "false");
});

test("browser stop-sharing ends the capture set and terminates preview", async ({ page }) => {
  await instrumentScreenStops(page);
  await installScreenFixture(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Start screen share" }).click();
  await expect(page.locator("#screen-capture")).toHaveAttribute("data-preview-attached", "true");

  await page.evaluate(() => {
    const fixture = window.__screenFixture;
    fixture.end(fixture.captures[0].video);
  });

  await expect(page.locator("#screen-capture")).toHaveAttribute("data-video-stopped", "true");
  await expect(page.locator("#screen-capture")).toHaveAttribute(
    "data-system-audio-stopped",
    "true",
  );
  await expect(page.locator("#screen-capture")).toHaveAttribute("data-preview-attached", "false");
  await expect(page.locator("#screen-capture")).toHaveAttribute(
    "data-audio-preview-attached",
    "false",
  );
  await expect.poll(() => page.locator("#screen-video").evaluate((video) => video.srcObject)).toBeNull();
  await expect.poll(() => page.locator("#screen-audio").evaluate((audio) => audio.srcObject)).toBeNull();
  await expect.poll(() => page.evaluate(() => window.__screenStopCalls.audio)).toBeGreaterThan(0);
});

test("explicit stop releases all selected screen resources", async ({ page }) => {
  await instrumentScreenStops(page);
  await installScreenFixture(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Start screen share" }).click();

  await page.getByRole("button", { name: "Stop screen share" }).click();

  await expect(page.locator("#screen-capture")).toHaveAttribute("data-video-stopped", "true");
  await expect(page.locator("#screen-capture")).toHaveAttribute(
    "data-system-audio-stopped",
    "true",
  );
  await expect(page.locator('#lifecycle-guide [data-step="capture"]')).toHaveAttribute(
    "data-complete",
    "true",
  );
  await expect(page.locator('#lifecycle-guide [data-step="stop"]')).toHaveAttribute(
    "data-complete",
    "true",
  );
  expect(await page.evaluate(() => window.__screenStopCalls)).toEqual({ audio: 1, video: 1 });
});

test("final-owner drop releases all selected screen resources", async ({ page }) => {
  await instrumentScreenStops(page);
  await installScreenFixture(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Start screen share" }).click();
  await expect(page.locator("#screen-capture")).toHaveAttribute("data-video-stopped", "false");
  await expect(page.locator("#screen-capture")).toHaveAttribute(
    "data-system-audio-stopped",
    "false",
  );

  await page.locator("#drop-screen-capture").dispatchEvent("click");
  await expect(page.locator("#screen-capture")).toHaveAttribute("data-present", "false");
  await expect.poll(() => page.evaluate(() => window.__screenStopCalls.audio)).toBe(1);
  await expect.poll(() => page.evaluate(() => window.__screenStopCalls.video)).toBe(1);
});

test("abandoning screen capture releases eventual display and audio tracks", async ({ page }) => {
  await instrumentScreenStops(page);
  await installScreenFixture(page);
  await page.goto("/");
  await page.evaluate(() => {
    const create = window.LivekitClient.createLocalScreenTracks;
    window.LivekitClient.createLocalScreenTracks = async (...args) => {
      const tracks = await create(...args);
      await new Promise((resolve) => setTimeout(resolve, 500));
      return tracks;
    };
  });

  await page.locator("#abandon-screen-capture").dispatchEvent("click");

  await expect.poll(() => page.evaluate(() => window.__screenStopCalls.video)).toBe(1);
  await expect.poll(() => page.evaluate(() => window.__screenStopCalls.audio)).toBe(1);
  await expect(page.locator("#screen-capture")).toHaveAttribute("data-present", "false");
});

test("screen-share controls and previews fit a phone-sized browser", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await installScreenFixture(page, false);
  await page.goto("/");

  await expect(page.getByRole("button", { name: "Start screen share" })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
});
