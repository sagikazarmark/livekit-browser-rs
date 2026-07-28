import { defineConfig, type PlaywrightTestOptions } from "@playwright/test";

const baseURL = process.env.PLAYWRIGHT_BASE_URL ?? "http://127.0.0.1:8081";

// Media capture needs a secure context. `127.0.0.1` is one by definition, but the
// Dagger check serves the demo from a service hostname over plain HTTP, so Chromium
// has to be told to treat that origin as secure. `chrome-headless-shell`, which is
// what `browserName: "chromium"` picks by default, ignores that switch — hence the
// `channel` below, which selects the full build and its own headless mode.
const chromium: Pick<
  PlaywrightTestOptions,
  "browserName" | "channel" | "launchOptions"
> = {
  browserName: "chromium",
  channel: "chromium",
  launchOptions: {
    args: [
      "--use-fake-device-for-media-stream",
      "--use-fake-ui-for-media-stream",
      "--autoplay-policy=no-user-gesture-required",
      `--unsafely-treat-insecure-origin-as-secure=${new URL(baseURL).origin}`,
    ],
  },
};

const browserContractTests = [
  "runtime.spec.ts",
  "room-session.spec.ts",
  "snapshot-normalization.spec.ts",
  "local-video-track.spec.ts",
  "local-audio-track.spec.ts",
  "local-clip-recording.spec.ts",
  "media-devices.spec.ts",
  "screen-media.spec.ts",
  "remote-media.spec.ts",
  "remote-participants.spec.ts",
  "publisher-permissions.spec.ts",
  "participant-state.spec.ts",
  "data-packets.spec.ts",
  "data-streams.spec.ts",
  "rpc.spec.ts",
  "chat.spec.ts",
  "transcription.spec.ts",
  "media-health.spec.ts",
  "audio-analysis.spec.ts",
  "dioxus-integration.spec.ts",
  "demo-experience.spec.ts",
];

export default defineConfig({
  testDir: "./e2e",
  outputDir: "./build/playwright/test-results",
  timeout: 45_000,
  retries: 0,
  workers: 1,
  reporter: [
    ["line"],
    ["html", { open: "never", outputFolder: "./build/playwright/report" }],
  ],
  use: {
    baseURL,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [
    {
      name: "chromium",
      testIgnore: "mobile-compatibility.spec.ts",
      use: {
        ...chromium,
        permissions: ["camera", "microphone"],
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
      testMatch: "mobile-compatibility.spec.ts",
      use: {
        ...chromium,
        deviceScaleFactor: 2,
        hasTouch: true,
        isMobile: true,
        viewport: { width: 390, height: 844 },
      },
    },
    {
      name: "mobile-webkit",
      testMatch: "mobile-compatibility.spec.ts",
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
