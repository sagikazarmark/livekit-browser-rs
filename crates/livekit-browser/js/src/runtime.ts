import type { LiveKitClientSdk } from "./livekit-client";
import { localClipRecordingSupported } from "./recording";

type DynamicSdk = LiveKitClientSdk & Record<string, any>;

function requireConstructor(value: unknown, name: string): void {
  try {
    Reflect.construct(String, [], value as Function);
  } catch {
    throw new TypeError(`LivekitClient.${name} is not a constructor`);
  }
}

export function requireMethods(
  target: object | null | undefined,
  name: string,
  methods: readonly string[],
): void {
  for (const method of methods) {
    if (typeof (target as Record<string, unknown> | null | undefined)?.[method] !== "function") {
      const prefix = name.length > 0 ? `${name}.` : "";
      throw new TypeError(`LivekitClient.${prefix}${method} is not a function`);
    }
  }
}

function requireStringValues(
  target: Record<string, unknown> | null | undefined,
  name: string,
  values: readonly string[],
): void {
  for (const value of values) {
    if (typeof target?.[value] !== "string") {
      throw new TypeError(`LivekitClient.${name}.${value} is not a string`);
    }
  }
}

export function inspectBrowserRuntime(expectedVersion: string) {
  const sdk = globalThis.LivekitClient as DynamicSdk | undefined;
  if (!sdk) return { status: "sdkMissing" };

  const actualVersion = typeof sdk.version === "string" ? sdk.version : null;
  if (actualVersion !== expectedVersion) {
    return { status: "sdkVersionMismatch", actual_version: actualVersion };
  }

  for (const constructor of [
    "Room",
    "LocalParticipant",
    "LocalAudioTrack",
    "LocalVideoTrack",
    "LocalTrackPublication",
    "RemoteParticipant",
    "RemoteAudioTrack",
    "RemoteDataTrack",
    "RemoteVideoTrack",
    "RemoteTrackPublication",
    "RpcError",
  ]) {
    requireConstructor(sdk[constructor], constructor);
  }
  requireMethods(sdk, "", [
    "isBrowserSupported",
    "setLogExtension",
    "createLocalAudioTrack",
    "createLocalVideoTrack",
    "createLocalScreenTracks",
  ]);
  requireMethods(sdk.Room, "Room", ["getLocalDevices"]);
  requireMethods(sdk.Room.prototype, "Room", [
    "on",
    "off",
    "removeAllListeners",
    "getSid",
    "registerTextStreamHandler",
    "unregisterTextStreamHandler",
    "registerByteStreamHandler",
    "unregisterByteStreamHandler",
    "registerRpcMethod",
    "unregisterRpcMethod",
  ]);
  requireMethods(sdk.LocalParticipant.prototype, "LocalParticipant", [
    "publishTrack",
    "unpublishTrack",
    "setTrackSubscriptionPermissions",
    "setMetadata",
    "setAttributes",
    "publishData",
    "sendChatMessage",
    "streamText",
    "streamBytes",
    "performRpc",
  ]);
  for (const track of ["LocalAudioTrack", "LocalVideoTrack"]) {
    requireMethods(sdk[track].prototype, track, [
      "on",
      "off",
      "attach",
      "detach",
      "stop",
      "getDeviceId",
      "restartTrack",
      "mute",
      "unmute",
      "getSenderStats",
    ]);
  }
  for (const track of ["RemoteAudioTrack", "RemoteVideoTrack"]) {
    requireMethods(sdk[track].prototype, track, [
      "on",
      "off",
      "attach",
      "detach",
      "getReceiverStats",
    ]);
  }
  requireMethods(sdk.RemoteTrackPublication.prototype, "RemoteTrackPublication", ["setSubscribed"]);
  requireStringValues(sdk.TrackEvent, "TrackEvent", ["Ended"]);
  requireStringValues(sdk.RoomEvent, "RoomEvent", [
    "ConnectionStateChanged",
    "ParticipantMetadataChanged",
    "ParticipantAttributesChanged",
    "ParticipantPermissionsChanged",
    "ConnectionQualityChanged",
    "ActiveSpeakersChanged",
    "LocalTrackPublished",
    "LocalTrackUnpublished",
    "TrackMuted",
    "TrackUnmuted",
    "ParticipantConnected",
    "ParticipantDisconnected",
    "TrackPublished",
    "TrackUnpublished",
    "TrackSubscribed",
    "TrackUnsubscribed",
    "TrackSubscriptionFailed",
    "TrackSubscriptionStatusChanged",
    "DataTrackPublished",
    "DataTrackUnpublished",
    "DataReceived",
    "ChatMessage",
    "RoomMetadataChanged",
    "AudioPlaybackStatusChanged",
  ]);
  if (
    !Number.isInteger(sdk.DataPacket_Kind?.RELIABLE) ||
    !Number.isInteger(sdk.DataPacket_Kind?.LOSSY) ||
    Number(sdk.DataPacket_Kind.RELIABLE) === Number(sdk.DataPacket_Kind.LOSSY)
  ) {
    throw new TypeError("LivekitClient.DataPacket_Kind reliable and lossy values are invalid");
  }
  if (!sdk.isBrowserSupported()) return { status: "unsupportedBrowser" };

  const mediaDevices = globalThis.navigator?.mediaDevices;
  const mediaElement = globalThis.HTMLMediaElement?.prototype;
  return {
    status: "ready",
    capabilities: {
      web_rtc: true,
      secure_context: globalThis.isSecureContext === true,
      user_media: typeof mediaDevices?.getUserMedia === "function",
      display_capture: typeof mediaDevices?.getDisplayMedia === "function",
      audio_playback: typeof mediaElement?.play === "function",
      audio_output_selection: typeof mediaElement?.setSinkId === "function",
      transcription: typeof sdk.RoomEvent?.TranscriptionReceived === "string",
      local_clip_recording: localClipRecordingSupported(),
    },
  };
}
