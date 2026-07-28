import { expect, test } from "@playwright/test";

async function installMediaDeviceFixture(page) {
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
    const videoTrack = canvas.captureStream(30).getVideoTracks()[0];

    const AudioContext = window.AudioContext || window.webkitAudioContext;
    const audioContext = new AudioContext({ sampleRate: 48_000 });
    const oscillator = audioContext.createOscillator();
    const destination = audioContext.createMediaStreamDestination();
    oscillator.connect(destination);
    oscillator.start();
    const audioTrack = destination.stream.getAudioTracks()[0];
    const devices = [
      { deviceId: "camera-front", groupId: "camera-group-1", kind: "videoinput", label: "Front camera" },
      { deviceId: "camera-rear", groupId: "camera-group-2", kind: "videoinput", label: "Rear camera" },
      { deviceId: "microphone-built-in", groupId: "audio-group-1", kind: "audioinput", label: "Built-in microphone" },
      { deviceId: "microphone-usb", groupId: "audio-group-2", kind: "audioinput", label: "USB microphone" },
    ];
    const constraintValue = (constraint, fallback) =>
      constraint?.exact ?? constraint?.ideal ?? constraint ?? fallback;
    const withDeviceSettings = (track, deviceId) => {
      const settings = track.getSettings.bind(track);
      const device = devices.find((candidate) => candidate.deviceId === deviceId);
      const appliedSettings = {};
      Object.defineProperty(track, "getSettings", {
        configurable: true,
        value: () => ({
          ...settings(),
          ...appliedSettings,
          deviceId,
          groupId: device?.groupId || "",
        }),
      });
      Object.defineProperty(track, "applyConstraints", {
        configurable: true,
        value: async (constraints) => {
          if (track.kind === "video") {
            window.__fixtureAppliedVideoConstraints.push(structuredClone(constraints));
          }
          const failure = window.__rejectNextFixtureVideoConstraint;
          if (track.kind === "video" && failure && Object.keys(constraints || {}).length > 0) {
            window.__rejectNextFixtureVideoConstraint = null;
            throw new DOMException(failure.message, failure.name);
          }
          for (const [name, value] of Object.entries(constraints || {})) {
            appliedSettings[name] = value?.exact ?? value?.ideal ?? value;
          }
        },
      });
      return track;
    };

    mediaDevices.enumerateDevices = async () => devices.map((device) => ({ ...device }));
    window.__fixtureAppliedVideoConstraints = [];
    Object.defineProperty(mediaDevices, "getUserMedia", {
      configurable: true,
      writable: true,
      value: async (constraints) => {
        const tracks = [];
        if (constraints?.video) {
          const videoConstraints = constraints.video === true ? {} : constraints.video;
          const deviceId = constraintValue(videoConstraints.deviceId, "camera-front");
          tracks.push(withDeviceSettings(videoTrack.clone(), deviceId));
        }
        if (constraints?.audio) {
          const audioConstraints = constraints.audio === true ? {} : constraints.audio;
          const deviceId = constraintValue(audioConstraints.deviceId, "microphone-built-in");
          tracks.push(withDeviceSettings(audioTrack.clone(), deviceId));
        }
        return new MediaStream(tracks);
      },
    });
    window.__mediaDeviceFixture = { audioContext, canvas, destination, oscillator };
  });
}

test("enumerates normalized camera and microphone inputs with permission-gated labels", async ({
  page,
}) => {
  await page.goto("/");
  await page.evaluate(() => {
    window.__deviceEnumerationCalls = [];
    window.LivekitClient.Room.getLocalDevices = async (kind, requestPermissions) => {
      window.__deviceEnumerationCalls.push({ kind, requestPermissions });
      return kind === "videoinput"
        ? [
            {
              deviceId: "camera-front",
              groupId: "camera-group",
              label: requestPermissions ? "Front camera" : "",
              kind,
            },
          ]
        : [
            {
              deviceId: "microphone-usb",
              groupId: "audio-group",
              label: requestPermissions ? "USB microphone" : "",
              kind,
            },
          ];
    };
  });

  await page.getByRole("button", { name: "Refresh input devices" }).click();

  await expect(page.getByLabel("Camera input")).toContainText("Front camera");
  await expect(page.getByLabel("Microphone input")).toContainText("USB microphone");
  await expect(page.locator("#media-devices")).toHaveAttribute("data-camera-count", "1");
  await expect(page.locator("#media-devices")).toHaveAttribute("data-microphone-count", "1");
  expect(await page.evaluate(() => window.__deviceEnumerationCalls)).toEqual([
    { kind: "videoinput", requestPermissions: true },
    { kind: "audioinput", requestPermissions: true },
  ]);
});

test("typed camera and microphone constraints reach LiveKit capture", async ({ page }) => {
  await page.goto("/");
  await installMediaDeviceFixture(page);
  await page.evaluate(() => {
    window.__typedCaptureOptions = { camera: null, microphone: null };
    const createVideo = window.LivekitClient.createLocalVideoTrack;
    const createAudio = window.LivekitClient.createLocalAudioTrack;
    window.LivekitClient.createLocalVideoTrack = function (options) {
      window.__typedCaptureOptions.camera = structuredClone(options);
      return createVideo.call(this, options);
    };
    window.LivekitClient.createLocalAudioTrack = function (options) {
      window.__typedCaptureOptions.microphone = structuredClone(options);
      return createAudio.call(this, options);
    };
  });

  await page.getByRole("button", { name: "Start camera" }).click();
  await expect(page.locator("#local-video-track")).toHaveAttribute("data-stopped", "false");
  await page.getByRole("button", { name: "Start microphone" }).click();

  const options = await page.evaluate(() => window.__typedCaptureOptions);
  expect(options.camera).toMatchObject({
    facingMode: "user",
    resolution: { width: 640, height: 360 },
    frameRate: 30,
  });
  expect(options.microphone).toMatchObject({
    autoGainControl: true,
    channelCount: 1,
    echoCancellation: true,
    noiseSuppression: true,
    sampleRate: 48_000,
  });
  await expect(page.locator("#local-audio-track")).toHaveAttribute("data-stopped", "false");
});

test("restarts a published Local Media Track without replacing its Room Session or Track Publication", async ({
  page,
}) => {
  await page.goto("/");
  await installMediaDeviceFixture(page);
  await page.getByLabel("Room name").fill(`camera-restart-${Date.now()}`);
  await page.getByRole("button", { name: "Start camera" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish camera" }).click();
  const publication = page.locator('.local-track-publication[data-kind="video"]');
  await expect(publication).toHaveCount(1);
  const publicationSid = await publication.getAttribute("data-publication-sid");
  const participantSid = await page.locator("#room-session").getAttribute("data-participant-sid");

  await page.getByLabel("Camera facing mode").selectOption("environment");
  await page.getByRole("button", { name: "Restart camera" }).click();

  await expect(page.locator("#connection-state")).toHaveText("connected");
  await expect(page.locator("#room-session")).toHaveAttribute("data-participant-sid", participantSid);
  await expect(publication).toHaveAttribute("data-publication-sid", publicationSid);
  await expect(page.locator("#local-video-track")).toHaveAttribute("data-stopped", "false");
  await expect(page.locator("#local-video-track")).toHaveAttribute(
    "data-preview-attached",
    "true",
  );
  await expect
    .poll(() => page.locator("#local-video").evaluate((video) => video.srcObject?.active === true))
    .toBe(true);

  await page.evaluate(() => {
    window.__fixtureAppliedVideoConstraints = [];
  });
  await page.getByRole("button", { name: "Mute camera publication" }).click();
  await expect(publication).toHaveAttribute("data-muted", "true");
  await page.locator("#unmute-local-video-publication").dispatchEvent("click");
  await expect(publication).toHaveAttribute("data-muted", "false");
  expect(
    await page.evaluate(() =>
      window.__fixtureAppliedVideoConstraints.find(
        (constraints) => Object.keys(constraints).length > 0,
      ),
    ),
  ).toMatchObject({
    width: { ideal: 640 },
    height: { ideal: 360 },
    frameRate: { ideal: 30 },
  });
  await expect(publication).toHaveAttribute("data-publication-sid", publicationSid);
});

test("switches published camera and microphone inputs without replacing Track Publications", async ({
  page,
}) => {
  await page.goto("/");
  await installMediaDeviceFixture(page);
  await page.getByRole("button", { name: "Refresh input devices" }).click();
  await expect(page.locator("#media-devices")).toHaveAttribute("data-camera-count", /[1-9]/);
  await expect(page.locator("#media-devices")).toHaveAttribute("data-microphone-count", /[1-9]/);
  const initialCameraDevice = await page
    .getByLabel("Camera input")
    .locator("option")
    .nth(1)
    .getAttribute("value");
  const initialMicrophoneDevice = await page
    .getByLabel("Microphone input")
    .locator("option")
    .nth(1)
    .getAttribute("value");
  await page.getByLabel("Camera input").selectOption(initialCameraDevice);
  await page.getByLabel("Microphone input").selectOption(initialMicrophoneDevice);
  await page.evaluate(() => {
    window.__switchDeviceOptions = { camera: null, microphone: null };
    const video = window.LivekitClient.LocalVideoTrack.prototype;
    const audio = window.LivekitClient.LocalAudioTrack.prototype;
    const restartVideo = video.restartTrack;
    const restartAudio = audio.restartTrack;
    video.restartTrack = function (options) {
      window.__switchDeviceOptions.camera = structuredClone(options);
      return restartVideo.call(this, options);
    };
    audio.restartTrack = function (options) {
      window.__switchDeviceOptions.microphone = structuredClone(options);
      return restartAudio.call(this, options);
    };
  });
  await page.getByLabel("Room name").fill(`device-switch-${Date.now()}`);
  await page.getByRole("button", { name: "Start camera" }).click();
  await expect(page.locator("#local-video-track")).toHaveAttribute("data-stopped", "false");
  await page.getByRole("button", { name: "Start microphone" }).click();
  await expect(page.locator("#local-audio-track")).toHaveAttribute("data-stopped", "false");
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish camera" }).click();
  await expect(page.locator('.local-track-publication[data-kind="video"]')).toHaveCount(1);
  await page.getByRole("button", { name: "Publish microphone" }).click();
  const cameraPublication = page.locator('.local-track-publication[data-kind="video"]');
  const audioPublication = page.locator('.local-track-publication[data-kind="audio"]');
  await expect(audioPublication).toHaveCount(1);
  const cameraSid = await cameraPublication.getAttribute("data-publication-sid");
  const audioSid = await audioPublication.getAttribute("data-publication-sid");

  const cameraDevice = await page
    .getByLabel("Camera input")
    .locator("option")
    .nth(2)
    .getAttribute("value");
  const microphoneDevice = await page
    .getByLabel("Microphone input")
    .locator("option")
    .nth(2)
    .getAttribute("value");
  await page.getByLabel("Camera input").selectOption(cameraDevice);
  await page.getByLabel("Microphone input").selectOption(microphoneDevice);

  await page.getByRole("button", { name: "Switch camera" }).click();
  await expect(page.locator("#media-devices")).toHaveAttribute(
    "data-active-camera-id",
    cameraDevice,
  );
  await page.getByRole("button", { name: "Switch microphone" }).click();

  await expect(page.locator("#media-devices")).toHaveAttribute(
    "data-active-camera-id",
    cameraDevice,
  );
  await expect(page.locator("#media-devices")).toHaveAttribute(
    "data-active-microphone-id",
    microphoneDevice,
  );
  expect(await page.evaluate(() => window.__switchDeviceOptions)).toMatchObject({
    camera: { deviceId: { exact: cameraDevice } },
    microphone: { deviceId: { exact: microphoneDevice } },
  });
  await expect(cameraPublication).toHaveAttribute("data-publication-sid", cameraSid);
  await expect(audioPublication).toHaveAttribute("data-publication-sid", audioSid);
  await expect(page.locator("#room-session")).toHaveAttribute("data-publication-count", "2");
  await expect(page.locator("#connection-state")).toHaveText("connected");
});

test("failed device changes are classified and preserve existing Track Publications", async ({
  page,
}) => {
  await page.goto("/");
  await installMediaDeviceFixture(page);
  await page.getByLabel("Room name").fill(`failed-device-change-${Date.now()}`);
  await page.getByRole("button", { name: "Start camera" }).click();
  await expect(page.locator("#local-video-track")).toHaveAttribute("data-stopped", "false");
  await page.getByRole("button", { name: "Start microphone" }).click();
  await expect(page.locator("#local-audio-track")).toHaveAttribute("data-stopped", "false");
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish camera" }).click();
  await page.getByRole("button", { name: "Publish microphone" }).click();
  const cameraPublication = page.locator('.local-track-publication[data-kind="video"]');
  const audioPublication = page.locator('.local-track-publication[data-kind="audio"]');
  await expect(cameraPublication).toHaveCount(1, { timeout: 15_000 });
  await expect(audioPublication).toHaveCount(1, { timeout: 15_000 });
  const cameraSid = await cameraPublication.getAttribute("data-publication-sid");
  const audioSid = await audioPublication.getAttribute("data-publication-sid");
  await page.evaluate(() => {
    const video = window.LivekitClient.LocalVideoTrack.prototype;
    const audio = window.LivekitClient.LocalAudioTrack.prototype;
    const restartVideo = video.restartTrack;
    const restartAudio = audio.restartTrack;
    window.__failNextDeviceChange = null;
    video.restartTrack = function (...args) {
      const failure = window.__failNextDeviceChange;
      if (failure?.kind === "video") {
        window.__failNextDeviceChange = null;
        return Promise.reject(new DOMException("synthetic device failure", failure.name));
      }
      return restartVideo.apply(this, args);
    };
    audio.restartTrack = function (...args) {
      const failure = window.__failNextDeviceChange;
      if (failure?.kind === "audio") {
        window.__failNextDeviceChange = null;
        return Promise.reject(new DOMException("synthetic device failure", failure.name));
      }
      return restartAudio.apply(this, args);
    };
  });

  for (const [name, expected] of [
    ["NotAllowedError", "permissionDenied"],
    ["OverconstrainedError", "deviceUnavailable"],
  ]) {
    await page.evaluate((failureName) => {
      window.__failNextDeviceChange = { kind: "video", name: failureName };
    }, name);
    await page.getByRole("button", { name: "Restart camera" }).click();
    await expect(page.locator("#media-operation-error")).toHaveAttribute(
      "data-kind",
      expected,
    );
    await expect(cameraPublication).toHaveAttribute("data-publication-sid", cameraSid);
    await expect(page.locator("#local-video-track")).toHaveAttribute("data-stopped", "false");
  }

  await page.evaluate(() => {
    window.__failNextDeviceChange = { kind: "audio", name: "NotReadableError" };
  });
  await page.getByRole("button", { name: "Restart microphone" }).click();
  await expect(page.locator("#media-operation-error")).toHaveAttribute(
    "data-kind",
    "deviceInUse",
  );
  await expect(audioPublication).toHaveAttribute("data-publication-sid", audioSid);
  await expect(page.locator("#local-audio-track")).toHaveAttribute("data-stopped", "false");
  await expect(page.locator("#connection-state")).toHaveText("connected");
});

test("failed camera constraint restart stops the rejected replacement before rollback", async ({
  page,
}) => {
  await page.goto("/");
  await installMediaDeviceFixture(page);
  await page.getByLabel("Room name").fill(`constraint-rollback-${Date.now()}`);
  await page.getByRole("button", { name: "Start camera" }).click();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish camera" }).click();
  const publication = page.locator('.local-track-publication[data-kind="video"]');
  await expect(publication).toHaveCount(1, { timeout: 15_000 });
  const publicationSid = await publication.getAttribute("data-publication-sid");
  await page.evaluate(() => {
    window.__restartCameraTracks = [];
    const getUserMedia = navigator.mediaDevices.getUserMedia;
    navigator.mediaDevices.getUserMedia = async (...args) => {
      const stream = await getUserMedia(...args);
      window.__restartCameraTracks.push(...stream.getVideoTracks());
      return stream;
    };
    window.__rejectNextFixtureVideoConstraint = {
      name: "OverconstrainedError",
      message: "unsupported camera constraint",
    };
  });

  await page.getByRole("button", { name: "Restart camera" }).click();

  await expect(page.locator("#media-operation-error")).toHaveAttribute(
    "data-kind",
    "deviceUnavailable",
  );
  await expect(publication).toHaveAttribute("data-publication-sid", publicationSid);
  await expect(page.locator("#local-video-track")).toHaveAttribute("data-stopped", "false");
  await expect
    .poll(() =>
      page.evaluate(
        () => window.__restartCameraTracks.filter((track) => track.readyState === "live").length,
      ),
    )
    .toBe(1);
});
