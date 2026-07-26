use std::{cell::Cell, fmt, marker::PhantomData, rc::Rc, time::Duration};

use livekit_browser::Error;

#[cfg(target_arch = "wasm32")]
use livekit_browser::local_clip_recording_support;

/// Why the browser stopped one Local Clip Recording.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LocalClipStopReason {
    /// The application called [`crate::LocalClipRecording::stop`].
    Explicit,
    /// The configured active-duration limit elapsed.
    DurationLimit,
    /// Delivered chunks reached the configured accumulated-size threshold.
    SizeLimit,
    /// Every selected Local Media Track ended and the browser stopped the recorder.
    SourceEnded,
}

/// A completed browser-local media artifact.
///
/// Cloning this value clones a browser Blob handle; encoded bytes are never copied into WASM
/// linear memory.
#[derive(Clone)]
pub struct LocalClip {
    blob: web_sys::Blob,
    mime_type: String,
    duration: Duration,
    stop_reason: LocalClipStopReason,
    suggested_filename: String,
    size_bytes: u64,
    _browser_thread: PhantomData<Rc<()>>,
}

impl LocalClip {
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn completed(
        blob: web_sys::Blob,
        mime_type: String,
        duration: Duration,
        stop_reason: LocalClipStopReason,
        size_bytes: u64,
    ) -> Self {
        Self {
            blob,
            suggested_filename: suggested_filename(&mime_type),
            mime_type,
            duration,
            stop_reason,
            size_bytes,
            _browser_thread: PhantomData,
        }
    }

    /// Returns the browser-owned Blob without reading its bytes into WASM memory.
    pub fn blob(&self) -> &web_sys::Blob {
        &self.blob
    }

    /// Transfers the browser-owned Blob handle to the caller.
    pub fn into_blob(self) -> web_sys::Blob {
        self.blob
    }

    /// Returns the actual MIME type reported by MediaRecorder.
    pub fn mime_type(&self) -> &str {
        &self.mime_type
    }

    /// Returns active recording time, excluding pauses.
    pub const fn duration(&self) -> Duration {
        self.duration
    }

    /// Returns why this Local Clip Recording stopped.
    pub const fn stop_reason(&self) -> LocalClipStopReason {
        self.stop_reason
    }

    /// Returns a browser-download filename derived from the actual MIME type.
    pub fn suggested_filename(&self) -> &str {
        &self.suggested_filename
    }

    /// Returns the browser Blob size without reading its bytes.
    pub const fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    /// Creates an explicitly revocable object URL for application-defined artifact handling.
    pub fn create_object_url(&self) -> Result<RevocableObjectUrl, Error> {
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        {
            let url = local_clip_recording_support::create_object_url(&self.blob)?;
            Ok(RevocableObjectUrl {
                url,
                active: Cell::new(true),
                _browser_thread: PhantomData,
            })
        }
    }
}

impl fmt::Debug for LocalClip {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalClip")
            .field("mime_type", &self.mime_type)
            .field("duration", &self.duration)
            .field("stop_reason", &self.stop_reason)
            .field("suggested_filename", &self.suggested_filename)
            .field("size_bytes", &self.size_bytes)
            .finish_non_exhaustive()
    }
}

/// Guard for a browser Blob object URL with explicit and final-drop cleanup.
pub struct RevocableObjectUrl {
    url: String,
    active: Cell<bool>,
    _browser_thread: PhantomData<Rc<()>>,
}

impl RevocableObjectUrl {
    /// Returns the URL while the guard remains active.
    pub fn as_str(&self) -> &str {
        &self.url
    }

    /// Whether this guard still owns an active object URL.
    pub fn is_active(&self) -> bool {
        self.active.get()
    }

    /// Revokes the object URL. Repeated calls are no-ops.
    pub fn revoke(&self) -> Result<(), Error> {
        if !self.active.get() {
            return Ok(());
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        {
            local_clip_recording_support::revoke_object_url(&self.url)?;
            self.active.set(false);
            Ok(())
        }
    }
}

impl fmt::Debug for RevocableObjectUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RevocableObjectUrl")
            .field("url", &self.url)
            .field("active", &self.is_active())
            .finish()
    }
}

impl Drop for RevocableObjectUrl {
    fn drop(&mut self) {
        #[cfg(target_arch = "wasm32")]
        if self.active.replace(false) {
            let _ = local_clip_recording_support::revoke_object_url(&self.url);
        }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn suggested_filename(mime_type: &str) -> String {
    let base = mime_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let extension = match base.as_str() {
        "video/webm" | "audio/webm" => "webm",
        "video/mp4" => "mp4",
        "audio/mp4" => "m4a",
        "video/ogg" => "ogv",
        "audio/ogg" => "ogg",
        _ => "bin",
    };
    format!("local-clip.{extension}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_suggestions_follow_actual_mime_type() {
        for (mime_type, expected) in [
            ("video/webm;codecs=vp8,opus", "local-clip.webm"),
            ("audio/mp4", "local-clip.m4a"),
            ("video/mp4", "local-clip.mp4"),
            ("application/octet-stream", "local-clip.bin"),
        ] {
            assert_eq!(suggested_filename(mime_type), expected);
        }
    }
}
