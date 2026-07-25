import { expect, test } from "@playwright/test";

async function trackRuntimeMutations(page) {
  await page.addInitScript(() => {
    const calls = {
      audioPlayback: 0,
      displayCapture: 0,
      mediaRecorder: 0,
      mediaStream: 0,
      rooms: 0,
      sdkMedia: 0,
      userMedia: 0,
    };
    window.__livekitRuntimeCalls = calls;

    const countMethodCalls = (target, method, counter) => {
      const original = target?.[method];
      if (typeof original !== "function") return;
      target[method] = function (...args) {
        calls[counter] += 1;
        return original.apply(this, args);
      };
    };

    countMethodCalls(navigator.mediaDevices, "getUserMedia", "userMedia");
    countMethodCalls(navigator.mediaDevices, "getDisplayMedia", "displayCapture");
    countMethodCalls(HTMLMediaElement.prototype, "play", "audioPlayback");

    const countConstruction = (name, counter) => {
      const Original = globalThis[name];
      if (typeof Original !== "function") return;
      globalThis[name] = new Proxy(Original, {
        construct(target, args, newTarget) {
          calls[counter] += 1;
          return Reflect.construct(target, args, newTarget);
        },
      });
    };
    countConstruction("MediaStream", "mediaStream");
    countConstruction("MediaRecorder", "mediaRecorder");
  });
}

async function mutateLoadedSdk(page, mutation) {
  await page.addInitScript((sdkMutation) => {
    Object.defineProperty(globalThis, "LivekitClient", {
      configurable: true,
      set(sdk) {
        Object.defineProperty(globalThis, "LivekitClient", {
          configurable: true,
          writable: true,
          value: sdk,
        });
        queueMicrotask(() => {
          const runtimeCalls = globalThis.__livekitRuntimeCalls;
          if (runtimeCalls) {
            const InstrumentedRoom = new Proxy(sdk.Room, {
              construct(target, args) {
                runtimeCalls.rooms += 1;
                return Reflect.construct(target, args);
              },
            });
            const mediaMethods = new Map();
            for (const method of [
              "createLocalAudioTrack",
              "createLocalVideoTrack",
              "createLocalScreenTracks",
            ]) {
              const original = sdk[method];
              if (typeof original !== "function") continue;
              mediaMethods.set(method, function (...args) {
                runtimeCalls.sdkMedia += 1;
                return original.apply(this, args);
              });
            }
            globalThis.LivekitClient = new Proxy(sdk, {
              get(target, property, receiver) {
                if (property === "Room") return InstrumentedRoom;
                return mediaMethods.get(property) ?? Reflect.get(target, property, receiver);
              },
            });
          }
          globalThis.eval(sdkMutation);
        });
      },
    });
  }, mutation);
}

async function expectNoRuntimeMutations(page) {
  expect(await page.evaluate(() => window.__livekitRuntimeCalls)).toEqual({
    audioPlayback: 0,
    displayCapture: 0,
    mediaRecorder: 0,
    mediaStream: 0,
    rooms: 0,
    sdkMedia: 0,
    userMedia: 0,
  });
}

test("initializes typed capabilities without requesting browser permissions", async ({ page }) => {
  await trackRuntimeMutations(page);
  await mutateLoadedSdk(page, "");

  await page.goto("/");

  const runtime = page.locator("#browser-runtime");
  const browserCapabilities = await page.evaluate(() => ({
    audioPlayback: typeof HTMLMediaElement.prototype.play === "function",
    displayCapture: typeof navigator.mediaDevices?.getDisplayMedia === "function",
    userMedia: typeof navigator.mediaDevices?.getUserMedia === "function",
  }));
  await expect(runtime).toHaveAttribute("data-state", "ready");
  await expect(runtime).toHaveAttribute("data-sdk-version", "2.20.1");
  await expect(runtime).toHaveAttribute("data-webrtc", "true");
  await expect(runtime).toHaveAttribute("data-secure-context", "true");
  await expect(runtime).toHaveAttribute("data-user-media", String(browserCapabilities.userMedia));
  await expect(runtime).toHaveAttribute(
    "data-display-capture",
    String(browserCapabilities.displayCapture),
  );
  await expect(runtime).toHaveAttribute(
    "data-audio-playback",
    String(browserCapabilities.audioPlayback),
  );
  await expect(runtime).toHaveAttribute("data-transcription", "true");
  await expect(runtime).toHaveAttribute("data-local-clip-recording", /^(true|false)$/);
  await expectNoRuntimeMutations(page);
});

test("raw-js preserves actual Room and Local Participant identity", async ({ page }) => {
  await page.goto("/");
  await page.getByLabel("Room name").fill(`raw-js-${Date.now()}`);
  await page.getByLabel("Your identity").fill("raw-js-participant");
  await page.getByRole("button", { name: "Join room" }).click();

  await expect(page.locator("#connection-state")).toHaveText("connected", {
    timeout: 30_000,
  });
  await expect(page.locator("#raw-js-contract")).toHaveAttribute("data-state", "ready");
  await page.getByRole("button", { name: "Leave room" }).click();
});

test("delivers Room SDK logs through typed Rust records while retaining console output", async ({
  page,
}) => {
  const consoleMessages = [];
  page.on("console", (message) => consoleMessages.push(message.text()));
  await page.goto("/");

  await page.evaluate(() => {
    const sdk = globalThis.LivekitClient;
    sdk.setLogLevel(sdk.LogLevel.debug, sdk.LoggerNames.Room);
  });
  await page.getByLabel("Room name").fill(`diagnostics-${Date.now()}`);
  await page.getByLabel("Your identity").fill("diagnostics-participant");
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });

  await page.waitForTimeout(100);
  expect(
    consoleMessages.filter((message) => message.includes("LiveKit Rust log callback failed")),
  ).toEqual([]);

  const record = page.locator("#livekit-log-record");
  await expect(record).toHaveAttribute("data-level", /^(debug|info|warn|error)$/);
  await expect(record).toHaveAttribute("data-message", /.+/);
  const message = await record.getAttribute("data-message");
  const context = JSON.parse(await record.getAttribute("data-context"));
  expect(context).toBeTruthy();
  expect(consoleMessages.some((consoleMessage) => consoleMessage.includes(message))).toBe(true);
  await page.getByRole("button", { name: "Leave room" }).click();
});

test("rejects unavailable user media before permission or SDK media mutation", async ({ page }) => {
  await trackRuntimeMutations(page);
  await page.addInitScript(() => {
    Object.defineProperty(navigator.mediaDevices, "getUserMedia", {
      configurable: true,
      value: undefined,
    });
  });
  await mutateLoadedSdk(page, "");

  await page.goto("/");

  const runtime = page.locator("#browser-runtime");
  await expect(runtime).toHaveAttribute("data-state", "ready");
  await expect(runtime).toHaveAttribute("data-user-media", "false");
  await page.getByRole("button", { name: "Start camera" }).click();
  await expect(page.locator("#media-operation-error")).toHaveAttribute(
    "data-kind",
    "unsupportedBrowser",
  );
  await page.getByRole("button", { name: "Start microphone" }).click();
  await expect(page.locator("#camera-state")).toHaveText("off");
  await expect(page.locator("#microphone-state")).toHaveText("off");
  await expectNoRuntimeMutations(page);
});

test("represents unavailable transcription without destabilizing a Room Session", async ({ page }) => {
  await mutateLoadedSdk(
    page,
    `const roomEventsWithoutTranscription = { ...globalThis.LivekitClient.RoomEvent };
     delete roomEventsWithoutTranscription.TranscriptionReceived;
     globalThis.LivekitClient = new Proxy(globalThis.LivekitClient, {
       get(target, property, receiver) {
         return property === "RoomEvent"
           ? roomEventsWithoutTranscription
           : Reflect.get(target, property, receiver);
       },
     });`,
  );

  await page.goto("/");

  await expect(page.locator("#browser-runtime")).toHaveAttribute("data-state", "ready");
  await expect(page.locator("#browser-runtime")).toHaveAttribute("data-transcription", "false");
  await page.getByLabel("Room name").fill(`no-transcription-${Date.now()}`);
  await page.getByRole("button", { name: "Join room" }).click();
  await expect
    .poll(
      async () => ({
        connection: await page.locator("#connection-state").textContent(),
        errors: await page.locator(".notice.error").allTextContents(),
      }),
      { timeout: 30_000 },
    )
    .toEqual({ connection: "connected", errors: [] });
  await expect(page.locator("#transcription-segments")).toHaveAttribute(
    "data-observing",
    "true",
  );
  await page.getByRole("button", { name: "Leave room" }).click();
  await expect(page.locator("#transcription-segments")).toHaveAttribute("data-closed", "true");
});

for (const scenario of [
  {
    name: "a missing SDK global",
    mutation: "delete globalThis.LivekitClient;",
    error: "LiveKit browser SDK is missing",
  },
  {
    name: "an exact-version mismatch",
    mutation: `globalThis.LivekitClient = new Proxy(globalThis.LivekitClient, {
      get(target, property, receiver) {
        return property === "version" ? "2.20.0" : Reflect.get(target, property, receiver);
      },
    });`,
    error: "expected livekit-client 2.20.1, found 2.20.0",
  },
  {
    name: "an unsupported required capability",
    mutation: `globalThis.LivekitClient = new Proxy(globalThis.LivekitClient, {
      get(target, property, receiver) {
        return property === "isBrowserSupported"
          ? () => false
          : Reflect.get(target, property, receiver);
      },
    });`,
    error: "WebRTC required by LiveKit is unavailable",
  },
  {
    name: "a missing data-stream API",
    mutation: "delete globalThis.LivekitClient.LocalParticipant.prototype.streamText;",
    error: "LivekitClient.LocalParticipant.streamText is not a function",
  },
  {
    name: "a missing Participant RPC API",
    mutation: "delete globalThis.LivekitClient.LocalParticipant.prototype.performRpc;",
    error: "LivekitClient.LocalParticipant.performRpc is not a function",
  },
  {
    name: "a missing chat API",
    mutation: "delete globalThis.LivekitClient.LocalParticipant.prototype.sendChatMessage;",
    error: "LivekitClient.LocalParticipant.sendChatMessage is not a function",
  },
  {
    name: "a missing device enumeration API",
    mutation: "delete globalThis.LivekitClient.Room.getLocalDevices;",
    error: "LivekitClient.Room.getLocalDevices is not a function",
  },
  {
    name: "a missing publication API",
    mutation: "delete globalThis.LivekitClient.LocalParticipant.prototype.publishTrack;",
    error: "LivekitClient.LocalParticipant.publishTrack is not a function",
  },
  {
    name: "a missing media lifecycle API",
    mutation: "delete globalThis.LivekitClient.LocalVideoTrack.prototype.restartTrack;",
    error: "LivekitClient.LocalVideoTrack.restartTrack is not a function",
  },
  {
    name: "a missing subscription API",
    mutation: "delete globalThis.LivekitClient.RemoteTrackPublication.prototype.setSubscribed;",
    error: "LivekitClient.RemoteTrackPublication.setSubscribed is not a function",
  },
  {
    name: "a missing required Room event",
    mutation: `globalThis.LivekitClient.RoomEvent = {
      ...globalThis.LivekitClient.RoomEvent,
      ConnectionStateChanged: undefined,
    };`,
    error: "LivekitClient.RoomEvent.ConnectionStateChanged is not a string",
  },
  {
    name: "a missing diagnostics API",
    mutation: "delete globalThis.LivekitClient.setLogExtension;",
    error: "LivekitClient.setLogExtension is not a function",
  },
]) {
  test(`fails before room or media state for ${scenario.name}`, async ({ page }) => {
    await trackRuntimeMutations(page);
    await mutateLoadedSdk(page, scenario.mutation);

    await page.goto("/");

    await expect(page.locator("#browser-runtime")).toHaveAttribute("data-state", "failed");
    await expect(page.locator("#browser-runtime-error")).toContainText(scenario.error);
    await expect(page.getByRole("button", { name: "Join room" })).toBeDisabled();
    await expect(page.locator("#connection-state")).toHaveText("disconnected");
    await expect(page.locator("#camera-state")).toHaveText("off");
    await expect(page.locator("#microphone-state")).toHaveText("off");
    await expectNoRuntimeMutations(page);
  });
}
