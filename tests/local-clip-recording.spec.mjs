import { expect, test } from "@playwright/test";

const MIME_PREFERENCES = [
  "video/x-livekit-unsupported",
  "video/webm;codecs=vp8,opus",
  "video/webm;codecs=vp8",
  "video/webm",
  "video/mp4;codecs=avc1.42E01E,mp4a.40.2",
  "video/mp4",
];

async function installWebKitMedia(page, browserName) {
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
    const context2d = canvas.getContext("2d");
    let frame = 0;
    const timer = setInterval(() => {
      context2d.fillStyle = frame++ % 2 === 0 ? "#d9ff43" : "#111827";
      context2d.fillRect(0, 0, canvas.width, canvas.height);
    }, 100);
    const videoStream = canvas.captureStream(30);
    const AudioContext = window.AudioContext || window.webkitAudioContext;
    const audioContext = new AudioContext({ sampleRate: 48_000 });
    const oscillator = audioContext.createOscillator();
    const destination = audioContext.createMediaStreamDestination();
    oscillator.connect(destination);
    oscillator.start();
    const original = mediaDevices.getUserMedia.bind(mediaDevices);
    Object.defineProperty(mediaDevices, "getUserMedia", {
      configurable: true,
      writable: true,
      value: async (constraints) => {
        if (constraints?.video && !constraints?.audio) {
          return new MediaStream([videoStream.getVideoTracks()[0].clone()]);
        }
        if (constraints?.audio && !constraints?.video) {
          await audioContext.resume().catch(() => {});
          return new MediaStream([destination.stream.getAudioTracks()[0].clone()]);
        }
        return original(constraints);
      },
    });
    window.__localClipMedia = {
      audioContext,
      canvas,
      destination,
      oscillator,
      timer,
      videoStream,
    };
  });
}

async function instrumentArtifacts(page) {
  await page.addInitScript(() => {
    const calls = { arrayBuffer: 0, created: [], listenersAdded: 0, listenersRemoved: 0, revoked: [] };
    window.__localClipArtifactCalls = calls;
    const createObjectURL = URL.createObjectURL.bind(URL);
    const revokeObjectURL = URL.revokeObjectURL.bind(URL);
    const arrayBuffer = Blob.prototype.arrayBuffer;
    URL.createObjectURL = function (blob) {
      const url = createObjectURL(blob);
      calls.created.push({ url, size: blob.size, type: blob.type });
      return url;
    };
    URL.revokeObjectURL = function (url) {
      calls.revoked.push(url);
      return revokeObjectURL(url);
    };
    Blob.prototype.arrayBuffer = function (...args) {
      calls.arrayBuffer += 1;
      return arrayBuffer.apply(this, args);
    };
    if (globalThis.MediaRecorder) {
      const addEventListener = MediaRecorder.prototype.addEventListener;
      const removeEventListener = MediaRecorder.prototype.removeEventListener;
      MediaRecorder.prototype.addEventListener = function (...args) {
        calls.listenersAdded += 1;
        return addEventListener.apply(this, args);
      };
      MediaRecorder.prototype.removeEventListener = function (...args) {
        calls.listenersRemoved += 1;
        return removeEventListener.apply(this, args);
      };
    }
  });
}

async function installDeterministicChunkRecorder(page) {
  await page.addInitScript(() => {
    const evidence = { chunkSizes: [], stopAtBytes: 0, stopCalls: 0 };
    window.__deterministicRecorderEvidence = evidence;

    class DeterministicMediaRecorder extends EventTarget {
      static isTypeSupported(mimeType) {
        return mimeType === "video/webm";
      }

      constructor(_stream, options = {}) {
        super();
        this.mimeType = options.mimeType || "video/webm";
        this.state = "inactive";
        this.timer = null;
      }

      start() {
        this.state = "recording";
        this.timer = setInterval(() => {
          if (this.state !== "recording") return;
          const chunk = new Blob([new Uint8Array([1, 2, 3, 4])], { type: this.mimeType });
          evidence.chunkSizes.push(chunk.size);
          const event = new Event("dataavailable");
          Object.defineProperty(event, "data", { value: chunk });
          this.dispatchEvent(event);
        }, 20);
      }

      pause() {
        if (this.state !== "recording") throw new DOMException("not recording", "InvalidStateError");
        this.state = "paused";
        this.dispatchEvent(new Event("pause"));
      }

      resume() {
        if (this.state !== "paused") throw new DOMException("not paused", "InvalidStateError");
        this.state = "recording";
        this.dispatchEvent(new Event("resume"));
      }

      stop() {
        if (this.state === "inactive") return;
        evidence.stopCalls += 1;
        evidence.stopAtBytes = evidence.chunkSizes.reduce((sum, size) => sum + size, 0);
        clearInterval(this.timer);
        this.state = "inactive";
        queueMicrotask(() => {
          const event = new Event("dataavailable");
          Object.defineProperty(event, "data", {
            value: new Blob([], { type: this.mimeType }),
          });
          this.dispatchEvent(event);
          this.dispatchEvent(new Event("stop"));
        });
      }
    }

    Object.defineProperty(globalThis, "MediaRecorder", {
      configurable: true,
      writable: true,
      value: DeterministicMediaRecorder,
    });
  });
}

async function startLocalTracks(page) {
  await page.getByRole("button", { name: "Start camera" }).click();
  await page.getByRole("button", { name: "Start microphone" }).click();
  await expect(page.locator("#local-video-track")).toHaveAttribute("data-stopped", "false");
  await expect(page.locator("#local-audio-track")).toHaveAttribute("data-stopped", "false");
}

async function expectCompletedClip(page, reason, objectUrlActive = true) {
  const state = page.locator("#local-clip-recording");
  await expect(state).toHaveAttribute("data-state", "completed", { timeout: 15_000 });
  await expect(state).toHaveAttribute("data-stop-reason", reason);
  await expect(state).toHaveAttribute("data-blob-present", "true");
  await expect(state).toHaveAttribute("data-object-url-active", String(objectUrlActive));
  const artifact = await state.evaluate((element) => ({
    blobSize: Number(element.dataset.blobSize),
    blobType: element.dataset.blobType,
    durationMs: Number(element.dataset.durationMs),
    filename: element.dataset.filename,
    mimeType: element.dataset.mimeType,
  }));
  expect(artifact.blobSize).toBeGreaterThan(0);
  expect(artifact.durationMs).toBeGreaterThan(0);
  expect(artifact.mimeType).not.toBe("");
  expect(artifact.blobType).toBe(artifact.mimeType);
  expect(artifact.filename).toMatch(/^local-clip\.(webm|mp4|m4a|ogv|ogg|bin)$/);
  if (objectUrlActive) {
    await expect(page.locator("#local-clip-download")).toHaveAttribute(
      "download",
      artifact.filename,
    );

    const fetched = await page.locator("#local-clip-download").evaluate(async (link) => {
      const blob = await fetch(link.href).then((response) => response.blob());
      return { size: blob.size, type: blob.type };
    });
    expect(fetched.size).toBe(artifact.blobSize);
    expect(fetched.type).not.toBe("");
  } else {
    await expect(page.locator("#local-clip-download")).toHaveCount(0);
  }
}

test("stops only after multiple chunks cross the accumulated-size limit", async ({
  page,
  browserName,
}) => {
  await installDeterministicChunkRecorder(page);
  await instrumentArtifacts(page);
  await page.goto("/");
  await installWebKitMedia(page, browserName);
  await startLocalTracks(page);

  const state = page.locator("#local-clip-recording");
  const limit = Number(await state.getAttribute("data-size-limit-bytes"));
  expect(limit).toBe(10);
  await page.locator("#start-size-limited-local-clip").dispatchEvent("click");
  await expectCompletedClip(page, "sizeLimit");

  const evidence = await page.evaluate(() => ({
    ...window.__deterministicRecorderEvidence,
    artifactSize: window.__localClipArtifactCalls.created.at(-1).size,
    arrayBufferCalls: window.__localClipArtifactCalls.arrayBuffer,
  }));
  expect(evidence.chunkSizes).toEqual([4, 4, 4]);
  expect(evidence.chunkSizes.slice(0, -1).reduce((sum, size) => sum + size, 0)).toBeLessThan(
    limit,
  );
  expect(evidence.stopAtBytes).toBeGreaterThanOrEqual(limit);
  expect(evidence.artifactSize).toBe(evidence.stopAtBytes);
  expect(evidence.stopCalls).toBe(1);
  expect(evidence.arrayBufferCalls).toBe(0);
});

test("reports an unavailable object URL without completing artifact handling", async ({
  page,
  browserName,
}) => {
  await installDeterministicChunkRecorder(page);
  await page.addInitScript(() => {
    URL.createObjectURL = () => {
      throw new DOMException("synthetic object URL failure", "OperationError");
    };
  });
  await page.goto("/");
  await installWebKitMedia(page, browserName);
  await startLocalTracks(page);

  await page.locator("#start-size-limited-local-clip").dispatchEvent("click");
  await expectCompletedClip(page, "sizeLimit", false);
  await expect(page.locator("#local-clip-recording")).toHaveAttribute(
    "data-error-kind",
    "objectUrlFailed",
  );
  await expect(page.locator("#local-clip-artifact")).toHaveAttribute(
    "data-url-state",
    "unavailable",
  );
  await expect(page.locator('#lifecycle-guide [data-step="artifact"]')).toHaveAttribute(
    "data-complete",
    "false",
  );
});

test("records bounded local clips from unpublished and published Local Media Tracks", async ({
  page,
  browserName,
}) => {
  test.setTimeout(120_000);
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await instrumentArtifacts(page);
  await page.goto("/");
  await installWebKitMedia(page, browserName);
  await startLocalTracks(page);
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");

  const state = page.locator("#local-clip-recording");
  const supported = (await state.getAttribute("data-supported")) === "true";
  if (!supported) {
    await page.locator("#start-duration-limited-local-clip").dispatchEvent("click");
    await expect(state).toHaveAttribute("data-error-kind", "unsupportedBrowser");
    await expect(state).toHaveAttribute("data-state", "idle");
    expect(pageErrors).toEqual([]);
    return;
  }

  const expectedMime = await page.evaluate((preferences) =>
    preferences.find((preference) => MediaRecorder.isTypeSupported(preference)),
  MIME_PREFERENCES);
  await page.evaluate(() => {
    const stop = MediaRecorder.prototype.stop;
    MediaRecorder.prototype.stop = function () {
      setTimeout(() => stop.call(this), 200);
    };
  });
  await page.locator("#start-local-clip").click();
  await expect(state).toHaveAttribute("data-state", "recording");
  await expect(state).toHaveAttribute("data-selected-mime", expectedMime);
  await page.waitForTimeout(500);
  await page.locator("#pause-local-clip").click();
  await expect(state).toHaveAttribute("data-state", "paused");
  await page.waitForTimeout(400);
  await page.locator("#resume-local-clip").click();
  await expect(state).toHaveAttribute("data-state", "recording");
  await page.waitForTimeout(700);
  await page.locator("#stop-local-clip").click();
  await expect(state).toHaveAttribute("data-state", "stopping");
  await expectCompletedClip(page, "explicit");
  await expect(state).toHaveAttribute("data-stop-returned", "true");
  const artifact = page.locator("#local-clip-artifact");
  await expect(artifact).toBeVisible();
  await expect(artifact).toHaveAttribute("data-url-state", "active");
  await expect(artifact).toContainText(expectedMime);
  await expect(artifact).toContainText("explicit");
  await expect(page.locator('#lifecycle-guide [data-step="record"]')).toHaveAttribute(
    "data-complete",
    "true",
  );

  const explicitUrl = await page.locator("#local-clip-download").getAttribute("href");
  await page.locator("#revoke-local-clip-url").click();
  await expect(state).toHaveAttribute("data-object-url-active", "false");
  await expect(artifact).toHaveAttribute("data-url-state", "revoked");
  await expect(page.locator('#lifecycle-guide [data-step="artifact"]')).toHaveAttribute(
    "data-complete",
    "true",
  );
  await expect(page.locator("#local-clip-download")).toHaveCount(0);
  await expect
    .poll(() => page.evaluate(() => window.__localClipArtifactCalls.revoked))
    .toContain(explicitUrl);

  await page.getByLabel("Room name").fill(`local-clip-${Date.now()}`);
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish camera" }).click();
  await page.getByRole("button", { name: "Publish microphone", exact: true }).click();
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "2");

  await page.locator("#start-duration-limited-local-clip").dispatchEvent("click");
  await expectCompletedClip(page, "durationLimit");
  const durationUrl = await page.locator("#local-clip-download").getAttribute("href");
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "2");
  await expect(page.locator("#local-video-track")).toHaveAttribute("data-stopped", "false");
  await expect(page.locator("#local-audio-track")).toHaveAttribute("data-stopped", "false");

  await page.locator("#start-size-limited-local-clip").dispatchEvent("click");
  await expect
    .poll(() => page.evaluate(() => window.__localClipArtifactCalls.revoked))
    .toContain(durationUrl);
  await expectCompletedClip(page, "sizeLimit");
  expect(await page.evaluate(() => window.__localClipArtifactCalls.arrayBuffer)).toBe(0);
  expect(await page.evaluate(() => window.__localClipArtifactCalls.created.length)).toBe(3);
  expect(
    await page.evaluate(() => ({
      added: window.__localClipArtifactCalls.listenersAdded,
      removed: window.__localClipArtifactCalls.listenersRemoved,
    })),
  ).toEqual({ added: 9, removed: 9 });
  expect(pageErrors).toEqual([]);
});

test("reports source-ended when every selected Local Media Track ends", async ({
  page,
  browserName,
}) => {
  test.setTimeout(60_000);
  await instrumentArtifacts(page);
  await page.goto("/");
  await installWebKitMedia(page, browserName);
  await startLocalTracks(page);
  const state = page.locator("#local-clip-recording");
  if ((await state.getAttribute("data-supported")) !== "true") return;

  await page.locator("#start-local-clip").click();
  await expect(state).toHaveAttribute("data-state", "recording");
  await page.waitForTimeout(500);
  await page.getByRole("button", { name: "Stop camera" }).click();
  await expect(state).toHaveAttribute("data-state", "recording");
  await page.getByRole("button", { name: "Stop microphone" }).click();

  await expectCompletedClip(page, "sourceEnded");
  await expect(state).toHaveAttribute("data-stop-returned", "false");
});

test("rejects unsupported MIME preferences before constructing MediaRecorder", async ({
  page,
  browserName,
}) => {
  await instrumentArtifacts(page);
  await page.goto("/");
  await installWebKitMedia(page, browserName);
  await startLocalTracks(page);
  const state = page.locator("#local-clip-recording");
  if ((await state.getAttribute("data-supported")) !== "true") return;
  await page.evaluate(() => {
    const MediaRecorder = globalThis.MediaRecorder;
    window.__localClipMediaRecorderConstructions = 0;
    globalThis.MediaRecorder = new Proxy(MediaRecorder, {
      construct(target, args, newTarget) {
        window.__localClipMediaRecorderConstructions += 1;
        return Reflect.construct(target, args, newTarget);
      },
      get(target, property, receiver) {
        if (property === "isTypeSupported") return () => false;
        return Reflect.get(target, property, receiver);
      },
    });
  });

  await page.locator("#start-unsupported-mime-local-clip").dispatchEvent("click");

  await expect(state).toHaveAttribute("data-error-kind", "unsupportedMimeType");
  await expect(state).toHaveAttribute("data-state", "idle");
  await expect(state).toHaveAttribute("data-blob-present", "false");
  expect(await page.evaluate(() => window.__localClipMediaRecorderConstructions)).toBe(0);
  expect(await page.evaluate(() => window.__localClipArtifactCalls.created)).toEqual([]);
});

test("cleans up recorder listeners and chunks after an encoder error", async ({
  page,
  browserName,
}) => {
  await instrumentArtifacts(page);
  await page.goto("/");
  await installWebKitMedia(page, browserName);
  await startLocalTracks(page);
  const state = page.locator("#local-clip-recording");
  if ((await state.getAttribute("data-supported")) !== "true") return;
  await page.evaluate(() => {
    const MediaRecorder = globalThis.MediaRecorder;
    const stop = MediaRecorder.prototype.stop;
    window.__localClipRecorderStopCalls = 0;
    MediaRecorder.prototype.stop = function (...args) {
      window.__localClipRecorderStopCalls += 1;
      return stop.apply(this, args);
    };
    globalThis.MediaRecorder = new Proxy(MediaRecorder, {
      construct(target, args, newTarget) {
        const recorder = Reflect.construct(target, args, newTarget);
        window.__localClipLastRecorder = recorder;
        return recorder;
      },
    });
  });
  await page.locator("#start-local-clip").click();
  await expect(state).toHaveAttribute("data-state", "recording");

  await page.evaluate(() => {
    const event = new Event("error");
    Object.defineProperty(event, "error", {
      value: new DOMException("synthetic encoder failure", "EncodingError"),
    });
    window.__localClipLastRecorder.dispatchEvent(event);
  });

  await expect(state).toHaveAttribute("data-error-kind", "recordingFailed");
  await expect(state).toHaveAttribute("data-blob-present", "false");
  expect(await page.evaluate(() => window.__localClipRecorderStopCalls)).toBe(1);
  expect(
    await page.evaluate(() => ({
      added: window.__localClipArtifactCalls.listenersAdded,
      removed: window.__localClipArtifactCalls.listenersRemoved,
    })),
  ).toEqual({ added: 3, removed: 3 });
});

test("reports missing MediaRecorder before recording or artifact mutation", async ({
  page,
  browserName,
}) => {
  await page.addInitScript(() => {
    Object.defineProperty(globalThis, "MediaRecorder", {
      configurable: true,
      value: undefined,
    });
  });
  await instrumentArtifacts(page);
  await page.goto("/");
  await installWebKitMedia(page, browserName);
  await startLocalTracks(page);
  const state = page.locator("#local-clip-recording");
  await expect(state).toHaveAttribute("data-supported", "false");

  await page.locator("#start-duration-limited-local-clip").dispatchEvent("click");

  await expect(state).toHaveAttribute("data-error-kind", "unsupportedBrowser");
  await expect(state).toHaveAttribute("data-state", "idle");
  await expect(state).toHaveAttribute("data-blob-present", "false");
  await expect(state).toHaveAttribute("data-object-url-active", "false");
  await expect(page.locator("#local-video-track")).toHaveAttribute("data-stopped", "false");
  await expect(page.locator("#local-audio-track")).toHaveAttribute("data-stopped", "false");
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");
  expect(await page.evaluate(() => window.__localClipArtifactCalls.created)).toEqual([]);
});

test("final owner drops release an active recorder and object URL", async ({
  page,
  browserName,
}) => {
  test.setTimeout(60_000);
  await instrumentArtifacts(page);
  await page.goto("/");
  await installWebKitMedia(page, browserName);
  await startLocalTracks(page);
  const state = page.locator("#local-clip-recording");
  if ((await state.getAttribute("data-supported")) !== "true") return;
  await page.evaluate(() => {
    window.__localClipRecorderStopCalls = 0;
    const stop = MediaRecorder.prototype.stop;
    MediaRecorder.prototype.stop = function (...args) {
      window.__localClipRecorderStopCalls += 1;
      return stop.apply(this, args);
    };
  });

  await page.locator("#start-local-clip").click();
  await expect(state).toHaveAttribute("data-state", "recording");
  await page.locator("#drop-local-clip-recording").dispatchEvent("click");

  await expect(state).toHaveAttribute("data-state", "idle");
  await expect.poll(() => page.evaluate(() => window.__localClipRecorderStopCalls)).toBe(1);
  await expect
    .poll(() =>
      page.evaluate(() => ({
        added: window.__localClipArtifactCalls.listenersAdded,
        removed: window.__localClipArtifactCalls.listenersRemoved,
      })),
    )
    .toEqual({ added: 3, removed: 3 });
  expect(await page.evaluate(() => window.__localClipArtifactCalls.created)).toEqual([]);

  await page.locator("#start-duration-limited-local-clip").dispatchEvent("click");
  await expectCompletedClip(page, "durationLimit");
  const url = await page.locator("#local-clip-download").getAttribute("href");
  await page.locator("#drop-local-clip-object-url").dispatchEvent("click");

  await expect(state).toHaveAttribute("data-object-url-active", "false");
  await expect(page.locator("#local-clip-download")).toHaveCount(0);
  await expect
    .poll(() => page.evaluate(() => window.__localClipArtifactCalls.revoked))
    .toContain(url);
});
