import { expect } from "@playwright/test";

async function installDeterministicMedia(page, browserName) {
  if (browserName !== "webkit" && browserName !== "firefox") return;
  await page.evaluate(() => {
    const mediaDevices = navigator.mediaDevices;
    Object.defineProperty(navigator, "mediaDevices", {
      configurable: true,
      value: mediaDevices,
    });
    const originalGetUserMedia = mediaDevices.getUserMedia.bind(mediaDevices);
    const isWebKit = /AppleWebKit/.test(navigator.userAgent) && !/Chrome/.test(navigator.userAgent);
    let canvas;
    let videoStream;
    let animation;
    if (isWebKit) {
      canvas = document.createElement("canvas");
      canvas.width = 640;
      canvas.height = 360;
      const canvasContext = canvas.getContext("2d");
      canvasContext.fillStyle = "#d9ff43";
      canvasContext.fillRect(0, 0, canvas.width, canvas.height);
      videoStream = canvas.captureStream(30);
      let frame = 0;
      animation = setInterval(() => {
        canvasContext.fillStyle = frame++ % 2 === 0 ? "#d9ff43" : "#43d9ff";
        canvasContext.fillRect(0, 0, canvas.width, canvas.height);
      }, 100);
    }

    const AudioContext = window.AudioContext || window.webkitAudioContext;
    const audioContext = new AudioContext({ sampleRate: 48_000 });
    const oscillator = audioContext.createOscillator();
    const gain = audioContext.createGain();
    const destination = audioContext.createMediaStreamDestination();
    oscillator.frequency.value = 997;
    gain.gain.value = 0.35;
    oscillator.connect(gain).connect(destination);
    oscillator.start();
    void audioContext.resume();

    Object.defineProperty(mediaDevices, "getUserMedia", {
      configurable: true,
      writable: true,
      value: async (constraints) => {
        const tracks = [];
        if (constraints?.video) {
          if (videoStream) tracks.push(videoStream.getVideoTracks()[0].clone());
          else {
            const video = await originalGetUserMedia({ video: constraints.video });
            tracks.push(...video.getVideoTracks());
          }
        }
        if (constraints?.audio) tracks.push(destination.stream.getAudioTracks()[0].clone());
        return new MediaStream(tracks);
      },
    });
    window.__liveKitTestMedia = {
      canvas,
      videoStream,
      audioContext,
      oscillator,
      gain,
      destination,
      animation,
    };
  });
}

export async function openRoomParticipant({
  browser,
  browserName,
  baseURL,
  room,
  identity,
  prepare,
  participantToken,
}) {
  const context = await browser.newContext({
    baseURL,
    permissions: browserName === "chromium" ? ["camera", "microphone"] : undefined,
  });
  const page = await context.newPage();
  if (participantToken) {
    // Only the token is faked. The rest of the response has to stay whatever the
    // real endpoint returns: without its `serverUrl` the client falls back to
    // deriving a signaling URL from the page origin, which only reaches LiveKit
    // when something proxies it onto that origin.
    await page.route("**/api/token", async (route) => {
      const response = await route.fetch();
      const body = await response.json();
      await route.fulfill({
        response,
        contentType: "application/json",
        body: JSON.stringify({ ...body, participantToken }),
      });
    });
  }
  await page.goto("/");
  await installDeterministicMedia(page, browserName);
  await prepare?.(page);
  await page.getByLabel("Room name").fill(room);
  await page.getByLabel("Your identity").fill(identity);
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  return { context, page };
}
