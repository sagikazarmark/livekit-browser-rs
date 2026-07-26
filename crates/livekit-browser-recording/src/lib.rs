//! Browser Local Clip Recording for LiveKit Local Media Tracks.

mod clip;
mod options;
mod recording;

pub use clip::{LocalClip, LocalClipStopReason, RevocableObjectUrl};
pub use options::{LocalClipLimits, LocalClipOptions};
pub use recording::{
    LocalClipFinished, LocalClipRecorder, LocalClipRecording, LocalClipRecordingState,
    LocalClipTrack,
};
