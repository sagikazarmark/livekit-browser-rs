//! Browser Local Clip Recording for LiveKit Local Media Tracks.
//!
//! A Local Clip Recording encodes selected Local Media Tracks into a bounded, browser-owned
//! artifact using `MediaRecorder`. It is independent of LiveKit: recording requires no Room
//! Session and no Track Publication, and it neither starts nor stops capture.
//!
//! This is deliberately not server-side Room Recording. Only [`LocalAudioTrack`] and
//! [`LocalVideoTrack`] convert into [`LocalClipTrack`]; Remote Media Tracks are rejected at
//! compile time, because capturing other participants belongs to LiveKit Egress.
//!
//! # Lifecycle
//!
//! [`LocalClipRecorder::new`] negotiates browser capabilities without requesting permissions or
//! constructing a `MediaRecorder`. [`LocalClipRecorder::start`] takes ownership of the selected
//! tracks for the recording's lifetime and returns a [`LocalClipRecording`] that can be paused,
//! resumed, and stopped. Recording also stops on its own when a configured [`LocalClipLimits`]
//! duration or size threshold is reached, or when every selected track ends; [`LocalClipStopReason`]
//! distinguishes these.
//!
//! # Artifact ownership
//!
//! A finished [`LocalClip`] wraps a browser `Blob`. Encoded bytes stay in browser memory and are
//! never copied into WASM linear memory. Use [`LocalClip::create_object_url`] to obtain a
//! [`RevocableObjectUrl`] for download; it revokes on explicit call or final drop, so the browser
//! reclaims the artifact.
//!
//! # Browser support
//!
//! `MediaRecorder` and its supported MIME types vary by browser. [`LocalClipOptions`] takes MIME
//! types in caller preference order, and support is checked before any recorder is constructed, so
//! an unsupported format fails before mutating state.
//!
//! [`LocalAudioTrack`]: livekit_browser::LocalAudioTrack
//! [`LocalVideoTrack`]: livekit_browser::LocalVideoTrack

mod clip;
mod options;
mod recording;

pub use clip::{LocalClip, LocalClipStopReason, RevocableObjectUrl};
pub use options::{LocalClipLimits, LocalClipOptions};
pub use recording::{
    LocalClipFinished, LocalClipRecorder, LocalClipRecording, LocalClipRecordingState,
    LocalClipTrack,
};
