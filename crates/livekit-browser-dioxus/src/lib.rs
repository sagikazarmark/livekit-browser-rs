//! Headless Dioxus integration for LiveKit browser Room Sessions.
//!
//! Applications fetch credentials and join through `livekit-browser`, then provide the joined
//! Room Session to [`RoomSessionProvider`]. The provider owns only its subtree's Room Session
//! observation and cleanup. Media nodes attach existing Media Tracks to their mounted browser
//! elements; they never capture, publish, mute, or change subscription policy.

mod actions;
mod media;
mod room;

pub use actions::{use_room_actions, RemoteTrackPublicationSubscription, RoomSessionActions};
pub use media::{
    LocalAudio, LocalAudioProps, LocalVideo, LocalVideoProps, RemoteAudio, RemoteAudioProps,
    RemoteVideo, RemoteVideoProps,
};
pub use room::{
    use_audio_playback, use_browser_capabilities, use_participants, use_room_session,
    use_room_snapshot, use_track_publications, ParticipantSnapshots, RoomSessionProvider,
    RoomSessionProviderProps, TrackPublicationSnapshots,
};
