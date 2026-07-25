//! Framework-neutral LiveKit browser integration.
//!
//! State-changing handle operations start when their methods are called. Dropping one of those
//! returned Rust futures only abandons that waiter; it does not claim to cancel LiveKit's
//! JavaScript Promise. Creation and join futures instead clean up resources that finish after
//! their Rust waiter is abandoned.
//!
//! # Unstable JavaScript escape hatches
//!
//! The optional `raw-js` feature exposes the exact `livekit-client` 2.20.1 objects used by this
//! wrapper. Those values have no source or behavioral compatibility guarantee. Calling methods on
//! them can bypass mutation serialization, invalidate snapshots and typed handles, remove wrapper
//! listeners, keep terminal objects alive, or prevent deterministic cleanup. Prefer the typed API;
//! raw access is intended only for uncommon integrations that accept those risks.
#![cfg_attr(
    not(feature = "raw-js"),
    doc = r#"
The JavaScript escape hatches are absent unless `raw-js` is enabled:

```compile_fail
use livekit_browser::RoomSession;

fn expose_raw_room(session: &RoomSession) {
    let _ = session.raw_js_room();
}
```

```compile_fail
use livekit_browser::Error;

fn expose_raw_cause(error: &Error) {
    let _ = error.raw_js_cause();
}
```

```compile_fail
use livekit_browser::CameraCaptureOptions;
use wasm_bindgen::JsValue;

let _ = CameraCaptureOptions::default().with_raw_js_options(JsValue::NULL);
```

```compile_fail
use livekit_browser::LocalParticipant;

fn expose_raw_participant(participant: &LocalParticipant) {
    let _ = participant.raw_js_participant();
}
```

```compile_fail
use livekit_browser::LocalVideoTrackPublication;

fn expose_raw_publication(publication: &LocalVideoTrackPublication) {
    let _ = publication.raw_js_publication();
}
```

```compile_fail
use livekit_browser::LocalVideoTrack;

fn expose_raw_media(track: &LocalVideoTrack) {
    let _ = track.raw_js_media_track();
}
```

```compile_fail
use livekit_browser::LocalVideoTrack;
use wasm_bindgen::JsValue;

fn install_raw_processor(track: &LocalVideoTrack) {
    let _ = track.set_raw_js_processor(JsValue::NULL, false);
}
```

```compile_fail
use livekit_browser::LocalVideoTrackPublication;

fn expose_raw_statistics(publication: &LocalVideoTrackPublication) {
    let _ = publication.raw_js_rtc_stats_report();
}
```
"#
)]

use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::{BTreeMap, VecDeque},
    fmt,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    rc::{Rc, Weak},
    task::{Context, Poll, Waker},
    time::{Duration, SystemTime},
};

use futures_core::Stream;
use serde::{Deserialize, Serialize};

#[cfg(target_arch = "wasm32")]
macro_rules! raw_js_options {
    ($options:expr, $field:ident) => {{
        #[cfg(feature = "raw-js")]
        {
            $options
                .$field
                .clone()
                .unwrap_or(wasm_bindgen::JsValue::UNDEFINED)
        }
        #[cfg(not(feature = "raw-js"))]
        {
            wasm_bindgen::JsValue::UNDEFINED
        }
    }};
}

/// The only `livekit-client` version supported by this crate.
pub const LIVEKIT_CLIENT_VERSION: &str = "2.20.1";

thread_local! {
    static INITIALIZED_CLIENT: OnceCell<Rc<ClientInner>> = const { OnceCell::new() };
}

/// A typed LiveKit SDK log level.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LiveKitLogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LiveKitLogLevel {
    #[cfg(any(target_arch = "wasm32", test))]
    fn from_sdk_level(level: u8) -> Option<Self> {
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
fn emit_tracing_record(record: &LiveKitLogRecord) {
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
    raw_js_room_options: Option<wasm_bindgen::JsValue>,
    #[cfg(feature = "raw-js")]
    raw_js_connect_options: Option<wasm_bindgen::JsValue>,
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
    web_rtc: bool,
    secure_context: bool,
    user_media: bool,
    display_capture: bool,
    audio_playback: bool,
    audio_output_selection: bool,
    transcription: bool,
    local_clip_recording: bool,
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

/// An actionable failure from the browser integration.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Error {
    /// The application did not load the LiveKit UMD artifact before WASM.
    SdkMissing,
    /// The loaded LiveKit SDK does not exactly match this crate.
    SdkVersionMismatch {
        /// The exact SDK version required by this crate.
        expected: &'static str,
        /// The loaded version, or `None` if the global did not report one.
        actual: Option<String>,
    },
    /// A browser capability required for all supported workflows is absent.
    UnsupportedBrowser {
        /// The first required capability found to be absent.
        missing: BrowserCapability,
    },
    /// The private JavaScript adapter could not inspect the runtime contract.
    AdapterFailure(AdapterError),
    /// LiveKit rejected a Room Session join.
    JoinFailed(AdapterError),
    /// LiveKit could not finish disconnecting a terminal Room Session.
    LeaveFailed(AdapterError),
    /// The browser or LiveKit SDK could not start local media capture.
    CaptureFailed(AdapterError),
    /// The browser or LiveKit source grants denied an operation.
    PermissionDenied(AdapterError),
    /// The display chooser was cancelled or browser policy rejected the request.
    ///
    /// Browsers commonly report both outcomes as `NotAllowedError`, so they cannot be classified
    /// more narrowly without misleading callers.
    DisplayCaptureNotAllowed(AdapterError),
    /// Display capture was not started directly from an active, focused user interaction.
    DisplayCaptureRequiresActivation(AdapterError),
    /// No capture device satisfying the request is available.
    DeviceUnavailable(AdapterError),
    /// The capture device cannot be read, commonly because another application owns it.
    DeviceInUse(AdapterError),
    /// A local Media Track or browser element operation failed.
    MediaControlFailed(AdapterError),
    /// LiveKit rejected a local Track Publication operation.
    PublicationFailed(AdapterError),
    /// LiveKit rejected a remote Track Publication subscription operation.
    SubscriptionFailed(AdapterError),
    /// LiveKit or the browser could not query normalized media statistics.
    StatisticsFailed(AdapterError),
    /// The browser could not create, read, or close an audio analyser.
    AudioAnalysisFailed(AdapterError),
    /// LiveKit rejected a Local Participant publisher-permission operation.
    PublisherPermissionsFailed(AdapterError),
    /// LiveKit rejected a Local Participant metadata or attribute update.
    ParticipantUpdateFailed(AdapterError),
    /// LiveKit rejected a Local Participant data-packet send operation.
    DataPacketSendFailed(AdapterError),
    /// LiveKit rejected a Local Participant chat-message send operation.
    ChatMessageSendFailed(AdapterError),
    /// LiveKit could not open an outgoing text or byte stream.
    DataStreamOpenFailed(AdapterError),
    /// LiveKit could not write an outgoing text or byte stream chunk.
    DataStreamWriteFailed(AdapterError),
    /// LiveKit could not complete an outgoing text or byte stream.
    DataStreamCompletionFailed(AdapterError),
    /// LiveKit could not read an incoming text or byte stream.
    DataStreamReadFailed(AdapterError),
    /// LiveKit rejected a text or byte stream handler registration.
    DataStreamHandlerFailed(AdapterError),
    /// A Participant RPC invocation failed remotely or in transport.
    RpcInvocationFailed(RpcFailure),
    /// LiveKit rejected a Participant RPC method registration.
    RpcMethodRegistrationFailed(AdapterError),
    /// Local Clip Recording options are invalid or cannot be represented safely in JavaScript.
    InvalidLocalClipRecordingOptions {
        /// Why the options were rejected.
        reason: &'static str,
    },
    /// None of the requested Local Clip Recording formats are browser-supported.
    UnsupportedLocalClipRecordingFormat {
        /// MIME preferences in the order supplied by the caller.
        requested: Vec<String>,
    },
    /// The browser could not encode or finish a Local Clip Recording.
    LocalClipRecordingFailed(AdapterError),
    /// The browser encoder stopped without producing media bytes.
    EmptyLocalClip,
    /// The browser could not create or revoke a Local Clip object URL.
    LocalClipObjectUrlFailed(AdapterError),
    /// An operation is not valid in the current resource state.
    InvalidState {
        /// The operation that was rejected.
        operation: &'static str,
        /// The resource state that prevents it.
        reason: &'static str,
    },
    /// Initialization was attempted outside a browser WASM target.
    UnsupportedTarget,
}

impl Error {
    fn adapter_error(&self) -> Option<&AdapterError> {
        match self {
            Self::AdapterFailure(error)
            | Self::JoinFailed(error)
            | Self::LeaveFailed(error)
            | Self::CaptureFailed(error)
            | Self::PermissionDenied(error)
            | Self::DisplayCaptureNotAllowed(error)
            | Self::DisplayCaptureRequiresActivation(error)
            | Self::DeviceUnavailable(error)
            | Self::DeviceInUse(error)
            | Self::MediaControlFailed(error)
            | Self::PublicationFailed(error)
            | Self::SubscriptionFailed(error)
            | Self::StatisticsFailed(error)
            | Self::AudioAnalysisFailed(error)
            | Self::PublisherPermissionsFailed(error)
            | Self::ParticipantUpdateFailed(error)
            | Self::DataPacketSendFailed(error)
            | Self::ChatMessageSendFailed(error)
            | Self::DataStreamOpenFailed(error)
            | Self::DataStreamWriteFailed(error)
            | Self::DataStreamCompletionFailed(error)
            | Self::DataStreamReadFailed(error)
            | Self::DataStreamHandlerFailed(error)
            | Self::RpcMethodRegistrationFailed(error)
            | Self::LocalClipRecordingFailed(error)
            | Self::LocalClipObjectUrlFailed(error) => Some(error),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SdkMissing => write!(
                formatter,
                "LiveKit browser SDK is missing; load livekit-client {LIVEKIT_CLIENT_VERSION} before WASM"
            ),
            Self::SdkVersionMismatch { expected, actual } => write!(
                formatter,
                "expected livekit-client {expected}, found {}",
                actual.as_deref().unwrap_or("an SDK with no reported version")
            ),
            Self::UnsupportedBrowser { missing } => write!(formatter, "{missing} is unavailable"),
            Self::AdapterFailure(error) => {
                write!(formatter, "LiveKit browser adapter failed: {error}")
            }
            Self::JoinFailed(error) => write!(formatter, "could not join LiveKit room: {error}"),
            Self::LeaveFailed(error) => {
                write!(formatter, "could not disconnect LiveKit room: {error}")
            }
            Self::CaptureFailed(error) => write!(formatter, "could not capture local media: {error}"),
            Self::PermissionDenied(error) => {
                write!(formatter, "operation permission denied: {error}")
            }
            Self::DisplayCaptureNotAllowed(error) => {
                write!(formatter, "display capture was cancelled or not allowed: {error}")
            }
            Self::DisplayCaptureRequiresActivation(error) => write!(
                formatter,
                "display capture requires an active, focused user interaction: {error}"
            ),
            Self::DeviceUnavailable(error) => {
                write!(formatter, "media capture device unavailable: {error}")
            }
            Self::DeviceInUse(error) => {
                write!(formatter, "media capture device is already in use: {error}")
            }
            Self::MediaControlFailed(error) => {
                write!(formatter, "could not control browser media: {error}")
            }
            Self::PublicationFailed(error) => {
                write!(formatter, "could not control LiveKit Track Publication: {error}")
            }
            Self::SubscriptionFailed(error) => {
                write!(formatter, "could not control LiveKit subscription: {error}")
            }
            Self::StatisticsFailed(error) => {
                write!(formatter, "could not query LiveKit media statistics: {error}")
            }
            Self::AudioAnalysisFailed(error) => {
                write!(formatter, "could not analyse browser audio: {error}")
            }
            Self::PublisherPermissionsFailed(error) => {
                write!(formatter, "could not control publisher permissions: {error}")
            }
            Self::ParticipantUpdateFailed(error) => {
                write!(formatter, "could not update Local Participant state: {error}")
            }
            Self::DataPacketSendFailed(error) => {
                write!(formatter, "could not send LiveKit data packet: {error}")
            }
            Self::ChatMessageSendFailed(error) => {
                write!(formatter, "could not send LiveKit chat message: {error}")
            }
            Self::DataStreamOpenFailed(error) => {
                write!(formatter, "could not open LiveKit data stream: {error}")
            }
            Self::DataStreamWriteFailed(error) => {
                write!(formatter, "could not write LiveKit data stream: {error}")
            }
            Self::DataStreamCompletionFailed(error) => {
                write!(formatter, "could not complete LiveKit data stream: {error}")
            }
            Self::DataStreamReadFailed(error) => {
                write!(formatter, "could not read LiveKit data stream: {error}")
            }
            Self::DataStreamHandlerFailed(error) => {
                write!(formatter, "could not register LiveKit data stream handler: {error}")
            }
            Self::RpcInvocationFailed(error) => {
                write!(formatter, "Participant RPC invocation failed: {error}")
            }
            Self::RpcMethodRegistrationFailed(error) => {
                write!(formatter, "could not register Participant RPC method: {error}")
            }
            Self::InvalidLocalClipRecordingOptions { reason } => {
                write!(formatter, "invalid Local Clip Recording options: {reason}")
            }
            Self::UnsupportedLocalClipRecordingFormat { requested } => write!(
                formatter,
                "none of the requested Local Clip Recording MIME types are supported: {}",
                requested.join(", ")
            ),
            Self::LocalClipRecordingFailed(error) => {
                write!(formatter, "could not create Local Clip Recording: {error}")
            }
            Self::EmptyLocalClip => {
                formatter.write_str("Local Clip Recording produced an empty artifact")
            }
            Self::LocalClipObjectUrlFailed(error) => {
                write!(formatter, "could not manage Local Clip object URL: {error}")
            }
            Self::InvalidState { operation, reason } => {
                write!(formatter, "cannot {operation}: {reason}")
            }
            Self::UnsupportedTarget => {
                formatter.write_str("LiveKit browser initialization requires wasm32 in a browser")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Some(error) = self.adapter_error() {
            Some(error)
        } else if let Self::RpcInvocationFailed(error) = self {
            Some(error)
        } else {
            None
        }
    }
}

#[cfg(feature = "raw-js")]
impl Error {
    /// Returns the original JavaScript failure retained by this typed error, when one exists.
    ///
    /// The returned value is an unstable diagnostic escape hatch and must not be used to decide
    /// routine application control flow.
    pub fn raw_js_cause(&self) -> Option<&wasm_bindgen::JsValue> {
        self.adapter_error()
            .and_then(AdapterError::raw_js_cause)
            .or_else(|| match self {
                Self::RpcInvocationFailed(error) => error.raw_js_cause(),
                _ => None,
            })
    }
}

/// A normalized failure from the private JavaScript adapter.
///
/// The original JavaScript cause is retained internally on WASM but is not available to routine
/// Rust callers.
#[derive(Clone)]
pub struct AdapterError {
    name: String,
    message: String,
    status: Option<u16>,
    reason_name: Option<String>,
    #[cfg(target_arch = "wasm32")]
    _cause: Option<wasm_bindgen::JsValue>,
}

impl AdapterError {
    /// Returns the JavaScript error name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the JavaScript error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    #[cfg(feature = "raw-js")]
    fn raw_js_cause(&self) -> Option<&wasm_bindgen::JsValue> {
        #[cfg(target_arch = "wasm32")]
        {
            self._cause.as_ref()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            None
        }
    }

    /// Returns LiveKit's typed data-stream failure reason, when this is a stream failure.
    pub fn data_stream_reason(&self) -> Option<DataStreamErrorReason> {
        match self.reason_name.as_deref()? {
            "AlreadyOpened" => Some(DataStreamErrorReason::AlreadyOpened),
            "AbnormalEnd" => Some(DataStreamErrorReason::AbnormalEnd),
            "DecodeFailed" => Some(DataStreamErrorReason::DecodeFailed),
            "LengthExceeded" => Some(DataStreamErrorReason::LengthExceeded),
            "Incomplete" => Some(DataStreamErrorReason::Incomplete),
            "HandlerAlreadyRegistered" => Some(DataStreamErrorReason::HandlerAlreadyRegistered),
            "EncryptionTypeMismatch" => Some(DataStreamErrorReason::EncryptionTypeMismatch),
            "DestinationUnavailable" => Some(DataStreamErrorReason::DestinationUnavailable),
            "RemoteCancelled" => Some(DataStreamErrorReason::RemoteCancelled),
            "FlowControlTimeout" => Some(DataStreamErrorReason::FlowControlTimeout),
            "FlowControlProtocol" => Some(DataStreamErrorReason::FlowControlProtocol),
            _ => None,
        }
    }

    /// Returns the HTTP-style status reported by LiveKit, when available.
    #[cfg(any(target_arch = "wasm32", test))]
    const fn status(&self) -> Option<u16> {
        self.status
    }

    #[cfg(target_arch = "wasm32")]
    fn contract(message: String) -> Self {
        Self {
            name: "ContractError".into(),
            message,
            status: None,
            reason_name: None,
            #[cfg(target_arch = "wasm32")]
            _cause: None,
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn from_cause(cause: wasm_bindgen::JsValue) -> Self {
        let property = |name| {
            js_sys::Reflect::get(&cause, &wasm_bindgen::JsValue::from_str(name))
                .ok()
                .and_then(|value| value.as_string())
        };
        let name = property("name").unwrap_or_else(|| "Error".into());
        let message = property("message")
            .or_else(|| cause.as_string())
            .unwrap_or_else(|| "unknown JavaScript failure".into());
        let reason_name = property("reasonName");
        let status = js_sys::Reflect::get(&cause, &wasm_bindgen::JsValue::from_str("status"))
            .ok()
            .and_then(|value| value.as_f64())
            .filter(|value| value.is_finite() && value.fract() == 0.0)
            .and_then(|value| u16::try_from(value as i64).ok());

        Self {
            name,
            message,
            status,
            reason_name,
            _cause: Some(cause),
        }
    }
}

impl fmt::Debug for AdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdapterError")
            .field("name", &self.name)
            .field("message", &self.message)
            .field("status", &self.status)
            .field("reason_name", &self.reason_name)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for AdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.name, self.message)
    }
}

impl PartialEq for AdapterError {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.message == other.message
            && self.status == other.status
            && self.reason_name == other.reason_name
    }
}

impl Eq for AdapterError {}

impl std::error::Error for AdapterError {}

#[cfg(any(target_arch = "wasm32", test))]
fn classify_capture_error(error: AdapterError) -> Error {
    match error.name() {
        "NotAllowedError" | "SecurityError" | "PermissionDeniedError" => {
            Error::PermissionDenied(error)
        }
        "NotFoundError" | "OverconstrainedError" | "DevicesNotFoundError" => {
            Error::DeviceUnavailable(error)
        }
        "NotReadableError" | "AbortError" | "TrackStartError" => Error::DeviceInUse(error),
        _ => Error::CaptureFailed(error),
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn classify_display_capture_error(error: AdapterError) -> Error {
    match error.name() {
        "DeviceUnsupportedError" => Error::UnsupportedBrowser {
            missing: BrowserCapability::DisplayCapture,
        },
        "NotAllowedError" | "PermissionDeniedError" => Error::DisplayCaptureNotAllowed(error),
        "InvalidStateError" => Error::DisplayCaptureRequiresActivation(error),
        "SecurityError" => Error::PermissionDenied(error),
        "NotFoundError" | "OverconstrainedError" | "DevicesNotFoundError" => {
            Error::DeviceUnavailable(error)
        }
        _ => Error::CaptureFailed(error),
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn classify_publisher_mutation_error(error: AdapterError) -> Error {
    if is_permission_error(&error) {
        Error::PermissionDenied(error)
    } else {
        Error::PublicationFailed(error)
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn classify_subscription_permission_error(error: AdapterError) -> Error {
    if is_permission_error(&error) {
        Error::PermissionDenied(error)
    } else {
        Error::PublisherPermissionsFailed(error)
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn classify_participant_update_error(error: AdapterError) -> Error {
    if is_permission_error(&error) {
        Error::PermissionDenied(error)
    } else {
        Error::ParticipantUpdateFailed(error)
    }
}

#[cfg(target_arch = "wasm32")]
fn deserialize_statistics<T>(
    operation: Result<js_sys::Promise, wasm_bindgen::JsValue>,
) -> Pin<Box<dyn Future<Output = Result<T, Error>> + 'static>>
where
    T: serde::de::DeserializeOwned + 'static,
{
    let operation = operation
        .map_err(AdapterError::from_cause)
        .map_err(Error::StatisticsFailed);
    Box::pin(async move {
        let value = wasm_bindgen_futures::JsFuture::from(operation?)
            .await
            .map_err(AdapterError::from_cause)
            .map_err(Error::StatisticsFailed)?;
        serde_wasm_bindgen::from_value(value)
            .map_err(|error| AdapterError::contract(error.to_string()))
            .map_err(Error::StatisticsFailed)
    })
}

#[cfg(all(feature = "raw-js", target_arch = "wasm32"))]
fn raw_js_handle(
    result: Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>,
) -> Result<wasm_bindgen::JsValue, Error> {
    result
        .map_err(AdapterError::from_cause)
        .map_err(Error::AdapterFailure)
}

#[cfg(all(feature = "raw-js", target_arch = "wasm32"))]
fn raw_js_statistics(
    operation: Result<js_sys::Promise, wasm_bindgen::JsValue>,
) -> Pin<Box<dyn Future<Output = Result<Option<wasm_bindgen::JsValue>, Error>> + 'static>> {
    let operation = operation
        .map_err(AdapterError::from_cause)
        .map_err(Error::StatisticsFailed);
    Box::pin(async move {
        let value = wasm_bindgen_futures::JsFuture::from(operation?)
            .await
            .map_err(AdapterError::from_cause)
            .map_err(Error::StatisticsFailed)?;
        Ok((!value.is_null() && !value.is_undefined()).then_some(value))
    })
}

#[cfg(all(feature = "raw-js", target_arch = "wasm32"))]
fn raw_js_processor_operation(
    operation: Result<js_sys::Promise, wasm_bindgen::JsValue>,
) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
    let operation = operation
        .map_err(AdapterError::from_cause)
        .map_err(Error::MediaControlFailed);
    Box::pin(async move {
        wasm_bindgen_futures::JsFuture::from(operation?)
            .await
            .map(|_| ())
            .map_err(AdapterError::from_cause)
            .map_err(Error::MediaControlFailed)
    })
}

#[cfg(any(target_arch = "wasm32", test))]
fn classify_data_packet_send_error(error: AdapterError) -> Error {
    if is_permission_error(&error) {
        Error::PermissionDenied(error)
    } else {
        Error::DataPacketSendFailed(error)
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn classify_chat_message_send_error(error: AdapterError) -> Error {
    if is_permission_error(&error) {
        Error::PermissionDenied(error)
    } else {
        Error::ChatMessageSendFailed(error)
    }
}

#[cfg(target_arch = "wasm32")]
fn classify_data_stream_open_error(error: AdapterError) -> Error {
    if is_permission_error(&error) {
        Error::PermissionDenied(error)
    } else {
        Error::DataStreamOpenFailed(error)
    }
}

#[cfg(target_arch = "wasm32")]
fn participant_update_operation(
    operation: Result<js_sys::Promise, Error>,
) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
    Box::pin(async move {
        let operation = operation?;
        wasm_bindgen_futures::JsFuture::from(operation)
            .await
            .map(|_| ())
            .map_err(AdapterError::from_cause)
            .map_err(classify_participant_update_error)
    })
}

#[cfg(any(target_arch = "wasm32", test))]
fn is_permission_error(error: &AdapterError) -> bool {
    error.status() == Some(403)
        || error.reason_name.as_deref() == Some("NOT_ALLOWED")
        || matches!(
            error.name(),
            "NotAllowedError" | "PermissionDeniedError" | "UnauthorizedError"
        )
}

/// A browser media input kind exposed by this crate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
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
struct ExactDeviceId {
    exact: String,
}

/// A camera's requested facing direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum FacingMode {
    User,
    Environment,
    Left,
    Right,
}

/// A requested camera capture resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
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
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CameraCaptureOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    device_id: Option<ExactDeviceId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    facing_mode: Option<FacingMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolution: Option<CameraResolution>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frame_rate: Option<f64>,
    #[cfg(feature = "raw-js")]
    #[serde(skip)]
    raw_js_options: Option<wasm_bindgen::JsValue>,
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
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct MicrophoneCaptureOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    device_id: Option<ExactDeviceId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_gain_control: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    echo_cancellation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    noise_suppression: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample_size: Option<u32>,
    #[cfg(feature = "raw-js")]
    #[serde(skip)]
    raw_js_options: Option<wasm_bindgen::JsValue>,
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
#[cfg_attr(not(feature = "raw-js"), derive(Eq))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ScreenCaptureOptions {
    audio: bool,
    #[cfg(feature = "raw-js")]
    #[serde(skip)]
    raw_js_options: Option<wasm_bindgen::JsValue>,
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
    inner: Rc<ClientInner>,
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

struct ClientInner {
    capabilities: BrowserCapabilities,
    #[cfg(target_arch = "wasm32")]
    _log_extension:
        Option<wasm_bindgen::closure::Closure<dyn Fn(u8, String, wasm_bindgen::JsValue)>>,
}

#[cfg(target_arch = "wasm32")]
fn install_log_extension(
    options: InitializeOptions,
) -> Result<Option<wasm_bindgen::closure::Closure<dyn Fn(u8, String, wasm_bindgen::JsValue)>>, Error>
{
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
fn local_media_device_id(
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
fn start_local_media_device_change(
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

/// A preferred codec for publishing a Local Video Track.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum VideoCodec {
    Vp8,
    Vp9,
    H264,
    Av1,
}

/// A standard sender encoding for a published Local Video Track.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
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
    pub fn with_raw_js_options(mut self, options: wasm_bindgen::JsValue) -> Self {
        self.raw_js_options = Some(options);
        self
    }
}

/// The codec policy for publishing a Local Audio Track.
///
/// LiveKit 2.20.1 does not expose an audio codec selector: browser audio is Opus, with RED
/// configured separately. This type makes that fixed policy explicit at the Rust boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum AudioCodec {
    Opus,
}

/// A room-visible source supported by a Local Audio Track.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum AudioTrackSource {
    Microphone,
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
    pub fn with_raw_js_options(mut self, options: wasm_bindgen::JsValue) -> Self {
        self.raw_js_options = Some(options);
        self
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

struct LocalMediaTrackEndState {
    reason: Cell<Option<LocalMediaTrackEndReason>>,
    next_waiter_id: Cell<u64>,
    waiters: RefCell<Vec<(u64, Waker)>>,
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
    display_video: LocalVideoTrack,
    system_audio: Option<LocalAudioTrack>,
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

/// A browser-thread-local local video capture owned independently of any Room Session.
///
/// Clones share one underlying browser capture. Explicit stop is terminal for every clone, and
/// dropping the final clone stops capture as a fallback.
#[derive(Clone)]
pub struct LocalVideoTrack {
    inner: Rc<LocalVideoTrackInner>,
}

impl PartialEq for LocalVideoTrack {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for LocalVideoTrack {}

impl LocalVideoTrack {
    /// Returns the authoritative source that created this track.
    pub fn source(&self) -> TrackSource {
        self.inner.source
    }

    /// Whether capture has been stopped for every clone.
    pub fn is_stopped(&self) -> bool {
        self.inner.lifecycle.get() == LocalVideoTrackLifecycle::Stopped
    }

    /// Returns the unstable `livekit-client` Local Video Track object.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_media_track(&self) -> Result<wasm_bindgen::JsValue, Error> {
        if self.is_stopped() {
            return Err(Error::InvalidState {
                operation: "access raw Local Video Track",
                reason: "capture has stopped",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        raw_js_handle(adapter::raw_local_media_track(self.inner.track_id, "video"))
    }

    /// Installs an unstable JavaScript video processor through the track mutation queue.
    #[cfg(feature = "raw-js")]
    pub fn set_raw_js_processor(
        &self,
        _processor: wasm_bindgen::JsValue,
        _show_processed_stream_locally: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if self.is_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "set raw Local Video Track processor",
                    reason: "capture has stopped",
                })
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });
        #[cfg(target_arch = "wasm32")]
        raw_js_processor_operation(adapter::set_local_media_track_processor(
            self.inner.track_id,
            "video",
            _processor,
            _show_processed_stream_locally,
        ))
    }

    /// Returns the currently installed unstable JavaScript video processor.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_processor(&self) -> Result<Option<wasm_bindgen::JsValue>, Error> {
        if self.is_stopped() {
            return Err(Error::InvalidState {
                operation: "access raw Local Video Track processor",
                reason: "capture has stopped",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        {
            let value = adapter::raw_local_media_track_processor(self.inner.track_id, "video")
                .map_err(AdapterError::from_cause)
                .map_err(Error::MediaControlFailed)?;
            Ok((!value.is_null() && !value.is_undefined()).then_some(value))
        }
    }

    /// Stops the currently installed unstable JavaScript video processor.
    #[cfg(feature = "raw-js")]
    pub fn stop_raw_js_processor(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if self.is_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "stop raw Local Video Track processor",
                    reason: "capture has stopped",
                })
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });
        #[cfg(target_arch = "wasm32")]
        raw_js_processor_operation(adapter::stop_local_media_track_processor(
            self.inner.track_id,
            "video",
        ))
    }

    /// Returns the browser device ID currently producing this track.
    pub fn device_id(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, Error>> + 'static>> {
        if self.source() != TrackSource::Camera {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "inspect Local Video Track device",
                    reason: "display capture has no selectable camera device",
                })
            });
        }
        if self.is_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "inspect Local Video Track device",
                    reason: "capture has stopped",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            local_media_device_id(
                adapter::local_video_track_device_id(self.inner.track_id),
                "camera device query returned a non-string ID",
            )
        }
    }

    /// Restarts this track with its currently committed camera constraints.
    pub fn restart(&self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if let Err(error) = self.ensure_active_for_device_change("restart Local Video Track") {
            return Box::pin(async move { Err(error) });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let inner = Rc::clone(&self.inner);
            start_local_media_device_change(
                move || {
                    inner.mark_stopped();
                },
                |callback| adapter::restart_local_video_track(self.inner.track_id, callback),
            )
        }
    }

    /// Restarts this track with new typed camera constraints.
    ///
    /// The existing Room Session, Track Publication, mute state, and preview attachment are kept.
    /// If applying the new constraints fails, the adapter restores the previously committed input.
    pub fn restart_with_options(
        &self,
        _options: CameraCaptureOptions,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if let Err(error) = self.ensure_active_for_device_change("restart Local Video Track") {
            return Box::pin(async move { Err(error) });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let raw_options = raw_js_options!(_options, raw_js_options);
            let options = match serde_wasm_bindgen::to_value(&_options) {
                Ok(options) => options,
                Err(error) => {
                    return Box::pin(async move {
                        Err(Error::AdapterFailure(AdapterError::contract(
                            error.to_string(),
                        )))
                    });
                }
            };
            let inner = Rc::clone(&self.inner);
            start_local_media_device_change(
                move || {
                    inner.mark_stopped();
                },
                |callback| {
                    adapter::restart_local_video_track_with_options(
                        self.inner.track_id,
                        options,
                        raw_options,
                        callback,
                    )
                },
            )
        }
    }

    /// Switches this track to one exact camera while retaining its other constraints.
    pub fn switch_device(
        &self,
        device_id: impl Into<String>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if let Err(error) = self.ensure_active_for_device_change("switch Local Video Track device")
        {
            return Box::pin(async move { Err(error) });
        }
        let _device_id = device_id.into();

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let inner = Rc::clone(&self.inner);
            start_local_media_device_change(
                move || {
                    inner.mark_stopped();
                },
                |callback| {
                    adapter::switch_local_video_track_device(
                        self.inner.track_id,
                        &_device_id,
                        callback,
                    )
                },
            )
        }
    }

    fn ensure_active_for_device_change(&self, operation: &'static str) -> Result<(), Error> {
        if self.source() != TrackSource::Camera {
            Err(Error::InvalidState {
                operation,
                reason: "display capture cannot restart or switch camera devices",
            })
        } else if self.is_stopped() {
            Err(Error::InvalidState {
                operation,
                reason: "capture has stopped",
            })
        } else {
            Ok(())
        }
    }

    /// Returns why this capture ended, if it is already terminal.
    pub fn end_reason(&self) -> Option<LocalMediaTrackEndReason> {
        self.inner.end_state.reason.get()
    }

    /// Resolves when this capture becomes terminal.
    pub fn ended(&self) -> LocalMediaTrackEnded {
        LocalMediaTrackEnded {
            state: Rc::clone(&self.inner.end_state),
            waiter_id: None,
        }
    }

    /// Makes every clone terminal and stops the shared camera capture.
    ///
    /// Terminal state and attachment termination are installed when this method is called. Await
    /// the returned local future for deterministic browser cleanup.
    pub fn stop(&self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        let _newly_stopped = self.inner.mark_stopped();
        #[cfg(target_arch = "wasm32")]
        if _newly_stopped || self.inner.stop_operation.borrow().is_none() {
            self.inner.start_stop();
        }

        let _inner = Rc::clone(&self.inner);
        Box::pin(async move {
            #[cfg(target_arch = "wasm32")]
            _inner
                .await_stop()
                .await
                .map_err(Error::MediaControlFailed)?;

            Ok(())
        })
    }

    /// Attaches this capture to one browser video element without publishing it.
    pub fn attach(
        &self,
        _element: &web_sys::HtmlVideoElement,
    ) -> Result<VideoTrackAttachment, Error> {
        if self.is_stopped() {
            return Err(Error::InvalidState {
                operation: "attach Local Video Track",
                reason: "capture has stopped",
            });
        }

        #[cfg(target_arch = "wasm32")]
        let attachment_id = adapter::attach_local_video_track(self.inner.track_id, _element)
            .map_err(AdapterError::from_cause)
            .map_err(Error::MediaControlFailed)?;
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        {
            let inner = Rc::new(VideoTrackAttachmentInner {
                active: Cell::new(true),
                track: Rc::downgrade(&self.inner),
                track_id: self.inner.track_id,
                attachment_id,
            });
            self.inner
                .attachments
                .borrow_mut()
                .push(Rc::downgrade(&inner));
            Ok(VideoTrackAttachment { inner })
        }
    }

    /// Returns this track's current publication command handle, including an in-flight publish.
    ///
    /// This lets callers observe and recover a publication whose Rust operation future was
    /// abandoned even though its underlying JavaScript Promise continued.
    pub fn publication(&self) -> Option<LocalVideoTrackPublication> {
        self.inner.clear_terminal_publication();
        let inner = self
            .inner
            .replacement_publication
            .borrow()
            .as_ref()
            .cloned()
            .or_else(|| self.inner.publication.borrow().as_ref().cloned());
        inner.map(|inner| LocalVideoTrackPublication { inner })
    }

    /// Publishes this capture into one active Room Session.
    ///
    /// A Local Video Track can have at most one active or in-flight publication association.
    pub fn publish(
        &self,
        session: &RoomSession,
        options: VideoPublishOptions,
    ) -> Pin<Box<dyn Future<Output = Result<LocalVideoTrackPublication, Error>> + 'static>> {
        if self.is_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "publish Local Video Track",
                    reason: "capture has stopped",
                })
            });
        }
        if session.inner.lifecycle.get() != SessionLifecycle::Active {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "publish Local Video Track",
                    reason: "Room Session is terminal",
                })
            });
        }
        if options.source != self.source() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "publish Local Video Track",
                    reason: "publication source does not match captured media",
                })
            });
        }

        self.inner.clear_terminal_publication();
        let is_replacement = self
            .inner
            .publication
            .borrow()
            .as_ref()
            .map(|publication| publication.lifecycle.get())
            == Some(LocalVideoTrackPublicationLifecycle::Unpublishing);
        if self.inner.replacement_publication.borrow().is_some()
            || (self.inner.publication.borrow().is_some() && !is_replacement)
        {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "publish Local Video Track",
                    reason: "track is already associated with a Room Session",
                })
            });
        }

        #[cfg(all(not(target_arch = "wasm32"), test))]
        {
            let publication = Rc::new(LocalVideoTrackPublicationInner {
                lifecycle: Cell::new(LocalVideoTrackPublicationLifecycle::Published),
                muted: Cell::new(self.inner.muted.get()),
                sid: RefCell::new(Some("TR_test".into())),
                session: Rc::downgrade(&session.inner),
                track: Rc::downgrade(&self.inner),
            });
            self.inner
                .publication
                .borrow_mut()
                .replace(Rc::clone(&publication));
            Box::pin(async move { Ok(LocalVideoTrackPublication { inner: publication }) })
        }

        #[cfg(all(not(target_arch = "wasm32"), not(test)))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            self.start_publish(session, options, is_replacement)
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn start_publish(
        &self,
        session: &RoomSession,
        options: VideoPublishOptions,
        is_replacement: bool,
    ) -> Pin<Box<dyn Future<Output = Result<LocalVideoTrackPublication, Error>> + 'static>> {
        use wasm_bindgen::JsCast;

        let raw_options = raw_js_options!(options, raw_js_options);
        let options = match serde_wasm_bindgen::to_value(&options) {
            Ok(options) => options,
            Err(error) => {
                return Box::pin(async move {
                    Err(Error::AdapterFailure(AdapterError::contract(
                        error.to_string(),
                    )))
                });
            }
        };
        let publication = Rc::new(LocalVideoTrackPublicationInner {
            lifecycle: Cell::new(LocalVideoTrackPublicationLifecycle::Publishing),
            muted: Cell::new(self.inner.muted.get()),
            sid: RefCell::new(None),
            session: Rc::downgrade(&session.inner),
            track: Rc::downgrade(&self.inner),
            publication_id: self.inner.allocate_publication_id(),
            track_id: self.inner.track_id,
            room_id: session.inner.room_id,
        });
        if is_replacement {
            self.inner
                .replacement_publication
                .borrow_mut()
                .replace(Rc::clone(&publication));
        } else {
            self.inner
                .publication
                .borrow_mut()
                .replace(Rc::clone(&publication));
        }

        let callback_track = Rc::clone(&self.inner);
        let callback_publication = Rc::clone(&publication);
        let callback = wasm_bindgen::closure::Closure::once_into_js(
            move |success: bool, value: wasm_bindgen::JsValue| {
                if success {
                    if let Some(sid) = value.as_string() {
                        callback_track.clear_sdk_replacement_source();
                        if is_replacement {
                            callback_track.promote_replacement_publication(&callback_publication);
                        }
                        callback_publication.sid.borrow_mut().replace(sid);
                        if callback_publication.lifecycle.get()
                            == LocalVideoTrackPublicationLifecycle::Publishing
                        {
                            callback_publication
                                .lifecycle
                                .set(LocalVideoTrackPublicationLifecycle::Published);
                        }
                        return;
                    }
                }
                callback_publication
                    .lifecycle
                    .set(LocalVideoTrackPublicationLifecycle::Unpublished);
                callback_track.clear_publication(&callback_publication);
            },
        );
        let callback: js_sys::Function = callback.unchecked_into();
        let replacement_callback: &js_sys::Function = self
            .inner
            .publication_replacement_callback()
            .as_ref()
            .unchecked_ref();
        let operation = adapter::publish_local_video_track(
            session.inner.room_id,
            self.inner.track_id,
            publication.publication_id,
            options,
            raw_options,
            replacement_callback,
            &callback,
        )
        .map_err(AdapterError::from_cause)
        .map_err(classify_publisher_mutation_error);
        let operation = match operation {
            Ok(operation) => operation,
            Err(error) => {
                publication
                    .lifecycle
                    .set(LocalVideoTrackPublicationLifecycle::Unpublished);
                self.inner.clear_publication(&publication);
                return Box::pin(async move { Err(error) });
            }
        };

        Box::pin(async move {
            let sid = wasm_bindgen_futures::JsFuture::from(operation)
                .await
                .map_err(AdapterError::from_cause)
                .map_err(classify_publisher_mutation_error)?
                .as_string()
                .ok_or_else(|| {
                    Error::AdapterFailure(AdapterError::contract(
                        "publication returned an invalid SID".into(),
                    ))
                })?;
            if publication.sid.borrow().as_deref() != Some(sid.as_str()) {
                return Err(Error::AdapterFailure(AdapterError::contract(
                    "publication completion did not install its SID".into(),
                )));
            }
            Ok(LocalVideoTrackPublication { inner: publication })
        })
    }

    #[cfg(test)]
    fn test_track(stop_count: Rc<Cell<usize>>) -> Self {
        Self::test_track_with_source(stop_count, TrackSource::Camera)
    }

    #[cfg(test)]
    fn test_track_with_source(stop_count: Rc<Cell<usize>>, source: TrackSource) -> Self {
        Self {
            inner: Rc::new(LocalVideoTrackInner {
                lifecycle: Cell::new(LocalVideoTrackLifecycle::Active),
                source,
                end_state: LocalMediaTrackEndState::new(),
                muted: Cell::new(false),
                attachments: RefCell::new(Vec::new()),
                publication: RefCell::new(None),
                replacement_publication: RefCell::new(None),
                stop_count,
            }),
        }
    }
}

impl fmt::Debug for LocalVideoTrack {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalVideoTrack")
            .field("source", &self.source())
            .field("stopped", &self.is_stopped())
            .finish()
    }
}

struct LocalVideoTrackInner {
    lifecycle: Cell<LocalVideoTrackLifecycle>,
    source: TrackSource,
    end_state: Rc<LocalMediaTrackEndState>,
    #[cfg(any(target_arch = "wasm32", test))]
    muted: Cell<bool>,
    attachments: RefCell<Vec<Weak<VideoTrackAttachmentInner>>>,
    publication: RefCell<Option<Rc<LocalVideoTrackPublicationInner>>>,
    replacement_publication: RefCell<Option<Rc<LocalVideoTrackPublicationInner>>>,
    #[cfg(target_arch = "wasm32")]
    next_publication_id: Cell<u32>,
    #[cfg(target_arch = "wasm32")]
    publication_replacement_callback:
        OnceCell<wasm_bindgen::closure::Closure<dyn Fn(u32, Option<String>, bool) -> u32>>,
    #[cfg(target_arch = "wasm32")]
    sdk_replacement_source: RefCell<Option<Rc<LocalVideoTrackPublicationInner>>>,
    #[cfg(target_arch = "wasm32")]
    track_id: u32,
    #[cfg(target_arch = "wasm32")]
    stop_operation: RefCell<Option<Result<js_sys::Promise, AdapterError>>>,
    #[cfg(target_arch = "wasm32")]
    ended_callback: RefCell<Option<wasm_bindgen::closure::Closure<dyn Fn()>>>,
    #[cfg(test)]
    stop_count: Rc<Cell<usize>>,
}

impl LocalVideoTrackInner {
    fn remove_attachment(&self, target: *const VideoTrackAttachmentInner) {
        self.attachments
            .borrow_mut()
            .retain(|attachment| attachment.as_ptr() != target && attachment.strong_count() > 0);
    }

    #[cfg(target_arch = "wasm32")]
    fn allocate_publication_id(&self) -> u32 {
        let publication_id = self.next_publication_id.get();
        self.next_publication_id
            .set(publication_id.checked_add(1).unwrap_or(1));
        publication_id
    }

    fn clear_terminal_publication(&self) {
        #[cfg(target_arch = "wasm32")]
        if self
            .sdk_replacement_source
            .borrow()
            .as_ref()
            .is_some_and(|publication| {
                publication
                    .session
                    .upgrade()
                    .is_none_or(|session| session.lifecycle.get() != SessionLifecycle::Active)
            })
        {
            self.sdk_replacement_source.borrow_mut().take();
        }
        let replacement_terminal = self
            .replacement_publication
            .borrow()
            .as_ref()
            .is_some_and(|publication| publication.association_released());
        if replacement_terminal {
            if let Some(publication) = self.replacement_publication.borrow_mut().take() {
                publication
                    .lifecycle
                    .set(LocalVideoTrackPublicationLifecycle::Unpublished);
            }
        }

        let terminal = self
            .publication
            .borrow()
            .as_ref()
            .is_some_and(|publication| publication.association_released());
        if terminal {
            if let Some(publication) = self.publication.borrow_mut().take() {
                publication
                    .lifecycle
                    .set(LocalVideoTrackPublicationLifecycle::Unpublished);
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn reconcile_sdk_publication(
        self: &Rc<Self>,
        expected_publication_id: u32,
        sid: Option<String>,
        muted: bool,
    ) -> u32 {
        let previous = if let Some(sid) = sid.as_ref() {
            if sid.is_empty() || self.publication.borrow().is_some() {
                return 0;
            }
            let Some(previous) = self.sdk_replacement_source.borrow_mut().take() else {
                return 0;
            };
            if previous.publication_id != expected_publication_id
                || previous
                    .session
                    .upgrade()
                    .is_none_or(|session| session.lifecycle.get() != SessionLifecycle::Active)
            {
                self.sdk_replacement_source.borrow_mut().replace(previous);
                return 0;
            }
            previous
        } else {
            let Some(previous) = self.publication.borrow_mut().take() else {
                return 0;
            };
            if previous.publication_id != expected_publication_id || previous.is_terminal() {
                self.publication.borrow_mut().replace(previous);
                return 0;
            }
            previous
                .lifecycle
                .set(LocalVideoTrackPublicationLifecycle::Unpublished);
            self.sdk_replacement_source.borrow_mut().replace(previous);
            return 0;
        };
        let publication_id = self.allocate_publication_id();
        let replacement = Rc::new(LocalVideoTrackPublicationInner {
            lifecycle: Cell::new(LocalVideoTrackPublicationLifecycle::Published),
            muted: Cell::new(muted),
            sid: RefCell::new(sid),
            session: previous.session.clone(),
            track: Rc::downgrade(self),
            publication_id,
            track_id: previous.track_id,
            room_id: previous.room_id,
        });
        self.publication.borrow_mut().replace(replacement);
        publication_id
    }

    #[cfg(target_arch = "wasm32")]
    fn clear_sdk_replacement_source(&self) {
        self.sdk_replacement_source.borrow_mut().take();
    }

    #[cfg(target_arch = "wasm32")]
    fn publication_replacement_callback(
        self: &Rc<Self>,
    ) -> &wasm_bindgen::closure::Closure<dyn Fn(u32, Option<String>, bool) -> u32> {
        self.publication_replacement_callback.get_or_init(|| {
            let track = Rc::downgrade(self);
            wasm_bindgen::closure::Closure::wrap(Box::new(move |publication_id, sid, muted| {
                track
                    .upgrade()
                    .map(|track| track.reconcile_sdk_publication(publication_id, sid, muted))
                    .unwrap_or(0)
            })
                as Box<dyn Fn(u32, Option<String>, bool) -> u32>)
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn clear_publication(&self, publication: &Rc<LocalVideoTrackPublicationInner>) {
        let pending_matches = self
            .replacement_publication
            .borrow()
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, publication));
        if pending_matches {
            self.replacement_publication.borrow_mut().take();
        }
        let matches = self
            .publication
            .borrow()
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, publication));
        if matches {
            self.publication.borrow_mut().take();
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn promote_replacement_publication(&self, publication: &Rc<LocalVideoTrackPublicationInner>) {
        let matches = self
            .replacement_publication
            .borrow()
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, publication));
        if matches {
            self.replacement_publication.borrow_mut().take();
            self.publication
                .borrow_mut()
                .replace(Rc::clone(publication));
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn start_stop(self: &Rc<Self>) {
        let operation =
            adapter::stop_local_video_track(self.track_id).map_err(AdapterError::from_cause);
        self.stop_operation.borrow_mut().replace(operation);
    }

    #[cfg(target_arch = "wasm32")]
    async fn await_stop(&self) -> Result<(), AdapterError> {
        let operation = self.stop_operation.borrow().clone();
        let Some(operation) = operation else {
            return Ok(());
        };
        let result = match operation {
            Ok(operation) => wasm_bindgen_futures::JsFuture::from(operation)
                .await
                .map(|_| ())
                .map_err(AdapterError::from_cause),
            Err(error) => Err(error),
        };
        if result.is_err() {
            self.stop_operation.borrow_mut().take();
        }
        result
    }

    fn mark_stopped(&self) -> bool {
        if self.lifecycle.replace(LocalVideoTrackLifecycle::Stopped)
            == LocalVideoTrackLifecycle::Stopped
        {
            return false;
        }

        self.end_state.finish(LocalMediaTrackEndReason::Stopped);
        for attachment in self.attachments.borrow_mut().drain(..) {
            if let Some(attachment) = attachment.upgrade() {
                attachment.active.set(false);
            }
        }
        #[cfg(test)]
        self.stop_count.set(self.stop_count.get() + 1);
        true
    }

    #[cfg(any(target_arch = "wasm32", test))]
    fn mark_browser_ended(&self) {
        if self.lifecycle.replace(LocalVideoTrackLifecycle::Stopped)
            == LocalVideoTrackLifecycle::Stopped
        {
            return;
        }
        self.end_state
            .finish(LocalMediaTrackEndReason::BrowserEnded);
        for attachment in self.attachments.borrow_mut().drain(..) {
            if let Some(attachment) = attachment.upgrade() {
                attachment.active.set(false);
            }
        }
        if let Some(publication) = self.replacement_publication.borrow_mut().take() {
            publication
                .lifecycle
                .set(LocalVideoTrackPublicationLifecycle::Unpublished);
        }
        if let Some(publication) = self.publication.borrow_mut().take() {
            publication
                .lifecycle
                .set(LocalVideoTrackPublicationLifecycle::Unpublished);
        }
    }
}

impl Drop for LocalVideoTrackInner {
    fn drop(&mut self) {
        self.mark_stopped();
        #[cfg(target_arch = "wasm32")]
        adapter::drop_local_video_track(self.track_id);
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LocalVideoTrackLifecycle {
    Active,
    Stopped,
}

/// A typed local-video preview attachment with explicit and drop cleanup.
pub struct VideoTrackAttachment {
    inner: Rc<VideoTrackAttachmentInner>,
}

impl VideoTrackAttachment {
    /// Whether this guard still owns an active browser element attachment.
    pub fn is_active(&self) -> bool {
        self.inner.active.get()
            && self
                .inner
                .track
                .upgrade()
                .is_some_and(|track| track.lifecycle.get() == LocalVideoTrackLifecycle::Active)
    }

    /// Detaches the video element without stopping or publishing capture.
    pub fn close(&self) -> Result<(), Error> {
        if !self.inner.active.get() {
            return Ok(());
        }

        #[cfg(target_arch = "wasm32")]
        adapter::detach_local_video_track(self.inner.track_id, self.inner.attachment_id)
            .map_err(AdapterError::from_cause)
            .map_err(Error::MediaControlFailed)?;

        self.inner.active.set(false);
        if let Some(track) = self.inner.track.upgrade() {
            track.remove_attachment(Rc::as_ptr(&self.inner));
        }
        Ok(())
    }

    #[cfg(test)]
    fn test_attachment(track: &LocalVideoTrack) -> Self {
        let inner = Rc::new(VideoTrackAttachmentInner {
            active: Cell::new(true),
            track: Rc::downgrade(&track.inner),
        });
        track
            .inner
            .attachments
            .borrow_mut()
            .push(Rc::downgrade(&inner));
        Self { inner }
    }
}

impl fmt::Debug for VideoTrackAttachment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VideoTrackAttachment")
            .field("active", &self.is_active())
            .finish()
    }
}

struct VideoTrackAttachmentInner {
    active: Cell<bool>,
    track: Weak<LocalVideoTrackInner>,
    #[cfg(target_arch = "wasm32")]
    track_id: u32,
    #[cfg(target_arch = "wasm32")]
    attachment_id: u32,
}

impl Drop for VideoTrackAttachmentInner {
    fn drop(&mut self) {
        if self.active.replace(false) {
            #[cfg(target_arch = "wasm32")]
            adapter::drop_local_video_track_attachment(self.track_id, self.attachment_id);
        }
        if let Some(track) = self.track.upgrade() {
            track.remove_attachment(self as *const _);
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn parse_local_video_track_id(value: wasm_bindgen::JsValue) -> Result<u32, AdapterError> {
    value
        .as_f64()
        .filter(|id| id.is_finite() && id.fract() == 0.0 && *id > 0.0)
        .map(|id| id as u32)
        .ok_or_else(|| {
            AdapterError::contract("camera capture returned an invalid Local Video Track ID".into())
        })
}

#[cfg(target_arch = "wasm32")]
struct BrowserCaptureState<T> {
    result: RefCell<Option<Result<T, AdapterError>>>,
    abandoned: Cell<bool>,
    waker: RefCell<Option<Waker>>,
}

#[cfg(target_arch = "wasm32")]
impl<T> BrowserCaptureState<T> {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            result: RefCell::new(None),
            abandoned: Cell::new(false),
            waker: RefCell::new(None),
        })
    }
}

#[cfg(target_arch = "wasm32")]
struct BrowserCaptureFuture<T, O> {
    state: Rc<BrowserCaptureState<T>>,
    finish: fn(Result<T, AdapterError>) -> Result<O, Error>,
    cleanup: fn(T),
}

#[cfg(target_arch = "wasm32")]
impl<T, O> Future for BrowserCaptureFuture<T, O> {
    type Output = Result<O, Error>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.as_ref().get_ref();
        let Some(result) = this.state.result.borrow_mut().take() else {
            this.state
                .waker
                .borrow_mut()
                .replace(context.waker().clone());
            return Poll::Pending;
        };
        Poll::Ready((this.finish)(result))
    }
}

#[cfg(target_arch = "wasm32")]
impl<T, O> Drop for BrowserCaptureFuture<T, O> {
    fn drop(&mut self) {
        if let Some(Ok(value)) = self.state.result.borrow_mut().take() {
            (self.cleanup)(value);
        } else {
            self.state.abandoned.set(true);
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn complete_browser_capture<T>(
    state: &BrowserCaptureState<T>,
    result: Result<T, AdapterError>,
    cleanup: fn(T),
) {
    if state.abandoned.get() {
        if let Ok(value) = result {
            cleanup(value);
        }
        return;
    }
    state.result.borrow_mut().replace(result);
    if let Some(waker) = state.waker.borrow_mut().take() {
        waker.wake();
    }
}

#[cfg(target_arch = "wasm32")]
fn finish_camera_capture(result: Result<u32, AdapterError>) -> Result<LocalVideoTrack, Error> {
    result
        .map(|track_id| create_local_video_track(track_id, TrackSource::Camera))
        .map_err(classify_capture_error)
}

#[cfg(target_arch = "wasm32")]
fn drop_captured_video(track_id: u32) {
    adapter::drop_local_video_track(track_id);
}

#[cfg(target_arch = "wasm32")]
fn create_local_video_track(track_id: u32, source: TrackSource) -> LocalVideoTrack {
    LocalVideoTrack {
        inner: Rc::new(LocalVideoTrackInner {
            lifecycle: Cell::new(LocalVideoTrackLifecycle::Active),
            source,
            end_state: LocalMediaTrackEndState::new(),
            muted: Cell::new(false),
            attachments: RefCell::new(Vec::new()),
            publication: RefCell::new(None),
            replacement_publication: RefCell::new(None),
            next_publication_id: Cell::new(1),
            publication_replacement_callback: OnceCell::new(),
            sdk_replacement_source: RefCell::new(None),
            track_id,
            stop_operation: RefCell::new(None),
            ended_callback: RefCell::new(None),
        }),
    }
}

#[cfg(target_arch = "wasm32")]
fn capture_camera_video_track(
    options: CameraCaptureOptions,
) -> Pin<Box<dyn Future<Output = Result<LocalVideoTrack, Error>> + 'static>> {
    use wasm_bindgen::JsCast;

    let state = BrowserCaptureState::new();
    let callback_state = Rc::clone(&state);
    let callback = wasm_bindgen::closure::Closure::once_into_js(
        move |success: bool, value: wasm_bindgen::JsValue| {
            let result = if success {
                parse_local_video_track_id(value)
            } else {
                Err(AdapterError::from_cause(value))
            };
            complete_browser_capture(&callback_state, result, drop_captured_video);
        },
    );
    let callback: js_sys::Function = callback.unchecked_into();
    let raw_options = raw_js_options!(options, raw_js_options);
    let options = match serde_wasm_bindgen::to_value(&options) {
        Ok(options) => options,
        Err(error) => {
            state
                .result
                .borrow_mut()
                .replace(Err(AdapterError::contract(error.to_string())));
            return Box::pin(BrowserCaptureFuture {
                state,
                finish: finish_camera_capture,
                cleanup: drop_captured_video,
            });
        }
    };
    if let Err(cause) = adapter::create_camera_video_track(options, raw_options, &callback) {
        state
            .result
            .borrow_mut()
            .replace(Err(AdapterError::from_cause(cause)));
    }

    Box::pin(BrowserCaptureFuture {
        state,
        finish: finish_camera_capture,
        cleanup: drop_captured_video,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn capture_camera_video_track(
    _options: CameraCaptureOptions,
) -> Pin<Box<dyn Future<Output = Result<LocalVideoTrack, Error>> + 'static>> {
    Box::pin(async { Err(Error::UnsupportedTarget) })
}

/// A browser-thread-local command handle for one Local Video Track Publication.
#[derive(Clone)]
pub struct LocalVideoTrackPublication {
    inner: Rc<LocalVideoTrackPublicationInner>,
}

impl LocalVideoTrackPublication {
    /// Returns the LiveKit publication SID after publishing completes.
    pub fn sid(&self) -> Option<String> {
        self.inner.sid.borrow().clone()
    }

    /// Whether the publication has been muted without being removed.
    pub fn is_muted(&self) -> bool {
        self.inner.muted.get()
    }

    /// Whether this command handle can no longer control its original publication.
    pub fn is_terminal(&self) -> bool {
        self.inner.is_terminal()
    }

    /// Returns the unstable `livekit-client` Local Track Publication object.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_publication(&self) -> Result<wasm_bindgen::JsValue, Error> {
        if self.is_terminal() || self.sid().is_none() {
            return Err(Error::InvalidState {
                operation: "access raw Local Video Track Publication",
                reason: "Track Publication is pending or terminal",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        raw_js_handle(adapter::raw_local_publication(
            self.inner.track_id,
            self.inner.publication_id,
            "video",
        ))
    }

    /// Returns the browser's unstable raw `RTCStatsReport` for this publication.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_rtc_stats_report(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<wasm_bindgen::JsValue>, Error>> + 'static>> {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "query raw Local Video Track RTC statistics",
                    reason: "Track Publication is terminal",
                })
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });
        #[cfg(target_arch = "wasm32")]
        raw_js_statistics(adapter::raw_local_rtc_stats_report(
            self.inner.track_id,
            self.inner.publication_id,
            "video",
        ))
    }

    /// Queries one normalized sender-statistics sample per active video encoding.
    pub fn sender_statistics(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<VideoSenderStatistics>, Error>> + 'static>> {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "query Local Video Track sender statistics",
                    reason: "Track Publication is terminal",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            deserialize_statistics(adapter::local_video_sender_statistics(
                self.inner.track_id,
                self.inner.publication_id,
            ))
        }
    }

    /// Mutes or unmutes while keeping the Track Publication alive.
    pub fn set_muted(
        &self,
        _muted: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if !_muted && self.inner.capture_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "unmute Track Publication",
                    reason: "Local Video Track capture has stopped",
                })
            });
        }
        if let Err(error) = self.inner.begin_mute_control() {
            return Box::pin(async move { Err(error) });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;

            let publication = Rc::clone(&self.inner);
            let callback_publication = Rc::clone(&publication);
            let muted = _muted;
            let callback = wasm_bindgen::closure::Closure::once_into_js(move |success: bool| {
                if success {
                    callback_publication.muted.set(muted);
                    if let Some(track) = callback_publication.track.upgrade() {
                        track.muted.set(muted);
                    }
                }
            });
            let callback: js_sys::Function = callback.unchecked_into();
            let operation = adapter::set_local_video_track_muted(
                publication.track_id,
                publication.publication_id,
                _muted,
                &callback,
            )
            .map_err(AdapterError::from_cause)
            .map_err(classify_publisher_mutation_error);
            let operation = match operation {
                Ok(operation) => operation,
                Err(error) => return Box::pin(async move { Err(error) }),
            };
            Box::pin(async move {
                wasm_bindgen_futures::JsFuture::from(operation)
                    .await
                    .map_err(AdapterError::from_cause)
                    .map_err(classify_publisher_mutation_error)?;
                Ok(())
            })
        }
    }

    /// Removes the Track Publication while keeping capture and attachments alive.
    pub fn unpublish(&self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        let _previous_lifecycle = match self.inner.begin_unpublish() {
            Ok(previous) => previous,
            Err(error) => return Box::pin(async move { Err(error) }),
        };

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;

            let publication = Rc::clone(&self.inner);
            let callback_track = publication.track.clone();
            let callback_publication = Rc::clone(&publication);
            let callback = wasm_bindgen::closure::Closure::once_into_js(move |success: bool| {
                if success {
                    callback_publication
                        .lifecycle
                        .set(LocalVideoTrackPublicationLifecycle::Unpublished);
                    if let Some(track) = callback_track.upgrade() {
                        track.clear_publication(&callback_publication);
                    }
                } else if callback_publication.lifecycle.get()
                    != LocalVideoTrackPublicationLifecycle::Unpublished
                {
                    callback_publication
                        .lifecycle
                        .set(LocalVideoTrackPublicationLifecycle::Published);
                }
            });
            let callback: js_sys::Function = callback.unchecked_into();
            let operation = adapter::unpublish_local_video_track(
                publication.room_id,
                publication.track_id,
                publication.publication_id,
                &callback,
            )
            .map_err(AdapterError::from_cause)
            .map_err(classify_publisher_mutation_error);
            let operation = match operation {
                Ok(operation) => operation,
                Err(error) => {
                    publication.lifecycle.set(_previous_lifecycle);
                    return Box::pin(async move { Err(error) });
                }
            };
            Box::pin(async move {
                wasm_bindgen_futures::JsFuture::from(operation)
                    .await
                    .map_err(AdapterError::from_cause)
                    .map_err(classify_publisher_mutation_error)?;
                Ok(())
            })
        }
    }
}

impl fmt::Debug for LocalVideoTrackPublication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalVideoTrackPublication")
            .field("sid", &self.sid())
            .field("muted", &self.is_muted())
            .field("terminal", &self.is_terminal())
            .finish()
    }
}

struct LocalVideoTrackPublicationInner {
    lifecycle: Cell<LocalVideoTrackPublicationLifecycle>,
    muted: Cell<bool>,
    sid: RefCell<Option<String>>,
    session: Weak<SessionInner>,
    track: Weak<LocalVideoTrackInner>,
    #[cfg(target_arch = "wasm32")]
    publication_id: u32,
    #[cfg(target_arch = "wasm32")]
    track_id: u32,
    #[cfg(target_arch = "wasm32")]
    room_id: u32,
}

impl LocalVideoTrackPublicationInner {
    fn capture_stopped(&self) -> bool {
        self.track
            .upgrade()
            .is_none_or(|track| track.lifecycle.get() == LocalVideoTrackLifecycle::Stopped)
    }

    fn is_terminal(&self) -> bool {
        self.lifecycle.get() == LocalVideoTrackPublicationLifecycle::Unpublished
            || self
                .session
                .upgrade()
                .is_none_or(|session| session.lifecycle.get() != SessionLifecycle::Active)
    }

    fn association_released(&self) -> bool {
        self.lifecycle.get() == LocalVideoTrackPublicationLifecycle::Unpublished
            || self
                .session
                .upgrade()
                .is_none_or(|session| session.lifecycle.get() == SessionLifecycle::Left)
    }

    fn begin_mute_control(&self) -> Result<(), Error> {
        if self.is_terminal() {
            return Err(Error::InvalidState {
                operation: "change Track Publication mute",
                reason: "Track Publication is terminal",
            });
        }
        Ok(())
    }

    fn begin_unpublish(&self) -> Result<LocalVideoTrackPublicationLifecycle, Error> {
        if self.is_terminal()
            || self.lifecycle.get() == LocalVideoTrackPublicationLifecycle::Unpublishing
        {
            return Err(Error::InvalidState {
                operation: "unpublish Local Video Track",
                reason: "Track Publication is terminal or being unpublished",
            });
        }
        let previous = self
            .lifecycle
            .replace(LocalVideoTrackPublicationLifecycle::Unpublishing);
        Ok(previous)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LocalVideoTrackPublicationLifecycle {
    #[cfg(target_arch = "wasm32")]
    Publishing,
    #[cfg(any(target_arch = "wasm32", test))]
    Published,
    Unpublishing,
    Unpublished,
}

/// One owned level and waveform sample from an [`AudioAnalyser`].
#[derive(Clone, Debug, PartialEq)]
pub struct AudioAnalysis {
    level: f32,
    waveform: Vec<f32>,
}

impl AudioAnalysis {
    #[cfg(any(target_arch = "wasm32", test))]
    fn from_waveform(waveform: Vec<f32>) -> Self {
        let mean_square = if waveform.is_empty() {
            0.0
        } else {
            waveform
                .iter()
                .map(|sample| f64::from(*sample) * f64::from(*sample))
                .sum::<f64>()
                / waveform.len() as f64
        };
        Self {
            level: mean_square.sqrt().clamp(0.0, 1.0) as f32,
            waveform,
        }
    }

    /// Returns the root-mean-square level normalized to `0.0..=1.0`.
    pub fn level(&self) -> f32 {
        self.level
    }

    /// Returns normalized time-domain samples owned by this Rust frame.
    pub fn waveform(&self) -> &[f32] {
        &self.waveform
    }

    /// Consumes this frame and returns its owned normalized time-domain samples.
    pub fn into_waveform(self) -> Vec<f32> {
        self.waveform
    }
}

/// A browser-thread-local Web Audio analyser for one exact Local or Remote Media Track source.
///
/// Clones share one Web Audio graph. Explicit close is terminal for every clone, and dropping the
/// final clone releases the graph as a fallback. Ending or replacing the source also terminates the
/// analyser.
#[derive(Clone)]
pub struct AudioAnalyser {
    inner: Rc<AudioAnalyserInner>,
}

impl AudioAnalyser {
    /// Whether this analyser and its exact source track remain available.
    pub fn is_active(&self) -> bool {
        self.inner.state.active.get() && self.inner.source.is_active()
    }

    /// Reads one owned Rust frame containing level and waveform data.
    pub fn read(&self) -> Result<AudioAnalysis, Error> {
        if !self.is_active() {
            self.inner.state.active.set(false);
            #[cfg(target_arch = "wasm32")]
            adapter::drop_audio_analyser(self.inner.analyser_id);
            return Err(Error::InvalidState {
                operation: "read Audio Analyser",
                reason: "Audio Analyser or source track is terminal",
            });
        }

        #[cfg(target_arch = "wasm32")]
        {
            let samples = adapter::read_audio_analyser(self.inner.analyser_id)
                .map_err(AdapterError::from_cause)
                .map_err(Error::AudioAnalysisFailed)?;
            return Ok(AudioAnalysis::from_waveform(samples.to_vec()));
        }

        #[cfg(all(not(target_arch = "wasm32"), test))]
        return Ok(AudioAnalysis::from_waveform(vec![0.0]));

        #[cfg(all(not(target_arch = "wasm32"), not(test)))]
        Err(Error::UnsupportedTarget)
    }

    /// Makes every clone terminal and releases its Web Audio graph.
    ///
    /// Terminal state is installed when this method is called. Await the returned local future for
    /// deterministic `AudioContext` closure.
    pub fn close(&self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        let newly_closed = self.inner.state.active.replace(false);
        #[cfg(target_arch = "wasm32")]
        if newly_closed {
            self.inner.start_close();
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = newly_closed;

        let _inner = Rc::clone(&self.inner);
        Box::pin(async move {
            #[cfg(target_arch = "wasm32")]
            _inner
                .await_close()
                .await
                .map_err(Error::AudioAnalysisFailed)?;
            Ok(())
        })
    }

    #[cfg(all(not(target_arch = "wasm32"), test))]
    fn test_analyser(source: AudioAnalyserSource) -> Self {
        let state = Rc::new(AudioAnalyserState {
            active: Cell::new(true),
        });
        source.register(&state);
        Self {
            inner: Rc::new(AudioAnalyserInner { state, source }),
        }
    }
}

impl fmt::Debug for AudioAnalyser {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AudioAnalyser")
            .field("active", &self.is_active())
            .finish()
    }
}

struct AudioAnalyserState {
    active: Cell<bool>,
}

#[cfg(any(target_arch = "wasm32", test))]
fn register_audio_analyser(
    analysers: &RefCell<Vec<Weak<AudioAnalyserState>>>,
    state: &Rc<AudioAnalyserState>,
) {
    analysers
        .borrow_mut()
        .retain(|analyser| analyser.strong_count() > 0);
    analysers.borrow_mut().push(Rc::downgrade(state));
}

fn terminate_audio_analysers(analysers: &RefCell<Vec<Weak<AudioAnalyserState>>>) {
    for analyser in analysers.borrow_mut().drain(..) {
        if let Some(analyser) = analyser.upgrade() {
            analyser.active.set(false);
        }
    }
}

enum AudioAnalyserSource {
    Local(Weak<LocalAudioTrackInner>),
    Remote(Weak<RemoteMediaTrackInner>),
}

impl AudioAnalyserSource {
    fn is_active(&self) -> bool {
        match self {
            Self::Local(track) => track
                .upgrade()
                .is_some_and(|track| track.lifecycle.get() == LocalAudioTrackLifecycle::Active),
            Self::Remote(track) => track.upgrade().is_some_and(|track| !track.is_terminal()),
        }
    }

    #[cfg(any(target_arch = "wasm32", test))]
    fn register(&self, state: &Rc<AudioAnalyserState>) {
        match self {
            Self::Local(track) => {
                if let Some(track) = track.upgrade() {
                    register_audio_analyser(&track.analysers, state);
                }
            }
            Self::Remote(track) => {
                if let Some(track) = track.upgrade() {
                    register_audio_analyser(&track.analysers, state);
                }
            }
        }
    }
}

struct AudioAnalyserInner {
    state: Rc<AudioAnalyserState>,
    source: AudioAnalyserSource,
    #[cfg(target_arch = "wasm32")]
    analyser_id: u32,
    #[cfg(target_arch = "wasm32")]
    close_operation: RefCell<Option<Result<js_sys::Promise, AdapterError>>>,
    #[cfg(target_arch = "wasm32")]
    _terminal_callback: wasm_bindgen::closure::Closure<dyn Fn()>,
}

#[cfg(target_arch = "wasm32")]
impl AudioAnalyserInner {
    fn start_close(&self) {
        let operation =
            adapter::close_audio_analyser(self.analyser_id).map_err(AdapterError::from_cause);
        self.close_operation.borrow_mut().replace(operation);
    }

    async fn await_close(&self) -> Result<(), AdapterError> {
        let operation = self.close_operation.borrow().clone();
        let Some(operation) = operation else {
            return Ok(());
        };
        match operation {
            Ok(operation) => wasm_bindgen_futures::JsFuture::from(operation)
                .await
                .map(|_| ())
                .map_err(AdapterError::from_cause),
            Err(error) => Err(error),
        }
    }
}

impl Drop for AudioAnalyserInner {
    fn drop(&mut self) {
        self.state.active.set(false);
        #[cfg(target_arch = "wasm32")]
        adapter::drop_audio_analyser(self.analyser_id);
    }
}

#[cfg(target_arch = "wasm32")]
fn create_audio_analyser(
    source: AudioAnalyserSource,
    create: impl FnOnce(&js_sys::Function) -> Result<u32, wasm_bindgen::JsValue>,
) -> Result<AudioAnalyser, Error> {
    use wasm_bindgen::JsCast;

    let state = Rc::new(AudioAnalyserState {
        active: Cell::new(true),
    });
    let weak_state = Rc::downgrade(&state);
    let callback = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
        if let Some(state) = weak_state.upgrade() {
            state.active.set(false);
        }
    }) as Box<dyn Fn()>);
    let analyser_id = create(callback.as_ref().unchecked_ref())
        .map_err(AdapterError::from_cause)
        .map_err(Error::AudioAnalysisFailed)?;
    source.register(&state);
    Ok(AudioAnalyser {
        inner: Rc::new(AudioAnalyserInner {
            state,
            source,
            analyser_id,
            close_operation: RefCell::new(None),
            _terminal_callback: callback,
        }),
    })
}

/// A browser-thread-local local audio capture owned independently of any Room Session.
///
/// Clones share one underlying browser capture. Explicit stop is terminal for every clone, and
/// dropping the final clone stops capture as a fallback.
#[derive(Clone)]
pub struct LocalAudioTrack {
    inner: Rc<LocalAudioTrackInner>,
}

impl PartialEq for LocalAudioTrack {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for LocalAudioTrack {}

impl LocalAudioTrack {
    /// Returns the authoritative source that created this track.
    pub fn source(&self) -> AudioTrackSource {
        self.inner.source
    }

    /// Whether capture has been stopped for every clone.
    pub fn is_stopped(&self) -> bool {
        self.inner.lifecycle.get() == LocalAudioTrackLifecycle::Stopped
    }

    /// Returns the unstable `livekit-client` Local Audio Track object.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_media_track(&self) -> Result<wasm_bindgen::JsValue, Error> {
        if self.is_stopped() {
            return Err(Error::InvalidState {
                operation: "access raw Local Audio Track",
                reason: "capture has stopped",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        raw_js_handle(adapter::raw_local_media_track(self.inner.track_id, "audio"))
    }

    /// Installs an unstable JavaScript audio processor through the track mutation queue.
    #[cfg(feature = "raw-js")]
    pub fn set_raw_js_processor(
        &self,
        _processor: wasm_bindgen::JsValue,
        _show_processed_stream_locally: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if self.is_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "set raw Local Audio Track processor",
                    reason: "capture has stopped",
                })
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });
        #[cfg(target_arch = "wasm32")]
        raw_js_processor_operation(adapter::set_local_media_track_processor(
            self.inner.track_id,
            "audio",
            _processor,
            _show_processed_stream_locally,
        ))
    }

    /// Returns the currently installed unstable JavaScript audio processor.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_processor(&self) -> Result<Option<wasm_bindgen::JsValue>, Error> {
        if self.is_stopped() {
            return Err(Error::InvalidState {
                operation: "access raw Local Audio Track processor",
                reason: "capture has stopped",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        {
            let value = adapter::raw_local_media_track_processor(self.inner.track_id, "audio")
                .map_err(AdapterError::from_cause)
                .map_err(Error::MediaControlFailed)?;
            Ok((!value.is_null() && !value.is_undefined()).then_some(value))
        }
    }

    /// Stops the currently installed unstable JavaScript audio processor.
    #[cfg(feature = "raw-js")]
    pub fn stop_raw_js_processor(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if self.is_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "stop raw Local Audio Track processor",
                    reason: "capture has stopped",
                })
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });
        #[cfg(target_arch = "wasm32")]
        raw_js_processor_operation(adapter::stop_local_media_track_processor(
            self.inner.track_id,
            "audio",
        ))
    }

    /// Returns the browser device ID currently producing this track.
    pub fn device_id(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, Error>> + 'static>> {
        if self.source() != AudioTrackSource::Microphone {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "inspect Local Audio Track device",
                    reason: "system audio has no selectable microphone device",
                })
            });
        }
        if self.is_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "inspect Local Audio Track device",
                    reason: "capture has stopped",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            local_media_device_id(
                adapter::local_audio_track_device_id(self.inner.track_id),
                "microphone device query returned a non-string ID",
            )
        }
    }

    /// Restarts this track with its currently committed microphone constraints.
    pub fn restart(&self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if let Err(error) = self.ensure_active_for_device_change("restart Local Audio Track") {
            return Box::pin(async move { Err(error) });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let inner = Rc::clone(&self.inner);
            start_local_media_device_change(
                move || {
                    inner.mark_stopped();
                },
                |callback| adapter::restart_local_audio_track(self.inner.track_id, callback),
            )
        }
    }

    /// Restarts this track with new typed microphone constraints.
    ///
    /// The existing Room Session, Track Publication, and mute state are kept. If applying the new
    /// constraints fails, the adapter restores the previously committed input.
    pub fn restart_with_options(
        &self,
        _options: MicrophoneCaptureOptions,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if let Err(error) = self.ensure_active_for_device_change("restart Local Audio Track") {
            return Box::pin(async move { Err(error) });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let raw_options = raw_js_options!(_options, raw_js_options);
            let options = match serde_wasm_bindgen::to_value(&_options) {
                Ok(options) => options,
                Err(error) => {
                    return Box::pin(async move {
                        Err(Error::AdapterFailure(AdapterError::contract(
                            error.to_string(),
                        )))
                    });
                }
            };
            let inner = Rc::clone(&self.inner);
            start_local_media_device_change(
                move || {
                    inner.mark_stopped();
                },
                |callback| {
                    adapter::restart_local_audio_track_with_options(
                        self.inner.track_id,
                        options,
                        raw_options,
                        callback,
                    )
                },
            )
        }
    }

    /// Switches this track to one exact microphone while retaining its other constraints.
    pub fn switch_device(
        &self,
        device_id: impl Into<String>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if let Err(error) = self.ensure_active_for_device_change("switch Local Audio Track device")
        {
            return Box::pin(async move { Err(error) });
        }
        let _device_id = device_id.into();

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let inner = Rc::clone(&self.inner);
            start_local_media_device_change(
                move || {
                    inner.mark_stopped();
                },
                |callback| {
                    adapter::switch_local_audio_track_device(
                        self.inner.track_id,
                        &_device_id,
                        callback,
                    )
                },
            )
        }
    }

    fn ensure_active_for_device_change(&self, operation: &'static str) -> Result<(), Error> {
        if self.source() != AudioTrackSource::Microphone {
            Err(Error::InvalidState {
                operation,
                reason: "system audio cannot restart or switch microphone devices",
            })
        } else if self.is_stopped() {
            Err(Error::InvalidState {
                operation,
                reason: "capture has stopped",
            })
        } else {
            Ok(())
        }
    }

    /// Returns why this capture ended, if it is already terminal.
    pub fn end_reason(&self) -> Option<LocalMediaTrackEndReason> {
        self.inner.end_state.reason.get()
    }

    /// Resolves when this capture becomes terminal.
    pub fn ended(&self) -> LocalMediaTrackEnded {
        LocalMediaTrackEnded {
            state: Rc::clone(&self.inner.end_state),
            waiter_id: None,
        }
    }

    /// Creates a typed level and waveform analyser for this exact audio source.
    pub fn create_analyser(&self) -> Result<AudioAnalyser, Error> {
        if self.is_stopped() {
            return Err(Error::InvalidState {
                operation: "create Local Media Track audio analyser",
                reason: "capture has stopped",
            });
        }

        let _source = AudioAnalyserSource::Local(Rc::downgrade(&self.inner));
        #[cfg(target_arch = "wasm32")]
        return create_audio_analyser(_source, |callback| {
            adapter::create_local_audio_analyser(self.inner.track_id, callback)
        });

        #[cfg(all(not(target_arch = "wasm32"), test))]
        return Ok(AudioAnalyser::test_analyser(_source));

        #[cfg(all(not(target_arch = "wasm32"), not(test)))]
        Err(Error::UnsupportedTarget)
    }

    /// Makes every clone terminal and stops the shared microphone capture.
    ///
    /// Terminal state is installed when this method is called. Await the returned local future for
    /// deterministic browser cleanup.
    pub fn stop(&self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        let _newly_stopped = self.inner.mark_stopped();
        #[cfg(target_arch = "wasm32")]
        if _newly_stopped || self.inner.stop_operation.borrow().is_none() {
            self.inner.start_stop();
        }

        let _inner = Rc::clone(&self.inner);
        Box::pin(async move {
            #[cfg(target_arch = "wasm32")]
            _inner
                .await_stop()
                .await
                .map_err(Error::MediaControlFailed)?;

            Ok(())
        })
    }

    /// Attaches this capture to one browser audio element without publishing it.
    pub fn attach(
        &self,
        _element: &web_sys::HtmlAudioElement,
    ) -> Result<AudioTrackAttachment, Error> {
        if self.is_stopped() {
            return Err(Error::InvalidState {
                operation: "attach Local Audio Track",
                reason: "capture has stopped",
            });
        }

        #[cfg(target_arch = "wasm32")]
        let attachment_id = adapter::attach_local_audio_track(self.inner.track_id, _element)
            .map_err(AdapterError::from_cause)
            .map_err(Error::MediaControlFailed)?;
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        {
            let inner = Rc::new(AudioTrackAttachmentInner {
                active: Cell::new(true),
                track: Rc::downgrade(&self.inner),
                track_id: self.inner.track_id,
                attachment_id,
            });
            self.inner
                .attachments
                .borrow_mut()
                .push(Rc::downgrade(&inner));
            Ok(AudioTrackAttachment { inner })
        }
    }

    /// Returns this track's current publication command handle, including an in-flight publish.
    ///
    /// This lets callers observe and recover a publication whose Rust operation future was
    /// abandoned even though its underlying JavaScript Promise continued.
    pub fn publication(&self) -> Option<LocalAudioTrackPublication> {
        self.inner.clear_terminal_publication();
        let inner = self
            .inner
            .replacement_publication
            .borrow()
            .as_ref()
            .cloned()
            .or_else(|| self.inner.publication.borrow().as_ref().cloned());
        inner.map(|inner| LocalAudioTrackPublication { inner })
    }

    /// Publishes this capture into one active Room Session.
    ///
    /// A Local Audio Track can have at most one active or in-flight publication association.
    pub fn publish(
        &self,
        session: &RoomSession,
        options: AudioPublishOptions,
    ) -> Pin<Box<dyn Future<Output = Result<LocalAudioTrackPublication, Error>> + 'static>> {
        if self.is_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "publish Local Audio Track",
                    reason: "capture has stopped",
                })
            });
        }
        if session.inner.lifecycle.get() != SessionLifecycle::Active {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "publish Local Audio Track",
                    reason: "Room Session is terminal",
                })
            });
        }
        if options.source != self.source() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "publish Local Audio Track",
                    reason: "publication source does not match captured media",
                })
            });
        }

        self.inner.clear_terminal_publication();
        let is_replacement = self
            .inner
            .publication
            .borrow()
            .as_ref()
            .map(|publication| publication.lifecycle.get())
            == Some(LocalAudioTrackPublicationLifecycle::Unpublishing);
        if self.inner.replacement_publication.borrow().is_some()
            || (self.inner.publication.borrow().is_some() && !is_replacement)
        {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "publish Local Audio Track",
                    reason: "track is already associated with a Room Session",
                })
            });
        }

        #[cfg(all(not(target_arch = "wasm32"), test))]
        {
            let publication = Rc::new(LocalAudioTrackPublicationInner {
                lifecycle: Cell::new(LocalAudioTrackPublicationLifecycle::Published),
                muted: Cell::new(self.inner.muted.get()),
                sid: RefCell::new(Some("TR_audio_test".into())),
                session: Rc::downgrade(&session.inner),
                track: Rc::downgrade(&self.inner),
            });
            self.inner
                .publication
                .borrow_mut()
                .replace(Rc::clone(&publication));
            Box::pin(async move { Ok(LocalAudioTrackPublication { inner: publication }) })
        }

        #[cfg(all(not(target_arch = "wasm32"), not(test)))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            self.start_publish(session, options, is_replacement)
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn start_publish(
        &self,
        session: &RoomSession,
        options: AudioPublishOptions,
        is_replacement: bool,
    ) -> Pin<Box<dyn Future<Output = Result<LocalAudioTrackPublication, Error>> + 'static>> {
        use wasm_bindgen::JsCast;

        let raw_options = raw_js_options!(options, raw_js_options);
        let options = match serde_wasm_bindgen::to_value(&options) {
            Ok(options) => options,
            Err(error) => {
                return Box::pin(async move {
                    Err(Error::AdapterFailure(AdapterError::contract(
                        error.to_string(),
                    )))
                });
            }
        };
        let publication = Rc::new(LocalAudioTrackPublicationInner {
            lifecycle: Cell::new(LocalAudioTrackPublicationLifecycle::Publishing),
            muted: Cell::new(self.inner.muted.get()),
            sid: RefCell::new(None),
            session: Rc::downgrade(&session.inner),
            track: Rc::downgrade(&self.inner),
            publication_id: self.inner.allocate_publication_id(),
            track_id: self.inner.track_id,
            room_id: session.inner.room_id,
        });
        if is_replacement {
            self.inner
                .replacement_publication
                .borrow_mut()
                .replace(Rc::clone(&publication));
        } else {
            self.inner
                .publication
                .borrow_mut()
                .replace(Rc::clone(&publication));
        }

        let callback_track = Rc::clone(&self.inner);
        let callback_publication = Rc::clone(&publication);
        let callback = wasm_bindgen::closure::Closure::once_into_js(
            move |success: bool, value: wasm_bindgen::JsValue| {
                if success {
                    if let Some(sid) = value.as_string() {
                        callback_track.clear_sdk_replacement_source();
                        if is_replacement {
                            callback_track.promote_replacement_publication(&callback_publication);
                        }
                        callback_publication.sid.borrow_mut().replace(sid);
                        if callback_publication.lifecycle.get()
                            == LocalAudioTrackPublicationLifecycle::Publishing
                        {
                            callback_publication
                                .lifecycle
                                .set(LocalAudioTrackPublicationLifecycle::Published);
                        }
                        return;
                    }
                }
                callback_publication
                    .lifecycle
                    .set(LocalAudioTrackPublicationLifecycle::Unpublished);
                callback_track.clear_publication(&callback_publication);
            },
        );
        let callback: js_sys::Function = callback.unchecked_into();
        let replacement_callback: &js_sys::Function = self
            .inner
            .publication_replacement_callback()
            .as_ref()
            .unchecked_ref();
        let operation = adapter::publish_local_audio_track(
            session.inner.room_id,
            self.inner.track_id,
            publication.publication_id,
            options,
            raw_options,
            replacement_callback,
            &callback,
        )
        .map_err(AdapterError::from_cause)
        .map_err(classify_publisher_mutation_error);
        let operation = match operation {
            Ok(operation) => operation,
            Err(error) => {
                publication
                    .lifecycle
                    .set(LocalAudioTrackPublicationLifecycle::Unpublished);
                self.inner.clear_publication(&publication);
                return Box::pin(async move { Err(error) });
            }
        };

        Box::pin(async move {
            let sid = wasm_bindgen_futures::JsFuture::from(operation)
                .await
                .map_err(AdapterError::from_cause)
                .map_err(classify_publisher_mutation_error)?
                .as_string()
                .ok_or_else(|| {
                    Error::AdapterFailure(AdapterError::contract(
                        "audio publication returned an invalid SID".into(),
                    ))
                })?;
            if publication.sid.borrow().as_deref() != Some(sid.as_str()) {
                return Err(Error::AdapterFailure(AdapterError::contract(
                    "audio publication completion did not install its SID".into(),
                )));
            }
            Ok(LocalAudioTrackPublication { inner: publication })
        })
    }

    #[cfg(test)]
    fn test_track(stop_count: Rc<Cell<usize>>) -> Self {
        Self::test_track_with_source(stop_count, AudioTrackSource::Microphone)
    }

    #[cfg(test)]
    fn test_track_with_source(stop_count: Rc<Cell<usize>>, source: AudioTrackSource) -> Self {
        Self {
            inner: Rc::new(LocalAudioTrackInner {
                lifecycle: Cell::new(LocalAudioTrackLifecycle::Active),
                source,
                end_state: LocalMediaTrackEndState::new(),
                muted: Cell::new(false),
                attachments: RefCell::new(Vec::new()),
                publication: RefCell::new(None),
                replacement_publication: RefCell::new(None),
                analysers: RefCell::new(Vec::new()),
                stop_count,
            }),
        }
    }
}

impl fmt::Debug for LocalAudioTrack {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalAudioTrack")
            .field("source", &self.source())
            .field("stopped", &self.is_stopped())
            .finish()
    }
}

struct LocalAudioTrackInner {
    lifecycle: Cell<LocalAudioTrackLifecycle>,
    source: AudioTrackSource,
    end_state: Rc<LocalMediaTrackEndState>,
    #[cfg(any(target_arch = "wasm32", test))]
    muted: Cell<bool>,
    attachments: RefCell<Vec<Weak<AudioTrackAttachmentInner>>>,
    publication: RefCell<Option<Rc<LocalAudioTrackPublicationInner>>>,
    replacement_publication: RefCell<Option<Rc<LocalAudioTrackPublicationInner>>>,
    analysers: RefCell<Vec<Weak<AudioAnalyserState>>>,
    #[cfg(target_arch = "wasm32")]
    next_publication_id: Cell<u32>,
    #[cfg(target_arch = "wasm32")]
    publication_replacement_callback:
        OnceCell<wasm_bindgen::closure::Closure<dyn Fn(u32, Option<String>, bool) -> u32>>,
    #[cfg(target_arch = "wasm32")]
    sdk_replacement_source: RefCell<Option<Rc<LocalAudioTrackPublicationInner>>>,
    #[cfg(target_arch = "wasm32")]
    track_id: u32,
    #[cfg(target_arch = "wasm32")]
    stop_operation: RefCell<Option<Result<js_sys::Promise, AdapterError>>>,
    #[cfg(target_arch = "wasm32")]
    ended_callback: RefCell<Option<wasm_bindgen::closure::Closure<dyn Fn()>>>,
    #[cfg(test)]
    stop_count: Rc<Cell<usize>>,
}

impl LocalAudioTrackInner {
    fn remove_attachment(&self, target: *const AudioTrackAttachmentInner) {
        self.attachments
            .borrow_mut()
            .retain(|attachment| attachment.as_ptr() != target && attachment.strong_count() > 0);
    }

    #[cfg(target_arch = "wasm32")]
    fn allocate_publication_id(&self) -> u32 {
        let publication_id = self.next_publication_id.get();
        self.next_publication_id
            .set(publication_id.checked_add(1).unwrap_or(1));
        publication_id
    }

    #[cfg(target_arch = "wasm32")]
    fn reconcile_sdk_publication(
        self: &Rc<Self>,
        expected_publication_id: u32,
        sid: Option<String>,
        muted: bool,
    ) -> u32 {
        let previous = if let Some(sid) = sid.as_ref() {
            if sid.is_empty() || self.publication.borrow().is_some() {
                return 0;
            }
            let Some(previous) = self.sdk_replacement_source.borrow_mut().take() else {
                return 0;
            };
            if previous.publication_id != expected_publication_id
                || previous
                    .session
                    .upgrade()
                    .is_none_or(|session| session.lifecycle.get() != SessionLifecycle::Active)
            {
                self.sdk_replacement_source.borrow_mut().replace(previous);
                return 0;
            }
            previous
        } else {
            let Some(previous) = self.publication.borrow_mut().take() else {
                return 0;
            };
            if previous.publication_id != expected_publication_id || previous.is_terminal() {
                self.publication.borrow_mut().replace(previous);
                return 0;
            }
            previous
                .lifecycle
                .set(LocalAudioTrackPublicationLifecycle::Unpublished);
            self.sdk_replacement_source.borrow_mut().replace(previous);
            return 0;
        };
        let publication_id = self.allocate_publication_id();
        let replacement = Rc::new(LocalAudioTrackPublicationInner {
            lifecycle: Cell::new(LocalAudioTrackPublicationLifecycle::Published),
            muted: Cell::new(muted),
            sid: RefCell::new(sid),
            session: previous.session.clone(),
            track: Rc::downgrade(self),
            publication_id,
            track_id: previous.track_id,
            room_id: previous.room_id,
        });
        self.publication.borrow_mut().replace(replacement);
        publication_id
    }

    #[cfg(target_arch = "wasm32")]
    fn clear_sdk_replacement_source(&self) {
        self.sdk_replacement_source.borrow_mut().take();
    }

    #[cfg(target_arch = "wasm32")]
    fn publication_replacement_callback(
        self: &Rc<Self>,
    ) -> &wasm_bindgen::closure::Closure<dyn Fn(u32, Option<String>, bool) -> u32> {
        self.publication_replacement_callback.get_or_init(|| {
            let track = Rc::downgrade(self);
            wasm_bindgen::closure::Closure::wrap(Box::new(move |publication_id, sid, muted| {
                track
                    .upgrade()
                    .map(|track| track.reconcile_sdk_publication(publication_id, sid, muted))
                    .unwrap_or(0)
            })
                as Box<dyn Fn(u32, Option<String>, bool) -> u32>)
        })
    }

    fn clear_terminal_publication(&self) {
        #[cfg(target_arch = "wasm32")]
        if self
            .sdk_replacement_source
            .borrow()
            .as_ref()
            .is_some_and(|publication| {
                publication
                    .session
                    .upgrade()
                    .is_none_or(|session| session.lifecycle.get() != SessionLifecycle::Active)
            })
        {
            self.sdk_replacement_source.borrow_mut().take();
        }
        let replacement_terminal = self
            .replacement_publication
            .borrow()
            .as_ref()
            .is_some_and(|publication| publication.association_released());
        if replacement_terminal {
            if let Some(publication) = self.replacement_publication.borrow_mut().take() {
                publication
                    .lifecycle
                    .set(LocalAudioTrackPublicationLifecycle::Unpublished);
            }
        }

        let terminal = self
            .publication
            .borrow()
            .as_ref()
            .is_some_and(|publication| publication.association_released());
        if terminal {
            if let Some(publication) = self.publication.borrow_mut().take() {
                publication
                    .lifecycle
                    .set(LocalAudioTrackPublicationLifecycle::Unpublished);
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn clear_publication(&self, publication: &Rc<LocalAudioTrackPublicationInner>) {
        let pending_matches = self
            .replacement_publication
            .borrow()
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, publication));
        if pending_matches {
            self.replacement_publication.borrow_mut().take();
        }
        let matches = self
            .publication
            .borrow()
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, publication));
        if matches {
            self.publication.borrow_mut().take();
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn promote_replacement_publication(&self, publication: &Rc<LocalAudioTrackPublicationInner>) {
        let matches = self
            .replacement_publication
            .borrow()
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, publication));
        if matches {
            self.replacement_publication.borrow_mut().take();
            self.publication
                .borrow_mut()
                .replace(Rc::clone(publication));
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn start_stop(self: &Rc<Self>) {
        let operation =
            adapter::stop_local_audio_track(self.track_id).map_err(AdapterError::from_cause);
        self.stop_operation.borrow_mut().replace(operation);
    }

    #[cfg(target_arch = "wasm32")]
    async fn await_stop(&self) -> Result<(), AdapterError> {
        let operation = self.stop_operation.borrow().clone();
        let Some(operation) = operation else {
            return Ok(());
        };
        let result = match operation {
            Ok(operation) => wasm_bindgen_futures::JsFuture::from(operation)
                .await
                .map(|_| ())
                .map_err(AdapterError::from_cause),
            Err(error) => Err(error),
        };
        if result.is_err() {
            self.stop_operation.borrow_mut().take();
        }
        result
    }

    fn mark_stopped(&self) -> bool {
        if self.lifecycle.replace(LocalAudioTrackLifecycle::Stopped)
            == LocalAudioTrackLifecycle::Stopped
        {
            return false;
        }

        self.end_state.finish(LocalMediaTrackEndReason::Stopped);
        terminate_audio_analysers(&self.analysers);
        for attachment in self.attachments.borrow_mut().drain(..) {
            if let Some(attachment) = attachment.upgrade() {
                attachment.active.set(false);
            }
        }
        #[cfg(test)]
        self.stop_count.set(self.stop_count.get() + 1);
        true
    }

    #[cfg(any(target_arch = "wasm32", test))]
    fn mark_browser_ended(&self) {
        if self.lifecycle.replace(LocalAudioTrackLifecycle::Stopped)
            == LocalAudioTrackLifecycle::Stopped
        {
            return;
        }
        self.end_state
            .finish(LocalMediaTrackEndReason::BrowserEnded);
        terminate_audio_analysers(&self.analysers);
        for attachment in self.attachments.borrow_mut().drain(..) {
            if let Some(attachment) = attachment.upgrade() {
                attachment.active.set(false);
            }
        }
        if let Some(publication) = self.replacement_publication.borrow_mut().take() {
            publication
                .lifecycle
                .set(LocalAudioTrackPublicationLifecycle::Unpublished);
        }
        if let Some(publication) = self.publication.borrow_mut().take() {
            publication
                .lifecycle
                .set(LocalAudioTrackPublicationLifecycle::Unpublished);
        }
    }
}

impl Drop for LocalAudioTrackInner {
    fn drop(&mut self) {
        self.mark_stopped();
        #[cfg(target_arch = "wasm32")]
        adapter::drop_local_audio_track(self.track_id);
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LocalAudioTrackLifecycle {
    Active,
    Stopped,
}

/// A typed local-audio preview attachment with explicit and drop cleanup.
pub struct AudioTrackAttachment {
    inner: Rc<AudioTrackAttachmentInner>,
}

impl AudioTrackAttachment {
    /// Whether this guard still owns an active browser element attachment.
    pub fn is_active(&self) -> bool {
        self.inner.active.get()
            && self
                .inner
                .track
                .upgrade()
                .is_some_and(|track| track.lifecycle.get() == LocalAudioTrackLifecycle::Active)
    }

    /// Detaches the audio element without stopping or publishing capture.
    pub fn close(&self) -> Result<(), Error> {
        if !self.inner.active.get() {
            return Ok(());
        }

        #[cfg(target_arch = "wasm32")]
        adapter::detach_local_audio_track(self.inner.track_id, self.inner.attachment_id)
            .map_err(AdapterError::from_cause)
            .map_err(Error::MediaControlFailed)?;

        self.inner.active.set(false);
        if let Some(track) = self.inner.track.upgrade() {
            track.remove_attachment(Rc::as_ptr(&self.inner));
        }
        Ok(())
    }

    #[cfg(test)]
    fn test_attachment(track: &LocalAudioTrack) -> Self {
        let inner = Rc::new(AudioTrackAttachmentInner {
            active: Cell::new(true),
            track: Rc::downgrade(&track.inner),
        });
        track
            .inner
            .attachments
            .borrow_mut()
            .push(Rc::downgrade(&inner));
        Self { inner }
    }
}

impl fmt::Debug for AudioTrackAttachment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AudioTrackAttachment")
            .field("active", &self.is_active())
            .finish()
    }
}

struct AudioTrackAttachmentInner {
    active: Cell<bool>,
    track: Weak<LocalAudioTrackInner>,
    #[cfg(target_arch = "wasm32")]
    track_id: u32,
    #[cfg(target_arch = "wasm32")]
    attachment_id: u32,
}

impl Drop for AudioTrackAttachmentInner {
    fn drop(&mut self) {
        if self.active.replace(false) {
            #[cfg(target_arch = "wasm32")]
            adapter::drop_local_audio_track_attachment(self.track_id, self.attachment_id);
        }
        if let Some(track) = self.track.upgrade() {
            track.remove_attachment(self as *const _);
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn parse_local_audio_track_id(value: wasm_bindgen::JsValue) -> Result<u32, AdapterError> {
    value
        .as_f64()
        .filter(|id| id.is_finite() && id.fract() == 0.0 && *id > 0.0)
        .map(|id| id as u32)
        .ok_or_else(|| {
            AdapterError::contract(
                "microphone capture returned an invalid Local Audio Track ID".into(),
            )
        })
}

#[cfg(target_arch = "wasm32")]
fn create_local_audio_track(track_id: u32, source: AudioTrackSource) -> LocalAudioTrack {
    LocalAudioTrack {
        inner: Rc::new(LocalAudioTrackInner {
            lifecycle: Cell::new(LocalAudioTrackLifecycle::Active),
            source,
            end_state: LocalMediaTrackEndState::new(),
            muted: Cell::new(false),
            attachments: RefCell::new(Vec::new()),
            publication: RefCell::new(None),
            replacement_publication: RefCell::new(None),
            analysers: RefCell::new(Vec::new()),
            next_publication_id: Cell::new(1),
            publication_replacement_callback: OnceCell::new(),
            sdk_replacement_source: RefCell::new(None),
            track_id,
            stop_operation: RefCell::new(None),
            ended_callback: RefCell::new(None),
        }),
    }
}

#[cfg(target_arch = "wasm32")]
fn finish_microphone_capture(result: Result<u32, AdapterError>) -> Result<LocalAudioTrack, Error> {
    result
        .map(|track_id| create_local_audio_track(track_id, AudioTrackSource::Microphone))
        .map_err(classify_capture_error)
}

#[cfg(target_arch = "wasm32")]
fn drop_captured_audio(track_id: u32) {
    adapter::drop_local_audio_track(track_id);
}

#[cfg(target_arch = "wasm32")]
fn capture_microphone_audio_track(
    options: MicrophoneCaptureOptions,
) -> Pin<Box<dyn Future<Output = Result<LocalAudioTrack, Error>> + 'static>> {
    use wasm_bindgen::JsCast;

    let state = BrowserCaptureState::new();
    let callback_state = Rc::clone(&state);
    let callback = wasm_bindgen::closure::Closure::once_into_js(
        move |success: bool, value: wasm_bindgen::JsValue| {
            let result = if success {
                parse_local_audio_track_id(value)
            } else {
                Err(AdapterError::from_cause(value))
            };
            complete_browser_capture(&callback_state, result, drop_captured_audio);
        },
    );
    let callback: js_sys::Function = callback.unchecked_into();
    let raw_options = raw_js_options!(options, raw_js_options);
    let options = match serde_wasm_bindgen::to_value(&options) {
        Ok(options) => options,
        Err(error) => {
            state
                .result
                .borrow_mut()
                .replace(Err(AdapterError::contract(error.to_string())));
            return Box::pin(BrowserCaptureFuture {
                state,
                finish: finish_microphone_capture,
                cleanup: drop_captured_audio,
            });
        }
    };
    if let Err(cause) = adapter::create_microphone_audio_track(options, raw_options, &callback) {
        state
            .result
            .borrow_mut()
            .replace(Err(AdapterError::from_cause(cause)));
    }

    Box::pin(BrowserCaptureFuture {
        state,
        finish: finish_microphone_capture,
        cleanup: drop_captured_audio,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn capture_microphone_audio_track(
    _options: MicrophoneCaptureOptions,
) -> Pin<Box<dyn Future<Output = Result<LocalAudioTrack, Error>> + 'static>> {
    Box::pin(async { Err(Error::UnsupportedTarget) })
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Copy, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
struct AdapterScreenCapture {
    video_track_id: u32,
    audio_track_id: Option<u32>,
}

#[cfg(target_arch = "wasm32")]
fn finish_screen_capture(
    result: Result<AdapterScreenCapture, AdapterError>,
) -> Result<ScreenCapture, Error> {
    match result {
        Ok(capture) => create_screen_capture(capture).map_err(Error::AdapterFailure),
        Err(error) => Err(classify_display_capture_error(error)),
    }
}

#[cfg(target_arch = "wasm32")]
fn drop_captured_screen(capture: AdapterScreenCapture) {
    adapter::drop_local_video_track(capture.video_track_id);
    if let Some(track_id) = capture.audio_track_id {
        adapter::drop_local_audio_track(track_id);
    }
}

#[cfg(target_arch = "wasm32")]
fn create_screen_capture(capture: AdapterScreenCapture) -> Result<ScreenCapture, AdapterError> {
    use wasm_bindgen::JsCast;

    let display_video = create_local_video_track(capture.video_track_id, TrackSource::ScreenShare);
    let video_inner = Rc::clone(&display_video.inner);
    let video_weak = Rc::downgrade(&video_inner);
    let video_callback = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
        if let Some(inner) = video_weak.upgrade() {
            inner.mark_browser_ended();
        }
    }) as Box<dyn Fn()>);
    adapter::observe_local_video_track_ended(
        capture.video_track_id,
        video_callback.as_ref().unchecked_ref(),
    )
    .map_err(AdapterError::from_cause)?;
    video_inner
        .ended_callback
        .borrow_mut()
        .replace(video_callback);

    let system_audio = capture
        .audio_track_id
        .map(|track_id| {
            let track = create_local_audio_track(track_id, AudioTrackSource::ScreenShareAudio);
            let audio_inner = Rc::clone(&track.inner);
            let audio_weak = Rc::downgrade(&audio_inner);
            let callback = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
                if let Some(inner) = audio_weak.upgrade() {
                    inner.mark_browser_ended();
                }
            }) as Box<dyn Fn()>);
            adapter::observe_local_audio_track_ended(track_id, callback.as_ref().unchecked_ref())
                .map_err(AdapterError::from_cause)?;
            audio_inner.ended_callback.borrow_mut().replace(callback);
            Ok(track)
        })
        .transpose()?;

    Ok(ScreenCapture {
        display_video,
        system_audio,
    })
}

#[cfg(target_arch = "wasm32")]
fn capture_screen(
    options: ScreenCaptureOptions,
) -> Pin<Box<dyn Future<Output = Result<ScreenCapture, Error>> + 'static>> {
    use wasm_bindgen::JsCast;

    let state = BrowserCaptureState::new();
    let callback_state = Rc::clone(&state);
    let callback = wasm_bindgen::closure::Closure::once_into_js(
        move |success: bool, value: wasm_bindgen::JsValue| {
            let result = if success {
                serde_wasm_bindgen::from_value::<AdapterScreenCapture>(value)
                    .map_err(|error| AdapterError::contract(error.to_string()))
            } else {
                Err(AdapterError::from_cause(value))
            };
            complete_browser_capture(&callback_state, result, drop_captured_screen);
        },
    );
    let callback: js_sys::Function = callback.unchecked_into();
    let raw_options = raw_js_options!(options, raw_js_options);
    let options = match serde_wasm_bindgen::to_value(&options) {
        Ok(options) => options,
        Err(error) => {
            state
                .result
                .borrow_mut()
                .replace(Err(AdapterError::contract(error.to_string())));
            return Box::pin(BrowserCaptureFuture {
                state,
                finish: finish_screen_capture,
                cleanup: drop_captured_screen,
            });
        }
    };
    if let Err(cause) = adapter::create_screen_capture(options, raw_options, &callback) {
        state
            .result
            .borrow_mut()
            .replace(Err(AdapterError::from_cause(cause)));
    }

    Box::pin(BrowserCaptureFuture {
        state,
        finish: finish_screen_capture,
        cleanup: drop_captured_screen,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn capture_screen(
    _options: ScreenCaptureOptions,
) -> Pin<Box<dyn Future<Output = Result<ScreenCapture, Error>> + 'static>> {
    Box::pin(async { Err(Error::UnsupportedTarget) })
}

/// A browser-thread-local command handle for one Local Audio Track Publication.
#[derive(Clone)]
pub struct LocalAudioTrackPublication {
    inner: Rc<LocalAudioTrackPublicationInner>,
}

impl LocalAudioTrackPublication {
    /// Returns the LiveKit publication SID after publishing completes.
    pub fn sid(&self) -> Option<String> {
        self.inner.sid.borrow().clone()
    }

    /// Whether the publication has been muted without being removed.
    pub fn is_muted(&self) -> bool {
        self.inner.muted.get()
    }

    /// Whether this command handle can no longer control its original publication.
    pub fn is_terminal(&self) -> bool {
        self.inner.is_terminal()
    }

    /// Returns the unstable `livekit-client` Local Track Publication object.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_publication(&self) -> Result<wasm_bindgen::JsValue, Error> {
        if self.is_terminal() || self.sid().is_none() {
            return Err(Error::InvalidState {
                operation: "access raw Local Audio Track Publication",
                reason: "Track Publication is pending or terminal",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        raw_js_handle(adapter::raw_local_publication(
            self.inner.track_id,
            self.inner.publication_id,
            "audio",
        ))
    }

    /// Returns the browser's unstable raw `RTCStatsReport` for this publication.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_rtc_stats_report(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<wasm_bindgen::JsValue>, Error>> + 'static>> {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "query raw Local Audio Track RTC statistics",
                    reason: "Track Publication is terminal",
                })
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });
        #[cfg(target_arch = "wasm32")]
        raw_js_statistics(adapter::raw_local_rtc_stats_report(
            self.inner.track_id,
            self.inner.publication_id,
            "audio",
        ))
    }

    /// Queries the latest normalized sender statistics without changing publication state.
    pub fn sender_statistics(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<AudioSenderStatistics>, Error>> + 'static>> {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "query Local Audio Track sender statistics",
                    reason: "Track Publication is terminal",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            deserialize_statistics(adapter::local_audio_sender_statistics(
                self.inner.track_id,
                self.inner.publication_id,
            ))
        }
    }

    /// Mutes or unmutes while keeping the Track Publication alive.
    pub fn set_muted(
        &self,
        _muted: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if !_muted && self.inner.capture_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "unmute Track Publication",
                    reason: "Local Audio Track capture has stopped",
                })
            });
        }
        if let Err(error) = self.inner.begin_mute_control() {
            return Box::pin(async move { Err(error) });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;

            let publication = Rc::clone(&self.inner);
            let callback_publication = Rc::clone(&publication);
            let muted = _muted;
            let callback = wasm_bindgen::closure::Closure::once_into_js(move |success: bool| {
                if success {
                    callback_publication.muted.set(muted);
                    if let Some(track) = callback_publication.track.upgrade() {
                        track.muted.set(muted);
                    }
                }
            });
            let callback: js_sys::Function = callback.unchecked_into();
            let operation = adapter::set_local_audio_track_muted(
                publication.track_id,
                publication.publication_id,
                _muted,
                &callback,
            )
            .map_err(AdapterError::from_cause)
            .map_err(classify_publisher_mutation_error);
            let operation = match operation {
                Ok(operation) => operation,
                Err(error) => return Box::pin(async move { Err(error) }),
            };
            Box::pin(async move {
                wasm_bindgen_futures::JsFuture::from(operation)
                    .await
                    .map_err(AdapterError::from_cause)
                    .map_err(classify_publisher_mutation_error)?;
                Ok(())
            })
        }
    }

    /// Removes the Track Publication while keeping microphone capture alive.
    pub fn unpublish(&self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        let _previous_lifecycle = match self.inner.begin_unpublish() {
            Ok(previous) => previous,
            Err(error) => return Box::pin(async move { Err(error) }),
        };

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;

            let publication = Rc::clone(&self.inner);
            let callback_track = publication.track.clone();
            let callback_publication = Rc::clone(&publication);
            let callback = wasm_bindgen::closure::Closure::once_into_js(move |success: bool| {
                if success {
                    callback_publication
                        .lifecycle
                        .set(LocalAudioTrackPublicationLifecycle::Unpublished);
                    if let Some(track) = callback_track.upgrade() {
                        track.clear_publication(&callback_publication);
                    }
                } else if callback_publication.lifecycle.get()
                    != LocalAudioTrackPublicationLifecycle::Unpublished
                {
                    callback_publication
                        .lifecycle
                        .set(LocalAudioTrackPublicationLifecycle::Published);
                }
            });
            let callback: js_sys::Function = callback.unchecked_into();
            let operation = adapter::unpublish_local_audio_track(
                publication.room_id,
                publication.track_id,
                publication.publication_id,
                &callback,
            )
            .map_err(AdapterError::from_cause)
            .map_err(classify_publisher_mutation_error);
            let operation = match operation {
                Ok(operation) => operation,
                Err(error) => {
                    publication.lifecycle.set(_previous_lifecycle);
                    return Box::pin(async move { Err(error) });
                }
            };
            Box::pin(async move {
                wasm_bindgen_futures::JsFuture::from(operation)
                    .await
                    .map_err(AdapterError::from_cause)
                    .map_err(classify_publisher_mutation_error)?;
                Ok(())
            })
        }
    }
}

impl fmt::Debug for LocalAudioTrackPublication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalAudioTrackPublication")
            .field("sid", &self.sid())
            .field("muted", &self.is_muted())
            .field("terminal", &self.is_terminal())
            .finish()
    }
}

struct LocalAudioTrackPublicationInner {
    lifecycle: Cell<LocalAudioTrackPublicationLifecycle>,
    muted: Cell<bool>,
    sid: RefCell<Option<String>>,
    session: Weak<SessionInner>,
    track: Weak<LocalAudioTrackInner>,
    #[cfg(target_arch = "wasm32")]
    publication_id: u32,
    #[cfg(target_arch = "wasm32")]
    track_id: u32,
    #[cfg(target_arch = "wasm32")]
    room_id: u32,
}

impl LocalAudioTrackPublicationInner {
    fn capture_stopped(&self) -> bool {
        self.track
            .upgrade()
            .is_none_or(|track| track.lifecycle.get() == LocalAudioTrackLifecycle::Stopped)
    }

    fn is_terminal(&self) -> bool {
        self.lifecycle.get() == LocalAudioTrackPublicationLifecycle::Unpublished
            || self
                .session
                .upgrade()
                .is_none_or(|session| session.lifecycle.get() != SessionLifecycle::Active)
    }

    fn association_released(&self) -> bool {
        self.lifecycle.get() == LocalAudioTrackPublicationLifecycle::Unpublished
            || self
                .session
                .upgrade()
                .is_none_or(|session| session.lifecycle.get() == SessionLifecycle::Left)
    }

    fn begin_mute_control(&self) -> Result<(), Error> {
        if self.is_terminal() {
            return Err(Error::InvalidState {
                operation: "change Track Publication mute",
                reason: "Track Publication is terminal",
            });
        }
        Ok(())
    }

    fn begin_unpublish(&self) -> Result<LocalAudioTrackPublicationLifecycle, Error> {
        if self.is_terminal()
            || self.lifecycle.get() == LocalAudioTrackPublicationLifecycle::Unpublishing
        {
            return Err(Error::InvalidState {
                operation: "unpublish Local Audio Track",
                reason: "Track Publication is terminal or being unpublished",
            });
        }
        let previous = self
            .lifecycle
            .replace(LocalAudioTrackPublicationLifecycle::Unpublishing);
        Ok(previous)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LocalAudioTrackPublicationLifecycle {
    #[cfg(target_arch = "wasm32")]
    Publishing,
    #[cfg(any(target_arch = "wasm32", test))]
    Published,
    Unpublishing,
    Unpublished,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
struct AdapterJoin {
    snapshot: AdapterRoomSnapshot,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
struct AdapterUpdate {
    snapshot: AdapterRoomSnapshot,
    events: Vec<RoomEventKind>,
}

#[cfg(target_arch = "wasm32")]
async fn join_room_session(
    credentials: ConnectionCredentials,
    options: RoomOptions,
) -> Result<RoomSession, Error> {
    use wasm_bindgen::JsCast;

    let raw_room_options = raw_js_options!(options, raw_js_room_options);
    let raw_connect_options = raw_js_options!(options, raw_js_connect_options);

    let room_id = adapter::create_room(raw_room_options)
        .map_err(AdapterError::from_cause)
        .map_err(Error::JoinFailed)?;
    let mut pending = PendingJoin::new(room_id);
    let joined = adapter::connect_room(
        room_id,
        credentials.server_url(),
        credentials.participant_token(),
        options.auto_subscribe(),
        raw_connect_options,
    )
    .map_err(AdapterError::from_cause)
    .map_err(Error::JoinFailed)?;
    let joined = wasm_bindgen_futures::JsFuture::from(joined)
        .await
        .map_err(AdapterError::from_cause)
        .map_err(Error::JoinFailed)?;
    let joined: AdapterJoin = serde_wasm_bindgen::from_value(joined)
        .map_err(|error| Error::AdapterFailure(AdapterError::contract(error.to_string())))?;

    let target: Rc<RefCell<Weak<SessionInner>>> = Rc::new(RefCell::new(Weak::new()));
    let callback_target = Rc::clone(&target);
    let event_callback =
        wasm_bindgen::closure::Closure::new(move |value: wasm_bindgen::JsValue| {
            let Some(inner) = callback_target.borrow().upgrade() else {
                return;
            };
            let Ok(update) = serde_wasm_bindgen::from_value::<AdapterUpdate>(value) else {
                return;
            };
            inner.install_adapter_update(update);
        });
    let callback_target = Rc::clone(&target);
    let data_packet_callback = wasm_bindgen::closure::Closure::new(
        move |payload: js_sys::Uint8Array,
              sender_identity: wasm_bindgen::JsValue,
              reliable: bool| {
            let Some(inner) = callback_target.borrow().upgrade() else {
                return;
            };
            inner.install_data_packet(DataPacket {
                payload: payload.to_vec(),
                sender_identity: sender_identity.as_string(),
                delivery: if reliable {
                    DataPacketDelivery::Reliable
                } else {
                    DataPacketDelivery::Lossy
                },
            });
        },
    );
    let callback_target = Rc::clone(&target);
    let chat_message_callback =
        wasm_bindgen::closure::Closure::new(move |value: wasm_bindgen::JsValue| {
            let Some(inner) = callback_target.borrow().upgrade() else {
                return;
            };
            let Ok(message) = serde_wasm_bindgen::from_value::<AdapterChatMessage>(value) else {
                return;
            };
            inner.install_chat_message(message.into());
        });
    let callback_target = Rc::clone(&target);
    let transcription_segment_callback =
        wasm_bindgen::closure::Closure::new(move |value: wasm_bindgen::JsValue| {
            let Some(inner) = callback_target.borrow().upgrade() else {
                return;
            };
            let Ok(segment) = serde_wasm_bindgen::from_value::<AdapterTranscriptionSegment>(value)
            else {
                return;
            };
            let Some(segment) = segment.into_segment() else {
                return;
            };
            inner.install_transcription_segment(segment);
        });

    let inner = Rc::new(SessionInner {
        snapshot: RefCell::new(Rc::new(joined.snapshot.into())),
        subscribers: RefCell::new(Vec::new()),
        data_packet_subscribers: RefCell::new(Vec::new()),
        chat_message_subscribers: RefCell::new(Vec::new()),
        transcription_segment_subscribers: RefCell::new(Vec::new()),
        rpc_handler_tasks: RefCell::new(Vec::new()),
        lifecycle: Cell::new(SessionLifecycle::Active),
        cleanup_started: Cell::new(false),
        #[cfg(test)]
        cleanup_count: Rc::new(Cell::new(0)),
        room_id,
        leave_operation: RefCell::new(None),
        _event_callback: event_callback,
        _data_packet_callback: data_packet_callback,
        _chat_message_callback: chat_message_callback,
        _transcription_segment_callback: transcription_segment_callback,
    });
    *target.borrow_mut() = Rc::downgrade(&inner);

    adapter::activate_room(
        room_id,
        inner._event_callback.as_ref().unchecked_ref(),
        inner._data_packet_callback.as_ref().unchecked_ref(),
        inner._chat_message_callback.as_ref().unchecked_ref(),
        inner
            ._transcription_segment_callback
            .as_ref()
            .unchecked_ref(),
    )
    .map_err(AdapterError::from_cause)
    .map_err(Error::AdapterFailure)?;

    pending.disarm();
    Ok(RoomSession { inner })
}

#[cfg(target_arch = "wasm32")]
struct PendingJoin {
    room_id: u32,
    armed: bool,
}

#[cfg(target_arch = "wasm32")]
impl PendingJoin {
    fn new(room_id: u32) -> Self {
        Self {
            room_id,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for PendingJoin {
    fn drop(&mut self) {
        if self.armed {
            adapter::drop_room(self.room_id);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn join_room_session(
    _credentials: ConnectionCredentials,
    _options: RoomOptions,
) -> Result<RoomSession, Error> {
    Err(Error::UnsupportedTarget)
}

/// The observable connection state of a Room Session.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum RoomConnectionState {
    Connecting,
    Connected,
    Reconnecting,
    SignalReconnecting,
    Disconnecting,
    Disconnected,
}

/// LiveKit's normalized estimate of a participant's connection quality.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum ConnectionQuality {
    Excellent,
    Good,
    Poor,
    Lost,
    Unknown,
}

/// Statistics shared by normalized audio and video sender samples.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SenderStatistics {
    timestamp: f64,
    packets_sent: Option<u64>,
    bytes_sent: Option<u64>,
    jitter: Option<f64>,
    packets_lost: Option<i64>,
    round_trip_time: Option<f64>,
    stream_id: Option<String>,
}

impl SenderStatistics {
    /// Returns the browser statistics timestamp in milliseconds.
    pub const fn timestamp(&self) -> f64 {
        self.timestamp
    }

    /// Returns the number of RTP packets sent, when reported.
    pub const fn packets_sent(&self) -> Option<u64> {
        self.packets_sent
    }

    /// Returns the number of payload and header bytes sent, when reported.
    pub const fn bytes_sent(&self) -> Option<u64> {
        self.bytes_sent
    }

    /// Returns remote-observed jitter in seconds, when reported.
    pub const fn jitter(&self) -> Option<f64> {
        self.jitter
    }

    /// Returns remote-reported packet loss, when reported.
    pub const fn packets_lost(&self) -> Option<i64> {
        self.packets_lost
    }

    /// Returns remote-reported round-trip time in seconds, when reported.
    pub const fn round_trip_time(&self) -> Option<f64> {
        self.round_trip_time
    }

    /// Returns the outbound RTP stream identifier, when reported.
    pub fn stream_id(&self) -> Option<&str> {
        self.stream_id.as_deref()
    }
}

/// Normalized sender statistics for one published Local Audio Track.
pub type AudioSenderStatistics = SenderStatistics;

/// Normalized sender statistics for one encoding of a published Local Video Track.
///
/// Browser-dependent values remain optional even where the SDK's TypeScript declarations mark
/// them required, because Firefox and WebKit omit some of them at runtime.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct VideoSenderStatistics {
    #[serde(flatten)]
    sender: SenderStatistics,
    fir_count: Option<u64>,
    pli_count: Option<u64>,
    nack_count: Option<u64>,
    rid: Option<String>,
    frame_width: Option<u32>,
    frame_height: Option<u32>,
    frames_per_second: Option<f64>,
    frames_sent: Option<u64>,
    quality_limitation_reason: Option<String>,
    quality_limitation_durations: Option<BTreeMap<String, f64>>,
    quality_limitation_resolution_changes: Option<u64>,
    retransmitted_packets_sent: Option<u64>,
    target_bitrate: Option<f64>,
}

impl VideoSenderStatistics {
    /// Returns fields shared by audio and video sender samples.
    pub const fn sender(&self) -> &SenderStatistics {
        &self.sender
    }

    pub const fn fir_count(&self) -> Option<u64> {
        self.fir_count
    }

    pub const fn pli_count(&self) -> Option<u64> {
        self.pli_count
    }

    pub const fn nack_count(&self) -> Option<u64> {
        self.nack_count
    }

    /// Returns the simulcast RID, when reported.
    pub fn rid(&self) -> Option<&str> {
        self.rid.as_deref()
    }

    pub const fn frame_width(&self) -> Option<u32> {
        self.frame_width
    }

    pub const fn frame_height(&self) -> Option<u32> {
        self.frame_height
    }

    pub const fn frames_per_second(&self) -> Option<f64> {
        self.frames_per_second
    }

    pub const fn frames_sent(&self) -> Option<u64> {
        self.frames_sent
    }

    pub fn quality_limitation_reason(&self) -> Option<&str> {
        self.quality_limitation_reason.as_deref()
    }

    pub fn quality_limitation_durations(&self) -> Option<&BTreeMap<String, f64>> {
        self.quality_limitation_durations.as_ref()
    }

    pub const fn quality_limitation_resolution_changes(&self) -> Option<u64> {
        self.quality_limitation_resolution_changes
    }

    pub const fn retransmitted_packets_sent(&self) -> Option<u64> {
        self.retransmitted_packets_sent
    }

    pub const fn target_bitrate(&self) -> Option<f64> {
        self.target_bitrate
    }
}

/// Statistics shared by normalized audio and video receiver samples.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ReceiverStatistics {
    timestamp: f64,
    jitter_buffer_delay: Option<f64>,
    packets_lost: Option<i64>,
    packets_received: Option<u64>,
    bytes_received: Option<u64>,
    stream_id: Option<String>,
    jitter: Option<f64>,
}

impl ReceiverStatistics {
    /// Returns the browser statistics timestamp in milliseconds.
    pub const fn timestamp(&self) -> f64 {
        self.timestamp
    }

    pub const fn jitter_buffer_delay(&self) -> Option<f64> {
        self.jitter_buffer_delay
    }

    pub const fn packets_lost(&self) -> Option<i64> {
        self.packets_lost
    }

    pub const fn packets_received(&self) -> Option<u64> {
        self.packets_received
    }

    pub const fn bytes_received(&self) -> Option<u64> {
        self.bytes_received
    }

    pub fn stream_id(&self) -> Option<&str> {
        self.stream_id.as_deref()
    }

    pub const fn jitter(&self) -> Option<f64> {
        self.jitter
    }
}

/// Normalized receiver statistics for one subscribed Remote Audio Track.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct AudioReceiverStatistics {
    #[serde(flatten)]
    receiver: ReceiverStatistics,
    concealed_samples: Option<u64>,
    concealment_events: Option<u64>,
    silent_concealed_samples: Option<u64>,
    silent_concealment_events: Option<u64>,
    total_audio_energy: Option<f64>,
    total_samples_duration: Option<f64>,
}

impl AudioReceiverStatistics {
    /// Returns fields shared by audio and video receiver samples.
    pub const fn receiver(&self) -> &ReceiverStatistics {
        &self.receiver
    }

    pub const fn concealed_samples(&self) -> Option<u64> {
        self.concealed_samples
    }

    pub const fn concealment_events(&self) -> Option<u64> {
        self.concealment_events
    }

    pub const fn silent_concealed_samples(&self) -> Option<u64> {
        self.silent_concealed_samples
    }

    pub const fn silent_concealment_events(&self) -> Option<u64> {
        self.silent_concealment_events
    }

    pub const fn total_audio_energy(&self) -> Option<f64> {
        self.total_audio_energy
    }

    pub const fn total_samples_duration(&self) -> Option<f64> {
        self.total_samples_duration
    }
}

/// Normalized receiver statistics for one subscribed Remote Video Track.
///
/// Browser-dependent values remain optional even where the SDK's TypeScript declarations mark
/// them required, because Firefox and WebKit omit some of them at runtime.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct VideoReceiverStatistics {
    #[serde(flatten)]
    receiver: ReceiverStatistics,
    frames_decoded: Option<u64>,
    frames_dropped: Option<u64>,
    frames_received: Option<u64>,
    frame_width: Option<u32>,
    frame_height: Option<u32>,
    fir_count: Option<u64>,
    pli_count: Option<u64>,
    nack_count: Option<u64>,
    decoder_implementation: Option<String>,
    mime_type: Option<String>,
}

impl VideoReceiverStatistics {
    /// Returns fields shared by audio and video receiver samples.
    pub const fn receiver(&self) -> &ReceiverStatistics {
        &self.receiver
    }

    pub const fn frames_decoded(&self) -> Option<u64> {
        self.frames_decoded
    }

    pub const fn frames_dropped(&self) -> Option<u64> {
        self.frames_dropped
    }

    pub const fn frames_received(&self) -> Option<u64> {
        self.frames_received
    }

    pub const fn frame_width(&self) -> Option<u32> {
        self.frame_width
    }

    pub const fn frame_height(&self) -> Option<u32> {
        self.frame_height
    }

    pub const fn fir_count(&self) -> Option<u64> {
        self.fir_count
    }

    pub const fn pli_count(&self) -> Option<u64> {
        self.pli_count
    }

    pub const fn nack_count(&self) -> Option<u64> {
        self.nack_count
    }

    pub fn decoder_implementation(&self) -> Option<&str> {
        self.decoder_implementation.as_deref()
    }

    pub fn mime_type(&self) -> Option<&str> {
        self.mime_type.as_deref()
    }
}

/// Whether browser autoplay policy currently permits remote audio playback.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum AudioPlaybackState {
    /// Remote audio may play without another user activation.
    Allowed,
    /// The browser blocked remote audio until a user-activation retry.
    Blocked,
}

/// An immutable, coherent view of one Room Session.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct RoomSnapshot {
    revision: u64,
    connection: RoomConnectionState,
    audio_playback: AudioPlaybackState,
    active_speakers: Vec<String>,
    room_sid: String,
    room_name: String,
    room_metadata: Option<String>,
    local_participant: LocalParticipantSnapshot,
    remote_participants: BTreeMap<String, RemoteParticipantSnapshot>,
}

impl RoomSnapshot {
    /// Returns the monotonically increasing snapshot revision.
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the Room Session's connection state at this revision.
    pub const fn connection(&self) -> RoomConnectionState {
        self.connection
    }

    /// Returns the browser's current remote-audio playback state.
    pub const fn audio_playback(&self) -> AudioPlaybackState {
        self.audio_playback
    }

    /// Returns active Participant Identities in LiveKit's loudest-first order.
    pub fn active_speakers(&self) -> &[String] {
        &self.active_speakers
    }

    /// Returns the authoritative server SID for the room.
    pub fn room_sid(&self) -> &str {
        &self.room_sid
    }

    /// Returns the room name supplied by LiveKit.
    pub fn room_name(&self) -> &str {
        &self.room_name
    }

    /// Returns the room metadata supplied by LiveKit.
    pub fn room_metadata(&self) -> Option<&str> {
        self.room_metadata.as_deref()
    }

    /// Returns the authoritative Local Participant state.
    pub const fn local_participant(&self) -> &LocalParticipantSnapshot {
        &self.local_participant
    }

    /// Returns Remote Participants keyed by stable Participant Identity.
    pub const fn remote_participants(&self) -> &BTreeMap<String, RemoteParticipantSnapshot> {
        &self.remote_participants
    }
}

/// The immutable Local Participant state in a Room Snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[non_exhaustive]
pub struct LocalParticipantSnapshot {
    identity: String,
    sid: String,
    connection_quality: ConnectionQuality,
    metadata: Option<String>,
    attributes: BTreeMap<String, String>,
    grants: ParticipantGrants,
    track_publications: BTreeMap<String, LocalTrackPublicationSnapshot>,
}

impl LocalParticipantSnapshot {
    /// Returns the stable Participant Identity.
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Returns the current LiveKit participant SID.
    pub fn sid(&self) -> &str {
        &self.sid
    }

    /// Returns LiveKit's current connection-quality estimate.
    pub const fn connection_quality(&self) -> ConnectionQuality {
        self.connection_quality
    }

    /// Returns participant metadata supplied by LiveKit.
    pub fn metadata(&self) -> Option<&str> {
        self.metadata.as_deref()
    }

    /// Returns participant attributes supplied by LiveKit.
    pub const fn attributes(&self) -> &BTreeMap<String, String> {
        &self.attributes
    }

    /// Returns authoritative source grants supplied by LiveKit.
    pub const fn grants(&self) -> &ParticipantGrants {
        &self.grants
    }

    /// Returns Local Participant Track Publications keyed by publication SID.
    pub const fn track_publications(&self) -> &BTreeMap<String, LocalTrackPublicationSnapshot> {
        &self.track_publications
    }
}

/// The immutable state of a Remote Participant in a Room Snapshot.
#[derive(Clone)]
#[non_exhaustive]
pub struct RemoteParticipantSnapshot {
    identity: String,
    sid: String,
    generation: u32,
    connection_quality: ConnectionQuality,
    metadata: Option<String>,
    attributes: BTreeMap<String, String>,
    track_publications: BTreeMap<String, RemoteTrackPublicationSnapshot>,
    data_tracks: BTreeMap<String, RemoteDataTrackSnapshot>,
}

impl RemoteParticipantSnapshot {
    /// Returns the stable Participant Identity used as the Room Snapshot key.
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Returns the current LiveKit participant SID.
    pub fn sid(&self) -> &str {
        &self.sid
    }

    fn generation(&self) -> u32 {
        self.generation
    }

    /// Returns LiveKit's current connection-quality estimate.
    pub const fn connection_quality(&self) -> ConnectionQuality {
        self.connection_quality
    }

    /// Returns participant metadata supplied by LiveKit.
    pub fn metadata(&self) -> Option<&str> {
        self.metadata.as_deref()
    }

    /// Returns participant attributes supplied by LiveKit.
    pub const fn attributes(&self) -> &BTreeMap<String, String> {
        &self.attributes
    }

    /// Returns Track Publications keyed by publication SID.
    pub const fn track_publications(&self) -> &BTreeMap<String, RemoteTrackPublicationSnapshot> {
        &self.track_publications
    }

    /// Returns Remote Data Tracks keyed by their LiveKit data-track SID.
    pub const fn data_tracks(&self) -> &BTreeMap<String, RemoteDataTrackSnapshot> {
        &self.data_tracks
    }
}

impl fmt::Debug for RemoteParticipantSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteParticipantSnapshot")
            .field("identity", &self.identity)
            .field("sid", &self.sid)
            .field("connection_quality", &self.connection_quality)
            .field("metadata", &self.metadata)
            .field("attributes", &self.attributes)
            .field("track_publications", &self.track_publications)
            .field("data_tracks", &self.data_tracks)
            .finish()
    }
}

impl PartialEq for RemoteParticipantSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.sid == other.sid
            && self.connection_quality == other.connection_quality
            && self.metadata == other.metadata
            && self.attributes == other.attributes
            && self.track_publications == other.track_publications
            && self.data_tracks == other.data_tracks
    }
}

impl Eq for RemoteParticipantSnapshot {}

/// LiveKit source grants for the Local Participant.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[non_exhaustive]
pub struct ParticipantGrants {
    can_subscribe: bool,
    can_publish: bool,
    can_publish_data: bool,
    can_publish_sources: Vec<TrackSource>,
    can_update_metadata: bool,
    hidden: bool,
    recorder: bool,
    agent: bool,
    can_subscribe_metrics: bool,
    can_manage_agent_session: bool,
}

impl ParticipantGrants {
    /// Whether the Local Participant may subscribe to remote media.
    pub const fn can_subscribe(&self) -> bool {
        self.can_subscribe
    }

    /// Whether the Local Participant may publish media.
    pub const fn can_publish(&self) -> bool {
        self.can_publish
    }

    /// Whether the Local Participant may publish data.
    pub const fn can_publish_data(&self) -> bool {
        self.can_publish_data
    }

    /// Returns source-specific publication grants.
    ///
    /// LiveKit treats an empty list as all sources when `can_publish` is true.
    pub fn can_publish_sources(&self) -> &[TrackSource] {
        &self.can_publish_sources
    }

    /// Whether the Local Participant may update its metadata and attributes.
    pub const fn can_update_metadata(&self) -> bool {
        self.can_update_metadata
    }

    /// Whether the Local Participant is hidden from other participants.
    pub const fn hidden(&self) -> bool {
        self.hidden
    }

    /// Whether LiveKit identifies this participant as a recorder.
    pub const fn recorder(&self) -> bool {
        self.recorder
    }

    /// Whether LiveKit identifies this participant as an agent.
    pub const fn agent(&self) -> bool {
        self.agent
    }

    /// Whether the Local Participant may subscribe to metrics.
    pub const fn can_subscribe_metrics(&self) -> bool {
        self.can_subscribe_metrics
    }

    /// Whether the Local Participant may manage agent sessions.
    pub const fn can_manage_agent_session(&self) -> bool {
        self.can_manage_agent_session
    }
}

/// A LiveKit media source authorized or represented in a Track Publication.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum TrackSource {
    Unknown,
    Camera,
    Microphone,
    ScreenShare,
    ScreenShareAudio,
}

/// The media kind announced by a Track Publication.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum TrackKind {
    Audio,
    Video,
    Unknown,
}

/// Immutable state for a separately published Remote Data Track.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[non_exhaustive]
pub struct RemoteDataTrackSnapshot {
    sid: String,
    name: String,
    uses_e2ee: bool,
}

impl RemoteDataTrackSnapshot {
    /// Returns the LiveKit data-track SID used as its participant snapshot key.
    pub fn sid(&self) -> &str {
        &self.sid
    }

    /// Returns the publisher-supplied data-track name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether LiveKit reports end-to-end encryption for this data track.
    pub const fn uses_e2ee(&self) -> bool {
        self.uses_e2ee
    }
}

/// Publisher-side policy controlling who may subscribe to Local Participant Track Publications.
///
/// Participant-specific policy is keyed only by stable Participant Identity. Participants omitted
/// from a participant-specific policy cannot subscribe to this publisher's media.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationSubscriptionPermissions {
    all_participants_allowed: bool,
    participant_identities: Vec<String>,
}

impl PublicationSubscriptionPermissions {
    /// Allows every room participant to subscribe to all current and future Track Publications.
    pub fn all_participants() -> Self {
        Self {
            all_participants_allowed: true,
            participant_identities: Vec::new(),
        }
    }

    /// Restricts access to the complete identity-keyed permission list.
    ///
    /// An empty list denies every participant. Participants omitted from the list are denied.
    pub fn participant_specific<I, S>(participant_identities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            all_participants_allowed: false,
            participant_identities: participant_identities.into_iter().map(Into::into).collect(),
        }
    }

    /// Whether every participant may subscribe to every Track Publication.
    pub const fn allows_all_participants(&self) -> bool {
        self.all_participants_allowed
    }

    /// Returns the complete Participant Identity-keyed permission list.
    pub fn participant_identities(&self) -> &[String] {
        &self.participant_identities
    }

    fn is_valid(&self) -> bool {
        self.all_participants_allowed
            || self
                .participant_identities
                .iter()
                .all(|identity| !identity.is_empty())
    }
}

impl Default for PublicationSubscriptionPermissions {
    fn default() -> Self {
        Self::all_participants()
    }
}

/// Immutable state for a Local Participant Track Publication.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[non_exhaustive]
pub struct LocalTrackPublicationSnapshot {
    sid: String,
    name: String,
    source: TrackSource,
    kind: TrackKind,
    muted: bool,
}

impl LocalTrackPublicationSnapshot {
    /// Returns the publication SID.
    pub fn sid(&self) -> &str {
        &self.sid
    }

    /// Returns the publication name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the published source.
    pub const fn source(&self) -> TrackSource {
        self.source
    }

    /// Returns the published media kind.
    pub const fn kind(&self) -> TrackKind {
        self.kind
    }

    /// Whether this publication is muted.
    pub const fn muted(&self) -> bool {
        self.muted
    }
}

/// Immutable state for a Remote Participant Track Publication.
///
/// A publication is room-visible state independent of whether its media is currently available in
/// this browser. No live Media Track handle is stored in a Room Snapshot.
#[derive(Clone)]
#[non_exhaustive]
pub struct RemoteTrackPublicationSnapshot {
    sid: String,
    generation: u32,
    name: String,
    source: TrackSource,
    kind: TrackKind,
    muted: bool,
    subscribed: bool,
    media_available: bool,
    media_generation: Option<u32>,
}

impl RemoteTrackPublicationSnapshot {
    /// Returns the publication SID used as its participant snapshot key.
    pub fn sid(&self) -> &str {
        &self.sid
    }

    fn generation(&self) -> u32 {
        self.generation
    }

    /// Returns the publication name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the published source.
    pub const fn source(&self) -> TrackSource {
        self.source
    }

    /// Returns the published media kind.
    pub const fn kind(&self) -> TrackKind {
        self.kind
    }

    /// Whether this publication is muted.
    pub const fn muted(&self) -> bool {
        self.muted
    }

    /// Whether the Local Participant currently subscribes to this publication.
    pub const fn subscribed(&self) -> bool {
        self.subscribed
    }

    /// Whether subscribed media for this publication is currently available in this browser.
    pub const fn media_available(&self) -> bool {
        self.media_available
    }

    fn media_generation(&self) -> Option<u32> {
        self.media_generation
    }
}

impl fmt::Debug for RemoteTrackPublicationSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteTrackPublicationSnapshot")
            .field("sid", &self.sid)
            .field("name", &self.name)
            .field("source", &self.source)
            .field("kind", &self.kind)
            .field("muted", &self.muted)
            .field("subscribed", &self.subscribed)
            .field("media_available", &self.media_available)
            .finish()
    }
}

impl PartialEq for RemoteTrackPublicationSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.sid == other.sid
            && self.name == other.name
            && self.source == other.source
            && self.kind == other.kind
            && self.muted == other.muted
            && self.subscribed == other.subscribed
            && self.media_available == other.media_available
    }
}

impl Eq for RemoteTrackPublicationSnapshot {}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct AdapterRoomSnapshot {
    #[cfg_attr(test, ts(type = "number"))]
    revision: u64,
    connection: RoomConnectionState,
    audio_playback: AudioPlaybackState,
    active_speakers: Vec<String>,
    room_sid: String,
    room_name: String,
    room_metadata: Option<String>,
    local_participant: LocalParticipantSnapshot,
    remote_participants: BTreeMap<String, AdapterRemoteParticipantSnapshot>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct AdapterRemoteParticipantSnapshot {
    identity: String,
    sid: String,
    generation: u32,
    connection_quality: ConnectionQuality,
    metadata: Option<String>,
    attributes: BTreeMap<String, String>,
    track_publications: BTreeMap<String, AdapterRemoteTrackPublicationSnapshot>,
    data_tracks: BTreeMap<String, RemoteDataTrackSnapshot>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct AdapterRemoteTrackPublicationSnapshot {
    sid: String,
    generation: u32,
    name: String,
    source: TrackSource,
    kind: TrackKind,
    muted: bool,
    subscribed: bool,
    media_available: bool,
    media_generation: Option<u32>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl From<AdapterRoomSnapshot> for RoomSnapshot {
    fn from(snapshot: AdapterRoomSnapshot) -> Self {
        Self {
            revision: snapshot.revision,
            connection: snapshot.connection,
            audio_playback: snapshot.audio_playback,
            active_speakers: snapshot.active_speakers,
            room_sid: snapshot.room_sid,
            room_name: snapshot.room_name,
            room_metadata: snapshot.room_metadata,
            local_participant: snapshot.local_participant,
            remote_participants: snapshot
                .remote_participants
                .into_iter()
                .map(|(identity, participant)| (identity, participant.into()))
                .collect(),
        }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
impl From<AdapterRemoteParticipantSnapshot> for RemoteParticipantSnapshot {
    fn from(participant: AdapterRemoteParticipantSnapshot) -> Self {
        Self {
            identity: participant.identity,
            sid: participant.sid,
            generation: participant.generation,
            connection_quality: participant.connection_quality,
            metadata: participant.metadata,
            attributes: participant.attributes,
            track_publications: participant
                .track_publications
                .into_iter()
                .map(|(sid, publication)| (sid, publication.into()))
                .collect(),
            data_tracks: participant.data_tracks,
        }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
impl From<AdapterRemoteTrackPublicationSnapshot> for RemoteTrackPublicationSnapshot {
    fn from(publication: AdapterRemoteTrackPublicationSnapshot) -> Self {
        Self {
            sid: publication.sid,
            generation: publication.generation,
            name: publication.name,
            source: publication.source,
            kind: publication.kind,
            muted: publication.muted,
            subscribed: publication.subscribed,
            media_available: publication.media_available,
            media_generation: publication.media_generation,
        }
    }
}

#[derive(Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct RemoteParticipantLocator {
    identity: String,
    sid: String,
    generation: u32,
}

#[derive(Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
struct RemotePublicationLocator {
    participant: RemoteParticipantLocator,
    sid: String,
    generation: u32,
    #[serde(skip)]
    kind: TrackKind,
}

impl RoomSnapshot {
    fn participant_for(
        &self,
        locator: &RemoteParticipantLocator,
    ) -> Option<&RemoteParticipantSnapshot> {
        self.remote_participants
            .get(&locator.identity)
            .filter(|participant| {
                participant.sid == locator.sid && participant.generation == locator.generation
            })
    }

    fn publication_for(
        &self,
        locator: &RemotePublicationLocator,
    ) -> Option<&RemoteTrackPublicationSnapshot> {
        self.participant_for(&locator.participant)?
            .track_publications
            .get(&locator.sid)
            .filter(|publication| {
                publication.kind == locator.kind && publication.generation == locator.generation
            })
    }
}

/// A handle for one exact Remote Participant instance.
///
/// Participant Identity remains the logical snapshot key. If LiveKit replaces the participant,
/// this handle becomes terminal instead of retargeting the replacement.
#[derive(Clone)]
pub struct RemoteParticipant {
    inner: Rc<RemoteParticipantInner>,
}

impl RemoteParticipant {
    /// Returns the stable Participant Identity.
    pub fn identity(&self) -> &str {
        &self.inner.locator.identity
    }

    /// Returns the LiveKit participant SID captured by this handle.
    pub fn sid(&self) -> &str {
        &self.inner.locator.sid
    }

    /// Whether this exact participant instance is no longer current in its Room Session.
    pub fn is_terminal(&self) -> bool {
        self.current_snapshot().is_none()
    }

    /// Returns the unstable `livekit-client` Remote Participant object for this exact generation.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_participant(&self) -> Result<wasm_bindgen::JsValue, Error> {
        let Some(_session) = self.inner.session.upgrade() else {
            return Err(Error::InvalidState {
                operation: "access raw Remote Participant",
                reason: "Room Session is terminal",
            });
        };
        if self.is_terminal() {
            return Err(Error::InvalidState {
                operation: "access raw Remote Participant",
                reason: "Remote Participant is no longer current",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        {
            let locator = serde_wasm_bindgen::to_value(&self.inner.locator)
                .map_err(|error| AdapterError::contract(error.to_string()))
                .map_err(Error::AdapterFailure)?;
            raw_js_handle(adapter::raw_remote_participant(_session.room_id, &locator))
        }
    }

    /// Returns the current immutable state only while this exact participant remains current.
    pub fn snapshot(&self) -> Option<RemoteParticipantSnapshot> {
        self.current_snapshot()
    }

    /// Resolves a replacement-safe command handle for one current Track Publication.
    pub fn track_publication(&self, publication_sid: &str) -> Option<RemoteTrackPublication> {
        let participant = self.current_snapshot()?;
        let publication = participant.track_publications().get(publication_sid)?;
        Some(RemoteTrackPublication {
            inner: Rc::new(RemoteTrackPublicationInner {
                locator: RemotePublicationLocator {
                    participant: self.inner.locator.clone(),
                    sid: publication_sid.into(),
                    generation: publication.generation(),
                    kind: publication.kind(),
                },
                session: self.inner.session.clone(),
            }),
        })
    }

    fn current_snapshot(&self) -> Option<RemoteParticipantSnapshot> {
        let session = self.inner.session.upgrade()?;
        if session.lifecycle.get() != SessionLifecycle::Active {
            return None;
        }
        let participant = session
            .snapshot
            .borrow()
            .participant_for(&self.inner.locator)
            .cloned();
        participant
    }
}

impl fmt::Debug for RemoteParticipant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteParticipant")
            .field("identity", &self.identity())
            .field("sid", &self.sid())
            .field("terminal", &self.is_terminal())
            .finish()
    }
}

struct RemoteParticipantInner {
    locator: RemoteParticipantLocator,
    session: Weak<SessionInner>,
}

/// A live command handle for one Remote Participant Track Publication.
///
/// The immutable publication state remains in `RoomSnapshot`; this handle only resolves the
/// current Remote Media Track and controls subscription policy.
#[derive(Clone)]
pub struct RemoteTrackPublication {
    inner: Rc<RemoteTrackPublicationInner>,
}

impl RemoteTrackPublication {
    /// Returns the stable Participant Identity that owns this publication.
    pub fn participant_identity(&self) -> &str {
        &self.inner.locator.participant.identity
    }

    /// Returns the publication SID.
    pub fn sid(&self) -> &str {
        &self.inner.locator.sid
    }

    /// Returns the publication's announced media kind.
    pub fn kind(&self) -> TrackKind {
        self.inner.locator.kind
    }

    /// Whether this handle no longer identifies a current publication in its Room Session.
    pub fn is_terminal(&self) -> bool {
        self.current_snapshot().is_none()
    }

    /// Returns the unstable `livekit-client` Remote Track Publication object.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_publication(&self) -> Result<wasm_bindgen::JsValue, Error> {
        let Some(_session) = self.inner.session.upgrade() else {
            return Err(Error::InvalidState {
                operation: "access raw Remote Track Publication",
                reason: "Room Session is terminal",
            });
        };
        if self.is_terminal() {
            return Err(Error::InvalidState {
                operation: "access raw Remote Track Publication",
                reason: "Track Publication is no longer current",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        {
            let locator = serde_wasm_bindgen::to_value(&self.inner.locator)
                .map_err(|error| AdapterError::contract(error.to_string()))
                .map_err(Error::AdapterFailure)?;
            raw_js_handle(adapter::raw_remote_publication(_session.room_id, &locator))
        }
    }

    /// Returns the current immutable state for this publication.
    pub fn snapshot(&self) -> Option<RemoteTrackPublicationSnapshot> {
        self.current_snapshot()
    }

    /// Enables or disables subscription without attaching or detaching browser elements.
    ///
    /// The command is sent before the returned local future is polled. Authoritative subscription
    /// and media availability continue to arrive through Room Snapshots and Room Events.
    pub fn set_subscribed(
        &self,
        _subscribed: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "change Remote Track Publication subscription",
                    reason: "Track Publication is terminal",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let Some(session) = self.inner.session.upgrade() else {
                return Box::pin(async {
                    Err(Error::InvalidState {
                        operation: "change Remote Track Publication subscription",
                        reason: "Room Session is terminal",
                    })
                });
            };
            let locator = match serde_wasm_bindgen::to_value(&self.inner.locator) {
                Ok(locator) => locator,
                Err(error) => {
                    let error = Error::AdapterFailure(AdapterError::contract(error.to_string()));
                    return Box::pin(async move { Err(error) });
                }
            };
            let result =
                adapter::set_remote_track_subscription(session.room_id, &locator, _subscribed)
                    .map_err(AdapterError::from_cause)
                    .map_err(Error::SubscriptionFailed);
            Box::pin(async move { result })
        }
    }

    /// Resolves the currently available generation-specific Remote Media Track.
    ///
    /// `Ok(None)` preserves the distinction between an observable Track Publication and locally
    /// unavailable media. A later subscription or replacement must be resolved again and never
    /// retargets an older handle.
    pub fn media_track(&self) -> Result<Option<RemoteMediaTrack>, Error> {
        let Some(snapshot) = self.current_snapshot() else {
            return Err(Error::InvalidState {
                operation: "resolve Remote Media Track",
                reason: "Track Publication is terminal",
            });
        };
        if !snapshot.media_available() {
            return Ok(None);
        }
        if !matches!(self.kind(), TrackKind::Audio | TrackKind::Video) {
            return Err(Error::InvalidState {
                operation: "resolve Remote Media Track",
                reason: "Track Publication does not represent audio or video",
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        resolve_remote_media_track(self)
    }

    fn current_snapshot(&self) -> Option<RemoteTrackPublicationSnapshot> {
        let session = self.inner.session.upgrade()?;
        if session.lifecycle.get() != SessionLifecycle::Active {
            return None;
        }
        let publication = session
            .snapshot
            .borrow()
            .publication_for(&self.inner.locator)
            .cloned();
        publication
    }
}

impl fmt::Debug for RemoteTrackPublication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteTrackPublication")
            .field("participant_identity", &self.participant_identity())
            .field("sid", &self.sid())
            .field("kind", &self.kind())
            .field("terminal", &self.is_terminal())
            .finish()
    }
}

struct RemoteTrackPublicationInner {
    locator: RemotePublicationLocator,
    session: Weak<SessionInner>,
}

/// A currently available typed Remote Media Track.
#[derive(Clone)]
#[non_exhaustive]
pub enum RemoteMediaTrack {
    Audio(RemoteAudioTrack),
    Video(RemoteVideoTrack),
}

impl RemoteMediaTrack {
    /// Whether this exact Remote Media Track generation has ended or been replaced.
    pub fn is_terminal(&self) -> bool {
        match self {
            Self::Audio(track) => track.is_terminal(),
            Self::Video(track) => track.is_terminal(),
        }
    }

    /// Returns the unstable `livekit-client` Remote Media Track object.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_media_track(&self) -> Result<wasm_bindgen::JsValue, Error> {
        match self {
            Self::Audio(track) => track.raw_js_media_track(),
            Self::Video(track) => track.raw_js_media_track(),
        }
    }
}

/// One generation of a locally available subscribed Remote Audio Track.
#[derive(Clone)]
pub struct RemoteAudioTrack {
    inner: Rc<RemoteMediaTrackInner>,
}

impl PartialEq for RemoteAudioTrack {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for RemoteAudioTrack {}

impl RemoteAudioTrack {
    /// Returns the Participant Identity that supplied this media.
    pub fn participant_identity(&self) -> &str {
        &self.inner.publication.participant.identity
    }

    /// Returns the Track Publication SID associated with this generation.
    pub fn publication_sid(&self) -> &str {
        &self.inner.publication.sid
    }

    /// Whether this exact media generation has ended or been replaced.
    pub fn is_terminal(&self) -> bool {
        self.inner.is_terminal()
    }

    /// Returns the unstable `livekit-client` Remote Audio Track object.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_media_track(&self) -> Result<wasm_bindgen::JsValue, Error> {
        if self.is_terminal() {
            return Err(Error::InvalidState {
                operation: "access raw Remote Audio Track",
                reason: "Remote Media Track is terminal",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        raw_js_handle(adapter::raw_remote_media_track(self.inner.track_id))
    }

    /// Returns the browser's unstable raw `RTCStatsReport` for this media generation.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_rtc_stats_report(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<wasm_bindgen::JsValue>, Error>> + 'static>> {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "query raw Remote Audio Track RTC statistics",
                    reason: "Remote Media Track is terminal",
                })
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });
        #[cfg(target_arch = "wasm32")]
        raw_js_statistics(adapter::raw_remote_rtc_stats_report(self.inner.track_id))
    }

    /// Queries the latest normalized receiver statistics for this exact media generation.
    pub fn receiver_statistics(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<AudioReceiverStatistics>, Error>> + 'static>>
    {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "query Remote Audio Track receiver statistics",
                    reason: "Remote Media Track is terminal",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            deserialize_statistics(adapter::remote_audio_receiver_statistics(
                self.inner.track_id,
            ))
        }
    }

    /// Creates a typed level and waveform analyser for this exact media generation.
    pub fn create_analyser(&self) -> Result<AudioAnalyser, Error> {
        if self.is_terminal() {
            return Err(Error::InvalidState {
                operation: "create Remote Media Track audio analyser",
                reason: "Remote Media Track is terminal",
            });
        }

        let _source = AudioAnalyserSource::Remote(Rc::downgrade(&self.inner));
        #[cfg(target_arch = "wasm32")]
        return create_audio_analyser(_source, |callback| {
            adapter::create_remote_audio_analyser(self.inner.track_id, callback)
        });

        #[cfg(all(not(target_arch = "wasm32"), test))]
        return Ok(AudioAnalyser::test_analyser(_source));

        #[cfg(all(not(target_arch = "wasm32"), not(test)))]
        Err(Error::UnsupportedTarget)
    }

    /// Attaches this Remote Audio Track without changing subscription policy.
    pub fn attach(
        &self,
        _element: &web_sys::HtmlAudioElement,
    ) -> Result<RemoteAudioTrackAttachment, Error> {
        if self.is_terminal() {
            return Err(Error::InvalidState {
                operation: "attach Remote Audio Track",
                reason: "Remote Media Track is terminal",
            });
        }

        #[cfg(target_arch = "wasm32")]
        let attachment_id = adapter::attach_remote_audio_track(self.inner.track_id, _element)
            .map_err(AdapterError::from_cause)
            .map_err(Error::MediaControlFailed)?;
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        {
            let state = Rc::new(RemoteTrackAttachmentState {
                active: Cell::new(true),
            });
            self.inner
                .attachments
                .borrow_mut()
                .push(Rc::downgrade(&state));
            Ok(RemoteAudioTrackAttachment {
                inner: Rc::new(RemoteAudioTrackAttachmentInner {
                    state,
                    track: Rc::downgrade(&self.inner),
                    track_id: self.inner.track_id,
                    attachment_id,
                }),
            })
        }
    }

    #[cfg(test)]
    fn test_track(session: &RoomSession) -> Self {
        Self {
            inner: test_remote_media_track(
                session,
                "TR_microphone",
                "microphone",
                TrackSource::Microphone,
                TrackKind::Audio,
            ),
        }
    }
}

impl fmt::Debug for RemoteAudioTrack {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteAudioTrack")
            .field("participant_identity", &self.participant_identity())
            .field("publication_sid", &self.publication_sid())
            .field("terminal", &self.is_terminal())
            .finish()
    }
}

#[cfg(test)]
fn test_remote_media_track(
    session: &RoomSession,
    sid: &str,
    name: &str,
    source: TrackSource,
    kind: TrackKind,
) -> Rc<RemoteMediaTrackInner> {
    let mut snapshot = session.snapshot().as_ref().clone();
    snapshot.revision += 1;
    snapshot.remote_participants.insert(
        "student".into(),
        RemoteParticipantSnapshot {
            identity: "student".into(),
            sid: "PA_student".into(),
            generation: 1,
            connection_quality: ConnectionQuality::Good,
            metadata: None,
            attributes: BTreeMap::new(),
            track_publications: BTreeMap::from([(
                sid.into(),
                RemoteTrackPublicationSnapshot {
                    sid: sid.into(),
                    generation: 2,
                    name: name.into(),
                    source,
                    kind,
                    muted: false,
                    subscribed: true,
                    media_available: true,
                    media_generation: Some(3),
                },
            )]),
            data_tracks: BTreeMap::new(),
        },
    );
    session
        .inner
        .install_normalized_update(snapshot, std::iter::empty());
    Rc::new(RemoteMediaTrackInner {
        lifecycle: Cell::new(RemoteMediaTrackLifecycle::Active),
        publication: RemotePublicationLocator {
            participant: RemoteParticipantLocator {
                identity: "student".into(),
                sid: "PA_student".into(),
                generation: 1,
            },
            sid: sid.into(),
            generation: 2,
            kind,
        },
        media_generation: 3,
        session: Rc::downgrade(&session.inner),
        attachments: RefCell::new(Vec::new()),
        analysers: RefCell::new(Vec::new()),
    })
}

/// One generation of a locally available subscribed Remote Video Track.
#[derive(Clone)]
pub struct RemoteVideoTrack {
    inner: Rc<RemoteMediaTrackInner>,
}

impl PartialEq for RemoteVideoTrack {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for RemoteVideoTrack {}

impl RemoteVideoTrack {
    /// Returns the Participant Identity that supplied this media.
    pub fn participant_identity(&self) -> &str {
        &self.inner.publication.participant.identity
    }

    /// Returns the Track Publication SID associated with this generation.
    pub fn publication_sid(&self) -> &str {
        &self.inner.publication.sid
    }

    /// Whether this exact media generation has ended or been replaced.
    pub fn is_terminal(&self) -> bool {
        self.inner.is_terminal()
    }

    /// Returns the unstable `livekit-client` Remote Video Track object.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_media_track(&self) -> Result<wasm_bindgen::JsValue, Error> {
        if self.is_terminal() {
            return Err(Error::InvalidState {
                operation: "access raw Remote Video Track",
                reason: "Remote Media Track is terminal",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        raw_js_handle(adapter::raw_remote_media_track(self.inner.track_id))
    }

    /// Returns the browser's unstable raw `RTCStatsReport` for this media generation.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_rtc_stats_report(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<wasm_bindgen::JsValue>, Error>> + 'static>> {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "query raw Remote Video Track RTC statistics",
                    reason: "Remote Media Track is terminal",
                })
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });
        #[cfg(target_arch = "wasm32")]
        raw_js_statistics(adapter::raw_remote_rtc_stats_report(self.inner.track_id))
    }

    /// Queries the latest normalized receiver statistics for this exact media generation.
    pub fn receiver_statistics(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<VideoReceiverStatistics>, Error>> + 'static>>
    {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "query Remote Video Track receiver statistics",
                    reason: "Remote Media Track is terminal",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            deserialize_statistics(adapter::remote_video_receiver_statistics(
                self.inner.track_id,
            ))
        }
    }

    /// Attaches this Remote Video Track without changing subscription policy.
    pub fn attach(
        &self,
        _element: &web_sys::HtmlVideoElement,
    ) -> Result<RemoteVideoTrackAttachment, Error> {
        if self.is_terminal() {
            return Err(Error::InvalidState {
                operation: "attach Remote Video Track",
                reason: "Remote Media Track is terminal",
            });
        }

        #[cfg(target_arch = "wasm32")]
        let attachment_id = adapter::attach_remote_video_track(self.inner.track_id, _element)
            .map_err(AdapterError::from_cause)
            .map_err(Error::MediaControlFailed)?;
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        {
            let state = Rc::new(RemoteTrackAttachmentState {
                active: Cell::new(true),
            });
            self.inner
                .attachments
                .borrow_mut()
                .push(Rc::downgrade(&state));
            Ok(RemoteVideoTrackAttachment {
                inner: Rc::new(RemoteVideoTrackAttachmentInner {
                    state,
                    track: Rc::downgrade(&self.inner),
                    track_id: self.inner.track_id,
                    attachment_id,
                }),
            })
        }
    }

    #[cfg(test)]
    fn test_track(session: &RoomSession) -> Self {
        Self {
            inner: test_remote_media_track(
                session,
                "TR_camera",
                "camera",
                TrackSource::Camera,
                TrackKind::Video,
            ),
        }
    }
}

impl fmt::Debug for RemoteVideoTrack {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteVideoTrack")
            .field("participant_identity", &self.participant_identity())
            .field("publication_sid", &self.publication_sid())
            .field("terminal", &self.is_terminal())
            .finish()
    }
}

struct RemoteMediaTrackInner {
    lifecycle: Cell<RemoteMediaTrackLifecycle>,
    publication: RemotePublicationLocator,
    media_generation: u32,
    session: Weak<SessionInner>,
    attachments: RefCell<Vec<Weak<RemoteTrackAttachmentState>>>,
    analysers: RefCell<Vec<Weak<AudioAnalyserState>>>,
    #[cfg(target_arch = "wasm32")]
    track_id: u32,
    #[cfg(target_arch = "wasm32")]
    _terminal_callback: wasm_bindgen::closure::Closure<dyn Fn()>,
}

impl RemoteMediaTrackInner {
    fn is_terminal(&self) -> bool {
        self.lifecycle.get() == RemoteMediaTrackLifecycle::Terminal
            || self.session.upgrade().is_none_or(|session| {
                session.lifecycle.get() != SessionLifecycle::Active
                    || !self.is_current_generation(&session.snapshot.borrow())
            })
    }

    fn is_current_generation(&self, snapshot: &RoomSnapshot) -> bool {
        snapshot
            .publication_for(&self.publication)
            .is_some_and(|publication| {
                publication.media_generation() == Some(self.media_generation)
            })
    }

    fn mark_terminal(&self) {
        if self.lifecycle.replace(RemoteMediaTrackLifecycle::Terminal)
            == RemoteMediaTrackLifecycle::Terminal
        {
            return;
        }
        terminate_audio_analysers(&self.analysers);
        for attachment in self.attachments.borrow_mut().drain(..) {
            if let Some(attachment) = attachment.upgrade() {
                attachment.active.set(false);
            }
        }
    }

    fn remove_attachment(&self, state: &Rc<RemoteTrackAttachmentState>) {
        self.attachments.borrow_mut().retain(|attachment| {
            attachment
                .upgrade()
                .is_some_and(|attachment| !Rc::ptr_eq(&attachment, state))
        });
    }
}

impl Drop for RemoteMediaTrackInner {
    fn drop(&mut self) {
        self.mark_terminal();
        #[cfg(target_arch = "wasm32")]
        adapter::drop_remote_media_track(self.track_id);
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum RemoteMediaTrackLifecycle {
    #[allow(dead_code)]
    Active,
    Terminal,
}

struct RemoteTrackAttachmentState {
    active: Cell<bool>,
}

/// A typed Remote Audio Track element attachment with explicit and drop cleanup.
pub struct RemoteAudioTrackAttachment {
    inner: Rc<RemoteAudioTrackAttachmentInner>,
}

impl RemoteAudioTrackAttachment {
    /// Whether this guard still owns an active attachment to a current media generation.
    pub fn is_active(&self) -> bool {
        self.inner.state.active.get()
            && self
                .inner
                .track
                .upgrade()
                .is_some_and(|track| !track.is_terminal())
    }

    /// Detaches the element without changing network subscription state.
    pub fn close(&self) -> Result<(), Error> {
        if !self.inner.state.active.get() {
            return Ok(());
        }
        #[cfg(target_arch = "wasm32")]
        adapter::detach_remote_audio_track(self.inner.track_id, self.inner.attachment_id)
            .map_err(AdapterError::from_cause)
            .map_err(Error::MediaControlFailed)?;
        self.inner.state.active.set(false);
        if let Some(track) = self.inner.track.upgrade() {
            track.remove_attachment(&self.inner.state);
        }
        Ok(())
    }
}

impl fmt::Debug for RemoteAudioTrackAttachment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteAudioTrackAttachment")
            .field("active", &self.is_active())
            .finish()
    }
}

struct RemoteAudioTrackAttachmentInner {
    state: Rc<RemoteTrackAttachmentState>,
    track: Weak<RemoteMediaTrackInner>,
    #[cfg(target_arch = "wasm32")]
    track_id: u32,
    #[cfg(target_arch = "wasm32")]
    attachment_id: u32,
}

impl Drop for RemoteAudioTrackAttachmentInner {
    fn drop(&mut self) {
        if self.state.active.replace(false) {
            #[cfg(target_arch = "wasm32")]
            adapter::drop_remote_audio_track_attachment(self.track_id, self.attachment_id);
        }
        if let Some(track) = self.track.upgrade() {
            track.remove_attachment(&self.state);
        }
    }
}

/// A typed Remote Video Track element attachment with explicit and drop cleanup.
pub struct RemoteVideoTrackAttachment {
    inner: Rc<RemoteVideoTrackAttachmentInner>,
}

impl RemoteVideoTrackAttachment {
    /// Whether this guard still owns an active attachment to a current media generation.
    pub fn is_active(&self) -> bool {
        self.inner.state.active.get()
            && self
                .inner
                .track
                .upgrade()
                .is_some_and(|track| !track.is_terminal())
    }

    /// Detaches the element without changing network subscription state.
    pub fn close(&self) -> Result<(), Error> {
        if !self.inner.state.active.get() {
            return Ok(());
        }
        #[cfg(target_arch = "wasm32")]
        adapter::detach_remote_video_track(self.inner.track_id, self.inner.attachment_id)
            .map_err(AdapterError::from_cause)
            .map_err(Error::MediaControlFailed)?;
        self.inner.state.active.set(false);
        if let Some(track) = self.inner.track.upgrade() {
            track.remove_attachment(&self.inner.state);
        }
        Ok(())
    }

    #[cfg(test)]
    fn test_attachment(track: &RemoteVideoTrack) -> Self {
        let state = Rc::new(RemoteTrackAttachmentState {
            active: Cell::new(true),
        });
        track
            .inner
            .attachments
            .borrow_mut()
            .push(Rc::downgrade(&state));
        Self {
            inner: Rc::new(RemoteVideoTrackAttachmentInner {
                state,
                track: Rc::downgrade(&track.inner),
            }),
        }
    }
}

impl fmt::Debug for RemoteVideoTrackAttachment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteVideoTrackAttachment")
            .field("active", &self.is_active())
            .finish()
    }
}

struct RemoteVideoTrackAttachmentInner {
    state: Rc<RemoteTrackAttachmentState>,
    track: Weak<RemoteMediaTrackInner>,
    #[cfg(target_arch = "wasm32")]
    track_id: u32,
    #[cfg(target_arch = "wasm32")]
    attachment_id: u32,
}

impl Drop for RemoteVideoTrackAttachmentInner {
    fn drop(&mut self) {
        if self.state.active.replace(false) {
            #[cfg(target_arch = "wasm32")]
            adapter::drop_remote_video_track_attachment(self.track_id, self.attachment_id);
        }
        if let Some(track) = self.track.upgrade() {
            track.remove_attachment(&self.state);
        }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
struct AdapterRemoteMediaTrack {
    id: u32,
    kind: TrackKind,
    generation: u32,
}

#[cfg(target_arch = "wasm32")]
fn resolve_remote_media_track(
    publication: &RemoteTrackPublication,
) -> Result<Option<RemoteMediaTrack>, Error> {
    use wasm_bindgen::JsCast;

    let Some(session) = publication.inner.session.upgrade() else {
        return Err(Error::InvalidState {
            operation: "resolve Remote Media Track",
            reason: "Room Session is terminal",
        });
    };
    let target: Rc<RefCell<Weak<RemoteMediaTrackInner>>> = Rc::new(RefCell::new(Weak::new()));
    let callback_target = Rc::clone(&target);
    let terminal_callback = wasm_bindgen::closure::Closure::new(move || {
        if let Some(inner) = callback_target.borrow().upgrade() {
            inner.mark_terminal();
        }
    });
    let locator = serde_wasm_bindgen::to_value(&publication.inner.locator)
        .map_err(|error| Error::AdapterFailure(AdapterError::contract(error.to_string())))?;
    let descriptor = adapter::resolve_remote_media_track(
        session.room_id,
        &locator,
        publication
            .current_snapshot()
            .and_then(|snapshot| snapshot.media_generation())
            .ok_or(Error::InvalidState {
                operation: "resolve Remote Media Track",
                reason: "Remote Media Track is no longer available",
            })?,
        terminal_callback.as_ref().unchecked_ref(),
    )
    .map_err(AdapterError::from_cause)
    .map_err(Error::MediaControlFailed)?;
    if descriptor.is_null() || descriptor.is_undefined() {
        return Ok(None);
    }
    let parsed = serde_wasm_bindgen::from_value(descriptor.clone());
    let descriptor: AdapterRemoteMediaTrack = match parsed {
        Ok(descriptor) => descriptor,
        Err(error) => {
            adapter::drop_remote_media_track_descriptor(descriptor);
            return Err(Error::AdapterFailure(AdapterError::contract(
                error.to_string(),
            )));
        }
    };
    if descriptor.kind != publication.kind() {
        adapter::drop_remote_media_track(descriptor.id);
        return Err(Error::AdapterFailure(AdapterError::contract(
            "Remote Media Track kind did not match its Track Publication".into(),
        )));
    }
    let Some(snapshot) = publication.current_snapshot() else {
        adapter::drop_remote_media_track(descriptor.id);
        return Err(Error::InvalidState {
            operation: "resolve Remote Media Track",
            reason: "Track Publication is terminal",
        });
    };
    if snapshot.media_generation() != Some(descriptor.generation) {
        adapter::drop_remote_media_track(descriptor.id);
        return Err(Error::InvalidState {
            operation: "resolve Remote Media Track",
            reason: "Remote Media Track was replaced while resolving",
        });
    }
    let inner = Rc::new(RemoteMediaTrackInner {
        lifecycle: Cell::new(RemoteMediaTrackLifecycle::Active),
        publication: publication.inner.locator.clone(),
        media_generation: descriptor.generation,
        session: Rc::downgrade(&session),
        attachments: RefCell::new(Vec::new()),
        analysers: RefCell::new(Vec::new()),
        track_id: descriptor.id,
        _terminal_callback: terminal_callback,
    });
    *target.borrow_mut() = Rc::downgrade(&inner);
    Ok(Some(match descriptor.kind {
        TrackKind::Audio => RemoteMediaTrack::Audio(RemoteAudioTrack { inner }),
        TrackKind::Video => RemoteMediaTrack::Video(RemoteVideoTrack { inner }),
        _ => unreachable!("validated Remote Media Track kind"),
    }))
}

/// The default time a Participant RPC caller waits for a response.
pub const DEFAULT_RPC_RESPONSE_TIMEOUT: Duration = Duration::from_secs(15);

/// The minimum response timeout normalized by this wrapper for consistent caller/handler behavior.
pub const MIN_RPC_RESPONSE_TIMEOUT: Duration = Duration::from_secs(8);

/// The maximum encoded request or response accepted by the opinionated RPC interface.
///
/// Larger content belongs on a Data Stream so the pinned SDK does not buffer it as one RPC string.
pub const MAX_RPC_PAYLOAD_BYTES: usize = 15_360;

/// Default maximum number of concurrent invocations for one Participant RPC method.
pub const DEFAULT_RPC_HANDLER_CAPACITY: usize = 64;

const RPC_WIRE_PREFIX: &str = "~livekit-browser-rpc-v1:";
const RPC_TEXT_PREFIX: &str = "~livekit-browser-rpc-v1:text:";
const RPC_BYTES_PREFIX: &str = "~livekit-browser-rpc-v1:bytes:";

/// A native application payload sent or returned through Participant RPC.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RpcPayload {
    Text(String),
    Bytes(Vec<u8>),
}

impl RpcPayload {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            Self::Bytes(_) => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Text(_) => None,
            Self::Bytes(value) => Some(value),
        }
    }
}

fn encode_rpc_payload(payload: &RpcPayload) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    match payload {
        RpcPayload::Text(value) if value.starts_with(RPC_WIRE_PREFIX) => {
            format!("{RPC_TEXT_PREFIX}{value}")
        }
        RpcPayload::Text(value) => value.clone(),
        RpcPayload::Bytes(value) => format!("{RPC_BYTES_PREFIX}{}", STANDARD.encode(value)),
    }
}

#[cfg(target_arch = "wasm32")]
fn decode_rpc_payload(payload: String) -> RpcPayload {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    if let Some(encoded) = payload.strip_prefix(RPC_BYTES_PREFIX) {
        if let Ok(bytes) = STANDARD.decode(encoded) {
            return RpcPayload::Bytes(bytes);
        }
    }
    if let Some(text) = payload.strip_prefix(RPC_TEXT_PREFIX) {
        return RpcPayload::Text(text.into());
    }
    RpcPayload::Text(payload)
}

/// Options for one Participant RPC invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RpcOptions {
    destination_identity: String,
    method: String,
    payload: RpcPayload,
    response_timeout: Duration,
}

impl RpcOptions {
    pub fn new(
        destination_identity: impl Into<String>,
        method: impl Into<String>,
        payload: RpcPayload,
    ) -> Self {
        Self {
            destination_identity: destination_identity.into(),
            method: method.into(),
            payload,
            response_timeout: DEFAULT_RPC_RESPONSE_TIMEOUT,
        }
    }

    pub fn with_response_timeout(mut self, response_timeout: Duration) -> Self {
        self.response_timeout = response_timeout.max(MIN_RPC_RESPONSE_TIMEOUT);
        self
    }

    pub fn destination_identity(&self) -> &str {
        &self.destination_identity
    }

    pub fn method(&self) -> &str {
        &self.method
    }

    pub const fn payload(&self) -> &RpcPayload {
        &self.payload
    }

    pub const fn response_timeout(&self) -> Duration {
        self.response_timeout
    }
}

/// One Participant RPC request accepted by a registered method handler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RpcInvocation {
    request_id: String,
    caller_identity: String,
    payload: RpcPayload,
    response_timeout: Duration,
}

impl RpcInvocation {
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn caller_identity(&self) -> &str {
        &self.caller_identity
    }

    pub const fn payload(&self) -> &RpcPayload {
        &self.payload
    }

    pub const fn response_timeout(&self) -> Duration {
        self.response_timeout
    }
}

/// An application-defined failure returned by a Participant RPC handler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RpcApplicationError {
    code: u32,
    message: String,
    data: Option<String>,
}

impl RpcApplicationError {
    /// Creates a remote application failure.
    ///
    /// LiveKit reserves codes 1001 through 1999. Reserved values are normalized to its generic
    /// application-error code so they cannot be mistaken for transport failures by the caller.
    pub fn new(code: u32, message: impl Into<String>) -> Self {
        Self {
            code: if (1001..=1999).contains(&code) {
                1500
            } else {
                code
            },
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(mut self, data: impl Into<String>) -> Self {
        self.data = Some(data.into());
        self
    }

    pub const fn code(&self) -> u32 {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn data(&self) -> Option<&str> {
        self.data.as_deref()
    }
}

/// A stable category for a failed Participant RPC invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RpcFailureKind {
    UnsupportedMethod,
    RemoteParticipantNotFound,
    RequestPayloadTooLarge,
    UnsupportedServer,
    UnsupportedVersion,
    ApplicationError,
    ConnectionTimeout,
    ResponseTimeout,
    RemoteParticipantDisconnected,
    ResponsePayloadTooLarge,
    SendFailed,
    ApplicationDefined,
    Unknown,
}

/// A normalized remote or transport failure from Participant RPC.
#[derive(Clone)]
pub struct RpcFailure {
    code: Option<u32>,
    message: String,
    data: Option<String>,
    #[allow(dead_code)]
    cause: Option<AdapterError>,
}

impl RpcFailure {
    pub const fn kind(&self) -> RpcFailureKind {
        match self.code {
            Some(1400) => RpcFailureKind::UnsupportedMethod,
            Some(1401) => RpcFailureKind::RemoteParticipantNotFound,
            Some(1402) => RpcFailureKind::RequestPayloadTooLarge,
            Some(1403) => RpcFailureKind::UnsupportedServer,
            Some(1404) => RpcFailureKind::UnsupportedVersion,
            Some(1500) => RpcFailureKind::ApplicationError,
            Some(1501) => RpcFailureKind::ConnectionTimeout,
            Some(1502) => RpcFailureKind::ResponseTimeout,
            Some(1503) => RpcFailureKind::RemoteParticipantDisconnected,
            Some(1504) => RpcFailureKind::ResponsePayloadTooLarge,
            Some(1505) => RpcFailureKind::SendFailed,
            Some(0..=1000 | 2000..=u32::MAX) => RpcFailureKind::ApplicationDefined,
            _ => RpcFailureKind::Unknown,
        }
    }

    pub const fn code(&self) -> Option<u32> {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn data(&self) -> Option<&str> {
        self.data.as_deref()
    }

    #[cfg(feature = "raw-js")]
    fn raw_js_cause(&self) -> Option<&wasm_bindgen::JsValue> {
        self.cause.as_ref().and_then(AdapterError::raw_js_cause)
    }
}

impl fmt::Debug for RpcFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RpcFailure")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("data", &self.data)
            .finish_non_exhaustive()
    }
}

impl PartialEq for RpcFailure {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code && self.message == other.message && self.data == other.data
    }
}

impl Eq for RpcFailure {}

impl fmt::Display for RpcFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(code) = self.code {
            write!(formatter, "RPC {code}: {}", self.message)
        } else {
            formatter.write_str(&self.message)
        }
    }
}

impl std::error::Error for RpcFailure {}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
#[serde(rename_all = "camelCase")]
struct AdapterRpcOptions {
    destination_identity: String,
    method: String,
    payload: String,
    response_timeout_ms: u32,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
struct AdapterRpcFailure {
    #[serde(default)]
    terminal: bool,
    #[serde(default)]
    permission_denied: bool,
    code: Option<u32>,
    message: String,
    data: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
struct AdapterRpcInvocation {
    id: u32,
    request_id: String,
    caller_identity: String,
    payload: String,
    #[cfg_attr(test, ts(type = "number"))]
    response_timeout_ms: u64,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
#[serde(rename_all = "camelCase")]
struct AdapterRpcHandlerCompletion {
    success: bool,
    payload: Option<String>,
    code: Option<u32>,
    message: Option<String>,
    data: Option<String>,
}

#[cfg(target_arch = "wasm32")]
struct RpcInvocationState {
    result: RefCell<Option<Result<RpcPayload, Error>>>,
    waker: RefCell<Option<Waker>>,
}

#[cfg(target_arch = "wasm32")]
impl RpcInvocationState {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            result: RefCell::new(None),
            waker: RefCell::new(None),
        })
    }

    fn complete(&self, result: Result<RpcPayload, Error>) {
        if self.result.borrow().is_some() {
            return;
        }
        self.result.borrow_mut().replace(result);
        if let Some(waker) = self.waker.borrow_mut().take() {
            waker.wake();
        }
    }
}

#[cfg(target_arch = "wasm32")]
struct RpcInvocationFuture {
    state: Rc<RpcInvocationState>,
}

#[cfg(target_arch = "wasm32")]
impl Future for RpcInvocationFuture {
    type Output = Result<RpcPayload, Error>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(result) = self.state.result.borrow_mut().take() else {
            self.state
                .waker
                .borrow_mut()
                .replace(context.waker().clone());
            return Poll::Pending;
        };
        Poll::Ready(result)
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for RpcInvocationFuture {
    fn drop(&mut self) {
        self.state.waker.borrow_mut().take();
    }
}

#[cfg(target_arch = "wasm32")]
fn start_rpc_invocation(
    room_id: u32,
    options: RpcOptions,
) -> Pin<Box<dyn Future<Output = Result<RpcPayload, Error>> + 'static>> {
    use wasm_bindgen::JsCast;

    let state = RpcInvocationState::new();
    let callback_state = Rc::clone(&state);
    let callback = wasm_bindgen::closure::Closure::once_into_js(
        move |success: bool, value: wasm_bindgen::JsValue| {
            let result = if success {
                value.as_string().map(decode_rpc_payload).ok_or_else(|| {
                    Error::AdapterFailure(AdapterError::contract(
                        "Participant RPC returned a non-string payload".into(),
                    ))
                })
            } else {
                let raw_cause =
                    js_sys::Reflect::get(&value, &wasm_bindgen::JsValue::from_str("cause"))
                        .ok()
                        .map(AdapterError::from_cause);
                match serde_wasm_bindgen::from_value::<AdapterRpcFailure>(value) {
                    Ok(failure) if failure.terminal => Err(Error::InvalidState {
                        operation: "invoke participant RPC",
                        reason: "Room Session is terminal",
                    }),
                    Ok(failure) if failure.permission_denied => {
                        Err(Error::PermissionDenied(raw_cause.unwrap_or_else(|| {
                            AdapterError {
                                name: "PermissionDeniedError".into(),
                                message: failure.message,
                                status: Some(403),
                                reason_name: None,
                                _cause: None,
                            }
                        })))
                    }
                    Ok(failure) => Err(Error::RpcInvocationFailed(RpcFailure {
                        code: failure.code,
                        message: failure.message,
                        data: failure.data,
                        cause: raw_cause,
                    })),
                    Err(error) => Err(Error::AdapterFailure(AdapterError::contract(
                        error.to_string(),
                    ))),
                }
            };
            callback_state.complete(result);
        },
    );
    let callback: js_sys::Function = callback.unchecked_into();
    let options = AdapterRpcOptions {
        destination_identity: options.destination_identity,
        method: options.method,
        payload: encode_rpc_payload(&options.payload),
        response_timeout_ms: u32::try_from(options.response_timeout.as_millis())
            .unwrap_or(u32::MAX),
    };
    let options = match serde_wasm_bindgen::to_value(&options) {
        Ok(options) => options,
        Err(error) => {
            state.complete(Err(Error::AdapterFailure(AdapterError::contract(
                error.to_string(),
            ))));
            return Box::pin(RpcInvocationFuture { state });
        }
    };
    if let Err(cause) = adapter::invoke_participant_rpc(room_id, &options, &callback) {
        let error = AdapterError::from_cause(cause);
        state.complete(Err(if is_permission_error(&error) {
            Error::PermissionDenied(error)
        } else {
            Error::RpcInvocationFailed(RpcFailure {
                code: None,
                message: error.to_string(),
                data: None,
                cause: Some(error),
            })
        }));
    }
    Box::pin(RpcInvocationFuture { state })
}

#[cfg(target_arch = "wasm32")]
struct RpcHandlerTask {
    invocation_id: u32,
    abort: futures_util::future::AbortHandle,
}

#[cfg(any(target_arch = "wasm32", test))]
struct RpcHandlerCapacity {
    limit: usize,
    in_flight: Rc<Cell<usize>>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl RpcHandlerCapacity {
    fn new(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            in_flight: Rc::new(Cell::new(0)),
        }
    }

    fn try_acquire(&self) -> Option<RpcHandlerPermit> {
        let in_flight = self.in_flight.get();
        if in_flight >= self.limit {
            return None;
        }
        self.in_flight.set(in_flight + 1);
        Some(RpcHandlerPermit {
            in_flight: Rc::clone(&self.in_flight),
        })
    }
}

#[cfg(any(target_arch = "wasm32", test))]
struct RpcHandlerPermit {
    in_flight: Rc<Cell<usize>>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl Drop for RpcHandlerPermit {
    fn drop(&mut self) {
        self.in_flight.set(self.in_flight.get().saturating_sub(1));
    }
}

#[cfg(target_arch = "wasm32")]
fn complete_rpc_handler(invocation_id: u32, completion: AdapterRpcHandlerCompletion) {
    let completion = match serde_wasm_bindgen::to_value(&completion) {
        Ok(completion) => completion,
        Err(error) => {
            let fallback = AdapterRpcHandlerCompletion {
                success: false,
                payload: None,
                code: Some(1500),
                message: Some("could not encode Participant RPC response".into()),
                data: Some(error.to_string()),
            };
            let Ok(completion) = serde_wasm_bindgen::to_value(&fallback) else {
                adapter::drop_participant_rpc_invocation(invocation_id);
                return;
            };
            completion
        }
    };
    adapter::complete_participant_rpc_invocation(invocation_id, &completion);
}

struct RpcMethodRegistrationInner {
    active: Rc<Cell<bool>>,
    session: Weak<SessionInner>,
    #[cfg(target_arch = "wasm32")]
    registration_id: u32,
    #[cfg(target_arch = "wasm32")]
    _callback: wasm_bindgen::closure::Closure<dyn FnMut(wasm_bindgen::JsValue)>,
}

impl RpcMethodRegistrationInner {
    fn is_active(&self) -> bool {
        self.active.get()
            && self
                .session
                .upgrade()
                .is_some_and(|session| session.lifecycle.get() == SessionLifecycle::Active)
    }

    fn unregister(&self) {
        if self.active.replace(false) {
            #[cfg(target_arch = "wasm32")]
            adapter::drop_participant_rpc_method(self.registration_id);
        }
    }
}

impl Drop for RpcMethodRegistrationInner {
    fn drop(&mut self) {
        self.unregister();
    }
}

/// Registration guard for one Participant RPC method in a Room Session.
pub struct RpcMethodRegistration {
    inner: Rc<RpcMethodRegistrationInner>,
}

impl RpcMethodRegistration {
    pub fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    pub fn unregister(&self) {
        self.inner.unregister();
    }
}

/// The largest text UTF-8 or byte chunk accepted by an outgoing data stream write.
///
/// Keeping each crossing no larger than LiveKit's protocol chunk avoids an extra unbounded WASM
/// allocation and lets each awaited write preserve browser data-channel backpressure.
pub const MAX_DATA_STREAM_CHUNK_BYTES: usize = 15_000;

/// The wrapper admits one unconsumed application chunk per outgoing stream.
pub const DATA_STREAM_RECEIVE_WINDOW_CHUNKS: usize = 1;

/// The maximum number of incoming wrapper-controlled streams retained by one Room Session.
pub const MAX_ACTIVE_INCOMING_DATA_STREAMS: usize = 16;

/// A typed terminal reason reported by LiveKit's data-stream protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DataStreamErrorReason {
    AlreadyOpened,
    AbnormalEnd,
    DecodeFailed,
    LengthExceeded,
    Incomplete,
    HandlerAlreadyRegistered,
    EncryptionTypeMismatch,
    DestinationUnavailable,
    RemoteCancelled,
    FlowControlTimeout,
    FlowControlProtocol,
}

/// Topic and optional finite size for an outgoing text stream.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct TextStreamOptions {
    destination_identity: String,
    topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    total_size: Option<u64>,
}

impl TextStreamOptions {
    pub fn new(destination_identity: impl Into<String>, topic: impl Into<String>) -> Self {
        Self {
            destination_identity: destination_identity.into(),
            topic: topic.into(),
            total_size: None,
        }
    }

    pub const fn with_total_size(mut self, total_size: u64) -> Self {
        self.total_size = Some(total_size);
        self
    }

    pub fn destination_identity(&self) -> &str {
        &self.destination_identity
    }

    pub fn topic(&self) -> &str {
        &self.topic
    }

    pub const fn total_size(&self) -> Option<u64> {
        self.total_size
    }
}

/// Topic and optional finite size for an outgoing byte stream.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ByteStreamOptions {
    destination_identity: String,
    topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    total_size: Option<u64>,
}

impl ByteStreamOptions {
    pub fn new(destination_identity: impl Into<String>, topic: impl Into<String>) -> Self {
        Self {
            destination_identity: destination_identity.into(),
            topic: topic.into(),
            total_size: None,
        }
    }

    pub const fn with_total_size(mut self, total_size: u64) -> Self {
        self.total_size = Some(total_size);
        self
    }

    pub fn destination_identity(&self) -> &str {
        &self.destination_identity
    }

    pub fn topic(&self) -> &str {
        &self.topic
    }

    pub const fn total_size(&self) -> Option<u64> {
        self.total_size
    }
}

/// Stable metadata for one text stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TextStreamInfo {
    id: String,
    topic: String,
    #[cfg_attr(test, ts(type = "number | null"))]
    size: Option<u64>,
    sender_identity: Option<String>,
}

impl TextStreamInfo {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn topic(&self) -> &str {
        &self.topic
    }

    pub const fn size(&self) -> Option<u64> {
        self.size
    }

    pub fn sender_identity(&self) -> Option<&str> {
        self.sender_identity.as_deref()
    }
}

/// Stable metadata for one byte stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ByteStreamInfo {
    id: String,
    topic: String,
    #[cfg_attr(test, ts(type = "number | null"))]
    size: Option<u64>,
    sender_identity: Option<String>,
}

impl ByteStreamInfo {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn topic(&self) -> &str {
        &self.topic
    }

    pub const fn size(&self) -> Option<u64> {
        self.size
    }

    pub fn sender_identity(&self) -> Option<&str> {
        self.sender_identity.as_deref()
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
struct AdapterOutgoingTextStream {
    id: u32,
    info: TextStreamInfo,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
struct AdapterOutgoingByteStream {
    id: u32,
    info: ByteStreamInfo,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum OutgoingStreamLifecycle {
    Open,
    Writing,
    Completing,
    Terminal,
}

struct OutgoingStreamInner {
    lifecycle: Cell<OutgoingStreamLifecycle>,
    session: Weak<SessionInner>,
    #[cfg(target_arch = "wasm32")]
    stream_id: u32,
}

impl OutgoingStreamInner {
    fn is_terminal(&self) -> bool {
        matches!(
            self.lifecycle.get(),
            OutgoingStreamLifecycle::Completing | OutgoingStreamLifecycle::Terminal
        ) || self
            .session
            .upgrade()
            .is_none_or(|session| session.lifecycle.get() != SessionLifecycle::Active)
    }

    fn begin_write(&self, operation: &'static str) -> Result<(), Error> {
        if self
            .session
            .upgrade()
            .is_none_or(|session| session.lifecycle.get() != SessionLifecycle::Active)
        {
            return Err(Error::InvalidState {
                operation,
                reason: "Room Session is terminal",
            });
        }
        if self.lifecycle.get() != OutgoingStreamLifecycle::Open {
            return Err(Error::InvalidState {
                operation,
                reason: "data stream is busy or terminal",
            });
        }
        self.lifecycle.set(OutgoingStreamLifecycle::Writing);
        Ok(())
    }

    fn begin_complete(&self, operation: &'static str) -> Result<(), Error> {
        if self
            .session
            .upgrade()
            .is_none_or(|session| session.lifecycle.get() != SessionLifecycle::Active)
        {
            return Err(Error::InvalidState {
                operation,
                reason: "Room Session is terminal",
            });
        }
        if self.lifecycle.get() != OutgoingStreamLifecycle::Open {
            return Err(Error::InvalidState {
                operation,
                reason: "data stream is busy or terminal",
            });
        }
        self.lifecycle.set(OutgoingStreamLifecycle::Completing);
        Ok(())
    }
}

impl Drop for OutgoingStreamInner {
    fn drop(&mut self) {
        if self.lifecycle.replace(OutgoingStreamLifecycle::Terminal)
            != OutgoingStreamLifecycle::Terminal
        {
            #[cfg(target_arch = "wasm32")]
            adapter::drop_outgoing_data_stream(self.stream_id);
        }
    }
}

#[cfg(target_arch = "wasm32")]
struct StreamOperationState {
    result: RefCell<Option<Result<(), Error>>>,
    waker: RefCell<Option<Waker>>,
}

#[cfg(target_arch = "wasm32")]
impl StreamOperationState {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            result: RefCell::new(None),
            waker: RefCell::new(None),
        })
    }

    fn complete(&self, result: Result<(), Error>) {
        self.result.borrow_mut().replace(result);
        if let Some(waker) = self.waker.borrow_mut().take() {
            waker.wake();
        }
    }
}

#[cfg(target_arch = "wasm32")]
struct StreamOperationFuture {
    state: Rc<StreamOperationState>,
}

#[cfg(target_arch = "wasm32")]
impl Future for StreamOperationFuture {
    type Output = Result<(), Error>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(result) = self.state.result.borrow_mut().take() else {
            self.state
                .waker
                .borrow_mut()
                .replace(context.waker().clone());
            return Poll::Pending;
        };
        Poll::Ready(result)
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for StreamOperationFuture {
    fn drop(&mut self) {
        self.state.waker.borrow_mut().take();
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy)]
enum StreamOperationKind {
    Write,
    Complete,
}

#[cfg(target_arch = "wasm32")]
fn start_stream_operation(
    inner: Rc<OutgoingStreamInner>,
    kind: StreamOperationKind,
    start: impl FnOnce(&js_sys::Function) -> Result<(), wasm_bindgen::JsValue>,
) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
    use wasm_bindgen::JsCast;

    let state = StreamOperationState::new();
    let callback_state = Rc::clone(&state);
    let callback_inner = Rc::clone(&inner);
    let callback = wasm_bindgen::closure::Closure::once_into_js(
        move |success: bool, value: wasm_bindgen::JsValue| {
            let result = if success {
                Ok(())
            } else {
                let error = AdapterError::from_cause(value);
                Err(match kind {
                    StreamOperationKind::Write => Error::DataStreamWriteFailed(error),
                    StreamOperationKind::Complete => Error::DataStreamCompletionFailed(error),
                })
            };
            match (kind, result.is_ok()) {
                (StreamOperationKind::Write, true) => {
                    callback_inner.lifecycle.set(OutgoingStreamLifecycle::Open)
                }
                _ => {
                    callback_inner
                        .lifecycle
                        .set(OutgoingStreamLifecycle::Terminal);
                    adapter::drop_outgoing_data_stream(callback_inner.stream_id);
                }
            }
            callback_state.complete(result);
        },
    );
    let callback: js_sys::Function = callback.unchecked_into();
    if let Err(cause) = start(&callback) {
        inner.lifecycle.set(OutgoingStreamLifecycle::Terminal);
        adapter::drop_outgoing_data_stream(inner.stream_id);
        let error = AdapterError::from_cause(cause);
        state.complete(Err(match kind {
            StreamOperationKind::Write => Error::DataStreamWriteFailed(error),
            StreamOperationKind::Complete => Error::DataStreamCompletionFailed(error),
        }));
    }
    Box::pin(StreamOperationFuture { state })
}

fn ready_stream_operation_error(
    error: Error,
) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
    Box::pin(async move { Err(error) })
}

/// An incrementally writable outgoing text stream.
pub struct OutgoingTextStream {
    inner: Rc<OutgoingStreamInner>,
    info: TextStreamInfo,
}

impl OutgoingTextStream {
    pub const fn info(&self) -> &TextStreamInfo {
        &self.info
    }

    pub fn is_terminal(&self) -> bool {
        self.inner.is_terminal()
    }

    /// Writes one bounded string and resolves after the destination pulls that exact chunk.
    pub fn write(
        &self,
        chunk: impl Into<String>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        let chunk = chunk.into();
        if chunk.len() > MAX_DATA_STREAM_CHUNK_BYTES {
            return ready_stream_operation_error(Error::InvalidState {
                operation: "write text stream",
                reason: "stream chunk exceeds 15000 UTF-8 bytes",
            });
        }
        if chunk.is_empty() {
            return Box::pin(async { Ok(()) });
        }
        if let Err(error) = self.inner.begin_write("write text stream") {
            return ready_stream_operation_error(error);
        }

        #[cfg(not(target_arch = "wasm32"))]
        return ready_stream_operation_error(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        {
            let inner = Rc::clone(&self.inner);
            start_stream_operation(
                Rc::clone(&inner),
                StreamOperationKind::Write,
                move |callback| adapter::write_text_stream(inner.stream_id, &chunk, callback),
            )
        }
    }

    /// Completes the stream after all prior writes and makes this handle terminal immediately.
    pub fn complete(&self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if let Err(error) = self.inner.begin_complete("complete text stream") {
            return ready_stream_operation_error(error);
        }

        #[cfg(not(target_arch = "wasm32"))]
        return ready_stream_operation_error(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        {
            let inner = Rc::clone(&self.inner);
            start_stream_operation(
                Rc::clone(&inner),
                StreamOperationKind::Complete,
                move |callback| adapter::complete_outgoing_data_stream(inner.stream_id, callback),
            )
        }
    }
}

/// An incrementally writable outgoing byte stream.
pub struct OutgoingByteStream {
    inner: Rc<OutgoingStreamInner>,
    info: ByteStreamInfo,
}

impl OutgoingByteStream {
    pub const fn info(&self) -> &ByteStreamInfo {
        &self.info
    }

    pub fn is_terminal(&self) -> bool {
        self.inner.is_terminal()
    }

    /// Writes one bounded byte chunk and resolves after the destination pulls that exact chunk.
    pub fn write(
        &self,
        chunk: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if chunk.len() > MAX_DATA_STREAM_CHUNK_BYTES {
            return ready_stream_operation_error(Error::InvalidState {
                operation: "write byte stream",
                reason: "stream chunk exceeds 15000 bytes",
            });
        }
        if chunk.is_empty() {
            return Box::pin(async { Ok(()) });
        }
        if let Err(error) = self.inner.begin_write("write byte stream") {
            return ready_stream_operation_error(error);
        }

        #[cfg(not(target_arch = "wasm32"))]
        return ready_stream_operation_error(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        {
            let inner = Rc::clone(&self.inner);
            let chunk = js_sys::Uint8Array::from(chunk.as_slice());
            start_stream_operation(
                Rc::clone(&inner),
                StreamOperationKind::Write,
                move |callback| adapter::write_byte_stream(inner.stream_id, &chunk, callback),
            )
        }
    }

    /// Completes the stream after all prior writes and makes this handle terminal immediately.
    pub fn complete(&self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if let Err(error) = self.inner.begin_complete("complete byte stream") {
            return ready_stream_operation_error(error);
        }

        #[cfg(not(target_arch = "wasm32"))]
        return ready_stream_operation_error(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        {
            let inner = Rc::clone(&self.inner);
            start_stream_operation(
                Rc::clone(&inner),
                StreamOperationKind::Complete,
                move |callback| adapter::complete_outgoing_data_stream(inner.stream_id, callback),
            )
        }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
struct AdapterTextStreamRead {
    done: bool,
    value: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
struct AdapterIncomingTextStream {
    id: u32,
    info: TextStreamInfo,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
struct AdapterIncomingByteStream {
    id: u32,
    info: ByteStreamInfo,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
struct AdapterByteStreamRead {
    done: bool,
    value: Option<Vec<u8>>,
}

/// A pull-driven incoming text stream.
///
/// Each successful pull acknowledges one chunk to its sender. Dropping or cancelling the stream
/// notifies the sender and drains the private SDK reader through its terminal trailer.
pub struct IncomingTextStream {
    info: TextStreamInfo,
    session: Weak<SessionInner>,
    terminal: bool,
    #[cfg(target_arch = "wasm32")]
    stream_id: u32,
    #[cfg(target_arch = "wasm32")]
    pending: Option<Pin<Box<wasm_bindgen_futures::JsFuture>>>,
}

impl IncomingTextStream {
    pub const fn info(&self) -> &TextStreamInfo {
        &self.info
    }

    pub fn is_terminal(&self) -> bool {
        self.terminal
            || self
                .session
                .upgrade()
                .is_none_or(|session| session.lifecycle.get() != SessionLifecycle::Active)
    }

    pub fn cancel(&mut self) {
        if !self.terminal {
            self.terminal = true;
            #[cfg(target_arch = "wasm32")]
            adapter::drop_incoming_data_stream(self.stream_id);
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl Stream for IncomingTextStream {
    type Item = Result<String, Error>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.is_terminal() {
            self.cancel();
            return Poll::Ready(None);
        }
        if self.pending.is_none() {
            let operation = match adapter::read_text_stream(self.stream_id) {
                Ok(operation) => operation,
                Err(cause) => {
                    self.terminal = true;
                    return Poll::Ready(Some(Err(Error::DataStreamReadFailed(
                        AdapterError::from_cause(cause),
                    ))));
                }
            };
            self.pending = Some(Box::pin(wasm_bindgen_futures::JsFuture::from(operation)));
        }

        let result = match self.pending.as_mut().unwrap().as_mut().poll(context) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(result) => result,
        };
        self.pending = None;
        let result = match result {
            Ok(value) => serde_wasm_bindgen::from_value::<AdapterTextStreamRead>(value)
                .map_err(|error| AdapterError::contract(error.to_string())),
            Err(cause) => Err(AdapterError::from_cause(cause)),
        };
        match result {
            Ok(result) if result.done => {
                self.terminal = true;
                adapter::finish_incoming_data_stream(self.stream_id);
                Poll::Ready(None)
            }
            Ok(result) => Poll::Ready(Some(Ok(result.value.unwrap_or_default()))),
            Err(error) => {
                self.terminal = true;
                adapter::drop_incoming_data_stream(self.stream_id);
                Poll::Ready(Some(Err(Error::DataStreamReadFailed(error))))
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Stream for IncomingTextStream {
    type Item = Result<String, Error>;

    fn poll_next(mut self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.terminal {
            Poll::Ready(None)
        } else {
            self.terminal = true;
            Poll::Ready(Some(Err(Error::UnsupportedTarget)))
        }
    }
}

impl Drop for IncomingTextStream {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// A pull-driven incoming byte stream.
///
/// Each successful pull acknowledges one chunk to its sender. Dropping or cancelling the stream
/// notifies the sender and drains the private SDK reader through its terminal trailer.
pub struct IncomingByteStream {
    info: ByteStreamInfo,
    session: Weak<SessionInner>,
    terminal: bool,
    #[cfg(target_arch = "wasm32")]
    stream_id: u32,
    #[cfg(target_arch = "wasm32")]
    pending: Option<Pin<Box<wasm_bindgen_futures::JsFuture>>>,
}

impl IncomingByteStream {
    pub const fn info(&self) -> &ByteStreamInfo {
        &self.info
    }

    pub fn is_terminal(&self) -> bool {
        self.terminal
            || self
                .session
                .upgrade()
                .is_none_or(|session| session.lifecycle.get() != SessionLifecycle::Active)
    }

    pub fn cancel(&mut self) {
        if !self.terminal {
            self.terminal = true;
            #[cfg(target_arch = "wasm32")]
            adapter::drop_incoming_data_stream(self.stream_id);
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl Stream for IncomingByteStream {
    type Item = Result<Vec<u8>, Error>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.is_terminal() {
            self.cancel();
            return Poll::Ready(None);
        }
        if self.pending.is_none() {
            let operation = match adapter::read_byte_stream(self.stream_id) {
                Ok(operation) => operation,
                Err(cause) => {
                    self.terminal = true;
                    return Poll::Ready(Some(Err(Error::DataStreamReadFailed(
                        AdapterError::from_cause(cause),
                    ))));
                }
            };
            self.pending = Some(Box::pin(wasm_bindgen_futures::JsFuture::from(operation)));
        }

        let result = match self.pending.as_mut().unwrap().as_mut().poll(context) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(result) => result,
        };
        self.pending = None;
        let result = match result {
            Ok(value) => serde_wasm_bindgen::from_value::<AdapterByteStreamRead>(value)
                .map_err(|error| AdapterError::contract(error.to_string())),
            Err(cause) => Err(AdapterError::from_cause(cause)),
        };
        match result {
            Ok(result) if result.done => {
                self.terminal = true;
                adapter::finish_incoming_data_stream(self.stream_id);
                Poll::Ready(None)
            }
            Ok(result) => Poll::Ready(Some(Ok(result.value.unwrap_or_default()))),
            Err(error) => {
                self.terminal = true;
                adapter::drop_incoming_data_stream(self.stream_id);
                Poll::Ready(Some(Err(Error::DataStreamReadFailed(error))))
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Stream for IncomingByteStream {
    type Item = Result<Vec<u8>, Error>;

    fn poll_next(mut self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.terminal {
            Poll::Ready(None)
        } else {
            self.terminal = true;
            Poll::Ready(Some(Err(Error::UnsupportedTarget)))
        }
    }
}

impl Drop for IncomingByteStream {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct StreamHandlerRegistrationInner {
    active: Rc<Cell<bool>>,
    session: Weak<SessionInner>,
    #[cfg(target_arch = "wasm32")]
    registration_id: u32,
    #[cfg(target_arch = "wasm32")]
    _callback: wasm_bindgen::closure::Closure<dyn FnMut(wasm_bindgen::JsValue)>,
}

impl StreamHandlerRegistrationInner {
    fn is_active(&self) -> bool {
        self.active.get()
            && self
                .session
                .upgrade()
                .is_some_and(|session| session.lifecycle.get() == SessionLifecycle::Active)
    }

    fn unregister(&self) {
        if self.active.replace(false) {
            #[cfg(target_arch = "wasm32")]
            adapter::drop_data_stream_handler(self.registration_id);
        }
    }
}

impl Drop for StreamHandlerRegistrationInner {
    fn drop(&mut self) {
        self.unregister();
    }
}

/// Registration guard for one Room Session text-stream topic.
pub struct TextStreamHandlerRegistration {
    inner: Rc<StreamHandlerRegistrationInner>,
}

impl TextStreamHandlerRegistration {
    pub fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    pub fn unregister(&self) {
        self.inner.unregister();
    }
}

/// Registration guard for one Room Session byte-stream topic.
pub struct ByteStreamHandlerRegistration {
    inner: Rc<StreamHandlerRegistrationInner>,
}

impl ByteStreamHandlerRegistration {
    pub fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    pub fn unregister(&self) {
        self.inner.unregister();
    }
}

#[cfg(target_arch = "wasm32")]
fn finish_text_stream_open(
    result: Result<AdapterOutgoingTextStream, AdapterError>,
) -> Result<AdapterOutgoingTextStream, Error> {
    result.map_err(classify_data_stream_open_error)
}

#[cfg(target_arch = "wasm32")]
fn finish_byte_stream_open(
    result: Result<AdapterOutgoingByteStream, AdapterError>,
) -> Result<AdapterOutgoingByteStream, Error> {
    result.map_err(classify_data_stream_open_error)
}

#[cfg(target_arch = "wasm32")]
fn drop_opened_text_stream(stream: AdapterOutgoingTextStream) {
    adapter::drop_outgoing_data_stream(stream.id);
}

#[cfg(target_arch = "wasm32")]
fn drop_opened_byte_stream(stream: AdapterOutgoingByteStream) {
    adapter::drop_outgoing_data_stream(stream.id);
}

#[cfg(target_arch = "wasm32")]
fn begin_text_stream_open(
    room_id: u32,
    options: TextStreamOptions,
) -> BrowserCaptureFuture<AdapterOutgoingTextStream, AdapterOutgoingTextStream> {
    use wasm_bindgen::JsCast;

    let state = BrowserCaptureState::new();
    let callback_state = Rc::clone(&state);
    let callback = wasm_bindgen::closure::Closure::once_into_js(
        move |success: bool, value: wasm_bindgen::JsValue| {
            let result = if success {
                serde_wasm_bindgen::from_value(value.clone()).map_err(|error| {
                    adapter::drop_outgoing_data_stream_descriptor(value);
                    AdapterError::contract(error.to_string())
                })
            } else {
                Err(AdapterError::from_cause(value))
            };
            complete_browser_capture(&callback_state, result, drop_opened_text_stream);
        },
    );
    let callback: js_sys::Function = callback.unchecked_into();
    let options = match serde_wasm_bindgen::to_value(&options) {
        Ok(options) => options,
        Err(error) => {
            state
                .result
                .borrow_mut()
                .replace(Err(AdapterError::contract(error.to_string())));
            return BrowserCaptureFuture {
                state,
                finish: finish_text_stream_open,
                cleanup: drop_opened_text_stream,
            };
        }
    };
    if let Err(cause) = adapter::open_text_stream(room_id, &options, &callback) {
        state
            .result
            .borrow_mut()
            .replace(Err(AdapterError::from_cause(cause)));
    }
    BrowserCaptureFuture {
        state,
        finish: finish_text_stream_open,
        cleanup: drop_opened_text_stream,
    }
}

#[cfg(target_arch = "wasm32")]
fn begin_byte_stream_open(
    room_id: u32,
    options: ByteStreamOptions,
) -> BrowserCaptureFuture<AdapterOutgoingByteStream, AdapterOutgoingByteStream> {
    use wasm_bindgen::JsCast;

    let state = BrowserCaptureState::new();
    let callback_state = Rc::clone(&state);
    let callback = wasm_bindgen::closure::Closure::once_into_js(
        move |success: bool, value: wasm_bindgen::JsValue| {
            let result = if success {
                serde_wasm_bindgen::from_value(value.clone()).map_err(|error| {
                    adapter::drop_outgoing_data_stream_descriptor(value);
                    AdapterError::contract(error.to_string())
                })
            } else {
                Err(AdapterError::from_cause(value))
            };
            complete_browser_capture(&callback_state, result, drop_opened_byte_stream);
        },
    );
    let callback: js_sys::Function = callback.unchecked_into();
    let options = match serde_wasm_bindgen::to_value(&options) {
        Ok(options) => options,
        Err(error) => {
            state
                .result
                .borrow_mut()
                .replace(Err(AdapterError::contract(error.to_string())));
            return BrowserCaptureFuture {
                state,
                finish: finish_byte_stream_open,
                cleanup: drop_opened_byte_stream,
            };
        }
    };
    if let Err(cause) = adapter::open_byte_stream(room_id, &options, &callback) {
        state
            .result
            .borrow_mut()
            .replace(Err(AdapterError::from_cause(cause)));
    }
    BrowserCaptureFuture {
        state,
        finish: finish_byte_stream_open,
        cleanup: drop_opened_byte_stream,
    }
}

/// The delivery mode LiveKit uses for a data packet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DataPacketDelivery {
    /// Retransmit lost packets and preserve ordering at the cost of head-of-line blocking.
    Reliable,
    /// Prefer low latency by allowing packets to be lost.
    Lossy,
}

/// One native byte packet received from another participant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataPacket {
    payload: Vec<u8>,
    sender_identity: Option<String>,
    delivery: DataPacketDelivery,
}

impl DataPacket {
    /// Returns the packet payload without applying a codec or text conversion.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Returns the sending Participant Identity when LiveKit supplied one.
    pub fn sender_identity(&self) -> Option<&str> {
        self.sender_identity.as_deref()
    }

    /// Returns the packet's LiveKit delivery mode.
    pub const fn delivery(&self) -> DataPacketDelivery {
        self.delivery
    }
}

/// One observation from a bounded data-packet subscription.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DataPacketStreamItem {
    /// A native byte packet received in the subscription's delivery mode.
    Packet(DataPacket),
    /// This subscriber fell behind and the oldest buffered packets were dropped.
    Lagged {
        /// The exact number of packets dropped since the previous stream item.
        dropped: usize,
    },
}

struct BoundedQueue<T> {
    #[cfg(any(target_arch = "wasm32", test))]
    capacity: usize,
    items: VecDeque<T>,
    dropped: usize,
    closed: bool,
    waker: Option<Waker>,
}

impl<T> BoundedQueue<T> {
    fn poll_next_mapped<U>(
        &mut self,
        context: &mut Context<'_>,
        item: impl FnOnce(T) -> U,
        lagged: impl FnOnce(usize) -> U,
    ) -> Poll<Option<U>> {
        if self.dropped > 0 {
            let dropped = std::mem::take(&mut self.dropped);
            return Poll::Ready(Some(lagged(dropped)));
        }
        if let Some(value) = self.items.pop_front() {
            return Poll::Ready(Some(item(value)));
        }
        if self.closed {
            return Poll::Ready(None);
        }

        self.waker = Some(context.waker().clone());
        Poll::Pending
    }

    #[cfg(any(target_arch = "wasm32", test))]
    fn push(&mut self, item: T) {
        if self.items.len() == self.capacity {
            self.items.pop_front();
            self.dropped += 1;
        }
        self.items.push_back(item);

        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }

    fn close(&mut self) {
        self.closed = true;
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

fn close_bounded_queues<T>(queues: impl IntoIterator<Item = Weak<RefCell<BoundedQueue<T>>>>) {
    for queue in queues {
        if let Some(queue) = queue.upgrade() {
            queue.borrow_mut().close();
        }
    }
}

type DataPacketQueue = BoundedQueue<DataPacket>;

/// A browser-thread-local, bounded stream of packets for one delivery mode.
pub struct DataPacketStream {
    queue: Rc<RefCell<DataPacketQueue>>,
}

impl Stream for DataPacketStream {
    type Item = DataPacketStreamItem;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.queue
            .borrow_mut()
            .poll_next_mapped(context, DataPacketStreamItem::Packet, |dropped| {
                DataPacketStreamItem::Lagged { dropped }
            })
    }
}

struct DataPacketSubscription {
    #[cfg(any(target_arch = "wasm32", test))]
    delivery: DataPacketDelivery,
    queue: Weak<RefCell<DataPacketQueue>>,
}

/// One chat message sent through LiveKit's participant messaging workflow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatMessage {
    id: String,
    message: String,
    sender_identity: Option<String>,
    timestamp: SystemTime,
    edit_timestamp: Option<SystemTime>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Deserialize)]
struct AdapterChatMessage {
    id: String,
    message: String,
    sender_identity: Option<String>,
    timestamp_millis: u64,
    edit_timestamp_millis: Option<u64>,
}

#[cfg(target_arch = "wasm32")]
impl From<AdapterChatMessage> for ChatMessage {
    fn from(message: AdapterChatMessage) -> Self {
        Self {
            id: message.id,
            message: message.message,
            sender_identity: message.sender_identity,
            timestamp: std::time::UNIX_EPOCH + Duration::from_millis(message.timestamp_millis),
            edit_timestamp: message
                .edit_timestamp_millis
                .map(|timestamp| std::time::UNIX_EPOCH + Duration::from_millis(timestamp)),
        }
    }
}

impl ChatMessage {
    /// Returns LiveKit's stable identifier for this message.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the message content as its native string.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the sending Participant Identity when LiveKit supplied one.
    pub fn sender_identity(&self) -> Option<&str> {
        self.sender_identity.as_deref()
    }

    /// Returns the time at which LiveKit says the message was created.
    pub const fn timestamp(&self) -> SystemTime {
        self.timestamp
    }

    /// Returns the time of the latest LiveKit chat edit, when present.
    pub const fn edit_timestamp(&self) -> Option<SystemTime> {
        self.edit_timestamp
    }
}

/// One observation from a bounded chat-message subscription.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ChatMessageStreamItem {
    /// A chat message sent by the Local Participant or a Remote Participant.
    Message(ChatMessage),
    /// This subscriber fell behind and the oldest buffered messages were dropped.
    Lagged {
        /// The exact number of messages dropped since the previous stream item.
        dropped: usize,
    },
}

/// A browser-thread-local, bounded stream of chat messages.
pub struct ChatMessageStream {
    queue: Rc<RefCell<ChatMessageQueue>>,
}

impl Stream for ChatMessageStream {
    type Item = ChatMessageStreamItem;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.queue.borrow_mut().poll_next_mapped(
            context,
            ChatMessageStreamItem::Message,
            |dropped| ChatMessageStreamItem::Lagged { dropped },
        )
    }
}

type ChatMessageQueue = BoundedQueue<ChatMessage>;

struct ChatMessageSubscription {
    queue: Weak<RefCell<ChatMessageQueue>>,
}

/// One LiveKit-normalized update to a speech transcription segment.
///
/// Successive interim and final updates retain the same segment ID. Participant Identity and
/// Track Publication SID are optional because LiveKit may not resolve their browser objects.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranscriptionSegment {
    id: String,
    participant_identity: Option<String>,
    track_publication_sid: Option<String>,
    text: String,
    start_time: u64,
    end_time: u64,
    language: String,
    is_final: bool,
    first_received_at: SystemTime,
    last_received_at: SystemTime,
}

impl TranscriptionSegment {
    /// Returns LiveKit's stable identity for this segment across interim and final updates.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the transcribed Participant Identity when LiveKit resolved it.
    pub fn participant_identity(&self) -> Option<&str> {
        self.participant_identity.as_deref()
    }

    /// Returns the transcribed Track Publication SID when LiveKit resolved it.
    pub fn track_publication_sid(&self) -> Option<&str> {
        self.track_publication_sid.as_deref()
    }

    /// Returns the latest text for this segment.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns LiveKit's normalized start-time integer without inventing units not specified by
    /// the pinned SDK.
    pub const fn start_time(&self) -> u64 {
        self.start_time
    }

    /// Returns LiveKit's normalized end-time integer without inventing units not specified by the
    /// pinned SDK.
    pub const fn end_time(&self) -> u64 {
        self.end_time
    }

    /// Returns the BCP-47 language tag supplied by LiveKit, or an empty string when absent.
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Whether this update completes the segment rather than replacing interim text.
    pub const fn is_final(&self) -> bool {
        self.is_final
    }

    /// Returns when LiveKit first normalized this segment in the local browser.
    pub const fn first_received_at(&self) -> SystemTime {
        self.first_received_at
    }

    /// Returns when LiveKit normalized this specific update in the local browser.
    pub const fn last_received_at(&self) -> SystemTime {
        self.last_received_at
    }
}

/// One observation from a bounded transcription-segment subscription.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TranscriptionSegmentStreamItem {
    /// An interim or final transcription segment update.
    Segment(TranscriptionSegment),
    /// This subscriber fell behind and the oldest buffered updates were dropped.
    Lagged {
        /// The exact number of updates dropped since the previous stream item.
        dropped: usize,
    },
}

type TranscriptionSegmentQueue = BoundedQueue<TranscriptionSegment>;

/// A browser-thread-local, bounded stream of transcription segment updates.
pub struct TranscriptionSegmentStream {
    queue: Rc<RefCell<TranscriptionSegmentQueue>>,
}

impl Stream for TranscriptionSegmentStream {
    type Item = TranscriptionSegmentStreamItem;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.queue.borrow_mut().poll_next_mapped(
            context,
            TranscriptionSegmentStreamItem::Segment,
            |dropped| TranscriptionSegmentStreamItem::Lagged { dropped },
        )
    }
}

struct TranscriptionSegmentSubscription {
    queue: Weak<RefCell<TranscriptionSegmentQueue>>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
struct AdapterTranscriptionSegment {
    id: String,
    participant_identity: Option<String>,
    track_publication_sid: Option<String>,
    text: String,
    start_time: u64,
    end_time: u64,
    language: String,
    is_final: bool,
    first_received_at_millis: u64,
    last_received_at_millis: u64,
}

#[cfg(any(target_arch = "wasm32", test))]
impl AdapterTranscriptionSegment {
    fn into_segment(self) -> Option<TranscriptionSegment> {
        Some(TranscriptionSegment {
            id: self.id,
            participant_identity: self.participant_identity,
            track_publication_sid: self.track_publication_sid,
            text: self.text,
            start_time: self.start_time,
            end_time: self.end_time,
            language: self.language,
            is_final: self.is_final,
            first_received_at: std::time::UNIX_EPOCH
                .checked_add(Duration::from_millis(self.first_received_at_millis))?,
            last_received_at: std::time::UNIX_EPOCH
                .checked_add(Duration::from_millis(self.last_received_at_millis))?,
        })
    }
}

/// The state change represented by a Room Event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "camelCase")]
#[non_exhaustive]
pub enum RoomEventKind {
    /// The Room Session's connection state changed.
    ConnectionStateChanged { state: RoomConnectionState },
    /// Local Participant identity, SID, metadata, attributes, or grants changed.
    LocalParticipantChanged,
    /// The Local Participant's Track Publications changed.
    LocalTrackPublicationsChanged,
    /// A Remote Participant became visible in this Room Session.
    RemoteParticipantJoined { identity: String },
    /// A Remote Participant's metadata or attributes changed.
    RemoteParticipantChanged { identity: String },
    /// LiveKit's connection-quality estimate changed for a participant.
    ParticipantConnectionQualityChanged {
        participant_identity: String,
        quality: ConnectionQuality,
    },
    /// LiveKit's ordered active-speaker set changed.
    ActiveSpeakersChanged,
    /// A Remote Participant ceased to be visible in this Room Session.
    RemoteParticipantLeft { identity: String },
    /// A Remote Participant's Track Publications or their availability changed.
    RemoteTrackPublicationsChanged { participant_identity: String },
    /// A Remote Participant's separately published Data Tracks changed.
    RemoteDataTracksChanged { participant_identity: String },
    /// Room metadata changed.
    RoomMetadataChanged,
    /// Browser autoplay policy for remote audio changed.
    AudioPlaybackChanged { state: AudioPlaybackState },
    /// This subscriber fell behind and must recover from `RoomSession::snapshot`.
    Lagged,
}

/// A bounded state notification tagged with the snapshot installed before it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoomEvent {
    revision: u64,
    kind: RoomEventKind,
}

impl RoomEvent {
    /// Returns the revision of the snapshot installed before this event.
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the state change represented by this event.
    pub const fn kind(&self) -> &RoomEventKind {
        &self.kind
    }
}

/// A browser-thread-local, bounded stream of Room Events.
pub struct RoomEventStream {
    queue: Rc<RefCell<EventQueue>>,
}

impl Stream for RoomEventStream {
    type Item = RoomEvent;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut queue = self.queue.borrow_mut();
        if let Some(revision) = queue.lagged_revision.take() {
            return Poll::Ready(Some(RoomEvent {
                revision,
                kind: RoomEventKind::Lagged,
            }));
        }
        if let Some(event) = queue.events.pop_front() {
            return Poll::Ready(Some(event));
        }
        if queue.closed {
            return Poll::Ready(None);
        }

        queue.waker = Some(context.waker().clone());
        Poll::Pending
    }
}

struct EventQueue {
    capacity: usize,
    events: VecDeque<RoomEvent>,
    lagged_revision: Option<u64>,
    closed: bool,
    waker: Option<Waker>,
}

impl EventQueue {
    fn push(&mut self, event: RoomEvent) {
        if self.lagged_revision.is_some() || self.events.len() == self.capacity {
            self.events.clear();
            self.lagged_revision = Some(event.revision);
        } else {
            self.events.push_back(event);
        }

        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

/// A cloneable handle to one Local Participant's room participation lifetime.
///
/// This handle is deliberately neither `Send` nor `Sync`.
#[derive(Clone)]
pub struct RoomSession {
    inner: Rc<SessionInner>,
}

impl PartialEq for RoomSession {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for RoomSession {}

/// A command handle for the Local Participant in one Room Session.
///
/// This handle shares the Room Session lifetime and is deliberately neither `Send` nor `Sync`.
#[derive(Clone)]
pub struct LocalParticipant {
    inner: Rc<SessionInner>,
}

impl LocalParticipant {
    /// Returns the unstable `livekit-client` Local Participant object.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_participant(&self) -> Result<wasm_bindgen::JsValue, Error> {
        if self.inner.lifecycle.get() != SessionLifecycle::Active {
            return Err(Error::InvalidState {
                operation: "access raw Local Participant",
                reason: "Room Session is terminal",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        raw_js_handle(adapter::raw_local_participant(self.inner.room_id))
    }

    /// Sends a native string through LiveKit's chat workflow.
    ///
    /// The browser operation starts before the returned local future is polled. Success returns
    /// LiveKit's message identifier and timing information; it is not a remote delivery
    /// acknowledgement. The same sent message is also delivered to local chat subscriptions.
    pub fn send_chat_message(
        &self,
        _message: impl Into<String>,
    ) -> Pin<Box<dyn Future<Output = Result<ChatMessage, Error>> + 'static>> {
        if self.inner.lifecycle.get() != SessionLifecycle::Active {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "send chat message",
                    reason: "Room Session is terminal",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let operation = adapter::send_chat_message(self.inner.room_id, &_message.into())
                .map_err(AdapterError::from_cause)
                .map_err(classify_chat_message_send_error);
            Box::pin(async move {
                let operation = operation?;
                let value = wasm_bindgen_futures::JsFuture::from(operation)
                    .await
                    .map_err(AdapterError::from_cause)
                    .map_err(classify_chat_message_send_error)?;
                serde_wasm_bindgen::from_value::<AdapterChatMessage>(value)
                    .map(ChatMessage::from)
                    .map_err(|error| AdapterError::contract(error.to_string()))
                    .map_err(classify_chat_message_send_error)
            })
        }
    }

    /// Invokes a registered method on one Remote Participant.
    pub fn invoke_rpc(
        &self,
        options: RpcOptions,
    ) -> Pin<Box<dyn Future<Output = Result<RpcPayload, Error>> + 'static>> {
        if self.inner.lifecycle.get() != SessionLifecycle::Active {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "invoke participant RPC",
                    reason: "Room Session is terminal",
                })
            });
        }
        if options.destination_identity.is_empty() || options.method.is_empty() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "invoke participant RPC",
                    reason: "destination Participant Identity and method cannot be empty",
                })
            });
        }
        if encode_rpc_payload(&options.payload).len() > MAX_RPC_PAYLOAD_BYTES {
            return Box::pin(async {
                Err(Error::RpcInvocationFailed(RpcFailure {
                    code: Some(1402),
                    message: "request payload too large".into(),
                    data: None,
                    cause: None,
                }))
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        start_rpc_invocation(self.inner.room_id, options)
    }

    /// Opens an incrementally writable reliable text stream.
    pub fn open_text_stream(
        &self,
        _options: TextStreamOptions,
    ) -> Pin<Box<dyn Future<Output = Result<OutgoingTextStream, Error>> + 'static>> {
        if self.inner.lifecycle.get() != SessionLifecycle::Active {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "open text stream",
                    reason: "Room Session is terminal",
                })
            });
        }
        if _options.topic.is_empty() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "open text stream",
                    reason: "stream topic cannot be empty",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let session = Rc::downgrade(&self.inner);
            let opening = begin_text_stream_open(self.inner.room_id, _options);
            Box::pin(async move {
                let stream = opening.await?;
                if session
                    .upgrade()
                    .is_none_or(|session| session.lifecycle.get() != SessionLifecycle::Active)
                {
                    adapter::drop_outgoing_data_stream(stream.id);
                    return Err(Error::InvalidState {
                        operation: "open text stream",
                        reason: "Room Session is terminal",
                    });
                }
                Ok(OutgoingTextStream {
                    info: stream.info,
                    inner: Rc::new(OutgoingStreamInner {
                        lifecycle: Cell::new(OutgoingStreamLifecycle::Open),
                        session,
                        stream_id: stream.id,
                    }),
                })
            })
        }
    }

    /// Opens an incrementally writable reliable byte stream.
    pub fn open_byte_stream(
        &self,
        _options: ByteStreamOptions,
    ) -> Pin<Box<dyn Future<Output = Result<OutgoingByteStream, Error>> + 'static>> {
        if self.inner.lifecycle.get() != SessionLifecycle::Active {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "open byte stream",
                    reason: "Room Session is terminal",
                })
            });
        }
        if _options.topic.is_empty() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "open byte stream",
                    reason: "stream topic cannot be empty",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let session = Rc::downgrade(&self.inner);
            let opening = begin_byte_stream_open(self.inner.room_id, _options);
            Box::pin(async move {
                let stream = opening.await?;
                if session
                    .upgrade()
                    .is_none_or(|session| session.lifecycle.get() != SessionLifecycle::Active)
                {
                    adapter::drop_outgoing_data_stream(stream.id);
                    return Err(Error::InvalidState {
                        operation: "open byte stream",
                        reason: "Room Session is terminal",
                    });
                }
                Ok(OutgoingByteStream {
                    info: stream.info,
                    inner: Rc::new(OutgoingStreamInner {
                        lifecycle: Cell::new(OutgoingStreamLifecycle::Open),
                        session,
                        stream_id: stream.id,
                    }),
                })
            })
        }
    }

    /// Sends native bytes to the other participants through LiveKit.
    ///
    /// The browser operation starts before the returned local future is polled. `Reliable` asks
    /// LiveKit to retransmit and preserve ordering, while `Lossy` prioritizes latency and may drop
    /// packets. Success means the SDK accepted the send; it is not a delivery acknowledgement.
    pub fn send_data_packet(
        &self,
        _payload: Vec<u8>,
        _delivery: DataPacketDelivery,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if self.inner.lifecycle.get() != SessionLifecycle::Active {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "send data packet",
                    reason: "Room Session is terminal",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let payload = js_sys::Uint8Array::from(_payload.as_slice());
            let operation = adapter::send_data_packet(
                self.inner.room_id,
                &payload,
                _delivery == DataPacketDelivery::Reliable,
            )
            .map_err(AdapterError::from_cause)
            .map_err(classify_data_packet_send_error);
            Box::pin(async move {
                let operation = operation?;
                wasm_bindgen_futures::JsFuture::from(operation)
                    .await
                    .map(|_| ())
                    .map_err(AdapterError::from_cause)
                    .map_err(classify_data_packet_send_error)
            })
        }
    }

    /// Updates the Local Participant metadata stored by LiveKit.
    ///
    /// The browser operation starts before the returned local future is polled. The current Room
    /// Snapshot remains authoritative and is not changed optimistically; a successful update
    /// becomes observable through a later `RoomEventKind::LocalParticipantChanged` revision.
    /// The pinned LiveKit server does not support clearing metadata, so an empty value returns
    /// `Error::ParticipantUpdateFailed` without changing the snapshot.
    pub fn set_metadata(
        &self,
        _metadata: impl Into<String>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if self.inner.lifecycle.get() != SessionLifecycle::Active {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "update Local Participant metadata",
                    reason: "Room Session is terminal",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let metadata = _metadata.into();
            participant_update_operation(
                adapter::set_local_participant_metadata(self.inner.room_id, &metadata)
                    .map_err(AdapterError::from_cause)
                    .map_err(classify_participant_update_error),
            )
        }
    }

    /// Applies a Local Participant attribute patch through LiveKit.
    ///
    /// Keys omitted from the map remain unchanged, while an empty value removes its key. The
    /// current Room Snapshot remains authoritative and is not changed optimistically; successful
    /// updates become observable through a later `RoomEventKind::LocalParticipantChanged`
    /// revision.
    pub fn update_attributes(
        &self,
        _attributes: BTreeMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if self.inner.lifecycle.get() != SessionLifecycle::Active {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "update Local Participant attributes",
                    reason: "Room Session is terminal",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let attributes = match _attributes
                .serialize(&serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true))
            {
                Ok(attributes) => attributes,
                Err(error) => {
                    let error = Error::AdapterFailure(AdapterError::contract(error.to_string()));
                    return Box::pin(async move { Err(error) });
                }
            };
            participant_update_operation(
                adapter::update_local_participant_attributes(self.inner.room_id, &attributes)
                    .map_err(AdapterError::from_cause)
                    .map_err(classify_participant_update_error),
            )
        }
    }
}

impl RoomSession {
    /// Returns the unstable `livekit-client` Room object for this Room Session.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_room(&self) -> Result<wasm_bindgen::JsValue, Error> {
        if self.inner.lifecycle.get() != SessionLifecycle::Active {
            return Err(Error::InvalidState {
                operation: "access raw Room Session",
                reason: "Room Session is terminal",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        raw_js_handle(adapter::raw_room(self.inner.room_id))
    }

    /// Registers one browser-thread-local Participant RPC method handler.
    ///
    /// At most [`DEFAULT_RPC_HANDLER_CAPACITY`] invocations run concurrently for this method;
    /// excess invocations fail immediately with Participant RPC application-error code 1500.
    pub fn register_rpc_method<F, Fut>(
        &self,
        method: impl Into<String>,
        handler: F,
    ) -> Result<RpcMethodRegistration, Error>
    where
        F: FnMut(RpcInvocation) -> Fut + 'static,
        Fut: Future<Output = Result<RpcPayload, RpcApplicationError>> + 'static,
    {
        self.register_rpc_method_with_capacity(method, DEFAULT_RPC_HANDLER_CAPACITY, handler)
    }

    /// Registers one browser-thread-local Participant RPC method with bounded concurrency.
    ///
    /// Invocations received while `capacity` handlers are running fail immediately with a remote
    /// application error. A zero capacity is normalized to one.
    pub fn register_rpc_method_with_capacity<F, Fut>(
        &self,
        method: impl Into<String>,
        _capacity: usize,
        _handler: F,
    ) -> Result<RpcMethodRegistration, Error>
    where
        F: FnMut(RpcInvocation) -> Fut + 'static,
        Fut: Future<Output = Result<RpcPayload, RpcApplicationError>> + 'static,
    {
        let method = method.into();
        if self.inner.lifecycle.get() != SessionLifecycle::Active {
            return Err(Error::InvalidState {
                operation: "register participant RPC method",
                reason: "Room Session is terminal",
            });
        }
        if method.is_empty() {
            return Err(Error::InvalidState {
                operation: "register participant RPC method",
                reason: "RPC method cannot be empty",
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;

            let mut handler = _handler;
            let session = Rc::downgrade(&self.inner);
            let callback_session = session.clone();
            let active = Rc::new(Cell::new(true));
            let callback_active = Rc::clone(&active);
            let capacity = RpcHandlerCapacity::new(_capacity);
            let callback =
                wasm_bindgen::closure::Closure::new(move |value: wasm_bindgen::JsValue| {
                    let descriptor =
                        match serde_wasm_bindgen::from_value::<AdapterRpcInvocation>(value.clone())
                        {
                            Ok(descriptor) => descriptor,
                            Err(_) => {
                                adapter::drop_participant_rpc_invocation_descriptor(value);
                                return;
                            }
                        };
                    let Some(session) = callback_session.upgrade() else {
                        adapter::drop_participant_rpc_invocation(descriptor.id);
                        return;
                    };
                    if !callback_active.get() || session.lifecycle.get() != SessionLifecycle::Active
                    {
                        complete_rpc_handler(
                            descriptor.id,
                            AdapterRpcHandlerCompletion {
                                success: false,
                                payload: None,
                                code: Some(1400),
                                message: Some(
                                    "Participant RPC method is no longer registered".into(),
                                ),
                                data: None,
                            },
                        );
                        return;
                    }

                    let Some(permit) = capacity.try_acquire() else {
                        complete_rpc_handler(
                            descriptor.id,
                            AdapterRpcHandlerCompletion {
                                success: false,
                                payload: None,
                                code: Some(1500),
                                message: Some("Participant RPC handler capacity exhausted".into()),
                                data: Some("Participant RPC handler capacity exhausted".into()),
                            },
                        );
                        return;
                    };

                    let invocation_id = descriptor.id;
                    let invocation = RpcInvocation {
                        request_id: descriptor.request_id,
                        caller_identity: descriptor.caller_identity,
                        payload: decode_rpc_payload(descriptor.payload),
                        response_timeout: Duration::from_millis(descriptor.response_timeout_ms),
                    };
                    let (abort, registration) = futures_util::future::AbortHandle::new_pair();
                    let task = Rc::new(RpcHandlerTask {
                        invocation_id,
                        abort,
                    });
                    session
                        .rpc_handler_tasks
                        .borrow_mut()
                        .push(Rc::downgrade(&task));
                    let operation = handler(invocation);
                    let task_session = Rc::downgrade(&session);
                    wasm_bindgen_futures::spawn_local(async move {
                        let _permit = permit;
                        let result =
                            futures_util::future::Abortable::new(operation, registration).await;
                        let _task = task;
                        match result {
                            Ok(Ok(payload)) => {
                                let payload = encode_rpc_payload(&payload);
                                if payload.len() > MAX_RPC_PAYLOAD_BYTES {
                                    complete_rpc_handler(
                                        invocation_id,
                                        AdapterRpcHandlerCompletion {
                                            success: false,
                                            payload: None,
                                            code: Some(1504),
                                            message: Some("response payload too large".into()),
                                            data: None,
                                        },
                                    );
                                } else {
                                    complete_rpc_handler(
                                        invocation_id,
                                        AdapterRpcHandlerCompletion {
                                            success: true,
                                            payload: Some(payload),
                                            code: None,
                                            message: None,
                                            data: None,
                                        },
                                    );
                                }
                            }
                            Ok(Err(error)) => complete_rpc_handler(
                                invocation_id,
                                AdapterRpcHandlerCompletion {
                                    success: false,
                                    payload: None,
                                    code: Some(error.code),
                                    message: Some(error.message),
                                    data: error.data,
                                },
                            ),
                            Err(_) => adapter::drop_participant_rpc_invocation(invocation_id),
                        }
                        if let Some(session) = task_session.upgrade() {
                            session.rpc_handler_tasks.borrow_mut().retain(|task| {
                                task.upgrade()
                                    .is_some_and(|task| task.invocation_id != invocation_id)
                            });
                        }
                    });
                });
            let registration_id = adapter::register_participant_rpc_method(
                self.inner.room_id,
                &method,
                callback.as_ref().unchecked_ref(),
            )
            .map_err(AdapterError::from_cause)
            .map_err(Error::RpcMethodRegistrationFailed)?;
            Ok(RpcMethodRegistration {
                inner: Rc::new(RpcMethodRegistrationInner {
                    active,
                    session,
                    registration_id,
                    _callback: callback,
                }),
            })
        }
    }

    /// Registers the single handler for an incoming text-stream topic.
    pub fn register_text_stream_handler(
        &self,
        topic: impl Into<String>,
        _handler: impl FnMut(IncomingTextStream) + 'static,
    ) -> Result<TextStreamHandlerRegistration, Error> {
        let topic = topic.into();
        if self.inner.lifecycle.get() != SessionLifecycle::Active {
            return Err(Error::InvalidState {
                operation: "register text stream handler",
                reason: "Room Session is terminal",
            });
        }
        if topic.is_empty() {
            return Err(Error::InvalidState {
                operation: "register text stream handler",
                reason: "stream topic cannot be empty",
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;

            let mut handler = _handler;
            let session = Rc::downgrade(&self.inner);
            let callback_session = session.clone();
            let active = Rc::new(Cell::new(true));
            let callback_active = Rc::clone(&active);
            let callback =
                wasm_bindgen::closure::Closure::new(move |value: wasm_bindgen::JsValue| {
                    if !callback_active.get()
                        || callback_session.upgrade().is_none_or(|session| {
                            session.lifecycle.get() != SessionLifecycle::Active
                        })
                    {
                        adapter::drop_incoming_data_stream_descriptor(value);
                        return;
                    }
                    let stream = match serde_wasm_bindgen::from_value::<AdapterIncomingTextStream>(
                        value.clone(),
                    ) {
                        Ok(stream) => stream,
                        Err(_) => {
                            adapter::drop_incoming_data_stream_descriptor(value);
                            return;
                        }
                    };
                    handler(IncomingTextStream {
                        info: stream.info,
                        session: callback_session.clone(),
                        terminal: false,
                        stream_id: stream.id,
                        pending: None,
                    });
                });
            let registration_id = adapter::register_text_stream_handler(
                self.inner.room_id,
                &topic,
                callback.as_ref().unchecked_ref(),
            )
            .map_err(AdapterError::from_cause)
            .map_err(Error::DataStreamHandlerFailed)?;
            Ok(TextStreamHandlerRegistration {
                inner: Rc::new(StreamHandlerRegistrationInner {
                    active,
                    session,
                    registration_id,
                    _callback: callback,
                }),
            })
        }
    }

    /// Registers the single handler for an incoming byte-stream topic.
    pub fn register_byte_stream_handler(
        &self,
        topic: impl Into<String>,
        _handler: impl FnMut(IncomingByteStream) + 'static,
    ) -> Result<ByteStreamHandlerRegistration, Error> {
        let topic = topic.into();
        if self.inner.lifecycle.get() != SessionLifecycle::Active {
            return Err(Error::InvalidState {
                operation: "register byte stream handler",
                reason: "Room Session is terminal",
            });
        }
        if topic.is_empty() {
            return Err(Error::InvalidState {
                operation: "register byte stream handler",
                reason: "stream topic cannot be empty",
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;

            let mut handler = _handler;
            let session = Rc::downgrade(&self.inner);
            let callback_session = session.clone();
            let active = Rc::new(Cell::new(true));
            let callback_active = Rc::clone(&active);
            let callback =
                wasm_bindgen::closure::Closure::new(move |value: wasm_bindgen::JsValue| {
                    if !callback_active.get()
                        || callback_session.upgrade().is_none_or(|session| {
                            session.lifecycle.get() != SessionLifecycle::Active
                        })
                    {
                        adapter::drop_incoming_data_stream_descriptor(value);
                        return;
                    }
                    let stream = match serde_wasm_bindgen::from_value::<AdapterIncomingByteStream>(
                        value.clone(),
                    ) {
                        Ok(stream) => stream,
                        Err(_) => {
                            adapter::drop_incoming_data_stream_descriptor(value);
                            return;
                        }
                    };
                    handler(IncomingByteStream {
                        info: stream.info,
                        session: callback_session.clone(),
                        terminal: false,
                        stream_id: stream.id,
                        pending: None,
                    });
                });
            let registration_id = adapter::register_byte_stream_handler(
                self.inner.room_id,
                &topic,
                callback.as_ref().unchecked_ref(),
            )
            .map_err(AdapterError::from_cause)
            .map_err(Error::DataStreamHandlerFailed)?;
            Ok(ByteStreamHandlerRegistration {
                inner: Rc::new(StreamHandlerRegistrationInner {
                    active,
                    session,
                    registration_id,
                    _callback: callback,
                }),
            })
        }
    }

    /// Returns the latest immutable Room Snapshot.
    pub fn snapshot(&self) -> Rc<RoomSnapshot> {
        Rc::clone(&self.inner.snapshot.borrow())
    }

    /// Returns the Local Participant command handle for this Room Session.
    pub fn local_participant(&self) -> LocalParticipant {
        LocalParticipant {
            inner: Rc::clone(&self.inner),
        }
    }

    /// Resolves a handle for the currently observable Remote Participant instance.
    pub fn remote_participant(&self, identity: &str) -> Option<RemoteParticipant> {
        if self.inner.lifecycle.get() != SessionLifecycle::Active {
            return None;
        }
        let snapshot = self.inner.snapshot.borrow();
        let participant = snapshot.remote_participants().get(identity)?;
        Some(RemoteParticipant {
            inner: Rc::new(RemoteParticipantInner {
                locator: RemoteParticipantLocator {
                    identity: identity.into(),
                    sid: participant.sid().into(),
                    generation: participant.generation(),
                },
                session: Rc::downgrade(&self.inner),
            }),
        })
    }

    /// Resolves a live command handle from one currently observable Track Publication.
    pub fn remote_track_publication(
        &self,
        participant_identity: &str,
        publication_sid: &str,
    ) -> Option<RemoteTrackPublication> {
        self.remote_participant(participant_identity)?
            .track_publication(publication_sid)
    }

    /// Replaces the Local Participant's publisher-side subscription permissions.
    ///
    /// The command is sent before the returned local future is polled. LiveKit source grants remain
    /// authoritative; a Local Participant without publisher permission receives
    /// `Error::PermissionDenied`. Changing this policy does not mute, unpublish, or stop local
    /// capture.
    pub fn set_publication_subscription_permissions(
        &self,
        _permissions: PublicationSubscriptionPermissions,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if self.inner.lifecycle.get() != SessionLifecycle::Active {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "change publisher subscription permissions",
                    reason: "Room Session is terminal",
                })
            });
        }
        if !_permissions.is_valid() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "change publisher subscription permissions",
                    reason: "Participant Identities cannot be empty",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let permissions = match serde_wasm_bindgen::to_value(&_permissions) {
                Ok(permissions) => permissions,
                Err(error) => {
                    let error = Error::AdapterFailure(AdapterError::contract(error.to_string()));
                    return Box::pin(async move { Err(error) });
                }
            };
            let result =
                adapter::set_track_subscription_permissions(self.inner.room_id, &permissions)
                    .map_err(AdapterError::from_cause)
                    .map_err(classify_subscription_permission_error);
            Box::pin(async move { result })
        }
    }

    /// Retries remote audio playback from a browser user-activation handler.
    ///
    /// The browser operation starts before the returned local future is polled so transient user
    /// activation is not lost at an async suspension point.
    pub fn start_audio_playback(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if self.inner.lifecycle.get() != SessionLifecycle::Active {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "start remote audio playback",
                    reason: "Room Session is terminal",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let operation = adapter::start_room_audio(self.inner.room_id)
                .map_err(AdapterError::from_cause)
                .map_err(Error::MediaControlFailed);
            Box::pin(async move {
                let operation = operation?;
                wasm_bindgen_futures::JsFuture::from(operation)
                    .await
                    .map(|_| ())
                    .map_err(AdapterError::from_cause)
                    .map_err(Error::MediaControlFailed)
            })
        }
    }

    /// Subscribes to bounded Room Events.
    ///
    /// A capacity of zero is treated as one. When a consumer falls behind, queued details are
    /// replaced by a `Lagged` event directing it to recover through `snapshot`.
    pub fn subscribe_events(&self, capacity: usize) -> RoomEventStream {
        let closed = self.inner.lifecycle.get() == SessionLifecycle::Left;
        let queue = Rc::new(RefCell::new(EventQueue {
            capacity: capacity.max(1),
            events: VecDeque::new(),
            lagged_revision: None,
            closed,
            waker: None,
        }));
        if !closed {
            self.inner
                .subscribers
                .borrow_mut()
                .push(Rc::downgrade(&queue));
        }
        RoomEventStream { queue }
    }

    /// Subscribes to native byte packets received with one LiveKit delivery mode.
    ///
    /// Reliable and lossy subscriptions have independent buffers. A capacity of zero is treated
    /// as one. When a consumer falls behind, the oldest buffered packet is dropped and the stream
    /// reports the exact count through `DataPacketStreamItem::Lagged` before yielding retained
    /// packets. This stream closes when the Room Session begins leaving.
    pub fn subscribe_data_packets(
        &self,
        delivery: DataPacketDelivery,
        capacity: usize,
    ) -> DataPacketStream {
        #[cfg(not(any(target_arch = "wasm32", test)))]
        let _ = (delivery, capacity);
        let closed = self.inner.lifecycle.get() != SessionLifecycle::Active;
        let queue = Rc::new(RefCell::new(DataPacketQueue {
            #[cfg(any(target_arch = "wasm32", test))]
            capacity: capacity.max(1),
            items: VecDeque::new(),
            dropped: 0,
            closed,
            waker: None,
        }));
        if !closed {
            self.inner
                .data_packet_subscribers
                .borrow_mut()
                .push(DataPacketSubscription {
                    #[cfg(any(target_arch = "wasm32", test))]
                    delivery,
                    queue: Rc::downgrade(&queue),
                });
        }
        DataPacketStream { queue }
    }

    /// Subscribes to chat messages through a buffer independent from Room Events.
    ///
    /// A capacity of zero is treated as one. When a consumer falls behind, the oldest buffered
    /// message is dropped and the stream reports the exact count through
    /// `ChatMessageStreamItem::Lagged` before yielding retained messages. This stream closes when
    /// the Room Session begins leaving.
    pub fn subscribe_chat_messages(&self, capacity: usize) -> ChatMessageStream {
        #[cfg(not(any(target_arch = "wasm32", test)))]
        let _ = capacity;
        let closed = self.inner.lifecycle.get() != SessionLifecycle::Active;
        let queue = Rc::new(RefCell::new(ChatMessageQueue {
            #[cfg(any(target_arch = "wasm32", test))]
            capacity: capacity.max(1),
            items: VecDeque::new(),
            dropped: 0,
            closed,
            waker: None,
        }));
        if !closed {
            self.inner
                .chat_message_subscribers
                .borrow_mut()
                .push(ChatMessageSubscription {
                    queue: Rc::downgrade(&queue),
                });
        }
        ChatMessageStream { queue }
    }

    /// Subscribes to LiveKit-normalized transcription segments independently from Room Events.
    ///
    /// A capacity of zero is treated as one. When a consumer falls behind, the oldest buffered
    /// update is dropped and the stream reports the exact count through
    /// `TranscriptionSegmentStreamItem::Lagged` before yielding retained updates. Segment IDs and
    /// stream order let consumers replace interim text with a later final update. This stream
    /// closes when the Room Session begins leaving.
    pub fn subscribe_transcription_segments(&self, capacity: usize) -> TranscriptionSegmentStream {
        #[cfg(not(any(target_arch = "wasm32", test)))]
        let _ = capacity;
        let closed = self.inner.lifecycle.get() != SessionLifecycle::Active;
        let queue = Rc::new(RefCell::new(TranscriptionSegmentQueue {
            #[cfg(any(target_arch = "wasm32", test))]
            capacity: capacity.max(1),
            items: VecDeque::new(),
            dropped: 0,
            closed,
            waker: None,
        }));
        if !closed {
            self.inner
                .transcription_segment_subscribers
                .borrow_mut()
                .push(TranscriptionSegmentSubscription {
                    queue: Rc::downgrade(&queue),
                });
        }
        TranscriptionSegmentStream { queue }
    }

    /// Returns whether explicit leave has made this Room Session terminal.
    pub fn is_terminal(&self) -> bool {
        self.inner.lifecycle.get() != SessionLifecycle::Active
    }

    /// Makes every clone terminal and disconnects browser resources deterministically.
    ///
    /// Terminal state is installed when this method is called, before the returned local future is
    /// polled. The future requires no particular Rust async runtime.
    pub fn leave(&self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        self.inner.begin_leave();
        let inner = Rc::clone(&self.inner);
        Box::pin(async move {
            #[cfg(target_arch = "wasm32")]
            let result = inner.await_cleanup().await.map_err(Error::LeaveFailed);
            #[cfg(not(target_arch = "wasm32"))]
            let result = Ok(());

            inner.finish_leave();
            result
        })
    }

    #[cfg(test)]
    fn test_session(snapshot: RoomSnapshot) -> Self {
        Self::test_session_with_cleanup(snapshot, Rc::new(Cell::new(0)))
    }

    #[cfg(test)]
    fn test_session_with_cleanup(snapshot: RoomSnapshot, cleanup_count: Rc<Cell<usize>>) -> Self {
        Self {
            inner: Rc::new(SessionInner {
                snapshot: RefCell::new(Rc::new(snapshot)),
                subscribers: RefCell::new(Vec::new()),
                data_packet_subscribers: RefCell::new(Vec::new()),
                chat_message_subscribers: RefCell::new(Vec::new()),
                transcription_segment_subscribers: RefCell::new(Vec::new()),
                #[cfg(target_arch = "wasm32")]
                rpc_handler_tasks: RefCell::new(Vec::new()),
                lifecycle: Cell::new(SessionLifecycle::Active),
                cleanup_started: Cell::new(false),
                cleanup_count,
                #[cfg(target_arch = "wasm32")]
                room_id: 0,
                #[cfg(target_arch = "wasm32")]
                leave_operation: RefCell::new(None),
                #[cfg(target_arch = "wasm32")]
                _event_callback: wasm_bindgen::closure::Closure::new(|_: wasm_bindgen::JsValue| {}),
                #[cfg(target_arch = "wasm32")]
                _data_packet_callback: wasm_bindgen::closure::Closure::new(
                    |_: js_sys::Uint8Array, _: wasm_bindgen::JsValue, _: bool| {},
                ),
                #[cfg(target_arch = "wasm32")]
                _chat_message_callback: wasm_bindgen::closure::Closure::new(
                    |_: wasm_bindgen::JsValue| {},
                ),
                #[cfg(target_arch = "wasm32")]
                _transcription_segment_callback: wasm_bindgen::closure::Closure::new(
                    |_: wasm_bindgen::JsValue| {},
                ),
            }),
        }
    }

    #[cfg(test)]
    fn install_test_update(&self, snapshot: RoomSnapshot, kind: RoomEventKind) {
        self.inner.install_normalized_update(snapshot, [kind]);
    }

    #[cfg(test)]
    fn install_test_data_packet(&self, packet: DataPacket) {
        self.inner.install_data_packet(packet);
    }

    #[cfg(test)]
    fn install_test_chat_message(&self, message: ChatMessage) {
        self.inner.install_chat_message(message);
    }

    #[cfg(test)]
    fn install_test_transcription_segment(&self, segment: TranscriptionSegment) {
        self.inner.install_transcription_segment(segment);
    }
}

struct SessionInner {
    snapshot: RefCell<Rc<RoomSnapshot>>,
    subscribers: RefCell<Vec<Weak<RefCell<EventQueue>>>>,
    data_packet_subscribers: RefCell<Vec<DataPacketSubscription>>,
    chat_message_subscribers: RefCell<Vec<ChatMessageSubscription>>,
    transcription_segment_subscribers: RefCell<Vec<TranscriptionSegmentSubscription>>,
    #[cfg(target_arch = "wasm32")]
    rpc_handler_tasks: RefCell<Vec<Weak<RpcHandlerTask>>>,
    lifecycle: Cell<SessionLifecycle>,
    cleanup_started: Cell<bool>,
    #[cfg(test)]
    cleanup_count: Rc<Cell<usize>>,
    #[cfg(target_arch = "wasm32")]
    room_id: u32,
    #[cfg(target_arch = "wasm32")]
    leave_operation: RefCell<Option<Result<js_sys::Promise, AdapterError>>>,
    #[cfg(target_arch = "wasm32")]
    _event_callback: wasm_bindgen::closure::Closure<dyn FnMut(wasm_bindgen::JsValue)>,
    #[cfg(target_arch = "wasm32")]
    _data_packet_callback:
        wasm_bindgen::closure::Closure<dyn FnMut(js_sys::Uint8Array, wasm_bindgen::JsValue, bool)>,
    #[cfg(target_arch = "wasm32")]
    _chat_message_callback: wasm_bindgen::closure::Closure<dyn FnMut(wasm_bindgen::JsValue)>,
    #[cfg(target_arch = "wasm32")]
    _transcription_segment_callback:
        wasm_bindgen::closure::Closure<dyn FnMut(wasm_bindgen::JsValue)>,
}

impl SessionInner {
    #[cfg(any(target_arch = "wasm32", test))]
    fn install_data_packet(&self, packet: DataPacket) {
        if self.lifecycle.get() != SessionLifecycle::Active {
            return;
        }

        self.data_packet_subscribers
            .borrow_mut()
            .retain(|subscriber| {
                let Some(queue) = subscriber.queue.upgrade() else {
                    return false;
                };
                if subscriber.delivery == packet.delivery {
                    queue.borrow_mut().push(packet.clone());
                }
                true
            });
    }

    #[cfg(any(target_arch = "wasm32", test))]
    fn install_chat_message(&self, message: ChatMessage) {
        if self.lifecycle.get() != SessionLifecycle::Active {
            return;
        }

        self.chat_message_subscribers
            .borrow_mut()
            .retain(|subscriber| {
                let Some(queue) = subscriber.queue.upgrade() else {
                    return false;
                };
                queue.borrow_mut().push(message.clone());
                true
            });
    }

    #[cfg(any(target_arch = "wasm32", test))]
    fn install_transcription_segment(&self, segment: TranscriptionSegment) {
        if self.lifecycle.get() != SessionLifecycle::Active {
            return;
        }

        self.transcription_segment_subscribers
            .borrow_mut()
            .retain(|subscriber| {
                let Some(queue) = subscriber.queue.upgrade() else {
                    return false;
                };
                queue.borrow_mut().push(segment.clone());
                true
            });
    }

    fn install(&self, snapshot: RoomSnapshot, events: impl IntoIterator<Item = RoomEventKind>) {
        if snapshot.revision <= self.snapshot.borrow().revision {
            return;
        }
        let revision = snapshot.revision;
        let events: Vec<_> = events.into_iter().collect();
        *self.snapshot.borrow_mut() = Rc::new(snapshot);

        self.subscribers.borrow_mut().retain(|subscriber| {
            let Some(subscriber) = subscriber.upgrade() else {
                return false;
            };
            let mut subscriber = subscriber.borrow_mut();
            for kind in &events {
                subscriber.push(RoomEvent {
                    revision,
                    kind: kind.clone(),
                });
            }
            true
        });
    }

    #[cfg(target_arch = "wasm32")]
    fn install_adapter_update(self: &Rc<Self>, update: AdapterUpdate) {
        self.install_normalized_update(update.snapshot.into(), update.events);
    }

    #[cfg(any(target_arch = "wasm32", test))]
    fn install_normalized_update(
        self: &Rc<Self>,
        snapshot: RoomSnapshot,
        events: impl IntoIterator<Item = RoomEventKind>,
    ) {
        if self.lifecycle.get() != SessionLifecycle::Active {
            return;
        }

        let terminal = snapshot.connection == RoomConnectionState::Disconnected;
        self.install(snapshot, events);
        if terminal {
            self.lifecycle.set(SessionLifecycle::Left);
            #[cfg(target_arch = "wasm32")]
            self.cancel_rpc_handler_tasks();
            self.start_cleanup();
            self.close_subscribers();
        }
    }

    fn begin_leave(self: &Rc<Self>) {
        if self.lifecycle.get() != SessionLifecycle::Active {
            return;
        }
        self.lifecycle.set(SessionLifecycle::Leaving);

        #[cfg(target_arch = "wasm32")]
        self.cancel_rpc_handler_tasks();

        self.transition(RoomConnectionState::Disconnecting);
        self.close_data_packet_subscribers();
        self.close_chat_message_subscribers();
        self.close_transcription_segment_subscribers();
        self.start_cleanup();
    }

    fn finish_leave(&self) {
        if self.lifecycle.replace(SessionLifecycle::Left) == SessionLifecycle::Left {
            return;
        }

        self.transition(RoomConnectionState::Disconnected);
        self.close_subscribers();
    }

    fn transition(&self, connection: RoomConnectionState) {
        let mut snapshot = self.snapshot.borrow().as_ref().clone();
        snapshot.revision += 1;
        snapshot.connection = connection;
        self.install(
            snapshot,
            [RoomEventKind::ConnectionStateChanged { state: connection }],
        );
    }

    fn close_subscribers(&self) {
        for subscriber in self.subscribers.borrow_mut().drain(..) {
            let Some(subscriber) = subscriber.upgrade() else {
                continue;
            };
            let mut subscriber = subscriber.borrow_mut();
            subscriber.closed = true;
            if let Some(waker) = subscriber.waker.take() {
                waker.wake();
            }
        }
        self.close_data_packet_subscribers();
        self.close_chat_message_subscribers();
        self.close_transcription_segment_subscribers();
    }

    fn close_data_packet_subscribers(&self) {
        close_bounded_queues(
            self.data_packet_subscribers
                .borrow_mut()
                .drain(..)
                .map(|subscriber| subscriber.queue),
        );
    }

    fn close_chat_message_subscribers(&self) {
        close_bounded_queues(
            self.chat_message_subscribers
                .borrow_mut()
                .drain(..)
                .map(|subscriber| subscriber.queue),
        );
    }

    fn close_transcription_segment_subscribers(&self) {
        close_bounded_queues(
            self.transcription_segment_subscribers
                .borrow_mut()
                .drain(..)
                .map(|subscriber| subscriber.queue),
        );
    }

    #[cfg(target_arch = "wasm32")]
    fn cancel_rpc_handler_tasks(&self) {
        for task in self.rpc_handler_tasks.borrow_mut().drain(..) {
            if let Some(task) = task.upgrade() {
                task.abort.abort();
            }
        }
    }

    fn start_cleanup(self: &Rc<Self>) {
        if !self.cleanup_started.replace(true) {
            #[cfg(test)]
            {
                self.cleanup_count.set(self.cleanup_count.get() + 1);
                self.finish_leave();
            }

            #[cfg(target_arch = "wasm32")]
            {
                use wasm_bindgen::JsCast;

                let inner = Rc::clone(self);
                let callback = wasm_bindgen::closure::Closure::once_into_js(move || {
                    inner.finish_leave();
                });
                let callback: js_sys::Function = callback.unchecked_into();
                let operation =
                    adapter::leave_room(self.room_id, &callback).map_err(AdapterError::from_cause);
                if operation.is_err() {
                    self.finish_leave();
                }
                self.leave_operation.borrow_mut().replace(operation);
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    async fn await_cleanup(&self) -> Result<(), AdapterError> {
        let operation = self.leave_operation.borrow().clone();
        let Some(operation) = operation else {
            return Ok(());
        };
        let promise = operation?;
        wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map(|_| ())
            .map_err(AdapterError::from_cause)
    }
}

impl Drop for SessionInner {
    fn drop(&mut self) {
        #[cfg(target_arch = "wasm32")]
        self.cancel_rpc_handler_tasks();
        if !self.cleanup_started.replace(true) {
            #[cfg(test)]
            self.cleanup_count.set(self.cleanup_count.get() + 1);

            #[cfg(target_arch = "wasm32")]
            adapter::drop_room(self.room_id);
        }
        self.close_subscribers();
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum SessionLifecycle {
    Active,
    Leaving,
    Left,
}

/// Internal integration seam used by `livekit-browser-recording`.
///
/// This module is public because Rust has no friend-crate visibility. It deliberately exposes no
/// LiveKit JavaScript objects or registry identifiers.
#[doc(hidden)]
pub mod local_clip_recording_support {
    use super::*;

    /// Opaque browser stream containing selected Local Media Track sources.
    pub struct LocalMediaStream {
        #[cfg(target_arch = "wasm32")]
        stream: web_sys::MediaStream,
        _browser_thread: PhantomData<Rc<()>>,
    }

    /// Builds a browser MediaStream over the selected active Local Media Tracks.
    pub fn local_media_stream(
        audio_tracks: &[LocalAudioTrack],
        video_tracks: &[LocalVideoTrack],
    ) -> Result<LocalMediaStream, Error> {
        if audio_tracks.is_empty() && video_tracks.is_empty() {
            return Err(Error::InvalidState {
                operation: "start Local Clip Recording",
                reason: "at least one Local Media Track is required",
            });
        }
        if audio_tracks.iter().any(LocalAudioTrack::is_stopped)
            || video_tracks.iter().any(LocalVideoTrack::is_stopped)
        {
            return Err(Error::InvalidState {
                operation: "start Local Clip Recording",
                reason: "a selected Local Media Track has stopped",
            });
        }
        if audio_tracks.iter().enumerate().any(|(index, track)| {
            audio_tracks[..index]
                .iter()
                .any(|other| Rc::ptr_eq(&track.inner, &other.inner))
        }) || video_tracks.iter().enumerate().any(|(index, track)| {
            video_tracks[..index]
                .iter()
                .any(|other| Rc::ptr_eq(&track.inner, &other.inner))
        }) {
            return Err(Error::InvalidState {
                operation: "start Local Clip Recording",
                reason: "the same Local Media Track was selected more than once",
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;

            let audio_ids = js_sys::Array::new();
            for track in audio_tracks {
                audio_ids.push(&wasm_bindgen::JsValue::from(track.inner.track_id));
            }
            let video_ids = js_sys::Array::new();
            for track in video_tracks {
                video_ids.push(&wasm_bindgen::JsValue::from(track.inner.track_id));
            }
            let stream = adapter::create_local_clip_media_stream(&audio_ids, &video_ids)
                .map_err(AdapterError::from_cause)
                .map_err(Error::LocalClipRecordingFailed)?
                .dyn_into::<web_sys::MediaStream>()
                .map_err(|_| {
                    Error::LocalClipRecordingFailed(AdapterError::contract(
                        "Local Clip Recording stream was not a MediaStream".into(),
                    ))
                })?;
            Ok(LocalMediaStream {
                stream,
                _browser_thread: PhantomData,
            })
        }
    }

    /// Rechecks MediaRecorder capability immediately before Local Clip Recording starts.
    #[cfg(target_arch = "wasm32")]
    pub fn supported() -> bool {
        adapter::local_clip_recording_supported()
    }

    /// Selects the first browser-supported MIME preference in caller order.
    #[cfg(target_arch = "wasm32")]
    pub fn select_mime_type(preferences: &[String]) -> Option<String> {
        let values = js_sys::Array::new();
        for preference in preferences {
            values.push(&wasm_bindgen::JsValue::from_str(preference));
        }
        adapter::select_local_clip_mime_type(&values)
    }

    /// Why the private adapter automatically stopped a Local Clip Recording.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum StopReason {
        /// The application explicitly requested stop.
        Explicit,
        /// The configured active-duration limit elapsed.
        DurationLimit,
        /// Delivered chunks reached the configured accumulated-size threshold.
        SizeLimit,
        /// Every selected Local Media Track ended and the browser stopped the recorder.
        SourceEnded,
    }

    /// Normalized event from the private Local Clip Recording adapter.
    pub enum Event {
        /// The browser is flushing the final encoded chunk.
        Stopping,
        /// The browser produced a complete artifact.
        Completed {
            /// Browser-owned artifact bytes.
            blob: web_sys::Blob,
            /// Actual MIME type reported by MediaRecorder.
            mime_type: String,
            /// Active recording time, excluding pauses.
            duration: Duration,
            /// Browser Blob size without reading its bytes.
            size_bytes: u64,
            /// Why recording stopped.
            stop_reason: StopReason,
        },
        /// The browser encoder produced no media bytes.
        Empty,
        /// The browser encoder or private adapter failed.
        Failed(Error),
    }

    /// Active private MediaRecorder adapter with deterministic callback and final-drop cleanup.
    pub struct ActiveLocalClipRecording {
        #[cfg(target_arch = "wasm32")]
        recording_id: u32,
        #[cfg(target_arch = "wasm32")]
        mime_type: String,
        #[cfg(target_arch = "wasm32")]
        terminal: Rc<Cell<bool>>,
        #[cfg(target_arch = "wasm32")]
        _callback: wasm_bindgen::closure::Closure<dyn FnMut(wasm_bindgen::JsValue)>,
        _browser_thread: PhantomData<Rc<()>>,
    }

    impl ActiveLocalClipRecording {
        /// Returns the actual MIME type selected by MediaRecorder.
        #[cfg(target_arch = "wasm32")]
        pub fn mime_type(&self) -> &str {
            &self.mime_type
        }

        /// Pauses this Local Clip Recording.
        #[cfg(target_arch = "wasm32")]
        pub fn pause(&self) -> Result<(), Error> {
            adapter::pause_local_clip_recording(self.recording_id).map_err(recording_failure)
        }

        /// Resumes this Local Clip Recording.
        #[cfg(target_arch = "wasm32")]
        pub fn resume(&self) -> Result<(), Error> {
            adapter::resume_local_clip_recording(self.recording_id).map_err(recording_failure)
        }

        /// Requests an explicit stop for this Local Clip Recording.
        #[cfg(target_arch = "wasm32")]
        pub fn stop(&self) -> Result<(), Error> {
            adapter::stop_local_clip_recording(self.recording_id).map_err(recording_failure)
        }
    }

    impl Drop for ActiveLocalClipRecording {
        fn drop(&mut self) {
            #[cfg(target_arch = "wasm32")]
            if !self.terminal.get() {
                adapter::drop_local_clip_recording(self.recording_id);
            }
        }
    }

    /// Starts the private MediaRecorder adapter over an opaque local stream.
    #[cfg(target_arch = "wasm32")]
    pub fn start(
        stream: &LocalMediaStream,
        selected_mime_type: &str,
        max_duration_ms: f64,
        max_bytes: f64,
        mut on_event: impl FnMut(Event) + 'static,
    ) -> Result<ActiveLocalClipRecording, Error> {
        use wasm_bindgen::JsCast;

        let terminal = Rc::new(Cell::new(false));
        let callback_terminal = Rc::clone(&terminal);
        let callback = wasm_bindgen::closure::Closure::wrap(Box::new(
            move |raw_event: wasm_bindgen::JsValue| {
                let event = parse_event(raw_event);
                if !matches!(event, Event::Stopping) {
                    callback_terminal.set(true);
                }
                on_event(event);
            },
        )
            as Box<dyn FnMut(wasm_bindgen::JsValue)>);
        let descriptor = adapter::start_local_clip_recording(
            &stream.stream,
            selected_mime_type,
            max_duration_ms,
            max_bytes,
            callback.as_ref().unchecked_ref(),
        )
        .map_err(recording_failure)?;
        let recording_id = required_number(&descriptor, "id")?;
        if recording_id.fract() != 0.0 || !(1.0..=u32::MAX as f64).contains(&recording_id) {
            return Err(contract_failure(
                "Local Clip Recording returned an invalid identifier",
            ));
        }
        let recording_id = recording_id as u32;
        let mime_type = match required_string(&descriptor, "mimeType") {
            Ok(mime_type) if !mime_type.is_empty() => mime_type,
            Ok(_) => {
                adapter::drop_local_clip_recording(recording_id);
                return Err(contract_failure(
                    "Local Clip Recording returned an empty MIME type",
                ));
            }
            Err(error) => {
                adapter::drop_local_clip_recording(recording_id);
                return Err(error);
            }
        };
        Ok(ActiveLocalClipRecording {
            recording_id,
            mime_type,
            terminal,
            _callback: callback,
            _browser_thread: PhantomData,
        })
    }

    /// Creates a browser object URL for a completed Local Clip Recording artifact.
    #[cfg(target_arch = "wasm32")]
    pub fn create_object_url(blob: &web_sys::Blob) -> Result<String, Error> {
        adapter::create_local_clip_object_url(blob)
            .map_err(|cause| Error::LocalClipObjectUrlFailed(AdapterError::from_cause(cause)))
    }

    /// Revokes a browser object URL for a completed Local Clip Recording artifact.
    #[cfg(target_arch = "wasm32")]
    pub fn revoke_object_url(url: &str) -> Result<(), Error> {
        adapter::revoke_local_clip_object_url(url)
            .map_err(|cause| Error::LocalClipObjectUrlFailed(AdapterError::from_cause(cause)))
    }

    /// Normalizes a browser recording failure without exposing its JavaScript cause.
    #[cfg(target_arch = "wasm32")]
    fn recording_failure(cause: wasm_bindgen::JsValue) -> Error {
        Error::LocalClipRecordingFailed(AdapterError::from_cause(cause))
    }

    #[cfg(target_arch = "wasm32")]
    fn contract_failure(message: impl Into<String>) -> Error {
        Error::LocalClipRecordingFailed(AdapterError::contract(message.into()))
    }

    #[cfg(target_arch = "wasm32")]
    fn parse_event(raw_event: wasm_bindgen::JsValue) -> Event {
        match parse_event_result(raw_event) {
            Ok(event) => event,
            Err(error) => Event::Failed(error),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn parse_event_result(raw_event: wasm_bindgen::JsValue) -> Result<Event, Error> {
        use wasm_bindgen::JsCast;

        match required_string(&raw_event, "kind")?.as_str() {
            "stopping" => Ok(Event::Stopping),
            "empty" => Ok(Event::Empty),
            "failed" => Ok(Event::Failed(recording_failure(property(
                &raw_event, "error",
            )?))),
            "completed" => {
                let blob = property(&raw_event, "blob")?
                    .dyn_into::<web_sys::Blob>()
                    .map_err(|_| contract_failure("Local Clip Recording did not return a Blob"))?;
                let mime_type = required_string(&raw_event, "mimeType")?;
                let duration_ms = required_number(&raw_event, "durationMs")?;
                let size_bytes = required_number(&raw_event, "sizeBytes")?;
                if !duration_ms.is_finite() || duration_ms < 0.0 {
                    return Err(contract_failure(
                        "Local Clip Recording returned an invalid duration",
                    ));
                }
                if !size_bytes.is_finite()
                    || size_bytes <= 0.0
                    || size_bytes.fract() != 0.0
                    || size_bytes > 9_007_199_254_740_991.0
                {
                    return Err(contract_failure(
                        "Local Clip Recording returned an invalid Blob size",
                    ));
                }
                let stop_reason = match required_string(&raw_event, "stopReason")?.as_str() {
                    "explicit" => StopReason::Explicit,
                    "durationLimit" => StopReason::DurationLimit,
                    "sizeLimit" => StopReason::SizeLimit,
                    "sourceEnded" => StopReason::SourceEnded,
                    _ => {
                        return Err(contract_failure(
                            "Local Clip Recording returned an invalid stop reason",
                        ));
                    }
                };
                Ok(Event::Completed {
                    blob,
                    mime_type,
                    duration: Duration::from_secs_f64(duration_ms / 1_000.0),
                    size_bytes: size_bytes as u64,
                    stop_reason,
                })
            }
            _ => Err(contract_failure(
                "Local Clip Recording returned an invalid event kind",
            )),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn property(value: &wasm_bindgen::JsValue, name: &str) -> Result<wasm_bindgen::JsValue, Error> {
        js_sys::Reflect::get(value, &wasm_bindgen::JsValue::from_str(name)).map_err(|_| {
            contract_failure(format!(
                "could not read Local Clip Recording adapter property {name}"
            ))
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn required_string(value: &wasm_bindgen::JsValue, name: &str) -> Result<String, Error> {
        property(value, name)?.as_string().ok_or_else(|| {
            contract_failure(format!(
                "Local Clip Recording adapter property {name} was not a string"
            ))
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn required_number(value: &wasm_bindgen::JsValue, name: &str) -> Result<f64, Error> {
        property(value, name)?.as_f64().ok_or_else(|| {
            contract_failure(format!(
                "Local Clip Recording adapter property {name} was not a number"
            ))
        })
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "status")]
enum RuntimeInspection {
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
fn finish_initialization(inspection: RuntimeInspection) -> Result<BrowserCapabilities, Error> {
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

#[cfg(target_arch = "wasm32")]
mod adapter {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(module = "/js/livekit-browser.js")]
    extern "C" {
        #[wasm_bindgen(catch, js_name = inspectBrowserRuntime)]
        pub fn inspect_browser_runtime(expected_version: &str) -> Result<JsValue, JsValue>;

        #[wasm_bindgen(catch, js_name = installLogExtension)]
        pub fn install_log_extension(callback: &js_sys::Function) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = enumerateMediaDevices)]
        pub fn enumerate_media_devices(
            kind: &str,
            request_permissions: bool,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = createRoom)]
        pub fn create_room(raw_options: JsValue) -> Result<u32, JsValue>;

        #[wasm_bindgen(catch, js_name = connectRoom)]
        pub fn connect_room(
            room_id: u32,
            server_url: &str,
            participant_token: &str,
            auto_subscribe: bool,
            raw_options: JsValue,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = activateRoom)]
        pub fn activate_room(
            room_id: u32,
            callback: &js_sys::Function,
            data_packet_callback: &js_sys::Function,
            chat_message_callback: &js_sys::Function,
            transcription_segment_callback: &js_sys::Function,
        ) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = leaveRoom)]
        pub fn leave_room(
            room_id: u32,
            on_complete: &js_sys::Function,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(js_name = dropRoom)]
        pub fn drop_room(room_id: u32);

        #[wasm_bindgen(catch, js_name = rawRoom)]
        pub fn raw_room(room_id: u32) -> Result<JsValue, JsValue>;

        #[wasm_bindgen(catch, js_name = rawLocalParticipant)]
        pub fn raw_local_participant(room_id: u32) -> Result<JsValue, JsValue>;

        #[wasm_bindgen(catch, js_name = rawRemoteParticipant)]
        pub fn raw_remote_participant(room_id: u32, locator: &JsValue) -> Result<JsValue, JsValue>;

        #[wasm_bindgen(catch, js_name = rawRemotePublication)]
        pub fn raw_remote_publication(room_id: u32, locator: &JsValue) -> Result<JsValue, JsValue>;

        #[wasm_bindgen(catch, js_name = setRemoteTrackSubscription)]
        pub fn set_remote_track_subscription(
            room_id: u32,
            locator: &JsValue,
            subscribed: bool,
        ) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = setTrackSubscriptionPermissions)]
        pub fn set_track_subscription_permissions(
            room_id: u32,
            permissions: &JsValue,
        ) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = setLocalParticipantMetadata)]
        pub fn set_local_participant_metadata(
            room_id: u32,
            metadata: &str,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = updateLocalParticipantAttributes)]
        pub fn update_local_participant_attributes(
            room_id: u32,
            attributes: &JsValue,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = sendDataPacket)]
        pub fn send_data_packet(
            room_id: u32,
            payload: &js_sys::Uint8Array,
            reliable: bool,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = sendChatMessage)]
        pub fn send_chat_message(room_id: u32, message: &str) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = openTextStream)]
        pub fn open_text_stream(
            room_id: u32,
            options: &JsValue,
            on_complete: &js_sys::Function,
        ) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = openByteStream)]
        pub fn open_byte_stream(
            room_id: u32,
            options: &JsValue,
            on_complete: &js_sys::Function,
        ) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = writeTextStream)]
        pub fn write_text_stream(
            stream_id: u32,
            chunk: &str,
            on_complete: &js_sys::Function,
        ) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = writeByteStream)]
        pub fn write_byte_stream(
            stream_id: u32,
            chunk: &js_sys::Uint8Array,
            on_complete: &js_sys::Function,
        ) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = completeOutgoingDataStream)]
        pub fn complete_outgoing_data_stream(
            stream_id: u32,
            on_complete: &js_sys::Function,
        ) -> Result<(), JsValue>;

        #[wasm_bindgen(js_name = dropOutgoingDataStream)]
        pub fn drop_outgoing_data_stream(stream_id: u32);

        #[wasm_bindgen(js_name = dropOutgoingDataStreamDescriptor)]
        pub fn drop_outgoing_data_stream_descriptor(descriptor: JsValue);

        #[wasm_bindgen(catch, js_name = registerTextStreamHandler)]
        pub fn register_text_stream_handler(
            room_id: u32,
            topic: &str,
            callback: &js_sys::Function,
        ) -> Result<u32, JsValue>;

        #[wasm_bindgen(catch, js_name = registerByteStreamHandler)]
        pub fn register_byte_stream_handler(
            room_id: u32,
            topic: &str,
            callback: &js_sys::Function,
        ) -> Result<u32, JsValue>;

        #[wasm_bindgen(js_name = dropDataStreamHandler)]
        pub fn drop_data_stream_handler(registration_id: u32);

        #[wasm_bindgen(catch, js_name = readTextStream)]
        pub fn read_text_stream(stream_id: u32) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = readByteStream)]
        pub fn read_byte_stream(stream_id: u32) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(js_name = finishIncomingDataStream)]
        pub fn finish_incoming_data_stream(stream_id: u32);

        #[wasm_bindgen(js_name = dropIncomingDataStream)]
        pub fn drop_incoming_data_stream(stream_id: u32);

        #[wasm_bindgen(js_name = dropIncomingDataStreamDescriptor)]
        pub fn drop_incoming_data_stream_descriptor(descriptor: JsValue);

        #[wasm_bindgen(catch, js_name = invokeParticipantRpc)]
        pub fn invoke_participant_rpc(
            room_id: u32,
            options: &JsValue,
            on_complete: &js_sys::Function,
        ) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = registerParticipantRpcMethod)]
        pub fn register_participant_rpc_method(
            room_id: u32,
            method: &str,
            callback: &js_sys::Function,
        ) -> Result<u32, JsValue>;

        #[wasm_bindgen(js_name = dropParticipantRpcMethod)]
        pub fn drop_participant_rpc_method(registration_id: u32);

        #[wasm_bindgen(js_name = completeParticipantRpcInvocation)]
        pub fn complete_participant_rpc_invocation(invocation_id: u32, completion: &JsValue);

        #[wasm_bindgen(js_name = dropParticipantRpcInvocation)]
        pub fn drop_participant_rpc_invocation(invocation_id: u32);

        #[wasm_bindgen(js_name = dropParticipantRpcInvocationDescriptor)]
        pub fn drop_participant_rpc_invocation_descriptor(descriptor: JsValue);

        #[wasm_bindgen(catch, js_name = resolveRemoteMediaTrack)]
        pub fn resolve_remote_media_track(
            room_id: u32,
            locator: &JsValue,
            media_generation: u32,
            on_terminal: &js_sys::Function,
        ) -> Result<JsValue, JsValue>;

        #[wasm_bindgen(catch, js_name = attachRemoteVideoTrack)]
        pub fn attach_remote_video_track(
            track_id: u32,
            element: &web_sys::HtmlVideoElement,
        ) -> Result<u32, JsValue>;

        #[wasm_bindgen(catch, js_name = remoteVideoReceiverStatistics)]
        pub fn remote_video_receiver_statistics(track_id: u32) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = rawRemoteRtcStatsReport)]
        pub fn raw_remote_rtc_stats_report(track_id: u32) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = detachRemoteVideoTrack)]
        pub fn detach_remote_video_track(track_id: u32, attachment_id: u32) -> Result<(), JsValue>;

        #[wasm_bindgen(js_name = dropRemoteVideoTrackAttachment)]
        pub fn drop_remote_video_track_attachment(track_id: u32, attachment_id: u32);

        #[wasm_bindgen(catch, js_name = attachRemoteAudioTrack)]
        pub fn attach_remote_audio_track(
            track_id: u32,
            element: &web_sys::HtmlAudioElement,
        ) -> Result<u32, JsValue>;

        #[wasm_bindgen(catch, js_name = remoteAudioReceiverStatistics)]
        pub fn remote_audio_receiver_statistics(track_id: u32) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = detachRemoteAudioTrack)]
        pub fn detach_remote_audio_track(track_id: u32, attachment_id: u32) -> Result<(), JsValue>;

        #[wasm_bindgen(js_name = dropRemoteAudioTrackAttachment)]
        pub fn drop_remote_audio_track_attachment(track_id: u32, attachment_id: u32);

        #[wasm_bindgen(js_name = dropRemoteMediaTrack)]
        pub fn drop_remote_media_track(track_id: u32);

        #[wasm_bindgen(js_name = dropRemoteMediaTrackDescriptor)]
        pub fn drop_remote_media_track_descriptor(descriptor: JsValue);

        #[wasm_bindgen(catch, js_name = rawRemoteMediaTrack)]
        pub fn raw_remote_media_track(track_id: u32) -> Result<JsValue, JsValue>;

        #[wasm_bindgen(catch, js_name = createRemoteAudioAnalyser)]
        pub fn create_remote_audio_analyser(
            track_id: u32,
            on_terminal: &js_sys::Function,
        ) -> Result<u32, JsValue>;

        #[wasm_bindgen(catch, js_name = createLocalAudioAnalyser)]
        pub fn create_local_audio_analyser(
            track_id: u32,
            on_terminal: &js_sys::Function,
        ) -> Result<u32, JsValue>;

        #[wasm_bindgen(catch, js_name = readAudioAnalyser)]
        pub fn read_audio_analyser(analyser_id: u32) -> Result<js_sys::Float32Array, JsValue>;

        #[wasm_bindgen(catch, js_name = closeAudioAnalyser)]
        pub fn close_audio_analyser(analyser_id: u32) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(js_name = dropAudioAnalyser)]
        pub fn drop_audio_analyser(analyser_id: u32);

        #[wasm_bindgen(catch, js_name = startRoomAudio)]
        pub fn start_room_audio(room_id: u32) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = createCameraVideoTrack)]
        pub fn create_camera_video_track(
            options: JsValue,
            raw_options: JsValue,
            on_complete: &js_sys::Function,
        ) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = localVideoTrackDeviceId)]
        pub fn local_video_track_device_id(track_id: u32) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = restartLocalVideoTrack)]
        pub fn restart_local_video_track(
            track_id: u32,
            on_complete: &js_sys::Function,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = restartLocalVideoTrackWithOptions)]
        pub fn restart_local_video_track_with_options(
            track_id: u32,
            options: JsValue,
            raw_options: JsValue,
            on_complete: &js_sys::Function,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = switchLocalVideoTrackDevice)]
        pub fn switch_local_video_track_device(
            track_id: u32,
            device_id: &str,
            on_complete: &js_sys::Function,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = attachLocalVideoTrack)]
        pub fn attach_local_video_track(
            track_id: u32,
            element: &web_sys::HtmlVideoElement,
        ) -> Result<u32, JsValue>;

        #[wasm_bindgen(catch, js_name = detachLocalVideoTrack)]
        pub fn detach_local_video_track(track_id: u32, attachment_id: u32) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = stopLocalVideoTrack)]
        pub fn stop_local_video_track(track_id: u32) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(js_name = dropLocalVideoTrackAttachment)]
        pub fn drop_local_video_track_attachment(track_id: u32, attachment_id: u32);

        #[wasm_bindgen(js_name = dropLocalVideoTrack)]
        pub fn drop_local_video_track(track_id: u32);

        #[wasm_bindgen(catch, js_name = rawLocalMediaTrack)]
        pub fn raw_local_media_track(track_id: u32, kind: &str) -> Result<JsValue, JsValue>;

        #[wasm_bindgen(catch, js_name = rawLocalPublication)]
        pub fn raw_local_publication(
            track_id: u32,
            publication_id: u32,
            kind: &str,
        ) -> Result<JsValue, JsValue>;

        #[wasm_bindgen(catch, js_name = setLocalMediaTrackProcessor)]
        pub fn set_local_media_track_processor(
            track_id: u32,
            kind: &str,
            processor: JsValue,
            show_processed_stream_locally: bool,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = rawLocalMediaTrackProcessor)]
        pub fn raw_local_media_track_processor(
            track_id: u32,
            kind: &str,
        ) -> Result<JsValue, JsValue>;

        #[wasm_bindgen(catch, js_name = stopLocalMediaTrackProcessor)]
        pub fn stop_local_media_track_processor(
            track_id: u32,
            kind: &str,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = publishLocalVideoTrack)]
        pub fn publish_local_video_track(
            room_id: u32,
            track_id: u32,
            publication_id: u32,
            options: JsValue,
            raw_options: JsValue,
            on_replaced: &js_sys::Function,
            on_complete: &js_sys::Function,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = setLocalVideoTrackMuted)]
        pub fn set_local_video_track_muted(
            track_id: u32,
            publication_id: u32,
            muted: bool,
            on_complete: &js_sys::Function,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = unpublishLocalVideoTrack)]
        pub fn unpublish_local_video_track(
            room_id: u32,
            track_id: u32,
            publication_id: u32,
            on_complete: &js_sys::Function,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = localVideoSenderStatistics)]
        pub fn local_video_sender_statistics(
            track_id: u32,
            publication_id: u32,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = rawLocalRtcStatsReport)]
        pub fn raw_local_rtc_stats_report(
            track_id: u32,
            publication_id: u32,
            kind: &str,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = createMicrophoneAudioTrack)]
        pub fn create_microphone_audio_track(
            options: JsValue,
            raw_options: JsValue,
            on_complete: &js_sys::Function,
        ) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = createScreenCapture)]
        pub fn create_screen_capture(
            options: JsValue,
            raw_options: JsValue,
            on_complete: &js_sys::Function,
        ) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = observeLocalVideoTrackEnded)]
        pub fn observe_local_video_track_ended(
            track_id: u32,
            on_ended: &js_sys::Function,
        ) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = observeLocalAudioTrackEnded)]
        pub fn observe_local_audio_track_ended(
            track_id: u32,
            on_ended: &js_sys::Function,
        ) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = localAudioTrackDeviceId)]
        pub fn local_audio_track_device_id(track_id: u32) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = restartLocalAudioTrack)]
        pub fn restart_local_audio_track(
            track_id: u32,
            on_complete: &js_sys::Function,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = restartLocalAudioTrackWithOptions)]
        pub fn restart_local_audio_track_with_options(
            track_id: u32,
            options: JsValue,
            raw_options: JsValue,
            on_complete: &js_sys::Function,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = switchLocalAudioTrackDevice)]
        pub fn switch_local_audio_track_device(
            track_id: u32,
            device_id: &str,
            on_complete: &js_sys::Function,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = attachLocalAudioTrack)]
        pub fn attach_local_audio_track(
            track_id: u32,
            element: &web_sys::HtmlAudioElement,
        ) -> Result<u32, JsValue>;

        #[wasm_bindgen(catch, js_name = detachLocalAudioTrack)]
        pub fn detach_local_audio_track(track_id: u32, attachment_id: u32) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = stopLocalAudioTrack)]
        pub fn stop_local_audio_track(track_id: u32) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(js_name = dropLocalAudioTrackAttachment)]
        pub fn drop_local_audio_track_attachment(track_id: u32, attachment_id: u32);

        #[wasm_bindgen(js_name = dropLocalAudioTrack)]
        pub fn drop_local_audio_track(track_id: u32);

        #[wasm_bindgen(catch, js_name = publishLocalAudioTrack)]
        pub fn publish_local_audio_track(
            room_id: u32,
            track_id: u32,
            publication_id: u32,
            options: JsValue,
            raw_options: JsValue,
            on_replaced: &js_sys::Function,
            on_complete: &js_sys::Function,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = setLocalAudioTrackMuted)]
        pub fn set_local_audio_track_muted(
            track_id: u32,
            publication_id: u32,
            muted: bool,
            on_complete: &js_sys::Function,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = unpublishLocalAudioTrack)]
        pub fn unpublish_local_audio_track(
            room_id: u32,
            track_id: u32,
            publication_id: u32,
            on_complete: &js_sys::Function,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = localAudioSenderStatistics)]
        pub fn local_audio_sender_statistics(
            track_id: u32,
            publication_id: u32,
        ) -> Result<js_sys::Promise, JsValue>;

        #[wasm_bindgen(catch, js_name = createLocalClipMediaStream)]
        pub fn create_local_clip_media_stream(
            audio_track_ids: &js_sys::Array,
            video_track_ids: &js_sys::Array,
        ) -> Result<JsValue, JsValue>;

        #[wasm_bindgen(js_name = localClipRecordingSupported)]
        pub fn local_clip_recording_supported() -> bool;

        #[wasm_bindgen(js_name = selectLocalClipMimeType)]
        pub fn select_local_clip_mime_type(preferences: &js_sys::Array) -> Option<String>;

        #[wasm_bindgen(catch, js_name = startLocalClipRecording)]
        pub fn start_local_clip_recording(
            stream: &web_sys::MediaStream,
            selected_mime_type: &str,
            max_duration_ms: f64,
            max_bytes: f64,
            on_event: &js_sys::Function,
        ) -> Result<JsValue, JsValue>;

        #[wasm_bindgen(catch, js_name = pauseLocalClipRecording)]
        pub fn pause_local_clip_recording(recording_id: u32) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = resumeLocalClipRecording)]
        pub fn resume_local_clip_recording(recording_id: u32) -> Result<(), JsValue>;

        #[wasm_bindgen(catch, js_name = stopLocalClipRecording)]
        pub fn stop_local_clip_recording(recording_id: u32) -> Result<(), JsValue>;

        #[wasm_bindgen(js_name = dropLocalClipRecording)]
        pub fn drop_local_clip_recording(recording_id: u32);

        #[wasm_bindgen(catch, js_name = createLocalClipObjectUrl)]
        pub fn create_local_clip_object_url(blob: &web_sys::Blob) -> Result<String, JsValue>;

        #[wasm_bindgen(catch, js_name = revokeLocalClipObjectUrl)]
        pub fn revoke_local_clip_object_url(url: &str) -> Result<(), JsValue>;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_executor::block_on;
    use futures_util::{FutureExt, StreamExt};
    use std::path::PathBuf;
    use ts_rs::{Config, TS};

    const ADAPTER_CONTRACTS_HEADER: &str =
        "// Generated from Rust DTOs by npm run contracts:generate. Do not edit.\n\n";

    fn adapter_contracts_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("js/src/generated/adapter-contracts.ts")
    }

    fn append_contract<T: TS>(output: &mut String, config: &Config) {
        output.push_str("export ");
        output.push_str(&T::decl(config));
        output.push_str("\n\n");
    }

    fn render_adapter_contracts() -> String {
        let mut output = String::from(ADAPTER_CONTRACTS_HEADER);
        let config = Config::default();

        // Keep this registry explicit and ordered so contract additions are intentional and diffs
        // are stable across filesystems.
        append_contract::<BrowserCapabilities>(&mut output, &config);
        append_contract::<RoomConnectionState>(&mut output, &config);
        append_contract::<ConnectionQuality>(&mut output, &config);
        append_contract::<AudioPlaybackState>(&mut output, &config);
        append_contract::<TrackSource>(&mut output, &config);
        append_contract::<TrackKind>(&mut output, &config);
        append_contract::<ParticipantGrants>(&mut output, &config);
        append_contract::<LocalTrackPublicationSnapshot>(&mut output, &config);
        append_contract::<LocalParticipantSnapshot>(&mut output, &config);
        append_contract::<RemoteDataTrackSnapshot>(&mut output, &config);
        append_contract::<AdapterRemoteTrackPublicationSnapshot>(&mut output, &config);
        append_contract::<AdapterRemoteParticipantSnapshot>(&mut output, &config);
        append_contract::<AdapterRoomSnapshot>(&mut output, &config);
        append_contract::<RoomEventKind>(&mut output, &config);
        append_contract::<AdapterJoin>(&mut output, &config);
        append_contract::<AdapterUpdate>(&mut output, &config);
        append_contract::<RuntimeInspection>(&mut output, &config);
        append_contract::<AdapterScreenCapture>(&mut output, &config);
        append_contract::<RemoteParticipantLocator>(&mut output, &config);
        append_contract::<RemotePublicationLocator>(&mut output, &config);
        append_contract::<AdapterRemoteMediaTrack>(&mut output, &config);
        append_contract::<AdapterRpcOptions>(&mut output, &config);
        append_contract::<AdapterRpcFailure>(&mut output, &config);
        append_contract::<AdapterRpcInvocation>(&mut output, &config);
        append_contract::<AdapterRpcHandlerCompletion>(&mut output, &config);
        append_contract::<TextStreamOptions>(&mut output, &config);
        append_contract::<ByteStreamOptions>(&mut output, &config);
        append_contract::<TextStreamInfo>(&mut output, &config);
        append_contract::<ByteStreamInfo>(&mut output, &config);
        append_contract::<AdapterOutgoingTextStream>(&mut output, &config);
        append_contract::<AdapterOutgoingByteStream>(&mut output, &config);
        append_contract::<AdapterTextStreamRead>(&mut output, &config);
        append_contract::<AdapterIncomingTextStream>(&mut output, &config);
        append_contract::<AdapterIncomingByteStream>(&mut output, &config);
        append_contract::<AdapterByteStreamRead>(&mut output, &config);
        assert_eq!(output.pop(), Some('\n'));

        assert!(output.is_ascii(), "adapter contracts must remain ASCII");
        assert!(
            !output.contains('\r'),
            "adapter contracts must use LF endings"
        );
        output
    }

    #[test]
    #[ignore = "writes the committed TypeScript adapter contracts"]
    fn generate_adapter_contracts() {
        let path = adapter_contracts_path();
        std::fs::create_dir_all(path.parent().expect("generated contracts parent"))
            .expect("create generated contracts directory");
        std::fs::write(path, render_adapter_contracts())
            .expect("write generated adapter contracts");
    }

    #[test]
    fn adapter_contracts_are_fresh() {
        let path = adapter_contracts_path();
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "read generated adapter contracts at {}: {error}",
                path.display()
            )
        });
        assert_eq!(
            committed,
            render_adapter_contracts(),
            "adapter contracts are stale; run npm run contracts:generate --prefix crates/livekit-browser/js"
        );
    }

    #[test]
    fn sdk_log_levels_map_to_typed_records_and_silent_is_ignored() {
        assert_eq!(
            [0, 1, 2, 3, 4].map(LiveKitLogLevel::from_sdk_level),
            [
                Some(LiveKitLogLevel::Trace),
                Some(LiveKitLogLevel::Debug),
                Some(LiveKitLogLevel::Info),
                Some(LiveKitLogLevel::Warn),
                Some(LiveKitLogLevel::Error),
            ]
        );
        assert_eq!(LiveKitLogLevel::from_sdk_level(5), None);
        assert_eq!(LiveKitLogLevel::from_sdk_level(u8::MAX), None);
    }

    #[cfg(feature = "tracing")]
    #[test]
    fn tracing_integration_emits_each_level_with_message_and_context() {
        use std::sync::{Arc, Mutex};
        use tracing::{
            field::{Field, Visit},
            span::{Attributes, Id, Record},
            Event, Level, Metadata, Subscriber,
        };

        #[derive(Default)]
        struct Fields(String);

        impl Visit for Fields {
            fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
                use std::fmt::Write;
                let _ = write!(self.0, "{}={value:?};", field.name());
            }
        }

        struct Capture(Arc<Mutex<Vec<(Level, String)>>>);

        impl Subscriber for Capture {
            fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
                true
            }

            fn new_span(&self, _span: &Attributes<'_>) -> Id {
                Id::from_u64(1)
            }

            fn record(&self, _span: &Id, _values: &Record<'_>) {}

            fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

            fn event(&self, event: &Event<'_>) {
                let mut fields = Fields::default();
                event.record(&mut fields);
                self.0
                    .lock()
                    .expect("capture lock")
                    .push((*event.metadata().level(), fields.0));
            }

            fn enter(&self, _span: &Id) {}

            fn exit(&self, _span: &Id) {}
        }

        let captured = Arc::new(Mutex::new(Vec::new()));
        tracing::subscriber::with_default(Capture(Arc::clone(&captured)), || {
            for level in [
                LiveKitLogLevel::Trace,
                LiveKitLogLevel::Debug,
                LiveKitLogLevel::Info,
                LiveKitLogLevel::Warn,
                LiveKitLogLevel::Error,
            ] {
                emit_tracing_record(&LiveKitLogRecord {
                    level,
                    message: "connected".into(),
                    context: serde_json::json!({ "room": "RM_test" }),
                });
            }
        });

        let captured = captured.lock().expect("capture lock");
        assert_eq!(
            captured.iter().map(|event| event.0).collect::<Vec<_>>(),
            [
                Level::TRACE,
                Level::DEBUG,
                Level::INFO,
                Level::WARN,
                Level::ERROR
            ]
        );
        assert!(
            captured
                .iter()
                .all(|event| event.1.contains("message=connected")
                    && event.1.contains("room")
                    && event.1.contains("RM_test")),
            "captured fields: {captured:?}"
        );
    }

    fn supported_capabilities() -> BrowserCapabilities {
        BrowserCapabilities {
            web_rtc: true,
            secure_context: true,
            user_media: true,
            display_capture: true,
            audio_playback: true,
            audio_output_selection: false,
            transcription: true,
            local_clip_recording: true,
        }
    }

    fn test_snapshot(revision: u64) -> RoomSnapshot {
        RoomSnapshot {
            revision,
            connection: RoomConnectionState::Connected,
            audio_playback: AudioPlaybackState::Allowed,
            active_speakers: Vec::new(),
            room_sid: "RM_test".into(),
            room_name: "test".into(),
            room_metadata: None,
            local_participant: LocalParticipantSnapshot {
                identity: "test".into(),
                sid: "PA_test".into(),
                connection_quality: ConnectionQuality::Unknown,
                metadata: None,
                attributes: BTreeMap::new(),
                grants: ParticipantGrants {
                    can_subscribe: true,
                    can_publish: true,
                    can_publish_data: true,
                    can_publish_sources: Vec::new(),
                    can_update_metadata: true,
                    hidden: false,
                    recorder: false,
                    agent: false,
                    can_subscribe_metrics: false,
                    can_manage_agent_session: false,
                },
                track_publications: BTreeMap::new(),
            },
            remote_participants: BTreeMap::new(),
        }
    }

    #[test]
    fn ready_inspection_returns_typed_capabilities() {
        let capabilities = supported_capabilities();
        let inspection: RuntimeInspection = serde_json::from_str(
            r#"{
                "status": "ready",
                "capabilities": {
                    "web_rtc": true,
                    "secure_context": true,
                    "user_media": true,
                    "display_capture": true,
                    "audio_playback": true,
                    "audio_output_selection": false,
                    "transcription": true,
                    "local_clip_recording": true
                }
            }"#,
        )
        .unwrap();

        assert_eq!(finish_initialization(inspection), Ok(capabilities));
        assert!(capabilities.transcription());
        assert!(capabilities.local_clip_recording());
    }

    #[test]
    fn setup_failures_map_to_actionable_errors() {
        let inspection = |json| serde_json::from_str(json).unwrap();

        assert_eq!(
            finish_initialization(inspection(r#"{"status":"sdkMissing"}"#)),
            Err(Error::SdkMissing)
        );
        assert_eq!(
            finish_initialization(inspection(
                r#"{"status":"sdkVersionMismatch","actual_version":"2.20.0"}"#,
            )),
            Err(Error::SdkVersionMismatch {
                expected: LIVEKIT_CLIENT_VERSION,
                actual: Some("2.20.0".into()),
            })
        );
        assert_eq!(
            finish_initialization(inspection(r#"{"status":"unsupportedBrowser"}"#)),
            Err(Error::UnsupportedBrowser {
                missing: BrowserCapability::WebRtc,
            })
        );
    }

    #[test]
    fn media_operation_failures_have_actionable_device_categories() {
        let error = |name: &str| AdapterError {
            name: name.into(),
            message: "capture failed".into(),
            status: None,
            reason_name: None,
        };

        for name in ["NotAllowedError", "SecurityError", "PermissionDeniedError"] {
            assert!(matches!(
                classify_capture_error(error(name)),
                Error::PermissionDenied(_)
            ));
        }
        for name in [
            "NotFoundError",
            "OverconstrainedError",
            "DevicesNotFoundError",
        ] {
            assert!(matches!(
                classify_capture_error(error(name)),
                Error::DeviceUnavailable(_)
            ));
        }
        for name in ["NotReadableError", "AbortError", "TrackStartError"] {
            assert!(matches!(
                classify_capture_error(error(name)),
                Error::DeviceInUse(_)
            ));
        }
        assert!(matches!(
            classify_capture_error(error("UnknownError")),
            Error::CaptureFailed(_)
        ));
    }

    #[test]
    fn display_capture_failures_have_actionable_categories() {
        let error = |name: &str| AdapterError {
            name: name.into(),
            message: "display capture failed".into(),
            status: None,
            reason_name: None,
        };

        assert!(matches!(
            classify_display_capture_error(error("DeviceUnsupportedError")),
            Error::UnsupportedBrowser {
                missing: BrowserCapability::DisplayCapture
            }
        ));
        assert!(matches!(
            classify_display_capture_error(error("NotAllowedError")),
            Error::DisplayCaptureNotAllowed(_)
        ));
        assert!(matches!(
            classify_display_capture_error(error("InvalidStateError")),
            Error::DisplayCaptureRequiresActivation(_)
        ));
        assert!(matches!(
            classify_display_capture_error(error("SecurityError")),
            Error::PermissionDenied(_)
        ));
        assert!(matches!(
            classify_display_capture_error(error("AbortError")),
            Error::CaptureFailed(_)
        ));
    }

    #[test]
    fn publisher_mutation_permission_failures_are_typed() {
        let permission_error = AdapterError {
            name: "PublishTrackError".into(),
            message: "insufficient permissions".into(),
            status: Some(403),
            reason_name: None,
        };
        let other_error = AdapterError {
            name: "PublishTrackError".into(),
            message: "publication failed".into(),
            status: Some(500),
            reason_name: None,
        };
        let publisher_permissions_error = AdapterError {
            name: "SignalError".into(),
            message: "permission update failed".into(),
            status: Some(500),
            reason_name: None,
        };

        assert!(matches!(
            classify_publisher_mutation_error(permission_error.clone()),
            Error::PermissionDenied(_)
        ));
        assert!(matches!(
            classify_subscription_permission_error(permission_error),
            Error::PermissionDenied(_)
        ));
        assert!(matches!(
            classify_publisher_mutation_error(other_error),
            Error::PublicationFailed(_)
        ));
        assert!(matches!(
            classify_subscription_permission_error(publisher_permissions_error),
            Error::PublisherPermissionsFailed(_)
        ));
    }

    #[test]
    fn participant_update_failures_classify_livekit_denials() {
        let denied = AdapterError {
            name: "SignalRequestError".into(),
            message: "metadata update rejected".into(),
            status: None,
            reason_name: Some("NOT_ALLOWED".into()),
        };
        let timed_out = AdapterError {
            name: "SignalRequestError".into(),
            message: "metadata update timed out".into(),
            status: None,
            reason_name: Some("TimeoutError".into()),
        };

        assert!(matches!(
            classify_participant_update_error(denied),
            Error::PermissionDenied(_)
        ));
        assert!(matches!(
            classify_participant_update_error(timed_out),
            Error::ParticipantUpdateFailed(_)
        ));
    }

    #[test]
    fn data_packet_send_failures_classify_livekit_denials() {
        let denied = AdapterError {
            name: "PermissionDeniedError".into(),
            message: "data publishing rejected".into(),
            status: Some(403),
            reason_name: None,
        };
        let failed = AdapterError {
            name: "DataChannelError".into(),
            message: "data channel unavailable".into(),
            status: None,
            reason_name: None,
        };

        assert!(matches!(
            classify_data_packet_send_error(denied),
            Error::PermissionDenied(_)
        ));
        assert!(matches!(
            classify_data_packet_send_error(failed),
            Error::DataPacketSendFailed(_)
        ));
    }

    #[test]
    fn chat_message_send_failures_classify_livekit_denials() {
        let denied = AdapterError {
            name: "PermissionDeniedError".into(),
            message: "chat publishing rejected".into(),
            status: Some(403),
            reason_name: None,
        };
        let failed = AdapterError {
            name: "DataChannelError".into(),
            message: "data channel unavailable".into(),
            status: None,
            reason_name: None,
        };

        assert!(matches!(
            classify_chat_message_send_error(denied),
            Error::PermissionDenied(_)
        ));
        assert!(matches!(
            classify_chat_message_send_error(failed),
            Error::ChatMessageSendFailed(_)
        ));
    }

    #[test]
    fn terminal_room_session_rejects_local_participant_updates() {
        let session = RoomSession::test_session(test_snapshot(1));
        let participant = session.local_participant();
        block_on(session.leave()).unwrap();
        let terminal_revision = session.snapshot().revision();

        assert!(matches!(
            block_on(participant.set_metadata("ready")),
            Err(Error::InvalidState {
                operation: "update Local Participant metadata",
                reason: "Room Session is terminal",
            })
        ));
        assert!(matches!(
            block_on(
                participant.update_attributes(BTreeMap::from([("status".into(), "ready".into(),)]))
            ),
            Err(Error::InvalidState {
                operation: "update Local Participant attributes",
                reason: "Room Session is terminal",
            })
        ));
        assert_eq!(session.snapshot().revision(), terminal_revision);
        assert_eq!(session.snapshot().local_participant().metadata(), None);
        assert!(session
            .snapshot()
            .local_participant()
            .attributes()
            .is_empty());
    }

    #[test]
    fn media_devices_preserve_ids_kinds_and_permission_gated_labels() {
        let devices: Vec<MediaDeviceInfo> = serde_json::from_str(
            r#"[
                {
                    "device_id": "camera-front",
                    "group_id": "camera-group",
                    "label": "",
                    "kind": "camera"
                },
                {
                    "device_id": "microphone-default",
                    "group_id": "audio-group",
                    "label": "Built-in microphone",
                    "kind": "microphone"
                }
            ]"#,
        )
        .unwrap();

        assert_eq!(devices[0].kind(), MediaDeviceKind::Camera);
        assert_eq!(devices[0].device_id(), "camera-front");
        assert_eq!(devices[0].group_id(), "camera-group");
        assert_eq!(devices[0].label(), "");
        assert_eq!(devices[1].kind(), MediaDeviceKind::Microphone);
        assert_eq!(devices[1].label(), "Built-in microphone");
    }

    #[test]
    fn camera_capture_options_serialize_typed_common_constraints() {
        let options = CameraCaptureOptions::default()
            .with_device_id("camera-rear")
            .with_facing_mode(FacingMode::Environment)
            .with_resolution(CameraResolution::new(1280, 720))
            .with_frame_rate(30.0);

        assert_eq!(
            serde_json::to_value(options).unwrap(),
            serde_json::json!({
                "deviceId": { "exact": "camera-rear" },
                "facingMode": "environment",
                "resolution": { "width": 1280, "height": 720 },
                "frameRate": 30.0,
            })
        );
    }

    #[test]
    fn microphone_capture_options_serialize_typed_common_constraints() {
        let options = MicrophoneCaptureOptions::default()
            .with_device_id("microphone-usb")
            .with_auto_gain_control(false)
            .with_channel_count(1)
            .with_echo_cancellation(true)
            .with_latency(0.02)
            .with_noise_suppression(true)
            .with_sample_rate(48_000)
            .with_sample_size(16);

        assert_eq!(
            serde_json::to_value(options).unwrap(),
            serde_json::json!({
                "deviceId": { "exact": "microphone-usb" },
                "autoGainControl": false,
                "channelCount": 1,
                "echoCancellation": true,
                "latency": 0.02,
                "noiseSuppression": true,
                "sampleRate": 48000,
                "sampleSize": 16,
            })
        );
    }

    #[test]
    fn screen_capture_options_request_optional_system_audio() {
        assert_eq!(
            serde_json::to_value(ScreenCaptureOptions::default()).unwrap(),
            serde_json::json!({ "audio": false })
        );
        assert_eq!(
            serde_json::to_value(ScreenCaptureOptions::default().with_system_audio(true)).unwrap(),
            serde_json::json!({ "audio": true })
        );
    }

    #[test]
    fn room_options_default_to_automatic_subscription() {
        assert!(RoomOptions::default().auto_subscribe());
        assert!(!RoomOptions::default()
            .with_auto_subscribe(false)
            .auto_subscribe());
    }

    #[test]
    fn publisher_permissions_use_participant_identity() {
        let permissions =
            PublicationSubscriptionPermissions::participant_specific(["teacher", "student"]);

        assert!(!permissions.allows_all_participants());
        assert_eq!(
            permissions.participant_identities(),
            &["teacher".to_string(), "student".to_string()]
        );
        assert_eq!(
            serde_json::to_value(permissions).unwrap(),
            serde_json::json!({
                "allParticipantsAllowed": false,
                "participantIdentities": ["teacher", "student"],
            })
        );
        assert!(PublicationSubscriptionPermissions::default().allows_all_participants());
    }

    #[test]
    fn missing_display_capture_is_rejected_before_browser_mutation() {
        let mut capabilities = supported_capabilities();
        capabilities.display_capture = false;
        let client = BrowserClient {
            inner: Rc::new(ClientInner { capabilities }),
        };

        assert!(matches!(
            block_on(client.create_screen_capture(ScreenCaptureOptions::default())),
            Err(Error::UnsupportedBrowser {
                missing: BrowserCapability::DisplayCapture
            })
        ));
    }

    #[test]
    fn missing_user_media_is_rejected_before_browser_mutation() {
        let mut capabilities = supported_capabilities();
        capabilities.user_media = false;
        let client = BrowserClient {
            inner: Rc::new(ClientInner { capabilities }),
        };

        assert!(matches!(
            block_on(client.create_camera_video_track()),
            Err(Error::UnsupportedBrowser {
                missing: BrowserCapability::UserMedia
            })
        ));
        assert!(matches!(
            block_on(client.create_microphone_audio_track()),
            Err(Error::UnsupportedBrowser {
                missing: BrowserCapability::UserMedia
            })
        ));
        assert!(matches!(
            block_on(client.enumerate_media_devices(MediaDeviceKind::Camera, true)),
            Err(Error::UnsupportedBrowser {
                missing: BrowserCapability::UserMedia
            })
        ));
        assert!(matches!(
            block_on(client.enumerate_media_devices(MediaDeviceKind::Camera, false)),
            Err(Error::UnsupportedTarget)
        ));
    }

    #[test]
    fn room_events_follow_the_snapshot_they_are_tagged_with() {
        let session = RoomSession::test_session(test_snapshot(1));
        let mut events = session.subscribe_events(1);

        session.install_test_update(
            test_snapshot(2),
            RoomEventKind::ConnectionStateChanged {
                state: RoomConnectionState::Reconnecting,
            },
        );
        let event = block_on(events.next()).unwrap();

        assert_eq!(event.revision(), 2);
        assert_eq!(session.snapshot().revision(), event.revision());
        assert_eq!(
            event.kind(),
            &RoomEventKind::ConnectionStateChanged {
                state: RoomConnectionState::Reconnecting,
            }
        );
    }

    #[test]
    fn participant_connection_quality_is_in_revision_tagged_snapshot_events() {
        let mut initial = test_snapshot(1);
        initial.remote_participants.insert(
            "student".into(),
            RemoteParticipantSnapshot {
                identity: "student".into(),
                sid: "PA_student".into(),
                generation: 1,
                connection_quality: ConnectionQuality::Poor,
                metadata: None,
                attributes: BTreeMap::new(),
                track_publications: BTreeMap::new(),
                data_tracks: BTreeMap::new(),
            },
        );
        let session = RoomSession::test_session(initial);
        let mut events = session.subscribe_events(1);
        let mut updated = session.snapshot().as_ref().clone();
        updated.revision = 2;
        updated
            .remote_participants
            .get_mut("student")
            .unwrap()
            .connection_quality = ConnectionQuality::Excellent;
        let quality_event: RoomEventKind = serde_json::from_str(
            r#"{
                "type": "participantConnectionQualityChanged",
                "participant_identity": "student",
                "quality": "excellent"
            }"#,
        )
        .unwrap();

        session.install_test_update(updated, quality_event.clone());

        let event = block_on(events.next()).unwrap();
        assert_eq!(event.revision(), 2);
        assert_eq!(session.snapshot().revision(), event.revision());
        assert_eq!(
            session
                .snapshot()
                .remote_participants()
                .get("student")
                .unwrap()
                .connection_quality(),
            ConnectionQuality::Excellent
        );
        assert_eq!(event.kind(), &quality_event);
    }

    #[test]
    fn active_speakers_are_ordered_revision_tagged_snapshot_state() {
        let session = RoomSession::test_session(test_snapshot(1));
        let mut events = session.subscribe_events(1);
        let mut updated = session.snapshot().as_ref().clone();
        updated.revision = 2;
        updated.active_speakers = vec!["student".into(), "teacher".into()];
        let speaker_event: RoomEventKind =
            serde_json::from_str(r#"{"type":"activeSpeakersChanged"}"#).unwrap();

        session.install_test_update(updated, speaker_event.clone());

        let event = block_on(events.next()).unwrap();
        assert_eq!(event.revision(), 2);
        assert_eq!(session.snapshot().revision(), event.revision());
        assert_eq!(
            session.snapshot().active_speakers(),
            &["student".to_string(), "teacher".to_string()]
        );
        assert_eq!(event.kind(), &speaker_event);
    }

    #[test]
    fn audio_sender_statistics_preserve_normalized_sdk_fields() {
        let statistics: AudioSenderStatistics = serde_json::from_str(
            r#"{
                "timestamp": 1234.5,
                "packets_sent": 41,
                "bytes_sent": 8200,
                "jitter": 0.003,
                "packets_lost": -1,
                "round_trip_time": 0.04,
                "stream_id": "outbound-audio"
            }"#,
        )
        .unwrap();

        assert_eq!(statistics.timestamp(), 1234.5);
        assert_eq!(statistics.packets_sent(), Some(41));
        assert_eq!(statistics.bytes_sent(), Some(8200));
        assert_eq!(statistics.jitter(), Some(0.003));
        assert_eq!(statistics.packets_lost(), Some(-1));
        assert_eq!(statistics.round_trip_time(), Some(0.04));
        assert_eq!(statistics.stream_id(), Some("outbound-audio"));
    }

    #[test]
    fn video_sender_statistics_preserve_normalized_sdk_fields() {
        let statistics: VideoSenderStatistics = serde_json::from_str(
            r#"{
                "timestamp": 2234.5,
                "packets_sent": 101,
                "bytes_sent": 64000,
                "jitter": 0.004,
                "packets_lost": 2,
                "round_trip_time": 0.05,
                "stream_id": "outbound-video",
                "fir_count": 1,
                "pli_count": 2,
                "nack_count": 3,
                "rid": "f",
                "frame_width": 1280,
                "frame_height": 720,
                "frames_per_second": 30.0,
                "frames_sent": 90,
                "quality_limitation_reason": "bandwidth",
                "quality_limitation_durations": {"bandwidth": 1.5, "none": 4.0},
                "quality_limitation_resolution_changes": 2,
                "retransmitted_packets_sent": 4,
                "target_bitrate": 1500000.0
            }"#,
        )
        .unwrap();

        assert_eq!(statistics.sender().timestamp(), 2234.5);
        assert_eq!(statistics.sender().packets_sent(), Some(101));
        assert_eq!(statistics.sender().bytes_sent(), Some(64000));
        assert_eq!(statistics.sender().jitter(), Some(0.004));
        assert_eq!(statistics.sender().packets_lost(), Some(2));
        assert_eq!(statistics.sender().round_trip_time(), Some(0.05));
        assert_eq!(statistics.sender().stream_id(), Some("outbound-video"));
        assert_eq!(statistics.fir_count(), Some(1));
        assert_eq!(statistics.pli_count(), Some(2));
        assert_eq!(statistics.nack_count(), Some(3));
        assert_eq!(statistics.rid(), Some("f"));
        assert_eq!(statistics.frame_width(), Some(1280));
        assert_eq!(statistics.frame_height(), Some(720));
        assert_eq!(statistics.frames_per_second(), Some(30.0));
        assert_eq!(statistics.frames_sent(), Some(90));
        assert_eq!(statistics.quality_limitation_reason(), Some("bandwidth"));
        assert_eq!(
            statistics.quality_limitation_durations().unwrap()["bandwidth"],
            1.5
        );
        assert_eq!(statistics.quality_limitation_resolution_changes(), Some(2));
        assert_eq!(statistics.retransmitted_packets_sent(), Some(4));
        assert_eq!(statistics.target_bitrate(), Some(1_500_000.0));

        let without_durations: VideoSenderStatistics = serde_json::from_str(
            r#"{
                "timestamp": 2234.5,
                "fir_count": 0,
                "pli_count": 0,
                "nack_count": 0,
                "rid": "",
                "frame_width": 640,
                "frame_height": 360,
                "frames_per_second": 30.0,
                "frames_sent": 1,
                "target_bitrate": 500000.0
            }"#,
        )
        .unwrap();
        assert_eq!(without_durations.quality_limitation_durations(), None);
    }

    #[test]
    fn audio_receiver_statistics_preserve_normalized_sdk_fields() {
        let statistics: AudioReceiverStatistics = serde_json::from_str(
            r#"{
                "timestamp": 3234.5,
                "jitter_buffer_delay": 0.02,
                "packets_lost": 3,
                "packets_received": 200,
                "bytes_received": 48000,
                "stream_id": "inbound-audio",
                "jitter": 0.006,
                "concealed_samples": 10,
                "concealment_events": 2,
                "silent_concealed_samples": 4,
                "silent_concealment_events": 1,
                "total_audio_energy": 12.5,
                "total_samples_duration": 8.25
            }"#,
        )
        .unwrap();

        assert_eq!(statistics.receiver().timestamp(), 3234.5);
        assert_eq!(statistics.receiver().jitter_buffer_delay(), Some(0.02));
        assert_eq!(statistics.receiver().packets_lost(), Some(3));
        assert_eq!(statistics.receiver().packets_received(), Some(200));
        assert_eq!(statistics.receiver().bytes_received(), Some(48000));
        assert_eq!(statistics.receiver().stream_id(), Some("inbound-audio"));
        assert_eq!(statistics.receiver().jitter(), Some(0.006));
        assert_eq!(statistics.concealed_samples(), Some(10));
        assert_eq!(statistics.concealment_events(), Some(2));
        assert_eq!(statistics.silent_concealed_samples(), Some(4));
        assert_eq!(statistics.silent_concealment_events(), Some(1));
        assert_eq!(statistics.total_audio_energy(), Some(12.5));
        assert_eq!(statistics.total_samples_duration(), Some(8.25));
    }

    #[test]
    fn video_receiver_statistics_preserve_normalized_sdk_fields() {
        let statistics: VideoReceiverStatistics = serde_json::from_str(
            r#"{
                "timestamp": 4234.5,
                "jitter_buffer_delay": 0.03,
                "packets_lost": 4,
                "packets_received": 300,
                "bytes_received": 128000,
                "stream_id": "inbound-video",
                "jitter": 0.007,
                "frames_decoded": 120,
                "frames_dropped": 5,
                "frames_received": 125,
                "frame_width": 1920,
                "frame_height": 1080,
                "fir_count": 2,
                "pli_count": 3,
                "nack_count": 4,
                "decoder_implementation": "ExternalDecoder",
                "mime_type": "video/VP8"
            }"#,
        )
        .unwrap();

        assert_eq!(statistics.receiver().timestamp(), 4234.5);
        assert_eq!(statistics.receiver().jitter_buffer_delay(), Some(0.03));
        assert_eq!(statistics.receiver().packets_lost(), Some(4));
        assert_eq!(statistics.receiver().packets_received(), Some(300));
        assert_eq!(statistics.receiver().bytes_received(), Some(128000));
        assert_eq!(statistics.receiver().stream_id(), Some("inbound-video"));
        assert_eq!(statistics.receiver().jitter(), Some(0.007));
        assert_eq!(statistics.frames_decoded(), Some(120));
        assert_eq!(statistics.frames_dropped(), Some(5));
        assert_eq!(statistics.frames_received(), Some(125));
        assert_eq!(statistics.frame_width(), Some(1920));
        assert_eq!(statistics.frame_height(), Some(1080));
        assert_eq!(statistics.fir_count(), Some(2));
        assert_eq!(statistics.pli_count(), Some(3));
        assert_eq!(statistics.nack_count(), Some(4));
        assert_eq!(statistics.decoder_implementation(), Some("ExternalDecoder"));
        assert_eq!(statistics.mime_type(), Some("video/VP8"));
    }

    #[test]
    fn data_packet_delivery_is_mode_specific_and_separate_from_room_events() {
        let session = RoomSession::test_session(test_snapshot(1));
        let mut reliable = session.subscribe_data_packets(DataPacketDelivery::Reliable, 2);
        let mut lossy = session.subscribe_data_packets(DataPacketDelivery::Lossy, 2);
        let mut room_events = session.subscribe_events(1);

        session.install_test_data_packet(DataPacket {
            payload: vec![0, 255, 128, 65],
            sender_identity: Some("sender".into()),
            delivery: DataPacketDelivery::Reliable,
        });

        let DataPacketStreamItem::Packet(packet) = block_on(reliable.next()).unwrap() else {
            panic!("expected a reliable packet");
        };
        assert_eq!(packet.payload(), &[0, 255, 128, 65]);
        assert_eq!(packet.sender_identity(), Some("sender"));
        assert_eq!(packet.delivery(), DataPacketDelivery::Reliable);
        assert!(lossy.next().now_or_never().is_none());
        assert!(room_events.next().now_or_never().is_none());
    }

    #[test]
    fn chat_messages_are_typed_and_separate_from_room_events() {
        let session = RoomSession::test_session(test_snapshot(1));
        let mut messages = session.subscribe_chat_messages(2);
        let mut room_events = session.subscribe_events(1);
        let timestamp = std::time::UNIX_EPOCH + Duration::from_millis(1_721_234_567_890);
        let edit_timestamp = timestamp + Duration::from_millis(500);

        session.install_test_update(test_snapshot(2), RoomEventKind::LocalParticipantChanged);
        session.install_test_update(test_snapshot(3), RoomEventKind::RoomMetadataChanged);

        session.install_test_chat_message(ChatMessage {
            id: "message-1".into(),
            message: "hello lambda".into(),
            sender_identity: Some("sender".into()),
            timestamp,
            edit_timestamp: Some(edit_timestamp),
        });

        let ChatMessageStreamItem::Message(message) = block_on(messages.next()).unwrap() else {
            panic!("expected a chat message");
        };
        assert_eq!(message.id(), "message-1");
        assert_eq!(message.message(), "hello lambda");
        assert_eq!(message.sender_identity(), Some("sender"));
        assert_eq!(message.timestamp(), timestamp);
        assert_eq!(message.edit_timestamp(), Some(edit_timestamp));
        assert_eq!(
            block_on(room_events.next()).unwrap().kind(),
            &RoomEventKind::Lagged
        );
    }

    #[test]
    fn transcription_segments_are_typed_ordered_and_separate_from_room_events() {
        let session = RoomSession::test_session(test_snapshot(1));
        let mut transcriptions = session.subscribe_transcription_segments(2);
        let mut room_events = session.subscribe_events(1);
        let first_received_at = std::time::UNIX_EPOCH + Duration::from_millis(1_721_234_567_000);
        let interim_received_at = first_received_at + Duration::from_millis(25);
        let final_received_at = first_received_at + Duration::from_millis(70);

        for (text, is_final, last_received_at) in [
            ("hello wor", false, interim_received_at),
            ("hello world", true, final_received_at),
        ] {
            session.install_test_transcription_segment(TranscriptionSegment {
                id: "segment-1".into(),
                participant_identity: Some("speaker".into()),
                track_publication_sid: Some("TR_microphone".into()),
                text: text.into(),
                start_time: 120,
                end_time: 540,
                language: "en-US".into(),
                is_final,
                first_received_at,
                last_received_at,
            });
        }

        for (expected_text, expected_final, expected_last_received_at) in [
            ("hello wor", false, interim_received_at),
            ("hello world", true, final_received_at),
        ] {
            let TranscriptionSegmentStreamItem::Segment(segment) =
                block_on(transcriptions.next()).unwrap()
            else {
                panic!("expected a transcription segment");
            };
            assert_eq!(segment.id(), "segment-1");
            assert_eq!(segment.participant_identity(), Some("speaker"));
            assert_eq!(segment.track_publication_sid(), Some("TR_microphone"));
            assert_eq!(segment.text(), expected_text);
            assert_eq!(segment.start_time(), 120);
            assert_eq!(segment.end_time(), 540);
            assert_eq!(segment.language(), "en-US");
            assert_eq!(segment.is_final(), expected_final);
            assert_eq!(segment.first_received_at(), first_received_at);
            assert_eq!(segment.last_received_at(), expected_last_received_at);
        }
        assert!(room_events.next().now_or_never().is_none());
    }

    #[test]
    fn normalized_transcription_segment_converts_source_and_timing() {
        let normalized: AdapterTranscriptionSegment = serde_json::from_str(
            r#"{
                "id": "segment-1",
                "participant_identity": "speaker",
                "track_publication_sid": "TR_microphone",
                "text": "hello world",
                "start_time": 120,
                "end_time": 540,
                "language": "en-US",
                "is_final": true,
                "first_received_at_millis": 1721234567000,
                "last_received_at_millis": 1721234567070
            }"#,
        )
        .unwrap();

        let segment = normalized.into_segment().unwrap();

        assert_eq!(segment.participant_identity(), Some("speaker"));
        assert_eq!(segment.track_publication_sid(), Some("TR_microphone"));
        assert_eq!(segment.start_time(), 120);
        assert_eq!(segment.end_time(), 540);
        assert_eq!(
            segment
                .first_received_at()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap(),
            Duration::from_millis(1_721_234_567_000)
        );
        assert_eq!(
            segment
                .last_received_at()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap(),
            Duration::from_millis(1_721_234_567_070)
        );
        assert!(segment.is_final());

        let normalized_without_source: AdapterTranscriptionSegment = serde_json::from_str(
            r#"{
                "id": "segment-without-source",
                "participant_identity": null,
                "track_publication_sid": null,
                "text": "source unavailable",
                "start_time": 0,
                "end_time": 0,
                "language": "",
                "is_final": false,
                "first_received_at_millis": 1721234567000,
                "last_received_at_millis": 1721234567000
            }"#,
        )
        .unwrap();
        let segment_without_source = normalized_without_source.into_segment().unwrap();
        assert_eq!(segment_without_source.participant_identity(), None);
        assert_eq!(segment_without_source.track_publication_sid(), None);
    }

    #[test]
    fn transcription_segment_stream_reports_overflow_and_closes_when_leave_starts() {
        let session = RoomSession::test_session(test_snapshot(1));
        let mut transcriptions = session.subscribe_transcription_segments(1);

        for index in 1..=2 {
            session.install_test_transcription_segment(TranscriptionSegment {
                id: format!("segment-{index}"),
                participant_identity: Some("speaker".into()),
                track_publication_sid: Some("TR_microphone".into()),
                text: format!("text {index}"),
                start_time: index,
                end_time: index + 1,
                language: "en".into(),
                is_final: true,
                first_received_at: std::time::UNIX_EPOCH,
                last_received_at: std::time::UNIX_EPOCH,
            });
        }

        assert_eq!(
            block_on(transcriptions.next()),
            Some(TranscriptionSegmentStreamItem::Lagged { dropped: 1 })
        );
        let Some(TranscriptionSegmentStreamItem::Segment(segment)) =
            block_on(transcriptions.next())
        else {
            panic!("expected the retained transcription segment");
        };
        assert_eq!(segment.id(), "segment-2");

        let _leave = session.leave();

        assert_eq!(block_on(transcriptions.next()), None);
        assert_eq!(
            block_on(session.subscribe_transcription_segments(1).next()),
            None
        );
    }

    #[test]
    fn chat_message_stream_reports_exact_overflow_before_retained_messages() {
        let session = RoomSession::test_session(test_snapshot(1));
        let mut messages = session.subscribe_chat_messages(2);

        for index in 1..=4 {
            session.install_test_chat_message(ChatMessage {
                id: format!("message-{index}"),
                message: format!("message {index}"),
                sender_identity: Some("sender".into()),
                timestamp: std::time::UNIX_EPOCH + Duration::from_millis(index),
                edit_timestamp: None,
            });
        }

        assert_eq!(
            block_on(messages.next()),
            Some(ChatMessageStreamItem::Lagged { dropped: 2 })
        );
        for index in 3..=4 {
            let Some(ChatMessageStreamItem::Message(message)) = block_on(messages.next()) else {
                panic!("expected a retained chat message");
            };
            assert_eq!(message.id(), format!("message-{index}"));
        }
    }

    #[test]
    fn chat_message_stream_closes_when_leave_starts() {
        let session = RoomSession::test_session(test_snapshot(1));
        let mut messages = session.subscribe_chat_messages(1);

        let _leave = session.leave();

        assert_eq!(block_on(messages.next()), None);
        assert_eq!(block_on(session.subscribe_chat_messages(1).next()), None);
    }

    #[test]
    fn chat_message_send_rejects_a_terminal_session() {
        let session = RoomSession::test_session(test_snapshot(1));
        let local_participant = session.local_participant();
        let _leave = session.leave();

        assert!(matches!(
            block_on(local_participant.send_chat_message("hello")),
            Err(Error::InvalidState {
                operation: "send chat message",
                reason: "Room Session is terminal",
            })
        ));
    }

    #[test]
    fn data_packet_stream_reports_exact_overflow_before_retained_packets() {
        let session = RoomSession::test_session(test_snapshot(1));
        let mut packets = session.subscribe_data_packets(DataPacketDelivery::Lossy, 2);

        for byte in [1, 2, 3, 4] {
            session.install_test_data_packet(DataPacket {
                payload: vec![byte],
                sender_identity: Some("sender".into()),
                delivery: DataPacketDelivery::Lossy,
            });
        }

        assert_eq!(
            block_on(packets.next()),
            Some(DataPacketStreamItem::Lagged { dropped: 2 })
        );
        for byte in [3, 4] {
            let Some(DataPacketStreamItem::Packet(packet)) = block_on(packets.next()) else {
                panic!("expected retained packet");
            };
            assert_eq!(packet.payload(), &[byte]);
        }
    }

    #[test]
    fn data_packet_stream_closes_when_leave_starts() {
        let session = RoomSession::test_session(test_snapshot(1));
        let mut packets = session.subscribe_data_packets(DataPacketDelivery::Reliable, 1);

        let _leave = session.leave();

        assert_eq!(block_on(packets.next()), None);
        assert_eq!(
            block_on(
                session
                    .subscribe_data_packets(DataPacketDelivery::Reliable, 1)
                    .next()
            ),
            None
        );
    }

    #[test]
    fn data_packet_send_rejects_a_terminal_session() {
        let session = RoomSession::test_session(test_snapshot(1));
        let local_participant = session.local_participant();
        let _leave = session.leave();

        assert!(matches!(
            block_on(
                local_participant.send_data_packet(vec![0, 255], DataPacketDelivery::Reliable,)
            ),
            Err(Error::InvalidState {
                operation: "send data packet",
                reason: "Room Session is terminal",
            })
        ));
    }

    #[test]
    fn data_stream_options_preserve_topic_and_size() {
        let text = TextStreamOptions::new("student", "lesson").with_total_size(30_000);
        assert_eq!(
            serde_json::to_value(text).unwrap(),
            serde_json::json!({
                "destinationIdentity": "student",
                "topic": "lesson",
                "totalSize": 30_000,
            })
        );

        let bytes = ByteStreamOptions::new("student", "worksheet").with_total_size(45_000);
        assert_eq!(
            serde_json::to_value(bytes).unwrap(),
            serde_json::json!({
                "destinationIdentity": "student",
                "topic": "worksheet",
                "totalSize": 45_000,
            })
        );
    }

    #[test]
    fn data_stream_open_rejects_a_terminal_room_session() {
        let session = RoomSession::test_session(test_snapshot(1));
        let local_participant = session.local_participant();
        let _leave = session.leave();

        assert!(matches!(
            block_on(
                local_participant.open_text_stream(TextStreamOptions::new("student", "lesson"))
            ),
            Err(Error::InvalidState {
                operation: "open text stream",
                reason: "Room Session is terminal",
            })
        ));
        assert!(matches!(
            block_on(
                local_participant.open_byte_stream(ByteStreamOptions::new("student", "worksheet"))
            ),
            Err(Error::InvalidState {
                operation: "open byte stream",
                reason: "Room Session is terminal",
            })
        ));
    }

    #[test]
    fn data_stream_handlers_reject_a_terminal_room_session() {
        let session = RoomSession::test_session(test_snapshot(1));
        let _leave = session.leave();

        assert!(matches!(
            session.register_text_stream_handler("lesson", |_| {}),
            Err(Error::InvalidState {
                operation: "register text stream handler",
                reason: "Room Session is terminal",
            })
        ));
        assert!(matches!(
            session.register_byte_stream_handler("worksheet", |_| {}),
            Err(Error::InvalidState {
                operation: "register byte stream handler",
                reason: "Room Session is terminal",
            })
        ));
    }

    #[test]
    fn data_stream_failures_expose_livekit_reasons() {
        for (reason_name, expected) in [
            ("AbnormalEnd", DataStreamErrorReason::AbnormalEnd),
            ("DecodeFailed", DataStreamErrorReason::DecodeFailed),
            ("LengthExceeded", DataStreamErrorReason::LengthExceeded),
            ("Incomplete", DataStreamErrorReason::Incomplete),
            (
                "HandlerAlreadyRegistered",
                DataStreamErrorReason::HandlerAlreadyRegistered,
            ),
            (
                "EncryptionTypeMismatch",
                DataStreamErrorReason::EncryptionTypeMismatch,
            ),
            (
                "DestinationUnavailable",
                DataStreamErrorReason::DestinationUnavailable,
            ),
            ("RemoteCancelled", DataStreamErrorReason::RemoteCancelled),
            (
                "FlowControlTimeout",
                DataStreamErrorReason::FlowControlTimeout,
            ),
            (
                "FlowControlProtocol",
                DataStreamErrorReason::FlowControlProtocol,
            ),
        ] {
            let error = AdapterError {
                name: "DataStreamError".into(),
                message: "stream failed".into(),
                status: None,
                reason_name: Some(reason_name.into()),
            };
            assert_eq!(error.data_stream_reason(), Some(expected));
        }
    }

    #[test]
    fn rpc_options_keep_native_payloads_and_normalize_short_timeouts() {
        let text = RpcOptions::new(
            "student",
            "lesson.lookup",
            RpcPayload::Text("chapter 2".into()),
        );
        assert_eq!(text.destination_identity(), "student");
        assert_eq!(text.method(), "lesson.lookup");
        assert_eq!(text.payload(), &RpcPayload::Text("chapter 2".into()));
        assert_eq!(text.response_timeout(), DEFAULT_RPC_RESPONSE_TIMEOUT);

        let bytes = RpcOptions::new(
            "teacher",
            "worksheet.check",
            RpcPayload::Bytes(vec![0, 255, 128, 65]),
        )
        .with_response_timeout(std::time::Duration::from_millis(50));
        assert_eq!(bytes.payload(), &RpcPayload::Bytes(vec![0, 255, 128, 65]));
        assert_eq!(bytes.response_timeout(), MIN_RPC_RESPONSE_TIMEOUT);
    }

    #[test]
    fn rpc_handler_capacity_rejects_excess_work_and_releases_permits() {
        let capacity = RpcHandlerCapacity::new(2);
        let first = capacity.try_acquire().unwrap();
        let second = capacity.try_acquire().unwrap();
        assert!(capacity.try_acquire().is_none());

        drop(first);
        assert!(capacity.try_acquire().is_some());
        drop(second);

        let normalized_zero = RpcHandlerCapacity::new(0);
        let only = normalized_zero.try_acquire().unwrap();
        assert!(normalized_zero.try_acquire().is_none());
        drop(only);
        assert!(normalized_zero.try_acquire().is_some());
    }

    #[test]
    fn rpc_failures_have_stable_actionable_categories() {
        for (code, kind) in [
            (1400, RpcFailureKind::UnsupportedMethod),
            (1401, RpcFailureKind::RemoteParticipantNotFound),
            (1402, RpcFailureKind::RequestPayloadTooLarge),
            (1403, RpcFailureKind::UnsupportedServer),
            (1404, RpcFailureKind::UnsupportedVersion),
            (1500, RpcFailureKind::ApplicationError),
            (1501, RpcFailureKind::ConnectionTimeout),
            (1502, RpcFailureKind::ResponseTimeout),
            (1503, RpcFailureKind::RemoteParticipantDisconnected),
            (1504, RpcFailureKind::ResponsePayloadTooLarge),
            (1505, RpcFailureKind::SendFailed),
            (2001, RpcFailureKind::ApplicationDefined),
            (1000, RpcFailureKind::ApplicationDefined),
        ] {
            let failure = RpcFailure {
                code: Some(code),
                message: "failed".into(),
                data: Some("details".into()),
                cause: None,
            };
            assert_eq!(failure.kind(), kind);
            assert_eq!(failure.code(), Some(code));
            assert_eq!(failure.message(), "failed");
            assert_eq!(failure.data(), Some("details"));
        }

        let failure = RpcFailure {
            code: None,
            message: "transport failed".into(),
            data: None,
            cause: None,
        };
        assert_eq!(failure.kind(), RpcFailureKind::Unknown);
    }

    #[test]
    fn rpc_application_errors_cannot_impersonate_livekit_failures() {
        let reserved = RpcApplicationError::new(1502, "not a transport timeout");
        assert_eq!(reserved.code(), 1500);
        assert_eq!(reserved.message(), "not a transport timeout");

        let application_defined = RpcApplicationError::new(1000, "application failure");
        assert_eq!(application_defined.code(), 1000);
    }

    #[test]
    fn rpc_operations_reject_a_terminal_room_session() {
        let session = RoomSession::test_session(test_snapshot(1));
        let participant = session.local_participant();
        let _leave = session.leave();

        assert!(matches!(
            block_on(participant.invoke_rpc(RpcOptions::new(
                "student",
                "lesson.lookup",
                RpcPayload::Text(String::new()),
            ))),
            Err(Error::InvalidState {
                operation: "invoke participant RPC",
                reason: "Room Session is terminal",
            })
        ));
        assert!(matches!(
            session.register_rpc_method("lesson.lookup", |_| async {
                Ok(RpcPayload::Text("ready".into()))
            }),
            Err(Error::InvalidState {
                operation: "register participant RPC method",
                reason: "Room Session is terminal",
            })
        ));
    }

    #[test]
    fn application_state_events_share_one_installed_snapshot_revision() {
        let mut initial = test_snapshot(1);
        initial.remote_participants.insert(
            "observer".into(),
            RemoteParticipantSnapshot {
                identity: "observer".into(),
                sid: "PA_observer".into(),
                generation: 1,
                connection_quality: ConnectionQuality::Good,
                metadata: None,
                attributes: BTreeMap::new(),
                track_publications: BTreeMap::new(),
                data_tracks: BTreeMap::new(),
            },
        );
        let session = RoomSession::test_session(initial);
        let mut events = session.subscribe_events(3);
        let mut updated = session.snapshot().as_ref().clone();
        updated.revision = 2;
        updated.room_metadata = Some("chapter-2".into());
        updated.local_participant.metadata = Some("ready".into());
        updated
            .local_participant
            .attributes
            .insert("status".into(), "present".into());
        let observer = updated.remote_participants.get_mut("observer").unwrap();
        observer.metadata = Some("watching".into());
        observer.attributes.insert("topic".into(), "waves".into());

        session.inner.install_normalized_update(
            updated,
            [
                RoomEventKind::RoomMetadataChanged,
                RoomEventKind::LocalParticipantChanged,
                RoomEventKind::RemoteParticipantChanged {
                    identity: "observer".into(),
                },
            ],
        );

        for _ in 0..3 {
            let event = block_on(events.next()).unwrap();
            assert_eq!(event.revision(), 2);
            assert_eq!(session.snapshot().revision(), event.revision());
        }
        let snapshot = session.snapshot();
        assert_eq!(snapshot.room_metadata(), Some("chapter-2"));
        assert_eq!(snapshot.local_participant().metadata(), Some("ready"));
        assert_eq!(
            snapshot.local_participant().attributes().get("status"),
            Some(&"present".into())
        );
        let observer = snapshot.remote_participants().get("observer").unwrap();
        assert_eq!(observer.metadata(), Some("watching"));
        assert_eq!(observer.attributes().get("topic"), Some(&"waves".into()));
    }

    #[test]
    fn lagging_room_event_consumer_recovers_from_the_current_snapshot() {
        let session = RoomSession::test_session(test_snapshot(1));
        let mut slow_events = session.subscribe_events(1);
        let mut fast_events = session.subscribe_events(2);

        session.install_test_update(
            test_snapshot(2),
            RoomEventKind::ConnectionStateChanged {
                state: RoomConnectionState::Reconnecting,
            },
        );
        let mut current = test_snapshot(3);
        current.room_metadata = Some("current".into());
        session.install_test_update(current, RoomEventKind::RoomMetadataChanged);

        let lagged = block_on(slow_events.next()).unwrap();
        assert_eq!(lagged.kind(), &RoomEventKind::Lagged);
        assert_eq!(lagged.revision(), 3);
        assert_eq!(session.snapshot().revision(), lagged.revision());
        assert_eq!(session.snapshot().room_metadata(), Some("current"));

        assert_eq!(block_on(fast_events.next()).unwrap().revision(), 2);
        assert_eq!(block_on(fast_events.next()).unwrap().revision(), 3);

        let mut recovered = test_snapshot(4);
        recovered.room_metadata = Some("recovered".into());
        session.install_test_update(recovered, RoomEventKind::RoomMetadataChanged);
        let resumed = block_on(slow_events.next()).unwrap();
        assert_eq!(resumed.kind(), &RoomEventKind::RoomMetadataChanged);
        assert_eq!(resumed.revision(), 4);
        assert_eq!(session.snapshot().room_metadata(), Some("recovered"));
    }

    #[test]
    fn terminal_disconnection_closes_every_session_clone() {
        let cleanup_count = Rc::new(std::cell::Cell::new(0));
        let session =
            RoomSession::test_session_with_cleanup(test_snapshot(1), Rc::clone(&cleanup_count));
        let clone = session.clone();
        let mut events = session.subscribe_events(1);
        let mut disconnected = test_snapshot(2);
        disconnected.connection = RoomConnectionState::Disconnected;

        session.install_test_update(
            disconnected,
            RoomEventKind::ConnectionStateChanged {
                state: RoomConnectionState::Disconnected,
            },
        );

        assert!(session.is_terminal());
        assert!(clone.is_terminal());
        assert_eq!(cleanup_count.get(), 1);
        assert_eq!(block_on(events.next()).unwrap().revision(), 2);
        assert!(block_on(events.next()).is_none());
    }

    #[test]
    fn joined_snapshot_contains_authoritative_local_participant_state() {
        let snapshot: AdapterRoomSnapshot = serde_json::from_str(
            r#"{
                "revision": 1,
                "connection": "connected",
                "audio_playback": "allowed",
                "active_speakers": [],
                "room_sid": "RM_room",
                "room_name": "physics",
                "room_metadata": "room-meta",
                "local_participant": {
                    "identity": "teacher",
                    "sid": "PA_teacher",
                    "connection_quality": "excellent",
                    "metadata": "participant-meta",
                    "attributes": {"status": "ready"},
                    "grants": {
                        "can_subscribe": true,
                        "can_publish": true,
                        "can_publish_data": true,
                        "can_publish_sources": ["camera", "microphone"],
                        "can_update_metadata": true,
                        "hidden": false,
                        "recorder": false,
                        "agent": false,
                        "can_subscribe_metrics": false,
                        "can_manage_agent_session": false
                    },
                    "track_publications": {}
                },
                "remote_participants": {}
            }"#,
        )
        .unwrap();
        let snapshot: RoomSnapshot = snapshot.into();

        assert_eq!(snapshot.revision(), 1);
        assert_eq!(snapshot.room_sid(), "RM_room");
        assert_eq!(snapshot.room_name(), "physics");
        assert_eq!(snapshot.room_metadata(), Some("room-meta"));
        assert_eq!(snapshot.local_participant().identity(), "teacher");
        assert_eq!(snapshot.local_participant().sid(), "PA_teacher");
        assert_eq!(
            snapshot.local_participant().connection_quality(),
            ConnectionQuality::Excellent
        );
        assert_eq!(
            snapshot.local_participant().metadata(),
            Some("participant-meta")
        );
        assert_eq!(
            snapshot.local_participant().attributes().get("status"),
            Some(&"ready".to_string())
        );
        assert!(snapshot.local_participant().grants().can_publish());
        assert_eq!(
            snapshot.local_participant().grants().can_publish_sources(),
            &[TrackSource::Camera, TrackSource::Microphone]
        );
        assert!(snapshot.local_participant().track_publications().is_empty());
    }

    #[test]
    fn remote_participants_and_publications_keep_distinct_room_identity_and_availability() {
        let snapshot: AdapterRoomSnapshot = serde_json::from_str(
            r#"{
                "revision": 7,
                "connection": "connected",
                "audio_playback": "allowed",
                "active_speakers": [],
                "room_sid": "RM_room",
                "room_name": "physics",
                "room_metadata": null,
                "local_participant": {
                    "identity": "teacher",
                    "sid": "PA_teacher",
                    "connection_quality": "good",
                    "metadata": null,
                    "attributes": {},
                    "grants": {
                        "can_subscribe": true,
                        "can_publish": true,
                        "can_publish_data": true,
                        "can_publish_sources": [],
                        "can_update_metadata": false,
                        "hidden": false,
                        "recorder": false,
                        "agent": false,
                        "can_subscribe_metrics": false,
                        "can_manage_agent_session": false
                    },
                    "track_publications": {}
                },
                "remote_participants": {
                    "student": {
                        "identity": "student",
                        "sid": "PA_student_current",
                        "generation": 11,
                        "connection_quality": "poor",
                        "metadata": "ready",
                        "attributes": {"group": "blue"},
                        "track_publications": {
                            "TR_audio": {
                                "sid": "TR_audio",
                                "generation": 12,
                                "name": "microphone",
                                "source": "microphone",
                                "kind": "audio",
                                "muted": true,
                                "subscribed": true,
                                "media_available": false,
                                "media_generation": null
                            },
                            "TR_video": {
                                "sid": "TR_video",
                                "generation": 13,
                                "name": "camera",
                                "source": "camera",
                                "kind": "video",
                                "muted": false,
                                "subscribed": false,
                                "media_available": false,
                                "media_generation": null
                            }
                        },
                        "data_tracks": {
                            "DT_data": {
                                "sid": "DT_data",
                                "name": "reactions",
                                "uses_e2ee": true
                            }
                        }
                    }
                }
            }"#,
        )
        .unwrap();
        let snapshot: RoomSnapshot = snapshot.into();

        let student = snapshot.remote_participants().get("student").unwrap();
        assert_eq!(student.identity(), "student");
        assert_eq!(student.sid(), "PA_student_current");
        assert_eq!(student.generation(), 11);
        assert_eq!(student.connection_quality(), ConnectionQuality::Poor);
        assert_eq!(student.metadata(), Some("ready"));
        assert_eq!(
            student.attributes().get("group").map(String::as_str),
            Some("blue")
        );

        let publications = student.track_publications();
        assert_eq!(publications.len(), 2);
        let audio = publications.get("TR_audio").unwrap();
        assert_eq!(audio.sid(), "TR_audio");
        assert_eq!(audio.generation(), 12);
        assert_eq!(audio.kind(), TrackKind::Audio);
        assert!(audio.muted());
        assert!(audio.subscribed());
        assert!(!audio.media_available());
        assert_eq!(audio.media_generation(), None);
        let video = publications.get("TR_video").unwrap();
        assert_eq!(video.kind(), TrackKind::Video);
        assert!(!video.subscribed());
        assert!(!video.media_available());
        assert!(publications.get("DT_data").is_none());
        let data_track = student.data_tracks().get("DT_data").unwrap();
        assert_eq!(data_track.sid(), "DT_data");
        assert_eq!(data_track.name(), "reactions");
        assert!(data_track.uses_e2ee());
    }

    #[test]
    fn remote_track_publication_handle_becomes_terminal_without_hiding_snapshot_state() {
        let mut snapshot = test_snapshot(1);
        snapshot.remote_participants.insert(
            "student".into(),
            RemoteParticipantSnapshot {
                identity: "student".into(),
                sid: "PA_student".into(),
                generation: 1,
                connection_quality: ConnectionQuality::Good,
                metadata: None,
                attributes: BTreeMap::new(),
                track_publications: BTreeMap::from([(
                    "TR_camera".into(),
                    RemoteTrackPublicationSnapshot {
                        sid: "TR_camera".into(),
                        generation: 2,
                        name: "camera".into(),
                        source: TrackSource::Camera,
                        kind: TrackKind::Video,
                        muted: false,
                        subscribed: true,
                        media_available: true,
                        media_generation: Some(3),
                    },
                )]),
                data_tracks: BTreeMap::new(),
            },
        );
        let session = RoomSession::test_session(snapshot);

        let publication = session
            .remote_track_publication("student", "TR_camera")
            .unwrap();

        assert_eq!(publication.participant_identity(), "student");
        assert_eq!(publication.sid(), "TR_camera");
        assert_eq!(publication.kind(), TrackKind::Video);
        assert!(!publication.is_terminal());
        assert!(session
            .snapshot()
            .remote_participants()
            .get("student")
            .unwrap()
            .track_publications()
            .contains_key("TR_camera"));

        session.install_test_update(
            test_snapshot(2),
            RoomEventKind::RemoteParticipantLeft {
                identity: "student".into(),
            },
        );

        assert!(publication.is_terminal());
    }

    #[test]
    fn replacement_ids_and_instances_make_old_remote_handles_terminal_without_retargeting() {
        let session = RoomSession::test_session(test_snapshot(1));
        let old_media = RemoteVideoTrack::test_track(&session);
        let old_attachment = RemoteVideoTrackAttachment::test_attachment(&old_media);
        let old_participant = session.remote_participant("student").unwrap();
        let old_publication = old_participant.track_publication("TR_camera").unwrap();
        let mut events = session.subscribe_events(2);

        let mut replacement = session.snapshot().as_ref().clone();
        replacement.revision += 1;
        let participant = replacement.remote_participants.get_mut("student").unwrap();
        participant.sid = "PA_student_replacement".into();
        let publication = participant.track_publications.get_mut("TR_camera").unwrap();
        publication.generation = 5;
        publication.media_generation = Some(6);
        let replacement_revision = replacement.revision;
        session.inner.install_normalized_update(
            replacement,
            [
                RoomEventKind::RemoteParticipantJoined {
                    identity: "student".into(),
                },
                RoomEventKind::RemoteTrackPublicationsChanged {
                    participant_identity: "student".into(),
                },
            ],
        );

        assert!(old_participant.is_terminal());
        assert!(old_publication.is_terminal());
        assert!(old_media.is_terminal());
        assert!(!old_attachment.is_active());
        assert!(matches!(
            block_on(old_publication.set_subscribed(false)),
            Err(Error::InvalidState {
                operation: "change Remote Track Publication subscription",
                reason: "Track Publication is terminal",
            })
        ));

        let current_participant = session.remote_participant("student").unwrap();
        let current_publication = current_participant.track_publication("TR_camera").unwrap();
        assert_eq!(current_participant.sid(), "PA_student_replacement");
        assert!(!current_participant.is_terminal());
        assert!(!current_publication.is_terminal());

        for _ in 0..2 {
            let event = block_on(events.next()).unwrap();
            assert_eq!(event.revision(), replacement_revision);
            assert_eq!(session.snapshot().revision(), event.revision());
        }

        block_on(session.leave()).unwrap();
        let terminal_revision = session.snapshot().revision();
        let mut late_reconnect = session.snapshot().as_ref().clone();
        late_reconnect.revision += 1;
        late_reconnect.connection = RoomConnectionState::Connected;
        session.inner.install_normalized_update(
            late_reconnect,
            [RoomEventKind::ConnectionStateChanged {
                state: RoomConnectionState::Connected,
            }],
        );
        assert!(session.is_terminal());
        assert_eq!(session.snapshot().revision(), terminal_revision);
    }

    #[test]
    fn terminal_remote_media_track_terminates_existing_attachment() {
        let session = RoomSession::test_session(test_snapshot(1));
        let track = RemoteVideoTrack::test_track(&session);
        let attachment = RemoteVideoTrackAttachment::test_attachment(&track);

        assert!(!track.is_terminal());
        assert!(attachment.is_active());

        track.inner.mark_terminal();

        assert!(track.is_terminal());
        assert!(!attachment.is_active());
    }

    #[test]
    fn remote_audio_analyser_becomes_terminal_with_its_media_generation() {
        let session = RoomSession::test_session(test_snapshot(1));
        let track = RemoteAudioTrack::test_track(&session);
        let analyser = track.create_analyser().unwrap();

        assert!(analyser.is_active());
        assert_eq!(analyser.read().unwrap().waveform(), &[0.0]);

        let mut replacement = session.snapshot().as_ref().clone();
        replacement.revision += 1;
        replacement
            .remote_participants
            .get_mut("student")
            .unwrap()
            .track_publications
            .get_mut("TR_microphone")
            .unwrap()
            .media_generation = Some(4);
        session
            .inner
            .install_normalized_update(replacement, std::iter::empty());

        assert!(track.is_terminal());
        assert!(!analyser.is_active());
        assert!(matches!(
            analyser.read(),
            Err(Error::InvalidState {
                operation: "read Audio Analyser",
                reason: "Audio Analyser or source track is terminal",
            })
        ));
    }

    #[test]
    fn closed_and_dropped_remote_attachments_release_lifecycle_entries() {
        let session = RoomSession::test_session(test_snapshot(1));
        let track = RemoteVideoTrack::test_track(&session);
        let closed = RemoteVideoTrackAttachment::test_attachment(&track);

        assert_eq!(track.inner.attachments.borrow().len(), 1);
        closed.close().unwrap();
        assert!(track.inner.attachments.borrow().is_empty());

        let dropped = RemoteVideoTrackAttachment::test_attachment(&track);
        assert_eq!(track.inner.attachments.borrow().len(), 1);
        drop(dropped);
        assert!(track.inner.attachments.borrow().is_empty());
    }

    #[test]
    fn closed_and_dropped_local_attachments_release_lifecycle_entries() {
        let video = LocalVideoTrack::test_track(Rc::new(Cell::new(0)));
        let closed_video = VideoTrackAttachment::test_attachment(&video);
        assert_eq!(video.inner.attachments.borrow().len(), 1);
        closed_video.close().unwrap();
        assert!(video.inner.attachments.borrow().is_empty());

        let dropped_video = VideoTrackAttachment::test_attachment(&video);
        assert_eq!(video.inner.attachments.borrow().len(), 1);
        drop(dropped_video);
        assert!(video.inner.attachments.borrow().is_empty());

        let audio = LocalAudioTrack::test_track(Rc::new(Cell::new(0)));
        let closed_audio = AudioTrackAttachment::test_attachment(&audio);
        assert_eq!(audio.inner.attachments.borrow().len(), 1);
        closed_audio.close().unwrap();
        assert!(audio.inner.attachments.borrow().is_empty());

        let dropped_audio = AudioTrackAttachment::test_attachment(&audio);
        assert_eq!(audio.inner.attachments.borrow().len(), 1);
        drop(dropped_audio);
        assert!(audio.inner.attachments.borrow().is_empty());
    }

    #[test]
    fn same_update_installs_remote_snapshot_before_detailed_room_events() {
        let session = RoomSession::test_session(test_snapshot(1));
        let mut events = session.subscribe_events(4);
        let mut snapshot = test_snapshot(2);
        snapshot.remote_participants.insert(
            "student".into(),
            RemoteParticipantSnapshot {
                identity: "student".into(),
                sid: "PA_student".into(),
                generation: 1,
                connection_quality: ConnectionQuality::Good,
                metadata: None,
                attributes: BTreeMap::new(),
                track_publications: BTreeMap::from([(
                    "TR_microphone".into(),
                    RemoteTrackPublicationSnapshot {
                        sid: "TR_microphone".into(),
                        generation: 2,
                        name: "microphone".into(),
                        source: TrackSource::Microphone,
                        kind: TrackKind::Audio,
                        muted: false,
                        subscribed: true,
                        media_available: true,
                        media_generation: Some(3),
                    },
                )]),
                data_tracks: BTreeMap::new(),
            },
        );

        session.inner.install_normalized_update(
            snapshot,
            [
                RoomEventKind::RemoteParticipantJoined {
                    identity: "student".into(),
                },
                RoomEventKind::RemoteTrackPublicationsChanged {
                    participant_identity: "student".into(),
                },
            ],
        );

        let joined = block_on(events.next()).unwrap();
        let publications_changed = block_on(events.next()).unwrap();
        assert_eq!(joined.revision(), 2);
        assert_eq!(publications_changed.revision(), 2);
        assert_eq!(session.snapshot().revision(), 2);
        assert!(session
            .snapshot()
            .remote_participants()
            .get("student")
            .unwrap()
            .track_publications()
            .contains_key("TR_microphone"));
        assert_eq!(
            joined.kind(),
            &RoomEventKind::RemoteParticipantJoined {
                identity: "student".into()
            }
        );
        assert_eq!(
            publications_changed.kind(),
            &RoomEventKind::RemoteTrackPublicationsChanged {
                participant_identity: "student".into()
            }
        );
    }

    #[test]
    fn remote_participant_change_events_retain_logical_identity() {
        let changed: RoomEventKind =
            serde_json::from_str(r#"{"type":"remoteParticipantChanged","identity":"student"}"#)
                .unwrap();
        let left: RoomEventKind =
            serde_json::from_str(r#"{"type":"remoteParticipantLeft","identity":"student"}"#)
                .unwrap();

        assert_eq!(
            changed,
            RoomEventKind::RemoteParticipantChanged {
                identity: "student".into()
            }
        );
        assert_eq!(
            left,
            RoomEventKind::RemoteParticipantLeft {
                identity: "student".into()
            }
        );
    }

    #[test]
    fn leave_from_any_clone_is_terminal_and_cleans_up_once() {
        let cleanup_count = Rc::new(std::cell::Cell::new(0));
        let session =
            RoomSession::test_session_with_cleanup(test_snapshot(1), Rc::clone(&cleanup_count));
        let clone = session.clone();

        let leave = clone.leave();
        assert!(session.is_terminal());
        assert!(clone.is_terminal());
        assert_eq!(cleanup_count.get(), 1);
        block_on(leave).unwrap();
        block_on(session.leave()).unwrap();

        assert_eq!(
            session.snapshot().connection(),
            RoomConnectionState::Disconnected
        );
        assert_eq!(cleanup_count.get(), 1);

        let mut events = session.subscribe_events(1);
        assert!(block_on(events.next()).is_none());
    }

    #[test]
    fn dropping_leave_future_does_not_hide_terminal_completion() {
        let cleanup_count = Rc::new(std::cell::Cell::new(0));
        let session =
            RoomSession::test_session_with_cleanup(test_snapshot(1), Rc::clone(&cleanup_count));

        drop(session.leave());

        assert!(session.is_terminal());
        assert_eq!(
            session.snapshot().connection(),
            RoomConnectionState::Disconnected
        );
        assert_eq!(cleanup_count.get(), 1);
    }

    #[test]
    fn final_session_drop_schedules_best_effort_cleanup() {
        let cleanup_count = Rc::new(std::cell::Cell::new(0));
        let session =
            RoomSession::test_session_with_cleanup(test_snapshot(1), Rc::clone(&cleanup_count));
        let clone = session.clone();

        drop(session);
        assert_eq!(cleanup_count.get(), 0);
        drop(clone);

        assert_eq!(cleanup_count.get(), 1);
    }

    #[test]
    fn local_video_track_clones_share_terminal_stop_and_final_owner_cleanup() {
        let stop_count = Rc::new(Cell::new(0));
        let track = LocalVideoTrack::test_track(Rc::clone(&stop_count));
        let clone = track.clone();

        let stop = clone.stop();

        assert!(track.is_stopped());
        assert!(clone.is_stopped());
        assert_eq!(stop_count.get(), 1);
        block_on(stop).unwrap();
        block_on(track.stop()).unwrap();
        drop(track);
        assert_eq!(stop_count.get(), 1);
        drop(clone);
        assert_eq!(stop_count.get(), 1);

        let fallback_count = Rc::new(Cell::new(0));
        let track = LocalVideoTrack::test_track(Rc::clone(&fallback_count));
        let clone = track.clone();
        drop(track);
        assert_eq!(fallback_count.get(), 0);
        drop(clone);
        assert_eq!(fallback_count.get(), 1);
    }

    #[test]
    fn stopped_local_video_track_rejects_restart_and_device_switch() {
        let track = LocalVideoTrack::test_track(Rc::new(Cell::new(0)));
        block_on(track.stop()).unwrap();

        assert!(matches!(
            block_on(track.restart()),
            Err(Error::InvalidState {
                operation: "restart Local Video Track",
                reason: "capture has stopped",
            })
        ));
        assert!(matches!(
            block_on(track.switch_device("camera-rear")),
            Err(Error::InvalidState {
                operation: "switch Local Video Track device",
                reason: "capture has stopped",
            })
        ));
    }

    #[test]
    fn local_video_track_has_at_most_one_room_session_publication() {
        let track = LocalVideoTrack::test_track(Rc::new(Cell::new(0)));
        let first_session = RoomSession::test_session(test_snapshot(1));
        let second_session = RoomSession::test_session(test_snapshot(1));

        let first =
            block_on(track.publish(&first_session, VideoPublishOptions::default())).unwrap();
        let second = block_on(track.publish(&second_session, VideoPublishOptions::default()));

        assert!(!first.is_terminal());
        assert!(matches!(
            second,
            Err(Error::InvalidState {
                operation: "publish Local Video Track",
                reason: "track is already associated with a Room Session",
            })
        ));

        block_on(first_session.leave()).unwrap();
        assert!(first.is_terminal());
        assert!(block_on(track.publish(&second_session, VideoPublishOptions::default())).is_ok());
    }

    #[test]
    fn local_audio_track_clones_share_terminal_stop_and_final_owner_cleanup() {
        let stop_count = Rc::new(Cell::new(0));
        let track = LocalAudioTrack::test_track(Rc::clone(&stop_count));
        let clone = track.clone();

        let stop = clone.stop();

        assert!(track.is_stopped());
        assert!(clone.is_stopped());
        assert_eq!(stop_count.get(), 1);
        block_on(stop).unwrap();
        block_on(track.stop()).unwrap();
        drop(track);
        assert_eq!(stop_count.get(), 1);
        drop(clone);
        assert_eq!(stop_count.get(), 1);

        let fallback_count = Rc::new(Cell::new(0));
        let track = LocalAudioTrack::test_track(Rc::clone(&fallback_count));
        let clone = track.clone();
        drop(track);
        assert_eq!(fallback_count.get(), 0);
        drop(clone);
        assert_eq!(fallback_count.get(), 1);
    }

    #[test]
    fn audio_analysis_frames_own_waveform_and_report_rms_level() {
        let analysis = AudioAnalysis::from_waveform(vec![1.0, -1.0, 0.0, 0.0]);

        assert_eq!(analysis.waveform(), &[1.0, -1.0, 0.0, 0.0]);
        assert!((analysis.level() - std::f32::consts::FRAC_1_SQRT_2).abs() < f32::EPSILON);
        assert_eq!(analysis.into_waveform(), vec![1.0, -1.0, 0.0, 0.0]);
    }

    #[test]
    fn local_audio_analysers_share_close_and_end_with_their_source() {
        let track = LocalAudioTrack::test_track(Rc::new(Cell::new(0)));
        let analyser = track.create_analyser().unwrap();
        let clone = analyser.clone();

        assert!(analyser.is_active());
        block_on(clone.close()).unwrap();
        assert!(!analyser.is_active());
        assert!(matches!(
            analyser.read(),
            Err(Error::InvalidState {
                operation: "read Audio Analyser",
                reason: "Audio Analyser or source track is terminal",
            })
        ));

        let analyser = track.create_analyser().unwrap();
        block_on(track.stop()).unwrap();

        assert!(!analyser.is_active());
    }

    #[test]
    fn stopped_local_audio_track_rejects_restart_and_device_switch() {
        let track = LocalAudioTrack::test_track(Rc::new(Cell::new(0)));
        block_on(track.stop()).unwrap();

        assert!(matches!(
            block_on(track.restart()),
            Err(Error::InvalidState {
                operation: "restart Local Audio Track",
                reason: "capture has stopped",
            })
        ));
        assert!(matches!(
            block_on(track.switch_device("microphone-usb")),
            Err(Error::InvalidState {
                operation: "switch Local Audio Track device",
                reason: "capture has stopped",
            })
        ));
    }

    #[test]
    fn local_audio_track_has_at_most_one_room_session_publication() {
        let track = LocalAudioTrack::test_track(Rc::new(Cell::new(0)));
        let first_session = RoomSession::test_session(test_snapshot(1));
        let second_session = RoomSession::test_session(test_snapshot(1));

        let first =
            block_on(track.publish(&first_session, AudioPublishOptions::default())).unwrap();
        let second = block_on(track.publish(&second_session, AudioPublishOptions::default()));

        assert!(!first.is_terminal());
        assert!(matches!(
            second,
            Err(Error::InvalidState {
                operation: "publish Local Audio Track",
                reason: "track is already associated with a Room Session",
            })
        ));

        block_on(first_session.leave()).unwrap();
        assert!(first.is_terminal());
        assert!(block_on(track.publish(&second_session, AudioPublishOptions::default())).is_ok());
    }

    #[test]
    fn screen_tracks_keep_authoritative_sources_and_reject_device_controls() {
        let video = LocalVideoTrack::test_track_with_source(
            Rc::new(Cell::new(0)),
            TrackSource::ScreenShare,
        );
        let audio = LocalAudioTrack::test_track_with_source(
            Rc::new(Cell::new(0)),
            AudioTrackSource::ScreenShareAudio,
        );
        let session = RoomSession::test_session(test_snapshot(1));

        assert_eq!(video.source(), TrackSource::ScreenShare);
        assert_eq!(audio.source(), AudioTrackSource::ScreenShareAudio);
        assert!(matches!(
            block_on(video.restart()),
            Err(Error::InvalidState {
                reason: "display capture cannot restart or switch camera devices",
                ..
            })
        ));
        assert!(matches!(
            block_on(audio.switch_device("microphone")),
            Err(Error::InvalidState {
                reason: "system audio cannot restart or switch microphone devices",
                ..
            })
        ));
        assert!(matches!(
            block_on(video.publish(&session, VideoPublishOptions::default())),
            Err(Error::InvalidState {
                reason: "publication source does not match captured media",
                ..
            })
        ));
        assert!(block_on(video.publish(
            &session,
            VideoPublishOptions::default().with_source(TrackSource::ScreenShare),
        ))
        .is_ok());
    }

    #[test]
    fn browser_ended_screen_tracks_are_observable_and_terminal() {
        let video = LocalVideoTrack::test_track_with_source(
            Rc::new(Cell::new(0)),
            TrackSource::ScreenShare,
        );
        let audio = LocalAudioTrack::test_track_with_source(
            Rc::new(Cell::new(0)),
            AudioTrackSource::ScreenShareAudio,
        );
        let video_ended = video.ended();
        let audio_ended = audio.ended();

        video.inner.mark_browser_ended();
        audio.inner.mark_browser_ended();

        assert!(video.is_stopped());
        assert!(audio.is_stopped());
        assert_eq!(
            video.end_reason(),
            Some(LocalMediaTrackEndReason::BrowserEnded)
        );
        assert_eq!(
            audio.end_reason(),
            Some(LocalMediaTrackEndReason::BrowserEnded)
        );
        assert_eq!(
            block_on(video_ended),
            LocalMediaTrackEndReason::BrowserEnded
        );
        assert_eq!(
            block_on(audio_ended),
            LocalMediaTrackEndReason::BrowserEnded
        );
    }

    #[test]
    fn abandoned_local_media_track_ended_future_removes_its_waker() {
        let video = LocalVideoTrack::test_track_with_source(
            Rc::new(Cell::new(0)),
            TrackSource::ScreenShare,
        );
        let mut ended = Box::pin(video.ended());
        let waker = futures_util::task::noop_waker();
        let mut context = Context::from_waker(&waker);

        assert!(ended.as_mut().poll(&mut context).is_pending());
        assert_eq!(video.inner.end_state.waiters.borrow().len(), 1);
        drop(ended);
        assert!(video.inner.end_state.waiters.borrow().is_empty());
    }

    #[test]
    fn stopping_screen_capture_starts_cleanup_for_every_returned_track() {
        let video_stops = Rc::new(Cell::new(0));
        let audio_stops = Rc::new(Cell::new(0));
        let capture = ScreenCapture {
            display_video: LocalVideoTrack::test_track_with_source(
                Rc::clone(&video_stops),
                TrackSource::ScreenShare,
            ),
            system_audio: Some(LocalAudioTrack::test_track_with_source(
                Rc::clone(&audio_stops),
                AudioTrackSource::ScreenShareAudio,
            )),
        };

        let stop = capture.stop();
        assert_eq!(video_stops.get(), 1);
        assert_eq!(audio_stops.get(), 1);
        block_on(stop).unwrap();
    }

    #[test]
    fn audio_publish_options_serialize_the_typed_opus_policy() {
        let options = AudioPublishOptions::default()
            .with_source(AudioTrackSource::ScreenShareAudio)
            .with_name("system-audio")
            .with_codec(AudioCodec::Opus)
            .with_dtx(true)
            .with_red(false);

        assert_eq!(
            serde_json::to_value(options).unwrap(),
            serde_json::json!({
                "source": "screenShareAudio",
                "name": "system-audio",
                "audioCodec": "opus",
                "dtx": true,
                "red": false,
            })
        );
    }
}
