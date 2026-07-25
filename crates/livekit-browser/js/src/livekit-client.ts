import type * as LiveKitClient from "livekit-client";

export type LiveKitClientSdk = typeof LiveKitClient;

declare global {
  // The host page owns SDK loading. The adapter only consumes its UMD global.
  var LivekitClient: LiveKitClientSdk;
  var webkitAudioContext: typeof AudioContext | undefined;

  interface Window {
    webkitAudioContext?: typeof AudioContext;
  }
}

export function sdk(): LiveKitClientSdk {
  return globalThis.LivekitClient;
}
