//! Headless Dioxus integration for LiveKit browser Room Sessions.
//!
//! This crate is unstyled and renders no UI of its own. Applications fetch credentials and join
//! through [`livekit_browser`], then hand the joined Room Session to [`RoomSessionProvider`]. The
//! provider owns only its subtree's Room Session observation and cleanup; it never joins, leaves,
//! captures, or publishes on its own.
//!
//! # Reading state
//!
//! The provider subscribes to bounded Room Events and republishes the coherent [`RoomSnapshot`]
//! installed before each event, so consumers always read a self-consistent revision rather than
//! reassembling state from individual events.
//!
//! - [`use_room_snapshot`] for the whole snapshot
//! - [`use_participants`], [`use_track_publications`], [`use_audio_playback`] for memoized slices
//! - [`use_browser_capabilities`] for permission-free capability gating
//! - [`use_room_session`] to reach the underlying handle for anything not covered above
//!
//! These hooks panic when called outside a [`RoomSessionProvider`], matching Dioxus context hooks.
//!
//! # Changing state
//!
//! [`use_room_actions`] returns [`RoomSessionActions`], exposing Dioxus pending/result/error state
//! for common mutations. Each action starts its core operation synchronously before the returned
//! waiter is polled, which preserves browser user-activation requirements — important for
//! [`RoomSessionActions::start_audio_playback`], which must run inside a real user gesture.
//!
//! # Attaching media
//!
//! [`LocalVideo`], [`LocalAudio`], [`RemoteVideo`], and [`RemoteAudio`] render bare `video` and
//! `audio` elements and attach an existing Media Track to them. Attachment is display only:
//! mounting, unmounting, or swapping a track never captures, publishes, mutes, or changes
//! subscription policy. Replacing the `track` prop re-attaches; attachment failures go to
//! `on_error` rather than panicking.
//!
//! [`RoomSnapshot`]: livekit_browser::RoomSnapshot

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
