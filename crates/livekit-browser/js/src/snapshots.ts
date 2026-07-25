import { trackSources } from "./options";
import type {
  NormalizedRoomEvent,
  PendingRoomEvent,
  RemoteDataTrackLike,
} from "./room-events";
import type {
  LocalParticipant,
  Participant,
  RemoteParticipant,
  RemoteTrack,
  RemoteTrackPublication,
  Room,
  TrackPublication,
} from "livekit-client";

type RoomId = number;
export type ConnectionQuality = "excellent" | "good" | "poor" | "lost" | "unknown";
export type PublicationKind = "audio" | "video" | "data" | "unknown";
export type PublicationSource =
  | "camera"
  | "microphone"
  | "screenShare"
  | "screenShareAudio"
  | "unknown";

export interface PendingConnectionQualityChangedEvent extends PendingRoomEvent {
  type: "participantConnectionQualityChanged";
  participant_identity: string;
  source_participant: Participant;
}

export interface NormalizedConnectionQualityChangedEvent extends NormalizedRoomEvent {
  type: "participantConnectionQualityChanged";
  participant_identity: string;
  quality: ConnectionQuality;
}

export interface PublicationSnapshot {
  sid: string;
  name: string;
  source: PublicationSource;
  kind: PublicationKind;
  muted: boolean;
}

export interface RemotePublicationSnapshot extends PublicationSnapshot {
  generation: number;
  subscribed: boolean;
  media_available: boolean;
  media_generation: number | null;
}

export interface ParticipantGrantsSnapshot {
  can_subscribe: boolean;
  can_publish: boolean;
  can_publish_data: boolean;
  can_publish_sources: PublicationSource[];
  can_update_metadata: boolean;
  hidden: boolean;
  recorder: boolean;
  agent: boolean;
  can_subscribe_metrics: boolean;
  can_manage_agent_session: boolean;
}

export interface LocalParticipantSnapshot {
  identity: string;
  sid: string;
  connection_quality: ConnectionQuality;
  metadata: string | null;
  attributes: Record<string, string>;
  grants: ParticipantGrantsSnapshot;
  track_publications: Record<string, PublicationSnapshot>;
}

export interface RemoteDataTrackSnapshot {
  sid: string;
  name: string;
  uses_e2ee: boolean;
}

export interface RemoteParticipantSnapshot {
  identity: string;
  sid: string;
  generation: number;
  connection_quality: ConnectionQuality;
  metadata: string | null;
  attributes: Record<string, string>;
  track_publications: Record<string, RemotePublicationSnapshot>;
  data_tracks: Record<string, RemoteDataTrackSnapshot>;
}

export interface RoomSnapshot {
  revision: number;
  connection: Room["state"];
  audio_playback: "blocked" | "allowed";
  active_speakers: string[];
  room_sid: string;
  room_name: string;
  room_metadata: string | null;
  local_participant: LocalParticipantSnapshot;
  remote_participants: Record<string, RemoteParticipantSnapshot>;
}

export interface SnapshotRoomRecord {
  readonly id: RoomId;
  readonly room: Room;
  readonly roomSid: string;
  readonly revision: number;
  readonly remoteDataTracksBySid: Map<
    string,
    { participant: RemoteParticipant; track: RemoteDataTrackLike }
  >;
}

export interface LocalMediaSnapshotInterface {
  reconcileSnapshot(roomId: RoomId, participant: LocalParticipant): void;
}

export interface RemoteMediaSnapshotInterface {
  reconcileRoom(roomId: RoomId): void;
  participantGeneration(participant: RemoteParticipant): number;
  publicationGeneration(publication: RemoteTrackPublication): number;
  mediaGeneration(track: RemoteTrack): number;
}

export interface SnapshotNormalizerDependencies {
  localMedia: LocalMediaSnapshotInterface;
  remoteMedia: RemoteMediaSnapshotInterface;
}

const publicationSource = (source: unknown): PublicationSource =>
  (trackSources[source as keyof typeof trackSources]?.normalized as PublicationSource | undefined) ??
  "unknown";

const participantConnectionQuality = (quality: unknown): ConnectionQuality => {
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

const publicationKind = (kind: unknown): PublicationKind => {
  switch (kind) {
    case "audio":
    case "video":
    case "data":
      return kind;
    default:
      return "unknown";
  }
};

const grantSource = (source: unknown): PublicationSource => {
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

function normalizeGrants(permissions: LocalParticipant["permissions"]): ParticipantGrantsSnapshot {
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

function publicationState(publication: TrackPublication): PublicationSnapshot | null {
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

export function createSnapshotNormalizer(dependencies: SnapshotNormalizerDependencies) {
  const { localMedia, remoteMedia } = dependencies;

  function normalizeLocalPublications(participant: LocalParticipant) {
    const publications: Record<string, PublicationSnapshot> = {};
    for (const publication of participant.trackPublications.values()) {
      const state = publicationState(publication);
      if (state) publications[state.sid] = state;
    }
    return publications;
  }

  function normalizeRemotePublications(participant: RemoteParticipant) {
    const publications: Record<string, RemotePublicationSnapshot> = {};
    for (const publication of participant.trackPublications.values()) {
      const state = publicationState(publication);
      if (!state) continue;
      publications[state.sid] = {
        ...state,
        generation: remoteMedia.publicationGeneration(publication),
        subscribed: publication.isSubscribed === true,
        media_available: publication.track != null,
        media_generation: publication.track ? remoteMedia.mediaGeneration(publication.track) : null,
      };
    }
    return publications;
  }

  function normalizeRemoteDataTracks(record: SnapshotRoomRecord, participant: RemoteParticipant) {
    const tracks: Record<string, RemoteDataTrackSnapshot> = {};
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

  function normalizeRemoteParticipants(
    record: SnapshotRoomRecord,
  ): Record<string, RemoteParticipantSnapshot> {
    const participants: Record<string, RemoteParticipantSnapshot> = Object.create(null);
    record.remoteDataTracksBySid.clear();
    for (const participant of record.room.remoteParticipants.values()) {
      const identity = participant.identity;
      if (typeof identity !== "string" || identity.length === 0) continue;
      participants[identity] = {
        identity,
        sid: participant.sid,
        generation: remoteMedia.participantGeneration(participant),
        connection_quality: participantConnectionQuality(participant.connectionQuality),
        metadata: participant.metadata ?? null,
        attributes: { ...participant.attributes },
        track_publications: normalizeRemotePublications(participant),
        data_tracks: normalizeRemoteDataTracks(record, participant),
      };
    }
    return participants;
  }

  function normalizeActiveSpeakers(record: SnapshotRoomRecord) {
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

  function normalizeSnapshot(record: SnapshotRoomRecord): RoomSnapshot {
    remoteMedia.reconcileRoom(record.id);
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
    record: SnapshotRoomRecord,
    event: PendingRoomEvent,
    snapshot: RoomSnapshot,
  ): NormalizedRoomEvent | null {
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

  return { normalizeSnapshot, normalizeQueuedEvent };
}
