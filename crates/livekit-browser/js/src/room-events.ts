import type {
  LocalTrackPublication,
  Participant,
  RemoteParticipant,
  RemoteTrack,
  RemoteTrackPublication,
  Room,
  RoomEventCallbacks,
  TrackPublication,
} from "livekit-client";
import type { RoomEventKind } from "./generated/adapter-contracts";

export interface RemoteDataTrackLike {
  info: { sid?: string; name?: string; usesE2ee?: boolean };
  publisherIdentity: string;
}

type ConnectionQualityChangedEvent = Extract<
  RoomEventKind,
  { type: "participantConnectionQualityChanged" }
>;
type PendingConnectionQualityChangedEvent = Omit<ConnectionQualityChangedEvent, "quality"> & {
  source_participant: Participant;
};

export type PendingRoomEvent =
  | Exclude<RoomEventKind, ConnectionQualityChangedEvent>
  | PendingConnectionQualityChangedEvent;
export type NormalizedRoomEvent = RoomEventKind;

export interface RoomEventSnapshotState {
  readonly revision: number;
  readonly remoteDataTracksBySid: Map<
    string,
    { participant: RemoteParticipant; track: RemoteDataTrackLike }
  >;
}

type RoomListener = (...args: any[]) => void;
type RoomUpdateCallback<TSnapshot> = (
  update: { snapshot: TSnapshot; events: RoomEventKind[] },
) => void;
type DataPacketCallback = (
  payload: Uint8Array,
  participantIdentity: string | null,
  reliable: boolean,
) => void;
type MessageCallback = (message: unknown) => void;

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

interface RoomEvents {
  ConnectionStateChanged: keyof RoomEventCallbacks;
  ParticipantMetadataChanged: keyof RoomEventCallbacks;
  ParticipantAttributesChanged: keyof RoomEventCallbacks;
  ParticipantPermissionsChanged: keyof RoomEventCallbacks;
  ConnectionQualityChanged: keyof RoomEventCallbacks;
  ActiveSpeakersChanged: keyof RoomEventCallbacks;
  LocalTrackPublished: keyof RoomEventCallbacks;
  LocalTrackUnpublished: keyof RoomEventCallbacks;
  TrackMuted: keyof RoomEventCallbacks;
  TrackUnmuted: keyof RoomEventCallbacks;
  ParticipantConnected: keyof RoomEventCallbacks;
  ParticipantDisconnected: keyof RoomEventCallbacks;
  TrackPublished: keyof RoomEventCallbacks;
  TrackUnpublished: keyof RoomEventCallbacks;
  TrackSubscribed: keyof RoomEventCallbacks;
  TrackUnsubscribed: keyof RoomEventCallbacks;
  TrackSubscriptionFailed: keyof RoomEventCallbacks;
  TrackSubscriptionStatusChanged: keyof RoomEventCallbacks;
  DataTrackPublished: keyof RoomEventCallbacks;
  DataTrackUnpublished: keyof RoomEventCallbacks;
  DataReceived: keyof RoomEventCallbacks;
  ChatMessage: keyof RoomEventCallbacks;
  TranscriptionReceived?: keyof RoomEventCallbacks;
  RoomMetadataChanged: keyof RoomEventCallbacks;
  AudioPlaybackStatusChanged: keyof RoomEventCallbacks;
}

export interface RoomEventDependencies<TSnapshot> {
  room: Room;
  roomEvents: RoomEvents;
  dataPacketKinds: { RELIABLE: unknown; LOSSY: unknown };
  snapshots: {
    normalize(state: RoomEventSnapshotState): TSnapshot;
    normalizeEvent(
      state: RoomEventSnapshotState,
      event: PendingRoomEvent,
      snapshot: TSnapshot,
    ): NormalizedRoomEvent | null;
  };
  localMedia: {
    published(publication: LocalTrackPublication): void;
    unpublished(publication: LocalTrackPublication): void;
  };
  remoteMedia: {
    participantIsCurrent(participant: RemoteParticipant): boolean;
    publicationIsCurrent(
      participant: RemoteParticipant,
      publication: RemoteTrackPublication,
    ): boolean;
    publicationWasReplaced(
      participant: RemoteParticipant,
      publication: RemoteTrackPublication,
    ): boolean;
    participantDisconnected(participant: RemoteParticipant): void;
    publicationUnpublished(
      participant: RemoteParticipant,
      publication: RemoteTrackPublication,
    ): void;
    trackSubscribed(): void;
    trackUnsubscribed(
      track: RemoteTrack,
      publication: RemoteTrackPublication,
      participant: RemoteParticipant,
    ): void;
  };
  dataStreams: {
    handleFlowControl(
      payload: Uint8Array,
      participant: RemoteParticipant | undefined,
      reliable: boolean,
      topic: string,
    ): boolean;
    participantDisconnected(participant: RemoteParticipant): void;
  };
}

export interface RoomActivation<TSnapshot> {
  update: RoomUpdateCallback<TSnapshot>;
  dataPacket: DataPacketCallback;
  chatMessage: MessageCallback;
  transcriptionSegment: MessageCallback;
}

export function createRoomEventOrchestrator<TSnapshot>(
  dependencies: RoomEventDependencies<TSnapshot>,
) {
  const { room, roomEvents: events } = dependencies;
  const listeners: Array<[keyof RoomEventCallbacks, RoomListener]> = [];
  const pendingEvents: PendingRoomEvent[] = [];
  const state: { revision: number; remoteDataTracksBySid: RoomEventSnapshotState["remoteDataTracksBySid"] } = {
    revision: 1,
    remoteDataTracksBySid: new Map(),
  };
  let activation: RoomActivation<TSnapshot> | null = null;
  let updateTimer: ReturnType<typeof setTimeout> | null = null;
  let disposed = false;

  function scheduleUpdate() {
    if (!activation || updateTimer != null || pendingEvents.length === 0 || disposed) return;
    updateTimer = setTimeout(() => {
      updateTimer = null;
      if (!activation || pendingEvents.length === 0 || disposed) return;

      const queued = pendingEvents.splice(0);
      state.revision += 1;
      const snapshot = dependencies.snapshots.normalize(state);
      const normalized = queued
        .map((event) => dependencies.snapshots.normalizeEvent(state, event, snapshot))
        .filter((event): event is NormalizedRoomEvent => event != null);
      activation.update({ snapshot, events: normalized } satisfies {
        snapshot: TSnapshot;
        events: RoomEventKind[];
      });
    }, 0);
  }

  function queueEvent(event: PendingRoomEvent) {
    if (disposed) return;
    pendingEvents.push(event);
    scheduleUpdate();
  }

  function queueRemoteParticipantEvent(
    type: "remoteParticipantJoined" | "remoteParticipantChanged" | "remoteParticipantLeft",
    participant: RemoteParticipant,
  ) {
    const identity = participant?.identity;
    if (typeof identity !== "string" || identity.length === 0) return;
    const current = room.remoteParticipants.get(identity);
    if (type === "remoteParticipantLeft") {
      if (current && current !== participant) return;
    } else if (!dependencies.remoteMedia.participantIsCurrent(participant)) return;
    queueEvent({ type, identity });
  }

  function queueRemotePublicationsEventByIdentity(participantIdentity: unknown) {
    if (typeof participantIdentity === "string" && participantIdentity.length > 0) {
      queueEvent({
        type: "remoteTrackPublicationsChanged",
        participant_identity: participantIdentity,
      });
    }
  }

  function queueRemotePublicationsEvent(participant: RemoteParticipant) {
    if (room.remoteParticipants.get(participant?.identity) !== participant) return;
    queueRemotePublicationsEventByIdentity(participant?.identity);
  }

  function queueRemoteDataTracksEvent(participant: RemoteParticipant) {
    if (room.remoteParticipants.get(participant?.identity) !== participant) return;
    queueEvent({
      type: "remoteDataTracksChanged",
      participant_identity: participant.identity,
    });
  }

  function dataTrackWithSid(participant: RemoteParticipant | undefined, sid: string) {
    for (const track of participant?.dataTracks.values() ?? []) {
      if (track.info?.sid === sid) return track as RemoteDataTrackLike;
    }
    return null;
  }

  function listen(event: keyof RoomEventCallbacks, listener: RoomListener) {
    room.on(event, listener as RoomEventCallbacks[typeof event]);
    listeners.push([event, listener]);
  }

  try {
    listen(
      events.ConnectionStateChanged,
      ((connectionState) => {
        queueEvent({ type: "connectionStateChanged", state: connectionState });
      }) satisfies RoomEventCallbacks["connectionStateChanged"],
    );
    listen(events.ParticipantMetadataChanged, (_previous: unknown, participant: Participant) => {
      if (participant === room.localParticipant) queueEvent({ type: "localParticipantChanged" });
      else queueRemoteParticipantEvent("remoteParticipantChanged", participant as RemoteParticipant);
    });
    listen(events.ParticipantAttributesChanged, (_changed: unknown, participant: Participant) => {
      if (participant === room.localParticipant) queueEvent({ type: "localParticipantChanged" });
      else queueRemoteParticipantEvent("remoteParticipantChanged", participant as RemoteParticipant);
    });
    listen(events.ParticipantPermissionsChanged, (_previous: unknown, participant: Participant) => {
      if (participant === room.localParticipant) queueEvent({ type: "localParticipantChanged" });
    });
    listen(events.ConnectionQualityChanged, (_quality: unknown, participant: Participant) => {
      const identity = participant?.identity;
      if (typeof identity !== "string" || identity.length === 0) return;
      if (participant !== room.localParticipant && room.remoteParticipants.get(identity) !== participant) {
        return;
      }
      queueEvent({
        type: "participantConnectionQualityChanged",
        participant_identity: identity,
        source_participant: participant,
      });
    });
    listen(events.ActiveSpeakersChanged, () => queueEvent({ type: "activeSpeakersChanged" }));
    listen(events.LocalTrackPublished, (publication: LocalTrackPublication) => {
      dependencies.localMedia.published(publication);
      queueEvent({ type: "localTrackPublicationsChanged" });
    });
    listen(events.LocalTrackUnpublished, (publication: LocalTrackPublication) => {
      dependencies.localMedia.unpublished(publication);
      queueEvent({ type: "localTrackPublicationsChanged" });
    });
    listen(events.TrackMuted, (publication: TrackPublication, participant: Participant) => {
      if (participant === room.localParticipant) {
        queueEvent({ type: "localTrackPublicationsChanged" });
      } else if (dependencies.remoteMedia.publicationIsCurrent(
        participant as RemoteParticipant,
        publication as RemoteTrackPublication,
      )) {
        queueRemotePublicationsEvent(participant as RemoteParticipant);
      }
    });
    listen(events.TrackUnmuted, (publication: TrackPublication, participant: Participant) => {
      if (participant === room.localParticipant) {
        queueEvent({ type: "localTrackPublicationsChanged" });
      } else if (dependencies.remoteMedia.publicationIsCurrent(
        participant as RemoteParticipant,
        publication as RemoteTrackPublication,
      )) {
        queueRemotePublicationsEvent(participant as RemoteParticipant);
      }
    });
    listen(events.ParticipantConnected, (participant: RemoteParticipant) => {
      queueRemoteParticipantEvent("remoteParticipantJoined", participant);
    });
    listen(events.ParticipantDisconnected, (participant: RemoteParticipant) => {
      dependencies.dataStreams.participantDisconnected(participant);
      dependencies.remoteMedia.participantDisconnected(participant);
      queueRemoteParticipantEvent("remoteParticipantLeft", participant);
    });
    listen(events.TrackPublished, (
      publication: RemoteTrackPublication,
      participant: RemoteParticipant,
    ) => {
      if (!dependencies.remoteMedia.publicationIsCurrent(participant, publication)) return;
      queueRemotePublicationsEvent(participant);
    });
    listen(events.TrackUnpublished, (
      publication: RemoteTrackPublication,
      participant: RemoteParticipant,
    ) => {
      dependencies.remoteMedia.publicationUnpublished(participant, publication);
      if (dependencies.remoteMedia.publicationWasReplaced(participant, publication)) return;
      queueRemotePublicationsEvent(participant);
    });
    listen(events.TrackSubscribed, (
      _track: RemoteTrack,
      publication: RemoteTrackPublication,
      participant: RemoteParticipant,
    ) => {
      if (!dependencies.remoteMedia.publicationIsCurrent(participant, publication)) return;
      dependencies.remoteMedia.trackSubscribed();
      queueRemotePublicationsEvent(participant);
    });
    listen(events.TrackUnsubscribed, (
      track: RemoteTrack,
      publication: RemoteTrackPublication,
      participant: RemoteParticipant,
    ) => {
      dependencies.remoteMedia.trackUnsubscribed(track, publication, participant);
      if (dependencies.remoteMedia.publicationWasReplaced(participant, publication)) return;
      queueRemotePublicationsEvent(participant);
    });
    listen(events.TrackSubscriptionFailed, (_trackSid: string, participant: RemoteParticipant) => {
      queueRemotePublicationsEvent(participant);
    });
    listen(events.TrackSubscriptionStatusChanged, (
      publication: RemoteTrackPublication,
      _status: unknown,
      participant: RemoteParticipant,
    ) => {
      if (!dependencies.remoteMedia.publicationIsCurrent(participant, publication)) return;
      queueRemotePublicationsEvent(participant);
    });
    listen(events.DataTrackPublished, (track: RemoteDataTrackLike) => {
      const sid = track.info?.sid;
      if (typeof sid !== "string" || sid.length === 0) return;
      queueMicrotask(() => {
        if (disposed) return;
        const participant = room.remoteParticipants.get(track.publisherIdentity);
        if (!participant || dataTrackWithSid(participant, sid) !== track) return;
        state.remoteDataTracksBySid.set(sid, { participant, track });
        queueRemoteDataTracksEvent(participant);
      });
    });
    listen(events.DataTrackUnpublished, (sid: string) => {
      queueMicrotask(() => {
        if (disposed) return;
        const dataTrack = state.remoteDataTracksBySid.get(sid);
        if (!dataTrack) return;
        const currentParticipant = room.remoteParticipants.get(dataTrack.participant.identity);
        const currentTrack = dataTrackWithSid(currentParticipant, sid);
        if (currentTrack && currentParticipant) {
          state.remoteDataTracksBySid.set(sid, { participant: currentParticipant, track: currentTrack });
          return;
        }
        state.remoteDataTracksBySid.delete(sid);
        if (currentParticipant === dataTrack.participant) {
          queueRemoteDataTracksEvent(currentParticipant);
        }
      });
    });
    listen(events.DataReceived, (
      payload: Uint8Array,
      participant: RemoteParticipant | undefined,
      kind: unknown,
      topic: string,
    ) => {
      const reliable = kind === dependencies.dataPacketKinds.RELIABLE;
      if (!reliable && kind !== dependencies.dataPacketKinds.LOSSY) return;
      if (dependencies.dataStreams.handleFlowControl(payload, participant, reliable, topic)) return;
      activation?.dataPacket(
        payload,
        typeof participant?.identity === "string" ? participant.identity : null,
        reliable,
      );
    });
    listen(events.ChatMessage, (
      message: ChatMessageLike,
      participant: RemoteParticipant | undefined,
    ) => {
      try {
        activation?.chatMessage(normalizeChatMessage(message, participant?.identity));
      } catch {
        // Ignore malformed SDK events rather than exposing a fabricated typed Chat Message.
      }
    });
    if (typeof events.TranscriptionReceived === "string") {
      listen(events.TranscriptionReceived, (
        segments: unknown,
        participant: Participant | undefined,
        publication: TrackPublication | undefined,
      ) => {
        if (!Array.isArray(segments) || !transcriptionSourceIsCurrent(room, participant, publication)) {
          return;
        }
        for (const segment of segments) {
          try {
            activation?.transcriptionSegment(
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
    listen(events.RoomMetadataChanged, () => queueEvent({ type: "roomMetadataChanged" }));
    listen(events.AudioPlaybackStatusChanged, () => {
      queueEvent({
        type: "audioPlaybackChanged",
        state: room.canPlaybackAudio === false ? "blocked" : "allowed",
      });
    });
  } catch (error: unknown) {
    for (const [event, listener] of listeners.splice(0)) {
      try { room.off(event, listener as RoomEventCallbacks[typeof event]); } catch {}
    }
    disposed = true;
    throw error;
  }

  return {
    snapshot() {
      return dependencies.snapshots.normalize(state);
    },
    discardInitialEvents() {
      pendingEvents.length = 0;
    },
    activate(next: RoomActivation<TSnapshot>) {
      if (disposed) return;
      activation = next;
      scheduleUpdate();
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      if (updateTimer != null) clearTimeout(updateTimer);
      updateTimer = null;
      activation = null;
      pendingEvents.length = 0;
      state.remoteDataTracksBySid.clear();
      for (const [event, listener] of listeners.splice(0)) {
        try { room.off(event, listener as RoomEventCallbacks[typeof event]); } catch {}
      }
    },
  };
}

function normalizedChatTimestamp(timestamp: unknown) {
  const value = Number(timestamp);
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new TypeError("LiveKit chat timestamp is not a non-negative safe integer");
  }
  return value;
}

export function normalizeChatMessage(message: ChatMessageLike, senderIdentity: unknown) {
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
  room: Room,
  participant: Participant | undefined,
  publication: TrackPublication | undefined,
) {
  if (participant) {
    const currentParticipant = participant === room.localParticipant
      ? room.localParticipant
      : room.remoteParticipants.get(participant.identity);
    if (currentParticipant !== participant) return false;
  }
  if (publication) {
    if (typeof publication.trackSid !== "string") return false;
    const participants: Participant[] = [room.localParticipant, ...room.remoteParticipants.values()];
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
    track_publication_sid: typeof publication?.trackSid === "string" ? publication.trackSid : null,
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
