function requireConstructor(value, name) {
  try {
    Reflect.construct(String, [], value);
  } catch {
    throw new TypeError(`LivekitClient.${name} is not a constructor`);
  }
}

function requireMethods(target, name, methods) {
  for (const method of methods) {
    if (typeof target?.[method] !== "function") {
      const prefix = name.length > 0 ? `${name}.` : "";
      throw new TypeError(`LivekitClient.${prefix}${method} is not a function`);
    }
  }
}

function requireStringValues(target, name, values) {
  for (const value of values) {
    if (typeof target?.[value] !== "string") {
      throw new TypeError(`LivekitClient.${name}.${value} is not a string`);
    }
  }
}

export function inspectBrowserRuntime(expectedVersion) {
  const sdk = globalThis.LivekitClient;
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
    sdk.DataPacket_Kind.RELIABLE === sdk.DataPacket_Kind.LOSSY
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

let logExtensionInstalled = false;

function normalizeLogContext(value, seen = new WeakSet()) {
  if (value == null || typeof value === "string" || typeof value === "boolean") return value;
  if (typeof value === "number") return Number.isFinite(value) ? value : String(value);
  if (typeof value === "bigint" || typeof value === "symbol" || typeof value === "function") {
    return String(value);
  }
  if (seen.has(value)) return "[Circular]";
  seen.add(value);
  try {
    if (Array.isArray(value)) return value.map((entry) => normalizeLogContext(entry, seen));
    const normalized = {};
    if (value instanceof Error) {
      normalized.name = value.name;
      normalized.message = value.message;
    }
    for (const key of Object.keys(value).sort()) {
      try {
        normalized[key] = normalizeLogContext(value[key], seen);
      } catch {
        normalized[key] = "[Unserializable]";
      }
    }
    return normalized;
  } finally {
    seen.delete(value);
  }
}

export function installLogExtension(onLog) {
  if (logExtensionInstalled) return;
  const sdk = globalThis.LivekitClient;
  if (typeof sdk?.setLogExtension !== "function") {
    throw new TypeError("LivekitClient.setLogExtension is not a function");
  }
  sdk.setLogExtension((level, message, context) => {
    if (!Number.isInteger(level) || level < 0 || level > 4) return;
    try {
      onLog(level, String(message), normalizeLogContext(context ?? {}));
    } catch (error) {
      // Diagnostics must never interrupt LiveKit runtime behavior, but callback failures remain
      // visible through the SDK's existing console diagnostics.
      console.error("LiveKit Rust log callback failed", error);
    }
  });
  logExtensionInstalled = true;
}

const normalizedDeviceKind = (kind) => {
  switch (kind) {
    case "videoinput":
      return "camera";
    case "audioinput":
      return "microphone";
    default:
      throw new TypeError(`unsupported media input kind: ${kind}`);
  }
};

export async function enumerateMediaDevices(kind, requestPermissions) {
  const devices = await globalThis.LivekitClient.Room.getLocalDevices(kind, requestPermissions);
  return devices.map((device) => ({
    device_id: device.deviceId || "",
    group_id: device.groupId || "",
    label: device.label || "",
    kind: normalizedDeviceKind(device.kind),
  }));
}

let nextRoomId = 1;
const roomSessions = new Map();
let nextLocalVideoTrackId = 1;
let nextAttachmentId = 1;
const localVideoTracks = new Map();
let nextLocalAudioTrackId = 1;
const localAudioTracks = new Map();
let nextRemoteMediaTrackId = 1;
const remoteMediaTracks = new Map();
let nextAudioAnalyserId = 1;
const audioAnalysers = new Map();
let nextRemoteObjectGeneration = 1;
const remoteParticipantGenerations = new WeakMap();
const remotePublicationGenerations = new WeakMap();
const remoteMediaGenerations = new WeakMap();
let nextOutgoingDataStreamId = 1;
const outgoingDataStreams = new Map();
let nextIncomingDataStreamId = 1;
const incomingDataStreams = new Map();
let nextDataStreamHandlerId = 1;
const dataStreamHandlers = new Map();
let nextParticipantRpcMethodId = 1;
const participantRpcMethods = new Map();
let nextParticipantRpcInvocationId = 1;
const participantRpcInvocations = new Map();

function remoteParticipantGeneration(participant) {
  let state = remoteParticipantGenerations.get(participant);
  if (state?.identity !== participant.identity || state?.sid !== participant.sid) {
    state = {
      generation: nextRemoteObjectGeneration++,
      identity: participant.identity,
      sid: participant.sid,
    };
    remoteParticipantGenerations.set(participant, state);
  }
  return state.generation;
}

function remotePublicationGeneration(
  publication,
  sid = publication.trackSid ?? publication.info?.sid,
) {
  let state = remotePublicationGenerations.get(publication);
  if (!state || state.sid !== sid) {
    state = { generation: nextRemoteObjectGeneration++, sid };
    remotePublicationGenerations.set(publication, state);
  }
  return state.generation;
}

function remoteMediaGeneration(track) {
  const source = track.mediaStreamTrack ?? track;
  let state = remoteMediaGenerations.get(track);
  if (state?.source !== source) {
    state = { generation: nextRemoteObjectGeneration++, source };
    remoteMediaGenerations.set(track, state);
  }
  return state.generation;
}

function localVideoTrackRecord(trackId) {
  const record = localVideoTracks.get(trackId);
  if (!record) throw new Error("Local Video Track is no longer available");
  return record;
}

function enqueueLocalTrackMutation(record, operation) {
  const result = record.mutationTail.then(operation, operation);
  record.mutationTail = result.catch(() => {});
  return result;
}

function cloneCaptureOptions(options) {
  if (options == null || typeof options !== "object") return options;
  return Array.isArray(options) ? [...options] : { ...options };
}

function rawOptions(raw, fallback = {}) {
  const value = raw == null ? fallback : raw;
  if (typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError("raw LiveKit options must be a JavaScript object");
  }
  return { ...value };
}

function cameraCaptureOptions(options, raw) {
  const normalized = rawOptions(raw, options);
  if (options.deviceId != null) normalized.deviceId = cloneCaptureOptions(options.deviceId);
  if (options.facingMode != null) normalized.facingMode = options.facingMode;
  if (options.resolution != null) normalized.resolution = cloneCaptureOptions(options.resolution);
  if (options.frameRate != null) normalized.frameRate = options.frameRate;
  return normalized;
}

function microphoneCaptureOptions(options, raw) {
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

function screenCaptureOptions(options, raw) {
  const audio = options?.audio === true;
  return {
    ...rawOptions(raw, options),
    audio,
    systemAudio: audio ? "include" : "exclude",
  };
}

const trackSources = Object.freeze({
  camera: { livekit: "camera", normalized: "camera" },
  microphone: { livekit: "microphone", normalized: "microphone" },
  screenShare: { livekit: "screen_share", normalized: "screenShare" },
  screen_share: { livekit: "screen_share", normalized: "screenShare" },
  screenShareAudio: { livekit: "screen_share_audio", normalized: "screenShareAudio" },
  screen_share_audio: { livekit: "screen_share_audio", normalized: "screenShareAudio" },
});

const livekitTrackSource = (source) => {
  const normalized = trackSources[source];
  if (!normalized) throw new TypeError(`unsupported Local Media Track source: ${source}`);
  return normalized.livekit;
};

function registerLocalVideoTrack(track, captureOptions, source) {
  const id = nextLocalVideoTrackId++;
  localVideoTracks.set(id, {
    id,
    track,
    mediaKind: "video",
    source,
    captureOptions,
    captureStopped: false,
    stopRequested: false,
    stopOperation: null,
    ownerDropped: false,
    attachments: new Map(),
    publishedRoomId: null,
    pendingPublishRoomId: null,
    publicationId: null,
    pendingPublicationId: null,
    publication: null,
    publicationReplacementCallback: null,
    awaitingPublicationReplacement: null,
    explicitUnpublishId: null,
    explicitUnpublishObservedId: null,
    mutationTail: Promise.resolve(),
    ended: false,
    endedCallback: null,
    endedListener: null,
    browserEndedPeer: null,
    endedCleanupComplete: false,
    endedCleanupRunning: false,
  });
  return id;
}

function registerLocalAudioTrack(track, captureOptions, source) {
  const id = nextLocalAudioTrackId++;
  localAudioTracks.set(id, {
    id,
    track,
    mediaKind: "audio",
    source,
    captureOptions,
    captureStopped: false,
    stopRequested: false,
    stopOperation: null,
    ownerDropped: false,
    attachments: new Map(),
    audioAnalyserIds: new Set(),
    publishedRoomId: null,
    pendingPublishRoomId: null,
    publicationId: null,
    pendingPublicationId: null,
    publication: null,
    publicationReplacementCallback: null,
    awaitingPublicationReplacement: null,
    explicitUnpublishId: null,
    explicitUnpublishObservedId: null,
    mutationTail: Promise.resolve(),
    ended: false,
    endedCallback: null,
    endedListener: null,
    browserEndedPeer: null,
    endedCleanupComplete: false,
    endedCleanupRunning: false,
  });
  return id;
}

function removeLocalTrackEndedListener(record) {
  if (record.endedListener) {
    record.track.off(globalThis.LivekitClient.TrackEvent.Ended, record.endedListener);
    record.endedListener = null;
  }
  record.endedCallback = null;
  record.browserEndedPeer = null;
}

function finishBrowserEnded(record, sourceEnded) {
  if (record.ended) return;
  record.ended = true;
  record.stopRequested = true;
  record.captureStopped = sourceEnded;
  if (record.mediaKind === "audio") terminateAudioAnalysers(record);
  if (record.attachments) {
    for (const attachmentId of [...record.attachments.keys()]) {
      try {
        if (record.mediaKind === "video") detachLocalVideoElement(record, attachmentId);
        else detachLocalAudioElement(record, attachmentId);
      } catch {}
    }
  }
  if (record.endedListener) {
    record.track.off(globalThis.LivekitClient.TrackEvent.Ended, record.endedListener);
    record.endedListener = null;
  }
  const callback = record.endedCallback;
  record.endedCallback = null;
  callback?.();
  const peer = record.browserEndedPeer;
  record.browserEndedPeer = null;
  if (peer) finishBrowserEnded(peer, false);
  finishBrowserEndedCleanup(record);
}

function deleteFinishedBrowserEndedRecord(record) {
  if (
    !record.ownerDropped ||
    !record.endedCleanupComplete ||
    record.publication ||
    record.publishedRoomId != null ||
    record.pendingPublishRoomId != null ||
    record.attachments.size > 0
  ) {
    return;
  }
  const records = record.mediaKind === "video" ? localVideoTracks : localAudioTracks;
  records.delete(record.id);
}

function finishBrowserEndedCleanup(record, retryDelay = 0) {
  if (record.endedCleanupRunning || record.endedCleanupComplete) return;
  record.endedCleanupRunning = true;
  const operation = enqueueLocalTrackMutation(record, async () => {
    let failure = null;
    const room = roomSessions.get(record.publishedRoomId);
    if (record.publication && room && !room.released) {
      try {
        await room.room.localParticipant.unpublishTrack(record.track, false);
        record.publication = null;
        record.publishedRoomId = null;
        record.publicationId = null;
      } catch (error) {
        failure = error;
      }
    } else if (!room || room.released) {
      record.publication = null;
      record.publishedRoomId = null;
      record.publicationId = null;
    }
    try {
      if (record.mediaKind === "video") stopLocalVideoRecord(record);
      else stopLocalAudioRecord(record);
    } catch (error) {
      failure ??= error;
    }
    if (failure) throw failure;
  });
  record.stopOperation = operation;
  void operation.then(
    () => {
      if (record.stopOperation === operation) record.stopOperation = null;
      record.endedCleanupRunning = false;
      record.endedCleanupComplete = true;
      deleteFinishedBrowserEndedRecord(record);
    },
    () => {
      if (record.stopOperation === operation) record.stopOperation = null;
      record.endedCleanupRunning = false;
      const nextDelay = Math.min(Math.max(retryDelay * 2, 10), 1_000);
      setTimeout(() => finishBrowserEndedCleanup(record, nextDelay), nextDelay);
    },
  );
}

function installLocalTrackEndedListener(record, onEnded) {
  const listener = () => finishBrowserEnded(record, true);
  record.endedListener = listener;
  record.track.on(globalThis.LivekitClient.TrackEvent.Ended, listener);
  if (onEnded) record.endedCallback = onEnded;
}

export function observeLocalVideoTrackEnded(trackId, onEnded) {
  const record = localVideoTrackRecord(trackId);
  if (record.ended) queueMicrotask(onEnded);
  else record.endedCallback = onEnded;
}

export function observeLocalAudioTrackEnded(trackId, onEnded) {
  const record = localAudioTrackRecord(trackId);
  if (record.ended) queueMicrotask(onEnded);
  else record.endedCallback = onEnded;
}

export function createLocalClipMediaStream(audioTrackIds, videoTrackIds) {
  const source = (records, trackId, expectedKind) => {
    const record = records.get(trackId);
    if (!record || record.mediaKind !== expectedKind) {
      throw new Error(`Local ${expectedKind} Track is no longer available`);
    }
    if (record.stopRequested || record.captureStopped || record.ended) {
      throw new Error(`Local ${expectedKind} Track has stopped`);
    }
    const mediaStreamTrack = record.track?.mediaStreamTrack;
    if (!(mediaStreamTrack instanceof MediaStreamTrack) || mediaStreamTrack.readyState !== "live") {
      throw new TypeError(`Local ${expectedKind} Track has no live browser source`);
    }
    return mediaStreamTrack;
  };

  return new MediaStream([
    ...audioTrackIds.map((trackId) => source(localAudioTracks, trackId, "audio")),
    ...videoTrackIds.map((trackId) => source(localVideoTracks, trackId, "video")),
  ]);
}

async function restartVideoTrack(record, options) {
  const restartOptions = cloneCaptureOptions(options);
  delete restartOptions.resolution;
  delete restartOptions.frameRate;
  await record.track.restartTrack(restartOptions);

  const constraints = {};
  if (options.resolution?.width != null) {
    constraints.width = { ideal: options.resolution.width };
  }
  if (options.resolution?.height != null) {
    constraints.height = { ideal: options.resolution.height };
  }
  if (options.frameRate != null) constraints.frameRate = { ideal: options.frameRate };
  if (Object.keys(constraints).length > 0) {
    await record.track.mediaStreamTrack.applyConstraints(constraints);
  }

  // LiveKit uses this state when camera mute/unmute reacquires the browser source.
  record.track._constraints = { ...record.track._constraints, ...constraints };
}

function restartLocalTrack({
  record,
  nextOptions,
  normalizeOptions,
  performRestart,
  terminate,
  onComplete,
}) {
  let becameTerminal = false;
  let operation;
  try {
    if (typeof record.track.restartTrack !== "function") {
      throw new TypeError("LiveKit Local Track does not support restart");
    }
    operation = enqueueLocalTrackMutation(record, async () => {
      if (record.stopRequested) throw new Error("Local Media Track capture has stopped");
      const previousOptions = cloneCaptureOptions(record.captureOptions);
      const requestedOptions = normalizeOptions(nextOptions(previousOptions));
      const previousSource = record.mediaKind === "audio" ? record.track.mediaStreamTrack : null;
      try {
        await performRestart(record, cloneCaptureOptions(requestedOptions));
        record.captureOptions = requestedOptions;
      } catch (error) {
        try {
          await performRestart(record, cloneCaptureOptions(previousOptions));
        } catch {
          becameTerminal = true;
          terminate(record);
        }
        throw error;
      } finally {
        if (previousSource && record.track.mediaStreamTrack !== previousSource) {
          terminateAudioAnalysers(record);
        }
      }
    });
  } catch (error) {
    operation = Promise.reject(error);
  }

  return operation.then(
    (value) => {
      onComplete(false);
      return value;
    },
    (error) => {
      onComplete(becameTerminal);
      throw error;
    },
  );
}

function detachLocalVideoElement(record, attachmentId) {
  const element = record.attachments.get(attachmentId);
  if (!element) return;
  const clearIfStillDetached = () => {
    if (![...record.attachments.values()].includes(element)) element.srcObject = null;
  };
  try {
    record.track.detach(element);
  } finally {
    record.attachments.delete(attachmentId);
    clearIfStillDetached();
    queueMicrotask(clearIfStillDetached);
    setTimeout(clearIfStillDetached, 0);
  }
}

function detachLocalAudioElement(record, attachmentId) {
  const element = record.attachments.get(attachmentId);
  if (!element) return;
  const clearIfStillDetached = () => {
    if (![...record.attachments.values()].includes(element)) element.srcObject = null;
  };
  try {
    record.track.detach(element);
  } finally {
    record.attachments.delete(attachmentId);
    clearIfStillDetached();
    queueMicrotask(clearIfStillDetached);
    setTimeout(clearIfStillDetached, 0);
  }
}

function remoteMediaTrackRecord(trackId) {
  const record = remoteMediaTracks.get(trackId);
  if (!record || record.terminal) throw new Error("Remote Media Track is no longer available");
  return record;
}

function currentRemotePublication(roomRecord, locator) {
  const participant = currentRemoteParticipant(roomRecord, locator.participant);
  if (!participant) return null;
  const publication = participant.trackPublications.get(locator.sid);
  if (!publication || remotePublicationGeneration(publication) !== locator.generation) return null;
  return { participant, publication };
}

function currentRemoteParticipant(roomRecord, locator) {
  const participant = roomRecord.room.remoteParticipants.get(locator.identity);
  if (
    !participant ||
    participant.sid !== locator.sid ||
    remoteParticipantGeneration(participant) !== locator.generation
  ) {
    return null;
  }
  return participant;
}

export function rawRoom(roomId) {
  const record = roomSessions.get(roomId);
  if (!record || record.released) throw new Error("Room Session is no longer available");
  return record.room;
}

export function rawLocalParticipant(roomId) {
  return rawRoom(roomId).localParticipant;
}

export function rawRemoteParticipant(roomId, locator) {
  const record = roomSessions.get(roomId);
  if (!record || record.released) throw new Error("Room Session is no longer available");
  const participant = currentRemoteParticipant(record, locator);
  if (!participant) throw new Error("Remote Participant is no longer available");
  return participant;
}

export function rawRemotePublication(roomId, locator) {
  const record = roomSessions.get(roomId);
  if (!record || record.released) throw new Error("Room Session is no longer available");
  const current = currentRemotePublication(record, locator);
  if (!current) {
    throw new Error("Remote Track Publication is no longer available");
  }
  return current.publication;
}

function localMediaTrackResources(kind) {
  if (kind === "video") return { records: localVideoTracks, recordFor: localVideoTrackRecord };
  if (kind === "audio") return { records: localAudioTracks, recordFor: localAudioTrackRecord };
  throw new TypeError(`unsupported Local Media Track kind: ${kind}`);
}

function localMediaTrackRecord(trackId, kind) {
  return localMediaTrackResources(kind).recordFor(trackId);
}

export function rawLocalMediaTrack(trackId, kind) {
  const record = localMediaTrackRecord(trackId, kind);
  if (record.stopRequested || record.captureStopped || record.ended) {
    throw new Error("Local Media Track is no longer available");
  }
  return record.track;
}

export function rawLocalPublication(trackId, publicationId, kind) {
  const record = localMediaTrackRecord(trackId, kind);
  if (record.publicationId !== publicationId || !record.publication) {
    throw new Error("Local Track Publication is no longer available");
  }
  return record.publication;
}

export function rawRemoteMediaTrack(trackId) {
  const record = remoteMediaTrackRecord(trackId);
  if (!remoteMediaTrackIsCurrent(record)) {
    terminateRemoteMediaTrack(record);
    throw new Error("Remote Media Track is no longer available");
  }
  return record.track;
}

export function setLocalMediaTrackProcessor(trackId, kind, processor, showProcessedStreamLocally) {
  const record = localMediaTrackRecord(trackId, kind);
  if (record.stopRequested || record.captureStopped || record.ended) {
    return Promise.reject(new Error("Local Media Track is no longer available"));
  }
  if (typeof record.track.setProcessor !== "function") {
    return Promise.reject(new TypeError("LiveKit Local Media Track does not support processors"));
  }
  return enqueueLocalTrackMutation(record, async () => {
    if (record.stopRequested || record.captureStopped || record.ended) {
      throw new Error("Local Media Track is no longer available");
    }
    await record.track.setProcessor(processor, showProcessedStreamLocally);
    if (kind === "audio") terminateAudioAnalysers(record);
  });
}

export function rawLocalMediaTrackProcessor(trackId, kind) {
  const record = localMediaTrackRecord(trackId, kind);
  if (record.stopRequested || record.captureStopped || record.ended) {
    throw new Error("Local Media Track is no longer available");
  }
  if (typeof record.track.getProcessor !== "function") {
    throw new TypeError("LiveKit Local Media Track does not support processors");
  }
  return record.track.getProcessor() ?? null;
}

export function stopLocalMediaTrackProcessor(trackId, kind) {
  const record = localMediaTrackRecord(trackId, kind);
  if (typeof record.track.stopProcessor !== "function") {
    return Promise.reject(new TypeError("LiveKit Local Media Track does not support processors"));
  }
  return enqueueLocalTrackMutation(record, async () => {
    if (record.stopRequested || record.captureStopped || record.ended) {
      throw new Error("Local Media Track is no longer available");
    }
    await record.track.stopProcessor(false);
    if (kind === "audio") terminateAudioAnalysers(record);
  });
}

function clearDetachedRemoteElement(element) {
  if (!hasRemoteElementAttachment(null, element)) element.srcObject = null;
}

function hasRemoteElementAttachment(track, element) {
  for (const candidate of remoteMediaTracks.values()) {
    if (
      (!track || candidate.track === track) &&
      [...candidate.attachments.values()].includes(element)
    ) {
      return true;
    }
  }
  return false;
}

function detachRemoteElement(record, attachmentId) {
  const element = record.attachments.get(attachmentId);
  if (!element) return;
  const clearIfStillDetached = () => clearDetachedRemoteElement(element);
  record.attachments.delete(attachmentId);
  try {
    if (!hasRemoteElementAttachment(record.track, element)) record.track.detach(element);
  } finally {
    clearIfStillDetached();
    queueMicrotask(clearIfStillDetached);
    setTimeout(clearIfStillDetached, 0);
  }
}

function terminateRemoteMediaTrack(record) {
  if (record.terminal) return;
  record.terminal = true;
  terminateAudioAnalysers(record);
  for (const attachmentId of [...record.attachments.keys()]) {
    try {
      detachRemoteElement(record, attachmentId);
    } catch {}
  }
  if (record.endedListener) {
    record.track.off(globalThis.LivekitClient.TrackEvent.Ended, record.endedListener);
    record.endedListener = null;
  }
  const callback = record.terminalCallback;
  record.terminalCallback = null;
  remoteMediaTracks.delete(record.id);
  roomSessions.get(record.roomId)?.remoteMediaTrackIds.delete(record.id);
  callback?.();
}

function terminateRemoteMediaWhere(predicate) {
  for (const record of remoteMediaTracks.values()) {
    if (predicate(record)) terminateRemoteMediaTrack(record);
  }
}

function reconcileRemoteMediaTracks(roomRecord) {
  for (const trackId of roomRecord.remoteMediaTrackIds) {
    const record = remoteMediaTracks.get(trackId);
    if (!record || record.terminal) continue;
    if (!remoteMediaTrackIsCurrent(record)) terminateRemoteMediaTrack(record);
  }
}

function remoteMediaTrackIsCurrent(record) {
  const roomRecord = roomSessions.get(record.roomId);
  if (!roomRecord || roomRecord.released) return false;
  const publication = currentRemotePublication(roomRecord, record.locator);
  return (
    publication?.participant === record.participant &&
    publication?.publication === record.publication &&
    publication.publication.track === record.track &&
    remoteMediaGeneration(record.track) === record.mediaGeneration
  );
}

export function setRemoteTrackSubscription(roomId, locator, subscribed) {
  const roomRecord = roomSessions.get(roomId);
  if (!roomRecord || roomRecord.released) throw new Error("Room Session is no longer available");
  const current = currentRemotePublication(roomRecord, locator);
  if (!current) throw new Error("Remote Track Publication is no longer available");
  current.publication.setSubscribed(subscribed);
}

export function setTrackSubscriptionPermissions(roomId, permissions) {
  const roomRecord = roomSessions.get(roomId);
  if (!roomRecord || roomRecord.released) throw new Error("Room Session is no longer available");
  const participant = roomRecord.room.localParticipant;
  if (participant.permissions?.canPublish !== true) {
    const error = new Error("LiveKit source grants do not permit publishing");
    error.name = "PermissionDeniedError";
    error.status = 403;
    throw error;
  }

  const participantPermissions = (permissions?.participantIdentities || []).map(
    (participantIdentity) => ({ participantIdentity, allowAll: true }),
  );
  participant.setTrackSubscriptionPermissions(
    permissions?.allParticipantsAllowed === true,
    participantPermissions,
  );
}

function enqueueLocalParticipantMutation(roomRecord, mutation) {
  const operation = roomRecord.localParticipantMutation.then(() => {
    if (roomRecord.released) throw new Error("Room Session is no longer available");
    return mutation(roomRecord.room.localParticipant);
  });
  roomRecord.localParticipantMutation = operation.catch(() => {});
  return operation;
}

function requireLocalParticipantMetadataPermission(participant) {
  if (participant.permissions?.canUpdateMetadata === true) return;
  const error = new Error("LiveKit source grants do not permit participant metadata updates");
  error.name = "PermissionDeniedError";
  error.status = 403;
  throw error;
}

export function setLocalParticipantMetadata(roomId, metadata) {
  const roomRecord = roomSessions.get(roomId);
  if (!roomRecord || roomRecord.released) throw new Error("Room Session is no longer available");
  return enqueueLocalParticipantMutation(roomRecord, (participant) => {
    requireLocalParticipantMetadataPermission(participant);
    if (metadata.length === 0) {
      const error = new Error("LiveKit participant metadata cannot be cleared with an empty string");
      error.name = "InvalidMetadataError";
      throw error;
    }
    return participant.setMetadata(metadata);
  });
}

export function updateLocalParticipantAttributes(roomId, attributes) {
  const roomRecord = roomSessions.get(roomId);
  if (!roomRecord || roomRecord.released) throw new Error("Room Session is no longer available");
  return enqueueLocalParticipantMutation(roomRecord, (participant) => {
    requireLocalParticipantMetadataPermission(participant);
    return participant.setAttributes(attributes);
  });
}

function requireDataPublishing(participant) {
  if (participant.permissions?.canPublishData === false) {
    const error = new Error("LiveKit source grants do not permit publishing data");
    error.name = "PermissionDeniedError";
    error.status = 403;
    throw error;
  }
}

function enqueueReliableDataMutation(roomRecord, mutation) {
  const operation = roomRecord.reliableDataMutation.then(mutation, mutation);
  roomRecord.reliableDataMutation = operation.catch(() => {});
  return operation;
}

export function sendDataPacket(roomId, payload, reliable) {
  const roomRecord = roomSessions.get(roomId);
  if (!roomRecord || roomRecord.released) throw new Error("Room Session is no longer available");
  const participant = roomRecord.room.localParticipant;
  requireDataPublishing(participant);
  const send = () => participant.publishData(Uint8Array.from(payload), { reliable });
  return reliable
    ? enqueueBoundedReliableDataMutation(
        roomRecord,
        send,
        "LiveKit did not send the reliable data packet",
      )
    : send();
}

function normalizedChatTimestamp(timestamp) {
  const value = Number(timestamp);
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new TypeError("LiveKit chat timestamp is not a non-negative safe integer");
  }
  return value;
}

function normalizeChatMessage(message, senderIdentity) {
  if (typeof message?.id !== "string" || message.id.length === 0) {
    throw new TypeError("LiveKit chat message ID is missing");
  }
  if (typeof message.message !== "string") {
    throw new TypeError("LiveKit chat content is not a string");
  }
  return {
    id: message.id,
    message: message.message,
    sender_identity: typeof senderIdentity === "string" ? senderIdentity : null,
    timestamp_millis: normalizedChatTimestamp(message?.timestamp),
    edit_timestamp_millis:
      message?.editTimestamp == null ? null : normalizedChatTimestamp(message.editTimestamp),
  };
}

function normalizedTranscriptionTime(value, name) {
  const normalized = Number(value);
  if (!Number.isSafeInteger(normalized) || normalized < 0) {
    throw new TypeError(`LiveKit transcription ${name} is not a non-negative safe integer`);
  }
  return normalized;
}

function transcriptionSourceIsCurrent(record, participant, publication) {
  if (participant) {
    const currentParticipant =
      participant === record.room.localParticipant
        ? record.room.localParticipant
        : record.room.remoteParticipants.get(participant.identity);
    if (currentParticipant !== participant) return false;
  }
  if (publication) {
    if (typeof publication.trackSid !== "string") return false;
    const participants = [record.room.localParticipant, ...record.room.remoteParticipants.values()];
    const owner = participants.find(
      (candidate) => candidate.trackPublications.get(publication.trackSid) === publication,
    );
    if (!owner || (participant && owner !== participant)) return false;
  }
  return true;
}

function normalizeTranscriptionSegment(segment, participant, publication) {
  if (typeof segment?.id !== "string" || segment.id.length === 0) {
    throw new TypeError("LiveKit transcription segment ID is missing");
  }
  if (typeof segment.text !== "string" || typeof segment.language !== "string") {
    throw new TypeError("LiveKit transcription segment text or language is not a string");
  }
  if (typeof segment.final !== "boolean") {
    throw new TypeError("LiveKit transcription segment final state is not a boolean");
  }
  return {
    id: segment.id,
    participant_identity: typeof participant?.identity === "string" ? participant.identity : null,
    track_publication_sid:
      typeof publication?.trackSid === "string" ? publication.trackSid : null,
    text: segment.text,
    start_time: normalizedTranscriptionTime(segment.startTime, "start time"),
    end_time: normalizedTranscriptionTime(segment.endTime, "end time"),
    language: segment.language,
    is_final: segment.final,
    first_received_at_millis: normalizedTranscriptionTime(
      segment.firstReceivedTime,
      "first received time",
    ),
    last_received_at_millis: normalizedTranscriptionTime(
      segment.lastReceivedTime,
      "last received time",
    ),
  };
}

export function sendChatMessage(roomId, message) {
  const roomRecord = roomSessions.get(roomId);
  if (!roomRecord || roomRecord.released) throw new Error("Room Session is no longer available");
  const participant = roomRecord.room.localParticipant;
  requireDataPublishing(participant);
  return enqueueBoundedReliableDataMutation(
    roomRecord,
    async () => {
      const sent = await participant.sendChatMessage(message);
      return normalizeChatMessage(sent, participant.identity);
    },
    "LiveKit did not send the Chat Message",
  );
}

function normalizedStreamSize(size) {
  const normalized = Number(size);
  return Number.isSafeInteger(normalized) && normalized >= 0 ? normalized : null;
}

const FLOW_TEXT_TOPIC = "~livekit-browser/flow/v1/text";
const FLOW_BYTES_TOPIC = "~livekit-browser/flow/v1/bytes";
const FLOW_CONTROL_TOPIC = "~livekit-browser/flow/v1/control";
const FLOW_APPLICATION_TOPIC_ATTRIBUTE = "~livekit-browser.topic";
const FLOW_PROTOCOL_ATTRIBUTE = "~livekit-browser.flow";
const FLOW_PROTOCOL_VERSION = "1";
const MAX_ACTIVE_INCOMING_DATA_STREAMS = 16;
const FLOW_OPEN_TIMEOUT_MS = 10_000;
const FLOW_SDK_OPERATION_TIMEOUT_MS = 30_000;
const flowControlEncoder = new TextEncoder();
const flowControlDecoder = new TextDecoder();

function flowControlError(name, message, reasonName = name.replace(/Error$/, "")) {
  const error = new Error(message);
  error.name = name;
  error.reasonName = reasonName;
  return error;
}

function enqueueBoundedReliableDataMutation(roomRecord, mutation, timeoutMessage) {
  return enqueueReliableDataMutation(roomRecord, () => {
    let timer;
    let operation;
    try {
      operation = Promise.resolve(mutation());
    } catch (error) {
      operation = Promise.reject(error);
    }
    const timeout = new Promise((_, reject) => {
      timer = setTimeout(
        () => reject(flowControlError("FlowControlTimeoutError", timeoutMessage)),
        FLOW_SDK_OPERATION_TIMEOUT_MS,
      );
    });
    return Promise.race([operation, timeout]).finally(() => clearTimeout(timer));
  });
}

function flowWireTopic(kind) {
  return kind === "text" ? FLOW_TEXT_TOPIC : FLOW_BYTES_TOPIC;
}

function nativeStreamOptions(options, kind) {
  return {
    topic: flowWireTopic(kind),
    totalSize: options?.totalSize,
    destinationIdentities: [options.destinationIdentity],
    attributes: {
      [FLOW_PROTOCOL_ATTRIBUTE]: FLOW_PROTOCOL_VERSION,
      [FLOW_APPLICATION_TOPIC_ATTRIBUTE]: options.topic,
    },
  };
}

function sendFlowControl(roomRecord, destinationIdentity, message) {
  if (roomRecord.released) return Promise.reject(terminalDataStreamError());
  const payload = flowControlEncoder.encode(JSON.stringify({ v: 1, ...message }));
  return enqueueBoundedReliableDataMutation(
    roomRecord,
    () =>
      roomRecord.room.localParticipant.publishData(payload, {
        reliable: true,
        topic: FLOW_CONTROL_TOPIC,
        destinationIdentities: [destinationIdentity],
      }),
    "LiveKit did not send Data Stream flow control",
  );
}

function parseFlowControl(payload) {
  try {
    const message = JSON.parse(flowControlDecoder.decode(payload));
    if (message?.v !== 1 || typeof message.streamId !== "string") return null;
    return message;
  } catch {
    return null;
  }
}

function normalizeStreamInfo(info, senderIdentity) {
  return {
    id: typeof info?.id === "string" ? info.id : "",
    topic: typeof info?.topic === "string" ? info.topic : "",
    mime_type: typeof info?.mimeType === "string" ? info.mimeType : "",
    timestamp: Math.max(0, Math.trunc(Number(info?.timestamp) || 0)),
    size: normalizedStreamSize(info?.size),
    attributes: { ...(info?.attributes || {}) },
    sender_identity: typeof senderIdentity === "string" ? senderIdentity : null,
  };
}

function normalizeTextStreamInfo(info, senderIdentity = null) {
  return {
    ...normalizeStreamInfo(info, senderIdentity),
    attached_stream_ids: Array.isArray(info?.attachedStreamIds) ? [...info.attachedStreamIds] : [],
  };
}

function normalizeByteStreamInfo(info, senderIdentity = null) {
  return {
    ...normalizeStreamInfo(info, senderIdentity),
    name: typeof info?.name === "string" ? info.name : "",
  };
}

function roomForDataStream(roomId) {
  const roomRecord = roomSessions.get(roomId);
  if (!roomRecord || roomRecord.released) throw new Error("Room Session is no longer available");
  return roomRecord;
}

function beginOutgoingDataStream(roomId, options, kind, onComplete) {
  const roomRecord = roomForDataStream(roomId);
  const participant = roomRecord.room.localParticipant;
  requireDataPublishing(participant);
  const destinationIdentity = options?.destinationIdentity;
  const applicationTopic = options?.topic;
  if (typeof destinationIdentity !== "string" || destinationIdentity.length === 0) {
    throw new TypeError("Data Stream destination Participant Identity is required");
  }
  if (typeof applicationTopic !== "string" || applicationTopic.length === 0) {
    throw new TypeError("Data Stream application topic is required");
  }
  const destinationParticipant = roomRecord.room.remoteParticipants.get(destinationIdentity);
  if (!destinationParticipant) {
    throw flowControlError("DestinationUnavailableError", "Data Stream destination is unavailable");
  }
  let callback = onComplete;
  let openSettled = false;
  const settle = (success, value) => {
    if (!callback) return;
    const current = callback;
    callback = null;
    openSettled = true;
    roomRecord.pendingDataStreamOpens.delete(settle);
    current(success, value);
  };
  roomRecord.pendingDataStreamOpens.add(settle);
  const operation = (async () => {
    if (roomRecord.released) throw new Error("Room Session is no longer available");
    let nativeOpen;
    const writer = await enqueueBoundedReliableDataMutation(
      roomRecord,
      () => {
        nativeOpen =
          kind === "text"
            ? participant.streamText(nativeStreamOptions(options, kind))
            : participant.streamBytes(nativeStreamOptions(options, kind));
        void Promise.resolve(nativeOpen)
          .then((lateWriter) => {
            if (openSettled) void Promise.resolve(lateWriter?.close?.()).catch(() => {});
          })
          .catch(() => {});
        return nativeOpen;
      },
      "LiveKit did not open the native Data Stream",
    );
    try {
      requireMethods(writer, "Data Stream writer", ["write", "close"]);
    } catch (error) {
      void Promise.resolve(writer?.close?.()).catch(() => {});
      throw error;
    }
    if (typeof writer.info?.id !== "string" || writer.info.id.length === 0) {
      await writer.close().catch(() => {});
      throw new TypeError("LiveKit Data Stream writer info ID is missing");
    }
    if (
      openSettled ||
      roomRecord.released ||
      roomRecord.room.remoteParticipants.get(destinationIdentity) !== destinationParticipant
    ) {
      await writer.close().catch(() => {});
      throw flowControlError(
        "DestinationUnavailableError",
        "Data Stream destination changed while opening",
      );
    }
    const id = nextOutgoingDataStreamId++;
    const record = {
      id,
      roomId,
      writer,
      kind,
      wireId: writer.info.id,
      destinationIdentity,
      destinationParticipant,
      applicationTopic,
      sent: 0,
      acknowledged: 0,
      pendingAck: null,
      openResolve: null,
      openReject: null,
      openTimer: null,
      closing: false,
      terminal: false,
      pendingOperations: new Set(),
    };
    outgoingDataStreams.set(id, record);
    roomRecord.outgoingDataStreamIds.add(id);
    const info =
      kind === "text"
        ? normalizeTextStreamInfo(writer.info)
        : normalizeByteStreamInfo(writer.info);
    info.topic = applicationTopic;
    const descriptor = {
      id,
      info,
    };
    return new Promise((resolve, reject) => {
      record.openResolve = () => resolve(descriptor);
      record.openReject = reject;
      record.openTimer = setTimeout(() => {
        terminateOutgoingDataStream(
          record,
          flowControlError(
            "FlowControlTimeoutError",
            "Data Stream receiver did not acknowledge the stream",
          ),
          true,
        );
      }, FLOW_OPEN_TIMEOUT_MS);
    });
  })();
  operation.then(
    (descriptor) => settle(true, descriptor),
    (error) => settle(false, error),
  );
}

export function openTextStream(roomId, options, onComplete) {
  beginOutgoingDataStream(roomId, options, "text", onComplete);
}

export function openByteStream(roomId, options, onComplete) {
  beginOutgoingDataStream(roomId, options, "bytes", onComplete);
}

function outgoingDataStreamRecord(streamId, expectedKind) {
  const record = outgoingDataStreams.get(streamId);
  if (!record || record.terminal || record.closing) {
    throw new Error("Outgoing data stream is no longer available");
  }
  if (expectedKind && record.kind !== expectedKind) {
    throw new TypeError(`Outgoing data stream does not carry ${expectedKind}`);
  }
  return record;
}

function removeOutgoingDataStream(record) {
  outgoingDataStreams.delete(record.id);
  roomSessions.get(record.roomId)?.outgoingDataStreamIds.delete(record.id);
}

function clearOutgoingTimer(record) {
  if (record.openTimer != null) clearTimeout(record.openTimer);
  record.openTimer = null;
}

function terminateOutgoingDataStream(record, error = terminalDataStreamError(), notifyRemote = false) {
  if (record.terminal) return;
  record.terminal = true;
  clearOutgoingTimer(record);
  const rejectOpen = record.openReject;
  record.openResolve = null;
  record.openReject = null;
  rejectOpen?.(error);
  settleOutgoingOperations(record, error);
  record.pendingAck = null;
  removeOutgoingDataStream(record);
  const roomRecord = roomSessions.get(record.roomId);
  if (notifyRemote && roomRecord) {
    void sendFlowControl(roomRecord, record.destinationIdentity, {
      type: "cancel",
      streamId: record.wireId,
    }).catch(() => {});
  }
  if (record.closing) {
    try {
      void Promise.resolve(record.writer.defaultWriter?.abort(error)).catch(() => {});
    } catch {}
    releaseFailedSdkWriter(record);
  } else if (roomRecord) {
    void enqueueBoundedReliableDataMutation(
      roomRecord,
      () => record.writer.close(),
      "LiveKit did not release the native Data Stream",
    ).catch(() => releaseFailedSdkWriter(record));
  } else {
    releaseFailedSdkWriter(record);
  }
}

function trackOutgoingOperation(record, onComplete) {
  let callback = onComplete;
  const settle = (success, error) => {
    if (!callback) return;
    const current = callback;
    callback = null;
    record.pendingOperations.delete(settle);
    current(success, error);
  };
  record.pendingOperations.add(settle);
  return settle;
}

function terminalDataStreamError() {
  return new Error("Room Session or data stream became terminal");
}

function settleOutgoingOperations(record, error = terminalDataStreamError()) {
  for (const settle of [...record.pendingOperations]) settle(false, error);
}

function releaseFailedSdkWriter(record) {
  try {
    record.writer.defaultWriter?.releaseLock();
  } catch {
    // The pinned SDK can retain the lock after a sink error; cleanup remains best effort.
  }
  try {
    const onClose = record.writer.onClose;
    record.writer.onClose = undefined;
    onClose?.();
  } catch {
    // Preserve the actionable stream failure over a private SDK cleanup failure.
  }
}

function beginOutgoingWrite(streamId, expectedKind, chunk, onComplete) {
  const record = outgoingDataStreamRecord(streamId, expectedKind);
  const roomRecord = roomForDataStream(record.roomId);
  const settle = trackOutgoingOperation(record, onComplete);
  const expectedAcknowledgement = record.sent + 1;
  record.sent = expectedAcknowledgement;
  const pending = {
    expectedAcknowledgement,
    writerDone: false,
    settle,
  };
  record.pendingAck = pending;
  let operation;
  try {
    operation = enqueueBoundedReliableDataMutation(
      roomRecord,
      () => {
        if (record.terminal) throw terminalDataStreamError();
        return record.writer.write(chunk);
      },
      "LiveKit did not accept the native Data Stream chunk",
    );
  } catch (error) {
    terminateOutgoingDataStream(record, error, true);
    return;
  }
  operation.then(
    () => {
      pending.writerDone = true;
      if (record.pendingAck !== pending || record.acknowledged < expectedAcknowledgement) return;
      record.pendingAck = null;
      settle(true);
    },
    (error) => terminateOutgoingDataStream(record, error, true),
  );
}

export function writeTextStream(streamId, chunk, onComplete) {
  beginOutgoingWrite(streamId, "text", chunk, onComplete);
}

export function writeByteStream(streamId, chunk, onComplete) {
  beginOutgoingWrite(streamId, "bytes", Uint8Array.from(chunk), onComplete);
}

export function completeOutgoingDataStream(streamId, onComplete) {
  const record = outgoingDataStreamRecord(streamId);
  const roomRecord = roomForDataStream(record.roomId);
  record.closing = true;
  const settle = trackOutgoingOperation(record, onComplete);
  let operation;
  try {
    operation = enqueueBoundedReliableDataMutation(
      roomRecord,
      () => record.writer.close(),
      "LiveKit did not complete the native Data Stream",
    );
  } catch (error) {
    terminateOutgoingDataStream(record, error, true);
    return;
  }
  operation.then(
    () => {
      clearOutgoingTimer(record);
      record.closing = false;
      record.terminal = true;
      removeOutgoingDataStream(record);
      settle(true);
    },
    (error) => terminateOutgoingDataStream(record, error, true),
  );
}

export function dropOutgoingDataStream(streamId) {
  const record = outgoingDataStreams.get(streamId);
  if (!record) return;
  terminateOutgoingDataStream(record, terminalDataStreamError(), true);
}

export function dropOutgoingDataStreamDescriptor(descriptor) {
  if (Number.isInteger(descriptor?.id)) dropOutgoingDataStream(descriptor.id);
}

function incomingDataStreamRegistration(roomRecord, topic, kind) {
  for (const registrationId of roomRecord.dataStreamHandlerIds) {
    const registration = dataStreamHandlers.get(registrationId);
    if (registration?.kind === kind && registration.topic === topic) return registration;
  }
  return null;
}

function drainIncomingDataStream(record) {
  void record.readTail
    .then(async () => {
      try {
        await record.iterator.return?.();
      } catch {}
      try {
        for await (const _chunk of record.reader) {
          // Drain the one admitted chunk and trailer so LiveKit releases its stream controller.
        }
      } catch {}
    })
    .catch(() => {})
    .finally(() => removeIncomingDataStream(record));
}

function cancelIncomingDataStream(record, notifyRemote = true, force = false) {
  if (record.terminal) {
    if (force) {
      record.abortController.abort(new DOMException("Incoming data stream cancelled", "AbortError"));
      removeIncomingDataStream(record);
    }
    return;
  }
  record.terminal = true;
  const roomRecord = roomSessions.get(record.roomId);
  if (notifyRemote && roomRecord) {
    void sendFlowControl(roomRecord, record.senderIdentity, {
      type: "cancel",
      streamId: record.wireId,
    }).catch(() => cancelIncomingDataStream(record, false, true));
  }
  if (force) {
    record.abortController.abort(new DOMException("Incoming data stream cancelled", "AbortError"));
    removeIncomingDataStream(record);
  } else {
    drainIncomingDataStream(record);
  }
}

function handleIncomingNativeDataStream(roomRecord, kind, reader, participantInfo) {
  if (roomRecord.released) return;
  requireMethods(reader, "Data Stream reader", ["withAbortSignal"]);
  if (typeof reader?.[Symbol.asyncIterator] !== "function") {
    throw new TypeError("LiveKit Data Stream reader is not async iterable");
  }
  const wireId = reader.info?.id;
  const applicationTopic = reader.info?.attributes?.[FLOW_APPLICATION_TOPIC_ATTRIBUTE];
  const protocol = reader.info?.attributes?.[FLOW_PROTOCOL_ATTRIBUTE];
  const senderIdentity = participantInfo?.identity;
  const senderParticipant = roomRecord.room.remoteParticipants.get(senderIdentity);
  const registration = incomingDataStreamRegistration(roomRecord, applicationTopic, kind);
  if (
    typeof wireId !== "string" ||
    wireId.length === 0 ||
    protocol !== FLOW_PROTOCOL_VERSION ||
    typeof senderIdentity !== "string" ||
    !senderParticipant ||
    !registration ||
    roomRecord.incomingDataStreamIds.size >= MAX_ACTIVE_INCOMING_DATA_STREAMS
  ) {
    if (typeof wireId === "string" && wireId.length > 0 && typeof senderIdentity === "string") {
      void sendFlowControl(roomRecord, senderIdentity, {
        type: "cancel",
        streamId: wireId,
      }).catch(() => {});
    }
    return;
  }

  const streamId = nextIncomingDataStreamId++;
  const abortController = new AbortController();
  const iterator = reader.withAbortSignal(abortController.signal)[Symbol.asyncIterator]();
  requireMethods(iterator, "Data Stream iterator", ["next"]);
  const record = {
    id: streamId,
    roomId: roomRecord.id,
    kind,
    wireId,
    senderIdentity,
    senderParticipant,
    reader,
    iterator,
    abortController,
    readTail: Promise.resolve(),
    consumed: 0,
    terminal: false,
  };
  incomingDataStreams.set(streamId, record);
  roomRecord.incomingDataStreamIds.add(streamId);
  const info =
    kind === "text"
      ? normalizeTextStreamInfo(reader.info, senderIdentity)
      : normalizeByteStreamInfo(reader.info, senderIdentity);
  info.topic = applicationTopic;
  registration.callback({ id: streamId, info });
  void sendFlowControl(roomRecord, senderIdentity, {
    type: "ack",
    streamId: wireId,
    consumed: 0,
  }).catch(() => cancelIncomingDataStream(record, false));
}

function registerDataStreamHandler(roomId, topic, kind, callback) {
  const roomRecord = roomForDataStream(roomId);
  if (incomingDataStreamRegistration(roomRecord, topic, kind)) {
    throw flowControlError(
      "HandlerAlreadyRegisteredError",
      `A ${kind} stream handler already exists for topic ${topic}`,
    );
  }
  const id = nextDataStreamHandlerId++;
  if (kind === "text" && !roomRecord.flowTextHandlerInstalled) {
    roomRecord.room.registerTextStreamHandler(FLOW_TEXT_TOPIC, (reader, participantInfo) =>
      handleIncomingNativeDataStream(roomRecord, "text", reader, participantInfo),
    );
    roomRecord.flowTextHandlerInstalled = true;
  } else if (kind === "bytes" && !roomRecord.flowByteHandlerInstalled) {
    roomRecord.room.registerByteStreamHandler(FLOW_BYTES_TOPIC, (reader, participantInfo) =>
      handleIncomingNativeDataStream(roomRecord, "bytes", reader, participantInfo),
    );
    roomRecord.flowByteHandlerInstalled = true;
  }
  dataStreamHandlers.set(id, { id, roomId, topic, kind, callback });
  roomRecord.dataStreamHandlerIds.add(id);
  return id;
}

export function registerTextStreamHandler(roomId, topic, callback) {
  return registerDataStreamHandler(roomId, topic, "text", callback);
}

export function registerByteStreamHandler(roomId, topic, callback) {
  return registerDataStreamHandler(roomId, topic, "bytes", callback);
}

export function dropDataStreamHandler(registrationId) {
  const registration = dataStreamHandlers.get(registrationId);
  if (!registration) return;
  dataStreamHandlers.delete(registrationId);
  const roomRecord = roomSessions.get(registration.roomId);
  roomRecord?.dataStreamHandlerIds.delete(registrationId);
  if (!roomRecord) return;
  const hasKind = [...roomRecord.dataStreamHandlerIds].some(
    (id) => dataStreamHandlers.get(id)?.kind === registration.kind,
  );
  if (!hasKind && registration.kind === "text" && roomRecord.flowTextHandlerInstalled) {
    roomRecord.room.unregisterTextStreamHandler(FLOW_TEXT_TOPIC);
    roomRecord.flowTextHandlerInstalled = false;
  } else if (!hasKind && registration.kind === "bytes" && roomRecord.flowByteHandlerInstalled) {
    roomRecord.room.unregisterByteStreamHandler(FLOW_BYTES_TOPIC);
    roomRecord.flowByteHandlerInstalled = false;
  }
}

function incomingDataStreamRecord(streamId, expectedKind) {
  const record = incomingDataStreams.get(streamId);
  if (!record || record.terminal) throw new Error("Incoming data stream is no longer available");
  if (record.kind !== expectedKind) {
    throw new TypeError(`Incoming data stream does not carry ${expectedKind}`);
  }
  return record;
}

function readIncomingDataStream(streamId, expectedKind) {
  const record = incomingDataStreamRecord(streamId, expectedKind);
  const operation = record.readTail.then(() => record.iterator.next());
  record.readTail = operation.catch(() => {});
  return operation.then(async (result) => {
    if (record.terminal) return { done: true, value: null };
    if (result.done) finishIncomingDataStream(streamId);
    let value = null;
    if (!result.done && expectedKind === "text") {
      value = typeof result.value === "string" ? result.value : "";
    } else if (!result.done) {
      value = Array.from(result.value);
    }
    if (!result.done) {
      record.consumed += 1;
      const roomRecord = roomForDataStream(record.roomId);
      await sendFlowControl(roomRecord, record.senderIdentity, {
        type: "ack",
        streamId: record.wireId,
        consumed: record.consumed,
      });
    }
    return { done: result.done === true, value };
  });
}

export function readTextStream(streamId) {
  return readIncomingDataStream(streamId, "text");
}

export function readByteStream(streamId) {
  return readIncomingDataStream(streamId, "bytes");
}

function removeIncomingDataStream(record) {
  incomingDataStreams.delete(record.id);
  roomSessions.get(record.roomId)?.incomingDataStreamIds.delete(record.id);
}

export function finishIncomingDataStream(streamId) {
  const record = incomingDataStreams.get(streamId);
  if (!record) return;
  record.terminal = true;
  removeIncomingDataStream(record);
}

export function dropIncomingDataStream(streamId) {
  const record = incomingDataStreams.get(streamId);
  if (record) cancelIncomingDataStream(record);
}

export function dropIncomingDataStreamDescriptor(descriptor) {
  if (Number.isInteger(descriptor?.id)) dropIncomingDataStream(descriptor.id);
}

function outgoingDataStreamByWireId(roomRecord, wireId) {
  for (const streamId of roomRecord.outgoingDataStreamIds) {
    const record = outgoingDataStreams.get(streamId);
    if (record?.wireId === wireId) return record;
  }
  return null;
}

function incomingDataStreamByWireId(roomRecord, wireId) {
  for (const streamId of roomRecord.incomingDataStreamIds) {
    const record = incomingDataStreams.get(streamId);
    if (record?.wireId === wireId) return record;
  }
  return null;
}

function handleFlowControl(roomRecord, payload, participant, reliable, topic) {
  if (topic !== FLOW_CONTROL_TOPIC) return false;
  if (!reliable || !participant) return true;
  const message = parseFlowControl(payload);
  if (!message) return true;

  if (message.type === "ack") {
    const record = outgoingDataStreamByWireId(roomRecord, message.streamId);
    if (
      !record ||
      record.destinationParticipant !== participant ||
      roomRecord.room.remoteParticipants.get(record.destinationIdentity) !== participant
    ) {
      return true;
    }
    if (
      !Number.isSafeInteger(message.consumed) ||
      message.consumed < 0 ||
      message.consumed > record.sent ||
      message.consumed < record.acknowledged
    ) {
      terminateOutgoingDataStream(
        record,
        flowControlError("FlowControlProtocolError", "Data Stream acknowledgement is invalid"),
        true,
      );
      return true;
    }
    record.acknowledged = message.consumed;
    if (message.consumed === 0 && record.openResolve) {
      if (record.openTimer != null) clearTimeout(record.openTimer);
      record.openTimer = null;
      const resolve = record.openResolve;
      record.openResolve = null;
      record.openReject = null;
      resolve();
    }
    const pending = record.pendingAck;
    if (
      pending &&
      pending.writerDone &&
      record.acknowledged >= pending.expectedAcknowledgement
    ) {
      record.pendingAck = null;
      pending.settle(true);
    }
    return true;
  }

  if (message.type === "cancel") {
    const outgoing = outgoingDataStreamByWireId(roomRecord, message.streamId);
    if (outgoing?.destinationParticipant === participant) {
      terminateOutgoingDataStream(
        outgoing,
        flowControlError("RemoteCancelledError", "Data Stream receiver cancelled the stream"),
        false,
      );
    }
    const incoming = incomingDataStreamByWireId(roomRecord, message.streamId);
    if (incoming?.senderParticipant === participant) cancelIncomingDataStream(incoming, false);
    return true;
  }
  return true;
}

function normalizeRpcFailure(error) {
  return {
    terminal: false,
    permission_denied:
      error?.status === 403 ||
      error?.name === "NotAllowedError" ||
      error?.name === "PermissionDeniedError" ||
      error?.name === "UnauthorizedError",
    code: Number.isInteger(error?.code) && error.code >= 0 ? error.code : null,
    message:
      typeof error?.message === "string"
        ? error.message
        : typeof error === "string"
          ? error
          : "unknown Participant RPC failure",
    data: typeof error?.data === "string" ? error.data : null,
    cause: error,
  };
}

export function invokeParticipantRpc(roomId, options, onComplete) {
  let roomRecord;
  let callback = onComplete;
  const settle = (success, value) => {
    if (!callback) return;
    const current = callback;
    callback = null;
    roomRecord?.pendingParticipantRpcCalls.delete(settle);
    current(success, value);
  };

  try {
    roomRecord = roomForDataStream(roomId);
    const participant = roomRecord.room.localParticipant;
    requireDataPublishing(participant);
    const invocation = participant.performRpc({
      destinationIdentity: options.destinationIdentity,
      method: options.method,
      payload: options.payload,
      responseTimeout: options.responseTimeoutMs,
    });
    roomRecord.pendingParticipantRpcCalls.add(settle);
    Promise.resolve(invocation).then(
      (response) => settle(true, response),
      (error) => settle(false, normalizeRpcFailure(error)),
    );
  } catch (error) {
    settle(false, normalizeRpcFailure(error));
  }
}

export function registerParticipantRpcMethod(roomId, method, callback) {
  const roomRecord = roomForDataStream(roomId);
  const id = nextParticipantRpcMethodId++;
  roomRecord.room.registerRpcMethod(method, (invocation) => {
    return new Promise((resolve, reject) => {
      if (roomRecord.released || !participantRpcMethods.has(id)) {
        reject(
          new globalThis.LivekitClient.RpcError(
            1400,
            "Participant RPC method is no longer registered",
          ),
        );
        return;
      }
      const invocationId = nextParticipantRpcInvocationId++;
      participantRpcInvocations.set(invocationId, {
        id: invocationId,
        roomId,
        resolve,
        reject,
      });
      roomRecord.participantRpcInvocationIds.add(invocationId);
      try {
        callback({
          id: invocationId,
          request_id: invocation.requestId,
          caller_identity: invocation.callerIdentity,
          payload: invocation.payload,
          response_timeout_ms: invocation.responseTimeout,
        });
      } catch (error) {
        participantRpcInvocations.delete(invocationId);
        roomRecord.participantRpcInvocationIds.delete(invocationId);
        reject(new globalThis.LivekitClient.RpcError(1500, "Participant RPC handler failed"));
      }
    });
  });
  participantRpcMethods.set(id, { id, roomId, method });
  roomRecord.participantRpcMethodIds.add(id);
  return id;
}

export function dropParticipantRpcMethod(registrationId) {
  const registration = participantRpcMethods.get(registrationId);
  if (!registration) return;
  participantRpcMethods.delete(registrationId);
  const roomRecord = roomSessions.get(registration.roomId);
  roomRecord?.participantRpcMethodIds.delete(registrationId);
  roomRecord?.room.unregisterRpcMethod(registration.method);
}

export function completeParticipantRpcInvocation(invocationId, completion) {
  const invocation = participantRpcInvocations.get(invocationId);
  if (!invocation) return;
  participantRpcInvocations.delete(invocationId);
  roomSessions.get(invocation.roomId)?.participantRpcInvocationIds.delete(invocationId);
  if (completion?.success === true) {
    invocation.resolve(typeof completion.payload === "string" ? completion.payload : "");
  } else {
    invocation.reject(
      new globalThis.LivekitClient.RpcError(
        Number.isInteger(completion?.code) ? completion.code : 1500,
        typeof completion?.message === "string"
          ? completion.message
          : "Participant RPC handler failed",
        typeof completion?.data === "string" ? completion.data : undefined,
      ),
    );
  }
}

export function dropParticipantRpcInvocation(invocationId) {
  const invocation = participantRpcInvocations.get(invocationId);
  if (!invocation) return;
  participantRpcInvocations.delete(invocationId);
  roomSessions.get(invocation.roomId)?.participantRpcInvocationIds.delete(invocationId);
  invocation.reject(new globalThis.LivekitClient.RpcError(1503, "Room Session left"));
}

export function dropParticipantRpcInvocationDescriptor(descriptor) {
  if (Number.isInteger(descriptor?.id)) dropParticipantRpcInvocation(descriptor.id);
}

export function resolveRemoteMediaTrack(
  roomId,
  locator,
  mediaGeneration,
  onTerminal,
) {
  const roomRecord = roomSessions.get(roomId);
  if (!roomRecord || roomRecord.released) throw new Error("Room Session is no longer available");
  const current = currentRemotePublication(roomRecord, locator);
  const publication = current?.publication;
  const track = publication?.track;
  if (!track) return null;
  const currentMediaGeneration = remoteMediaGeneration(track);
  if (currentMediaGeneration !== mediaGeneration) {
    throw new Error("Remote Media Track is no longer available");
  }
  const kind = publicationKind(track.kind);
  if (kind !== "audio" && kind !== "video") {
    throw new TypeError("Remote Track Publication does not contain audio or video");
  }

  const id = nextRemoteMediaTrackId++;
  const record = {
    id,
    roomId,
    locator,
    participant: current.participant,
    publication,
    track,
    mediaGeneration: currentMediaGeneration,
    kind,
    attachments: new Map(),
    audioAnalyserIds: new Set(),
    terminal: false,
    terminalCallback: onTerminal,
    endedListener: null,
  };
  record.endedListener = () => terminateRemoteMediaTrack(record);
  track.on(globalThis.LivekitClient.TrackEvent.Ended, record.endedListener);
  remoteMediaTracks.set(id, record);
  roomRecord.remoteMediaTrackIds.add(id);
  return { id, kind, generation: currentMediaGeneration };
}

function normalizeReceiverStatistics(statistics) {
  if (!statistics || !Number.isFinite(statistics.timestamp)) {
    throw new TypeError("LiveKit returned invalid receiver statistics");
  }
  return {
    timestamp: statistics.timestamp,
    jitter_buffer_delay: optionalFiniteNumber(statistics.jitterBufferDelay),
    packets_lost: optionalSignedInteger(statistics.packetsLost),
    packets_received: optionalUnsignedInteger(statistics.packetsReceived),
    bytes_received: optionalUnsignedInteger(statistics.bytesReceived),
    stream_id: optionalString(statistics.streamId),
    jitter: optionalFiniteNumber(statistics.jitter),
  };
}

async function remoteTrackReceiverStatistics(trackId, kind) {
  const record = remoteMediaTrackRecord(trackId);
  if (record.kind !== kind) throw new TypeError(`Remote Media Track is not ${kind}`);
  if (!remoteMediaTrackIsCurrent(record)) {
    terminateRemoteMediaTrack(record);
    throw new Error("Remote Media Track is no longer available");
  }
  const track = record.track;
  const statistics = await track.getReceiverStats();
  if (
    remoteMediaTrackRecord(trackId) !== record ||
    record.track !== track ||
    !remoteMediaTrackIsCurrent(record)
  ) {
    terminateRemoteMediaTrack(record);
    throw new Error("Remote Media Track is no longer available");
  }
  return statistics;
}

export async function remoteAudioReceiverStatistics(trackId) {
  const statistics = await remoteTrackReceiverStatistics(trackId, "audio");
  if (!statistics) return null;
  return {
    ...normalizeReceiverStatistics(statistics),
    concealed_samples: optionalUnsignedInteger(statistics.concealedSamples),
    concealment_events: optionalUnsignedInteger(statistics.concealmentEvents),
    silent_concealed_samples: optionalUnsignedInteger(statistics.silentConcealedSamples),
    silent_concealment_events: optionalUnsignedInteger(statistics.silentConcealmentEvents),
    total_audio_energy: optionalFiniteNumber(statistics.totalAudioEnergy),
    total_samples_duration: optionalFiniteNumber(statistics.totalSamplesDuration),
  };
}

export async function remoteVideoReceiverStatistics(trackId) {
  const statistics = await remoteTrackReceiverStatistics(trackId, "video");
  if (!statistics) return null;
  return {
    ...normalizeReceiverStatistics(statistics),
    frames_decoded: optionalUnsignedInteger(statistics.framesDecoded),
    frames_dropped: optionalUnsignedInteger(statistics.framesDropped),
    frames_received: optionalUnsignedInteger(statistics.framesReceived),
    frame_width: optionalUnsignedInteger(statistics.frameWidth),
    frame_height: optionalUnsignedInteger(statistics.frameHeight),
    fir_count: optionalUnsignedInteger(statistics.firCount),
    pli_count: optionalUnsignedInteger(statistics.pliCount),
    nack_count: optionalUnsignedInteger(statistics.nackCount),
    decoder_implementation: optionalString(statistics.decoderImplementation),
    mime_type: optionalString(statistics.mimeType),
  };
}

export async function rawRemoteRtcStatsReport(trackId) {
  const record = remoteMediaTrackRecord(trackId);
  if (!remoteMediaTrackIsCurrent(record)) {
    terminateRemoteMediaTrack(record);
    throw new Error("Remote Media Track is no longer available");
  }
  const track = record.track;
  if (typeof track.getRTCStatsReport !== "function") {
    throw new TypeError("LiveKit Remote Media Track does not expose RTCStatsReport");
  }
  const report = await track.getRTCStatsReport();
  if (
    remoteMediaTrackRecord(trackId) !== record ||
    record.track !== track ||
    !remoteMediaTrackIsCurrent(record)
  ) {
    terminateRemoteMediaTrack(record);
    throw new Error("Remote Media Track is no longer available");
  }
  return report ?? null;
}

export function attachRemoteVideoTrack(trackId, element) {
  const record = remoteMediaTrackRecord(trackId);
  if (record.kind !== "video") throw new TypeError("Remote Media Track is not video");
  const attachmentId = nextAttachmentId++;
  record.track.attach(element);
  record.attachments.set(attachmentId, element);
  return attachmentId;
}

export function detachRemoteVideoTrack(trackId, attachmentId) {
  detachRemoteElement(remoteMediaTrackRecord(trackId), attachmentId);
}

export function dropRemoteVideoTrackAttachment(trackId, attachmentId) {
  const record = remoteMediaTracks.get(trackId);
  if (!record) return;
  try {
    detachRemoteElement(record, attachmentId);
  } catch {}
}

export function attachRemoteAudioTrack(trackId, element) {
  const record = remoteMediaTrackRecord(trackId);
  if (record.kind !== "audio") throw new TypeError("Remote Media Track is not audio");
  const attachmentId = nextAttachmentId++;
  record.track.attach(element);
  record.attachments.set(attachmentId, element);
  return attachmentId;
}

export function detachRemoteAudioTrack(trackId, attachmentId) {
  detachRemoteElement(remoteMediaTrackRecord(trackId), attachmentId);
}

export function dropRemoteAudioTrackAttachment(trackId, attachmentId) {
  const record = remoteMediaTracks.get(trackId);
  if (!record) return;
  try {
    detachRemoteElement(record, attachmentId);
  } catch {}
}

export function dropRemoteMediaTrack(trackId) {
  const record = remoteMediaTracks.get(trackId);
  if (!record) return;
  terminateRemoteMediaTrack(record);
}

export function dropRemoteMediaTrackDescriptor(descriptor) {
  if (Number.isInteger(descriptor?.id)) dropRemoteMediaTrack(descriptor.id);
}

function audioContextConstructor() {
  const AudioContext = globalThis.AudioContext || globalThis.webkitAudioContext;
  if (typeof AudioContext !== "function") {
    throw new TypeError("Web Audio AudioContext is unavailable");
  }
  return AudioContext;
}

function mediaStreamAudioSource(track) {
  const source = track?.mediaStreamTrack;
  if (!(source instanceof MediaStreamTrack) || source.kind !== "audio") {
    throw new TypeError("Media Track does not expose a browser audio source");
  }
  if (source.readyState === "ended") throw new Error("Media Track audio source has ended");
  return source;
}

function createAudioAnalyser(owner, ownerKind, sourceTrack, onTerminal) {
  const AudioContext = audioContextConstructor();
  const context = new AudioContext();
  let sourceNode;
  let analyserNode;
  let silentSink;
  try {
    sourceNode = context.createMediaStreamSource(new MediaStream([sourceTrack]));
    analyserNode = context.createAnalyser();
    analyserNode.fftSize = 2048;
    silentSink = context.createGain();
    silentSink.gain.value = 0;
    sourceNode.connect(analyserNode);
    analyserNode.connect(silentSink);
    silentSink.connect(context.destination);
  } catch (error) {
    void context.close().catch(() => {});
    throw error;
  }

  const id = nextAudioAnalyserId++;
  const record = {
    id,
    owner,
    ownerKind,
    sourceTrack,
    sourceNode,
    analyserNode,
    silentSink,
    context,
    terminal: false,
    terminalCallback: onTerminal,
    endedListener: null,
    closeOperation: null,
  };
  record.endedListener = () => void terminateAudioAnalyser(record).catch(() => {});
  sourceTrack.addEventListener("ended", record.endedListener);
  audioAnalysers.set(id, record);
  owner.audioAnalyserIds.add(id);
  if (context.state === "suspended") void context.resume().catch(() => {});
  return id;
}

function terminateAudioAnalyser(record) {
  if (record.terminal) return record.closeOperation || Promise.resolve();
  record.terminal = true;
  if (record.endedListener) {
    record.sourceTrack.removeEventListener("ended", record.endedListener);
    record.endedListener = null;
  }
  try {
    record.sourceNode.disconnect();
  } catch {}
  try {
    record.analyserNode.disconnect();
  } catch {}
  try {
    record.silentSink.disconnect();
  } catch {}
  record.owner.audioAnalyserIds.delete(record.id);
  audioAnalysers.delete(record.id);
  const callback = record.terminalCallback;
  record.terminalCallback = null;
  callback?.();
  try {
    record.closeOperation =
      record.context.state === "closed" ? Promise.resolve() : record.context.close();
  } catch (error) {
    record.closeOperation = Promise.reject(error);
  }
  return record.closeOperation;
}

function terminateAudioAnalysers(owner) {
  if (!owner.audioAnalyserIds) return;
  for (const analyserId of [...owner.audioAnalyserIds]) {
    const analyser = audioAnalysers.get(analyserId);
    if (analyser) void terminateAudioAnalyser(analyser).catch(() => {});
  }
}

function audioAnalyserIsCurrent(record) {
  if (record.sourceTrack.readyState === "ended") return false;
  if (record.ownerKind === "local") {
    return (
      localAudioTracks.get(record.owner.id) === record.owner &&
      !record.owner.stopRequested &&
      record.owner.track.mediaStreamTrack === record.sourceTrack
    );
  }
  return (
    remoteMediaTracks.get(record.owner.id) === record.owner &&
    !record.owner.terminal &&
    remoteMediaTrackIsCurrent(record.owner) &&
    record.owner.track.mediaStreamTrack === record.sourceTrack
  );
}

export function createLocalAudioAnalyser(trackId, onTerminal) {
  const record = localAudioTrackRecord(trackId);
  if (record.stopRequested) throw new Error("Local Audio Track capture has stopped");
  return createAudioAnalyser(record, "local", mediaStreamAudioSource(record.track), onTerminal);
}

export function createRemoteAudioAnalyser(trackId, onTerminal) {
  const record = remoteMediaTrackRecord(trackId);
  if (record.kind !== "audio") throw new TypeError("Remote Media Track is not audio");
  if (!remoteMediaTrackIsCurrent(record)) {
    terminateRemoteMediaTrack(record);
    throw new Error("Remote Media Track is no longer available");
  }
  return createAudioAnalyser(record, "remote", mediaStreamAudioSource(record.track), onTerminal);
}

export function readAudioAnalyser(analyserId) {
  const record = audioAnalysers.get(analyserId);
  if (!record || record.terminal) throw new Error("Audio Analyser is no longer available");
  if (!audioAnalyserIsCurrent(record)) {
    void terminateAudioAnalyser(record).catch(() => {});
    throw new Error("Audio Analyser source is no longer available");
  }
  const waveform = new Float32Array(record.analyserNode.fftSize);
  record.analyserNode.getFloatTimeDomainData(waveform);
  return waveform;
}

export function closeAudioAnalyser(analyserId) {
  const record = audioAnalysers.get(analyserId);
  if (!record) return Promise.resolve();
  return terminateAudioAnalyser(record);
}

export function dropAudioAnalyser(analyserId) {
  const record = audioAnalysers.get(analyserId);
  if (record) void terminateAudioAnalyser(record).catch(() => {});
}

function stopLocalVideoRecord(record) {
  let failure = null;
  for (const attachmentId of [...record.attachments.keys()]) {
    try {
      detachLocalVideoElement(record, attachmentId);
    } catch (error) {
      failure ??= error;
    }
  }
  if (!record.captureStopped) {
    try {
      record.track.stop();
      record.captureStopped = true;
    } catch (error) {
      failure ??= error;
    }
  }
  if (failure) throw failure;
}

function terminateLocalVideoAfterRestartFailure(record) {
  record.stopRequested = true;
  try {
    stopLocalVideoRecord(record);
  } catch {
    record.captureStopped = true;
  }
}

function queueLocalVideoStop(record) {
  record.stopRequested = true;
  removeLocalTrackEndedListener(record);
  if (record.stopOperation) return record.stopOperation;
  const operation = enqueueLocalTrackMutation(record, () => stopLocalVideoRecord(record));
  record.stopOperation = operation;
  void operation.catch(() => {
    if (record.stopOperation === operation) record.stopOperation = null;
  });
  return operation;
}

function requireLocalTrackInstance(track, name) {
  requireMethods(track, name, [
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

export function createCameraVideoTrack(options, raw, onComplete) {
  const captureOptions = cameraCaptureOptions(options, raw);
  void globalThis.LivekitClient.createLocalVideoTrack(cloneCaptureOptions(captureOptions)).then(
    (track) => {
      try {
        requireLocalTrackInstance(track, "LocalVideoTrack instance");
        const id = registerLocalVideoTrack(track, captureOptions, "camera");
        onComplete(true, id);
      } catch (error) {
        try {
          track?.stop?.();
        } catch {}
        onComplete(false, error);
      }
    },
    (error) => onComplete(false, error),
  );
}

export function localVideoTrackDeviceId(trackId) {
  const record = localVideoTrackRecord(trackId);
  return enqueueLocalTrackMutation(record, async () => {
    if (record.stopRequested) throw new Error("Local Video Track capture has stopped");
    return (await record.track.getDeviceId()) ?? null;
  });
}

export function restartLocalVideoTrack(trackId, onComplete) {
  const record = localVideoTrackRecord(trackId);
  return restartLocalTrack({
    record,
    nextOptions: (previous) => previous,
    normalizeOptions: cameraCaptureOptions,
    performRestart: restartVideoTrack,
    terminate: terminateLocalVideoAfterRestartFailure,
    onComplete,
  });
}

export function restartLocalVideoTrackWithOptions(trackId, options, raw, onComplete) {
  const record = localVideoTrackRecord(trackId);
  return restartLocalTrack({
    record,
    nextOptions: () => cameraCaptureOptions(options, raw),
    normalizeOptions: (requested) => requested,
    performRestart: restartVideoTrack,
    terminate: terminateLocalVideoAfterRestartFailure,
    onComplete,
  });
}

export function switchLocalVideoTrackDevice(trackId, deviceId, onComplete) {
  const record = localVideoTrackRecord(trackId);
  return restartLocalTrack({
    record,
    nextOptions: (previous) => ({ ...previous, deviceId: { exact: deviceId } }),
    normalizeOptions: cameraCaptureOptions,
    performRestart: restartVideoTrack,
    terminate: terminateLocalVideoAfterRestartFailure,
    onComplete,
  });
}

export function attachLocalVideoTrack(trackId, element) {
  const record = localVideoTrackRecord(trackId);
  if (record.stopRequested) throw new Error("Local Video Track capture has stopped");
  const attachmentId = nextAttachmentId++;
  record.track.attach(element);
  record.attachments.set(attachmentId, element);
  return attachmentId;
}

export function detachLocalVideoTrack(trackId, attachmentId) {
  detachLocalVideoElement(localVideoTrackRecord(trackId), attachmentId);
}

export function stopLocalVideoTrack(trackId) {
  return queueLocalVideoStop(localVideoTrackRecord(trackId));
}

export function dropLocalVideoTrackAttachment(trackId, attachmentId) {
  const record = localVideoTracks.get(trackId);
  if (record) detachLocalVideoElement(record, attachmentId);
}

function finishDroppedLocalVideoTrack(record, trackId, retryDelay = 0) {
  if (record.ended) {
    deleteFinishedBrowserEndedRecord(record);
    return;
  }
  void queueLocalVideoStop(record).then(
    () => {
      if (record.attachments.size === 0 && record.publishedRoomId == null) {
        localVideoTracks.delete(trackId);
      }
    },
    () => {
      const nextDelay = Math.min(Math.max(retryDelay * 2, 10), 1_000);
      setTimeout(() => finishDroppedLocalVideoTrack(record, trackId, nextDelay), nextDelay);
    },
  );
}

export function dropLocalVideoTrack(trackId) {
  const record = localVideoTracks.get(trackId);
  if (!record) return;
  removeLocalTrackEndedListener(record);
  record.ownerDropped = true;
  finishDroppedLocalVideoTrack(record, trackId);
}

function localAudioTrackRecord(trackId) {
  const record = localAudioTracks.get(trackId);
  if (!record) throw new Error("Local Audio Track is no longer available");
  return record;
}

function stopLocalAudioRecord(record) {
  let failure = null;
  terminateAudioAnalysers(record);
  for (const attachmentId of [...record.attachments.keys()]) {
    try {
      detachLocalAudioElement(record, attachmentId);
    } catch (error) {
      failure ??= error;
    }
  }
  if (!record.captureStopped) {
    try {
      record.track.stop();
      record.captureStopped = true;
    } catch (error) {
      failure ??= error;
    }
  }
  if (failure) throw failure;
}

function terminateLocalAudioAfterRestartFailure(record) {
  record.stopRequested = true;
  try {
    stopLocalAudioRecord(record);
  } catch {
    record.captureStopped = true;
  }
}

function queueLocalAudioStop(record) {
  record.stopRequested = true;
  removeLocalTrackEndedListener(record);
  if (record.stopOperation) return record.stopOperation;
  const operation = enqueueLocalTrackMutation(record, () => stopLocalAudioRecord(record));
  record.stopOperation = operation;
  void operation.catch(() => {
    if (record.stopOperation === operation) record.stopOperation = null;
  });
  return operation;
}

export function createMicrophoneAudioTrack(options, raw, onComplete) {
  const captureOptions = microphoneCaptureOptions(options, raw);
  void globalThis.LivekitClient.createLocalAudioTrack(cloneCaptureOptions(captureOptions)).then(
    (track) => {
      try {
        requireLocalTrackInstance(track, "LocalAudioTrack instance");
        const id = registerLocalAudioTrack(track, captureOptions, "microphone");
        onComplete(true, id);
      } catch (error) {
        try {
          track?.stop?.();
        } catch {}
        onComplete(false, error);
      }
    },
    (error) => onComplete(false, error),
  );
}

export function createScreenCapture(options, raw, onComplete) {
  const captureOptions = screenCaptureOptions(options, raw);
  void globalThis.LivekitClient.createLocalScreenTracks(cloneCaptureOptions(captureOptions)).then(
    (tracks) => {
      let videoTrackId = null;
      let audioTrackId = null;
      try {
        if (!Array.isArray(tracks)) {
          throw new TypeError("LiveKit display capture did not return a track array");
        }
        const videoTracks = tracks.filter((track) => track.kind === "video");
        const audioTracks = tracks.filter((track) => track.kind === "audio");
        if (videoTracks.length !== 1 || audioTracks.length > 1 || tracks.length > 2) {
          throw new TypeError(
            "LiveKit display capture must return one video track and at most one audio track",
          );
        }

        requireLocalTrackInstance(videoTracks[0], "LocalVideoTrack instance");
        videoTrackId = registerLocalVideoTrack(videoTracks[0], captureOptions, "screenShare");
        const videoRecord = localVideoTrackRecord(videoTrackId);
        installLocalTrackEndedListener(videoRecord, null);
        if (audioTracks.length === 1) {
          requireLocalTrackInstance(audioTracks[0], "LocalAudioTrack instance");
          audioTrackId = registerLocalAudioTrack(
            audioTracks[0],
            captureOptions,
            "screenShareAudio",
          );
          const audioRecord = localAudioTrackRecord(audioTrackId);
          installLocalTrackEndedListener(audioRecord, null);
          videoRecord.browserEndedPeer = audioRecord;
        }

        onComplete(true, {
          video_track_id: videoTrackId,
          audio_track_id: audioTrackId,
        });
      } catch (error) {
        if (videoTrackId != null) {
          const record = localVideoTracks.get(videoTrackId);
          if (record) {
            removeLocalTrackEndedListener(record);
            try {
              stopLocalVideoRecord(record);
            } catch {}
            localVideoTracks.delete(videoTrackId);
          }
        }
        if (audioTrackId != null) {
          const record = localAudioTracks.get(audioTrackId);
          if (record) {
            removeLocalTrackEndedListener(record);
            try {
              stopLocalAudioRecord(record);
            } catch {}
            localAudioTracks.delete(audioTrackId);
          }
        }
        for (const track of Array.isArray(tracks) ? tracks : []) {
          try {
            track.stop();
          } catch {}
        }
        onComplete(false, error);
      }
    },
    (error) => onComplete(false, error),
  );
}

export function localAudioTrackDeviceId(trackId) {
  const record = localAudioTrackRecord(trackId);
  return enqueueLocalTrackMutation(record, async () => {
    if (record.stopRequested) throw new Error("Local Audio Track capture has stopped");
    return (await record.track.getDeviceId()) ?? null;
  });
}

export function restartLocalAudioTrack(trackId, onComplete) {
  const record = localAudioTrackRecord(trackId);
  return restartLocalTrack({
    record,
    nextOptions: (previous) => previous,
    normalizeOptions: microphoneCaptureOptions,
    performRestart: (current, requested) => current.track.restartTrack(requested),
    terminate: terminateLocalAudioAfterRestartFailure,
    onComplete,
  });
}

export function restartLocalAudioTrackWithOptions(trackId, options, raw, onComplete) {
  const record = localAudioTrackRecord(trackId);
  return restartLocalTrack({
    record,
    nextOptions: () => microphoneCaptureOptions(options, raw),
    normalizeOptions: (requested) => requested,
    performRestart: (current, requested) => current.track.restartTrack(requested),
    terminate: terminateLocalAudioAfterRestartFailure,
    onComplete,
  });
}

export function switchLocalAudioTrackDevice(trackId, deviceId, onComplete) {
  const record = localAudioTrackRecord(trackId);
  return restartLocalTrack({
    record,
    nextOptions: (previous) => ({ ...previous, deviceId: { exact: deviceId } }),
    normalizeOptions: microphoneCaptureOptions,
    performRestart: (current, requested) => current.track.restartTrack(requested),
    terminate: terminateLocalAudioAfterRestartFailure,
    onComplete,
  });
}

export function attachLocalAudioTrack(trackId, element) {
  const record = localAudioTrackRecord(trackId);
  if (record.stopRequested) throw new Error("Local Audio Track capture has stopped");
  const attachmentId = nextAttachmentId++;
  record.track.attach(element);
  record.attachments.set(attachmentId, element);
  return attachmentId;
}

export function detachLocalAudioTrack(trackId, attachmentId) {
  detachLocalAudioElement(localAudioTrackRecord(trackId), attachmentId);
}

export function stopLocalAudioTrack(trackId) {
  return queueLocalAudioStop(localAudioTrackRecord(trackId));
}

export function dropLocalAudioTrackAttachment(trackId, attachmentId) {
  const record = localAudioTracks.get(trackId);
  if (record) detachLocalAudioElement(record, attachmentId);
}

function finishDroppedLocalAudioTrack(record, trackId, retryDelay = 0) {
  if (record.ended) {
    deleteFinishedBrowserEndedRecord(record);
    return;
  }
  void queueLocalAudioStop(record).then(
    () => {
      if (record.attachments.size === 0 && record.publishedRoomId == null) {
        localAudioTracks.delete(trackId);
      }
    },
    () => {
      const nextDelay = Math.min(Math.max(retryDelay * 2, 10), 1_000);
      setTimeout(() => finishDroppedLocalAudioTrack(record, trackId, nextDelay), nextDelay);
    },
  );
}

export function dropLocalAudioTrack(trackId) {
  const record = localAudioTracks.get(trackId);
  if (!record) return;
  removeLocalTrackEndedListener(record);
  record.ownerDropped = true;
  finishDroppedLocalAudioTrack(record, trackId);
}

function audioPublicationOptions(options, raw, record) {
  if (options.audioCodec != null && options.audioCodec !== "opus") {
    throw new TypeError(`unsupported audio codec: ${options.audioCodec}`);
  }
  // livekit-client 2.20.1 fixes browser audio to Opus and has no audioCodec publish option.
  const normalized = { ...rawOptions(raw), source: livekitTrackSource(record.source) };
  if (options.name != null) normalized.name = options.name;
  if (options.dtx != null) normalized.dtx = options.dtx;
  if (options.red != null) normalized.red = options.red;
  return normalized;
}

function requirePublicationSid(publication) {
  if (typeof publication.trackSid !== "string" || publication.trackSid.length === 0) {
    throw new TypeError("LiveKit returned a Track Publication without a SID");
  }
  return publication.trackSid;
}

function publishLocalTrack({
  roomId,
  trackId,
  publicationId,
  options,
  raw,
  onComplete,
  onReplaced,
  recordFor,
  normalizeOptions,
  mediaName,
}) {
  let operation;
  try {
    const room = roomSessions.get(roomId);
    if (!room || room.released) throw new Error("Room Session is no longer available");
    const record = recordFor(trackId);
    if (
      record.awaitingPublicationReplacement &&
      record.awaitingPublicationReplacement.roomId !== roomId
    ) {
      throw new Error(`${mediaName} is still associated with another Room Session`);
    }
    if (record.pendingPublishRoomId != null) {
      throw new Error(`${mediaName} already has a pending publication`);
    }
    const normalizedOptions = normalizeOptions(options, raw, record);
    record.pendingPublishRoomId = roomId;
    record.pendingPublicationId = publicationId;
    operation = enqueueLocalTrackMutation(record, async () => {
      let publication = null;
      try {
        if (room.released) throw new Error("Room Session is no longer available");
        if (record.stopRequested) throw new Error(`${mediaName} capture has stopped`);
        if (record.publishedRoomId != null) {
          throw new Error(`${mediaName} is already associated with a Room Session`);
        }
        publication = await room.room.localParticipant.publishTrack(
          record.track,
          normalizedOptions,
        );
        if (room.released) {
          try {
            await room.room.localParticipant.unpublishTrack(record.track, false);
          } catch {}
          publication = null;
          throw new Error("Room Session is no longer available");
        }
        if (record.ended) {
          try {
            await room.room.localParticipant.unpublishTrack(record.track, false);
          } catch (error) {
            record.publication = publication;
            record.publishedRoomId = roomId;
            record.publicationId = publicationId;
            publication = null;
            throw error;
          }
          publication = null;
          throw new Error(`${mediaName} capture ended while publication was completing`);
        }
        const sid = requirePublicationSid(publication);
        record.publication = publication;
        record.publishedRoomId = roomId;
        record.publicationId = publicationId;
        record.publicationReplacementCallback = onReplaced;
        record.awaitingPublicationReplacement = null;
        record.pendingPublishRoomId = null;
        record.pendingPublicationId = null;
        return sid;
      } catch (error) {
        if (publication) {
          try {
            await room.room.localParticipant.unpublishTrack(record.track, false);
          } catch {}
        }
        if (record.pendingPublicationId === publicationId) {
          record.pendingPublishRoomId = null;
          record.pendingPublicationId = null;
        }
        throw error;
      }
    });
  } catch (error) {
    operation = Promise.reject(error);
  }
  return operation.then(
    (sid) => {
      onComplete(true, sid);
      return sid;
    },
    (error) => {
      onComplete(false, null);
      throw error;
    },
  );
}

function setLocalTrackMuted({ trackId, publicationId, muted, onComplete, recordFor, mediaName }) {
  let operation;
  try {
    const record = recordFor(trackId);
    if (!muted && record.stopRequested) {
      throw new Error(`${mediaName} capture has stopped`);
    }
    operation = enqueueLocalTrackMutation(record, async () => {
      if (record.publicationId !== publicationId || !record.publication) {
        throw new Error("Track Publication is no longer available");
      }
      if (muted) await record.track.mute();
      else await record.track.unmute();
    });
  } catch (error) {
    operation = Promise.reject(error);
  }
  return operation.then(
    (value) => {
      onComplete(true);
      return value;
    },
    (error) => {
      onComplete(false);
      throw error;
    },
  );
}

function unpublishLocalTrack({
  roomId,
  trackId,
  publicationId,
  onComplete,
  recordFor,
  records,
}) {
  let operation;
  try {
    const room = roomSessions.get(roomId);
    if (!room || room.released) throw new Error("Room Session is no longer available");
    const record = recordFor(trackId);
    operation = enqueueLocalTrackMutation(record, async () => {
      if (
        record.publishedRoomId !== roomId ||
        record.publicationId !== publicationId ||
        !record.publication
      ) {
        throw new Error("Track Publication is no longer available");
      }
      record.explicitUnpublishId = publicationId;
      try {
        try {
          await room.room.localParticipant.unpublishTrack(record.track, false);
        } catch (error) {
          if (record.explicitUnpublishObservedId !== publicationId) throw error;
        }
        const replacement = [...room.room.localParticipant.trackPublications.values()].find(
          (candidate) => candidate.track === record.track,
        );
        if (replacement) {
          try {
            await room.room.localParticipant.unpublishTrack(record.track, false);
          } catch (error) {
            const stillPublished = [
              ...room.room.localParticipant.trackPublications.values(),
            ].some((candidate) => candidate.track === record.track);
            if (stillPublished) throw error;
          }
        }
      } finally {
        record.explicitUnpublishId = null;
      }
      record.publication = null;
      record.publishedRoomId = null;
      record.publicationId = null;
      record.publicationReplacementCallback = null;
      record.awaitingPublicationReplacement = null;
      record.explicitUnpublishObservedId = null;
      if (record.ownerDropped && record.pendingPublishRoomId == null) {
        records.delete(trackId);
      }
    });
  } catch (error) {
    operation = Promise.reject(error);
  }
  return operation.then(
    (value) => {
      onComplete(true);
      return value;
    },
    (error) => {
      onComplete(false);
      throw error;
    },
  );
}

export function publishLocalAudioTrack(
  roomId,
  trackId,
  publicationId,
  options,
  raw,
  onReplaced,
  onComplete,
) {
  return publishLocalTrack({
    roomId,
    trackId,
    publicationId,
    options,
    raw,
    onReplaced,
    onComplete,
    recordFor: localAudioTrackRecord,
    normalizeOptions: audioPublicationOptions,
    mediaName: "Local Audio Track",
  });
}

export function setLocalAudioTrackMuted(trackId, publicationId, muted, onComplete) {
  return setLocalTrackMuted({
    trackId,
    publicationId,
    muted,
    onComplete,
    recordFor: localAudioTrackRecord,
    mediaName: "Local Audio Track",
  });
}

export function unpublishLocalAudioTrack(roomId, trackId, publicationId, onComplete) {
  return unpublishLocalTrack({
    roomId,
    trackId,
    publicationId,
    onComplete,
    recordFor: localAudioTrackRecord,
    records: localAudioTracks,
  });
}

const optionalFiniteNumber = (value) =>
  typeof value === "number" && Number.isFinite(value) ? value : null;
const optionalUnsignedInteger = (value) =>
  Number.isSafeInteger(value) && value >= 0 ? value : null;
const optionalSignedInteger = (value) => (Number.isSafeInteger(value) ? value : null);
const optionalString = (value) => (typeof value === "string" ? value : null);
const requiredFiniteNumber = (value, field) => {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  throw new TypeError(`LiveKit returned invalid ${field}`);
};

function normalizeSenderStatistics(statistics) {
  if (!statistics || !Number.isFinite(statistics.timestamp)) {
    throw new TypeError("LiveKit returned invalid sender statistics");
  }
  return {
    timestamp: statistics.timestamp,
    packets_sent: optionalUnsignedInteger(statistics.packetsSent),
    bytes_sent: optionalUnsignedInteger(statistics.bytesSent),
    jitter: optionalFiniteNumber(statistics.jitter),
    packets_lost: optionalSignedInteger(statistics.packetsLost),
    round_trip_time: optionalFiniteNumber(statistics.roundTripTime),
    stream_id: optionalString(statistics.streamId),
  };
}

async function localTrackSenderStatistics(records, recordFor, trackId, publicationId) {
  const record = recordFor(trackId);
  const publication = record.publication;
  const track = record.track;
  const roomId = record.publishedRoomId;
  const roomRecord = roomSessions.get(roomId);
  if (record.publicationId !== publicationId || !publication || !roomRecord || roomRecord.released) {
    throw new Error("Track Publication is no longer available");
  }
  const statistics = await track.getSenderStats();
  if (
    records.get(trackId) !== record ||
    record.publicationId !== publicationId ||
    record.publication !== publication ||
    record.track !== track ||
    record.publishedRoomId !== roomId ||
    roomSessions.get(roomId) !== roomRecord ||
    roomRecord.released
  ) {
    throw new Error("Track Publication is no longer available");
  }
  return statistics;
}

export async function localAudioSenderStatistics(trackId, publicationId) {
  const statistics = await localTrackSenderStatistics(
    localAudioTracks,
    localAudioTrackRecord,
    trackId,
    publicationId,
  );
  return statistics ? normalizeSenderStatistics(statistics) : null;
}

export async function rawLocalRtcStatsReport(trackId, publicationId, kind) {
  const { records, recordFor } = localMediaTrackResources(kind);
  const record = recordFor(trackId);
  const publication = record.publication;
  const track = record.track;
  const roomId = record.publishedRoomId;
  const roomRecord = roomSessions.get(roomId);
  if (record.publicationId !== publicationId || !publication || !roomRecord || roomRecord.released) {
    throw new Error("Track Publication is no longer available");
  }
  if (typeof track.getRTCStatsReport !== "function") {
    throw new TypeError("LiveKit Local Media Track does not expose RTCStatsReport");
  }
  const report = await track.getRTCStatsReport();
  if (
    records.get(trackId) !== record ||
    record.publicationId !== publicationId ||
    record.publication !== publication ||
    record.track !== track ||
    record.publishedRoomId !== roomId ||
    roomSessions.get(roomId) !== roomRecord ||
    roomRecord.released
  ) {
    throw new Error("Track Publication is no longer available");
  }
  return report ?? null;
}

function videoPublicationOptions(options, raw, record) {
  const normalized = { ...rawOptions(raw), source: livekitTrackSource(record.source) };
  if (options.name != null) normalized.name = options.name;
  if (options.videoCodec != null) normalized.videoCodec = options.videoCodec;
  if (options.simulcast != null) normalized.simulcast = options.simulcast;
  if (options.videoEncoding != null) {
    const encoding = {
      maxBitrate: options.videoEncoding.maxBitrate,
      maxFramerate: options.videoEncoding.maxFramerate,
    };
    if (record.source === "screenShare") normalized.screenShareEncoding = encoding;
    else normalized.videoEncoding = encoding;
  }
  return normalized;
}

export function publishLocalVideoTrack(
  roomId,
  trackId,
  publicationId,
  options,
  raw,
  onReplaced,
  onComplete,
) {
  return publishLocalTrack({
    roomId,
    trackId,
    publicationId,
    options,
    raw,
    onReplaced,
    onComplete,
    recordFor: localVideoTrackRecord,
    normalizeOptions: videoPublicationOptions,
    mediaName: "Local Video Track",
  });
}

export function setLocalVideoTrackMuted(trackId, publicationId, muted, onComplete) {
  return setLocalTrackMuted({
    trackId,
    publicationId,
    muted,
    onComplete,
    recordFor: localVideoTrackRecord,
    mediaName: "Local Video Track",
  });
}

export function unpublishLocalVideoTrack(roomId, trackId, publicationId, onComplete) {
  return unpublishLocalTrack({
    roomId,
    trackId,
    publicationId,
    onComplete,
    recordFor: localVideoTrackRecord,
    records: localVideoTracks,
  });
}

function normalizeQualityLimitationDurations(value) {
  if (value == null) return null;
  if (typeof value !== "object") {
    throw new TypeError("LiveKit returned invalid quality limitation durations");
  }
  const normalized = {};
  for (const [key, duration] of Object.entries(value)) {
    normalized[key] = requiredFiniteNumber(duration, "quality limitation duration");
  }
  return normalized;
}

function normalizeVideoSenderStatistics(statistics) {
  return {
    ...normalizeSenderStatistics(statistics),
    fir_count: optionalUnsignedInteger(statistics.firCount),
    pli_count: optionalUnsignedInteger(statistics.pliCount),
    nack_count: optionalUnsignedInteger(statistics.nackCount),
    rid: optionalString(statistics.rid),
    frame_width: optionalUnsignedInteger(statistics.frameWidth),
    frame_height: optionalUnsignedInteger(statistics.frameHeight),
    frames_per_second: optionalFiniteNumber(statistics.framesPerSecond),
    frames_sent: optionalUnsignedInteger(statistics.framesSent),
    quality_limitation_reason: optionalString(statistics.qualityLimitationReason),
    quality_limitation_durations: normalizeQualityLimitationDurations(
      statistics.qualityLimitationDurations,
    ),
    quality_limitation_resolution_changes: optionalUnsignedInteger(
      statistics.qualityLimitationResolutionChanges,
    ),
    retransmitted_packets_sent: optionalUnsignedInteger(statistics.retransmittedPacketsSent),
    target_bitrate: optionalFiniteNumber(statistics.targetBitrate),
  };
}

export async function localVideoSenderStatistics(trackId, publicationId) {
  const statistics = await localTrackSenderStatistics(
    localVideoTracks,
    localVideoTrackRecord,
    trackId,
    publicationId,
  );
  if (!Array.isArray(statistics)) throw new TypeError("LiveKit returned invalid sender statistics");
  return statistics.map(normalizeVideoSenderStatistics);
}

const publicationSource = (source) => trackSources[source]?.normalized ?? "unknown";

const participantConnectionQuality = (quality) => {
  switch (quality) {
    case "excellent":
    case "good":
    case "poor":
    case "lost":
      return quality;
    default:
      return "unknown";
  }
};

const publicationKind = (kind) => {
  switch (kind) {
    case "audio":
    case "video":
    case "data":
      return kind;
    default:
      return "unknown";
  }
};

const grantSource = (source) => {
  switch (source) {
    case 1:
      return "camera";
    case 2:
      return "microphone";
    case 3:
      return "screenShare";
    case 4:
      return "screenShareAudio";
    default:
      return "unknown";
  }
};

function normalizeGrants(permissions) {
  if (!permissions) {
    throw new TypeError("connected LocalParticipant has no permissions");
  }

  return {
    can_subscribe: permissions.canSubscribe === true,
    can_publish: permissions.canPublish === true,
    can_publish_data: permissions.canPublishData === true,
    can_publish_sources: (permissions.canPublishSources || []).map(grantSource),
    can_update_metadata: permissions.canUpdateMetadata === true,
    hidden: permissions.hidden === true,
    recorder: permissions.recorder === true,
    agent: permissions.agent === true,
    can_subscribe_metrics: permissions.canSubscribeMetrics === true,
    can_manage_agent_session: permissions.canManageAgentSession === true,
  };
}

function publicationState(publication) {
  const sid = publication.trackSid;
  if (typeof sid !== "string" || sid.length === 0) return null;
  return {
    sid,
    name: publication.trackName || "",
    source: publicationSource(publication.source),
    kind: publicationKind(publication.kind),
    muted: publication.isMuted === true,
  };
}

function normalizeLocalPublications(participant) {
  const publications = {};
  for (const publication of participant.trackPublications.values()) {
    const state = publicationState(publication);
    if (state) publications[state.sid] = state;
  }
  return publications;
}

function normalizeRemotePublications(record, participant) {
  const publications = {};
  for (const publication of participant.trackPublications.values()) {
    const state = publicationState(publication);
    if (!state) continue;
    publications[state.sid] = {
      ...state,
      generation: remotePublicationGeneration(publication),
      subscribed: publication.isSubscribed === true,
      media_available: publication.track != null,
      media_generation: publication.track ? remoteMediaGeneration(publication.track) : null,
    };
  }
  return publications;
}

function normalizeRemoteDataTracks(record, participant) {
  const tracks = {};
  for (const dataTrack of participant.dataTracks.values()) {
    const sid = dataTrack.info?.sid;
    if (typeof sid !== "string" || sid.length === 0) continue;
    record.remoteDataTracksBySid.set(sid, { participant, track: dataTrack });
    tracks[sid] = {
      sid,
      name: dataTrack.info.name || "",
      uses_e2ee: dataTrack.info.usesE2ee === true,
    };
  }
  return tracks;
}

function normalizeRemoteParticipants(record) {
  const participants = Object.create(null);
  record.remoteDataTracksBySid.clear();
  for (const participant of record.room.remoteParticipants.values()) {
    const identity = participant.identity;
    if (typeof identity !== "string" || identity.length === 0) continue;
    participants[identity] = {
      identity,
      sid: participant.sid,
      generation: remoteParticipantGeneration(participant),
      connection_quality: participantConnectionQuality(participant.connectionQuality),
      metadata: participant.metadata ?? null,
      attributes: { ...participant.attributes },
      track_publications: normalizeRemotePublications(record, participant),
      data_tracks: normalizeRemoteDataTracks(record, participant),
    };
  }
  return participants;
}

function normalizeActiveSpeakers(record) {
  const identities = [];
  const seen = new Set();
  for (const participant of record.room.activeSpeakers || []) {
    const identity = participant?.identity;
    if (typeof identity !== "string" || identity.length === 0 || seen.has(identity)) continue;
    const current =
      participant === record.room.localParticipant
        ? record.room.localParticipant
        : record.room.remoteParticipants.get(identity);
    if (current !== participant) continue;
    seen.add(identity);
    identities.push(identity);
  }
  return identities;
}

function normalizeSnapshot(record) {
  reconcileRemoteMediaTracks(record);
  const participant = record.room.localParticipant;
  for (const publication of participant.trackPublications.values()) {
    reconcileLocalTrackPublished(record, publication);
  }
  return {
    revision: record.revision,
    connection: record.room.state,
    audio_playback: record.room.canPlaybackAudio === false ? "blocked" : "allowed",
    active_speakers: normalizeActiveSpeakers(record),
    room_sid: record.roomSid,
    room_name: record.room.name || "",
    room_metadata: record.room.metadata ?? null,
    local_participant: {
      identity: participant.identity,
      sid: participant.sid,
      connection_quality: participantConnectionQuality(participant.connectionQuality),
      metadata: participant.metadata ?? null,
      attributes: { ...participant.attributes },
      grants: normalizeGrants(participant.permissions),
      track_publications: normalizeLocalPublications(participant),
    },
    remote_participants: normalizeRemoteParticipants(record),
  };
}

function normalizeQueuedEvent(record, event, snapshot) {
  if (event.type !== "participantConnectionQualityChanged") return event;
  const { source_participant: participant, ...normalized } = event;
  if (participant === record.room.localParticipant) {
    normalized.quality = snapshot.local_participant.connection_quality;
    return normalized;
  }
  if (record.room.remoteParticipants.get(event.participant_identity) !== participant) return null;
  const participantSnapshot = snapshot.remote_participants[event.participant_identity];
  if (!participantSnapshot) return null;
  normalized.quality = participantSnapshot.connection_quality;
  return normalized;
}

function scheduleUpdate(record) {
  if (!record.callback || record.updateScheduled || record.pendingEvents.length === 0) return;
  record.updateScheduled = true;
  setTimeout(() => {
    record.updateScheduled = false;
    if (!record.callback || record.pendingEvents.length === 0 || !roomSessions.has(record.id)) return;

    const pendingEvents = record.pendingEvents.splice(0);
    record.revision += 1;
    const snapshot = normalizeSnapshot(record);
    const events = pendingEvents
      .map((event) => normalizeQueuedEvent(record, event, snapshot))
      .filter((event) => event != null);
    record.callback({ snapshot, events });
  }, 0);
}

function queueEvent(record, event) {
  record.pendingEvents.push(event);
  scheduleUpdate(record);
}

function queueRemoteParticipantEvent(record, type, participant) {
  const identity = participant?.identity;
  if (typeof identity !== "string" || identity.length === 0) return;
  const current = record.room.remoteParticipants.get(identity);
  if (type === "remoteParticipantLeft") {
    if (current && current !== participant) return;
  } else if (current !== participant) return;
  queueEvent(record, { type, identity });
}

function queueRemotePublicationsEvent(record, participant) {
  if (record.room.remoteParticipants.get(participant?.identity) !== participant) return;
  queueRemotePublicationsEventByIdentity(record, participant?.identity);
}

function queueRemoteDataTracksEvent(record, participant) {
  if (record.room.remoteParticipants.get(participant?.identity) !== participant) return;
  queueEvent(record, {
    type: "remoteDataTracksChanged",
    participant_identity: participant.identity,
  });
}

function remotePublicationIsCurrent(record, participant, publication) {
  return (
    record.room.remoteParticipants.get(participant?.identity) === participant &&
    participant?.trackPublications.get(publication?.trackSid) === publication
  );
}

function remotePublicationWasReplaced(record, participant, publication) {
  const currentParticipant = record.room.remoteParticipants.get(participant?.identity);
  if (currentParticipant !== participant) return true;
  const sid = publication?.trackSid;
  const currentPublication = participant?.trackPublications.get(sid);
  return currentPublication != null && currentPublication !== publication;
}

function dataTrackWithSid(participant, sid) {
  for (const track of participant?.dataTracks.values() ?? []) {
    if (track.info?.sid === sid) return track;
  }
  return null;
}

function queueRemotePublicationsEventByIdentity(record, participantIdentity) {
  if (typeof participantIdentity === "string" && participantIdentity.length > 0) {
    queueEvent(record, {
      type: "remoteTrackPublicationsChanged",
      participant_identity: participantIdentity,
    });
  }
}

function reconcileLocalTrackPublished(roomRecord, publication) {
  const reconcile = (records) => {
    for (const record of records.values()) {
      let awaiting = record.awaitingPublicationReplacement;
      if (
        !awaiting &&
        record.publishedRoomId === roomRecord.id &&
        record.publication &&
        record.publication !== publication &&
        record.publicationReplacementCallback &&
        publication?.track === record.track
      ) {
        const publicationId = record.publicationId;
        record.publicationReplacementCallback(publicationId, null, false);
        awaiting = {
          roomId: roomRecord.id,
          publicationId,
          callback: record.publicationReplacementCallback,
        };
        record.awaitingPublicationReplacement = awaiting;
        record.publication = null;
      }
      if (
        !awaiting ||
        awaiting.roomId !== roomRecord.id ||
        record.pendingPublishRoomId === roomRecord.id ||
        publication?.track !== record.track
      ) {
        continue;
      }
      const sid = requirePublicationSid(publication);
      const nextId = awaiting.callback(
        awaiting.publicationId,
        sid,
        publication.isMuted === true,
      );
      if (!Number.isInteger(nextId) || nextId <= 0) {
        throw new TypeError("Rust rejected the replacement Local Track Publication");
      }
      record.publication = publication;
      record.publicationId = nextId;
      record.publishedRoomId = roomRecord.id;
      record.publicationReplacementCallback = awaiting.callback;
      record.awaitingPublicationReplacement = null;
    }
  };
  reconcile(localVideoTracks);
  reconcile(localAudioTracks);
}

function reconcileLocalTrackUnpublished(roomRecord, publication) {
  const reconcile = (records, finishDropped) => {
    for (const [trackId, record] of records) {
      if (record.publication !== publication) continue;
      if (record.explicitUnpublishId === record.publicationId) {
        record.explicitUnpublishObservedId = record.publicationId;
        continue;
      }
      const callback = record.publicationReplacementCallback;
      if (record.publishedRoomId === roomRecord.id && callback) {
        const publicationId = record.publicationId;
        callback(publicationId, null, false);
        record.awaitingPublicationReplacement = {
          roomId: roomRecord.id,
          publicationId,
          callback,
        };
        record.publication = null;
        record.publishedRoomId = null;
        record.publicationId = null;
        record.publicationReplacementCallback = null;
        if (record.ownerDropped && record.pendingPublishRoomId == null) {
          finishDropped(record, trackId);
        }
        continue;
      }
      record.publication = null;
      record.publishedRoomId = null;
      record.publicationId = null;
      record.publicationReplacementCallback = null;
      record.awaitingPublicationReplacement = null;
      if (record.ownerDropped && record.pendingPublishRoomId == null) {
        finishDropped(record, trackId);
      }
    }
  };
  reconcile(localVideoTracks, finishDroppedLocalVideoTrack);
  reconcile(localAudioTracks, finishDroppedLocalAudioTrack);
}

function listen(record, event, listener) {
  record.room.on(event, listener);
  record.listeners.push([event, listener]);
}

function removeAdapterListeners(record) {
  for (const [event, listener] of record.listeners.splice(0)) {
    record.room.off(event, listener);
  }
  record.callback = null;
  record.dataPacketCallback = null;
  record.chatMessageCallback = null;
  record.transcriptionSegmentCallback = null;
  record.pendingEvents.length = 0;
}

function releaseDataStreams(record) {
  for (const settle of [...record.pendingDataStreamOpens]) {
    settle(false, terminalDataStreamError());
  }
  for (const registrationId of [...record.dataStreamHandlerIds]) {
    dropDataStreamHandler(registrationId);
  }
  for (const streamId of [...record.outgoingDataStreamIds]) {
    const stream = outgoingDataStreams.get(streamId);
    if (!stream) continue;
    dropOutgoingDataStream(streamId);
  }
  for (const streamId of [...record.incomingDataStreamIds]) {
    const stream = incomingDataStreams.get(streamId);
    if (stream) cancelIncomingDataStream(stream, true, true);
  }
}

function releaseParticipantRpc(record) {
  for (const settle of [...record.pendingParticipantRpcCalls]) {
    settle(false, {
      terminal: true,
      permission_denied: false,
      code: null,
      message: "Room Session is terminal",
      data: null,
    });
  }
  for (const registrationId of [...record.participantRpcMethodIds]) {
    dropParticipantRpcMethod(registrationId);
  }
  for (const invocationId of [...record.participantRpcInvocationIds]) {
    dropParticipantRpcInvocation(invocationId);
  }
}

function releaseLocalTrackAssociations(tracks, roomId, finishDropped) {
  for (const [trackId, trackRecord] of tracks) {
    if (
      trackRecord.publishedRoomId !== roomId &&
      trackRecord.pendingPublishRoomId !== roomId &&
      trackRecord.awaitingPublicationReplacement?.roomId !== roomId
    ) {
      continue;
    }
    trackRecord.publication = null;
    trackRecord.publicationReplacementCallback = null;
    trackRecord.awaitingPublicationReplacement = null;
    trackRecord.explicitUnpublishId = null;
    trackRecord.explicitUnpublishObservedId = null;
    if (trackRecord.publishedRoomId === roomId) {
      trackRecord.publishedRoomId = null;
      trackRecord.publicationId = null;
    }
    if (trackRecord.pendingPublishRoomId === roomId) {
      trackRecord.pendingPublishRoomId = null;
      trackRecord.pendingPublicationId = null;
    }
    if (trackRecord.ownerDropped) finishDropped(trackRecord, trackId);
  }
}

async function releaseRoom(record) {
  if (record.released) return;
  releaseDataStreams(record);
  record.released = true;
  releaseParticipantRpc(record);
  terminateRemoteMediaWhere((track) => track.roomId === record.id);
  removeAdapterListeners(record);
  roomSessions.delete(record.id);
  try {
    await record.room.disconnect(false);
  } finally {
    releaseLocalTrackAssociations(localVideoTracks, record.id, finishDroppedLocalVideoTrack);
    releaseLocalTrackAssociations(localAudioTracks, record.id, finishDroppedLocalAudioTrack);
    record.room.removeAllListeners();
  }
}

export function createRoom(raw) {
  const sdk = globalThis.LivekitClient;
  const room = new sdk.Room(rawOptions(raw));
  try {
    requireMethods(room, "Room", [
      "connect",
      "disconnect",
      "startAudio",
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
    requireMethods(room.localParticipant, "LocalParticipant instance", [
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
  } catch (error) {
    try {
      room.removeAllListeners?.();
      void Promise.resolve(room.disconnect?.(false)).catch(() => {});
    } catch {}
    throw error;
  }
  const record = {
    id: nextRoomId++,
    room,
    roomSid: "",
    revision: 1,
    callback: null,
    dataPacketCallback: null,
    chatMessageCallback: null,
    transcriptionSegmentCallback: null,
    listeners: [],
    pendingEvents: [],
    updateScheduled: false,
    localParticipantMutation: Promise.resolve(),
    reliableDataMutation: Promise.resolve(),
    released: false,
    remoteDataTracksBySid: new Map(),
    remoteMediaTrackIds: new Set(),
    outgoingDataStreamIds: new Set(),
    incomingDataStreamIds: new Set(),
    dataStreamHandlerIds: new Set(),
    flowTextHandlerInstalled: false,
    flowByteHandlerInstalled: false,
    pendingDataStreamOpens: new Set(),
    participantRpcMethodIds: new Set(),
    participantRpcInvocationIds: new Set(),
    pendingParticipantRpcCalls: new Set(),
  };
  roomSessions.set(record.id, record);

  const events = sdk.RoomEvent;
  listen(record, events.ConnectionStateChanged, (state) => {
    queueEvent(record, { type: "connectionStateChanged", state });
  });
  listen(record, events.ParticipantMetadataChanged, (_previous, participant) => {
    if (participant === room.localParticipant) {
      queueEvent(record, { type: "localParticipantChanged" });
    } else {
      queueRemoteParticipantEvent(record, "remoteParticipantChanged", participant);
    }
  });
  listen(record, events.ParticipantAttributesChanged, (_changed, participant) => {
    if (participant === room.localParticipant) {
      queueEvent(record, { type: "localParticipantChanged" });
    } else {
      queueRemoteParticipantEvent(record, "remoteParticipantChanged", participant);
    }
  });
  listen(record, events.ParticipantPermissionsChanged, (_previous, participant) => {
    if (participant === room.localParticipant) {
      queueEvent(record, { type: "localParticipantChanged" });
    }
  });
  listen(record, events.ConnectionQualityChanged, (_quality, participant) => {
    const identity = participant?.identity;
    if (typeof identity !== "string" || identity.length === 0) return;
    if (
      participant !== room.localParticipant &&
      room.remoteParticipants.get(identity) !== participant
    ) {
      return;
    }
    queueEvent(record, {
      type: "participantConnectionQualityChanged",
      participant_identity: identity,
      source_participant: participant,
    });
  });
  listen(record, events.ActiveSpeakersChanged, () => {
    queueEvent(record, { type: "activeSpeakersChanged" });
  });
  listen(record, events.LocalTrackPublished, (publication) => {
    reconcileLocalTrackPublished(record, publication);
    queueEvent(record, { type: "localTrackPublicationsChanged" });
  });
  listen(record, events.LocalTrackUnpublished, (publication) => {
    reconcileLocalTrackUnpublished(record, publication);
    queueEvent(record, { type: "localTrackPublicationsChanged" });
  });
  listen(record, events.TrackMuted, (publication, participant) => {
    if (participant === room.localParticipant) {
      queueEvent(record, { type: "localTrackPublicationsChanged" });
    } else if (remotePublicationIsCurrent(record, participant, publication)) {
      queueRemotePublicationsEvent(record, participant);
    }
  });
  listen(record, events.TrackUnmuted, (publication, participant) => {
    if (participant === room.localParticipant) {
      queueEvent(record, { type: "localTrackPublicationsChanged" });
    } else if (remotePublicationIsCurrent(record, participant, publication)) {
      queueRemotePublicationsEvent(record, participant);
    }
  });
  listen(record, events.ParticipantConnected, (participant) => {
    queueRemoteParticipantEvent(record, "remoteParticipantJoined", participant);
  });
  listen(record, events.ParticipantDisconnected, (participant) => {
    for (const streamId of [...record.outgoingDataStreamIds]) {
      const stream = outgoingDataStreams.get(streamId);
      if (stream?.destinationParticipant === participant) {
        terminateOutgoingDataStream(
          stream,
          flowControlError("DestinationUnavailableError", "Data Stream destination disconnected"),
          false,
        );
      }
    }
    for (const streamId of [...record.incomingDataStreamIds]) {
      const stream = incomingDataStreams.get(streamId);
      if (stream?.senderParticipant === participant) {
        cancelIncomingDataStream(stream, false, true);
      }
    }
    terminateRemoteMediaWhere(
      (track) => track.roomId === record.id && track.participant === participant,
    );
    queueRemoteParticipantEvent(record, "remoteParticipantLeft", participant);
  });
  listen(record, events.TrackPublished, (publication, participant) => {
    if (!remotePublicationIsCurrent(record, participant, publication)) return;
    queueRemotePublicationsEvent(record, participant);
  });
  listen(record, events.TrackUnpublished, (publication, participant) => {
    terminateRemoteMediaWhere(
      (track) =>
        track.roomId === record.id &&
        track.participant === participant &&
        track.publication === publication,
    );
    if (remotePublicationWasReplaced(record, participant, publication)) return;
    queueRemotePublicationsEvent(record, participant);
  });
  listen(record, events.TrackSubscribed, (_track, publication, participant) => {
    if (!remotePublicationIsCurrent(record, participant, publication)) return;
    reconcileRemoteMediaTracks(record);
    queueRemotePublicationsEvent(record, participant);
  });
  listen(record, events.TrackUnsubscribed, (track, publication, participant) => {
    terminateRemoteMediaWhere(
      (media) =>
        media.roomId === record.id &&
        media.participant === participant &&
        media.publication === publication &&
        media.track === track,
    );
    if (remotePublicationWasReplaced(record, participant, publication)) return;
    queueRemotePublicationsEvent(record, participant);
  });
  listen(record, events.TrackSubscriptionFailed, (_trackSid, participant) => {
    queueRemotePublicationsEvent(record, participant);
  });
  listen(record, events.TrackSubscriptionStatusChanged, (publication, _status, participant) => {
    if (!remotePublicationIsCurrent(record, participant, publication)) return;
    queueRemotePublicationsEvent(record, participant);
  });
  listen(record, events.DataTrackPublished, (track) => {
    const sid = track.info?.sid;
    if (typeof sid !== "string" || sid.length === 0) return;
    queueMicrotask(() => {
      if (record.released) return;
      const participant = room.remoteParticipants.get(track.publisherIdentity);
      if (!participant || dataTrackWithSid(participant, sid) !== track) return;
      record.remoteDataTracksBySid.set(sid, { participant, track });
      queueRemoteDataTracksEvent(record, participant);
    });
  });
  listen(record, events.DataTrackUnpublished, (sid) => {
    queueMicrotask(() => {
      if (record.released) return;
      const dataTrack = record.remoteDataTracksBySid.get(sid);
      if (!dataTrack) return;
      const currentParticipant = room.remoteParticipants.get(dataTrack.participant.identity);
      const currentTrack = dataTrackWithSid(currentParticipant, sid);
      if (currentTrack) {
        record.remoteDataTracksBySid.set(sid, {
          participant: currentParticipant,
          track: currentTrack,
        });
        return;
      }
      record.remoteDataTracksBySid.delete(sid);
      if (currentParticipant === dataTrack.participant) {
        queueRemoteDataTracksEvent(record, currentParticipant);
      }
    });
  });
  listen(record, events.DataReceived, (payload, participant, kind, topic) => {
    if (kind !== sdk.DataPacket_Kind.RELIABLE && kind !== sdk.DataPacket_Kind.LOSSY) return;
    if (
      handleFlowControl(
        record,
        payload,
        participant,
        kind === sdk.DataPacket_Kind.RELIABLE,
        topic,
      )
    ) {
      return;
    }
    record.dataPacketCallback?.(
      payload,
      typeof participant?.identity === "string" ? participant.identity : null,
      kind === sdk.DataPacket_Kind.RELIABLE,
    );
  });
  listen(record, events.ChatMessage, (message, participant) => {
    try {
      const normalized = normalizeChatMessage(
        message,
        typeof participant?.identity === "string" ? participant.identity : null,
      );
      record.chatMessageCallback?.(normalized);
    } catch {
      // Ignore malformed SDK events rather than exposing a fabricated typed Chat Message.
    }
  });
  if (typeof events.TranscriptionReceived === "string") {
    listen(record, events.TranscriptionReceived, (segments, participant, publication) => {
      if (
        !Array.isArray(segments) ||
        !transcriptionSourceIsCurrent(record, participant, publication)
      ) {
        return;
      }
      for (const segment of segments) {
        try {
          record.transcriptionSegmentCallback?.(
            normalizeTranscriptionSegment(segment, participant, publication),
          );
        } catch {
          // Ignore malformed SDK events rather than fabricating typed transcription data.
        }
      }
    });
  }
  listen(record, events.RoomMetadataChanged, () => {
    queueEvent(record, { type: "roomMetadataChanged" });
  });
  listen(record, events.AudioPlaybackStatusChanged, () => {
    queueEvent(record, {
      type: "audioPlaybackChanged",
      state: record.room.canPlaybackAudio === false ? "blocked" : "allowed",
    });
  });

  return record.id;
}

export async function connectRoom(roomId, serverUrl, participantToken, autoSubscribe, raw) {
  const record = roomSessions.get(roomId);
  if (!record) throw new Error("Room Session is no longer available");

  try {
    await record.room.connect(serverUrl, participantToken, { ...rawOptions(raw), autoSubscribe });
    record.roomSid = await record.room.getSid();
    record.pendingEvents.length = 0;
    return { snapshot: normalizeSnapshot(record) };
  } catch (error) {
    try {
      await releaseRoom(record);
    } catch {
      // Preserve the actionable join failure rather than a secondary cleanup failure.
    }
    throw error;
  }
}

export function activateRoom(
  roomId,
  callback,
  dataPacketCallback,
  chatMessageCallback,
  transcriptionSegmentCallback,
) {
  const record = roomSessions.get(roomId);
  if (!record) throw new Error("Room Session is no longer available");
  record.callback = callback;
  record.dataPacketCallback = dataPacketCallback;
  record.chatMessageCallback = chatMessageCallback;
  record.transcriptionSegmentCallback = transcriptionSegmentCallback;
  scheduleUpdate(record);
}

export function leaveRoom(roomId, onComplete) {
  const record = roomSessions.get(roomId);
  const operation = record ? releaseRoom(record) : Promise.resolve();
  return operation.finally(onComplete);
}

export function dropRoom(roomId) {
  const record = roomSessions.get(roomId);
  if (record) void releaseRoom(record).catch(() => {});
}

export function startRoomAudio(roomId) {
  const record = roomSessions.get(roomId);
  if (!record || record.released) return Promise.reject(new Error("Room Session is no longer available"));
  return Promise.resolve(record.room.startAudio());
}

const localClipRecordings = new Map();
let nextLocalClipRecordingId = 1;

const localClipNow = () => globalThis.performance.now();

export function localClipRecordingSupported() {
  return (
    typeof globalThis.MediaRecorder === "function" &&
    typeof globalThis.MediaRecorder.isTypeSupported === "function" &&
    typeof globalThis.MediaStream === "function" &&
    typeof globalThis.Blob === "function" &&
    typeof globalThis.URL?.createObjectURL === "function" &&
    typeof globalThis.URL?.revokeObjectURL === "function"
  );
}

export function selectLocalClipMimeType(preferences) {
  if (!localClipRecordingSupported()) return null;
  return preferences.find((preference) => globalThis.MediaRecorder.isTypeSupported(preference)) ?? null;
}

function localClipActiveDuration(record, at = localClipNow()) {
  return (
    record.activeDurationMs +
    (record.activeStartedAt == null ? 0 : Math.max(0, at - record.activeStartedAt))
  );
}

function pauseLocalClipDurationClock(record) {
  if (record.activeStartedAt == null) return;
  record.activeDurationMs = localClipActiveDuration(record);
  record.activeStartedAt = null;
}

function scheduleLocalClipDurationLimit(record) {
  if (record.maxDurationMs === 0 || record.activeStartedAt == null || record.stopReason) return;
  clearTimeout(record.durationTimer);
  const remaining = Math.max(0, record.maxDurationMs - localClipActiveDuration(record));
  record.durationTimer = setTimeout(
    () => {
      if (localClipActiveDuration(record) >= record.maxDurationMs) {
        requestLocalClipStop(record, "durationLimit");
      } else {
        scheduleLocalClipDurationLimit(record);
      }
    },
    Math.min(remaining, 2_147_483_647),
  );
}

function releaseLocalClipRecord(record) {
  clearTimeout(record.durationTimer);
  record.recorder.removeEventListener("dataavailable", record.onDataAvailable);
  record.recorder.removeEventListener("error", record.onError);
  record.recorder.removeEventListener("stop", record.onStop);
  localClipRecordings.delete(record.id);
}

function emitLocalClipFailure(record, error) {
  if (record.terminal) return;
  record.terminal = true;
  pauseLocalClipDurationClock(record);
  releaseLocalClipRecord(record);
  try {
    if (record.recorder.state !== "inactive") record.recorder.stop();
  } catch {}
  record.chunks = [];
  record.onEvent({ kind: "failed", error });
}

function requestLocalClipStop(record, reason) {
  if (record.terminal || record.stopReason) return;
  record.stopReason = reason;
  pauseLocalClipDurationClock(record);
  clearTimeout(record.durationTimer);
  record.onEvent({ kind: "stopping" });
  if (record.recorder.state !== "inactive") record.recorder.stop();
}

export function startLocalClipRecording(
  stream,
  selectedMimeType,
  maxDurationMs,
  maxBytes,
  onEvent,
) {
  if (!localClipRecordingSupported()) throw new Error("MediaRecorder is unavailable");

  const recorder = new MediaRecorder(stream, { mimeType: selectedMimeType });
  const id = nextLocalClipRecordingId++;
  const record = {
    id,
    recorder,
    onEvent,
    selectedMimeType,
    maxDurationMs,
    maxBytes,
    chunks: [],
    accumulatedBytes: 0,
    activeDurationMs: 0,
    activeStartedAt: localClipNow(),
    durationTimer: null,
    stopReason: null,
    terminal: false,
    onDataAvailable: null,
    onError: null,
    onStop: null,
  };

  record.onDataAvailable = (event) => {
    if (record.terminal || !(event.data instanceof Blob) || event.data.size === 0) return;
    record.chunks.push(event.data);
    record.accumulatedBytes += event.data.size;
    if (record.maxBytes > 0 && record.accumulatedBytes >= record.maxBytes) {
      requestLocalClipStop(record, "sizeLimit");
    }
  };
  record.onError = (event) => emitLocalClipFailure(record, event.error ?? event);
  record.onStop = () => {
    if (record.terminal) return;
    record.terminal = true;
    pauseLocalClipDurationClock(record);
    const durationMs = record.activeDurationMs;
    const firstChunkType = record.chunks.find((chunk) => chunk.type)?.type || "";
    const mimeType = recorder.mimeType || firstChunkType;
    const stopReason = record.stopReason || "sourceEnded";
    releaseLocalClipRecord(record);
    if (!mimeType) {
      const error = new Error("MediaRecorder did not report the artifact MIME type");
      error.name = "ContractError";
      onEvent({ kind: "failed", error });
    } else {
      const blob = new Blob(record.chunks, { type: mimeType });
      if (blob.size === 0) {
        onEvent({ kind: "empty" });
      } else {
        onEvent({
          kind: "completed",
          blob,
          mimeType,
          durationMs,
          sizeBytes: blob.size,
          stopReason,
        });
      }
    }
    record.chunks = [];
  };

  recorder.addEventListener("dataavailable", record.onDataAvailable);
  recorder.addEventListener("error", record.onError);
  recorder.addEventListener("stop", record.onStop);
  localClipRecordings.set(id, record);
  try {
    recorder.start(250);
    scheduleLocalClipDurationLimit(record);
  } catch (error) {
    releaseLocalClipRecord(record);
    throw error;
  }

  return { id, mimeType: recorder.mimeType };
}

function localClipRecording(recordingId) {
  const record = localClipRecordings.get(recordingId);
  if (!record || record.terminal) throw new Error("Local Clip Recording is no longer active");
  return record;
}

export function pauseLocalClipRecording(recordingId) {
  const record = localClipRecording(recordingId);
  if (record.recorder.state !== "recording") {
    throw new DOMException("Local Clip Recording is not recording", "InvalidStateError");
  }
  record.recorder.pause();
  pauseLocalClipDurationClock(record);
  clearTimeout(record.durationTimer);
}

export function resumeLocalClipRecording(recordingId) {
  const record = localClipRecording(recordingId);
  if (record.recorder.state !== "paused") {
    throw new DOMException("Local Clip Recording is not paused", "InvalidStateError");
  }
  record.recorder.resume();
  record.activeStartedAt = localClipNow();
  scheduleLocalClipDurationLimit(record);
}

export function stopLocalClipRecording(recordingId) {
  requestLocalClipStop(localClipRecording(recordingId), "explicit");
}

export function dropLocalClipRecording(recordingId) {
  const record = localClipRecordings.get(recordingId);
  if (!record) return;
  record.terminal = true;
  pauseLocalClipDurationClock(record);
  releaseLocalClipRecord(record);
  try {
    if (record.recorder.state !== "inactive") record.recorder.stop();
  } catch {}
  record.chunks = [];
}

export function createLocalClipObjectUrl(blob) {
  return URL.createObjectURL(blob);
}

export function revokeLocalClipObjectUrl(url) {
  URL.revokeObjectURL(url);
}
