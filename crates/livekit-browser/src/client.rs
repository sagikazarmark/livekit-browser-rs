use crate::*;

#[cfg(target_arch = "wasm32")]
use crate::error::classify_capture_error;
use crate::media::{capture_camera_video_track, capture_microphone_audio_track, capture_screen};
use crate::room::join_room_session;

/// The only `livekit-client` version supported by this crate.
pub const LIVEKIT_CLIENT_VERSION: &str = "2.20.1";

thread_local! {
    static INITIALIZED_CLIENT: OnceCell<Rc<ClientInner>> = const { OnceCell::new() };
}

/// A typed LiveKit SDK log level.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LiveKitLogLevel {
    /// Very fine-grained SDK tracing.
    Trace,
    /// Diagnostic detail useful while debugging.
    Debug,
    /// Routine lifecycle information.
    Info,
    /// A recoverable condition the SDK worked around.
    Warn,
    /// A failure the SDK could not recover from.
    Error,
}

impl LiveKitLogLevel {
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn from_sdk_level(level: u8) -> Option<Self> {
        match level {
            0 => Some(Self::Trace),
            1 => Some(Self::Debug),
            2 => Some(Self::Info),
            3 => Some(Self::Warn),
            4 => Some(Self::Error),
            _ => None,
        }
    }
}

/// One owned log record emitted by the application-loaded LiveKit SDK.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct LiveKitLogRecord {
    level: LiveKitLogLevel,
    message: String,
    context: serde_json::Value,
}

impl LiveKitLogRecord {
    #[cfg(all(test, not(target_arch = "wasm32"), feature = "tracing"))]
    pub(crate) fn test_record(
        level: LiveKitLogLevel,
        message: impl Into<String>,
        context: serde_json::Value,
    ) -> Self {
        Self {
            level,
            message: message.into(),
            context,
        }
    }

    /// Returns the normalized SDK log level.
    pub const fn level(&self) -> LiveKitLogLevel {
        self.level
    }

    /// Returns LiveKit's log message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns JSON-compatible structured context owned by this record.
    ///
    /// Unsupported JavaScript values are converted to descriptive strings by the private adapter.
    pub const fn context(&self) -> &serde_json::Value {
        &self.context
    }
}

type LiveKitLogCallback = Rc<dyn Fn(LiveKitLogRecord)>;

/// The SDK log extension closure kept alive for the lifetime of the runtime.
#[cfg(target_arch = "wasm32")]
type LogExtensionClosure =
    wasm_bindgen::closure::Closure<dyn Fn(u8, String, wasm_bindgen::JsValue)>;

/// Options for initializing the application-wide LiveKit browser runtime.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct InitializeOptions {
    log_callback: Option<LiveKitLogCallback>,
    #[cfg(feature = "tracing")]
    tracing: bool,
}

impl InitializeOptions {
    /// Installs an application-owned callback for typed LiveKit SDK log records.
    ///
    /// The first successful `BrowserClient::initialize` call owns this callback for the browser
    /// runtime lifetime. LiveKit's existing JavaScript console logging remains active.
    pub fn with_log_callback(mut self, callback: impl Fn(LiveKitLogRecord) + 'static) -> Self {
        self.log_callback = Some(Rc::new(callback));
        self
    }

    /// Emits LiveKit SDK log records through Rust `tracing` with target `livekit_client`.
    #[cfg(feature = "tracing")]
    pub const fn with_tracing(mut self) -> Self {
        self.tracing = true;
        self
    }

    #[cfg(target_arch = "wasm32")]
    fn receives_logs(&self) -> bool {
        self.log_callback.is_some() || {
            #[cfg(feature = "tracing")]
            {
                self.tracing
            }
            #[cfg(not(feature = "tracing"))]
            {
                false
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn dispatch_log(&self, record: LiveKitLogRecord) {
        if let Some(callback) = &self.log_callback {
            callback(record.clone());
        }
        #[cfg(feature = "tracing")]
        if self.tracing {
            emit_tracing_record(&record);
        }
    }
}

impl fmt::Debug for InitializeOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("InitializeOptions");
        debug.field("log_callback", &self.log_callback.is_some());
        #[cfg(feature = "tracing")]
        debug.field("tracing", &self.tracing);
        debug.finish()
    }
}

#[cfg(all(feature = "tracing", any(target_arch = "wasm32", test)))]
pub(crate) fn emit_tracing_record(record: &LiveKitLogRecord) {
    match record.level {
        LiveKitLogLevel::Trace => tracing::trace!(
            target: "livekit_client",
            context = ?record.context,
            "{}",
            record.message
        ),
        LiveKitLogLevel::Debug => tracing::debug!(
            target: "livekit_client",
            context = ?record.context,
            "{}",
            record.message
        ),
        LiveKitLogLevel::Info => tracing::info!(
            target: "livekit_client",
            context = ?record.context,
            "{}",
            record.message
        ),
        LiveKitLogLevel::Warn => tracing::warn!(
            target: "livekit_client",
            context = ?record.context,
            "{}",
            record.message
        ),
        LiveKitLogLevel::Error => tracing::error!(
            target: "livekit_client",
            context = ?record.context,
            "{}",
            record.message
        ),
    }
}

/// The participant credentials accepted by a browser Room Session join.
///
/// Participant tokens must be issued by an application server. LiveKit API secrets never belong
/// in browser code and are intentionally not represented by this type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionCredentials {
    server_url: String,
    participant_token: String,
}

impl ConnectionCredentials {
    /// Creates credentials from a LiveKit server URL and participant token.
    pub fn new(server_url: impl Into<String>, participant_token: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            participant_token: participant_token.into(),
        }
    }

    /// Returns the LiveKit WebSocket server URL.
    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    /// Returns the participant token issued by the application server.
    pub fn participant_token(&self) -> &str {
        &self.participant_token
    }
}

/// Options for joining a Room Session.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct RoomOptions {
    auto_subscribe: bool,
    #[cfg(feature = "raw-js")]
    pub(crate) raw_js_room_options: Option<wasm_bindgen::JsValue>,
    #[cfg(feature = "raw-js")]
    pub(crate) raw_js_connect_options: Option<wasm_bindgen::JsValue>,
}

impl Default for RoomOptions {
    fn default() -> Self {
        Self {
            auto_subscribe: true,
            #[cfg(feature = "raw-js")]
            raw_js_room_options: None,
            #[cfg(feature = "raw-js")]
            raw_js_connect_options: None,
        }
    }
}

impl RoomOptions {
    /// Enables or disables automatic subscription to Remote Media Tracks.
    pub const fn with_auto_subscribe(mut self, enabled: bool) -> Self {
        self.auto_subscribe = enabled;
        self
    }

    /// Whether LiveKit automatically subscribes to announced remote media.
    pub const fn auto_subscribe(&self) -> bool {
        self.auto_subscribe
    }

    /// Adds unstable `livekit-client` Room constructor options.
    ///
    /// Typed wrapper fields take precedence over colliding raw fields. See the crate-level
    /// `raw-js` warning before using this escape hatch.
    #[cfg(feature = "raw-js")]
    pub fn with_raw_js_room_options(mut self, options: wasm_bindgen::JsValue) -> Self {
        self.raw_js_room_options = Some(options);
        self
    }

    /// Adds unstable `livekit-client` Room connect options.
    ///
    /// `autoSubscribe` remains controlled by `with_auto_subscribe` when both are supplied.
    #[cfg(feature = "raw-js")]
    pub fn with_raw_js_connect_options(mut self, options: wasm_bindgen::JsValue) -> Self {
        self.raw_js_connect_options = Some(options);
        self
    }
}

/// A browser feature that can be required by the LiveKit runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum BrowserCapability {
    /// Browser WebRTC primitives required by `livekit-client`.
    WebRtc,
    /// Browser camera and microphone capture primitives.
    UserMedia,
    /// Browser display-capture primitives required by screen sharing.
    DisplayCapture,
    /// Browser media encoding required by Local Clip Recording.
    LocalClipRecording,
}

impl fmt::Display for BrowserCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WebRtc => formatter.write_str("WebRTC required by LiveKit"),
            Self::UserMedia => formatter.write_str("browser camera and microphone capture"),
            Self::DisplayCapture => formatter.write_str("browser display capture"),
            Self::LocalClipRecording => formatter.write_str("browser Local Clip Recording"),
        }
    }
}

/// Browser support detected without requesting device or playback permissions.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[non_exhaustive]
pub struct BrowserCapabilities {
    pub(crate) web_rtc: bool,
    pub(crate) secure_context: bool,
    pub(crate) user_media: bool,
    pub(crate) display_capture: bool,
    pub(crate) audio_playback: bool,
    pub(crate) audio_output_selection: bool,
    pub(crate) transcription: bool,
    pub(crate) local_clip_recording: bool,
}

impl BrowserCapabilities {
    /// Whether the browser has the WebRTC primitives required by LiveKit.
    pub const fn web_rtc(self) -> bool {
        self.web_rtc
    }

    /// Whether the page is running in a secure context.
    pub const fn secure_context(self) -> bool {
        self.secure_context
    }

    /// Whether camera and microphone capture APIs are present.
    ///
    /// This reports API availability, not device presence or permission state.
    pub const fn user_media(self) -> bool {
        self.user_media
    }

    /// Whether the browser exposes display capture.
    ///
    /// This reports API availability and does not prompt the user to choose a display.
    pub const fn display_capture(self) -> bool {
        self.display_capture
    }

    /// Whether browser media elements expose audio playback.
    ///
    /// Actual playback may still require a user activation under browser autoplay policy.
    pub const fn audio_playback(self) -> bool {
        self.audio_playback
    }

    /// Whether browser media elements support selecting an audio output device.
    pub const fn audio_output_selection(self) -> bool {
        self.audio_output_selection
    }

    /// Whether the loaded LiveKit SDK exposes normalized transcription segment events.
    pub const fn transcription(self) -> bool {
        self.transcription
    }

    /// Whether the browser exposes the MediaRecorder primitives used by Local Clip Recording.
    ///
    /// This reports API availability without creating a stream or encoder.
    pub const fn local_clip_recording(self) -> bool {
        self.local_clip_recording
    }
}

/// A browser media input kind exposed by this crate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum MediaDeviceKind {
    /// A camera video input.
    Camera,
    /// A microphone audio input.
    Microphone,
}

impl MediaDeviceKind {
    #[cfg(target_arch = "wasm32")]
    const fn adapter_name(self) -> &'static str {
        match self {
            Self::Camera => "videoinput",
            Self::Microphone => "audioinput",
        }
    }
}

/// Stable Rust data describing one browser media input at enumeration time.
///
/// Device IDs are browser-scoped opaque values and may change. Labels can be empty until browser
/// permission policy allows them to be exposed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[non_exhaustive]
pub struct MediaDeviceInfo {
    device_id: String,
    group_id: String,
    label: String,
    kind: MediaDeviceKind,
}

impl MediaDeviceInfo {
    /// Returns the browser's opaque device ID.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Returns the browser's opaque physical-device group ID, when provided.
    pub fn group_id(&self) -> &str {
        &self.group_id
    }

    /// Returns the permission-gated human-readable device label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns whether this input is a camera or microphone.
    pub const fn kind(&self) -> MediaDeviceKind {
        self.kind
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ExactDeviceId {
    exact: String,
}

/// A camera's requested facing direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum FacingMode {
    /// The camera facing the user, typically a front-facing camera.
    User,
    /// The camera facing away from the user, typically a rear-facing camera.
    Environment,
    /// A camera facing to the user's left.
    Left,
    /// A camera facing to the user's right.
    Right,
}

/// A requested camera capture resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CameraResolution {
    width: u32,
    height: u32,
}

impl CameraResolution {
    /// Creates a width-by-height camera resolution in pixels.
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Returns the requested width in pixels.
    pub const fn width(self) -> u32 {
        self.width
    }

    /// Returns the requested height in pixels.
    pub const fn height(self) -> u32 {
        self.height
    }
}

/// Typed common constraints for camera capture and restart.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CameraCaptureOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    device_id: Option<ExactDeviceId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    facing_mode: Option<FacingMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    resolution: Option<CameraResolution>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    frame_rate: Option<f64>,
    #[cfg(feature = "raw-js")]
    #[serde(skip)]
    pub(crate) raw_js_options: Option<wasm_bindgen::JsValue>,
}

impl CameraCaptureOptions {
    /// Selects one exact browser camera device.
    pub fn with_device_id(mut self, device_id: impl Into<String>) -> Self {
        self.device_id = Some(ExactDeviceId {
            exact: device_id.into(),
        });
        self
    }

    /// Requests a camera facing direction.
    pub const fn with_facing_mode(mut self, facing_mode: FacingMode) -> Self {
        self.facing_mode = Some(facing_mode);
        self
    }

    /// Requests a camera capture resolution.
    pub const fn with_resolution(mut self, resolution: CameraResolution) -> Self {
        self.resolution = Some(resolution);
        self
    }

    /// Requests a camera frame rate.
    pub const fn with_frame_rate(mut self, frame_rate: f64) -> Self {
        self.frame_rate = Some(frame_rate);
        self
    }

    /// Adds unstable `livekit-client` camera capture options.
    ///
    /// Typed fields configured on this value take precedence over colliding raw fields.
    #[cfg(feature = "raw-js")]
    pub fn with_raw_js_options(mut self, options: wasm_bindgen::JsValue) -> Self {
        self.raw_js_options = Some(options);
        self
    }
}

/// Typed common constraints for microphone capture and restart.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct MicrophoneCaptureOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    device_id: Option<ExactDeviceId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    auto_gain_control: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    channel_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    echo_cancellation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    latency: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    noise_suppression: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    sample_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    sample_size: Option<u32>,
    #[cfg(feature = "raw-js")]
    #[serde(skip)]
    pub(crate) raw_js_options: Option<wasm_bindgen::JsValue>,
}

impl MicrophoneCaptureOptions {
    /// Selects one exact browser microphone device.
    pub fn with_device_id(mut self, device_id: impl Into<String>) -> Self {
        self.device_id = Some(ExactDeviceId {
            exact: device_id.into(),
        });
        self
    }

    /// Requests browser automatic gain control.
    pub const fn with_auto_gain_control(mut self, enabled: bool) -> Self {
        self.auto_gain_control = Some(enabled);
        self
    }

    /// Requests an input channel count.
    pub const fn with_channel_count(mut self, channel_count: u32) -> Self {
        self.channel_count = Some(channel_count);
        self
    }

    /// Requests browser echo cancellation.
    pub const fn with_echo_cancellation(mut self, enabled: bool) -> Self {
        self.echo_cancellation = Some(enabled);
        self
    }

    /// Requests capture latency in seconds.
    pub const fn with_latency(mut self, latency: f64) -> Self {
        self.latency = Some(latency);
        self
    }

    /// Requests browser noise suppression.
    pub const fn with_noise_suppression(mut self, enabled: bool) -> Self {
        self.noise_suppression = Some(enabled);
        self
    }

    /// Requests an audio sample rate in hertz.
    pub const fn with_sample_rate(mut self, sample_rate: u32) -> Self {
        self.sample_rate = Some(sample_rate);
        self
    }

    /// Requests a sample size in bits.
    pub const fn with_sample_size(mut self, sample_size: u32) -> Self {
        self.sample_size = Some(sample_size);
        self
    }

    /// Adds unstable `livekit-client` microphone capture options.
    ///
    /// Typed fields configured on this value take precedence over colliding raw fields.
    #[cfg(feature = "raw-js")]
    pub fn with_raw_js_options(mut self, options: wasm_bindgen::JsValue) -> Self {
        self.raw_js_options = Some(options);
        self
    }
}

/// Typed options for one browser display-capture request.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(not(feature = "raw-js"), derive(Eq))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ScreenCaptureOptions {
    audio: bool,
    #[cfg(feature = "raw-js")]
    #[serde(skip)]
    pub(crate) raw_js_options: Option<wasm_bindgen::JsValue>,
}

impl ScreenCaptureOptions {
    /// Requests browser-provided system or tab audio with the display.
    ///
    /// A successful capture may still contain no audio because availability depends on the
    /// browser and the display source selected by the user.
    pub const fn with_system_audio(mut self, enabled: bool) -> Self {
        self.audio = enabled;
        self
    }

    /// Adds unstable `livekit-client` display capture options.
    ///
    /// The typed system-audio policy takes precedence over colliding raw fields.
    #[cfg(feature = "raw-js")]
    pub fn with_raw_js_options(mut self, options: wasm_bindgen::JsValue) -> Self {
        self.raw_js_options = Some(options);
        self
    }
}

/// The initialized, browser-thread-local LiveKit runtime.
///
/// Initialization is idempotent: every successful call in one WASM application returns a handle
/// to the same validated runtime. This handle is deliberately neither `Send` nor `Sync`.
#[derive(Clone)]
pub struct BrowserClient {
    pub(crate) inner: Rc<ClientInner>,
}

impl BrowserClient {
    /// Validates the application-loaded LiveKit SDK and required browser support.
    ///
    /// This only inspects browser APIs. It does not create Room Session or media state and does not
    /// request camera, microphone, display-capture, or audio-playback permission.
    pub fn initialize(_options: InitializeOptions) -> Result<Self, Error> {
        if let Some(inner) = INITIALIZED_CLIENT.with(|client| client.get().cloned()) {
            return Ok(Self { inner });
        }

        let capabilities = inspect_runtime()?;
        #[cfg(target_arch = "wasm32")]
        let log_extension = install_log_extension(_options)?;
        let inner = Rc::new(ClientInner {
            capabilities,
            #[cfg(target_arch = "wasm32")]
            _log_extension: log_extension,
        });
        INITIALIZED_CLIENT.with(|client| {
            let _ = client.set(Rc::clone(&inner));
        });

        Ok(Self { inner })
    }

    /// Returns permission-free browser capability information captured at initialization.
    pub fn capabilities(&self) -> BrowserCapabilities {
        self.inner.capabilities
    }

    /// Returns the exact validated LiveKit SDK version.
    pub const fn sdk_version(&self) -> &'static str {
        LIVEKIT_CLIENT_VERSION
    }

    /// Joins one Room Session without capturing or publishing browser media.
    pub fn join(
        &self,
        credentials: ConnectionCredentials,
        options: RoomOptions,
    ) -> Pin<Box<dyn Future<Output = Result<RoomSession, Error>> + 'static>> {
        Box::pin(join_room_session(credentials, options))
    }

    /// Enumerates camera or microphone inputs.
    ///
    /// When `request_permissions` is true, LiveKit may briefly request and release media so that
    /// browsers can return real devices and labels. With it false, labels may be empty.
    pub fn enumerate_media_devices(
        &self,
        kind: MediaDeviceKind,
        request_permissions: bool,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MediaDeviceInfo>, Error>> + 'static>> {
        if request_permissions {
            if let Err(error) = self.ensure_user_media_available() {
                return Box::pin(async move { Err(error) });
            }
        }
        enumerate_media_devices(kind, request_permissions)
    }

    /// Starts one room-independent camera capture.
    pub fn create_camera_video_track(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<LocalVideoTrack, Error>> + 'static>> {
        self.create_camera_video_track_with_options(CameraCaptureOptions::default())
    }

    /// Starts one room-independent camera capture with typed constraints.
    pub fn create_camera_video_track_with_options(
        &self,
        options: CameraCaptureOptions,
    ) -> Pin<Box<dyn Future<Output = Result<LocalVideoTrack, Error>> + 'static>> {
        if let Err(error) = self.ensure_user_media_available() {
            return Box::pin(async move { Err(error) });
        }
        capture_camera_video_track(options)
    }

    /// Starts one room-independent microphone capture.
    pub fn create_microphone_audio_track(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<LocalAudioTrack, Error>> + 'static>> {
        self.create_microphone_audio_track_with_options(MicrophoneCaptureOptions::default())
    }

    /// Starts one room-independent microphone capture with typed constraints.
    pub fn create_microphone_audio_track_with_options(
        &self,
        options: MicrophoneCaptureOptions,
    ) -> Pin<Box<dyn Future<Output = Result<LocalAudioTrack, Error>> + 'static>> {
        if let Err(error) = self.ensure_user_media_available() {
            return Box::pin(async move { Err(error) });
        }
        capture_microphone_audio_track(options)
    }

    /// Starts one room-independent browser display capture.
    ///
    /// The result always contains display video and may contain browser-provided system audio.
    /// This method must be called directly from an active user interaction in browsers that
    /// require transient activation for the display chooser.
    pub fn create_screen_capture(
        &self,
        options: ScreenCaptureOptions,
    ) -> Pin<Box<dyn Future<Output = Result<ScreenCapture, Error>> + 'static>> {
        if !self.inner.capabilities.display_capture() {
            return Box::pin(async {
                Err(Error::UnsupportedBrowser {
                    missing: BrowserCapability::DisplayCapture,
                })
            });
        }
        capture_screen(options)
    }

    fn ensure_user_media_available(&self) -> Result<(), Error> {
        if self.inner.capabilities.user_media() {
            Ok(())
        } else {
            Err(Error::UnsupportedBrowser {
                missing: BrowserCapability::UserMedia,
            })
        }
    }
}

impl fmt::Debug for BrowserClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BrowserClient")
            .field("sdk_version", &LIVEKIT_CLIENT_VERSION)
            .field("capabilities", &self.inner.capabilities)
            .finish()
    }
}

pub(crate) struct ClientInner {
    pub(crate) capabilities: BrowserCapabilities,
    #[cfg(target_arch = "wasm32")]
    _log_extension: Option<LogExtensionClosure>,
}

#[cfg(target_arch = "wasm32")]
fn install_log_extension(options: InitializeOptions) -> Result<Option<LogExtensionClosure>, Error> {
    use wasm_bindgen::JsCast;

    if !options.receives_logs() {
        return Ok(None);
    }
    let callback = wasm_bindgen::closure::Closure::new(
        move |level: u8, message: String, context: wasm_bindgen::JsValue| {
            let Some(level) = LiveKitLogLevel::from_sdk_level(level) else {
                return;
            };
            let context = serde_wasm_bindgen::from_value(context)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
            options.dispatch_log(LiveKitLogRecord {
                level,
                message,
                context,
            });
        },
    );
    adapter::install_log_extension(callback.as_ref().unchecked_ref())
        .map_err(AdapterError::from_cause)
        .map_err(Error::AdapterFailure)?;
    Ok(Some(callback))
}

#[cfg(target_arch = "wasm32")]
fn enumerate_media_devices(
    kind: MediaDeviceKind,
    request_permissions: bool,
) -> Pin<Box<dyn Future<Output = Result<Vec<MediaDeviceInfo>, Error>> + 'static>> {
    let operation = adapter::enumerate_media_devices(kind.adapter_name(), request_permissions)
        .map_err(AdapterError::from_cause)
        .map_err(classify_capture_error);
    let operation = match operation {
        Ok(operation) => operation,
        Err(error) => return Box::pin(async move { Err(error) }),
    };

    Box::pin(async move {
        let devices = wasm_bindgen_futures::JsFuture::from(operation)
            .await
            .map_err(AdapterError::from_cause)
            .map_err(classify_capture_error)?;
        serde_wasm_bindgen::from_value(devices)
            .map_err(|error| Error::AdapterFailure(AdapterError::contract(error.to_string())))
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn enumerate_media_devices(
    _kind: MediaDeviceKind,
    _request_permissions: bool,
) -> Pin<Box<dyn Future<Output = Result<Vec<MediaDeviceInfo>, Error>> + 'static>> {
    Box::pin(async { Err(Error::UnsupportedTarget) })
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn local_media_device_id(
    operation: Result<js_sys::Promise, wasm_bindgen::JsValue>,
    invalid_result_message: &'static str,
) -> Pin<Box<dyn Future<Output = Result<Option<String>, Error>> + 'static>> {
    let operation = operation
        .map_err(AdapterError::from_cause)
        .map_err(classify_capture_error);
    let operation = match operation {
        Ok(operation) => operation,
        Err(error) => return Box::pin(async move { Err(error) }),
    };
    Box::pin(async move {
        let value = wasm_bindgen_futures::JsFuture::from(operation)
            .await
            .map_err(AdapterError::from_cause)
            .map_err(classify_capture_error)?;
        if value.is_null() || value.is_undefined() {
            Ok(None)
        } else {
            value.as_string().map(Some).ok_or_else(|| {
                Error::AdapterFailure(AdapterError::contract(invalid_result_message.into()))
            })
        }
    })
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn start_local_media_device_change(
    mark_terminal: impl FnOnce() + 'static,
    start: impl FnOnce(&js_sys::Function) -> Result<js_sys::Promise, wasm_bindgen::JsValue>,
) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
    use wasm_bindgen::JsCast;

    let callback = wasm_bindgen::closure::Closure::once_into_js(move |became_terminal: bool| {
        if became_terminal {
            mark_terminal();
        }
    });
    let callback: js_sys::Function = callback.unchecked_into();
    let operation = start(&callback)
        .map_err(AdapterError::from_cause)
        .map_err(classify_capture_error);
    let operation = match operation {
        Ok(operation) => operation,
        Err(error) => return Box::pin(async move { Err(error) }),
    };
    Box::pin(async move {
        wasm_bindgen_futures::JsFuture::from(operation)
            .await
            .map_err(AdapterError::from_cause)
            .map_err(classify_capture_error)?;
        Ok(())
    })
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "status")]
pub(crate) enum RuntimeInspection {
    #[serde(rename = "ready")]
    Ready { capabilities: BrowserCapabilities },
    #[serde(rename = "sdkMissing")]
    SdkMissing,
    #[serde(rename = "sdkVersionMismatch")]
    SdkVersionMismatch { actual_version: Option<String> },
    #[serde(rename = "unsupportedBrowser")]
    UnsupportedBrowser,
}

#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn finish_initialization(
    inspection: RuntimeInspection,
) -> Result<BrowserCapabilities, Error> {
    match inspection {
        RuntimeInspection::Ready { capabilities } => Ok(capabilities),
        RuntimeInspection::SdkMissing => Err(Error::SdkMissing),
        RuntimeInspection::SdkVersionMismatch { actual_version } => {
            Err(Error::SdkVersionMismatch {
                expected: LIVEKIT_CLIENT_VERSION,
                actual: actual_version,
            })
        }
        RuntimeInspection::UnsupportedBrowser => Err(Error::UnsupportedBrowser {
            missing: BrowserCapability::WebRtc,
        }),
    }
}

#[cfg(target_arch = "wasm32")]
fn inspect_runtime() -> Result<BrowserCapabilities, Error> {
    let inspection = adapter::inspect_browser_runtime(LIVEKIT_CLIENT_VERSION)
        .map_err(AdapterError::from_cause)
        .map_err(Error::AdapterFailure)?;
    let inspection = serde_wasm_bindgen::from_value(inspection)
        .map_err(|error| Error::AdapterFailure(AdapterError::contract(error.to_string())))?;
    finish_initialization(inspection)
}

#[cfg(not(target_arch = "wasm32"))]
fn inspect_runtime() -> Result<BrowserCapabilities, Error> {
    Err(Error::UnsupportedTarget)
}
