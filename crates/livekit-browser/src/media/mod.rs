#[cfg(target_arch = "wasm32")]
use crate::client::{local_media_device_id, start_local_media_device_change};
#[cfg(target_arch = "wasm32")]
use crate::error::{
    classify_capture_error, classify_display_capture_error, classify_publisher_mutation_error,
    deserialize_statistics,
};
#[cfg(all(feature = "raw-js", target_arch = "wasm32"))]
use crate::error::{raw_js_handle, raw_js_processor_operation, raw_js_statistics};
use crate::*;

mod analyser;
mod local_audio;
mod local_video;

#[cfg(target_arch = "wasm32")]
pub(crate) use analyser::create_audio_analyser;
pub use analyser::{AudioAnalyser, AudioAnalysis};
pub(crate) use analyser::{AudioAnalyserSource, AudioAnalyserState, terminate_audio_analysers};
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use local_audio::AdapterScreenCapture;
pub use local_audio::{AudioTrackAttachment, LocalAudioTrack, LocalAudioTrackPublication};
pub(crate) use local_audio::{capture_microphone_audio_track, capture_screen};
pub(crate) use local_video::capture_camera_video_track;
#[cfg(target_arch = "wasm32")]
pub(crate) use local_video::{
    BrowserCaptureFuture, BrowserCaptureState, complete_browser_capture, create_local_video_track,
};
pub use local_video::{LocalVideoTrack, LocalVideoTrackPublication, VideoTrackAttachment};

/// The SDK callback invoked when a local Track Publication is replaced.
#[cfg(target_arch = "wasm32")]
pub(crate) type PublicationReplacementCallback =
    wasm_bindgen::closure::Closure<dyn Fn(u32, Option<String>, bool) -> u32>;

/// A codec requested for publishing a Local Video Track.
///
/// Browser support varies. LiveKit negotiates the final codec with the server and the receiving
/// participants, so a requested codec is a preference rather than a guarantee.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum VideoCodec {
    /// VP8, the most broadly supported codec across browsers.
    Vp8,
    /// VP9.
    Vp9,
    /// H.264.
    H264,
    /// AV1.
    Av1,
}

/// A standard sender encoding for a published Local Video Track.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VideoEncoding {
    max_bitrate: u32,
    max_framerate: u16,
}

impl VideoEncoding {
    /// Creates an encoding limit in bits per second and frames per second.
    pub const fn new(max_bitrate: u32, max_framerate: u16) -> Self {
        Self {
            max_bitrate,
            max_framerate,
        }
    }

    /// Returns the maximum sender bitrate in bits per second.
    pub const fn max_bitrate(self) -> u32 {
        self.max_bitrate
    }

    /// Returns the maximum sender framerate in frames per second.
    pub const fn max_framerate(self) -> u16 {
        self.max_framerate
    }
}

/// Typed common options for publishing a Local Video Track.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct VideoPublishOptions {
    source: TrackSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    video_codec: Option<VideoCodec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    simulcast: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    video_encoding: Option<VideoEncoding>,
    #[cfg(feature = "raw-js")]
    #[serde(skip)]
    raw_js_options: Option<wasm_bindgen::JsValue>,
}

impl Default for VideoPublishOptions {
    fn default() -> Self {
        Self {
            source: TrackSource::Camera,
            name: None,
            video_codec: None,
            simulcast: None,
            video_encoding: None,
            #[cfg(feature = "raw-js")]
            raw_js_options: None,
        }
    }
}

impl VideoPublishOptions {
    /// Sets the room-visible source represented by the publication.
    pub fn with_source(mut self, source: TrackSource) -> Self {
        self.source = source;
        self
    }

    /// Sets the room-visible Track Publication name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the preferred video codec.
    pub const fn with_codec(mut self, codec: VideoCodec) -> Self {
        self.video_codec = Some(codec);
        self
    }

    /// Enables or disables simulcast.
    pub const fn with_simulcast(mut self, simulcast: bool) -> Self {
        self.simulcast = Some(simulcast);
        self
    }

    /// Sets the sender encoding limits.
    pub const fn with_encoding(mut self, encoding: VideoEncoding) -> Self {
        self.video_encoding = Some(encoding);
        self
    }

    /// Adds unstable `livekit-client` Local Video Track publication options.
    ///
    /// Typed fields configured on this value take precedence over colliding raw fields.
    #[cfg(feature = "raw-js")]
    #[cfg_attr(docsrs, doc(cfg(feature = "raw-js")))]
    pub fn with_raw_js_options(mut self, options: wasm_bindgen::JsValue) -> Self {
        self.raw_js_options = Some(options);
        self
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdapterVideoPublishOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    video_codec: Option<VideoCodec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    simulcast: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    video_encoding: Option<VideoEncoding>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl From<&VideoPublishOptions> for AdapterVideoPublishOptions {
    fn from(options: &VideoPublishOptions) -> Self {
        Self {
            name: options.name.clone(),
            video_codec: options.video_codec,
            simulcast: options.simulcast,
            video_encoding: options.video_encoding,
        }
    }
}

/// The codec policy for publishing a Local Audio Track.
///
/// LiveKit 2.20.1 does not expose an audio codec selector: browser audio is Opus, with RED
/// configured separately. This type makes that fixed policy explicit at the Rust boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum AudioCodec {
    /// Opus, the only codec browsers publish through LiveKit 2.20.1.
    Opus,
}

/// A room-visible source supported by a Local Audio Track.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum AudioTrackSource {
    /// Microphone audio.
    Microphone,
    /// System or tab audio captured alongside display video.
    ScreenShareAudio,
}

/// Typed common options for publishing a Local Audio Track.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AudioPublishOptions {
    source: AudioTrackSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    audio_codec: AudioCodec,
    #[serde(skip_serializing_if = "Option::is_none")]
    dtx: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    red: Option<bool>,
    #[cfg(feature = "raw-js")]
    #[serde(skip)]
    raw_js_options: Option<wasm_bindgen::JsValue>,
}

impl Default for AudioPublishOptions {
    fn default() -> Self {
        Self {
            source: AudioTrackSource::Microphone,
            name: None,
            audio_codec: AudioCodec::Opus,
            dtx: None,
            red: None,
            #[cfg(feature = "raw-js")]
            raw_js_options: None,
        }
    }
}

impl AudioPublishOptions {
    /// Sets the room-visible source represented by the publication.
    pub fn with_source(mut self, source: AudioTrackSource) -> Self {
        self.source = source;
        self
    }

    /// Sets the room-visible Track Publication name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the fixed audio codec policy supported by this LiveKit version.
    pub const fn with_codec(mut self, codec: AudioCodec) -> Self {
        self.audio_codec = codec;
        self
    }

    /// Enables or disables Opus discontinuous transmission.
    pub const fn with_dtx(mut self, dtx: bool) -> Self {
        self.dtx = Some(dtx);
        self
    }

    /// Enables or disables Opus RED redundant encoding.
    pub const fn with_red(mut self, red: bool) -> Self {
        self.red = Some(red);
        self
    }

    /// Adds unstable `livekit-client` Local Audio Track publication options.
    ///
    /// Typed fields configured on this value take precedence over colliding raw fields.
    #[cfg(feature = "raw-js")]
    #[cfg_attr(docsrs, doc(cfg(feature = "raw-js")))]
    pub fn with_raw_js_options(mut self, options: wasm_bindgen::JsValue) -> Self {
        self.raw_js_options = Some(options);
        self
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdapterAudioPublishOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    name: Option<String>,
    audio_codec: AudioCodec,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    dtx: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    red: Option<bool>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl From<&AudioPublishOptions> for AdapterAudioPublishOptions {
    fn from(options: &AudioPublishOptions) -> Self {
        Self {
            name: options.name.clone(),
            audio_codec: options.audio_codec,
            dtx: options.dtx,
            red: options.red,
        }
    }
}

/// Why one Local Media Track became terminal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LocalMediaTrackEndReason {
    /// The application explicitly stopped the track or capture could not be recovered.
    Stopped,
    /// The browser ended the selected display source, such as through its stop-sharing control.
    BrowserEnded,
}

/// A local future that resolves when its Local Media Track becomes terminal.
pub struct LocalMediaTrackEnded {
    state: Rc<LocalMediaTrackEndState>,
    waiter_id: Option<u64>,
}

impl Future for LocalMediaTrackEnded {
    type Output = LocalMediaTrackEndReason;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(reason) = self.state.reason.get() {
            return Poll::Ready(reason);
        }
        let waiter_id = match self.waiter_id {
            Some(waiter_id) => waiter_id,
            None => {
                let waiter_id = self.state.next_waiter_id.get();
                self.state
                    .next_waiter_id
                    .set(waiter_id.checked_add(1).unwrap_or(1));
                self.waiter_id = Some(waiter_id);
                waiter_id
            }
        };
        let mut waiters = self.state.waiters.borrow_mut();
        if let Some((_, waker)) = waiters.iter_mut().find(|(id, _)| *id == waiter_id) {
            if !waker.will_wake(context.waker()) {
                *waker = context.waker().clone();
            }
        } else {
            waiters.push((waiter_id, context.waker().clone()));
        }
        Poll::Pending
    }
}

impl Drop for LocalMediaTrackEnded {
    fn drop(&mut self) {
        if let Some(waiter_id) = self.waiter_id {
            self.state
                .waiters
                .borrow_mut()
                .retain(|(id, _)| *id != waiter_id);
        }
    }
}

pub(crate) struct LocalMediaTrackEndState {
    reason: Cell<Option<LocalMediaTrackEndReason>>,
    next_waiter_id: Cell<u64>,
    pub(crate) waiters: RefCell<Vec<(u64, Waker)>>,
}

impl LocalMediaTrackEndState {
    #[cfg(any(target_arch = "wasm32", test))]
    fn new() -> Rc<Self> {
        Rc::new(Self {
            reason: Cell::new(None),
            next_waiter_id: Cell::new(1),
            waiters: RefCell::new(Vec::new()),
        })
    }

    fn finish(&self, reason: LocalMediaTrackEndReason) {
        if self.reason.replace(Some(reason)).is_none() {
            for (_, waker) in self.waiters.borrow_mut().drain(..) {
                waker.wake();
            }
        }
    }
}

/// One browser display-capture selection with required video and optional system audio.
///
/// Each returned track can be previewed, published, muted, and unpublished independently. Clones
/// share the same underlying captures.
#[derive(Clone)]
pub struct ScreenCapture {
    pub(crate) display_video: LocalVideoTrack,
    pub(crate) system_audio: Option<LocalAudioTrack>,
}

impl ScreenCapture {
    /// Returns the required display video track.
    pub fn display_video(&self) -> &LocalVideoTrack {
        &self.display_video
    }

    /// Returns browser-provided system or tab audio when the selected source supplied it.
    pub fn system_audio(&self) -> Option<&LocalAudioTrack> {
        self.system_audio.as_ref()
    }

    /// Stops every capture resource returned by the display chooser.
    ///
    /// Both stop operations are started before either result is awaited, so a video cleanup
    /// failure cannot prevent system-audio cleanup.
    pub fn stop(&self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        let video_stop = self.display_video.stop();
        let audio_stop = self.system_audio.as_ref().map(LocalAudioTrack::stop);
        Box::pin(async move {
            let video_result = video_stop.await;
            let audio_result = match audio_stop {
                Some(stop) => stop.await,
                None => Ok(()),
            };
            video_result.and(audio_result)
        })
    }
}

impl fmt::Debug for ScreenCapture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScreenCapture")
            .field("display_video", &self.display_video)
            .field("system_audio", &self.system_audio)
            .finish()
    }
}
