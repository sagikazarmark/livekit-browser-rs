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

test("room events discard the initial burst, batch updates, and cancel delivery on disposal", async () => {
  const roomEvents = Object.fromEntries(REQUIRED_ROOM_EVENTS.map((event) => [event, event]));
  const localParticipant = {
    identity: "local",
    sid: "PA_local",
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
    publishTrack: async () => {},
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
    constructor() {
      this.localParticipant = localParticipant;
      this.remoteParticipants = new Map();
      this.activeSpeakers = [];
      this.state = "disconnected";
      this.canPlaybackAudio = true;
      this.listeners = new Map();
      Room.instance = this;
    }
    on(event, listener) {
      const listeners = this.listeners.get(event) || [];
      listeners.push(listener);
      this.listeners.set(event, listeners);
    }
    off(event, listener) {
      const listeners = this.listeners.get(event) || [];
      this.listeners.set(event, listeners.filter((candidate) => candidate !== listener));
    }
    emit(event, ...args) {
      for (const listener of this.listeners.get(event) || []) listener(...args);
    }
    async connect() {
      this.state = "connected";
      this.emit(roomEvents.ConnectionStateChanged, "connecting");
      this.emit(roomEvents.RoomMetadataChanged);
    }
    async disconnect() {}
    async startAudio() {}
    async getSid() { return "RM_events"; }
    registerTextStreamHandler() {}
    unregisterTextStreamHandler() {}
    registerByteStreamHandler() {}
    unregisterByteStreamHandler() {}
    registerRpcMethod() {}
    unregisterRpcMethod() {}
    removeAllListeners() { this.listeners.clear(); }
  }

  globalThis.LivekitClient = {
    Room,
    RoomEvent: roomEvents,
    DataPacket_Kind: { RELIABLE: 0, LOSSY: 1 },
  };

  const roomId = adapter.createRoom({});
  const directPackets = [];
  Room.instance.emit(roomEvents.DataReceived, new Uint8Array([1]), undefined, 1, "topic");
  const joined = await adapter.connectRoom(roomId, "ws://example.test", "token", true, {});
  assert.equal(joined.snapshot.revision, 1);

  const updates = [];
  adapter.activateRoom(
    roomId,
    (update) => updates.push(update),
    (...packet) => directPackets.push(packet),
    () => {},
    () => {},
  );
  await new Promise((resolve) => setTimeout(resolve, 5));
  assert.deepEqual(updates, []);
  assert.deepEqual(directPackets, []);

  Room.instance.emit(roomEvents.ConnectionStateChanged, "connected");
  Room.instance.emit(roomEvents.RoomMetadataChanged);
  await new Promise((resolve) => setTimeout(resolve, 5));
  assert.equal(updates.length, 1);
  assert.equal(updates[0].snapshot.revision, 2);
  assert.deepEqual(updates[0].events, [
    { type: "connectionStateChanged", state: "connected" },
    { type: "roomMetadataChanged" },
  ]);

  Room.instance.emit(roomEvents.AudioPlaybackStatusChanged);
  adapter.dropRoom(roomId);
  await new Promise((resolve) => setTimeout(resolve, 5));
  assert.equal(updates.length, 1);
  assert.equal([...Room.instance.listeners.values()].flat().length, 0);
  adapter.dropRoom(roomId);
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
  const screenTrack = { ...track, kind: "video" };
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
    createLocalScreenTracks: async () => [screenTrack],
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

  const screenCapture = await new Promise((resolve, reject) => {
    adapter.createScreenCapture(
      { audio: false },
      {},
      (success, value) => (success ? resolve(value) : reject(value)),
    );
  });
  assert.deepEqual(screenCapture, {
    video_track_id: 2,
    audio_track_id: null,
  });
});

test("raw room, participant, publication, media, and statistics handles preserve identity", async () => {
  let roomConstructorOptions;
  let connectOptions;
  let publishOptions;
  let rpcHandler;
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
  const unsupportedDataPublication = {
    trackSid: "TR_unsupported_data",
    trackName: "legacy-data-publication",
    source: "unknown",
    kind: "data",
    isMuted: false,
    isSubscribed: false,
    track: null,
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
    trackPublications: new Map([
      [remotePublication.trackSid, remotePublication],
      [unsupportedDataPublication.trackSid, unsupportedDataPublication],
    ]),
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
    registerRpcMethod(_method, handler) {
      rpcHandler = handler;
    }
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
      ConnectionQualityChanged: "connectionQualityChanged",
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
  assert.equal(Object.getPrototypeOf(joined.snapshot.remote_participants), null);
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
  assert.equal(participantSnapshot.track_publications.TR_unsupported_data.kind, "unknown");
  assert.equal(participantSnapshot.track_publications.DT_remote, undefined);
  assert.throws(() =>
    adapter.rawRemotePublication(roomId, {
      participant: participantLocator,
      sid: "DT_remote",
      generation: publicationSnapshot.generation,
    }),
  );

  const qualityUpdate = new Promise((resolve) => {
    adapter.activateRoom(roomId, resolve, () => {}, () => {}, () => {});
  });
  const sdkRoom = adapter.rawRoom(roomId);
  sdkRoom.emit(
    globalThis.LivekitClient.RoomEvent.ConnectionQualityChanged,
    "poor",
    remoteParticipant,
  );
  sdkRoom.remoteParticipants.set(remoteParticipant.identity, { ...remoteParticipant });
  assert.deepEqual((await qualityUpdate).events, []);
  sdkRoom.remoteParticipants.set(remoteParticipant.identity, remoteParticipant);

  const remoteDescriptor = adapter.resolveRemoteMediaTrack(
    roomId,
    publicationLocator,
    publicationSnapshot.media_generation,
    () => {},
  );
  assert.deepEqual(remoteDescriptor, {
    id: 1,
    kind: "video",
    generation: publicationSnapshot.media_generation,
  });
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
  const { cause: _rpcCause, ...rpcFailureContract } = rpcFailure;
  assert.deepEqual(rpcFailureContract, {
    terminal: false,
    permission_denied: false,
    code: null,
    message: "transport exploded",
    data: null,
  });

  let rpcInvocation;
  adapter.registerParticipantRpcMethod(roomId, "echo", (invocation) => {
    rpcInvocation = invocation;
  });
  const rpcResponse = rpcHandler({
    requestId: "request-1",
    callerIdentity: "student",
    payload: "hello",
    responseTimeout: 2_500,
  });
  assert.deepEqual(rpcInvocation, {
    id: 1,
    request_id: "request-1",
    caller_identity: "student",
    payload: "hello",
    response_timeout_ms: 2_500,
  });
  adapter.completeParticipantRpcInvocation(rpcInvocation.id, {
    success: true,
    payload: "world",
    code: null,
    message: null,
    data: null,
  });
  assert.equal(await rpcResponse, "world");

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

test("concurrent leave shares completion while local media is reusable", async () => {
  let releaseDisconnect;
  const disconnectStalled = new Promise((resolve) => { releaseDisconnect = resolve; });
  const rooms = [];
  let nextPublication = 1;

  class Room {
    constructor() {
      this.listeners = new Map();
      this.localParticipant = {
        identity: "local",
        permissions: { canPublish: true },
        trackPublications: new Map(),
        publishTrack: async (track) => {
          const publication = { track, trackSid: `TR_${nextPublication++}`, isMuted: false };
          this.localParticipant.trackPublications.set(publication.trackSid, publication);
          return publication;
        },
        unpublishTrack: async (track) => {
          for (const [sid, publication] of this.localParticipant.trackPublications) {
            if (publication.track === track) this.localParticipant.trackPublications.delete(sid);
          }
        },
        setTrackSubscriptionPermissions() {},
        setMetadata: async () => {},
        setAttributes: async () => {},
        publishData: async () => {},
        sendChatMessage: async () => {},
        streamText() {},
        streamBytes() {},
        performRpc: async () => {},
      };
      rooms.push(this);
    }
    on(event, listener) {
      const listeners = this.listeners.get(event) || [];
      listeners.push(listener);
      this.listeners.set(event, listeners);
    }
    off(event, listener) {
      const listeners = this.listeners.get(event) || [];
      this.listeners.set(event, listeners.filter((candidate) => candidate !== listener));
    }
    connect = async () => {};
    disconnect = () => this === rooms[0] ? disconnectStalled : Promise.resolve();
    startAudio = async () => {};
    getSid = async () => "RM_cleanup";
    registerTextStreamHandler() {}
    unregisterTextStreamHandler() {}
    registerByteStreamHandler() {}
    unregisterByteStreamHandler() {}
    registerRpcMethod() {}
    unregisterRpcMethod() {}
    removeAllListeners() { this.listeners.clear(); }
  }

  const localTrack = {
    kind: "video",
    on() {},
    off() {},
    attach() {},
    detach() {},
    stop() {},
    getDeviceId: async () => "camera",
    restartTrack: async () => {},
    mute: async () => {},
    unmute: async () => {},
    getSenderStats: async () => [],
  };
  globalThis.LivekitClient = {
    Room,
    RoomEvent: Object.fromEntries(REQUIRED_ROOM_EVENTS.map((event) => [event, event])),
    TrackEvent: { Ended: "ended" },
    DataPacket_Kind: { RELIABLE: 0, LOSSY: 1 },
    createLocalVideoTrack: async () => localTrack,
  };

  const oldRoomId = adapter.createRoom({});
  const newRoomId = adapter.createRoom({});
  const trackId = await new Promise((resolve, reject) => {
    adapter.createCameraVideoTrack({}, {}, (success, value) =>
      success ? resolve(value) : reject(value),
    );
  });
  await adapter.publishLocalVideoTrack(
    oldRoomId, trackId, 1, {}, {}, () => 2, () => {},
  );

  let secondLeaveCompleted = false;
  const firstLeave = adapter.leaveRoom(oldRoomId, () => {});
  const secondLeave = adapter.leaveRoom(oldRoomId, () => { secondLeaveCompleted = true; });
  await Promise.resolve();

  let reuseFailure = null;
  try {
    await adapter.publishLocalVideoTrack(
      newRoomId, trackId, 2, {}, {}, () => 3, () => {},
    );
  } catch (error) {
    reuseFailure = error;
  }

  const secondLeaveCompletedBeforeDisconnect = secondLeaveCompleted;
  releaseDisconnect();
  await Promise.all([firstLeave, secondLeave]);
  adapter.dropRoom(newRoomId);

  assert.equal(
    secondLeaveCompletedBeforeDisconnect,
    false,
    "a concurrent leave must wait for disconnect",
  );
  assert.equal(reuseFailure, null, "local media must be reusable before disconnect completes");
});

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function installReleaseTestSdk(overrides = {}) {
  const rooms = [];
  let nextPublication = 1;
  const track = {
    kind: "video",
    on() {},
    off() {},
    attach() {},
    detach() {},
    stop() {},
    getDeviceId: async () => "camera",
    restartTrack: async () => {},
    mute: async () => {},
    unmute: async () => {},
    getSenderStats: async () => [],
  };

  class Room {
    constructor() {
      this.listeners = new Map();
      this.remoteParticipants = new Map([["remote", { identity: "remote" }]]);
      this.localParticipant = {
        identity: "local",
        permissions: { canPublish: true, canPublishData: true },
        trackPublications: new Map(),
        publishTrack: async (mediaTrack) => {
          if (overrides.publishTrack) return overrides.publishTrack.call(this, mediaTrack);
          const publication = {
            track: mediaTrack,
            trackSid: `TR_${nextPublication++}`,
            isMuted: false,
          };
          this.localParticipant.trackPublications.set(publication.trackSid, publication);
          return publication;
        },
        unpublishTrack: async (mediaTrack) => {
          for (const [sid, publication] of this.localParticipant.trackPublications) {
            if (publication.track === mediaTrack) this.localParticipant.trackPublications.delete(sid);
          }
        },
        setTrackSubscriptionPermissions() {},
        setMetadata: async () => {},
        setAttributes: async () => {},
        publishData: async () => {},
        sendChatMessage: async () => {},
        streamText: (...args) => overrides.streamText?.call(this, ...args),
        streamBytes: (...args) => overrides.streamBytes?.call(this, ...args),
        performRpc: (...args) => overrides.performRpc?.call(this, ...args),
      };
      rooms.push(this);
    }
    on(event, listener) {
      const listeners = this.listeners.get(event) || [];
      listeners.push(listener);
      this.listeners.set(event, listeners);
    }
    off(event, listener) {
      const listeners = this.listeners.get(event) || [];
      this.listeners.set(event, listeners.filter((candidate) => candidate !== listener));
    }
    emit(event, ...args) {
      for (const listener of this.listeners.get(event) || []) listener(...args);
    }
    connect(...args) { return overrides.connect?.call(this, ...args) ?? Promise.resolve(); }
    disconnect(...args) { return overrides.disconnect?.call(this, ...args) ?? Promise.resolve(); }
    startAudio = async () => {};
    getSid(...args) { return overrides.getSid?.call(this, ...args) ?? Promise.resolve("RM_release"); }
    registerTextStreamHandler() {}
    unregisterTextStreamHandler(...args) { return overrides.unregisterTextStreamHandler?.call(this, ...args); }
    registerByteStreamHandler() {}
    unregisterByteStreamHandler(...args) { return overrides.unregisterByteStreamHandler?.call(this, ...args); }
    registerRpcMethod() {}
    unregisterRpcMethod(...args) { return overrides.unregisterRpcMethod?.call(this, ...args); }
    removeAllListeners() { this.listeners.clear(); }
  }

  globalThis.LivekitClient = {
    Room,
    RoomEvent: Object.fromEntries(REQUIRED_ROOM_EVENTS.map((event) => [event, event])),
    TrackEvent: { Ended: "ended" },
    DataPacket_Kind: { RELIABLE: 0, LOSSY: 1 },
    RpcError: class RpcError extends Error {},
    createLocalVideoTrack: async () => track,
  };
  return { rooms, track };
}

test("local media remains reusable after old room disconnect rejects", async () => {
  const disconnectCause = new Error("old room disconnect failed");
  let environment;
  environment = installReleaseTestSdk({
    disconnect() {
      return this === environment.rooms[0]
        ? Promise.reject(disconnectCause)
        : Promise.resolve();
    },
  });
  const oldRoomId = adapter.createRoom({});
  const newRoomId = adapter.createRoom({});
  const trackId = await new Promise((resolve, reject) => {
    adapter.createCameraVideoTrack({}, {}, (success, value) =>
      success ? resolve(value) : reject(value),
    );
  });

  await adapter.publishLocalVideoTrack(
    oldRoomId, trackId, 1, {}, {}, () => 2, () => {},
  );
  await assert.rejects(adapter.leaveRoom(oldRoomId, () => {}), (error) => {
    assert.equal(error, disconnectCause);
    return true;
  });

  await assert.doesNotReject(() => adapter.publishLocalVideoTrack(
    newRoomId, trackId, 2, {}, {}, () => 3, () => {},
  ));
  assert.throws(() => adapter.rawLocalPublication(trackId, 1, "video"));
  assert.equal(
    adapter.rawLocalPublication(trackId, 2, "video"),
    environment.rooms[1].localParticipant.trackPublications.values().next().value,
  );

  adapter.dropRoom(newRoomId);
});

test("reentrant room drop during local publication replacement leaves the track reusable", async () => {
  const { rooms, track } = installReleaseTestSdk();
  const oldRoomId = adapter.createRoom({});
  const newRoomId = adapter.createRoom({});
  const trackId = await new Promise((resolve, reject) => {
    adapter.createCameraVideoTrack({}, {}, (success, value) =>
      success ? resolve(value) : reject(value),
    );
  });

  await adapter.publishLocalVideoTrack(
    oldRoomId,
    trackId,
    1,
    {},
    {},
    (publicationId, sid) => {
      if (sid === null) adapter.dropRoom(oldRoomId);
      return publicationId + 1;
    },
    () => {},
  );
  const replacement = { track, trackSid: "TR_replacement", isMuted: false };

  assert.doesNotThrow(() => rooms[0].emit("LocalTrackPublished", replacement));
  await assert.doesNotReject(() => adapter.publishLocalVideoTrack(
    newRoomId, trackId, 2, {}, {}, () => 3, () => {},
  ));

  adapter.dropRoom(newRoomId);
});

test("reentrant room leave during local unpublication leaves the track reusable", async () => {
  const { rooms } = installReleaseTestSdk();
  const oldRoomId = adapter.createRoom({});
  const newRoomId = adapter.createRoom({});
  const trackId = await new Promise((resolve, reject) => {
    adapter.createCameraVideoTrack({}, {}, (success, value) =>
      success ? resolve(value) : reject(value),
    );
  });
  let leave;

  await adapter.publishLocalVideoTrack(
    oldRoomId,
    trackId,
    1,
    {},
    {},
    (publicationId, sid) => {
      if (sid === null) leave = adapter.leaveRoom(oldRoomId, () => {});
      return publicationId + 1;
    },
    () => {},
  );
  const publication = rooms[0].localParticipant.trackPublications.values().next().value;

  assert.doesNotThrow(() => rooms[0].emit("LocalTrackUnpublished", publication));
  await leave;
  await assert.doesNotReject(() => adapter.publishLocalVideoTrack(
    newRoomId, trackId, 2, {}, {}, () => 3, () => {},
  ));

  adapter.dropRoom(newRoomId);
});

test("connectRoom becomes terminal when room release starts across either join await", async () => {
  for (const pendingStage of ["connect", "getSid"]) {
    const pending = deferred();
    const reachedGetSid = deferred();
    installReleaseTestSdk({
      connect: () => pendingStage === "connect" ? pending.promise : Promise.resolve(),
      getSid: () => {
        reachedGetSid.resolve();
        return pendingStage === "getSid" ? pending.promise : Promise.resolve("RM_joined");
      },
    });
    const roomId = adapter.createRoom({});
    const joining = adapter.connectRoom(roomId, "ws://example.test", "token", true, {});
    let leaving;

    if (pendingStage === "getSid") await reachedGetSid.promise;
    if (pendingStage === "connect") adapter.dropRoom(roomId);
    else leaving = adapter.leaveRoom(roomId, () => {});
    pending.resolve(pendingStage === "getSid" ? "RM_late" : undefined);

    await assert.rejects(joining, /Room Session is no longer available/, pendingStage);
    if (leaving) await leaving;
    assert.throws(() => adapter.rawRoom(roomId), /Room Session is no longer available/);
  }
});

test("room release settles every pending data stream open when one callback throws", async () => {
  const nativeOpen = deferred();
  installReleaseTestSdk({ streamText: () => nativeOpen.promise });
  const roomId = adapter.createRoom({});
  const firstFailure = new Error("first data stream release callback failed");
  const settled = [];

  adapter.openTextStream(
    roomId,
    { destinationIdentity: "remote", topic: "first" },
    (success) => {
      settled.push(["first", success]);
      throw firstFailure;
    },
  );
  adapter.openTextStream(
    roomId,
    { destinationIdentity: "remote", topic: "second" },
    (success) => settled.push(["second", success]),
  );
  await Promise.resolve();

  const releaseFailure = await adapter.leaveRoom(roomId, () => {}).then(
    () => null,
    (error) => error,
  );
  const settledDuringRelease = [...settled];
  nativeOpen.resolve({ info: { id: "native-late" }, write: async () => {}, close: async () => {} });
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(releaseFailure, firstFailure);
  assert.deepEqual(settledDuringRelease, [
    ["first", false],
    ["second", false],
  ]);
});

test("room release settles every pending participant RPC when one callback throws", async () => {
  const nativeCalls = [deferred(), deferred()];
  installReleaseTestSdk({ performRpc: () => nativeCalls.shift().promise });
  const roomId = adapter.createRoom({});
  const firstFailure = new Error("first participant RPC release callback failed");
  const settled = [];
  const options = {
    destinationIdentity: "remote",
    method: "pending",
    payload: "request",
    responseTimeoutMs: 1_000,
  };

  adapter.invokeParticipantRpc(roomId, options, (success) => {
    settled.push(["first", success]);
    throw firstFailure;
  });
  adapter.invokeParticipantRpc(roomId, options, (success) => {
    settled.push(["second", success]);
  });

  const releaseFailure = await adapter.leaveRoom(roomId, () => {}).then(
    () => null,
    (error) => error,
  );

  assert.equal(releaseFailure, firstFailure);
  assert.deepEqual(settled, [
    ["first", false],
    ["second", false],
  ]);
});

test("late old publication cannot replace a newer pending association or bypass serialization", async () => {
  const oldNativePublish = deferred();
  const newNativePublish = deferred();
  const oldPublishStarted = deferred();
  const newPublishStarted = deferred();
  const oldDisconnect = deferred();
  let environment;
  environment = installReleaseTestSdk({
    publishTrack(mediaTrack) {
      if (this === environment.rooms[0]) {
        oldPublishStarted.resolve();
        return oldNativePublish.promise;
      }
      newPublishStarted.resolve();
      return newNativePublish.promise;
    },
    disconnect() {
      return this === environment.rooms[0] ? oldDisconnect.promise : Promise.resolve();
    },
  });
  const oldRoomId = adapter.createRoom({});
  const newRoomId = adapter.createRoom({});
  const thirdRoomId = adapter.createRoom({});
  const trackId = await new Promise((resolve, reject) => {
    adapter.createCameraVideoTrack({}, {}, (success, value) =>
      success ? resolve(value) : reject(value),
    );
  });

  const oldPublishing = adapter.publishLocalVideoTrack(
    oldRoomId, trackId, 1, {}, {}, () => 2, () => {},
  );
  const oldOutcome = assert.rejects(oldPublishing, /Room Session is no longer available/);
  await oldPublishStarted.promise;
  const leaving = adapter.leaveRoom(oldRoomId, () => {});
  const newPublishing = adapter.publishLocalVideoTrack(
    newRoomId, trackId, 2, {}, {}, () => 3, () => {},
  );
  let newStarted = false;
  void newPublishStarted.promise.then(() => { newStarted = true; });
  await Promise.resolve();
  assert.equal(newStarted, false, "the newer publish must remain serialized behind the old publish");

  const oldPublication = { track: environment.track, trackSid: "TR_old_late", isMuted: false };
  environment.rooms[0].emit("LocalTrackPublished", oldPublication);
  oldNativePublish.resolve(oldPublication);
  await oldOutcome;
  await newPublishStarted.promise;
  await assert.rejects(
    adapter.publishLocalVideoTrack(thirdRoomId, trackId, 3, {}, {}, () => 4, () => {}),
    /already has a pending publication/,
  );

  const newPublication = { track: environment.track, trackSid: "TR_new", isMuted: false };
  newNativePublish.resolve(newPublication);
  await newPublishing;
  assert.equal(adapter.rawLocalPublication(trackId, 2, "video"), newPublication);

  oldDisconnect.resolve();
  await leaving;
  adapter.dropRoom(newRoomId);
  adapter.dropRoom(thirdRoomId);
});
