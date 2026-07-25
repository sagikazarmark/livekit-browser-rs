import { defineConfig } from "@playwright/test";

const browserContractTests = [
  "runtime.spec.mjs",
  "room-session.spec.mjs",
  "snapshot-normalization.spec.mjs",
  "local-video-track.spec.mjs",
  "local-audio-track.spec.mjs",
  "local-clip-recording.spec.mjs",
  "media-devices.spec.mjs",
  "screen-media.spec.mjs",
  "remote-media.spec.mjs",
  "remote-participants.spec.mjs",
  "publisher-permissions.spec.mjs",
  "participant-state.spec.mjs",
  "data-packets.spec.mjs",
  "data-streams.spec.mjs",
  "rpc.spec.mjs",
  "chat.spec.mjs",
  "transcription.spec.mjs",
  "media-health.spec.mjs",
  "audio-analysis.spec.mjs",
  "dioxus-integration.spec.mjs",
  "demo-experience.spec.mjs",
];

export default defineConfig({
  testDir: ".",
  timeout: 45_000,
  retries: 0,
  workers: 1,
  reporter: "line",
  use: {
    baseURL: process.env.BASE_URL || "http://127.0.0.1:8081",
  },
  projects: [
    {
      name: "chromium",
      testIgnore: "mobile-compatibility.spec.mjs",
      use: {
        browserName: "chromium",
        permissions: ["camera", "microphone"],
        launchOptions: {
          args: [
            "--use-fake-device-for-media-stream",
            "--use-fake-ui-for-media-stream",
            "--autoplay-policy=no-user-gesture-required",
          ],
        },
      },
    },
    {
      name: "firefox",
      testMatch: browserContractTests,
      use: {
        browserName: "firefox",
        launchOptions: {
          firefoxUserPrefs: {
            "media.navigator.permission.disabled": true,
            "media.navigator.streams.fake": true,
            "media.peerconnection.ice.obfuscate_host_addresses": false,
            "media.peerconnection.ice.loopback": true,
            "media.autoplay.default": 0,
            "media.autoplay.block-webaudio": false,
          },
        },
      },
    },
    {
      name: "webkit",
      testMatch: browserContractTests,
      use: { browserName: "webkit" },
    },
    {
      name: "mobile-chromium",
      testMatch: "mobile-compatibility.spec.mjs",
      use: {
        browserName: "chromium",
        deviceScaleFactor: 2,
        hasTouch: true,
        isMobile: true,
        viewport: { width: 390, height: 844 },
      },
    },
    {
      name: "mobile-webkit",
      testMatch: "mobile-compatibility.spec.mjs",
      use: {
        browserName: "webkit",
        deviceScaleFactor: 3,
        hasTouch: true,
        isMobile: true,
        viewport: { width: 390, height: 844 },
      },
    },
  ],
});
