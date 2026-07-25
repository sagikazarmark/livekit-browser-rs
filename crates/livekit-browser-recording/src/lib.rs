//! Browser Local Clip Recording for LiveKit Local Media Tracks.

use std::{
    cell::{Cell, RefCell},
    fmt,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
    time::Duration,
};

use livekit_browser::{
    BrowserCapabilities, BrowserCapability, Error, LocalAudioTrack, LocalVideoTrack,
};

#[cfg(target_arch = "wasm32")]
use livekit_browser::local_clip_recording_support;

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

/// One selected Local Participant media source for a Local Clip Recording.
///
/// Only Local Media Track types convert into this enum. Remote Media Tracks are intentionally not
/// accepted because multi-participant capture belongs to server-side Room Recording.
///
/// ```compile_fail
/// use livekit_browser::RemoteMediaTrack;
/// use livekit_browser_recording::LocalClipTrack;
///
/// fn select_remote(track: RemoteMediaTrack) -> LocalClipTrack {
///     track.into()
/// }
/// ```
#[derive(Clone)]
#[non_exhaustive]
pub enum LocalClipTrack {
    /// A microphone or system-audio Local Media Track.
    Audio(LocalAudioTrack),
    /// A camera or display-video Local Media Track.
    Video(LocalVideoTrack),
}

impl From<LocalAudioTrack> for LocalClipTrack {
    fn from(track: LocalAudioTrack) -> Self {
        Self::Audio(track)
    }
}

impl From<LocalVideoTrack> for LocalClipTrack {
    fn from(track: LocalVideoTrack) -> Self {
        Self::Video(track)
    }
}

impl fmt::Debug for LocalClipTrack {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Audio(track) => formatter.debug_tuple("Audio").field(track).finish(),
            Self::Video(track) => formatter.debug_tuple("Video").field(track).finish(),
        }
    }
}

/// Browser capability-negotiated entry point for Local Clip Recording.
#[derive(Clone, Copy, Debug)]
pub struct LocalClipRecorder {
    _browser_thread: PhantomData<Rc<()>>,
}

impl LocalClipRecorder {
    /// Creates a recorder without requesting media permissions or constructing a MediaRecorder.
    pub fn new(capabilities: BrowserCapabilities) -> Result<Self, Error> {
        if !capabilities.local_clip_recording() {
            return Err(Error::UnsupportedBrowser {
                missing: BrowserCapability::LocalClipRecording,
            });
        }
        Ok(Self {
            _browser_thread: PhantomData,
        })
    }

    /// Starts a Local Clip Recording from the selected Local Media Tracks.
    ///
    /// Local Media Tracks are selected independently of Track Publication state and remain owned
    /// for the recording lifetime. Format support is checked in caller order before a MediaStream
    /// or MediaRecorder is constructed.
    pub fn start(
        &self,
        tracks: impl IntoIterator<Item = LocalClipTrack>,
        options: LocalClipOptions,
    ) -> Result<LocalClipRecording, Error> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = tracks;
            let _ = options;
            Err(Error::UnsupportedTarget)
        }

        #[cfg(target_arch = "wasm32")]
        self.start_in_browser(tracks.into_iter().collect(), options)
    }

    #[cfg(target_arch = "wasm32")]
    fn start_in_browser(
        &self,
        tracks: Vec<LocalClipTrack>,
        options: LocalClipOptions,
    ) -> Result<LocalClipRecording, Error> {
        if !local_clip_recording_support::supported() {
            return Err(Error::UnsupportedBrowser {
                missing: BrowserCapability::LocalClipRecording,
            });
        }

        let selected_mime_type = local_clip_recording_support::select_mime_type(
            options.mime_preferences(),
        )
        .ok_or_else(|| Error::UnsupportedLocalClipRecordingFormat {
            requested: options.mime_preferences().to_vec(),
        })?;

        let audio_tracks = tracks
            .iter()
            .filter_map(|track| match track {
                LocalClipTrack::Audio(track) => Some(track.clone()),
                LocalClipTrack::Video(_) => None,
            })
            .collect::<Vec<_>>();
        let video_tracks = tracks
            .iter()
            .filter_map(|track| match track {
                LocalClipTrack::Audio(_) => None,
                LocalClipTrack::Video(track) => Some(track.clone()),
            })
            .collect::<Vec<_>>();
        let stream =
            local_clip_recording_support::local_media_stream(&audio_tracks, &video_tracks)?;
        let inner = Rc::new(LocalClipRecordingInner {
            state: Cell::new(LocalClipRecordingState::Recording),
            mime_type: RefCell::new(String::new()),
            completion: RefCell::new(None),
            next_waiter_id: Cell::new(1),
            waiters: RefCell::new(Vec::new()),
            _tracks: tracks,
            adapter: RefCell::new(None),
        });
        let weak_inner = Rc::downgrade(&inner);
        let adapter = local_clip_recording_support::start(
            &stream,
            &selected_mime_type,
            options
                .max_duration()
                .map(|duration| duration.as_secs_f64() * 1_000.0)
                .unwrap_or(0.0),
            options.max_bytes().map(|bytes| bytes as f64).unwrap_or(0.0),
            move |event| {
                if let Some(inner) = weak_inner.upgrade() {
                    inner.handle_event(event);
                }
            },
        )?;
        *inner.mime_type.borrow_mut() = adapter.mime_type().to_string();
        inner.adapter.borrow_mut().replace(adapter);
        Ok(LocalClipRecording { inner })
    }
}

/// The lifecycle state shared by every clone of a Local Clip Recording handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LocalClipRecordingState {
    /// The browser is encoding selected Local Media Tracks.
    Recording,
    /// Encoding is paused and duration-limit time is not accumulating.
    Paused,
    /// A stop was requested and the browser is flushing its final chunk.
    Stopping,
    /// The browser finished or failed terminally.
    Stopped,
}

/// One active browser-local encoder over selected Local Media Tracks.
#[derive(Clone)]
pub struct LocalClipRecording {
    inner: Rc<LocalClipRecordingInner>,
}

impl LocalClipRecording {
    /// Returns the shared recorder lifecycle state.
    pub fn state(&self) -> LocalClipRecordingState {
        self.inner.state.get()
    }

    /// Returns the actual MIME type selected by MediaRecorder.
    pub fn mime_type(&self) -> String {
        self.inner.mime_type.borrow().clone()
    }

    /// Pauses encoding and active-duration accumulation.
    pub fn pause(&self) -> Result<(), Error> {
        if self.state() != LocalClipRecordingState::Recording {
            return Err(invalid_recorder_state("pause"));
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.with_adapter(|adapter| adapter.pause())?;
            self.inner.state.set(LocalClipRecordingState::Paused);
            Ok(())
        }
    }

    /// Resumes encoding and active-duration accumulation.
    pub fn resume(&self) -> Result<(), Error> {
        if self.state() != LocalClipRecordingState::Paused {
            return Err(invalid_recorder_state("resume"));
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.with_adapter(|adapter| adapter.resume())?;
            self.inner.state.set(LocalClipRecordingState::Recording);
            Ok(())
        }
    }

    /// Requests an explicit stop and returns a future for the flushed browser Blob.
    pub fn stop(&self) -> Result<LocalClipFinished, Error> {
        match self.state() {
            LocalClipRecordingState::Recording | LocalClipRecordingState::Paused => {
                #[cfg(not(target_arch = "wasm32"))]
                return Err(Error::UnsupportedTarget);
                #[cfg(target_arch = "wasm32")]
                {
                    self.inner.with_adapter(|adapter| adapter.stop())?;
                    self.inner.state.set(LocalClipRecordingState::Stopping);
                }
            }
            LocalClipRecordingState::Stopping | LocalClipRecordingState::Stopped => {}
        }
        Ok(self.finished())
    }

    /// Returns a future for either explicit or automatic completion without requesting stop.
    pub fn finished(&self) -> LocalClipFinished {
        LocalClipFinished {
            inner: Rc::clone(&self.inner),
            waiter_id: None,
        }
    }
}

impl fmt::Debug for LocalClipRecording {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalClipRecording")
            .field("state", &self.state())
            .field("mime_type", &self.mime_type())
            .finish()
    }
}

struct LocalClipRecordingInner {
    state: Cell<LocalClipRecordingState>,
    mime_type: RefCell<String>,
    completion: RefCell<Option<Result<LocalClip, Error>>>,
    next_waiter_id: Cell<u64>,
    waiters: RefCell<Vec<(u64, Waker)>>,
    _tracks: Vec<LocalClipTrack>,
    #[cfg(target_arch = "wasm32")]
    adapter: RefCell<Option<local_clip_recording_support::ActiveLocalClipRecording>>,
}

impl LocalClipRecordingInner {
    #[cfg(target_arch = "wasm32")]
    fn with_adapter<T>(
        &self,
        operation: impl FnOnce(
            &local_clip_recording_support::ActiveLocalClipRecording,
        ) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let adapter = self.adapter.borrow();
        let adapter = adapter.as_ref().ok_or(Error::InvalidState {
            operation: "control Local Clip Recording",
            reason: "browser adapter was not initialized",
        })?;
        operation(adapter)
    }

    #[cfg(target_arch = "wasm32")]
    fn handle_event(&self, event: local_clip_recording_support::Event) {
        match event {
            local_clip_recording_support::Event::Stopping => {
                if matches!(
                    self.state.get(),
                    LocalClipRecordingState::Recording | LocalClipRecordingState::Paused
                ) {
                    self.state.set(LocalClipRecordingState::Stopping);
                }
            }
            local_clip_recording_support::Event::Completed {
                blob,
                mime_type,
                duration,
                size_bytes,
                stop_reason,
            } => {
                let stop_reason = match stop_reason {
                    local_clip_recording_support::StopReason::Explicit => {
                        LocalClipStopReason::Explicit
                    }
                    local_clip_recording_support::StopReason::DurationLimit => {
                        LocalClipStopReason::DurationLimit
                    }
                    local_clip_recording_support::StopReason::SizeLimit => {
                        LocalClipStopReason::SizeLimit
                    }
                    local_clip_recording_support::StopReason::SourceEnded => {
                        LocalClipStopReason::SourceEnded
                    }
                };
                self.finish(Ok(LocalClip {
                    blob,
                    suggested_filename: suggested_filename(&mime_type),
                    mime_type,
                    duration,
                    stop_reason,
                    size_bytes,
                    _browser_thread: PhantomData,
                }));
            }
            local_clip_recording_support::Event::Empty => self.finish(Err(Error::EmptyLocalClip)),
            local_clip_recording_support::Event::Failed(error) => self.finish(Err(error)),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn finish(&self, result: Result<LocalClip, Error>) {
        if self.completion.borrow().is_some() {
            return;
        }
        self.state.set(LocalClipRecordingState::Stopped);
        if let Ok(clip) = &result {
            *self.mime_type.borrow_mut() = clip.mime_type().to_string();
        }
        self.completion.borrow_mut().replace(result);
        let waiters = self.waiters.borrow_mut().drain(..).collect::<Vec<_>>();
        for (_, waker) in waiters {
            waker.wake();
        }
    }
}

/// Completion future for a Local Clip Recording.
pub struct LocalClipFinished {
    inner: Rc<LocalClipRecordingInner>,
    waiter_id: Option<u64>,
}

impl Future for LocalClipFinished {
    type Output = Result<LocalClip, Error>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let completed = self.inner.completion.borrow().clone();
        if let Some(completed) = completed {
            return Poll::Ready(completed);
        }

        let waiter_id = if let Some(waiter_id) = self.waiter_id {
            waiter_id
        } else {
            let waiter_id = self.inner.next_waiter_id.get();
            self.inner
                .next_waiter_id
                .set(waiter_id.checked_add(1).unwrap_or(1));
            self.waiter_id = Some(waiter_id);
            waiter_id
        };
        let mut waiters = self.inner.waiters.borrow_mut();
        if let Some((_, waker)) = waiters.iter_mut().find(|(id, _)| *id == waiter_id) {
            waker.clone_from(context.waker());
        } else {
            waiters.push((waiter_id, context.waker().clone()));
        }
        Poll::Pending
    }
}

impl Drop for LocalClipFinished {
    fn drop(&mut self) {
        if let Some(waiter_id) = self.waiter_id {
            self.inner
                .waiters
                .borrow_mut()
                .retain(|(id, _)| *id != waiter_id);
        }
    }
}

/// Why the browser stopped one Local Clip Recording.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LocalClipStopReason {
    /// The application called [`LocalClipRecording::stop`].
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

fn invalid_recorder_state(operation: &'static str) -> Error {
    Error::InvalidState {
        operation,
        reason: "Local Clip Recording is not in the required state",
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
