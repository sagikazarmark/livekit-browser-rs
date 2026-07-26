use std::{
    cell::{Cell, RefCell},
    fmt,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

use livekit_browser::{
    BrowserCapabilities, BrowserCapability, Error, LocalAudioTrack, LocalVideoTrack,
};

#[cfg(target_arch = "wasm32")]
use livekit_browser::local_clip_recording_support;

#[cfg(target_arch = "wasm32")]
use crate::LocalClipStopReason;
use crate::{LocalClip, LocalClipOptions};

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
                self.finish(Ok(LocalClip::completed(
                    blob,
                    mime_type,
                    duration,
                    stop_reason,
                    size_bytes,
                )));
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

fn invalid_recorder_state(operation: &'static str) -> Error {
    Error::InvalidState {
        operation,
        reason: "Local Clip Recording is not in the required state",
    }
}
