import { requireMethods } from "./runtime";
import {
  cameraCaptureOptions,
  cloneCaptureOptions,
  livekitTrackSource,
  microphoneCaptureOptions,
  rawOptions,
  screenCaptureOptions,
  trackSources,
} from "./options";
import {
  normalizeReceiverStatistics,
  normalizeSenderStatistics,
  normalizeVideoSenderStatistics,
  optionalFiniteNumber,
  optionalSignedInteger,
  optionalString,
  optionalUnsignedInteger,
} from "./statistics";
import { createDataStreamSubsystem } from "./data-streams";
import { createParticipantRpcSubsystem } from "./participant-rpc";
import type {
  AudioReceiverStats,
  AudioSenderStats,
  LocalAudioTrack,
  LocalParticipant,
  LocalTrackPublication,
  LocalVideoTrack,
  Participant,
  RemoteAudioTrack,
  RemoteParticipant,
  RemoteTrack,
  RemoteTrackPublication,
  RemoteVideoTrack,
  Room,
  RoomEventCallbacks,
  TrackPublication,
  VideoReceiverStats,
  VideoSenderStats,
} from "livekit-client";
import type { RawOptions } from "./options";

export { enumerateMediaDevices } from "./devices";
export { installLogExtension } from "./logging";
export { inspectBrowserRuntime } from "./runtime";
export * from "./recording";

type RoomId = number;
type LocalTrackId = number;
type RemoteMediaTrackId = number;
type PublicationId = number;
type AttachmentId = number;
type AudioAnalyserId = number;
type Generation = number;
type MediaKind = "audio" | "video";
type LocalTrackSource = "camera" | "microphone" | "screenShare" | "screenShareAudio";
type Completion = (success: boolean, value?: unknown) => void;
type TerminalCallback = () => void;
type PublicationReplacementCallback = (
  publicationId: PublicationId | null,
  sid: string | null,
  muted: boolean,
) => PublicationId;
type LocalMediaTrack = LocalAudioTrack | LocalVideoTrack;
type RemoteMediaTrack = RemoteAudioTrack | RemoteVideoTrack;
type MediaElement = HTMLAudioElement | HTMLVideoElement;

interface PublicationReplacementState {
  roomId: RoomId;
  publicationId: PublicationId | null;
  callback: PublicationReplacementCallback;
}

interface LocalTrackRecord<TTrack extends LocalMediaTrack, TKind extends MediaKind> {
  id: LocalTrackId;
  track: TTrack;
  mediaKind: TKind;
  source: LocalTrackSource;
  captureOptions: RawOptions;
  captureStopped: boolean;
  stopRequested: boolean;
  stopOperation: Promise<unknown> | null;
  ownerDropped: boolean;
  attachments: Map<AttachmentId, MediaElement>;
  audioAnalyserIds?: Set<AudioAnalyserId>;
  publishedRoomId: RoomId | null;
  pendingPublishRoomId: RoomId | null;
  publicationId: PublicationId | null;
  pendingPublicationId: PublicationId | null;
  publication: LocalTrackPublication | null;
  publicationReplacementCallback: PublicationReplacementCallback | null;
  awaitingPublicationReplacement: PublicationReplacementState | null;
  explicitUnpublishId: PublicationId | null;
  explicitUnpublishObservedId: PublicationId | null;
  mutationTail: Promise<unknown>;
  ended: boolean;
  endedCallback: TerminalCallback | null;
  endedListener: TerminalCallback | null;
  browserEndedPeer: AnyLocalTrackRecord | null;
  endedCleanupComplete: boolean;
  endedCleanupRunning: boolean;
}

type LocalVideoTrackRecord = LocalTrackRecord<LocalVideoTrack, "video">;
type LocalAudioTrackRecord = LocalTrackRecord<LocalAudioTrack, "audio">;
type AnyLocalTrackRecord = LocalVideoTrackRecord | LocalAudioTrackRecord;
type LocalTrackRegistry = Map<LocalTrackId, AnyLocalTrackRecord>;

interface RemoteParticipantLocator {
  identity: string;
  sid: string;
  generation: Generation;
}

interface RemotePublicationLocator {
  participant: RemoteParticipantLocator;
  sid: string;
  generation: Generation;
}

interface RemoteMediaTrackRecord {
  id: RemoteMediaTrackId;
  roomId: RoomId;
  locator: RemotePublicationLocator;
  participant: RemoteParticipant;
  publication: RemoteTrackPublication;
  track: RemoteMediaTrack;
  mediaGeneration: Generation;
  kind: MediaKind;
  attachments: Map<AttachmentId, MediaElement>;
  audioAnalyserIds: Set<AudioAnalyserId>;
  terminal: boolean;
  terminalCallback: TerminalCallback | null;
  endedListener: TerminalCallback | null;
}

interface AudioAnalyserOwner {
  id: number;
  audioAnalyserIds?: Set<AudioAnalyserId>;
}

interface AudioAnalyserRecord {
  id: AudioAnalyserId;
  owner: LocalAudioTrackRecord | RemoteMediaTrackRecord;
  ownerKind: "local" | "remote";
  sourceTrack: MediaStreamTrack;
  sourceNode: MediaStreamAudioSourceNode;
  analyserNode: AnalyserNode;
  silentSink: GainNode;
  context: AudioContext;
  terminal: boolean;
  terminalCallback: TerminalCallback | null;
  endedListener: TerminalCallback | null;
  closeOperation: Promise<void> | null;
}

interface RemoteObjectGenerationState {
  generation: Generation;
  identity?: string;
  sid?: string;
  source?: MediaStreamTrack | RemoteTrack;
}

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
  remoteMediaTrackIds: Set<RemoteMediaTrackId>;
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

interface AudioPublicationInput {
  audioCodec?: unknown;
  name?: string | null;
  dtx?: boolean | null;
  red?: boolean | null;
}

interface VideoPublicationInput {
  name?: string | null;
  videoCodec?: string | null;
  simulcast?: boolean | null;
  videoEncoding?: { maxBitrate?: number; maxFramerate?: number } | null;
}

let nextRoomId: RoomId = 1;
const roomSessions = new Map<RoomId, RoomRecord>();
let nextLocalVideoTrackId: LocalTrackId = 1;
let nextAttachmentId: AttachmentId = 1;
const localVideoTracks = new Map<LocalTrackId, LocalVideoTrackRecord>();
let nextLocalAudioTrackId: LocalTrackId = 1;
const localAudioTracks = new Map<LocalTrackId, LocalAudioTrackRecord>();
let nextRemoteMediaTrackId: RemoteMediaTrackId = 1;
const remoteMediaTracks = new Map<RemoteMediaTrackId, RemoteMediaTrackRecord>();
let nextAudioAnalyserId: AudioAnalyserId = 1;
const audioAnalysers = new Map<AudioAnalyserId, AudioAnalyserRecord>();
let nextRemoteObjectGeneration = 1;
const remoteParticipantGenerations = new WeakMap<RemoteParticipant, RemoteObjectGenerationState>();
const remotePublicationGenerations = new WeakMap<RemoteTrackPublication, RemoteObjectGenerationState>();
const remoteMediaGenerations = new WeakMap<RemoteTrack, RemoteObjectGenerationState>();

function remoteParticipantGeneration(participant: RemoteParticipant): Generation {
  let state = remoteParticipantGenerations.get(participant);
  if (state?.identity !== participant.identity || state?.sid !== participant.sid) {
    state = {
      generation: nextRemoteObjectGeneration++,
      identity: participant.identity,
      sid: participant.sid,
    };
    remoteParticipantGenerations.set(participant, state);
  }
  return state!.generation;
}

function remotePublicationGeneration(
  publication: RemoteTrackPublication,
  sid: string = publication.trackSid,
): Generation {
  let state = remotePublicationGenerations.get(publication);
  if (!state || state.sid !== sid) {
    state = { generation: nextRemoteObjectGeneration++, sid };
    remotePublicationGenerations.set(publication, state);
  }
  return state.generation;
}

function remoteMediaGeneration(track: RemoteTrack): Generation {
  const source = track.mediaStreamTrack ?? track;
  let state = remoteMediaGenerations.get(track);
  if (state?.source !== source) {
    state = { generation: nextRemoteObjectGeneration++, source };
    remoteMediaGenerations.set(track, state);
  }
  return state!.generation;
}

function localVideoTrackRecord(trackId: LocalTrackId): LocalVideoTrackRecord {
  const record = localVideoTracks.get(trackId);
  if (!record) throw new Error("Local Video Track is no longer available");
  return record;
}

function enqueueLocalTrackMutation<T>(
  record: AnyLocalTrackRecord,
  operation: () => T | PromiseLike<T>,
): Promise<T> {
  const result = record.mutationTail.then(operation, operation);
  record.mutationTail = result.catch(() => {});
  return result;
}

function registerLocalVideoTrack(
  track: LocalVideoTrack,
  captureOptions: RawOptions,
  source: LocalTrackSource,
): LocalTrackId {
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

function registerLocalAudioTrack(
  track: LocalAudioTrack,
  captureOptions: RawOptions,
  source: LocalTrackSource,
): LocalTrackId {
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

function removeLocalTrackEndedListener(record: AnyLocalTrackRecord) {
  if (record.endedListener) {
    record.track.off(globalThis.LivekitClient.TrackEvent.Ended, record.endedListener);
    record.endedListener = null;
  }
  record.endedCallback = null;
  record.browserEndedPeer = null;
}

function finishBrowserEnded(record: AnyLocalTrackRecord, sourceEnded: boolean) {
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

function deleteFinishedBrowserEndedRecord(record: AnyLocalTrackRecord) {
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

function finishBrowserEndedCleanup(record: AnyLocalTrackRecord, retryDelay = 0) {
  if (record.endedCleanupRunning || record.endedCleanupComplete) return;
  record.endedCleanupRunning = true;
  const operation = enqueueLocalTrackMutation(record, async () => {
    let failure: unknown = null;
    const room =
      record.publishedRoomId == null ? undefined : roomSessions.get(record.publishedRoomId);
    if (record.publication && room && !room.released) {
      try {
        await room.room.localParticipant.unpublishTrack(record.track, false);
        record.publication = null;
        record.publishedRoomId = null;
        record.publicationId = null;
      } catch (error: unknown) {
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
    } catch (error: unknown) {
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

function installLocalTrackEndedListener(
  record: AnyLocalTrackRecord,
  onEnded: TerminalCallback | null,
) {
  const listener = () => finishBrowserEnded(record, true);
  record.endedListener = listener;
  record.track.on(globalThis.LivekitClient.TrackEvent.Ended, listener);
  if (onEnded) record.endedCallback = onEnded;
}

export function observeLocalVideoTrackEnded(trackId: LocalTrackId, onEnded: TerminalCallback) {
  const record = localVideoTrackRecord(trackId);
  if (record.ended) queueMicrotask(onEnded);
  else record.endedCallback = onEnded;
}

export function observeLocalAudioTrackEnded(trackId: LocalTrackId, onEnded: TerminalCallback) {
  const record = localAudioTrackRecord(trackId);
  if (record.ended) queueMicrotask(onEnded);
  else record.endedCallback = onEnded;
}

export function createLocalClipMediaStream(
  audioTrackIds: LocalTrackId[],
  videoTrackIds: LocalTrackId[],
) {
  const source = (
    records: ReadonlyMap<LocalTrackId, AnyLocalTrackRecord>,
    trackId: LocalTrackId,
    expectedKind: MediaKind,
  ) => {
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

async function restartVideoTrack(record: LocalVideoTrackRecord, options: RawOptions) {
  const restartOptions = cloneCaptureOptions(options);
  delete restartOptions.resolution;
  delete restartOptions.frameRate;
  await record.track.restartTrack(restartOptions);

  const constraints: MediaTrackConstraints = {};
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
  (record.track as LocalVideoTrack & { _constraints: MediaTrackConstraints })._constraints = {
    ...(record.track as LocalVideoTrack & { _constraints: MediaTrackConstraints })._constraints,
    ...constraints,
  };
}

function restartLocalTrack<TRecord extends AnyLocalTrackRecord>({
  record,
  nextOptions,
  normalizeOptions,
  performRestart,
  terminate,
  onComplete,
}: {
  record: TRecord;
  nextOptions: (previous: RawOptions) => RawOptions;
  normalizeOptions: (options: RawOptions) => RawOptions;
  performRestart: (record: TRecord, options: RawOptions) => Promise<void>;
  terminate: (record: TRecord) => void;
  onComplete: (terminal: boolean) => void;
}) {
  let becameTerminal = false;
  let operation: Promise<unknown>;
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
      } catch (error: unknown) {
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
  } catch (error: unknown) {
    operation = Promise.reject(error);
  }

  return operation.then(
    (value) => {
      onComplete(false);
      return value;
    },
    (error: unknown) => {
      onComplete(becameTerminal);
      throw error;
    },
  );
}

function detachLocalVideoElement(record: LocalVideoTrackRecord, attachmentId: AttachmentId) {
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

function detachLocalAudioElement(record: LocalAudioTrackRecord, attachmentId: AttachmentId) {
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

function remoteMediaTrackRecord(trackId: RemoteMediaTrackId): RemoteMediaTrackRecord {
  const record = remoteMediaTracks.get(trackId);
  if (!record || record.terminal) throw new Error("Remote Media Track is no longer available");
  return record;
}

function currentRemotePublication(roomRecord: RoomRecord, locator: RemotePublicationLocator) {
  const participant = currentRemoteParticipant(roomRecord, locator.participant);
  if (!participant) return null;
  const publication = participant.trackPublications.get(locator.sid);
  if (!publication || remotePublicationGeneration(publication) !== locator.generation) return null;
  return { participant, publication };
}

function currentRemoteParticipant(roomRecord: RoomRecord, locator: RemoteParticipantLocator) {
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

export function rawRoom(roomId: RoomId): Room {
  const record = roomSessions.get(roomId);
  if (!record || record.released) throw new Error("Room Session is no longer available");
  return record.room;
}

export function rawLocalParticipant(roomId: RoomId): LocalParticipant {
  return rawRoom(roomId).localParticipant;
}

export function rawRemoteParticipant(roomId: RoomId, locator: RemoteParticipantLocator) {
  const record = roomSessions.get(roomId);
  if (!record || record.released) throw new Error("Room Session is no longer available");
  const participant = currentRemoteParticipant(record, locator);
  if (!participant) throw new Error("Remote Participant is no longer available");
  return participant;
}

export function rawRemotePublication(roomId: RoomId, locator: RemotePublicationLocator) {
  const record = roomSessions.get(roomId);
  if (!record || record.released) throw new Error("Room Session is no longer available");
  const current = currentRemotePublication(record, locator);
  if (!current) {
    throw new Error("Remote Track Publication is no longer available");
  }
  return current.publication;
}

function localMediaTrackResources(kind: MediaKind) {
  if (kind === "video") return { records: localVideoTracks, recordFor: localVideoTrackRecord };
  if (kind === "audio") return { records: localAudioTracks, recordFor: localAudioTrackRecord };
  throw new TypeError(`unsupported Local Media Track kind: ${kind}`);
}

function localMediaTrackRecord(trackId: LocalTrackId, kind: MediaKind) {
  return localMediaTrackResources(kind).recordFor(trackId);
}

export function rawLocalMediaTrack(trackId: LocalTrackId, kind: MediaKind) {
  const record = localMediaTrackRecord(trackId, kind);
  if (record.stopRequested || record.captureStopped || record.ended) {
    throw new Error("Local Media Track is no longer available");
  }
  return record.track;
}

export function rawLocalPublication(
  trackId: LocalTrackId,
  publicationId: PublicationId,
  kind: MediaKind,
) {
  const record = localMediaTrackRecord(trackId, kind);
  if (record.publicationId !== publicationId || !record.publication) {
    throw new Error("Local Track Publication is no longer available");
  }
  return record.publication;
}

export function rawRemoteMediaTrack(trackId: RemoteMediaTrackId) {
  const record = remoteMediaTrackRecord(trackId);
  if (!remoteMediaTrackIsCurrent(record)) {
    terminateRemoteMediaTrack(record);
    throw new Error("Remote Media Track is no longer available");
  }
  return record.track;
}

export function setLocalMediaTrackProcessor(
  trackId: LocalTrackId,
  kind: MediaKind,
  processor: any,
  showProcessedStreamLocally: boolean,
) {
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

export function rawLocalMediaTrackProcessor(trackId: LocalTrackId, kind: MediaKind) {
  const record = localMediaTrackRecord(trackId, kind);
  if (record.stopRequested || record.captureStopped || record.ended) {
    throw new Error("Local Media Track is no longer available");
  }
  if (typeof record.track.getProcessor !== "function") {
    throw new TypeError("LiveKit Local Media Track does not support processors");
  }
  return record.track.getProcessor() ?? null;
}

export function stopLocalMediaTrackProcessor(trackId: LocalTrackId, kind: MediaKind) {
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

function clearDetachedRemoteElement(element: MediaElement) {
  if (!hasRemoteElementAttachment(null, element)) element.srcObject = null;
}

function hasRemoteElementAttachment(track: RemoteMediaTrack | null, element: MediaElement) {
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

function detachRemoteElement(record: RemoteMediaTrackRecord, attachmentId: AttachmentId) {
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

function terminateRemoteMediaTrack(record: RemoteMediaTrackRecord) {
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

function terminateRemoteMediaWhere(predicate: (record: RemoteMediaTrackRecord) => boolean) {
  for (const record of remoteMediaTracks.values()) {
    if (predicate(record)) terminateRemoteMediaTrack(record);
  }
}

function reconcileRemoteMediaTracks(roomRecord: RoomRecord) {
  for (const trackId of roomRecord.remoteMediaTrackIds) {
    const record = remoteMediaTracks.get(trackId);
    if (!record || record.terminal) continue;
    if (!remoteMediaTrackIsCurrent(record)) terminateRemoteMediaTrack(record);
  }
}

function remoteMediaTrackIsCurrent(record: RemoteMediaTrackRecord) {
  const roomRecord = roomSessions.get(record.roomId);
  if (!roomRecord || roomRecord.released) return false;
  const publication = currentRemotePublication(roomRecord, record.locator);
  return (
    publication?.participant === record.participant &&
    publication?.publication === record.publication &&
    publication?.publication.track === record.track &&
    remoteMediaGeneration(record.track) === record.mediaGeneration
  );
}

export function setRemoteTrackSubscription(
  roomId: RoomId,
  locator: RemotePublicationLocator,
  subscribed: boolean,
) {
  const roomRecord = roomSessions.get(roomId);
  if (!roomRecord || roomRecord.released) throw new Error("Room Session is no longer available");
  const current = currentRemotePublication(roomRecord, locator);
  if (!current) throw new Error("Remote Track Publication is no longer available");
  current.publication.setSubscribed(subscribed);
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

export function resolveRemoteMediaTrack(
  roomId: RoomId,
  locator: RemotePublicationLocator,
  mediaGeneration: Generation,
  onTerminal: TerminalCallback,
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
  const record: RemoteMediaTrackRecord = {
    id,
    roomId,
    locator,
    participant: current!.participant,
    publication,
    track: track as RemoteMediaTrack,
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

function remoteTrackReceiverStatistics(
  trackId: RemoteMediaTrackId,
  kind: "audio",
): Promise<AudioReceiverStats | undefined>;
function remoteTrackReceiverStatistics(
  trackId: RemoteMediaTrackId,
  kind: "video",
): Promise<VideoReceiverStats | undefined>;
async function remoteTrackReceiverStatistics(trackId: RemoteMediaTrackId, kind: MediaKind) {
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

export async function remoteAudioReceiverStatistics(trackId: RemoteMediaTrackId) {
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

export async function remoteVideoReceiverStatistics(trackId: RemoteMediaTrackId) {
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

export async function rawRemoteRtcStatsReport(trackId: RemoteMediaTrackId) {
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

export function attachRemoteVideoTrack(trackId: RemoteMediaTrackId, element: HTMLVideoElement) {
  const record = remoteMediaTrackRecord(trackId);
  if (record.kind !== "video") throw new TypeError("Remote Media Track is not video");
  const attachmentId = nextAttachmentId++;
  record.track.attach(element);
  record.attachments.set(attachmentId, element);
  return attachmentId;
}

export function detachRemoteVideoTrack(trackId: RemoteMediaTrackId, attachmentId: AttachmentId) {
  detachRemoteElement(remoteMediaTrackRecord(trackId), attachmentId);
}

export function dropRemoteVideoTrackAttachment(
  trackId: RemoteMediaTrackId,
  attachmentId: AttachmentId,
) {
  const record = remoteMediaTracks.get(trackId);
  if (!record) return;
  try {
    detachRemoteElement(record, attachmentId);
  } catch {}
}

export function attachRemoteAudioTrack(trackId: RemoteMediaTrackId, element: HTMLAudioElement) {
  const record = remoteMediaTrackRecord(trackId);
  if (record.kind !== "audio") throw new TypeError("Remote Media Track is not audio");
  const attachmentId = nextAttachmentId++;
  record.track.attach(element);
  record.attachments.set(attachmentId, element);
  return attachmentId;
}

export function detachRemoteAudioTrack(trackId: RemoteMediaTrackId, attachmentId: AttachmentId) {
  detachRemoteElement(remoteMediaTrackRecord(trackId), attachmentId);
}

export function dropRemoteAudioTrackAttachment(
  trackId: RemoteMediaTrackId,
  attachmentId: AttachmentId,
) {
  const record = remoteMediaTracks.get(trackId);
  if (!record) return;
  try {
    detachRemoteElement(record, attachmentId);
  } catch {}
}

export function dropRemoteMediaTrack(trackId: RemoteMediaTrackId) {
  const record = remoteMediaTracks.get(trackId);
  if (!record) return;
  terminateRemoteMediaTrack(record);
}

export function dropRemoteMediaTrackDescriptor(descriptor: unknown) {
  const id = (descriptor as { id?: unknown } | null)?.id;
  if (typeof id === "number" && Number.isInteger(id)) dropRemoteMediaTrack(id);
}

function audioContextConstructor() {
  const AudioContext = globalThis.AudioContext || globalThis.webkitAudioContext;
  if (typeof AudioContext !== "function") {
    throw new TypeError("Web Audio AudioContext is unavailable");
  }
  return AudioContext;
}

function mediaStreamAudioSource(track: LocalAudioTrack | RemoteAudioTrack) {
  const source = track?.mediaStreamTrack;
  if (!(source instanceof MediaStreamTrack) || source.kind !== "audio") {
    throw new TypeError("Media Track does not expose a browser audio source");
  }
  if (source.readyState === "ended") throw new Error("Media Track audio source has ended");
  return source;
}

function createAudioAnalyser(
  owner: LocalAudioTrackRecord | RemoteMediaTrackRecord,
  ownerKind: "local" | "remote",
  sourceTrack: MediaStreamTrack,
  onTerminal: TerminalCallback,
) {
  const AudioContext = audioContextConstructor();
  const context = new AudioContext();
  let sourceNode: MediaStreamAudioSourceNode;
  let analyserNode: AnalyserNode;
  let silentSink: GainNode;
  try {
    sourceNode = context.createMediaStreamSource(new MediaStream([sourceTrack]));
    analyserNode = context.createAnalyser();
    analyserNode.fftSize = 2048;
    silentSink = context.createGain();
    silentSink.gain.value = 0;
    sourceNode.connect(analyserNode);
    analyserNode.connect(silentSink);
    silentSink.connect(context.destination);
  } catch (error: unknown) {
    void context.close().catch(() => {});
    throw error;
  }

  const id = nextAudioAnalyserId++;
  const record: AudioAnalyserRecord = {
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
  owner.audioAnalyserIds!.add(id);
  if (context.state === "suspended") void context.resume().catch(() => {});
  return id;
}

function terminateAudioAnalyser(record: AudioAnalyserRecord): Promise<void> {
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
  record.owner.audioAnalyserIds?.delete(record.id);
  audioAnalysers.delete(record.id);
  const callback = record.terminalCallback;
  record.terminalCallback = null;
  callback?.();
  try {
    record.closeOperation =
      record.context.state === "closed" ? Promise.resolve() : record.context.close();
  } catch (error: unknown) {
    record.closeOperation = Promise.reject(error);
  }
  return record.closeOperation;
}

function terminateAudioAnalysers(owner: AudioAnalyserOwner) {
  if (!owner.audioAnalyserIds) return;
  for (const analyserId of [...owner.audioAnalyserIds]) {
    const analyser = audioAnalysers.get(analyserId);
    if (analyser) void terminateAudioAnalyser(analyser).catch(() => {});
  }
}

function audioAnalyserIsCurrent(record: AudioAnalyserRecord) {
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

export function createLocalAudioAnalyser(trackId: LocalTrackId, onTerminal: TerminalCallback) {
  const record = localAudioTrackRecord(trackId);
  if (record.stopRequested) throw new Error("Local Audio Track capture has stopped");
  return createAudioAnalyser(record, "local", mediaStreamAudioSource(record.track), onTerminal);
}

export function createRemoteAudioAnalyser(trackId: RemoteMediaTrackId, onTerminal: TerminalCallback) {
  const record = remoteMediaTrackRecord(trackId);
  if (record.kind !== "audio") throw new TypeError("Remote Media Track is not audio");
  if (!remoteMediaTrackIsCurrent(record)) {
    terminateRemoteMediaTrack(record);
    throw new Error("Remote Media Track is no longer available");
  }
  return createAudioAnalyser(
    record,
    "remote",
    mediaStreamAudioSource(record.track as RemoteAudioTrack),
    onTerminal,
  );
}

export function readAudioAnalyser(analyserId: AudioAnalyserId) {
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

export function closeAudioAnalyser(analyserId: AudioAnalyserId) {
  const record = audioAnalysers.get(analyserId);
  if (!record) return Promise.resolve();
  return terminateAudioAnalyser(record);
}

export function dropAudioAnalyser(analyserId: AudioAnalyserId) {
  const record = audioAnalysers.get(analyserId);
  if (record) void terminateAudioAnalyser(record).catch(() => {});
}

function stopLocalVideoRecord(record: LocalVideoTrackRecord) {
  let failure: unknown = null;
  for (const attachmentId of [...record.attachments.keys()]) {
    try {
      detachLocalVideoElement(record, attachmentId);
    } catch (error: unknown) {
      failure ??= error;
    }
  }
  if (!record.captureStopped) {
    try {
      record.track.stop();
      record.captureStopped = true;
    } catch (error: unknown) {
      failure ??= error;
    }
  }
  if (failure) throw failure;
}

function terminateLocalVideoAfterRestartFailure(record: LocalVideoTrackRecord) {
  record.stopRequested = true;
  try {
    stopLocalVideoRecord(record);
  } catch {
    record.captureStopped = true;
  }
}

function queueLocalVideoStop(record: LocalVideoTrackRecord) {
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

function requireLocalTrackInstance(track: LocalMediaTrack, name: string) {
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

export function createCameraVideoTrack(options: RawOptions, raw: unknown, onComplete: Completion) {
  const captureOptions = cameraCaptureOptions(options, raw);
  void globalThis.LivekitClient.createLocalVideoTrack(cloneCaptureOptions(captureOptions)).then(
    (track) => {
      try {
        requireLocalTrackInstance(track, "LocalVideoTrack instance");
        const id = registerLocalVideoTrack(track, captureOptions, "camera");
        onComplete(true, id);
      } catch (error: unknown) {
        try {
          track?.stop?.();
        } catch {}
        onComplete(false, error);
      }
    },
    (error: unknown) => onComplete(false, error),
  );
}

export function localVideoTrackDeviceId(trackId: LocalTrackId) {
  const record = localVideoTrackRecord(trackId);
  return enqueueLocalTrackMutation(record, async () => {
    if (record.stopRequested) throw new Error("Local Video Track capture has stopped");
    return (await record.track.getDeviceId()) ?? null;
  });
}

export function restartLocalVideoTrack(trackId: LocalTrackId, onComplete: (terminal: boolean) => void) {
  const record = localVideoTrackRecord(trackId);
  return restartLocalTrack({
    record,
    nextOptions: (previous) => previous,
    normalizeOptions: (requested) => cameraCaptureOptions(requested, undefined),
    performRestart: restartVideoTrack,
    terminate: terminateLocalVideoAfterRestartFailure,
    onComplete,
  });
}

export function restartLocalVideoTrackWithOptions(
  trackId: LocalTrackId,
  options: RawOptions,
  raw: unknown,
  onComplete: (terminal: boolean) => void,
) {
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

export function switchLocalVideoTrackDevice(
  trackId: LocalTrackId,
  deviceId: string,
  onComplete: (terminal: boolean) => void,
) {
  const record = localVideoTrackRecord(trackId);
  return restartLocalTrack({
    record,
    nextOptions: (previous) => ({ ...previous, deviceId: { exact: deviceId } }),
    normalizeOptions: (requested) => cameraCaptureOptions(requested, undefined),
    performRestart: restartVideoTrack,
    terminate: terminateLocalVideoAfterRestartFailure,
    onComplete,
  });
}

export function attachLocalVideoTrack(trackId: LocalTrackId, element: HTMLVideoElement) {
  const record = localVideoTrackRecord(trackId);
  if (record.stopRequested) throw new Error("Local Video Track capture has stopped");
  const attachmentId = nextAttachmentId++;
  record.track.attach(element);
  record.attachments.set(attachmentId, element);
  return attachmentId;
}

export function detachLocalVideoTrack(trackId: LocalTrackId, attachmentId: AttachmentId) {
  detachLocalVideoElement(localVideoTrackRecord(trackId), attachmentId);
}

export function stopLocalVideoTrack(trackId: LocalTrackId) {
  return queueLocalVideoStop(localVideoTrackRecord(trackId));
}

export function dropLocalVideoTrackAttachment(trackId: LocalTrackId, attachmentId: AttachmentId) {
  const record = localVideoTracks.get(trackId);
  if (record) detachLocalVideoElement(record, attachmentId);
}

function finishDroppedLocalVideoTrack(
  record: LocalVideoTrackRecord,
  trackId: LocalTrackId,
  retryDelay = 0,
) {
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

export function dropLocalVideoTrack(trackId: LocalTrackId) {
  const record = localVideoTracks.get(trackId);
  if (!record) return;
  removeLocalTrackEndedListener(record);
  record.ownerDropped = true;
  finishDroppedLocalVideoTrack(record, trackId);
}

function localAudioTrackRecord(trackId: LocalTrackId): LocalAudioTrackRecord {
  const record = localAudioTracks.get(trackId);
  if (!record) throw new Error("Local Audio Track is no longer available");
  return record;
}

function stopLocalAudioRecord(record: LocalAudioTrackRecord) {
  let failure: unknown = null;
  terminateAudioAnalysers(record);
  for (const attachmentId of [...record.attachments.keys()]) {
    try {
      detachLocalAudioElement(record, attachmentId);
    } catch (error: unknown) {
      failure ??= error;
    }
  }
  if (!record.captureStopped) {
    try {
      record.track.stop();
      record.captureStopped = true;
    } catch (error: unknown) {
      failure ??= error;
    }
  }
  if (failure) throw failure;
}

function terminateLocalAudioAfterRestartFailure(record: LocalAudioTrackRecord) {
  record.stopRequested = true;
  try {
    stopLocalAudioRecord(record);
  } catch {
    record.captureStopped = true;
  }
}

function queueLocalAudioStop(record: LocalAudioTrackRecord) {
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

export function createMicrophoneAudioTrack(options: RawOptions, raw: unknown, onComplete: Completion) {
  const captureOptions = microphoneCaptureOptions(options, raw);
  void globalThis.LivekitClient.createLocalAudioTrack(cloneCaptureOptions(captureOptions)).then(
    (track) => {
      try {
        requireLocalTrackInstance(track, "LocalAudioTrack instance");
        const id = registerLocalAudioTrack(track, captureOptions, "microphone");
        onComplete(true, id);
      } catch (error: unknown) {
        try {
          track?.stop?.();
        } catch {}
        onComplete(false, error);
      }
    },
    (error: unknown) => onComplete(false, error),
  );
}

export function createScreenCapture(options: RawOptions, raw: unknown, onComplete: Completion) {
  const captureOptions = screenCaptureOptions(options, raw);
  void globalThis.LivekitClient.createLocalScreenTracks(cloneCaptureOptions(captureOptions)).then(
    (tracks) => {
      let videoTrackId: LocalTrackId | null = null;
      let audioTrackId: LocalTrackId | null = null;
      try {
        if (!Array.isArray(tracks)) {
          throw new TypeError("LiveKit display capture did not return a track array");
        }
        const videoTracks = tracks.filter(
          (track): track is LocalVideoTrack => track.kind === "video",
        );
        const audioTracks = tracks.filter(
          (track): track is LocalAudioTrack => track.kind === "audio",
        );
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
      } catch (error: unknown) {
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
    (error: unknown) => onComplete(false, error),
  );
}

export function localAudioTrackDeviceId(trackId: LocalTrackId) {
  const record = localAudioTrackRecord(trackId);
  return enqueueLocalTrackMutation(record, async () => {
    if (record.stopRequested) throw new Error("Local Audio Track capture has stopped");
    return (await record.track.getDeviceId()) ?? null;
  });
}

export function restartLocalAudioTrack(trackId: LocalTrackId, onComplete: (terminal: boolean) => void) {
  const record = localAudioTrackRecord(trackId);
  return restartLocalTrack({
    record,
    nextOptions: (previous) => previous,
    normalizeOptions: (requested) => microphoneCaptureOptions(requested, undefined),
    performRestart: (current, requested) => current.track.restartTrack(requested),
    terminate: terminateLocalAudioAfterRestartFailure,
    onComplete,
  });
}

export function restartLocalAudioTrackWithOptions(
  trackId: LocalTrackId,
  options: RawOptions,
  raw: unknown,
  onComplete: (terminal: boolean) => void,
) {
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

export function switchLocalAudioTrackDevice(
  trackId: LocalTrackId,
  deviceId: string,
  onComplete: (terminal: boolean) => void,
) {
  const record = localAudioTrackRecord(trackId);
  return restartLocalTrack({
    record,
    nextOptions: (previous) => ({ ...previous, deviceId: { exact: deviceId } }),
    normalizeOptions: (requested) => microphoneCaptureOptions(requested, undefined),
    performRestart: (current, requested) => current.track.restartTrack(requested),
    terminate: terminateLocalAudioAfterRestartFailure,
    onComplete,
  });
}

export function attachLocalAudioTrack(trackId: LocalTrackId, element: HTMLAudioElement) {
  const record = localAudioTrackRecord(trackId);
  if (record.stopRequested) throw new Error("Local Audio Track capture has stopped");
  const attachmentId = nextAttachmentId++;
  record.track.attach(element);
  record.attachments.set(attachmentId, element);
  return attachmentId;
}

export function detachLocalAudioTrack(trackId: LocalTrackId, attachmentId: AttachmentId) {
  detachLocalAudioElement(localAudioTrackRecord(trackId), attachmentId);
}

export function stopLocalAudioTrack(trackId: LocalTrackId) {
  return queueLocalAudioStop(localAudioTrackRecord(trackId));
}

export function dropLocalAudioTrackAttachment(trackId: LocalTrackId, attachmentId: AttachmentId) {
  const record = localAudioTracks.get(trackId);
  if (record) detachLocalAudioElement(record, attachmentId);
}

function finishDroppedLocalAudioTrack(
  record: LocalAudioTrackRecord,
  trackId: LocalTrackId,
  retryDelay = 0,
) {
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

export function dropLocalAudioTrack(trackId: LocalTrackId) {
  const record = localAudioTracks.get(trackId);
  if (!record) return;
  removeLocalTrackEndedListener(record);
  record.ownerDropped = true;
  finishDroppedLocalAudioTrack(record, trackId);
}

function audioPublicationOptions(
  options: AudioPublicationInput,
  raw: unknown,
  record: LocalAudioTrackRecord,
) {
  if (options.audioCodec != null && options.audioCodec !== "opus") {
    throw new TypeError(`unsupported audio codec: ${options.audioCodec}`);
  }
  // livekit-client 2.20.1 fixes browser audio to Opus and has no audioCodec publish option.
  const normalized: RawOptions = { ...rawOptions(raw), source: livekitTrackSource(record.source) };
  if (options.name != null) normalized.name = options.name;
  if (options.dtx != null) normalized.dtx = options.dtx;
  if (options.red != null) normalized.red = options.red;
  return normalized;
}

function requirePublicationSid(publication: TrackPublication): string {
  if (typeof publication.trackSid !== "string" || publication.trackSid.length === 0) {
    throw new TypeError("LiveKit returned a Track Publication without a SID");
  }
  return publication.trackSid;
}

function publishLocalTrack<TRecord extends AnyLocalTrackRecord, TOptions>({
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
}: {
  roomId: RoomId;
  trackId: LocalTrackId;
  publicationId: PublicationId;
  options: TOptions;
  raw: unknown;
  onComplete: (success: boolean, sid: string | null) => void;
  onReplaced: PublicationReplacementCallback;
  recordFor: (trackId: LocalTrackId) => TRecord;
  normalizeOptions: (options: TOptions, raw: unknown, record: TRecord) => RawOptions;
  mediaName: string;
}) {
  let operation: Promise<string>;
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
      let publication: LocalTrackPublication | null = null;
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
          } catch (error: unknown) {
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
      } catch (error: unknown) {
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
  } catch (error: unknown) {
    operation = Promise.reject(error);
  }
  return operation.then(
    (sid) => {
      onComplete(true, sid);
      return sid;
    },
    (error: unknown) => {
      onComplete(false, null);
      throw error;
    },
  );
}

function setLocalTrackMuted<TRecord extends AnyLocalTrackRecord>({
  trackId,
  publicationId,
  muted,
  onComplete,
  recordFor,
  mediaName,
}: {
  trackId: LocalTrackId;
  publicationId: PublicationId;
  muted: boolean;
  onComplete: (success: boolean) => void;
  recordFor: (trackId: LocalTrackId) => TRecord;
  mediaName: string;
}) {
  let operation: Promise<unknown>;
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
  } catch (error: unknown) {
    operation = Promise.reject(error);
  }
  return operation.then(
    (value) => {
      onComplete(true);
      return value;
    },
    (error: unknown) => {
      onComplete(false);
      throw error;
    },
  );
}

function unpublishLocalTrack<TRecord extends AnyLocalTrackRecord>({
  roomId,
  trackId,
  publicationId,
  onComplete,
  recordFor,
  records,
}: {
  roomId: RoomId;
  trackId: LocalTrackId;
  publicationId: PublicationId;
  onComplete: (success: boolean) => void;
  recordFor: (trackId: LocalTrackId) => TRecord;
  records: Map<LocalTrackId, TRecord>;
}) {
  let operation: Promise<unknown>;
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
        } catch (error: unknown) {
          if (record.explicitUnpublishObservedId !== publicationId) throw error;
        }
        const replacement = [...room.room.localParticipant.trackPublications.values()].find(
          (candidate) => candidate.track === record.track,
        );
        if (replacement) {
          try {
            await room.room.localParticipant.unpublishTrack(record.track, false);
          } catch (error: unknown) {
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
  } catch (error: unknown) {
    operation = Promise.reject(error);
  }
  return operation.then(
    (value) => {
      onComplete(true);
      return value;
    },
    (error: unknown) => {
      onComplete(false);
      throw error;
    },
  );
}

export function publishLocalAudioTrack(
  roomId: RoomId,
  trackId: LocalTrackId,
  publicationId: PublicationId,
  options: AudioPublicationInput,
  raw: unknown,
  onReplaced: PublicationReplacementCallback,
  onComplete: (success: boolean, sid: string | null) => void,
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

export function setLocalAudioTrackMuted(
  trackId: LocalTrackId,
  publicationId: PublicationId,
  muted: boolean,
  onComplete: (success: boolean) => void,
) {
  return setLocalTrackMuted({
    trackId,
    publicationId,
    muted,
    onComplete,
    recordFor: localAudioTrackRecord,
    mediaName: "Local Audio Track",
  });
}

export function unpublishLocalAudioTrack(
  roomId: RoomId,
  trackId: LocalTrackId,
  publicationId: PublicationId,
  onComplete: (success: boolean) => void,
) {
  return unpublishLocalTrack({
    roomId,
    trackId,
    publicationId,
    onComplete,
    recordFor: localAudioTrackRecord,
    records: localAudioTracks,
  });
}

function localTrackSenderStatistics(
  records: Map<LocalTrackId, LocalAudioTrackRecord>,
  recordFor: (trackId: LocalTrackId) => LocalAudioTrackRecord,
  trackId: LocalTrackId,
  publicationId: PublicationId,
): Promise<AudioSenderStats | undefined>;
function localTrackSenderStatistics(
  records: Map<LocalTrackId, LocalVideoTrackRecord>,
  recordFor: (trackId: LocalTrackId) => LocalVideoTrackRecord,
  trackId: LocalTrackId,
  publicationId: PublicationId,
): Promise<VideoSenderStats[]>;
async function localTrackSenderStatistics(
  records: LocalTrackRegistry,
  recordFor: (trackId: LocalTrackId) => AnyLocalTrackRecord,
  trackId: LocalTrackId,
  publicationId: PublicationId,
) {
  const record = recordFor(trackId);
  const publication = record.publication;
  const track = record.track;
  const roomId = record.publishedRoomId;
  const roomRecord = roomSessions.get(roomId as RoomId);
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
    roomSessions.get(roomId as RoomId) !== roomRecord ||
    roomRecord.released
  ) {
    throw new Error("Track Publication is no longer available");
  }
  return statistics;
}

export async function localAudioSenderStatistics(
  trackId: LocalTrackId,
  publicationId: PublicationId,
) {
  const statistics = await localTrackSenderStatistics(
    localAudioTracks,
    localAudioTrackRecord,
    trackId,
    publicationId,
  );
  return statistics ? normalizeSenderStatistics(statistics) : null;
}

export async function rawLocalRtcStatsReport(
  trackId: LocalTrackId,
  publicationId: PublicationId,
  kind: MediaKind,
) {
  const { records, recordFor } = localMediaTrackResources(kind);
  const record = recordFor(trackId);
  const publication = record.publication;
  const track = record.track;
  const roomId = record.publishedRoomId;
  const roomRecord = roomSessions.get(roomId as RoomId);
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
    roomSessions.get(roomId as RoomId) !== roomRecord ||
    roomRecord.released
  ) {
    throw new Error("Track Publication is no longer available");
  }
  return report ?? null;
}

function videoPublicationOptions(
  options: VideoPublicationInput,
  raw: unknown,
  record: LocalVideoTrackRecord,
) {
  const normalized: RawOptions = { ...rawOptions(raw), source: livekitTrackSource(record.source) };
  if (options.name != null) normalized.name = options.name;
  if (options.videoCodec != null) normalized.videoCodec = options.videoCodec;
  if (options.simulcast != null) normalized.simulcast = options.simulcast;
  if (options.videoEncoding != null) {
    const encoding: { maxBitrate?: number; maxFramerate?: number } = {
      maxBitrate: options.videoEncoding.maxBitrate,
      maxFramerate: options.videoEncoding.maxFramerate,
    };
    if (record.source === "screenShare") normalized.screenShareEncoding = encoding;
    else normalized.videoEncoding = encoding;
  }
  return normalized;
}

export function publishLocalVideoTrack(
  roomId: RoomId,
  trackId: LocalTrackId,
  publicationId: PublicationId,
  options: VideoPublicationInput,
  raw: unknown,
  onReplaced: PublicationReplacementCallback,
  onComplete: (success: boolean, sid: string | null) => void,
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

export function setLocalVideoTrackMuted(
  trackId: LocalTrackId,
  publicationId: PublicationId,
  muted: boolean,
  onComplete: (success: boolean) => void,
) {
  return setLocalTrackMuted({
    trackId,
    publicationId,
    muted,
    onComplete,
    recordFor: localVideoTrackRecord,
    mediaName: "Local Video Track",
  });
}

export function unpublishLocalVideoTrack(
  roomId: RoomId,
  trackId: LocalTrackId,
  publicationId: PublicationId,
  onComplete: (success: boolean) => void,
) {
  return unpublishLocalTrack({
    roomId,
    trackId,
    publicationId,
    onComplete,
    recordFor: localVideoTrackRecord,
    records: localVideoTracks,
  });
}

export async function localVideoSenderStatistics(
  trackId: LocalTrackId,
  publicationId: PublicationId,
) {
  const statistics = await localTrackSenderStatistics(
    localVideoTracks,
    localVideoTrackRecord,
    trackId,
    publicationId,
  );
  if (!Array.isArray(statistics)) throw new TypeError("LiveKit returned invalid sender statistics");
  return statistics.map(normalizeVideoSenderStatistics);
}

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
  } else if (current !== participant) return;
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

function remotePublicationIsCurrent(
  record: RoomRecord,
  participant: RemoteParticipant,
  publication: RemoteTrackPublication,
) {
  return (
    record.room.remoteParticipants.get(participant?.identity) === participant &&
    participant?.trackPublications.get(publication?.trackSid) === publication
  );
}

function remotePublicationWasReplaced(
  record: RoomRecord,
  participant: RemoteParticipant,
  publication: RemoteTrackPublication,
) {
  const currentParticipant = record.room.remoteParticipants.get(participant?.identity);
  if (currentParticipant !== participant) return true;
  const sid = publication?.trackSid;
  const currentPublication = participant?.trackPublications.get(sid);
  return currentPublication != null && currentPublication !== publication;
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

function reconcileLocalTrackPublished(roomRecord: RoomRecord, publication: LocalTrackPublication) {
  const reconcile = <TRecord extends AnyLocalTrackRecord>(
    records: Map<LocalTrackId, TRecord>,
  ) => {
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

function reconcileLocalTrackUnpublished(roomRecord: RoomRecord, publication: LocalTrackPublication) {
  const reconcile = <TRecord extends AnyLocalTrackRecord>(
    records: Map<LocalTrackId, TRecord>,
    finishDropped: (record: TRecord, trackId: LocalTrackId) => void,
  ) => {
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

function releaseLocalTrackAssociations<TRecord extends AnyLocalTrackRecord>(
  tracks: Map<LocalTrackId, TRecord>,
  roomId: RoomId,
  finishDropped: (record: TRecord, trackId: LocalTrackId) => void,
) {
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

async function releaseRoom(record: RoomRecord) {
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
    remoteMediaTrackIds: new Set(),
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
    reconcileLocalTrackPublished(record, publication);
    queueEvent(record, { type: "localTrackPublicationsChanged" });
  });
  listen(record, events.LocalTrackUnpublished, (publication: LocalTrackPublication) => {
    reconcileLocalTrackUnpublished(record, publication);
    queueEvent(record, { type: "localTrackPublicationsChanged" });
  });
  listen(record, events.TrackMuted, (publication: TrackPublication, participant: Participant) => {
    if (participant === room.localParticipant) {
      queueEvent(record, { type: "localTrackPublicationsChanged" });
    } else if (
      remotePublicationIsCurrent(
        record,
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
        record,
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
    terminateRemoteMediaWhere(
      (track) => track.roomId === record.id && track.participant === participant,
    );
    queueRemoteParticipantEvent(record, "remoteParticipantLeft", participant);
  });
  listen(record, events.TrackPublished, (
    publication: RemoteTrackPublication,
    participant: RemoteParticipant,
  ) => {
    if (!remotePublicationIsCurrent(record, participant, publication)) return;
    queueRemotePublicationsEvent(record, participant);
  });
  listen(record, events.TrackUnpublished, (
    publication: RemoteTrackPublication,
    participant: RemoteParticipant,
  ) => {
    terminateRemoteMediaWhere(
      (track) =>
        track.roomId === record.id &&
        track.participant === participant &&
        track.publication === publication,
    );
    if (remotePublicationWasReplaced(record, participant, publication)) return;
    queueRemotePublicationsEvent(record, participant);
  });
  listen(record, events.TrackSubscribed, (
    _track: RemoteTrack,
    publication: RemoteTrackPublication,
    participant: RemoteParticipant,
  ) => {
    if (!remotePublicationIsCurrent(record, participant, publication)) return;
    reconcileRemoteMediaTracks(record);
    queueRemotePublicationsEvent(record, participant);
  });
  listen(record, events.TrackUnsubscribed, (
    track: RemoteTrack,
    publication: RemoteTrackPublication,
    participant: RemoteParticipant,
  ) => {
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
  listen(record, events.TrackSubscriptionFailed, (_trackSid: string, participant: RemoteParticipant) => {
    queueRemotePublicationsEvent(record, participant);
  });
  listen(record, events.TrackSubscriptionStatusChanged, (
    publication: RemoteTrackPublication,
    _status: unknown,
    participant: RemoteParticipant,
  ) => {
    if (!remotePublicationIsCurrent(record, participant, publication)) return;
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
