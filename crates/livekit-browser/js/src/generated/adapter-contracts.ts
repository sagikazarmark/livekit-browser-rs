// Generated from Rust DTOs by npm run contracts:generate. Do not edit.

export type BrowserCapabilities = { web_rtc: boolean, secure_context: boolean, user_media: boolean, display_capture: boolean, audio_playback: boolean, audio_output_selection: boolean, transcription: boolean, local_clip_recording: boolean, };

export type RoomConnectionState = "connecting" | "connected" | "reconnecting" | "signalReconnecting" | "disconnecting" | "disconnected";

export type ConnectionQuality = "excellent" | "good" | "poor" | "lost" | "unknown";

export type AudioPlaybackState = "allowed" | "blocked";

export type TrackSource = "unknown" | "camera" | "microphone" | "screenShare" | "screenShareAudio";

export type TrackKind = "audio" | "video" | "unknown";

export type ParticipantGrants = { can_subscribe: boolean, can_publish: boolean, can_publish_data: boolean, can_publish_sources: Array<TrackSource>, can_update_metadata: boolean, hidden: boolean, recorder: boolean, agent: boolean, can_subscribe_metrics: boolean, can_manage_agent_session: boolean, };

export type LocalTrackPublicationSnapshot = { sid: string, name: string, source: TrackSource, kind: TrackKind, muted: boolean, };

export type LocalParticipantSnapshot = { identity: string, sid: string, connection_quality: ConnectionQuality, metadata: string | null, attributes: { [key in string]: string }, grants: ParticipantGrants, track_publications: { [key in string]: LocalTrackPublicationSnapshot }, };

export type RemoteDataTrackSnapshot = { sid: string, name: string, uses_e2ee: boolean, };

export type AdapterRemoteTrackPublicationSnapshot = { sid: string, generation: number, name: string, source: TrackSource, kind: TrackKind, muted: boolean, subscribed: boolean, media_available: boolean, media_generation: number | null, };

export type AdapterRemoteParticipantSnapshot = { identity: string, sid: string, generation: number, connection_quality: ConnectionQuality, metadata: string | null, attributes: { [key in string]: string }, track_publications: { [key in string]: AdapterRemoteTrackPublicationSnapshot }, data_tracks: { [key in string]: RemoteDataTrackSnapshot }, };

export type AdapterRoomSnapshot = { revision: number, connection: RoomConnectionState, audio_playback: AudioPlaybackState, active_speakers: Array<string>, room_sid: string, room_name: string, room_metadata: string | null, local_participant: LocalParticipantSnapshot, remote_participants: { [key in string]: AdapterRemoteParticipantSnapshot }, };

export type RoomEventKind = { "type": "connectionStateChanged", state: RoomConnectionState, } | { "type": "localParticipantChanged" } | { "type": "localTrackPublicationsChanged" } | { "type": "remoteParticipantJoined", identity: string, } | { "type": "remoteParticipantChanged", identity: string, } | { "type": "participantConnectionQualityChanged", participant_identity: string, quality: ConnectionQuality, } | { "type": "activeSpeakersChanged" } | { "type": "remoteParticipantLeft", identity: string, } | { "type": "remoteTrackPublicationsChanged", participant_identity: string, } | { "type": "remoteDataTracksChanged", participant_identity: string, } | { "type": "roomMetadataChanged" } | { "type": "audioPlaybackChanged", state: AudioPlaybackState, } | { "type": "lagged" };

export type AdapterJoin = { snapshot: AdapterRoomSnapshot, };

export type AdapterUpdate = { snapshot: AdapterRoomSnapshot, events: Array<RoomEventKind>, };

export type RuntimeInspection = { "status": "ready", capabilities: BrowserCapabilities, } | { "status": "sdkMissing" } | { "status": "sdkVersionMismatch", actual_version: string | null, } | { "status": "unsupportedBrowser" };

export type AdapterScreenCapture = { video_track_id: number, audio_track_id: number | null, };

export type RemoteParticipantLocator = { identity: string, sid: string, generation: number, };

export type RemotePublicationLocator = { participant: RemoteParticipantLocator, sid: string, generation: number, };

export type AdapterRemoteMediaTrack = { id: number, kind: TrackKind, generation: number, };

export type AdapterRpcOptions = { destinationIdentity: string, method: string, payload: string, responseTimeoutMs: number, };

export type AdapterRpcFailure = { terminal: boolean, permission_denied: boolean, code: number | null, message: string, data: string | null, };

export type AdapterRpcInvocation = { id: number, request_id: string, caller_identity: string, payload: string, response_timeout_ms: number, };

export type AdapterRpcHandlerCompletion = { success: boolean, payload: string | null, code: number | null, message: string | null, data: string | null, };

export type TextStreamOptions = { destinationIdentity: string, topic: string, totalSize?: number, };

export type ByteStreamOptions = { destinationIdentity: string, topic: string, totalSize?: number, };

export type TextStreamInfo = { id: string, topic: string, size: number | null, sender_identity: string | null, };

export type ByteStreamInfo = { id: string, topic: string, size: number | null, sender_identity: string | null, };

export type AdapterOutgoingTextStream = { id: number, info: TextStreamInfo, };

export type AdapterOutgoingByteStream = { id: number, info: ByteStreamInfo, };

export type AdapterTextStreamRead = { done: boolean, value: string | null, };

export type AdapterIncomingTextStream = { id: number, info: TextStreamInfo, };

export type AdapterIncomingByteStream = { id: number, info: ByteStreamInfo, };

export type AdapterByteStreamRead = { done: boolean, value: Array<number> | null, };

export type MediaDeviceKind = "camera" | "microphone";

export type MediaDeviceInfo = { device_id: string, group_id: string, label: string, kind: MediaDeviceKind, };

export type SenderStatistics = { timestamp: number, packets_sent: number | null, bytes_sent: number | null, jitter: number | null, packets_lost: number | null, round_trip_time: number | null, stream_id: string | null, };

export type VideoSenderStatistics = { fir_count: number | null, pli_count: number | null, nack_count: number | null, rid: string | null, frame_width: number | null, frame_height: number | null, frames_per_second: number | null, frames_sent: number | null, quality_limitation_reason: string | null, quality_limitation_durations: { [key in string]: number } | null, quality_limitation_resolution_changes: number | null, retransmitted_packets_sent: number | null, target_bitrate: number | null, timestamp: number, packets_sent: number | null, bytes_sent: number | null, jitter: number | null, packets_lost: number | null, round_trip_time: number | null, stream_id: string | null, };

export type ReceiverStatistics = { timestamp: number, jitter_buffer_delay: number | null, packets_lost: number | null, packets_received: number | null, bytes_received: number | null, stream_id: string | null, jitter: number | null, };

export type AudioReceiverStatistics = { concealed_samples: number | null, concealment_events: number | null, silent_concealed_samples: number | null, silent_concealment_events: number | null, total_audio_energy: number | null, total_samples_duration: number | null, timestamp: number, jitter_buffer_delay: number | null, packets_lost: number | null, packets_received: number | null, bytes_received: number | null, stream_id: string | null, jitter: number | null, };

export type VideoReceiverStatistics = { frames_decoded: number | null, frames_dropped: number | null, frames_received: number | null, frame_width: number | null, frame_height: number | null, fir_count: number | null, pli_count: number | null, nack_count: number | null, decoder_implementation: string | null, mime_type: string | null, timestamp: number, jitter_buffer_delay: number | null, packets_lost: number | null, packets_received: number | null, bytes_received: number | null, stream_id: string | null, jitter: number | null, };

export type AdapterChatMessage = { id: string, message: string, sender_identity: string | null, timestamp_millis: number, edit_timestamp_millis: number | null, };

export type AdapterTranscriptionSegment = { id: string, participant_identity: string | null, track_publication_sid: string | null, text: string, start_time: number, end_time: number, language: string, is_final: boolean, first_received_at_millis: number, last_received_at_millis: number, };

export type ExactDeviceId = { exact: string, };

export type FacingMode = "user" | "environment" | "left" | "right";

export type CameraResolution = { width: number, height: number, };

export type CameraCaptureOptions = { deviceId?: ExactDeviceId, facingMode?: FacingMode, resolution?: CameraResolution, frameRate?: number, };

export type MicrophoneCaptureOptions = { deviceId?: ExactDeviceId, autoGainControl?: boolean, channelCount?: number, echoCancellation?: boolean, latency?: number, noiseSuppression?: boolean, sampleRate?: number, sampleSize?: number, };

export type ScreenCaptureOptions = { audio: boolean, };

export type VideoCodec = "vp8" | "vp9" | "h264" | "av1";

export type VideoEncoding = { maxBitrate: number, maxFramerate: number, };

export type AdapterVideoPublishOptions = { name?: string, videoCodec?: VideoCodec, simulcast?: boolean, videoEncoding?: VideoEncoding, };

export type AudioCodec = "opus";

export type AdapterAudioPublishOptions = { name?: string, audioCodec: AudioCodec, dtx?: boolean, red?: boolean, };

export type PublicationSubscriptionPermissions = { allParticipantsAllowed: boolean, participantIdentities: Array<string>, };
