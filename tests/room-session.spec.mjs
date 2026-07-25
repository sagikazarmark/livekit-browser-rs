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
    canvas.getContext("2d").fillRect(0, 0, canvas.width, canvas.height);
    const stream = canvas.captureStream(30);
    const original = mediaDevices.getUserMedia.bind(mediaDevices);
    Object.defineProperty(mediaDevices, "getUserMedia", {
      configurable: true,
      writable: true,
      value: async (constraints) => constraints?.video ? stream.clone() : original(constraints),
    });
    window.__roomSessionWebKitCamera = { canvas, stream };
  });
}

async function instrumentRoomContract(page) {
  await page.addInitScript(() => {
    const calls = {
      activeListeners: 0,
      connectArgumentCount: 0,
      connectOptions: null,
      disconnect: 0,
      displayCapture: 0,
      listenersAdded: 0,
      listenersRemoved: 0,
      removeAllListeners: 0,
      userMedia: 0,
      roomsConstructed: 0,
      instrumentationLoaded: false,
      delayConnectCompletion: false,
    };
    window.__livekitRoomCalls = calls;

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
  });

}

async function installRoomInstrumentation(page) {
  await page.evaluate(() => {
    globalThis.__livekitRoomCalls.instrumentationLoaded = true;
    const prototype = globalThis.LivekitClient.Room.prototype;
    const activeListeners = [];
    const originalOn = prototype.on;
    const originalOff = prototype.off;
    const originalRemoveAllListeners = prototype.removeAllListeners;
    prototype.on = function (event, listener) {
      globalThis.__livekitRoomCalls.listenersAdded += 1;
      activeListeners.push([event, listener]);
      globalThis.__livekitRoomCalls.activeListeners = activeListeners.length;
      return originalOn.call(this, event, listener);
    };
    prototype.off = function (event, listener) {
      globalThis.__livekitRoomCalls.listenersRemoved += 1;
      const index = activeListeners.findIndex(
        ([activeEvent, activeListener]) => activeEvent === event && activeListener === listener
      );
      if (index >= 0) activeListeners.splice(index, 1);
      globalThis.__livekitRoomCalls.activeListeners = activeListeners.length;
      return originalOff.call(this, event, listener);
    };
    prototype.removeAllListeners = function (event) {
      if (event === undefined) globalThis.__livekitRoomCalls.removeAllListeners += 1;
      if (event === undefined) activeListeners.length = 0;
      else {
        for (let index = activeListeners.length - 1; index >= 0; index -= 1) {
          if (activeListeners[index][0] === event) activeListeners.splice(index, 1);
        }
      }
      globalThis.__livekitRoomCalls.activeListeners = activeListeners.length;
      return originalRemoveAllListeners.call(this, event);
    };
    const OriginalRoom = globalThis.LivekitClient.Room;
    const InstrumentedRoom = new Proxy(OriginalRoom, {
      construct(target, args) {
        globalThis.__livekitRoomCalls.roomsConstructed += 1;
        const room = Reflect.construct(target, args);
        globalThis.__livekitTestRoom = room;
        const originalConnect = room.connect;
        const originalDisconnect = room.disconnect;
        room.connect = function (...connectArgs) {
          globalThis.__livekitRoomCalls.connectArgumentCount = connectArgs.length;
          globalThis.__livekitRoomCalls.connectOptions = structuredClone(connectArgs[2]);
          const connected = originalConnect.apply(this, connectArgs);
          if (!globalThis.__livekitRoomCalls.delayConnectCompletion) return connected;
          return Promise.resolve(connected).then(
            (value) => new Promise((resolve) => setTimeout(() => resolve(value), 1_000))
          );
        };
        room.disconnect = function (...disconnectArgs) {
          globalThis.__livekitRoomCalls.disconnect += 1;
          return originalDisconnect.apply(this, disconnectArgs);
        };
        return room;
      },
    });
    globalThis.LivekitClient = new Proxy(globalThis.LivekitClient, {
      get(target, property, receiver) {
        return property === "Room" ? InstrumentedRoom : Reflect.get(target, property, receiver);
      },
    });
  });
}

test("joins without capture and leaves with deterministic cleanup", async ({ page }) => {
  await instrumentRoomContract(page);
  await page.goto("/");
  await installRoomInstrumentation(page);
  expect(await page.evaluate(() => window.__livekitRoomCalls.instrumentationLoaded)).toBe(true);
  await page.getByLabel("Room name").fill("physics");
  await page.getByLabel("Your identity").fill("teacher");

  await page.getByRole("button", { name: "Join room" }).click();

  const session = page.locator("#room-session");
  await expect
    .poll(async () => ({
      connection: await page.locator("#connection-state").textContent(),
      errors: await page.locator(".notice.error").allTextContents(),
    }), { timeout: 30_000 })
    .toEqual({ connection: "connected", errors: [] });
  const connectedRevision = Number(await session.getAttribute("data-revision"));
  expect(connectedRevision).toBeGreaterThanOrEqual(1);
  await expect(session).toHaveAttribute("data-identity", "teacher");
  await expect(session).toHaveAttribute("data-participant-sid", /^PA_/);
  await expect(session).toHaveAttribute("data-metadata", "");
  await expect(session).toHaveAttribute("data-attribute-count", "0");
  await expect(session).toHaveAttribute("data-publication-count", "0");
  await expect(session).toHaveAttribute("data-can-publish", "true");
  await expect(session).toHaveAttribute("data-can-subscribe", "true");
  await expect(session).toHaveAttribute("data-can-publish-data", "true");
  await expect(session).toHaveAttribute("data-can-publish-source-count", "0");
  await expect(session).toHaveAttribute("data-can-update-metadata", "true");
  await expect(session).toHaveAttribute("data-hidden", "false");
  await expect(session).toHaveAttribute("data-recorder", "false");
  await expect(session).toHaveAttribute("data-agent", "false");
  await expect(session).toHaveAttribute("data-can-subscribe-metrics", "false");
  await expect(session).toHaveAttribute("data-can-manage-agent-session", "false");
  await expect(session).toHaveAttribute("data-terminal", "false");

  expect(await page.evaluate(() => window.__livekitRoomCalls)).toMatchObject({
    connectArgumentCount: 3,
    connectOptions: { autoSubscribe: true },
    disconnect: 0,
    displayCapture: 0,
    userMedia: 0,
  });

  await page.getByRole("button", { name: "Leave room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("disconnected");
  await expect(session).toHaveAttribute("data-terminal", "true");
  expect(Number(await session.getAttribute("data-revision"))).toBeGreaterThanOrEqual(
    connectedRevision + 2,
  );

  const calls = await page.evaluate(() => window.__livekitRoomCalls);
  expect(calls.disconnect).toBe(1);
  expect(calls.listenersRemoved).toBeGreaterThan(0);
  expect(calls.activeListeners).toBe(0);
});

test("leave disconnects while a published track mutation remains stalled", async ({
  page,
  browserName,
}) => {
  await instrumentRoomContract(page);
  await page.goto("/");
  await installWebKitCamera(page, browserName);
  await installRoomInstrumentation(page);
  await page.getByRole("button", { name: "Start camera" }).click();
  await expect(page.locator("#camera-state")).toHaveText("on");
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.getByRole("button", { name: "Publish camera" }).click();
  await expect(page.locator('.local-track-publication[data-kind="video"]')).toHaveCount(1);
  await page.evaluate(() => {
    globalThis.__stalledRestartStarted = 0;
    globalThis.LivekitClient.LocalVideoTrack.prototype.restartTrack = () => {
      globalThis.__stalledRestartStarted += 1;
      return new Promise((resolve) => {
        globalThis.__releaseStalledRestart = resolve;
      });
    };
  });

  await page.getByRole("button", { name: "Restart camera" }).click();
  await expect.poll(() => page.evaluate(() => globalThis.__stalledRestartStarted)).toBe(1);
  await page.getByRole("button", { name: "Leave room" }).click();

  await expect(page.locator("#connection-state")).toHaveText("disconnected");
  await expect(page.locator("#room-session")).toHaveAttribute("data-terminal", "true");
  await expect.poll(() => page.evaluate(() => window.__livekitRoomCalls.disconnect)).toBe(1);
  await expect
    .poll(() => page.evaluate(() => window.__livekitRoomCalls.removeAllListeners))
    .toBeGreaterThan(0);
  await page.evaluate(() => globalThis.__releaseStalledRestart());
});

test("dropping the final Room Session handle schedules disconnect", async ({ page }) => {
  await instrumentRoomContract(page);
  await page.goto("/");
  await installRoomInstrumentation(page);

  await page.locator("#drop-unprovided-room-session").dispatchEvent("click");

  await expect.poll(() => page.evaluate(() => window.__livekitRoomCalls.disconnect)).toBe(1);
  await expect.poll(() => page.evaluate(() => window.__livekitRoomCalls.activeListeners)).toBe(0);
});

test("dropping an in-flight join future releases its browser room", async ({ page }) => {
  await instrumentRoomContract(page);
  await page.goto("/");
  await installRoomInstrumentation(page);
  await page.evaluate(() => {
    window.__livekitRoomCalls.delayConnectCompletion = true;
  });

  await page.locator("#abandon-room-join").dispatchEvent("click");

  await expect.poll(() => page.evaluate(() => window.__livekitRoomCalls.disconnect)).toBe(1);
  await expect.poll(() => page.evaluate(() => window.__livekitRoomCalls.activeListeners)).toBe(0);
  await expect(page.locator("#connection-state")).toHaveText("disconnected");
});

test("dropping a leave future still installs terminal state", async ({ page }) => {
  await instrumentRoomContract(page);
  await page.goto("/");
  await installRoomInstrumentation(page);
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  const connectedRevision = Number(
    await page.locator("#room-session").getAttribute("data-revision"),
  );

  await page.locator("#abandon-room-leave").dispatchEvent("click");

  const session = page.locator("#room-session");
  await expect(page.locator("#connection-state")).toHaveText("disconnected");
  await expect(session).toHaveAttribute("data-terminal", "true");
  expect(Number(await session.getAttribute("data-revision"))).toBeGreaterThanOrEqual(
    connectedRevision + 2,
  );
  await expect.poll(() => page.evaluate(() => window.__livekitRoomCalls.disconnect)).toBe(1);
  await expect.poll(() => page.evaluate(() => window.__livekitRoomCalls.activeListeners)).toBe(0);
});

test("a terminal SDK disconnection closes the Room Session", async ({ page }) => {
  await instrumentRoomContract(page);
  await page.goto("/");
  await installRoomInstrumentation(page);
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  const connectedRevision = Number(
    await page.locator("#room-session").getAttribute("data-revision"),
  );

  await page.evaluate(() => window.__livekitTestRoom.disconnect());
  await page.locator("#refresh-room-session").dispatchEvent("click");

  const session = page.locator("#room-session");
  await expect(session).toHaveAttribute("data-terminal", "true");
  expect(Number(await session.getAttribute("data-revision"))).toBeGreaterThan(connectedRevision);
  await expect.poll(() => page.evaluate(() => window.__livekitRoomCalls.activeListeners)).toBe(0);
});

test("same-turn Room Events follow their installed snapshot", async ({ page }) => {
  await instrumentRoomContract(page);
  await page.goto("/");
  await installRoomInstrumentation(page);
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.locator("#observe-room-events").dispatchEvent("click");
  await expect(page.locator("#room-events")).toHaveAttribute("data-observing", "true");
  const previousEventCount = Number(
    await page.locator("#room-events").getAttribute("data-count"),
  );
  const previousRevision = Number(
    await page.locator("#room-session").getAttribute("data-revision"),
  );

  await page.evaluate(() => {
    const room = window.__livekitTestRoom;
    const event = window.LivekitClient.RoomEvent.ConnectionStateChanged;
    room.state = "reconnecting";
    room.emit(event, room.state);
    room.state = "connected";
    room.emit(event, room.state);
  });

  const events = page.locator("#room-events");
  await expect(events).toHaveAttribute("data-count", String(previousEventCount + 2));
  const eventRevision = Number(await events.getAttribute("data-event-revision"));
  expect(eventRevision).toBeGreaterThan(previousRevision);
  await expect(events).toHaveAttribute("data-snapshot-revision", String(eventRevision));
  await expect(page.locator("#room-session")).toHaveAttribute(
    "data-revision",
    String(eventRevision),
  );

  await page.getByRole("button", { name: "Leave room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("disconnected");
});

test("automatic signal reconnection stays in one Room Session", async ({ page }) => {
  test.setTimeout(90_000);
  await instrumentRoomContract(page);
  await page.goto("/");
  await installRoomInstrumentation(page);
  await page.getByLabel("Room name").fill(`signal-reconnect-${Date.now()}`);
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  const session = page.locator("#room-session");
  const connectedRevision = Number(await session.getAttribute("data-revision"));
  const originalRoom = await page.evaluateHandle(() => window.__livekitTestRoom);

  await page.evaluate(() => window.__livekitTestRoom.simulateScenario("signal-reconnect"));

  await expect.poll(() => page.locator("#connection-state").textContent()).toBe("connected");
  await expect
    .poll(async () => Number(await session.getAttribute("data-revision")), { timeout: 30_000 })
    .toBeGreaterThan(connectedRevision);
  await expect(session).toHaveAttribute("data-terminal", "false");
  expect(await page.evaluate(() => window.__livekitRoomCalls.roomsConstructed)).toBe(1);
  expect(await page.evaluate((room) => room === window.__livekitTestRoom, originalRoom)).toBe(true);

  await page.getByRole("button", { name: "Leave room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("disconnected");
});
