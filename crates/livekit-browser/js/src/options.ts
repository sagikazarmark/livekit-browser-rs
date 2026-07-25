export type RawOptions = Record<string, any>;

export function cloneCaptureOptions<T>(options: T): T {
  if (options == null || typeof options !== "object") return options;
  return (Array.isArray(options) ? [...options] : { ...options }) as T;
}

export function rawOptions(raw: unknown, fallback: RawOptions = {}): RawOptions {
  const value = raw == null ? fallback : raw;
  if (typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError("raw LiveKit options must be a JavaScript object");
  }
  return { ...value };
}

export function cameraCaptureOptions(options: RawOptions, raw: unknown): RawOptions {
  const normalized = rawOptions(raw, options);
  if (options.deviceId != null) normalized.deviceId = cloneCaptureOptions(options.deviceId);
  if (options.facingMode != null) normalized.facingMode = options.facingMode;
  if (options.resolution != null) normalized.resolution = cloneCaptureOptions(options.resolution);
  if (options.frameRate != null) normalized.frameRate = options.frameRate;
  return normalized;
}

export function microphoneCaptureOptions(options: RawOptions, raw: unknown): RawOptions {
  const normalized = rawOptions(raw, options);
  for (const key of [
    "deviceId",
    "autoGainControl",
    "channelCount",
    "echoCancellation",
    "latency",
    "noiseSuppression",
    "sampleRate",
    "sampleSize",
  ]) {
    if (options[key] != null) normalized[key] = cloneCaptureOptions(options[key]);
  }
  return normalized;
}

export function screenCaptureOptions(options: RawOptions | null | undefined, raw: unknown): RawOptions {
  const audio = options?.audio === true;
  return {
    ...rawOptions(raw, options ?? {}),
    audio,
    systemAudio: audio ? "include" : "exclude",
  };
}

export const trackSources = Object.freeze({
  camera: { livekit: "camera", normalized: "camera" },
  microphone: { livekit: "microphone", normalized: "microphone" },
  screenShare: { livekit: "screen_share", normalized: "screenShare" },
  screen_share: { livekit: "screen_share", normalized: "screenShare" },
  screenShareAudio: { livekit: "screen_share_audio", normalized: "screenShareAudio" },
  screen_share_audio: { livekit: "screen_share_audio", normalized: "screenShareAudio" },
});

export function livekitTrackSource(source: string): string {
  const normalized = trackSources[source as keyof typeof trackSources];
  if (!normalized) throw new TypeError(`unsupported Local Media Track source: ${source}`);
  return normalized.livekit;
}
