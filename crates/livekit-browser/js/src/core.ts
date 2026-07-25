import { requireMethods } from "./runtime";
import {
  rawOptions,
  trackSources,
} from "./options";
import { createDataStreamSubsystem } from "./data-streams";
import { createParticipantRpcSubsystem } from "./participant-rpc";
import { createLocalMediaSubsystem } from "./local-media";
import { createRemoteMediaSubsystem } from "./remote-media";
import {
  createAudioAnalyser,
  createAudioAnalyserOwner,
  mediaStreamAudioSource,
  terminateAudioAnalyserOwner,
} from "./audio-analysers";
import type {
  LocalParticipant,
  LocalTrackPublication,
  Participant,
  RemoteParticipant,
  RemoteTrack,
  RemoteTrackPublication,
  Room,
  RoomEventCallbacks,
  TrackPublication,
} from "livekit-client";

export { enumerateMediaDevices } from "./devices";
export { installLogExtension } from "./logging";
export { inspectBrowserRuntime } from "./runtime";
export { closeAudioAnalyser, dropAudioAnalyser, readAudioAnalyser } from "./audio-analysers";
export * from "./recording";

type RoomId = number;
type AttachmentId = number;

interface RemoteDataTrackLike {
  info: { sid?: string; name?: string; usesE2ee?: boolean };
  publisherIdentity: string;
}

interface QueuedEvent {
  type: string;
  participant_identity?: string;
  source_participant?: Participant;
  [key: string]: unknown;
}

interface ParticipantSnapshot {
  connection_quality: string;
  [key: string]: unknown;
}

interface RoomSnapshot {
  local_participant: ParticipantSnapshot;
  remote_participants: Record<string, ParticipantSnapshot>;
  [key: string]: unknown;
}

type RoomUpdateCallback = (update: { snapshot: RoomSnapshot; events: QueuedEvent[] }) => void;
type DataPacketCallback = (
  payload: Uint8Array,
  participantIdentity: string | null,
  reliable: boolean,
) => void;
type MessageCallback = (message: unknown) => void;
type RoomListener = (...args: any[]) => void;

interface RoomRecord {
  id: RoomId;
  room: Room;
  roomSid: string;
  revision: number;
  callback: RoomUpdateCallback | null;
  dataPacketCallback: DataPacketCallback | null;
  chatMessageCallback: MessageCallback | null;
  transcriptionSegmentCallback: MessageCallback | null;
  listeners: Array<[keyof RoomEventCallbacks, RoomListener]>;
  pendingEvents: QueuedEvent[];
  updateScheduled: boolean;
  localParticipantMutation: Promise<unknown>;
  reliableDataMutation: Promise<unknown>;
  released: boolean;
  remoteDataTracksBySid: Map<
    string,
    { participant: RemoteParticipant; track: RemoteDataTrackLike }
  >;
}

interface ChatMessageLike {
  id?: unknown;
  message?: unknown;
  timestamp?: unknown;
  editTimestamp?: unknown;
}

interface TranscriptionSegmentLike {
  id?: unknown;
  text?: unknown;
  language?: unknown;
  final?: unknown;
  startTime?: unknown;
  endTime?: unknown;
  firstReceivedTime?: unknown;
  lastReceivedTime?: unknown;
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
  permissions: { participantIdentities?: string[]; allParticipantsAllowed?: boolean } | null,
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

  const participantPermissions = (permissions?.participantIdentities || []).map(
    (participantIdentity) => ({ participantIdentity, allowAll: true }),
  );
  participant.setTrackSubscriptionPermissions(
    permissions?.allParticipantsAllowed === true,
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

function normalizedChatTimestamp(timestamp: unknown) {
  const value = Number(timestamp);
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new TypeError("LiveKit chat timestamp is not a non-negative safe integer");
  }
  return value;
}

function normalizeChatMessage(message: ChatMessageLike, senderIdentity: unknown) {
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

function normalizedTranscriptionTime(value: unknown, name: string) {
  const normalized = Number(value);
  if (!Number.isSafeInteger(normalized) || normalized < 0) {
    throw new TypeError(`LiveKit transcription ${name} is not a non-negative safe integer`);
  }
  return normalized;
}

function transcriptionSourceIsCurrent(
  record: RoomRecord,
  participant: Participant | undefined,
  publication: TrackPublication | undefined,
) {
  if (participant) {
    const currentParticipant =
      participant === record.room.localParticipant
        ? record.room.localParticipant
        : record.room.remoteParticipants.get(participant.identity);
    if (currentParticipant !== participant) return false;
  }
  if (publication) {
    if (typeof publication.trackSid !== "string") return false;
    const participants: Participant[] = [
      record.room.localParticipant,
      ...record.room.remoteParticipants.values(),
    ];
    const owner = participants.find(
      (candidate) => candidate.trackPublications.get(publication.trackSid) === publication,
    );
    if (!owner || (participant && owner !== participant)) return false;
  }
  return true;
}

function normalizeTranscriptionSegment(
  segment: TranscriptionSegmentLike,
  participant: Participant | undefined,
  publication: TrackPublication | undefined,
) {
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

const publicationSource = (source: unknown) =>
  trackSources[source as keyof typeof trackSources]?.normalized ?? "unknown";

const participantConnectionQuality = (quality: unknown) => {
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

const publicationKind = (kind: unknown) => {
  switch (kind) {
    case "audio":
    case "video":
    case "data":
      return kind;
    default:
      return "unknown";
  }
};

const grantSource = (source: unknown) => {
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

function normalizeGrants(permissions: LocalParticipant["permissions"]) {
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

function publicationState(publication: TrackPublication) {
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

function normalizeLocalPublications(participant: LocalParticipant) {
  const publications: Record<string, NonNullable<ReturnType<typeof publicationState>>> = {};
  for (const publication of participant.trackPublications.values()) {
    const state = publicationState(publication);
    if (state) publications[state.sid] = state;
  }
  return publications;
}

function normalizeRemotePublications(_record: RoomRecord, participant: RemoteParticipant) {
  const publications: Record<string, Record<string, unknown>> = {};
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

function normalizeRemoteDataTracks(record: RoomRecord, participant: RemoteParticipant) {
  const tracks: Record<string, { sid: string; name: string; uses_e2ee: boolean }> = {};
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

function normalizeRemoteParticipants(record: RoomRecord): Record<string, ParticipantSnapshot> {
  const participants: Record<string, ParticipantSnapshot> = Object.create(null);
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

function normalizeActiveSpeakers(record: RoomRecord) {
  const identities: string[] = [];
  const seen = new Set<string>();
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

function normalizeSnapshot(record: RoomRecord): RoomSnapshot {
  reconcileRemoteMediaTracks(record.id);
  const participant = record.room.localParticipant;
  localMedia.reconcileSnapshot(record.id, participant);
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

function normalizeQueuedEvent(
  record: RoomRecord,
  event: QueuedEvent,
  snapshot: RoomSnapshot,
): QueuedEvent | null {
  if (event.type !== "participantConnectionQualityChanged") return event;
  const { source_participant: participant, ...normalized } = event;
  if (participant === record.room.localParticipant) {
    normalized.quality = snapshot.local_participant.connection_quality;
    return normalized;
  }
  const participantIdentity = event.participant_identity;
  if (
    participantIdentity == null ||
    record.room.remoteParticipants.get(participantIdentity) !== participant
  ) {
    return null;
  }
  const participantSnapshot = snapshot.remote_participants[participantIdentity];
  if (!participantSnapshot) return null;
  normalized.quality = participantSnapshot.connection_quality;
  return normalized;
}

function scheduleUpdate(record: RoomRecord) {
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
      .filter((event): event is QueuedEvent => event != null);
    record.callback({ snapshot, events });
  }, 0);
}

function queueEvent(record: RoomRecord, event: QueuedEvent) {
  record.pendingEvents.push(event);
  scheduleUpdate(record);
}

function queueRemoteParticipantEvent(
  record: RoomRecord,
  type: string,
  participant: RemoteParticipant,
) {
  const identity = participant?.identity;
  if (typeof identity !== "string" || identity.length === 0) return;
  const current = record.room.remoteParticipants.get(identity);
  if (type === "remoteParticipantLeft") {
    if (current && current !== participant) return;
  } else if (!remoteParticipantIsCurrent(record.id, participant)) return;
  queueEvent(record, { type, identity });
}

function queueRemotePublicationsEvent(record: RoomRecord, participant: RemoteParticipant) {
  if (record.room.remoteParticipants.get(participant?.identity) !== participant) return;
  queueRemotePublicationsEventByIdentity(record, participant?.identity);
}

function queueRemoteDataTracksEvent(record: RoomRecord, participant: RemoteParticipant) {
  if (record.room.remoteParticipants.get(participant?.identity) !== participant) return;
  queueEvent(record, {
    type: "remoteDataTracksChanged",
    participant_identity: participant.identity,
  });
}

function dataTrackWithSid(participant: RemoteParticipant | undefined, sid: string) {
  for (const track of participant?.dataTracks.values() ?? []) {
    if (track.info?.sid === sid) return track;
  }
  return null;
}

function queueRemotePublicationsEventByIdentity(
  record: RoomRecord,
  participantIdentity: unknown,
) {
  if (typeof participantIdentity === "string" && participantIdentity.length > 0) {
    queueEvent(record, {
      type: "remoteTrackPublicationsChanged",
      participant_identity: participantIdentity,
    });
  }
}

function listen(record: RoomRecord, event: keyof RoomEventCallbacks, listener: RoomListener) {
  record.room.on(event, listener as RoomEventCallbacks[typeof event]);
  record.listeners.push([event, listener]);
}

function removeAdapterListeners(record: RoomRecord) {
  for (const [event, listener] of record.listeners.splice(0)) {
    record.room.off(event, listener as RoomEventCallbacks[typeof event]);
  }
  record.callback = null;
  record.dataPacketCallback = null;
  record.chatMessageCallback = null;
  record.transcriptionSegmentCallback = null;
  record.pendingEvents.length = 0;
}

function releaseDataStreams(record: RoomRecord) {
  dataStreams.releaseRoom(record);
}

function releaseParticipantRpc(record: RoomRecord) {
  participantRpc.releaseRoom(record);
}

async function releaseRoom(record: RoomRecord) {
  if (record.released) return;
  releaseDataStreams(record);
  record.released = true;
  releaseParticipantRpc(record);
  remoteMedia.releaseRoom(record.id);
  removeAdapterListeners(record);
  roomSessions.delete(record.id);
  try {
    await record.room.disconnect(false);
  } finally {
    localMedia.releaseRoom(record.id);
    record.room.removeAllListeners();
  }
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
  };
  roomSessions.set(record.id, record);

  const events = sdk.RoomEvent;
  listen(record, events.ConnectionStateChanged, (state: unknown) => {
    queueEvent(record, { type: "connectionStateChanged", state });
  });
  listen(record, events.ParticipantMetadataChanged, (_previous: unknown, participant: Participant) => {
    if (participant === room.localParticipant) {
      queueEvent(record, { type: "localParticipantChanged" });
    } else {
      queueRemoteParticipantEvent(record, "remoteParticipantChanged", participant as RemoteParticipant);
    }
  });
  listen(record, events.ParticipantAttributesChanged, (_changed: unknown, participant: Participant) => {
    if (participant === room.localParticipant) {
      queueEvent(record, { type: "localParticipantChanged" });
    } else {
      queueRemoteParticipantEvent(record, "remoteParticipantChanged", participant as RemoteParticipant);
    }
  });
  listen(record, events.ParticipantPermissionsChanged, (_previous: unknown, participant: Participant) => {
    if (participant === room.localParticipant) {
      queueEvent(record, { type: "localParticipantChanged" });
    }
  });
  listen(record, events.ConnectionQualityChanged, (_quality: unknown, participant: Participant) => {
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
  listen(record, events.LocalTrackPublished, (publication: LocalTrackPublication) => {
    localMedia.reconcilePublished(record.id, publication);
    queueEvent(record, { type: "localTrackPublicationsChanged" });
  });
  listen(record, events.LocalTrackUnpublished, (publication: LocalTrackPublication) => {
    localMedia.reconcileUnpublished(record.id, publication);
    queueEvent(record, { type: "localTrackPublicationsChanged" });
  });
  listen(record, events.TrackMuted, (publication: TrackPublication, participant: Participant) => {
    if (participant === room.localParticipant) {
      queueEvent(record, { type: "localTrackPublicationsChanged" });
    } else if (
      remotePublicationIsCurrent(
        record.id,
        participant as RemoteParticipant,
        publication as RemoteTrackPublication,
      )
    ) {
      queueRemotePublicationsEvent(record, participant as RemoteParticipant);
    }
  });
  listen(record, events.TrackUnmuted, (publication: TrackPublication, participant: Participant) => {
    if (participant === room.localParticipant) {
      queueEvent(record, { type: "localTrackPublicationsChanged" });
    } else if (
      remotePublicationIsCurrent(
        record.id,
        participant as RemoteParticipant,
        publication as RemoteTrackPublication,
      )
    ) {
      queueRemotePublicationsEvent(record, participant as RemoteParticipant);
    }
  });
  listen(record, events.ParticipantConnected, (participant: RemoteParticipant) => {
    queueRemoteParticipantEvent(record, "remoteParticipantJoined", participant);
  });
  listen(record, events.ParticipantDisconnected, (participant: RemoteParticipant) => {
    dataStreams.participantDisconnected(record, participant);
    remoteMedia.participantDisconnected(record.id, participant);
    queueRemoteParticipantEvent(record, "remoteParticipantLeft", participant);
  });
  listen(record, events.TrackPublished, (
    publication: RemoteTrackPublication,
    participant: RemoteParticipant,
  ) => {
    if (!remotePublicationIsCurrent(record.id, participant, publication)) return;
    queueRemotePublicationsEvent(record, participant);
  });
  listen(record, events.TrackUnpublished, (
    publication: RemoteTrackPublication,
    participant: RemoteParticipant,
  ) => {
    remoteMedia.publicationUnpublished(record.id, participant, publication);
    if (remotePublicationWasReplaced(record.id, participant, publication)) return;
    queueRemotePublicationsEvent(record, participant);
  });
  listen(record, events.TrackSubscribed, (
    _track: RemoteTrack,
    publication: RemoteTrackPublication,
    participant: RemoteParticipant,
  ) => {
    if (!remotePublicationIsCurrent(record.id, participant, publication)) return;
    remoteMedia.trackSubscribed(record.id);
    queueRemotePublicationsEvent(record, participant);
  });
  listen(record, events.TrackUnsubscribed, (
    track: RemoteTrack,
    publication: RemoteTrackPublication,
    participant: RemoteParticipant,
  ) => {
    remoteMedia.trackUnsubscribed(record.id, track, publication, participant);
    if (remotePublicationWasReplaced(record.id, participant, publication)) return;
    queueRemotePublicationsEvent(record, participant);
  });
  listen(record, events.TrackSubscriptionFailed, (_trackSid: string, participant: RemoteParticipant) => {
    queueRemotePublicationsEvent(record, participant);
  });
  listen(record, events.TrackSubscriptionStatusChanged, (
    publication: RemoteTrackPublication,
    _status: unknown,
    participant: RemoteParticipant,
  ) => {
    if (!remotePublicationIsCurrent(record.id, participant, publication)) return;
    queueRemotePublicationsEvent(record, participant);
  });
  listen(record, events.DataTrackPublished, (track: RemoteDataTrackLike) => {
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
  listen(record, events.DataTrackUnpublished, (sid: string) => {
    queueMicrotask(() => {
      if (record.released) return;
      const dataTrack = record.remoteDataTracksBySid.get(sid);
      if (!dataTrack) return;
      const currentParticipant = room.remoteParticipants.get(dataTrack.participant.identity);
      const currentTrack = dataTrackWithSid(currentParticipant, sid);
      if (currentTrack && currentParticipant) {
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
  listen(record, events.DataReceived, (
    payload: Uint8Array,
    participant: RemoteParticipant | undefined,
    kind: unknown,
    topic: string,
  ) => {
    if (kind !== sdk.DataPacket_Kind.RELIABLE && kind !== sdk.DataPacket_Kind.LOSSY) return;
    if (
      dataStreams.handleFlowControl(
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
  listen(record, events.ChatMessage, (
    message: ChatMessageLike,
    participant: RemoteParticipant | undefined,
  ) => {
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
    listen(record, events.TranscriptionReceived, (
      segments: unknown,
      participant: Participant | undefined,
      publication: TrackPublication | undefined,
    ) => {
      if (
        !Array.isArray(segments) ||
        !transcriptionSourceIsCurrent(record, participant, publication)
      ) {
        return;
      }
      for (const segment of segments) {
        try {
          record.transcriptionSegmentCallback?.(
            normalizeTranscriptionSegment(
              segment as TranscriptionSegmentLike,
              participant,
              publication,
            ),
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

export async function connectRoom(
  roomId: RoomId,
  serverUrl: string,
  participantToken: string,
  autoSubscribe: boolean,
  raw: unknown,
) {
  const record = roomSessions.get(roomId);
  if (!record) throw new Error("Room Session is no longer available");

  try {
    await record.room.connect(serverUrl, participantToken, { ...rawOptions(raw), autoSubscribe });
    record.roomSid = await record.room.getSid();
    record.pendingEvents.length = 0;
    return { snapshot: normalizeSnapshot(record) };
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
  if (!record) throw new Error("Room Session is no longer available");
  record.callback = callback;
  record.dataPacketCallback = dataPacketCallback;
  record.chatMessageCallback = chatMessageCallback;
  record.transcriptionSegmentCallback = transcriptionSegmentCallback;
  scheduleUpdate(record);
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
