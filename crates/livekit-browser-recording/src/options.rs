use std::time::Duration;

use livekit_browser::Error;

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// The automatic bound applied to one Local Clip Recording.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LocalClipLimits {
    /// Stop after this much active recording time.
    Duration(Duration),
    /// Stop when delivered encoded chunks reach this accumulated size.
    Size(u64),
    /// Stop when either bound is reached.
    DurationOrSize {
        /// Maximum active recording time.
        max_duration: Duration,
        /// Accumulated encoded-byte threshold.
        max_bytes: u64,
    },
}

/// Format preferences and resource bounds for one Local Clip Recording.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalClipOptions {
    mime_preferences: Vec<String>,
    max_duration: Option<Duration>,
    max_bytes: Option<u64>,
}

impl LocalClipOptions {
    /// Validates options while preserving MIME preferences in caller order.
    pub fn new(mime_preferences: Vec<String>, limits: LocalClipLimits) -> Result<Self, Error> {
        if mime_preferences.is_empty() {
            return Err(Error::InvalidLocalClipRecordingOptions {
                reason: "at least one MIME preference is required",
            });
        }
        if mime_preferences
            .iter()
            .any(|preference| preference.trim().is_empty())
        {
            return Err(Error::InvalidLocalClipRecordingOptions {
                reason: "MIME preferences must not be empty",
            });
        }

        let (max_duration, max_bytes) = match limits {
            LocalClipLimits::Duration(duration) => (Some(duration), None),
            LocalClipLimits::Size(bytes) => (None, Some(bytes)),
            LocalClipLimits::DurationOrSize {
                max_duration,
                max_bytes,
            } => (Some(max_duration), Some(max_bytes)),
        };
        if max_duration.is_some_and(|duration| {
            duration.as_millis() == 0 || duration.as_millis() > u128::from(MAX_SAFE_INTEGER)
        }) {
            return Err(Error::InvalidLocalClipRecordingOptions {
                reason: "duration limit must be at least one millisecond and JavaScript-safe",
            });
        }
        if max_bytes.is_some_and(|bytes| bytes == 0 || bytes > MAX_SAFE_INTEGER) {
            return Err(Error::InvalidLocalClipRecordingOptions {
                reason: "size limit must be non-zero and JavaScript-safe",
            });
        }

        Ok(Self {
            mime_preferences,
            max_duration,
            max_bytes,
        })
    }

    /// Ordered MIME preferences used for browser capability negotiation.
    pub fn mime_preferences(&self) -> &[String] {
        &self.mime_preferences
    }

    /// Maximum active recording duration, when configured.
    pub const fn max_duration(&self) -> Option<Duration> {
        self.max_duration
    }

    /// Accumulated encoded-byte threshold, when configured.
    ///
    /// MediaRecorder emits chunks asynchronously, so the final artifact can exceed this threshold
    /// by one delivered chunk.
    pub const fn max_bytes(&self) -> Option<u64> {
        self.max_bytes
    }
}
