import { requireMethods } from "./runtime";
import {
  rawOptions,
} from "./options";
import { createDataStreamSubsystem } from "./data-streams";
import { createParticipantRpcSubsystem } from "./participant-rpc";
import { createLocalMediaSubsystem } from "./local-media";
import { createRemoteMediaSubsystem } from "./remote-media";
import { createSnapshotNormalizer } from "./snapshots";
import { createRoomEventOrchestrator, normalizeChatMessage } from "./room-events";
import type {
  AdapterJoin,
  AdapterRoomSnapshot as RoomSnapshot,
  AdapterUpdate,
  PublicationSubscriptionPermissions,
} from "./generated/adapter-contracts";
import {
  createAudioAnalyser,
  createAudioAnalyserOwner,
  mediaStreamAudioSource,
  terminateAudioAnalyserOwner,
} from "./audio-analysers";
import type {
  LocalParticipant,
  Room,
} from "livekit-client";

export { enumerateMediaDevices } from "./devices";
export { installLogExtension } from "./logging";
export { inspectBrowserRuntime } from "./runtime";
export { closeAudioAnalyser, dropAudioAnalyser, readAudioAnalyser } from "./audio-analysers";
export * from "./recording";

type RoomId = number;
type AttachmentId = number;

type RoomUpdateCallback = (
  update: AdapterUpdate,
) => void;
type DataPacketCallback = (
  payload: Uint8Array,
  participantIdentity: string | null,
  reliable: boolean,
) => void;
type MessageCallback = (message: unknown) => void;
type RoomEvents = ReturnType<typeof createRoomEventOrchestrator<RoomSnapshot>>;

interface RoomRecord {
  id: RoomId;
  room: Room;
  roomSid: string;
  events: RoomEvents | null;
  localParticipantMutation: Promise<unknown>;
  reliableDataMutation: Promise<unknown>;
  released: boolean;
  releaseOperation: Promise<void> | null;
}

let nextRoomId: RoomId = 1;
const roomSessions = new Map<RoomId, RoomRecord>();
let nextAttachmentId: AttachmentId = 1;

function allocateAttachmentId() {
  return nextAttachmentId++;
}

const audioAnalyserSubsystem = {
  createAudioAnalyser,
  createAudioAnalyserOwner,
  mediaStreamAudioSource,
  terminateAudioAnalyserOwner,
};

const localMedia = createLocalMediaSubsystem({
  getRoom: (roomId) => roomSessions.get(roomId),
  allocateAttachmentId,
  audioAnalysers: audioAnalyserSubsystem,
});

const remoteMedia = createRemoteMediaSubsystem({
  getRoom: (roomId) => roomSessions.get(roomId),
  allocateAttachmentId,
  audioAnalysers: audioAnalyserSubsystem,
});

export const {
  observeLocalVideoTrackEnded,
  observeLocalAudioTrackEnded,
  createLocalClipMediaStream,
  rawLocalMediaTrack,
  rawLocalPublication,
  setLocalMediaTrackProcessor,
  rawLocalMediaTrackProcessor,
  stopLocalMediaTrackProcessor,
  createLocalAudioAnalyser,
  createCameraVideoTrack,
  localVideoTrackDeviceId,
  restartLocalVideoTrack,
  restartLocalVideoTrackWithOptions,
  switchLocalVideoTrackDevice,
  attachLocalVideoTrack,
  detachLocalVideoTrack,
  stopLocalVideoTrack,
  dropLocalVideoTrackAttachment,
  dropLocalVideoTrack,
  createMicrophoneAudioTrack,
  createScreenCapture,
  localAudioTrackDeviceId,
  restartLocalAudioTrack,
  restartLocalAudioTrackWithOptions,
  switchLocalAudioTrackDevice,
  attachLocalAudioTrack,
  detachLocalAudioTrack,
  stopLocalAudioTrack,
  dropLocalAudioTrackAttachment,
  dropLocalAudioTrack,
  publishLocalAudioTrack,
  setLocalAudioTrackMuted,
  unpublishLocalAudioTrack,
  localAudioSenderStatistics,
  rawLocalRtcStatsReport,
  publishLocalVideoTrack,
  setLocalVideoTrackMuted,
  unpublishLocalVideoTrack,
  localVideoSenderStatistics,
} = localMedia;

const {
  participantGeneration: remoteParticipantGeneration,
  publicationGeneration: remotePublicationGeneration,
  mediaGeneration: remoteMediaGeneration,
  participantIsCurrent: remoteParticipantIsCurrent,
  publicationIsCurrent: remotePublicationIsCurrent,
  publicationWasReplaced: remotePublicationWasReplaced,
  reconcileRoom: reconcileRemoteMediaTracks,
  rawParticipant: rawRemoteParticipant,
  rawPublication: rawRemotePublication,
  rawMediaTrack: rawRemoteMediaTrack,
  setTrackSubscription: setRemoteTrackSubscription,
  resolveMediaTrack: resolveRemoteMediaTrack,
  audioReceiverStatistics: remoteAudioReceiverStatistics,
  videoReceiverStatistics: remoteVideoReceiverStatistics,
  rawRtcStatsReport: rawRemoteRtcStatsReport,
  attachVideoTrack: attachRemoteVideoTrack,
  detachVideoTrack: detachRemoteVideoTrack,
  dropVideoTrackAttachment: dropRemoteVideoTrackAttachment,
  attachAudioTrack: attachRemoteAudioTrack,
  detachAudioTrack: detachRemoteAudioTrack,
  dropAudioTrackAttachment: dropRemoteAudioTrackAttachment,
  dropMediaTrack: dropRemoteMediaTrack,
  dropMediaTrackDescriptor: dropRemoteMediaTrackDescriptor,
  createRemoteAudioAnalyser,
} = remoteMedia;

const snapshots = createSnapshotNormalizer({
  localMedia: {
    reconcileSnapshot: localMedia.reconcileSnapshot,
  },
  remoteMedia: {
    reconcileRoom: reconcileRemoteMediaTracks,
    participantGeneration: remoteParticipantGeneration,
    publicationGeneration: remotePublicationGeneration,
    mediaGeneration: remoteMediaGeneration,
  },
});

export {
  rawRemoteParticipant,
  rawRemotePublication,
  rawRemoteMediaTrack,
  setRemoteTrackSubscription,
  resolveRemoteMediaTrack,
  remoteAudioReceiverStatistics,
  remoteVideoReceiverStatistics,
  rawRemoteRtcStatsReport,
  attachRemoteVideoTrack,
  detachRemoteVideoTrack,
  dropRemoteVideoTrackAttachment,
  attachRemoteAudioTrack,
  detachRemoteAudioTrack,
  dropRemoteAudioTrackAttachment,
  dropRemoteMediaTrack,
  dropRemoteMediaTrackDescriptor,
  createRemoteAudioAnalyser,
};

export function rawRoom(roomId: RoomId): Room {
  const record = roomSessions.get(roomId);
  if (!record || record.released) throw new Error("Room Session is no longer available");
  return record.room;
}

export function rawLocalParticipant(roomId: RoomId): LocalParticipant {
  return rawRoom(roomId).localParticipant;
}

export function setTrackSubscriptionPermissions(
  roomId: RoomId,
  permissions: PublicationSubscriptionPermissions,
) {
  const roomRecord = roomSessions.get(roomId);
  if (!roomRecord || roomRecord.released) throw new Error("Room Session is no longer available");
  const participant = roomRecord.room.localParticipant;
  if (participant.permissions?.canPublish !== true) {
    const error = new Error("LiveKit source grants do not permit publishing");
    error.name = "PermissionDeniedError";
    (error as Error & { status: number }).status = 403;
    throw error;
  }

  const participantPermissions = permissions.participantIdentities.map(
    (participantIdentity) => ({ participantIdentity, allowAll: true }),
  );
  participant.setTrackSubscriptionPermissions(
    permissions.allParticipantsAllowed,
    participantPermissions,
  );
}

function enqueueLocalParticipantMutation<T>(
  roomRecord: RoomRecord,
  mutation: (participant: LocalParticipant) => T | PromiseLike<T>,
): Promise<T> {
  const operation = roomRecord.localParticipantMutation.then(() => {
    if (roomRecord.released) throw new Error("Room Session is no longer available");
    return mutation(roomRecord.room.localParticipant);
  });
  roomRecord.localParticipantMutation = operation.catch(() => {});
  return operation;
}

function requireLocalParticipantMetadataPermission(participant: LocalParticipant) {
  if (participant.permissions?.canUpdateMetadata === true) return;
  const error = new Error("LiveKit source grants do not permit participant metadata updates");
  error.name = "PermissionDeniedError";
  (error as Error & { status: number }).status = 403;
  throw error;
}

export function setLocalParticipantMetadata(roomId: RoomId, metadata: string) {
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

export function updateLocalParticipantAttributes(
  roomId: RoomId,
  attributes: Record<string, string>,
) {
  const roomRecord = roomSessions.get(roomId);
  if (!roomRecord || roomRecord.released) throw new Error("Room Session is no longer available");
  return enqueueLocalParticipantMutation(roomRecord, (participant) => {
    requireLocalParticipantMetadataPermission(participant);
    return participant.setAttributes(attributes);
  });
}

function requireDataPublishing(participant: LocalParticipant) {
  if (participant.permissions?.canPublishData === false) {
    const error = new Error("LiveKit source grants do not permit publishing data");
    error.name = "PermissionDeniedError";
    (error as Error & { status: number }).status = 403;
    throw error;
  }
}

function enqueueReliableDataMutation<T>(
  roomRecord: RoomRecord,
  mutation: () => T | PromiseLike<T>,
): Promise<T> {
  const operation = roomRecord.reliableDataMutation.then(mutation, mutation);
  roomRecord.reliableDataMutation = operation.catch(() => {});
  return operation;
}

const RELIABLE_DATA_OPERATION_TIMEOUT_MS = 30_000;

function reliableDataTimeoutError(message: string) {
  const error = new Error(message);
  error.name = "FlowControlTimeoutError";
  (error as Error & { reasonName: string }).reasonName = "FlowControlTimeout";
  return error;
}

function enqueueBoundedReliableDataMutation(
  roomRecord: RoomRecord,
  mutation: () => unknown | PromiseLike<unknown>,
  timeoutMessage: string,
) {
  return enqueueReliableDataMutation(roomRecord, () => {
    let timer: ReturnType<typeof setTimeout>;
    let operation: Promise<unknown>;
    try {
      operation = Promise.resolve(mutation());
    } catch (error: unknown) {
      operation = Promise.reject(error);
    }
    const timeout = new Promise<never>((_, reject) => {
      timer = setTimeout(
        () => reject(reliableDataTimeoutError(timeoutMessage)),
        RELIABLE_DATA_OPERATION_TIMEOUT_MS,
      );
    });
    return Promise.race([operation, timeout]).finally(() => clearTimeout(timer));
  });
}

export function sendDataPacket(roomId: RoomId, payload: ArrayLike<number>, reliable: boolean) {
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

export function sendChatMessage(roomId: RoomId, message: string) {
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

const dataStreams = createDataStreamSubsystem({
  getRoom: (roomId) => roomSessions.get(roomId),
  requireDataPublishing,
  enqueueReliableMutation: enqueueReliableDataMutation,
});

export const {
  openTextStream,
  openByteStream,
  writeTextStream,
  writeByteStream,
  completeOutgoingDataStream,
  dropOutgoingDataStream,
  dropOutgoingDataStreamDescriptor,
  registerTextStreamHandler,
  registerByteStreamHandler,
  dropDataStreamHandler,
  readTextStream,
  readByteStream,
  finishIncomingDataStream,
  dropIncomingDataStream,
  dropIncomingDataStreamDescriptor,
} = dataStreams;

const participantRpc = createParticipantRpcSubsystem({
  getRoom: (roomId) => roomSessions.get(roomId),
  requireDataPublishing,
});

export const {
  invokeParticipantRpc,
  registerParticipantRpcMethod,
  dropParticipantRpcMethod,
  completeParticipantRpcInvocation,
  dropParticipantRpcInvocation,
  dropParticipantRpcInvocationDescriptor,
} = participantRpc;

function releaseDataStreams(record: RoomRecord) {
  dataStreams.releaseRoom(record);
}

function releaseParticipantRpc(record: RoomRecord) {
  participantRpc.releaseRoom(record);
}

function releaseRoom(record: RoomRecord): Promise<void> {
  if (record.releaseOperation) return record.releaseOperation;
  record.released = true;
  let resolveRelease!: () => void;
  let rejectRelease!: (error: unknown) => void;
  const operation = new Promise<void>((resolve, reject) => {
    resolveRelease = resolve;
    rejectRelease = reject;
  });
  record.releaseOperation = operation;

  void (async () => {
    let failure: unknown;
    let failed = false;
    const recordFailure = (error: unknown) => {
      if (failed) return;
      failure = error;
      failed = true;
    };
    const cleanup = (callback: () => void) => {
      try {
        callback();
      } catch (error: unknown) {
        recordFailure(error);
      }
    };

    cleanup(() => record.events?.dispose());
    cleanup(() => localMedia.releaseRoom(record.id));
    cleanup(() => releaseDataStreams(record));
    cleanup(() => releaseParticipantRpc(record));
    cleanup(() => remoteMedia.releaseRoom(record.id));

    try {
      await record.room.disconnect(false);
    } catch (error: unknown) {
      recordFailure(error);
    } finally {
      roomSessions.delete(record.id);
      cleanup(() => record.room.removeAllListeners());
    }

    if (failed) throw failure;
  })().then(resolveRelease, rejectRelease);

  return operation;
}

export function createRoom(raw: unknown) {
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
  } catch (error: unknown) {
    try {
      room.removeAllListeners?.();
      void Promise.resolve(room.disconnect?.(false)).catch(() => {});
    } catch {}
    throw error;
  }
  const record: RoomRecord = {
    id: nextRoomId++,
    room,
    roomSid: "",
    events: null,
    localParticipantMutation: Promise.resolve(),
    reliableDataMutation: Promise.resolve(),
    released: false,
    releaseOperation: null,
  };
  roomSessions.set(record.id, record);

  try {
    record.events = createRoomEventOrchestrator({
      room,
      roomEvents: sdk.RoomEvent,
      dataPacketKinds: sdk.DataPacket_Kind,
      snapshots: {
        normalize: (state) => snapshots.normalizeSnapshot({
          id: record.id,
          room: record.room,
          roomSid: record.roomSid,
          revision: state.revision,
          remoteDataTracksBySid: state.remoteDataTracksBySid,
        }),
        normalizeEvent: (state, event, snapshot) => snapshots.normalizeQueuedEvent({
          id: record.id,
          room: record.room,
          roomSid: record.roomSid,
          revision: state.revision,
          remoteDataTracksBySid: state.remoteDataTracksBySid,
        }, event, snapshot),
      },
      localMedia: {
        published: (publication) => localMedia.reconcilePublished(record.id, publication),
        unpublished: (publication) => localMedia.reconcileUnpublished(record.id, publication),
      },
      remoteMedia: {
        participantIsCurrent: (participant) => remoteParticipantIsCurrent(record.id, participant),
        publicationIsCurrent: (participant, publication) =>
          remotePublicationIsCurrent(record.id, participant, publication),
        publicationWasReplaced: (participant, publication) =>
          remotePublicationWasReplaced(record.id, participant, publication),
        participantDisconnected: (participant) =>
          remoteMedia.participantDisconnected(record.id, participant),
        publicationUnpublished: (participant, publication) =>
          remoteMedia.publicationUnpublished(record.id, participant, publication),
        trackSubscribed: () => remoteMedia.trackSubscribed(record.id),
        trackUnsubscribed: (track, publication, participant) =>
          remoteMedia.trackUnsubscribed(record.id, track, publication, participant),
      },
      dataStreams: {
        handleFlowControl: (payload, participant, reliable, topic) =>
          dataStreams.handleFlowControl(record, payload, participant, reliable, topic),
        participantDisconnected: (participant) =>
          dataStreams.participantDisconnected(record, participant),
      },
    });
  } catch (error: unknown) {
    roomSessions.delete(record.id);
    try {
      room.removeAllListeners();
      void Promise.resolve(room.disconnect(false)).catch(() => {});
    } catch {}
    throw error;
  }

  return record.id;
}

export async function connectRoom(
  roomId: RoomId,
  serverUrl: string,
  participantToken: string,
  autoSubscribe: boolean,
  raw: unknown,
): Promise<AdapterJoin> {
  const record = roomSessions.get(roomId);
  if (!record || record.released) throw new Error("Room Session is no longer available");

  try {
    await record.room.connect(serverUrl, participantToken, { ...rawOptions(raw), autoSubscribe });
    if (roomSessions.get(roomId) !== record || record.released) {
      throw new Error("Room Session is no longer available");
    }
    const roomSid = await record.room.getSid();
    if (roomSessions.get(roomId) !== record || record.released) {
      throw new Error("Room Session is no longer available");
    }
    record.roomSid = roomSid;
    record.events!.discardInitialEvents();
    return { snapshot: record.events!.snapshot() } satisfies AdapterJoin;
  } catch (error: unknown) {
    try {
      await releaseRoom(record);
    } catch {
      // Preserve the actionable join failure rather than a secondary cleanup failure.
    }
    throw error;
  }
}

export function activateRoom(
  roomId: RoomId,
  callback: RoomUpdateCallback,
  dataPacketCallback: DataPacketCallback,
  chatMessageCallback: MessageCallback,
  transcriptionSegmentCallback: MessageCallback,
) {
  const record = roomSessions.get(roomId);
  if (!record || record.released) throw new Error("Room Session is no longer available");
  record.events!.activate({
    update: callback,
    dataPacket: dataPacketCallback,
    chatMessage: chatMessageCallback,
    transcriptionSegment: transcriptionSegmentCallback,
  });
}

export function leaveRoom(roomId: RoomId, onComplete: () => void) {
  const record = roomSessions.get(roomId);
  const operation = record ? releaseRoom(record) : Promise.resolve();
  return operation.finally(onComplete);
}

export function dropRoom(roomId: RoomId) {
  const record = roomSessions.get(roomId);
  if (record) void releaseRoom(record).catch(() => {});
}

export function startRoomAudio(roomId: RoomId) {
  const record = roomSessions.get(roomId);
  if (!record || record.released) return Promise.reject(new Error("Room Session is no longer available"));
  return Promise.resolve(record.room.startAudio());
}
