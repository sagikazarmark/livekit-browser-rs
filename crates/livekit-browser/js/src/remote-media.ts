import {
  normalizeReceiverStatistics,
  optionalFiniteNumber,
  optionalString,
  optionalUnsignedInteger,
} from "./statistics";
import type { AudioAnalyserOwner } from "./audio-analysers";
import type {
  AudioReceiverStats,
  RemoteAudioTrack,
  RemoteParticipant,
  RemoteTrack,
  RemoteTrackPublication,
  RemoteVideoTrack,
  Room,
  VideoReceiverStats,
} from "livekit-client";

type RoomId = number;
type RemoteMediaTrackId = number;
type AttachmentId = number;
export type RemoteObjectGeneration = number;
type MediaKind = "audio" | "video";
type TerminalCallback = () => void;
type RemoteMediaTrack = RemoteAudioTrack | RemoteVideoTrack;
type MediaElement = HTMLAudioElement | HTMLVideoElement;

export interface RemoteParticipantLocator {
  identity: string;
  sid: string;
  generation: RemoteObjectGeneration;
}

export interface RemotePublicationLocator {
  participant: RemoteParticipantLocator;
  sid: string;
  generation: RemoteObjectGeneration;
}

interface RemoteMediaRoom {
  readonly room: Room;
  readonly released: boolean;
}

interface AudioAnalyserSubsystem {
  createAudioAnalyserOwner(): AudioAnalyserOwner;
  createAudioAnalyser(
    owner: AudioAnalyserOwner,
    sourceTrack: MediaStreamTrack,
    isCurrent: () => boolean,
    onTerminal: TerminalCallback,
  ): number;
  mediaStreamAudioSource(track: { mediaStreamTrack?: MediaStreamTrack }): MediaStreamTrack;
  terminateAudioAnalyserOwner(owner: AudioAnalyserOwner): void;
}

export interface RemoteMediaDependencies {
  getRoom(roomId: RoomId): RemoteMediaRoom | undefined;
  allocateAttachmentId(): AttachmentId;
  audioAnalysers: AudioAnalyserSubsystem;
}

interface RemoteMediaTrackRecord {
  id: RemoteMediaTrackId;
  roomId: RoomId;
  locator: RemotePublicationLocator;
  participant: RemoteParticipant;
  publication: RemoteTrackPublication;
  track: RemoteMediaTrack;
  mediaGeneration: RemoteObjectGeneration;
  kind: MediaKind;
  attachments: Map<AttachmentId, MediaElement>;
  audioAnalyserOwner?: AudioAnalyserOwner;
  terminal: boolean;
  terminalCallback: TerminalCallback | null;
  endedListener: TerminalCallback | null;
}

interface RemoteObjectGenerationState {
  generation: RemoteObjectGeneration;
  identity?: string;
  sid?: string;
  source?: MediaStreamTrack | RemoteTrack;
}

export function createRemoteMediaSubsystem(dependencies: RemoteMediaDependencies) {
  const { getRoom, allocateAttachmentId, audioAnalysers } = dependencies;
  let nextRemoteMediaTrackId: RemoteMediaTrackId = 1;
  let nextRemoteObjectGeneration = 1;
  const remoteMediaTracks = new Map<RemoteMediaTrackId, RemoteMediaTrackRecord>();
  const remoteMediaTrackIdsByRoom = new Map<RoomId, Set<RemoteMediaTrackId>>();
  const remoteParticipantGenerations = new WeakMap<
    RemoteParticipant,
    RemoteObjectGenerationState
  >();
  const remotePublicationGenerations = new WeakMap<
    RemoteTrackPublication,
    RemoteObjectGenerationState
  >();
  const remoteMediaGenerations = new WeakMap<RemoteTrack, RemoteObjectGenerationState>();

  function participantGeneration(participant: RemoteParticipant): RemoteObjectGeneration {
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

  function publicationGeneration(
    publication: RemoteTrackPublication,
    sid: string = publication.trackSid,
  ): RemoteObjectGeneration {
    let state = remotePublicationGenerations.get(publication);
    if (!state || state.sid !== sid) {
      state = { generation: nextRemoteObjectGeneration++, sid };
      remotePublicationGenerations.set(publication, state);
    }
    return state.generation;
  }

  function mediaGeneration(track: RemoteTrack): RemoteObjectGeneration {
    const source = track.mediaStreamTrack ?? track;
    let state = remoteMediaGenerations.get(track);
    if (state?.source !== source) {
      state = { generation: nextRemoteObjectGeneration++, source };
      remoteMediaGenerations.set(track, state);
    }
    return state.generation;
  }

  function currentParticipant(roomId: RoomId, locator: RemoteParticipantLocator) {
    const roomRecord = getRoom(roomId);
    if (!roomRecord || roomRecord.released) return null;
    const participant = roomRecord.room.remoteParticipants.get(locator.identity);
    if (
      !participant ||
      participant.sid !== locator.sid ||
      participantGeneration(participant) !== locator.generation
    ) {
      return null;
    }
    return participant;
  }

  function currentPublication(roomId: RoomId, locator: RemotePublicationLocator) {
    const participant = currentParticipant(roomId, locator.participant);
    if (!participant) return null;
    const publication = participant.trackPublications.get(locator.sid);
    if (!publication || publicationGeneration(publication) !== locator.generation) return null;
    return { participant, publication };
  }

  function participantIsCurrent(roomId: RoomId, participant: RemoteParticipant) {
    const roomRecord = getRoom(roomId);
    return (
      !!roomRecord &&
      !roomRecord.released &&
      roomRecord.room.remoteParticipants.get(participant?.identity) === participant
    );
  }

  function publicationIsCurrent(
    roomId: RoomId,
    participant: RemoteParticipant,
    publication: RemoteTrackPublication,
  ) {
    return (
      participantIsCurrent(roomId, participant) &&
      participant?.trackPublications.get(publication?.trackSid) === publication
    );
  }

  function publicationWasReplaced(
    roomId: RoomId,
    participant: RemoteParticipant,
    publication: RemoteTrackPublication,
  ) {
    const roomRecord = getRoom(roomId);
    const currentParticipant = roomRecord?.room.remoteParticipants.get(participant?.identity);
    if (currentParticipant !== participant) return true;
    const sid = publication?.trackSid;
    const currentPublication = participant?.trackPublications.get(sid);
    return currentPublication != null && currentPublication !== publication;
  }

  function mediaTrackRecord(trackId: RemoteMediaTrackId): RemoteMediaTrackRecord {
    const record = remoteMediaTracks.get(trackId);
    if (!record || record.terminal) throw new Error("Remote Media Track is no longer available");
    return record;
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
    if (record.audioAnalyserOwner) {
      audioAnalysers.terminateAudioAnalyserOwner(record.audioAnalyserOwner);
    }
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
    const roomTrackIds = remoteMediaTrackIdsByRoom.get(record.roomId);
    roomTrackIds?.delete(record.id);
    if (roomTrackIds?.size === 0) remoteMediaTrackIdsByRoom.delete(record.roomId);
    callback?.();
  }

  function terminateRemoteMediaWhere(predicate: (record: RemoteMediaTrackRecord) => boolean) {
    for (const record of remoteMediaTracks.values()) {
      if (predicate(record)) terminateRemoteMediaTrack(record);
    }
  }

  function mediaTrackIsCurrent(record: RemoteMediaTrackRecord) {
    const publication = currentPublication(record.roomId, record.locator);
    return (
      publication?.participant === record.participant &&
      publication?.publication === record.publication &&
      publication?.publication.track === record.track &&
      mediaGeneration(record.track) === record.mediaGeneration
    );
  }

  function reconcileRoom(roomId: RoomId) {
    const trackIds = remoteMediaTrackIdsByRoom.get(roomId);
    if (!trackIds) return;
    for (const trackId of trackIds) {
      const record = remoteMediaTracks.get(trackId);
      if (!record || record.terminal) continue;
      if (!mediaTrackIsCurrent(record)) terminateRemoteMediaTrack(record);
    }
  }

  function rawParticipant(roomId: RoomId, locator: RemoteParticipantLocator) {
    const roomRecord = getRoom(roomId);
    if (!roomRecord || roomRecord.released) throw new Error("Room Session is no longer available");
    const participant = currentParticipant(roomId, locator);
    if (!participant) throw new Error("Remote Participant is no longer available");
    return participant;
  }

  function rawPublication(roomId: RoomId, locator: RemotePublicationLocator) {
    const roomRecord = getRoom(roomId);
    if (!roomRecord || roomRecord.released) throw new Error("Room Session is no longer available");
    const current = currentPublication(roomId, locator);
    if (!current) throw new Error("Remote Track Publication is no longer available");
    return current.publication;
  }

  function rawMediaTrack(trackId: RemoteMediaTrackId) {
    const record = mediaTrackRecord(trackId);
    if (!mediaTrackIsCurrent(record)) {
      terminateRemoteMediaTrack(record);
      throw new Error("Remote Media Track is no longer available");
    }
    return record.track;
  }

  function setTrackSubscription(
    roomId: RoomId,
    locator: RemotePublicationLocator,
    subscribed: boolean,
  ) {
    const roomRecord = getRoom(roomId);
    if (!roomRecord || roomRecord.released) throw new Error("Room Session is no longer available");
    const current = currentPublication(roomId, locator);
    if (!current) throw new Error("Remote Track Publication is no longer available");
    current.publication.setSubscribed(subscribed);
  }

  function resolveMediaTrack(
    roomId: RoomId,
    locator: RemotePublicationLocator,
    expectedMediaGeneration: RemoteObjectGeneration,
    onTerminal: TerminalCallback,
  ) {
    const roomRecord = getRoom(roomId);
    if (!roomRecord || roomRecord.released) throw new Error("Room Session is no longer available");
    const current = currentPublication(roomId, locator);
    const publication = current?.publication;
    const track = publication?.track;
    if (!track) return null;
    const currentMediaGeneration = mediaGeneration(track);
    if (currentMediaGeneration !== expectedMediaGeneration) {
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
      audioAnalyserOwner:
        kind === "audio" ? audioAnalysers.createAudioAnalyserOwner() : undefined,
      terminal: false,
      terminalCallback: onTerminal,
      endedListener: null,
    };
    record.endedListener = () => terminateRemoteMediaTrack(record);
    track.on(globalThis.LivekitClient.TrackEvent.Ended, record.endedListener);
    remoteMediaTracks.set(id, record);
    let roomTrackIds = remoteMediaTrackIdsByRoom.get(roomId);
    if (!roomTrackIds) {
      roomTrackIds = new Set();
      remoteMediaTrackIdsByRoom.set(roomId, roomTrackIds);
    }
    roomTrackIds.add(id);
    return { id, kind, generation: currentMediaGeneration };
  }

  async function remoteTrackReceiverStatistics(
    trackId: RemoteMediaTrackId,
    kind: MediaKind,
  ): Promise<AudioReceiverStats | VideoReceiverStats | undefined> {
    const record = mediaTrackRecord(trackId);
    if (record.kind !== kind) throw new TypeError(`Remote Media Track is not ${kind}`);
    if (!mediaTrackIsCurrent(record)) {
      terminateRemoteMediaTrack(record);
      throw new Error("Remote Media Track is no longer available");
    }
    const track = record.track;
    const statistics = await track.getReceiverStats();
    if (
      mediaTrackRecord(trackId) !== record ||
      record.track !== track ||
      !mediaTrackIsCurrent(record)
    ) {
      terminateRemoteMediaTrack(record);
      throw new Error("Remote Media Track is no longer available");
    }
    return statistics;
  }

  async function audioReceiverStatistics(trackId: RemoteMediaTrackId) {
    const statistics = (await remoteTrackReceiverStatistics(trackId, "audio")) as
      | AudioReceiverStats
      | undefined;
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

  async function videoReceiverStatistics(trackId: RemoteMediaTrackId) {
    const statistics = (await remoteTrackReceiverStatistics(trackId, "video")) as
      | VideoReceiverStats
      | undefined;
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

  async function rawRtcStatsReport(trackId: RemoteMediaTrackId) {
    const record = mediaTrackRecord(trackId);
    if (!mediaTrackIsCurrent(record)) {
      terminateRemoteMediaTrack(record);
      throw new Error("Remote Media Track is no longer available");
    }
    const track = record.track;
    if (typeof track.getRTCStatsReport !== "function") {
      throw new TypeError("LiveKit Remote Media Track does not expose RTCStatsReport");
    }
    const report = await track.getRTCStatsReport();
    if (
      mediaTrackRecord(trackId) !== record ||
      record.track !== track ||
      !mediaTrackIsCurrent(record)
    ) {
      terminateRemoteMediaTrack(record);
      throw new Error("Remote Media Track is no longer available");
    }
    return report ?? null;
  }

  function attachVideoTrack(trackId: RemoteMediaTrackId, element: HTMLVideoElement) {
    const record = mediaTrackRecord(trackId);
    if (record.kind !== "video") throw new TypeError("Remote Media Track is not video");
    const attachmentId = allocateAttachmentId();
    record.track.attach(element);
    record.attachments.set(attachmentId, element);
    return attachmentId;
  }

  function attachAudioTrack(trackId: RemoteMediaTrackId, element: HTMLAudioElement) {
    const record = mediaTrackRecord(trackId);
    if (record.kind !== "audio") throw new TypeError("Remote Media Track is not audio");
    const attachmentId = allocateAttachmentId();
    record.track.attach(element);
    record.attachments.set(attachmentId, element);
    return attachmentId;
  }

  function detachTrack(trackId: RemoteMediaTrackId, attachmentId: AttachmentId) {
    detachRemoteElement(mediaTrackRecord(trackId), attachmentId);
  }

  function dropTrackAttachment(trackId: RemoteMediaTrackId, attachmentId: AttachmentId) {
    const record = remoteMediaTracks.get(trackId);
    if (!record) return;
    try {
      detachRemoteElement(record, attachmentId);
    } catch {}
  }

  function dropMediaTrack(trackId: RemoteMediaTrackId) {
    const record = remoteMediaTracks.get(trackId);
    if (record) terminateRemoteMediaTrack(record);
  }

  function dropMediaTrackDescriptor(descriptor: unknown) {
    const id = (descriptor as { id?: unknown } | null)?.id;
    if (typeof id === "number" && Number.isInteger(id)) dropMediaTrack(id);
  }

  function createRemoteAudioAnalyser(
    trackId: RemoteMediaTrackId,
    onTerminal: TerminalCallback,
  ) {
    const record = mediaTrackRecord(trackId);
    if (record.kind !== "audio") throw new TypeError("Remote Media Track is not audio");
    if (!mediaTrackIsCurrent(record)) {
      terminateRemoteMediaTrack(record);
      throw new Error("Remote Media Track is no longer available");
    }
    const sourceTrack = audioAnalysers.mediaStreamAudioSource(record.track as RemoteAudioTrack);
    return audioAnalysers.createAudioAnalyser(
      record.audioAnalyserOwner!,
      sourceTrack,
      () =>
        remoteMediaTracks.get(record.id) === record &&
        !record.terminal &&
        mediaTrackIsCurrent(record) &&
        record.track.mediaStreamTrack === sourceTrack,
      onTerminal,
    );
  }

  function participantDisconnected(roomId: RoomId, participant: RemoteParticipant) {
    terminateRemoteMediaWhere(
      (track) => track.roomId === roomId && track.participant === participant,
    );
  }

  function publicationUnpublished(
    roomId: RoomId,
    participant: RemoteParticipant,
    publication: RemoteTrackPublication,
  ) {
    terminateRemoteMediaWhere(
      (track) =>
        track.roomId === roomId &&
        track.participant === participant &&
        track.publication === publication,
    );
  }

  function trackSubscribed(roomId: RoomId) {
    reconcileRoom(roomId);
  }

  function trackUnsubscribed(
    roomId: RoomId,
    track: RemoteTrack,
    publication: RemoteTrackPublication,
    participant: RemoteParticipant,
  ) {
    terminateRemoteMediaWhere(
      (media) =>
        media.roomId === roomId &&
        media.participant === participant &&
        media.publication === publication &&
        media.track === track,
    );
  }

  function releaseRoom(roomId: RoomId) {
    terminateRemoteMediaWhere((track) => track.roomId === roomId);
    remoteMediaTrackIdsByRoom.delete(roomId);
  }

  return {
    participantGeneration,
    publicationGeneration,
    mediaGeneration,
    participantIsCurrent,
    publicationIsCurrent,
    publicationWasReplaced,
    reconcileRoom,
    rawParticipant,
    rawPublication,
    rawMediaTrack,
    setTrackSubscription,
    resolveMediaTrack,
    audioReceiverStatistics,
    videoReceiverStatistics,
    rawRtcStatsReport,
    attachVideoTrack,
    detachVideoTrack: detachTrack,
    dropVideoTrackAttachment: dropTrackAttachment,
    attachAudioTrack,
    detachAudioTrack: detachTrack,
    dropAudioTrackAttachment: dropTrackAttachment,
    dropMediaTrack,
    dropMediaTrackDescriptor,
    createRemoteAudioAnalyser,
    participantDisconnected,
    publicationUnpublished,
    trackSubscribed,
    trackUnsubscribed,
    releaseRoom,
  };
}

function publicationKind(kind: unknown) {
  switch (kind) {
    case "audio":
    case "video":
    case "data":
      return kind;
    default:
      return "unknown";
  }
}
