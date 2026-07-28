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
        return stream.clone();
      },
    });
    window.__localAudioWebKitCamera = { canvas, stream };
  });
}

async function installWebKitMicrophone(page, browserName) {
  if (browserName !== "webkit") return;
  await page.evaluate(() => {
    const mediaDevices = navigator.mediaDevices;
    Object.defineProperty(navigator, "mediaDevices", {
      configurable: true,
      value: mediaDevices,
    });
    const AudioContext = window.AudioContext || window.webkitAudioContext;
    const context = new AudioContext({ sampleRate: 48_000 });
    const oscillator = context.createOscillator();
    const destination = context.createMediaStreamDestination();
    oscillator.connect(destination);
    oscillator.start();
    const original = mediaDevices.getUserMedia.bind(mediaDevices);
    Object.defineProperty(mediaDevices, "getUserMedia", {
      configurable: true,
      writable: true,
      value: async (constraints) => {
        if (!constraints?.audio || constraints?.video) return original(constraints);
        if (window.__localAudioCalls) window.__localAudioCalls.userMedia += 1;
        return new MediaStream([destination.stream.getAudioTracks()[0].clone()]);
      },
    });
    window.__localAudioWebKitMicrophone = { context, oscillator, destination };
  });
}

async function instrumentAudioTrackStops(page) {
  await page.addInitScript(() => {
    window.__localAudioStopCalls = 0;
    const original = MediaStreamTrack.prototype.stop;
    MediaStreamTrack.prototype.stop = function (...args) {
      if (this.kind === "audio") window.__localAudioStopCalls += 1;
      return original.apply(this, args);
    };
  });
}

test("captures microphone before joining without publication", async ({ page, browserName }) => {
  await page.addInitScript(() => {
    const original = navigator.mediaDevices.getUserMedia;
    window.__localAudioCalls = { userMedia: 0 };
    navigator.mediaDevices.getUserMedia = function (...args) {
      window.__localAudioCalls.userMedia += 1;
      return original.apply(this, args);
    };
  });
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);

  await page.getByRole("button", { name: "Start microphone" }).click();

  await expect(page.locator("#microphone-state")).toHaveText("on");
  await expect(page.locator("#local-audio-track")).toHaveAttribute("data-stopped", "false");
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");
  expect(await page.evaluate(() => window.__localAudioCalls.userMedia)).toBe(1);
});

test("publishes typed microphone options while mute and unpublish keep independent lifecycles", async ({
  page,
  browserName,
}) => {
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await page.getByLabel("Room name").fill(`microphone-publication-${Date.now()}`);
  await page.evaluate(() => {
    const calls = { publishOptions: null, unpublishStopOnUnpublish: null };
    window.__localAudioPublicationCalls = calls;
    const prototype = window.LivekitClient.LocalParticipant.prototype;
    const originalPublish = prototype.publishTrack;
    const originalUnpublish = prototype.unpublishTrack;
    prototype.publishTrack = function (track, options) {
      if (track.kind === "audio") calls.publishOptions = structuredClone(options);
      return originalPublish.call(this, track, options);
    };
    prototype.unpublishTrack = function (track, stopOnUnpublish) {
      if (track.kind === "audio") calls.unpublishStopOnUnpublish = stopOnUnpublish;
      return originalUnpublish.call(this, track, stopOnUnpublish);
    };
  });

  await page.getByRole("button", { name: "Start microphone" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });

  await page.getByRole("button", { name: "Publish microphone", exact: true }).click();

  const publication = page.locator('.local-track-publication[data-kind="audio"]');
  await expect(publication).toHaveCount(1);
  await expect(publication).toHaveAttribute("data-source", "microphone");
  await expect(publication).toHaveAttribute("data-name", "microphone-capture");
  await expect(publication).toHaveAttribute("data-muted", "false");
  expect(await page.evaluate(() => window.__localAudioPublicationCalls.publishOptions)).toEqual({
    source: "microphone",
    name: "microphone-capture",
    dtx: true,
    red: false,
  });

  await page.getByRole("button", { name: "Mute microphone publication" }).click();
  await expect(publication).toHaveAttribute("data-muted", "true");
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "1");
  await expect(page.locator("#microphone-state")).toHaveText("on");

  await page.getByRole("button", { name: "Unpublish microphone" }).click();
  await expect(publication).toHaveCount(0);
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");
  await expect(page.locator("#microphone-state")).toHaveText("on");
  expect(
    await page.evaluate(() => window.__localAudioPublicationCalls.unpublishStopOnUnpublish),
  ).toBe(false);
  await page.getByRole("button", { name: "Leave room" }).click();
});

test("captures microphone after joining without implicit publication", async ({
  page,
  browserName,
}) => {
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await page.getByLabel("Room name").fill(`post-join-microphone-${Date.now()}`);
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");

  await page.getByRole("button", { name: "Start microphone" }).click();

  await expect(page.locator("#microphone-state")).toHaveText("on");
  await expect(page.locator("#local-audio-track")).toHaveAttribute("data-stopped", "false");
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "0");
  await page.getByRole("button", { name: "Leave room" }).click();
});

test("leaving preserves published microphone capture for another Room Session", async ({
  page,
  browserName,
}) => {
  await instrumentAudioTrackStops(page);
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await page.getByLabel("Room name").fill(`reusable-microphone-${Date.now()}`);
  await page.getByRole("button", { name: "Start microphone" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish microphone" }).click();
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(1);

  await page.getByRole("button", { name: "Leave room" }).click();

  await expect(page.locator("#connection-state")).toHaveText("disconnected");
  await expect(page.locator("#local-audio-track")).toHaveAttribute("data-stopped", "false");
  expect(await page.evaluate(() => window.__localAudioStopCalls)).toBe(0);

  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish microphone" }).click();
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(1);
});

test("leaving rejects a late microphone publication without reinstalling its association", async ({
  page,
  browserName,
}) => {
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await page.getByLabel("Room name").fill(`leave-during-audio-publish-${Date.now()}`);
  await page.getByRole("button", { name: "Start microphone" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.evaluate(() => {
    window.__leaveDuringAudioPublish = [];
    window.__delayNextAudioPublish = true;
    const participant = window.LivekitClient.LocalParticipant.prototype;
    const original = participant.publishTrack;
    participant.publishTrack = async function (track, ...args) {
      if (track.kind !== "audio" || !window.__delayNextAudioPublish) {
        return original.call(this, track, ...args);
      }
      window.__delayNextAudioPublish = false;
      window.__leaveDuringAudioPublish.push("started");
      const publication = await original.call(this, track, ...args);
      window.__leaveDuringAudioPublish.push("accepted");
      await new Promise((resolve) => (window.__releaseAudioPublish = resolve));
      window.__leaveDuringAudioPublish.push("returned");
      return publication;
    };
  });

  await page.getByRole("button", { name: "Publish microphone" }).click();
  await expect
    .poll(() => page.evaluate(() => window.__leaveDuringAudioPublish))
    .toEqual(["started", "accepted"]);
  await page.getByRole("button", { name: "Leave room" }).click();

  await expect(page.locator("#connection-state")).toHaveText("disconnected");
  expect(await page.evaluate(() => window.__leaveDuringAudioPublish)).toEqual([
    "started",
    "accepted",
  ]);
  await page.evaluate(() => window.__releaseAudioPublish());
  await expect
    .poll(() => page.evaluate(() => window.__leaveDuringAudioPublish))
    .toEqual(["started", "accepted", "returned"]);

  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await expect(page.locator(".local-track-publication")).toHaveCount(0);
  await page.getByRole("button", { name: "Publish microphone", exact: true }).click();
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(1);
});

test("dropping the final Local Audio Track owner stops microphone capture", async ({
  page,
  browserName,
}) => {
  await instrumentAudioTrackStops(page);
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await page.getByRole("button", { name: "Start microphone" }).click();

  await page.locator("#drop-local-audio-track").dispatchEvent("click");

  await expect(page.locator("#local-audio-track")).toHaveAttribute("data-present", "false");
  await expect.poll(() => page.evaluate(() => window.__localAudioStopCalls)).toBeGreaterThan(0);
});

test("explicit stop is terminal and releases microphone capture", async ({ page, browserName }) => {
  await instrumentAudioTrackStops(page);
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await page.getByRole("button", { name: "Start microphone" }).click();

  await page.getByRole("button", { name: "Stop microphone" }).click();

  await expect(page.locator("#microphone-state")).toHaveText("off");
  await expect(page.locator("#local-audio-track")).toHaveAttribute("data-stopped", "true");
  await expect.poll(() => page.evaluate(() => window.__localAudioStopCalls)).toBeGreaterThan(0);
});

test("camera and microphone lifecycles remain independent", async ({ page, browserName }) => {
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await installWebKitCamera(page, browserName);
  await page.getByLabel("Room name").fill(`independent-local-media-${Date.now()}`);
  await page.getByRole("button", { name: "Start camera" }).click();
  await expect(page.locator("#local-video-track")).toHaveAttribute("data-stopped", "false");
  await page.getByRole("button", { name: "Start microphone" }).click();
  await expect(page.locator("#local-audio-track")).toHaveAttribute("data-stopped", "false");
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });

  await page.getByRole("button", { name: "Publish camera" }).click();
  const camera = page.locator('.local-track-publication[data-kind="video"]');
  const microphone = page.locator('.local-track-publication[data-kind="audio"]');
  await expect(camera).toHaveAttribute("data-muted", "false");
  await page.getByRole("button", { name: "Publish microphone" }).click();
  await expect(microphone).toHaveAttribute("data-muted", "false");

  await page.getByRole("button", { name: "Mute microphone publication" }).click();
  await expect(microphone).toHaveAttribute("data-muted", "true");
  await expect(camera).toHaveAttribute("data-muted", "false");

  await page.getByRole("button", { name: "Unpublish microphone" }).click();
  await expect(microphone).toHaveCount(0);
  await expect(camera).toHaveCount(1);
  await expect(page.locator("#microphone-state")).toHaveText("on");
  await expect(page.locator("#camera-state")).toHaveText("on");

  await page.getByRole("button", { name: "Stop microphone" }).click();
  await expect(page.locator("#microphone-state")).toHaveText("off");
  await expect(page.locator("#camera-state")).toHaveText("on");
  await expect(camera).toHaveCount(1);
  await page.getByRole("button", { name: "Leave room" }).click();
});

test("overlapping microphone mute and unpublish mutations run in order", async ({
  page,
  browserName,
}) => {
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await page.getByLabel("Room name").fill(`serialized-audio-controls-${Date.now()}`);
  await page.evaluate(() => {
    const events = [];
    window.__localAudioMutationEvents = events;
    const track = window.LivekitClient.LocalAudioTrack.prototype;
    const participant = window.LivekitClient.LocalParticipant.prototype;
    const originalMute = track.mute;
    const originalUnpublish = participant.unpublishTrack;
    track.mute = async function (...args) {
      events.push("mute-start");
      await new Promise((resolve) => setTimeout(resolve, 300));
      const result = await originalMute.apply(this, args);
      events.push("mute-end");
      return result;
    };
    participant.unpublishTrack = async function (...args) {
      if (args[0].kind === "audio") events.push("unpublish");
      return originalUnpublish.apply(this, args);
    };
  });
  await page.getByRole("button", { name: "Start microphone" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish microphone" }).click();
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(1);

  await page.getByRole("button", { name: "Mute microphone publication" }).click();
  await page.getByRole("button", { name: "Unpublish microphone" }).click();

  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(0);
  expect(await page.evaluate(() => window.__localAudioMutationEvents)).toEqual([
    "mute-start",
    "mute-end",
    "unpublish",
  ]);
  await expect(page.locator(".notice.error")).toHaveCount(0);
  await page.getByRole("button", { name: "Leave room" }).click();
});

test("a publish queued behind microphone unpublish runs after association release", async ({
  page,
  browserName,
}) => {
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await page.getByLabel("Room name").fill(`serialized-audio-republish-${Date.now()}`);
  await page.getByRole("button", { name: "Start microphone" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish microphone" }).click();
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(1);
  await page.evaluate(() => {
    window.__localAudioRepublishEvents = [];
    const participant = window.LivekitClient.LocalParticipant.prototype;
    const originalPublish = participant.publishTrack;
    const originalUnpublish = participant.unpublishTrack;
    participant.unpublishTrack = async function (track, ...args) {
      if (track.kind !== "audio") return originalUnpublish.call(this, track, ...args);
      window.__localAudioRepublishEvents.push("unpublish-start");
      await new Promise((resolve) => setTimeout(resolve, 300));
      const result = await originalUnpublish.call(this, track, ...args);
      window.__localAudioRepublishEvents.push("unpublish-end");
      return result;
    };
    participant.publishTrack = async function (track, ...args) {
      if (track.kind === "audio") window.__localAudioRepublishEvents.push("publish");
      return originalPublish.call(this, track, ...args);
    };
  });

  await page.locator("#republish-local-audio-track").dispatchEvent("click");

  await expect
    .poll(() => page.evaluate(() => window.__localAudioRepublishEvents))
    .toEqual(["unpublish-start", "unpublish-end", "publish"]);
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(1);
  await expect(page.locator(".notice.error")).toHaveCount(0);
});

test("a failed queued republish leaves the microphone available for recovery", async ({
  page,
  browserName,
}) => {
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await page.getByLabel("Room name").fill(`failed-audio-republish-${Date.now()}`);
  await page.getByRole("button", { name: "Start microphone" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish microphone", exact: true }).click();
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(1);
  await page.evaluate(() => {
    window.__failAudioUnpublish = true;
    const participant = window.LivekitClient.LocalParticipant.prototype;
    const original = participant.unpublishTrack;
    participant.unpublishTrack = function (track, ...args) {
      if (track.kind === "audio" && window.__failAudioUnpublish) {
        return Promise.reject(new Error("synthetic queued unpublish failure"));
      }
      return original.call(this, track, ...args);
    };
  });

  await page.locator("#republish-local-audio-track").dispatchEvent("click");
  await expect(page.locator(".notice.error")).toContainText("synthetic queued unpublish failure");

  await page.evaluate(() => (window.__failAudioUnpublish = false));
  await page.getByRole("button", { name: "Unpublish microphone", exact: true }).click();
  await page.getByRole("button", { name: "Publish microphone", exact: true }).click();
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(1);
});

test("an old audio publication handle cannot mute its replacement", async ({
  page,
  browserName,
}) => {
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await page.getByLabel("Room name").fill(`stale-audio-publication-${Date.now()}`);
  await page.getByRole("button", { name: "Start microphone" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish microphone", exact: true }).click();
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveAttribute(
    "data-muted",
    "false",
  );
  const initialSid = await page
    .locator('.local-track-publication[data-kind="audio"]')
    .getAttribute("data-publication-sid");

  await page.locator("#replace-and-mute-old-audio-publication").dispatchEvent("click");

  await expect
    .poll(() =>
      page
        .locator('.local-track-publication[data-kind="audio"]')
        .getAttribute("data-publication-sid"),
    )
    .not.toBe(initialSid);
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveAttribute(
    "data-muted",
    "false",
  );
  await expect(page.locator("#local-audio-track")).toHaveAttribute(
    "data-publication-muted",
    "false",
  );
  await expect(page.locator(".notice.error")).toHaveCount(0);
});

test("observed SDK unpublish remains authoritative after a later rejection", async ({
  page,
  browserName,
}) => {
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await page.getByLabel("Room name").fill(`post-event-unpublish-${browserName}-${Date.now()}`);
  await page.getByRole("button", { name: "Start microphone" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish microphone", exact: true }).click();
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(1);
  await page.evaluate(() => {
    const participant = window.LivekitClient.LocalParticipant.prototype;
    const original = participant.unpublishTrack;
    participant.unpublishTrack = async function (...args) {
      await original.apply(this, args);
      throw new Error("synthetic post-event negotiation failure");
    };
  });

  await page.getByRole("button", { name: "Unpublish microphone", exact: true }).click();

  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(0);
  await expect(page.locator("#local-audio-track")).toHaveAttribute("data-publication-sid", "");
  await expect(page.locator(".notice.error")).toHaveCount(0);
});

test("full reconnect terminals stale local publications and exposes replacements", async ({
  page,
  browserName,
}) => {
  test.setTimeout(120_000);
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await installWebKitCamera(page, browserName);
  await page.evaluate(() => {
    const OriginalRoom = window.LivekitClient.Room;
    const InstrumentedRoom = new Proxy(OriginalRoom, {
      construct(target, args) {
        const room = Reflect.construct(target, args);
        window.__localPublicationRoom = room;
        return room;
      },
    });
    window.LivekitClient = new Proxy(window.LivekitClient, {
      get(target, property, receiver) {
        return property === "Room" ? InstrumentedRoom : Reflect.get(target, property, receiver);
      },
    });
  });
  await page.getByLabel("Room name").fill(`local-reconnect-${browserName}-${Date.now()}`);
  await page.getByRole("button", { name: "Start camera" }).click();
  await page.getByRole("button", { name: "Start microphone" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  const videoTrack = page.locator("#local-video-track");
  const audioTrack = page.locator("#local-audio-track");
  await page.getByRole("button", { name: "Publish camera" }).click();
  await expect(videoTrack).toHaveAttribute("data-publication-sid", /^TR_/, { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish microphone", exact: true }).click();
  await expect(audioTrack).toHaveAttribute("data-publication-sid", /^TR_/, { timeout: 30_000 });
  await expect(videoTrack).toHaveAttribute("data-publication-terminal", "false");
  await expect(audioTrack).toHaveAttribute("data-publication-terminal", "false");
  await expect(page.locator(".local-track-publication")).toHaveCount(2);
  await expect(page.locator(".notice.error")).toHaveCount(0);
  await page.evaluate(() => {
    const publications = [...window.__localPublicationRoom.localParticipant.trackPublications.values()];
    window.__preReconnectLocalPublications = Object.fromEntries(
      publications.map((publication) => [publication.kind, {
        publication,
        track: publication.track,
        sid: publication.trackSid,
      }]),
    );
  });

  await page.evaluate(() => window.__localPublicationRoom.simulateScenario("full-reconnect"));

  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await expect(videoTrack).toHaveAttribute("data-publication-terminal", "true");
  await expect(audioTrack).toHaveAttribute("data-publication-terminal", "true");
  await expect
    .poll(() =>
      page.evaluate(() => {
        const publications = [...window.__localPublicationRoom.localParticipant.trackPublications.values()];
        return publications.length === 2 && publications.every((publication) => {
          const previous = window.__preReconnectLocalPublications[publication.kind];
          return publication !== previous.publication &&
            publication.track === previous.track &&
            publication.trackSid !== previous.sid;
        });
      }),
      { timeout: 30_000 },
    )
    .toBe(true);

  const videoSnapshot = page.locator('.local-track-publication[data-kind="video"]');
  const audioSnapshot = page.locator('.local-track-publication[data-kind="audio"]');
  const videoSid = await videoSnapshot.getAttribute("data-publication-sid");
  const audioSid = await audioSnapshot.getAttribute("data-publication-sid");
  await page.locator("#recover-local-publication-handles").dispatchEvent("click");
  await expect(videoTrack).toHaveAttribute("data-publication-terminal", "false");
  await expect(videoTrack).toHaveAttribute("data-publication-sid", videoSid);
  await expect(audioTrack).toHaveAttribute("data-publication-terminal", "false");
  await expect(audioTrack).toHaveAttribute("data-publication-sid", audioSid);

  await page.getByRole("button", { name: "Mute microphone publication" }).click();
  await expect(audioSnapshot).toHaveAttribute("data-muted", "true");
});

test("failed reconnect republish terminals the old handle and allows explicit recovery", async ({
  page,
  browserName,
}) => {
  test.setTimeout(120_000);
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await page.evaluate(() => {
    const OriginalRoom = window.LivekitClient.Room;
    const InstrumentedRoom = new Proxy(OriginalRoom, {
      construct(target, args) {
        const room = Reflect.construct(target, args);
        window.__failedRepublishRoom = room;
        return room;
      },
    });
    window.LivekitClient = new Proxy(window.LivekitClient, {
      get(target, property, receiver) {
        return property === "Room" ? InstrumentedRoom : Reflect.get(target, property, receiver);
      },
    });
  });
  await page.getByLabel("Room name").fill(`failed-republish-${browserName}-${Date.now()}`);
  await page.getByRole("button", { name: "Start microphone" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish microphone", exact: true }).click();
  const audioTrack = page.locator("#local-audio-track");
  await expect(audioTrack).toHaveAttribute("data-publication-sid", /^TR_/, { timeout: 30_000 });
  const originalSid = await audioTrack.getAttribute("data-publication-sid");
  await page.evaluate(() => {
    const publication = [...window.__failedRepublishRoom.localParticipant.trackPublications.values()]
      .find((candidate) => candidate.kind === "audio");
    const track = publication.track;
    const restart = track.restartTrack.bind(track);
    window.__failedReconnectRestart = false;
    track.restartTrack = async (...args) => {
      if (!window.__failedReconnectRestart) {
        window.__failedReconnectRestart = true;
        throw new Error("synthetic reconnect restart failure");
      }
      return restart(...args);
    };
  });

  await page.evaluate(() => window.__failedRepublishRoom.simulateScenario("full-reconnect"));

  await expect.poll(() => page.evaluate(() => window.__failedReconnectRestart)).toBe(true);
  await expect(audioTrack).toHaveAttribute("data-publication-terminal", "true");
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(0);
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });

  await page.getByRole("button", { name: "Publish microphone", exact: true }).click();
  await expect(audioTrack).toHaveAttribute("data-publication-terminal", "false");
  await expect
    .poll(() => audioTrack.getAttribute("data-publication-sid"), { timeout: 30_000 })
    .not.toBe(originalSid);
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(1);
});

test("dropping a mute future keeps its eventual state observable", async ({ page, browserName }) => {
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await page.getByLabel("Room name").fill(`abandoned-audio-mute-${Date.now()}`);
  await page.getByRole("button", { name: "Start microphone" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish microphone", exact: true }).click();
  const publication = page.locator('.local-track-publication[data-kind="audio"]');
  await expect(publication).toHaveAttribute("data-muted", "false");
  await page.evaluate(() => {
    window.__abandonedAudioMute = [];
    const track = window.LivekitClient.LocalAudioTrack.prototype;
    const original = track.mute;
    track.mute = async function (...args) {
      window.__abandonedAudioMute.push("started");
      await new Promise((resolve) => setTimeout(resolve, 300));
      const result = await original.apply(this, args);
      window.__abandonedAudioMute.push("completed");
      return result;
    };
  });

  await page.locator("#abandon-local-audio-mute").dispatchEvent("click");

  await expect
    .poll(() => page.evaluate(() => window.__abandonedAudioMute))
    .toEqual(["started", "completed"]);
  await expect(publication).toHaveAttribute("data-muted", "true");
  await expect(page.locator("#local-audio-track")).toHaveAttribute(
    "data-publication-muted",
    "true",
  );
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "1");
  await expect(page.locator(".notice.error")).toHaveCount(0);
});

test("a mute queued behind a failed unpublish still runs", async ({ page, browserName }) => {
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await page.getByLabel("Room name").fill(`reverse-serialized-audio-controls-${Date.now()}`);
  await page.evaluate(() => {
    const events = [];
    window.__reverseAudioMutationEvents = events;
    const track = window.LivekitClient.LocalAudioTrack.prototype;
    const participant = window.LivekitClient.LocalParticipant.prototype;
    const originalMute = track.mute;
    participant.unpublishTrack = async function (publishedTrack) {
      if (publishedTrack.kind === "audio") {
        events.push("unpublish-start");
        await new Promise((resolve) => setTimeout(resolve, 300));
        events.push("unpublish-failed");
        throw new Error("synthetic unpublish failure");
      }
    };
    track.mute = async function (...args) {
      events.push("mute");
      return originalMute.apply(this, args);
    };
  });
  await page.getByRole("button", { name: "Start microphone" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish microphone" }).click();
  const publication = page.locator('.local-track-publication[data-kind="audio"]');
  await expect(publication).toHaveAttribute("data-muted", "false");

  await page.getByRole("button", { name: "Unpublish microphone" }).click();
  await page.getByRole("button", { name: "Mute microphone publication" }).click();

  await expect(publication).toHaveAttribute("data-muted", "true");
  expect(await page.evaluate(() => window.__reverseAudioMutationEvents)).toEqual([
    "unpublish-start",
    "unpublish-failed",
    "mute",
  ]);
  await expect(page.locator(".notice.error")).toContainText("synthetic unpublish failure");
  await page.getByRole("button", { name: "Leave room" }).click();
});

test("republishing a muted microphone preserves typed mute state", async ({
  page,
  browserName,
}) => {
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await page.getByLabel("Room name").fill(`republished-muted-audio-${Date.now()}`);
  await page.getByRole("button", { name: "Start microphone" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish microphone", exact: true }).click();
  await page.getByRole("button", { name: "Mute microphone publication" }).click();
  await expect(page.locator("#local-audio-track")).toHaveAttribute(
    "data-publication-muted",
    "true",
  );
  await page.getByRole("button", { name: "Unpublish microphone" }).click();

  await page.getByRole("button", { name: "Publish microphone", exact: true }).click();

  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveAttribute(
    "data-muted",
    "true",
  );
  await expect(page.locator("#local-audio-track")).toHaveAttribute(
    "data-publication-muted",
    "true",
  );
});

test("a delayed microphone mutation does not block camera cleanup", async ({
  page,
  browserName,
}) => {
  await page.addInitScript(() => {
    window.__independentMutationCalls = {
      audioPublishDone: false,
      audioStoppedAfterPublish: false,
      videoStoppedFirst: false,
    };
    const original = MediaStreamTrack.prototype.stop;
    MediaStreamTrack.prototype.stop = function (...args) {
      if (this.kind === "audio") {
        window.__independentMutationCalls.audioStoppedAfterPublish =
          window.__independentMutationCalls.audioPublishDone;
      }
      if (this.kind === "video" && !window.__independentMutationCalls.audioPublishDone) {
        window.__independentMutationCalls.videoStoppedFirst = true;
      }
      return original.apply(this, args);
    };
  });
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await installWebKitCamera(page, browserName);
  await page.getByLabel("Room name").fill(`independent-mutations-${Date.now()}`);
  await page.getByRole("button", { name: "Start camera" }).click();
  await page.getByRole("button", { name: "Start microphone" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.evaluate(() => {
    const participant = window.LivekitClient.LocalParticipant.prototype;
    const original = participant.publishTrack;
    participant.publishTrack = async function (track, ...args) {
      if (track.kind === "audio") await new Promise((resolve) => setTimeout(resolve, 500));
      const publication = await original.call(this, track, ...args);
      if (track.kind === "audio") window.__independentMutationCalls.audioPublishDone = true;
      return publication;
    };
  });

  await page.getByRole("button", { name: "Publish microphone" }).click();
  await page.getByRole("button", { name: "Stop microphone" }).click();
  await page.getByRole("button", { name: "Stop camera" }).click();

  await expect
    .poll(() => page.evaluate(() => window.__independentMutationCalls.videoStoppedFirst))
    .toBe(true);
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(1);
  await expect
    .poll(() => page.evaluate(() => window.__independentMutationCalls.audioStoppedAfterPublish))
    .toBe(true);
  await expect(page.locator("#microphone-state")).toHaveText("off");
  await expect(page.locator(".notice.error")).toHaveCount(0);
  await page.getByRole("button", { name: "Leave room" }).click();
});

test("a stopped published track cannot reacquire the microphone through unmute", async ({
  page,
  browserName,
}) => {
  await page.addInitScript(() => {
    const original = navigator.mediaDevices.getUserMedia;
    window.__localAudioCalls = { userMedia: 0 };
    navigator.mediaDevices.getUserMedia = function (...args) {
      window.__localAudioCalls.userMedia += 1;
      return original.apply(this, args);
    };
  });
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await page.getByLabel("Room name").fill(`stopped-audio-publication-${Date.now()}`);
  await page.getByRole("button", { name: "Start microphone" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish microphone" }).click();
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(1);
  await page.getByRole("button", { name: "Mute microphone publication" }).click();
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveAttribute(
    "data-muted",
    "true",
  );

  await page.getByRole("button", { name: "Stop microphone" }).click();
  await page.locator("#unmute-local-audio-publication").dispatchEvent("click");

  await expect(page.locator(".notice.error")).toContainText("capture has stopped");
  await expect(page.locator("#local-audio-track")).toHaveAttribute("data-stopped", "true");
  expect(await page.evaluate(() => window.__localAudioCalls.userMedia)).toBe(1);
  await page.getByRole("button", { name: "Leave room" }).click();
});

test("a stopped microphone can still be unpublished without reopening capture", async ({
  page,
  browserName,
}) => {
  await page.goto("/");
  await installWebKitMicrophone(page, browserName);
  await page.getByLabel("Room name").fill(`stopped-audio-unpublish-${Date.now()}`);
  await page.getByRole("button", { name: "Start microphone" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish microphone" }).click();
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(1);

  await page.getByRole("button", { name: "Stop microphone" }).click();
  await page.getByRole("button", { name: "Unpublish microphone" }).click();

  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(0);
  await expect(page.locator("#local-audio-track")).toHaveAttribute("data-stopped", "true");
});

test("microphone controls fit a phone-sized browser", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");

  await expect(page.getByRole("button", { name: "Start microphone" })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
});
