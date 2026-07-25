import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const adapterSource = await readFile(
  new URL("./livekit-browser.js", import.meta.url),
  "utf8",
);
const adapter = await import(
  `data:text/javascript;base64,${Buffer.from(adapterSource).toString("base64")}`
);
const require = createRequire(import.meta.url);
const pinnedSdk = require(
  resolve(
    process.env.LIVEKIT_CLIENT_UMD ||
      fileURLToPath(
        new URL("./node_modules/livekit-client/dist/livekit-client.umd.js", import.meta.url),
      ),
  ),
);

const EXPECTED_EXPORTS = [
  "activateRoom", "attachLocalAudioTrack", "attachLocalVideoTrack", "attachRemoteAudioTrack",
  "attachRemoteVideoTrack", "closeAudioAnalyser", "completeOutgoingDataStream",
  "completeParticipantRpcInvocation", "connectRoom", "createCameraVideoTrack",
  "createLocalAudioAnalyser", "createLocalClipMediaStream", "createLocalClipObjectUrl",
  "createMicrophoneAudioTrack", "createRemoteAudioAnalyser", "createRoom", "createScreenCapture",
  "detachLocalAudioTrack", "detachLocalVideoTrack", "detachRemoteAudioTrack",
  "detachRemoteVideoTrack", "dropAudioAnalyser", "dropDataStreamHandler", "dropIncomingDataStream",
  "dropIncomingDataStreamDescriptor", "dropLocalAudioTrack", "dropLocalAudioTrackAttachment",
  "dropLocalClipRecording", "dropLocalVideoTrack", "dropLocalVideoTrackAttachment",
  "dropOutgoingDataStream", "dropOutgoingDataStreamDescriptor", "dropParticipantRpcInvocation",
  "dropParticipantRpcInvocationDescriptor", "dropParticipantRpcMethod",
  "dropRemoteAudioTrackAttachment", "dropRemoteMediaTrack", "dropRemoteMediaTrackDescriptor",
  "dropRemoteVideoTrackAttachment", "dropRoom", "enumerateMediaDevices", "finishIncomingDataStream",
  "inspectBrowserRuntime", "installLogExtension", "invokeParticipantRpc", "leaveRoom",
  "localAudioSenderStatistics", "localAudioTrackDeviceId", "localClipRecordingSupported",
  "localVideoSenderStatistics", "localVideoTrackDeviceId", "observeLocalAudioTrackEnded",
  "observeLocalVideoTrackEnded", "openByteStream", "openTextStream", "pauseLocalClipRecording",
  "publishLocalAudioTrack", "publishLocalVideoTrack", "rawLocalMediaTrack",
  "rawLocalMediaTrackProcessor", "rawLocalParticipant", "rawLocalPublication",
  "rawLocalRtcStatsReport", "rawRemoteMediaTrack", "rawRemoteParticipant", "rawRemotePublication",
  "rawRemoteRtcStatsReport", "rawRoom", "readAudioAnalyser", "readByteStream", "readTextStream",
  "registerByteStreamHandler", "registerParticipantRpcMethod", "registerTextStreamHandler",
  "remoteAudioReceiverStatistics", "remoteVideoReceiverStatistics", "resolveRemoteMediaTrack",
  "restartLocalAudioTrack", "restartLocalAudioTrackWithOptions", "restartLocalVideoTrack",
  "restartLocalVideoTrackWithOptions", "resumeLocalClipRecording", "revokeLocalClipObjectUrl",
  "selectLocalClipMimeType", "sendChatMessage", "sendDataPacket", "setLocalAudioTrackMuted",
  "setLocalMediaTrackProcessor", "setLocalParticipantMetadata", "setLocalVideoTrackMuted",
  "setRemoteTrackSubscription", "setTrackSubscriptionPermissions", "startLocalClipRecording",
  "startRoomAudio", "stopLocalAudioTrack", "stopLocalClipRecording", "stopLocalMediaTrackProcessor",
  "stopLocalVideoTrack", "switchLocalAudioTrackDevice", "switchLocalVideoTrackDevice",
  "unpublishLocalAudioTrack", "unpublishLocalVideoTrack", "updateLocalParticipantAttributes",
  "writeByteStream", "writeTextStream",
];

test("generated adapter preserves the Rust-facing export contract", () => {
  assert.deepEqual(Object.keys(adapter).sort(), EXPECTED_EXPORTS.sort());
});

const REQUIRED_ROOM_EVENTS = [
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
];

function contractClass(methods, staticMethods = []) {
  class Contract {}
  for (const method of methods) Contract.prototype[method] = () => {};
  for (const method of staticMethods) Contract[method] = () => {};
  return Contract;
}

function contractSdk() {
  const Room = contractClass(
    [
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
    ],
    ["getLocalDevices"],
  );
  const LocalParticipant = contractClass([
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
  const localTrackMethods = [
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
  ];
  const remoteTrackMethods = ["on", "off", "attach", "detach", "getReceiverStats"];
  return {
    version: "2.20.1",
    Room,
    LocalParticipant,
    LocalAudioTrack: contractClass(localTrackMethods),
    LocalVideoTrack: contractClass(localTrackMethods),
    LocalTrackPublication: class LocalTrackPublication {},
    RemoteParticipant: class RemoteParticipant {},
    RemoteAudioTrack: contractClass(remoteTrackMethods),
    RemoteDataTrack: class RemoteDataTrack {},
    RemoteVideoTrack: contractClass(remoteTrackMethods),
    RemoteTrackPublication: contractClass(["setSubscribed"]),
    RpcError: class RpcError {},
    isBrowserSupported: () => true,
    setLogExtension() {},
    createLocalAudioTrack() {},
    createLocalVideoTrack() {},
    createLocalScreenTracks() {},
    TrackEvent: { Ended: "ended" },
    RoomEvent: Object.fromEntries(REQUIRED_ROOM_EVENTS.map((event) => [event, event])),
    DataPacket_Kind: { RELIABLE: 0, LOSSY: 1 },
  };
}

function deletePath(target, path) {
  const parts = path.split(".");
  const property = parts.pop();
  let owner = target;
  for (const part of parts) owner = owner[part];
  delete owner[property];
}

test("preflights and preserves raw identity against the pinned LiveKit UMD", () => {
  globalThis.LivekitClient = pinnedSdk;

  assert.equal(pinnedSdk.version, "2.20.1");
  assert.doesNotThrow(() => adapter.inspectBrowserRuntime("2.20.1"));

  const roomId = adapter.createRoom({});
  const room = adapter.rawRoom(roomId);
  assert.equal(room instanceof pinnedSdk.Room, true);
  assert.equal(adapter.rawLocalParticipant(roomId), room.localParticipant);
  adapter.dropRoom(roomId);
});

test("preflights every inspectable LiveKit SDK contract before mutation", () => {
  globalThis.LivekitClient = contractSdk();
  assert.equal(adapter.inspectBrowserRuntime("2.20.1").status, "ready");

  const constructors = [
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
  ];
  const methodPaths = [
    "isBrowserSupported",
    "setLogExtension",
    "createLocalAudioTrack",
    "createLocalVideoTrack",
    "createLocalScreenTracks",
    "Room.getLocalDevices",
    ...[
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
    ].map((method) => `Room.prototype.${method}`),
    ...[
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
    ].map((method) => `LocalParticipant.prototype.${method}`),
    ...["LocalAudioTrack", "LocalVideoTrack"].flatMap((track) =>
      [
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
      ].map((method) => `${track}.prototype.${method}`),
    ),
    ...["RemoteAudioTrack", "RemoteVideoTrack"].flatMap((track) =>
      ["on", "off", "attach", "detach", "getReceiverStats"].map(
        (method) => `${track}.prototype.${method}`,
      ),
    ),
    "RemoteTrackPublication.prototype.setSubscribed",
  ];

  for (const constructor of constructors) {
    const sdk = contractSdk();
    delete sdk[constructor];
    globalThis.LivekitClient = sdk;
    assert.throws(
      () => adapter.inspectBrowserRuntime("2.20.1"),
      new RegExp(`LivekitClient\\.${constructor} is not a constructor`),
      constructor,
    );
  }
  for (const path of methodPaths) {
    const sdk = contractSdk();
    deletePath(sdk, path);
    globalThis.LivekitClient = sdk;
    const displayPath = path.replace(".prototype", "");
    assert.throws(
      () => adapter.inspectBrowserRuntime("2.20.1"),
      new RegExp(`LivekitClient\\.${displayPath.replaceAll(".", "\\.")} is not a function`),
      path,
    );
  }
  for (const event of REQUIRED_ROOM_EVENTS) {
    const sdk = contractSdk();
    delete sdk.RoomEvent[event];
    globalThis.LivekitClient = sdk;
    assert.throws(
      () => adapter.inspectBrowserRuntime("2.20.1"),
      new RegExp(`LivekitClient\\.RoomEvent\\.${event} is not a string`),
      event,
    );
  }

  const optionalTranscription = contractSdk();
  globalThis.LivekitClient = optionalTranscription;
  const inspected = adapter.inspectBrowserRuntime("2.20.1");
  assert.equal(inspected.status, "ready");
  assert.equal(inspected.capabilities.transcription, false);
});

test("normalizes LiveKit log levels and structured context for one extension", () => {
  let extension;
  globalThis.LivekitClient = {
    setLogExtension(callback) {
      assert.equal(extension, undefined);
      extension = callback;
    },
  };

  const records = [];
  adapter.installLogExtension((level, message, context) => {
    records.push({ level, message, context });
  });

  const context = { room: "RM_test", attempt: 2, nested: { active: true } };
  context.self = context;
  for (const level of [0, 1, 2, 3, 4]) extension(level, `level-${level}`, context);
  extension(5, "silent", { ignored: true });

  assert.deepEqual(
    records,
    [0, 1, 2, 3, 4].map((level) => ({
      level,
      message: `level-${level}`,
      context: {
        attempt: 2,
        nested: { active: true },
        room: "RM_test",
        self: "[Circular]",
      },
    })),
  );
});

test("raw camera options are additive while typed values retain precedence", async () => {
  let receivedOptions;
  let receivedAudioOptions;
  const track = {
    attach() {},
    detach() {},
    getDeviceId: async () => "camera",
    getSenderStats: async () => [],
    getProcessor: () => track.processor,
    getRTCStatsReport: async () => new Map([["codec", { mimeType: "video/VP8" }]]),
    on() {},
    off() {},
    restartTrack: async () => {},
    mute: async () => {},
    unmute: async () => {},
    setProcessor: async (processor) => {
      track.processor = processor;
    },
    stop() {},
    stopProcessor: async () => {
      track.processor = undefined;
    },
  };
  globalThis.LivekitClient = {
    TrackEvent: { Ended: "ended" },
    createLocalAudioTrack: async (options) => {
      receivedAudioOptions = options;
      return {
        attach() {},
        detach() {},
        getDeviceId: async () => "microphone",
        getSenderStats: async () => [],
        on() {},
        off() {},
        restartTrack: async () => {},
        mute: async () => {},
        unmute: async () => {},
        stop() {},
      };
    },
    createLocalVideoTrack: async (options) => {
      receivedOptions = options;
      return track;
    },
  };

  const trackId = await new Promise((resolve, reject) => {
    adapter.createCameraVideoTrack(
      { frameRate: 30 },
      { contentHint: "motion", frameRate: 1 },
      (success, value) => (success ? resolve(value) : reject(value)),
    );
  });

  assert.deepEqual(receivedOptions, { contentHint: "motion", frameRate: 30 });
  assert.equal(adapter.rawLocalMediaTrack(trackId, "video"), track);

  const processor = { name: "background-blur" };
  await adapter.setLocalMediaTrackProcessor(trackId, "video", processor, false);
  assert.equal(adapter.rawLocalMediaTrackProcessor(trackId, "video"), processor);
  await adapter.stopLocalMediaTrackProcessor(trackId, "video");
  assert.equal(adapter.rawLocalMediaTrackProcessor(trackId, "video"), null);

  await new Promise((resolve, reject) => {
    adapter.createMicrophoneAudioTrack(
      { echoCancellation: false, sampleRate: 48_000 },
      { noiseSuppression: true, sampleRate: 8_000 },
      (success, value) => (success ? resolve(value) : reject(value)),
    );
  });
  assert.deepEqual(receivedAudioOptions, {
    echoCancellation: false,
    noiseSuppression: true,
    sampleRate: 48_000,
  });
});

test("raw room, participant, publication, media, and statistics handles preserve identity", async () => {
  let roomConstructorOptions;
  let connectOptions;
  let publishOptions;
  const replacements = [];

  const remoteStats = new Map([["remote", { packetsReceived: 7 }]]);
  const remoteTrack = {
    kind: "video",
    mediaStreamTrack: {},
    getRTCStatsReport: async () => remoteStats,
    on() {},
    off() {},
  };
  const remotePublication = {
    trackSid: "TR_remote",
    trackName: "camera",
    source: "camera",
    kind: "video",
    isMuted: false,
    isSubscribed: true,
    track: remoteTrack,
  };
  const remoteDataTrack = {
    info: { sid: "DT_remote", name: "reactions", usesE2ee: true },
    publisherIdentity: "student",
  };
  const remoteParticipant = {
    identity: "student",
    sid: "PA_student",
    connectionQuality: "good",
    metadata: undefined,
    attributes: {},
    trackPublications: new Map([[remotePublication.trackSid, remotePublication]]),
    dataTracks: new Map([[remoteDataTrack.info.name, remoteDataTrack]]),
  };
  const localParticipant = {
    identity: "teacher",
    sid: "PA_teacher",
    connectionQuality: "excellent",
    metadata: undefined,
    attributes: {},
    permissions: {
      canSubscribe: true,
      canPublish: true,
      canPublishData: true,
      canPublishSources: [],
      canUpdateMetadata: true,
    },
    trackPublications: new Map(),
    publishTrack: async (_track, options) => {
      publishOptions = options;
      const publication = {
        trackSid: "TR_local",
        trackName: options.name,
        source: options.source,
        kind: "video",
        track: _track,
      };
      localParticipant.trackPublications.set(publication.trackSid, publication);
      return publication;
    },
    unpublishTrack: async () => {},
    setTrackSubscriptionPermissions() {},
    setMetadata: async () => {},
    setAttributes: async () => {},
    publishData: async () => {},
    sendChatMessage: async () => {},
    streamText: async () => {},
    streamBytes: async () => {},
    performRpc: async () => {},
  };

  class Room {
    constructor(options) {
      roomConstructorOptions = options;
      this.localParticipant = localParticipant;
      this.remoteParticipants = new Map([[remoteParticipant.identity, remoteParticipant]]);
      this.state = "connected";
      this.name = "diagnostics";
      this.metadata = undefined;
      this.canPlaybackAudio = true;
      this.activeSpeakers = [remoteParticipant, localParticipant, remoteParticipant];
      this.listeners = new Map();
    }

    on(event, listener) {
      if (typeof event === "string") this.listeners.set(event, listener);
    }
    off(event, listener) {
      if (this.listeners.get(event) === listener) this.listeners.delete(event);
    }
    emit(event, ...args) {
      this.listeners.get(event)?.(...args);
    }
    async connect(_url, _token, options) {
      connectOptions = options;
    }
    async disconnect() {}
    async startAudio() {}
    async getSid() {
      return "RM_diagnostics";
    }
    registerTextStreamHandler() {}
    unregisterTextStreamHandler() {}
    registerByteStreamHandler() {}
    unregisterByteStreamHandler() {}
    registerRpcMethod() {}
    unregisterRpcMethod() {}
    removeAllListeners() {
      this.listeners.clear();
    }
  }

  const localStats = new Map([["local", { packetsSent: 5 }]]);
  const localTrack = {
    attach() {},
    detach() {},
    getDeviceId: async () => "camera",
    getSenderStats: async () => [],
    kind: "video",
    getRTCStatsReport: async () => localStats,
    on() {},
    off() {},
    restartTrack: async () => {},
    mute: async () => {},
    unmute: async () => {},
    stop() {},
  };
  globalThis.LivekitClient = {
    Room,
    RoomEvent: {
      LocalTrackPublished: "localTrackPublished",
      LocalTrackUnpublished: "localTrackUnpublished",
    },
    TrackEvent: { Ended: "ended" },
    DataPacket_Kind: { RELIABLE: 0, LOSSY: 1 },
    createLocalVideoTrack: async () => localTrack,
  };

  const roomId = adapter.createRoom({ adaptiveStream: true });
  assert.deepEqual(roomConstructorOptions, { adaptiveStream: true });
  const joined = await adapter.connectRoom(
    roomId,
    "ws://example.test",
    "token",
    true,
    { autoSubscribe: false, websocketTimeout: 123 },
  );
  assert.deepEqual(connectOptions, { autoSubscribe: true, websocketTimeout: 123 });
  assert.deepEqual(joined.snapshot.active_speakers, ["student", "teacher"]);
  assert.equal(adapter.rawRoom(roomId) instanceof Room, true);
  assert.equal(adapter.rawLocalParticipant(roomId), localParticipant);

  const participantSnapshot = joined.snapshot.remote_participants.student;
  const participantLocator = {
    identity: participantSnapshot.identity,
    sid: participantSnapshot.sid,
    generation: participantSnapshot.generation,
  };
  assert.equal(adapter.rawRemoteParticipant(roomId, participantLocator), remoteParticipant);
  const publicationSnapshot = participantSnapshot.track_publications.TR_remote;
  const publicationLocator = {
    participant: participantLocator,
    sid: publicationSnapshot.sid,
    generation: publicationSnapshot.generation,
  };
  assert.equal(
    adapter.rawRemotePublication(roomId, publicationLocator),
    remotePublication,
  );
  assert.deepEqual(participantSnapshot.data_tracks, {
    DT_remote: { sid: "DT_remote", name: "reactions", uses_e2ee: true },
  });
  assert.equal(participantSnapshot.track_publications.DT_remote, undefined);
  assert.throws(() =>
    adapter.rawRemotePublication(roomId, {
      participant: participantLocator,
      sid: "DT_remote",
      generation: publicationSnapshot.generation,
    }),
  );
  const remoteDescriptor = adapter.resolveRemoteMediaTrack(
    roomId,
    publicationLocator,
    publicationSnapshot.media_generation,
    () => {},
  );
  assert.equal(adapter.rawRemoteMediaTrack(remoteDescriptor.id), remoteTrack);
  assert.equal(await adapter.rawRemoteRtcStatsReport(remoteDescriptor.id), remoteStats);

  const localTrackId = await new Promise((resolve, reject) => {
    adapter.createCameraVideoTrack({}, {}, (success, value) =>
      success ? resolve(value) : reject(value),
    );
  });
  await adapter.publishLocalVideoTrack(
    roomId,
    localTrackId,
    1,
    { source: "camera", name: "typed-camera", videoCodec: "vp8" },
    { source: "screen_share", degradationPreference: "maintain-framerate" },
    (previousId, sid, muted) => {
      replacements.push({ previousId, sid, muted });
      return previousId + 1;
    },
    () => {},
  );
  assert.deepEqual(publishOptions, {
    degradationPreference: "maintain-framerate",
    name: "typed-camera",
    source: "camera",
    videoCodec: "vp8",
  });
  assert.equal(adapter.rawLocalPublication(localTrackId, 1, "video").trackSid, "TR_local");
  assert.equal(await adapter.rawLocalRtcStatsReport(localTrackId, 1, "video"), localStats);

  const previousPublication = localParticipant.trackPublications.get("TR_local");
  const sdkRoom = adapter.rawRoom(roomId);
  sdkRoom.emit(globalThis.LivekitClient.RoomEvent.LocalTrackUnpublished, previousPublication);
  localParticipant.trackPublications.delete("TR_local");
  const replacementPublication = {
    ...previousPublication,
    trackSid: "TR_reconnected",
    isMuted: true,
  };
  localParticipant.trackPublications.set("TR_reconnected", replacementPublication);
  sdkRoom.emit(globalThis.LivekitClient.RoomEvent.LocalTrackPublished, replacementPublication);
  assert.deepEqual(replacements, [
    { previousId: 1, sid: null, muted: false },
    { previousId: 1, sid: "TR_reconnected", muted: true },
  ]);
  assert.throws(() => adapter.rawLocalPublication(localTrackId, 1, "video"));
  assert.equal(adapter.rawLocalPublication(localTrackId, 2, "video"), replacementPublication);

  const directReplacement = {
    ...replacementPublication,
    trackSid: "TR_direct_replacement",
    isMuted: false,
  };
  localParticipant.trackPublications.clear();
  localParticipant.trackPublications.set(directReplacement.trackSid, directReplacement);
  sdkRoom.emit(globalThis.LivekitClient.RoomEvent.LocalTrackPublished, directReplacement);
  assert.deepEqual(replacements.slice(-2), [
    { previousId: 2, sid: null, muted: false },
    { previousId: 2, sid: "TR_direct_replacement", muted: false },
  ]);
  assert.throws(() => adapter.rawLocalPublication(localTrackId, 2, "video"));
  assert.equal(adapter.rawLocalPublication(localTrackId, 3, "video"), directReplacement);

  const rpcCause = Object.assign(new Error("transport exploded"), { name: "NetworkSdkError" });
  localParticipant.performRpc = async () => {
    throw rpcCause;
  };
  const rpcFailure = await new Promise((resolve, reject) => {
    adapter.invokeParticipantRpc(
      roomId,
      {
        destinationIdentity: "student",
        method: "diagnose",
        payload: "request",
        responseTimeoutMs: 8_000,
      },
      (success, value) => (success ? reject(new Error("RPC unexpectedly succeeded")) : resolve(value)),
    );
  });
  assert.equal(rpcFailure.message, "transport exploded");
  assert.equal(rpcFailure.cause, rpcCause);

  localTrack.restartTrack = () => new Promise(() => {});
  adapter.restartLocalVideoTrack(localTrackId, () => {});
  await Promise.resolve();
  await adapter.leaveRoom(roomId, () => {});
  assert.throws(() => adapter.rawLocalPublication(localTrackId, 3, "video"));
});

test("validates constructor-installed Room methods before registering Room state", () => {
  class Room {
    constructor() {
      this.connect = async () => {};
      this.disconnect = async () => {};
    }
  }
  globalThis.LivekitClient = { Room };
  assert.throws(
    () => adapter.createRoom({}),
    /LivekitClient\.Room\.startAudio is not a function/,
  );
});
