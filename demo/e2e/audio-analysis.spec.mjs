import { expect, test } from "@playwright/test";
import { openRoomParticipant } from "./helpers/room-participant.mjs";

async function instrumentWebAudio(page) {
  await page.evaluate(() => {
    const OriginalAudioContext = globalThis.AudioContext || globalThis.webkitAudioContext;
    const analysedTracks = new WeakSet();
    const calls = {
      contextsCreated: 0,
      contextsClosed: 0,
      sourceDisconnects: 0,
      analyserDisconnects: 0,
      silentSinkDisconnects: 0,
      endedListenersAdded: 0,
      endedListenersRemoved: 0,
    };
    globalThis.__audioAnalysisCalls = calls;

    const originalAddEventListener = MediaStreamTrack.prototype.addEventListener;
    const originalRemoveEventListener = MediaStreamTrack.prototype.removeEventListener;
    MediaStreamTrack.prototype.addEventListener = function (type, ...args) {
      if (type === "ended" && analysedTracks.has(this)) calls.endedListenersAdded += 1;
      return originalAddEventListener.call(this, type, ...args);
    };
    MediaStreamTrack.prototype.removeEventListener = function (type, ...args) {
      if (type === "ended" && analysedTracks.has(this)) calls.endedListenersRemoved += 1;
      return originalRemoveEventListener.call(this, type, ...args);
    };

    const InstrumentedAudioContext = new Proxy(OriginalAudioContext, {
      construct(target, args) {
        const context = Reflect.construct(target, args);
        let analysedContext = false;
        calls.contextsCreated += 1;

        const originalClose = context.close.bind(context);
        context.close = (...closeArgs) => {
          if (analysedContext) calls.contextsClosed += 1;
          return originalClose(...closeArgs);
        };

        const originalCreateSource = context.createMediaStreamSource.bind(context);
        context.createMediaStreamSource = (stream) => {
          analysedContext = true;
          for (const track of stream.getAudioTracks()) analysedTracks.add(track);
          const source = originalCreateSource(stream);
          const originalDisconnect = source.disconnect.bind(source);
          source.disconnect = (...disconnectArgs) => {
            calls.sourceDisconnects += 1;
            return originalDisconnect(...disconnectArgs);
          };
          return source;
        };

        const originalCreateAnalyser = context.createAnalyser.bind(context);
        context.createAnalyser = () => {
          const analyser = originalCreateAnalyser();
          const originalDisconnect = analyser.disconnect.bind(analyser);
          analyser.disconnect = (...disconnectArgs) => {
            calls.analyserDisconnects += 1;
            return originalDisconnect(...disconnectArgs);
          };
          return analyser;
        };

        const originalCreateGain = context.createGain.bind(context);
        context.createGain = () => {
          const gain = originalCreateGain();
          const originalDisconnect = gain.disconnect.bind(gain);
          gain.disconnect = (...disconnectArgs) => {
            calls.silentSinkDisconnects += 1;
            return originalDisconnect(...disconnectArgs);
          };
          return gain;
        };
        return context;
      },
    });
    Object.defineProperty(globalThis, "AudioContext", {
      configurable: true,
      writable: true,
      value: InstrumentedAudioContext,
    });
    if (globalThis.webkitAudioContext) {
      Object.defineProperty(globalThis, "webkitAudioContext", {
        configurable: true,
        writable: true,
        value: InstrumentedAudioContext,
      });
    }
  });
}

async function expectAnalysis(node, readButton, nonStatic) {
  const fingerprints = new Set();
  await expect
    .poll(async () => {
      await readButton.dispatchEvent("click");
      await node.page().waitForTimeout(40);
      const sample = await node.evaluate((element) => ({
        level: Number(element.dataset.analyserLevel),
        waveformLength: Number(element.dataset.analyserWaveformLength),
        fingerprint: element.dataset.analyserWaveformFingerprint,
      }));
      if (sample.fingerprint) fingerprints.add(sample.fingerprint);
      return {
        validLevel: Number.isFinite(sample.level) && (!nonStatic || sample.level > 0),
        waveformLength: sample.waveformLength,
        changed: !nonStatic || fingerprints.size > 1,
      };
    })
    .toEqual({ validLevel: true, waveformLength: 2048, changed: true });
}

async function cleanupCalls(page) {
  return page.evaluate(() => ({ ...globalThis.__audioAnalysisCalls }));
}

async function clickWithActivation(button) {
  await button.evaluate((element) => (element.hidden = false));
  await button.click();
}

async function expectOneCleanup(page, previous) {
  await expect
    .poll(async () => {
      const calls = await cleanupCalls(page);
      return {
        contextsClosed: calls.contextsClosed - previous.contextsClosed,
        sourceDisconnects: calls.sourceDisconnects - previous.sourceDisconnects,
        analyserDisconnects: calls.analyserDisconnects - previous.analyserDisconnects,
        silentSinkDisconnects:
          calls.silentSinkDisconnects - previous.silentSinkDisconnects,
        endedListenerRemoved: calls.endedListenersRemoved > previous.endedListenersRemoved,
      };
    })
    .toEqual({
      contextsClosed: 1,
      sourceDisconnects: 1,
      analyserDisconnects: 1,
      silentSinkDisconnects: 1,
      endedListenerRemoved: true,
    });
}

async function closeAnalyserAndExpectCleanup(page, node, closeButton) {
  const before = await cleanupCalls(page);
  await closeButton.dispatchEvent("click");
  await expect(node).toHaveAttribute("data-analyser-active", "false");
  await expectOneCleanup(page, before);
}

async function dropAnalyserAndExpectCleanup(page, node, dropButton) {
  const before = await cleanupCalls(page);
  await dropButton.dispatchEvent("click");
  await expect(node).toHaveAttribute("data-analyser-present", "false");
  await expectOneCleanup(page, before);
}

test("Local and Remote Media Tracks expose typed live audio analysis with deterministic cleanup", async ({
  browser,
  browserName,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const room = `audio-analysis-${browserName}-${Date.now()}`;
  const speaker = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "speaker",
    prepare: instrumentWebAudio,
  });
  const listener = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "listener",
    prepare: instrumentWebAudio,
  });

  try {
    await speaker.page.getByRole("button", { name: "Start microphone" }).click();
    const local = speaker.page.locator("#local-audio-track");
    await clickWithActivation(speaker.page.locator("#create-local-audio-analyser"));
    await expect(local).toHaveAttribute("data-analyser-active", "true");
    await expectAnalysis(
      local,
      speaker.page.locator("#read-local-audio-analyser"),
      true,
    );

    await closeAnalyserAndExpectCleanup(
      speaker.page,
      local,
      speaker.page.locator("#close-local-audio-analyser"),
    );

    await clickWithActivation(speaker.page.locator("#create-local-audio-analyser"));
    await expect(local).toHaveAttribute("data-analyser-active", "true");
    await dropAnalyserAndExpectCleanup(
      speaker.page,
      local,
      speaker.page.locator("#drop-local-audio-analyser"),
    );

    await clickWithActivation(speaker.page.locator("#create-local-audio-analyser"));
    await expect(local).toHaveAttribute("data-analyser-active", "true");
    await speaker.page.evaluate(() => {
      const prototype = globalThis.LivekitClient.LocalAudioTrack.prototype;
      const original = prototype.restartTrack;
      globalThis.__audioAnalysisRestart = { original, calls: 0 };
      prototype.restartTrack = async function (...args) {
        const result = await original.apply(this, args);
        globalThis.__audioAnalysisRestart.calls += 1;
        if (globalThis.__audioAnalysisRestart.calls === 1) {
          throw new Error("synthetic restart failure after source replacement");
        }
        return result;
      };
    });
    let before = await cleanupCalls(speaker.page);
    await speaker.page.getByRole("button", { name: "Restart microphone" }).click();
    await expect(speaker.page.getByRole("alert")).toContainText("synthetic restart failure");
    await expect(local).toHaveAttribute("data-analyser-active", "false");
    await expectOneCleanup(speaker.page, before);
    expect(await speaker.page.evaluate(() => globalThis.__audioAnalysisRestart.calls)).toBe(2);
    await speaker.page.evaluate(() => {
      globalThis.LivekitClient.LocalAudioTrack.prototype.restartTrack =
        globalThis.__audioAnalysisRestart.original;
    });

    await speaker.page.getByRole("button", { name: "Publish microphone" }).click();
    const remote = listener.page.locator(
      '.remote-media-publication[data-participant-identity="speaker"][data-kind="audio"]',
    );
    await expect(remote).toHaveAttribute("data-media-available", "true", { timeout: 20_000 });
    await clickWithActivation(remote.locator(".create-remote-audio-analyser"));
    await expect(remote).toHaveAttribute("data-analyser-active", "true");
    await expectAnalysis(
      remote,
      remote.locator(".read-remote-audio-analyser"),
      true,
    );

    await closeAnalyserAndExpectCleanup(
      listener.page,
      remote,
      remote.locator(".close-remote-audio-analyser"),
    );

    await clickWithActivation(remote.locator(".create-remote-audio-analyser"));
    await expect(remote).toHaveAttribute("data-analyser-active", "true");
    await dropAnalyserAndExpectCleanup(
      listener.page,
      remote,
      remote.locator(".drop-remote-audio-analyser"),
    );

    await clickWithActivation(remote.locator(".create-remote-audio-analyser"));
    await expect(remote).toHaveAttribute("data-analyser-active", "true");
    before = await cleanupCalls(listener.page);
    await remote.locator(".remote-subscription-button").dispatchEvent("click");
    await expect(remote).toHaveAttribute("data-media-available", "false");
    await expect(remote).toHaveAttribute("data-analyser-active", "false");
    await expectOneCleanup(listener.page, before);

    await clickWithActivation(speaker.page.locator("#create-local-audio-analyser"));
    await expect(local).toHaveAttribute("data-analyser-active", "true");
    before = await cleanupCalls(speaker.page);
    await speaker.page.getByRole("button", { name: "Stop microphone" }).click();
    await expect(local).toHaveAttribute("data-analyser-active", "false");
    await expectOneCleanup(speaker.page, before);
  } finally {
    await Promise.all([speaker.context.close(), listener.context.close()]);
  }
});
