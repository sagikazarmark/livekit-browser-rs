use super::*;

/// The observable connection state of a Room Session.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum RoomConnectionState {
    /// The initial join is in progress.
    Connecting,
    /// The Room Session is established and media can flow.
    Connected,
    /// A full reconnection is in progress within the same Room Session.
    Reconnecting,
    /// Only the signaling connection is being re-established; media may continue to flow.
    SignalReconnecting,
    /// An explicit leave is in progress.
    Disconnecting,
    /// The Room Session is not connected, either before joining or after it ended.
    Disconnected,
}

/// LiveKit's normalized estimate of a participant's connection quality.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum ConnectionQuality {
    /// The connection comfortably sustains the participant's media.
    Excellent,
    /// The connection sustains media with some degradation.
    Good,
    /// The connection is degraded enough to affect media quality.
    Poor,
    /// LiveKit considers the participant's connection lost.
    Lost,
    /// LiveKit has not yet reported an estimate for this participant.
    Unknown,
}

/// Statistics shared by normalized audio and video sender samples.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SenderStatistics {
    timestamp: f64,
    #[cfg_attr(test, ts(type = "number | null"))]
    packets_sent: Option<u64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    bytes_sent: Option<u64>,
    jitter: Option<f64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    packets_lost: Option<i64>,
    round_trip_time: Option<f64>,
    stream_id: Option<String>,
}

impl SenderStatistics {
    /// Returns the browser statistics timestamp in milliseconds.
    pub const fn timestamp(&self) -> f64 {
        self.timestamp
    }

    /// Returns the number of RTP packets sent, when reported.
    pub const fn packets_sent(&self) -> Option<u64> {
        self.packets_sent
    }

    /// Returns the number of payload and header bytes sent, when reported.
    pub const fn bytes_sent(&self) -> Option<u64> {
        self.bytes_sent
    }

    /// Returns remote-observed jitter in seconds, when reported.
    pub const fn jitter(&self) -> Option<f64> {
        self.jitter
    }

    /// Returns remote-reported packet loss, when reported.
    pub const fn packets_lost(&self) -> Option<i64> {
        self.packets_lost
    }

    /// Returns remote-reported round-trip time in seconds, when reported.
    pub const fn round_trip_time(&self) -> Option<f64> {
        self.round_trip_time
    }

    /// Returns the outbound RTP stream identifier, when reported.
    pub fn stream_id(&self) -> Option<&str> {
        self.stream_id.as_deref()
    }
}

/// Normalized sender statistics for one published Local Audio Track.
pub type AudioSenderStatistics = SenderStatistics;

/// Normalized sender statistics for one encoding of a published Local Video Track.
///
/// Browser-dependent values remain optional even where the SDK's TypeScript declarations mark
/// them required, because Firefox and WebKit omit some of them at runtime.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VideoSenderStatistics {
    #[serde(flatten)]
    sender: SenderStatistics,
    #[cfg_attr(test, ts(type = "number | null"))]
    fir_count: Option<u64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pli_count: Option<u64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    nack_count: Option<u64>,
    rid: Option<String>,
    frame_width: Option<u32>,
    frame_height: Option<u32>,
    frames_per_second: Option<f64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    frames_sent: Option<u64>,
    quality_limitation_reason: Option<String>,
    quality_limitation_durations: Option<BTreeMap<String, f64>>,
    #[cfg_attr(test, ts(type = "number | null"))]
    quality_limitation_resolution_changes: Option<u64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    retransmitted_packets_sent: Option<u64>,
    target_bitrate: Option<f64>,
}

impl VideoSenderStatistics {
    /// Returns fields shared by audio and video sender samples.
    pub const fn sender(&self) -> &SenderStatistics {
        &self.sender
    }

    /// Returns the count of Full Intra Request messages received from the receiver, when reported.
    pub const fn fir_count(&self) -> Option<u64> {
        self.fir_count
    }

    /// Returns the count of Picture Loss Indication messages received from the receiver, when
    /// reported.
    ///
    /// A rising count indicates the receiver is losing video and asking for a new keyframe.
    pub const fn pli_count(&self) -> Option<u64> {
        self.pli_count
    }

    /// Returns the count of negative acknowledgements received from the receiver, when reported.
    ///
    /// A rising count indicates packet loss on the path to that receiver.
    pub const fn nack_count(&self) -> Option<u64> {
        self.nack_count
    }

    /// Returns the simulcast RID, when reported.
    pub fn rid(&self) -> Option<&str> {
        self.rid.as_deref()
    }

    /// Returns the width in pixels of the last frame sent for this encoding, when reported.
    pub const fn frame_width(&self) -> Option<u32> {
        self.frame_width
    }

    /// Returns the height in pixels of the last frame sent for this encoding, when reported.
    pub const fn frame_height(&self) -> Option<u32> {
        self.frame_height
    }

    /// Returns frames sent per second over the last second, when reported.
    pub const fn frames_per_second(&self) -> Option<f64> {
        self.frames_per_second
    }

    /// Returns the total number of frames sent on this encoding, when reported.
    pub const fn frames_sent(&self) -> Option<u64> {
        self.frames_sent
    }

    /// Returns why the browser is currently limiting encoding quality, when reported.
    ///
    /// Values come from the browser's `qualityLimitationReason`, typically `none`, `cpu`,
    /// `bandwidth`, or `other`.
    pub fn quality_limitation_reason(&self) -> Option<&str> {
        self.quality_limitation_reason.as_deref()
    }

    /// Returns cumulative seconds spent in each quality-limitation reason, when reported.
    ///
    /// Keys match the values returned by [`quality_limitation_reason`].
    ///
    /// [`quality_limitation_reason`]: Self::quality_limitation_reason
    pub fn quality_limitation_durations(&self) -> Option<&BTreeMap<String, f64>> {
        self.quality_limitation_durations.as_ref()
    }

    /// Returns how many times the browser changed resolution because of quality limitation, when
    /// reported.
    pub const fn quality_limitation_resolution_changes(&self) -> Option<u64> {
        self.quality_limitation_resolution_changes
    }

    /// Returns the number of packets retransmitted for this encoding, when reported.
    pub const fn retransmitted_packets_sent(&self) -> Option<u64> {
        self.retransmitted_packets_sent
    }

    /// Returns the encoder's current target bitrate in bits per second, when reported.
    pub const fn target_bitrate(&self) -> Option<f64> {
        self.target_bitrate
    }
}

/// Statistics shared by normalized audio and video receiver samples.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ReceiverStatistics {
    timestamp: f64,
    jitter_buffer_delay: Option<f64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    packets_lost: Option<i64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    packets_received: Option<u64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    bytes_received: Option<u64>,
    stream_id: Option<String>,
    jitter: Option<f64>,
}

impl ReceiverStatistics {
    /// Returns the browser statistics timestamp in milliseconds.
    pub const fn timestamp(&self) -> f64 {
        self.timestamp
    }

    /// Returns cumulative seconds packets spent in the jitter buffer, when reported.
    ///
    /// Divide by the browser's corresponding emitted-count to obtain an average delay.
    pub const fn jitter_buffer_delay(&self) -> Option<f64> {
        self.jitter_buffer_delay
    }

    /// Returns packets lost on the inbound stream, when reported.
    ///
    /// This value can be negative when duplicate packets arrive.
    pub const fn packets_lost(&self) -> Option<i64> {
        self.packets_lost
    }

    /// Returns the number of RTP packets received, when reported.
    pub const fn packets_received(&self) -> Option<u64> {
        self.packets_received
    }

    /// Returns the number of payload and header bytes received, when reported.
    pub const fn bytes_received(&self) -> Option<u64> {
        self.bytes_received
    }

    /// Returns the inbound RTP stream identifier, when reported.
    pub fn stream_id(&self) -> Option<&str> {
        self.stream_id.as_deref()
    }

    /// Returns packet arrival jitter in seconds, when reported.
    pub const fn jitter(&self) -> Option<f64> {
        self.jitter
    }
}

/// Normalized receiver statistics for one subscribed Remote Audio Track.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AudioReceiverStatistics {
    #[serde(flatten)]
    receiver: ReceiverStatistics,
    #[cfg_attr(test, ts(type = "number | null"))]
    concealed_samples: Option<u64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    concealment_events: Option<u64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    silent_concealed_samples: Option<u64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    silent_concealment_events: Option<u64>,
    total_audio_energy: Option<f64>,
    total_samples_duration: Option<f64>,
}

impl AudioReceiverStatistics {
    /// Returns fields shared by audio and video receiver samples.
    pub const fn receiver(&self) -> &ReceiverStatistics {
        &self.receiver
    }

    /// Returns samples synthesized to conceal lost or late audio, when reported.
    ///
    /// A rising count relative to the received sample count indicates audible degradation.
    pub const fn concealed_samples(&self) -> Option<u64> {
        self.concealed_samples
    }

    /// Returns the number of contiguous concealment runs, when reported.
    pub const fn concealment_events(&self) -> Option<u64> {
        self.concealment_events
    }

    /// Returns concealed samples that were synthesized as silence, when reported.
    pub const fn silent_concealed_samples(&self) -> Option<u64> {
        self.silent_concealed_samples
    }

    /// Returns the number of contiguous silent-concealment runs, when reported.
    pub const fn silent_concealment_events(&self) -> Option<u64> {
        self.silent_concealment_events
    }

    /// Returns cumulative audio energy of received samples, when reported.
    ///
    /// Combine with [`total_samples_duration`] to derive an average audio level.
    ///
    /// [`total_samples_duration`]: Self::total_samples_duration
    pub const fn total_audio_energy(&self) -> Option<f64> {
        self.total_audio_energy
    }

    /// Returns cumulative seconds of received audio, when reported.
    pub const fn total_samples_duration(&self) -> Option<f64> {
        self.total_samples_duration
    }
}

/// Normalized receiver statistics for one subscribed Remote Video Track.
///
/// Browser-dependent values remain optional even where the SDK's TypeScript declarations mark
/// them required, because Firefox and WebKit omit some of them at runtime.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VideoReceiverStatistics {
    #[serde(flatten)]
    receiver: ReceiverStatistics,
    #[cfg_attr(test, ts(type = "number | null"))]
    frames_decoded: Option<u64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    frames_dropped: Option<u64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    frames_received: Option<u64>,
    frame_width: Option<u32>,
    frame_height: Option<u32>,
    #[cfg_attr(test, ts(type = "number | null"))]
    fir_count: Option<u64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pli_count: Option<u64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    nack_count: Option<u64>,
    decoder_implementation: Option<String>,
    mime_type: Option<String>,
}

impl VideoReceiverStatistics {
    /// Returns fields shared by audio and video receiver samples.
    pub const fn receiver(&self) -> &ReceiverStatistics {
        &self.receiver
    }

    /// Returns the total number of frames decoded, when reported.
    pub const fn frames_decoded(&self) -> Option<u64> {
        self.frames_decoded
    }

    /// Returns frames dropped before display, when reported.
    pub const fn frames_dropped(&self) -> Option<u64> {
        self.frames_dropped
    }

    /// Returns the total number of complete frames received, when reported.
    pub const fn frames_received(&self) -> Option<u64> {
        self.frames_received
    }

    /// Returns the width in pixels of the last decoded frame, when reported.
    pub const fn frame_width(&self) -> Option<u32> {
        self.frame_width
    }

    /// Returns the height in pixels of the last decoded frame, when reported.
    pub const fn frame_height(&self) -> Option<u32> {
        self.frame_height
    }

    /// Returns the count of Full Intra Request messages this receiver sent, when reported.
    pub const fn fir_count(&self) -> Option<u64> {
        self.fir_count
    }

    /// Returns the count of Picture Loss Indication messages this receiver sent, when reported.
    pub const fn pli_count(&self) -> Option<u64> {
        self.pli_count
    }

    /// Returns the count of negative acknowledgements this receiver sent, when reported.
    pub const fn nack_count(&self) -> Option<u64> {
        self.nack_count
    }

    /// Returns the browser's decoder name, when reported.
    ///
    /// Useful for distinguishing hardware from software decoding; the exact strings are
    /// browser-specific and not stable across versions.
    pub fn decoder_implementation(&self) -> Option<&str> {
        self.decoder_implementation.as_deref()
    }

    /// Returns the negotiated codec MIME type, such as `video/VP8`, when reported.
    pub fn mime_type(&self) -> Option<&str> {
        self.mime_type.as_deref()
    }
}

/// Whether browser autoplay policy currently permits remote audio playback.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum AudioPlaybackState {
    /// Remote audio may play without another user activation.
    Allowed,
    /// The browser blocked remote audio until a user-activation retry.
    Blocked,
}

/// An immutable, coherent view of one Room Session.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct RoomSnapshot {
    pub(crate) revision: u64,
    pub(crate) connection: RoomConnectionState,
    pub(crate) audio_playback: AudioPlaybackState,
    pub(crate) active_speakers: Vec<String>,
    pub(crate) room_sid: String,
    pub(crate) room_name: String,
    pub(crate) room_metadata: Option<String>,
    pub(crate) local_participant: LocalParticipantSnapshot,
    pub(crate) remote_participants: BTreeMap<String, RemoteParticipantSnapshot>,
}

impl RoomSnapshot {
    /// Returns the monotonically increasing snapshot revision.
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the Room Session's connection state at this revision.
    pub const fn connection(&self) -> RoomConnectionState {
        self.connection
    }

    /// Returns the browser's current remote-audio playback state.
    pub const fn audio_playback(&self) -> AudioPlaybackState {
        self.audio_playback
    }

    /// Returns active Participant Identities in LiveKit's loudest-first order.
    pub fn active_speakers(&self) -> &[String] {
        &self.active_speakers
    }

    /// Returns the authoritative server SID for the room.
    pub fn room_sid(&self) -> &str {
        &self.room_sid
    }

    /// Returns the room name supplied by LiveKit.
    pub fn room_name(&self) -> &str {
        &self.room_name
    }

    /// Returns the room metadata supplied by LiveKit.
    pub fn room_metadata(&self) -> Option<&str> {
        self.room_metadata.as_deref()
    }

    /// Returns the authoritative Local Participant state.
    pub const fn local_participant(&self) -> &LocalParticipantSnapshot {
        &self.local_participant
    }

    /// Returns Remote Participants keyed by stable Participant Identity.
    pub const fn remote_participants(&self) -> &BTreeMap<String, RemoteParticipantSnapshot> {
        &self.remote_participants
    }
}

/// The immutable Local Participant state in a Room Snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[non_exhaustive]
pub struct LocalParticipantSnapshot {
    pub(crate) identity: String,
    pub(crate) sid: String,
    pub(crate) connection_quality: ConnectionQuality,
    pub(crate) metadata: Option<String>,
    pub(crate) attributes: BTreeMap<String, String>,
    pub(crate) grants: ParticipantGrants,
    pub(crate) track_publications: BTreeMap<String, LocalTrackPublicationSnapshot>,
}

impl LocalParticipantSnapshot {
    /// Returns the stable Participant Identity.
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Returns the current LiveKit participant SID.
    pub fn sid(&self) -> &str {
        &self.sid
    }

    /// Returns LiveKit's current connection-quality estimate.
    pub const fn connection_quality(&self) -> ConnectionQuality {
        self.connection_quality
    }

    /// Returns participant metadata supplied by LiveKit.
    pub fn metadata(&self) -> Option<&str> {
        self.metadata.as_deref()
    }

    /// Returns participant attributes supplied by LiveKit.
    pub const fn attributes(&self) -> &BTreeMap<String, String> {
        &self.attributes
    }

    /// Returns authoritative source grants supplied by LiveKit.
    pub const fn grants(&self) -> &ParticipantGrants {
        &self.grants
    }

    /// Returns Local Participant Track Publications keyed by publication SID.
    pub const fn track_publications(&self) -> &BTreeMap<String, LocalTrackPublicationSnapshot> {
        &self.track_publications
    }
}

/// The immutable state of a Remote Participant in a Room Snapshot.
#[derive(Clone)]
#[non_exhaustive]
pub struct RemoteParticipantSnapshot {
    pub(crate) identity: String,
    pub(crate) sid: String,
    pub(crate) generation: u32,
    pub(crate) connection_quality: ConnectionQuality,
    pub(crate) metadata: Option<String>,
    pub(crate) attributes: BTreeMap<String, String>,
    pub(crate) track_publications: BTreeMap<String, RemoteTrackPublicationSnapshot>,
    pub(crate) data_tracks: BTreeMap<String, RemoteDataTrackSnapshot>,
}

impl RemoteParticipantSnapshot {
    /// Returns the stable Participant Identity used as the Room Snapshot key.
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Returns the current LiveKit participant SID.
    pub fn sid(&self) -> &str {
        &self.sid
    }

    pub(crate) fn generation(&self) -> u32 {
        self.generation
    }

    /// Returns LiveKit's current connection-quality estimate.
    pub const fn connection_quality(&self) -> ConnectionQuality {
        self.connection_quality
    }

    /// Returns participant metadata supplied by LiveKit.
    pub fn metadata(&self) -> Option<&str> {
        self.metadata.as_deref()
    }

    /// Returns participant attributes supplied by LiveKit.
    pub const fn attributes(&self) -> &BTreeMap<String, String> {
        &self.attributes
    }

    /// Returns Track Publications keyed by publication SID.
    pub const fn track_publications(&self) -> &BTreeMap<String, RemoteTrackPublicationSnapshot> {
        &self.track_publications
    }

    /// Returns Remote Data Tracks keyed by their LiveKit data-track SID.
    pub const fn data_tracks(&self) -> &BTreeMap<String, RemoteDataTrackSnapshot> {
        &self.data_tracks
    }
}

impl fmt::Debug for RemoteParticipantSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteParticipantSnapshot")
            .field("identity", &self.identity)
            .field("sid", &self.sid)
            .field("connection_quality", &self.connection_quality)
            .field("metadata", &self.metadata)
            .field("attributes", &self.attributes)
            .field("track_publications", &self.track_publications)
            .field("data_tracks", &self.data_tracks)
            .finish()
    }
}

impl PartialEq for RemoteParticipantSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.sid == other.sid
            && self.connection_quality == other.connection_quality
            && self.metadata == other.metadata
            && self.attributes == other.attributes
            && self.track_publications == other.track_publications
            && self.data_tracks == other.data_tracks
    }
}

impl Eq for RemoteParticipantSnapshot {}

/// LiveKit source grants for the Local Participant.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[non_exhaustive]
pub struct ParticipantGrants {
    pub(crate) can_subscribe: bool,
    pub(crate) can_publish: bool,
    pub(crate) can_publish_data: bool,
    pub(crate) can_publish_sources: Vec<TrackSource>,
    pub(crate) can_update_metadata: bool,
    pub(crate) hidden: bool,
    pub(crate) recorder: bool,
    pub(crate) agent: bool,
    pub(crate) can_subscribe_metrics: bool,
    pub(crate) can_manage_agent_session: bool,
}

impl ParticipantGrants {
    /// Whether the Local Participant may subscribe to remote media.
    pub const fn can_subscribe(&self) -> bool {
        self.can_subscribe
    }

    /// Whether the Local Participant may publish media.
    pub const fn can_publish(&self) -> bool {
        self.can_publish
    }

    /// Whether the Local Participant may publish data.
    pub const fn can_publish_data(&self) -> bool {
        self.can_publish_data
    }

    /// Returns source-specific publication grants.
    ///
    /// LiveKit treats an empty list as all sources when `can_publish` is true.
    pub fn can_publish_sources(&self) -> &[TrackSource] {
        &self.can_publish_sources
    }

    /// Whether the Local Participant may update its metadata and attributes.
    pub const fn can_update_metadata(&self) -> bool {
        self.can_update_metadata
    }

    /// Whether the Local Participant is hidden from other participants.
    pub const fn hidden(&self) -> bool {
        self.hidden
    }

    /// Whether LiveKit identifies this participant as a recorder.
    pub const fn recorder(&self) -> bool {
        self.recorder
    }

    /// Whether LiveKit identifies this participant as an agent.
    pub const fn agent(&self) -> bool {
        self.agent
    }

    /// Whether the Local Participant may subscribe to metrics.
    pub const fn can_subscribe_metrics(&self) -> bool {
        self.can_subscribe_metrics
    }

    /// Whether the Local Participant may manage agent sessions.
    pub const fn can_manage_agent_session(&self) -> bool {
        self.can_manage_agent_session
    }
}

/// A LiveKit media source authorized or represented in a Track Publication.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum TrackSource {
    /// LiveKit did not report a recognized source for this publication.
    Unknown,
    /// Camera video.
    Camera,
    /// Microphone audio.
    Microphone,
    /// Display-capture video.
    ScreenShare,
    /// System or tab audio captured alongside display video.
    ScreenShareAudio,
}

/// The media kind announced by a Track Publication.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum TrackKind {
    /// The publication carries audio.
    Audio,
    /// The publication carries video.
    Video,
    /// LiveKit did not report a recognized media kind.
    Unknown,
}

/// Immutable state for a separately published Remote Data Track.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[non_exhaustive]
pub struct RemoteDataTrackSnapshot {
    sid: String,
    name: String,
    uses_e2ee: bool,
}

impl RemoteDataTrackSnapshot {
    /// Returns the LiveKit data-track SID used as its participant snapshot key.
    pub fn sid(&self) -> &str {
        &self.sid
    }

    /// Returns the publisher-supplied data-track name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether LiveKit reports end-to-end encryption for this data track.
    pub const fn uses_e2ee(&self) -> bool {
        self.uses_e2ee
    }
}

/// Publisher-side policy controlling who may subscribe to Local Participant Track Publications.
///
/// Participant-specific policy is keyed only by stable Participant Identity. Participants omitted
/// from a participant-specific policy cannot subscribe to this publisher's media.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PublicationSubscriptionPermissions {
    all_participants_allowed: bool,
    participant_identities: Vec<String>,
}

impl PublicationSubscriptionPermissions {
    /// Allows every room participant to subscribe to all current and future Track Publications.
    pub fn all_participants() -> Self {
        Self {
            all_participants_allowed: true,
            participant_identities: Vec::new(),
        }
    }

    /// Restricts access to the complete identity-keyed permission list.
    ///
    /// An empty list denies every participant. Participants omitted from the list are denied.
    pub fn participant_specific<I, S>(participant_identities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            all_participants_allowed: false,
            participant_identities: participant_identities.into_iter().map(Into::into).collect(),
        }
    }

    /// Whether every participant may subscribe to every Track Publication.
    pub const fn allows_all_participants(&self) -> bool {
        self.all_participants_allowed
    }

    /// Returns the complete Participant Identity-keyed permission list.
    pub fn participant_identities(&self) -> &[String] {
        &self.participant_identities
    }

    pub(super) fn is_valid(&self) -> bool {
        self.all_participants_allowed
            || self
                .participant_identities
                .iter()
                .all(|identity| !identity.is_empty())
    }
}

impl Default for PublicationSubscriptionPermissions {
    fn default() -> Self {
        Self::all_participants()
    }
}

/// Immutable state for a Local Participant Track Publication.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[non_exhaustive]
pub struct LocalTrackPublicationSnapshot {
    sid: String,
    name: String,
    source: TrackSource,
    kind: TrackKind,
    muted: bool,
}

impl LocalTrackPublicationSnapshot {
    /// Returns the publication SID.
    pub fn sid(&self) -> &str {
        &self.sid
    }

    /// Returns the publication name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the published source.
    pub const fn source(&self) -> TrackSource {
        self.source
    }

    /// Returns the published media kind.
    pub const fn kind(&self) -> TrackKind {
        self.kind
    }

    /// Whether this publication is muted.
    pub const fn muted(&self) -> bool {
        self.muted
    }
}

/// Immutable state for a Remote Participant Track Publication.
///
/// A publication is room-visible state independent of whether its media is currently available in
/// this browser. No live Media Track handle is stored in a Room Snapshot.
#[derive(Clone)]
#[non_exhaustive]
pub struct RemoteTrackPublicationSnapshot {
    pub(crate) sid: String,
    pub(crate) generation: u32,
    pub(crate) name: String,
    pub(crate) source: TrackSource,
    pub(crate) kind: TrackKind,
    pub(crate) muted: bool,
    pub(crate) subscribed: bool,
    pub(crate) media_available: bool,
    pub(crate) media_generation: Option<u32>,
}

impl RemoteTrackPublicationSnapshot {
    /// Returns the publication SID used as its participant snapshot key.
    pub fn sid(&self) -> &str {
        &self.sid
    }

    pub(crate) fn generation(&self) -> u32 {
        self.generation
    }

    /// Returns the publication name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the published source.
    pub const fn source(&self) -> TrackSource {
        self.source
    }

    /// Returns the published media kind.
    pub const fn kind(&self) -> TrackKind {
        self.kind
    }

    /// Whether this publication is muted.
    pub const fn muted(&self) -> bool {
        self.muted
    }

    /// Whether the Local Participant currently subscribes to this publication.
    pub const fn subscribed(&self) -> bool {
        self.subscribed
    }

    /// Whether subscribed media for this publication is currently available in this browser.
    pub const fn media_available(&self) -> bool {
        self.media_available
    }

    pub(crate) fn media_generation(&self) -> Option<u32> {
        self.media_generation
    }
}

impl fmt::Debug for RemoteTrackPublicationSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteTrackPublicationSnapshot")
            .field("sid", &self.sid)
            .field("name", &self.name)
            .field("source", &self.source)
            .field("kind", &self.kind)
            .field("muted", &self.muted)
            .field("subscribed", &self.subscribed)
            .field("media_available", &self.media_available)
            .finish()
    }
}

impl PartialEq for RemoteTrackPublicationSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.sid == other.sid
            && self.name == other.name
            && self.source == other.source
            && self.kind == other.kind
            && self.muted == other.muted
            && self.subscribed == other.subscribed
            && self.media_available == other.media_available
    }
}

impl Eq for RemoteTrackPublicationSnapshot {}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct AdapterRoomSnapshot {
    #[cfg_attr(test, ts(type = "number"))]
    revision: u64,
    connection: RoomConnectionState,
    audio_playback: AudioPlaybackState,
    active_speakers: Vec<String>,
    room_sid: String,
    room_name: String,
    room_metadata: Option<String>,
    local_participant: LocalParticipantSnapshot,
    remote_participants: BTreeMap<String, AdapterRemoteParticipantSnapshot>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct AdapterRemoteParticipantSnapshot {
    identity: String,
    sid: String,
    generation: u32,
    connection_quality: ConnectionQuality,
    metadata: Option<String>,
    attributes: BTreeMap<String, String>,
    track_publications: BTreeMap<String, AdapterRemoteTrackPublicationSnapshot>,
    data_tracks: BTreeMap<String, RemoteDataTrackSnapshot>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct AdapterRemoteTrackPublicationSnapshot {
    sid: String,
    generation: u32,
    name: String,
    source: TrackSource,
    kind: TrackKind,
    muted: bool,
    subscribed: bool,
    media_available: bool,
    media_generation: Option<u32>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl From<AdapterRoomSnapshot> for RoomSnapshot {
    fn from(snapshot: AdapterRoomSnapshot) -> Self {
        Self {
            revision: snapshot.revision,
            connection: snapshot.connection,
            audio_playback: snapshot.audio_playback,
            active_speakers: snapshot.active_speakers,
            room_sid: snapshot.room_sid,
            room_name: snapshot.room_name,
            room_metadata: snapshot.room_metadata,
            local_participant: snapshot.local_participant,
            remote_participants: snapshot
                .remote_participants
                .into_iter()
                .map(|(identity, participant)| (identity, participant.into()))
                .collect(),
        }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
impl From<AdapterRemoteParticipantSnapshot> for RemoteParticipantSnapshot {
    fn from(participant: AdapterRemoteParticipantSnapshot) -> Self {
        Self {
            identity: participant.identity,
            sid: participant.sid,
            generation: participant.generation,
            connection_quality: participant.connection_quality,
            metadata: participant.metadata,
            attributes: participant.attributes,
            track_publications: participant
                .track_publications
                .into_iter()
                .map(|(sid, publication)| (sid, publication.into()))
                .collect(),
            data_tracks: participant.data_tracks,
        }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
impl From<AdapterRemoteTrackPublicationSnapshot> for RemoteTrackPublicationSnapshot {
    fn from(publication: AdapterRemoteTrackPublicationSnapshot) -> Self {
        Self {
            sid: publication.sid,
            generation: publication.generation,
            name: publication.name,
            source: publication.source,
            kind: publication.kind,
            muted: publication.muted,
            subscribed: publication.subscribed,
            media_available: publication.media_available,
            media_generation: publication.media_generation,
        }
    }
}

#[derive(Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteParticipantLocator {
    pub(crate) identity: String,
    pub(crate) sid: String,
    pub(crate) generation: u32,
}

#[derive(Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemotePublicationLocator {
    pub(crate) participant: RemoteParticipantLocator,
    pub(crate) sid: String,
    pub(crate) generation: u32,
    #[serde(skip)]
    pub(crate) kind: TrackKind,
}

impl RoomSnapshot {
    pub(super) fn participant_for(
        &self,
        locator: &RemoteParticipantLocator,
    ) -> Option<&RemoteParticipantSnapshot> {
        self.remote_participants
            .get(&locator.identity)
            .filter(|participant| {
                participant.sid == locator.sid && participant.generation == locator.generation
            })
    }

    pub(super) fn publication_for(
        &self,
        locator: &RemotePublicationLocator,
    ) -> Option<&RemoteTrackPublicationSnapshot> {
        self.participant_for(&locator.participant)?
            .track_publications
            .get(&locator.sid)
            .filter(|publication| {
                publication.kind == locator.kind && publication.generation == locator.generation
            })
    }
}
