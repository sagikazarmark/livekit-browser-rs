use crate::*;

/// Every failure this crate reports to the application.
///
/// Variants stay distinguishable so callers can react without inspecting message text: a denied
/// permission, an unavailable device, and a device already in use are separate variants rather
/// than one opaque media failure. Failed operations roll back; a rejected mutation leaves the
/// Room Session, Track Publication, and Local Media Track state it touched unchanged.
///
/// This enum is `#[non_exhaustive]`; match with a wildcard arm.
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
                actual
                    .as_deref()
                    .unwrap_or("an SDK with no reported version")
            ),
            Self::UnsupportedBrowser { missing } => write!(formatter, "{missing} is unavailable"),
            Self::AdapterFailure(error) => {
                write!(formatter, "LiveKit browser adapter failed: {error}")
            }
            Self::JoinFailed(error) => write!(formatter, "could not join LiveKit room: {error}"),
            Self::LeaveFailed(error) => {
                write!(formatter, "could not disconnect LiveKit room: {error}")
            }
            Self::CaptureFailed(error) => {
                write!(formatter, "could not capture local media: {error}")
            }
            Self::PermissionDenied(error) => {
                write!(formatter, "operation permission denied: {error}")
            }
            Self::DisplayCaptureNotAllowed(error) => {
                write!(
                    formatter,
                    "display capture was cancelled or not allowed: {error}"
                )
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
                write!(
                    formatter,
                    "could not control LiveKit Track Publication: {error}"
                )
            }
            Self::SubscriptionFailed(error) => {
                write!(formatter, "could not control LiveKit subscription: {error}")
            }
            Self::StatisticsFailed(error) => {
                write!(
                    formatter,
                    "could not query LiveKit media statistics: {error}"
                )
            }
            Self::AudioAnalysisFailed(error) => {
                write!(formatter, "could not analyse browser audio: {error}")
            }
            Self::PublisherPermissionsFailed(error) => {
                write!(
                    formatter,
                    "could not control publisher permissions: {error}"
                )
            }
            Self::ParticipantUpdateFailed(error) => {
                write!(
                    formatter,
                    "could not update Local Participant state: {error}"
                )
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
                write!(
                    formatter,
                    "could not register LiveKit data stream handler: {error}"
                )
            }
            Self::RpcInvocationFailed(error) => {
                write!(formatter, "Participant RPC invocation failed: {error}")
            }
            Self::RpcMethodRegistrationFailed(error) => {
                write!(
                    formatter,
                    "could not register Participant RPC method: {error}"
                )
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
    pub(crate) name: String,
    pub(crate) message: String,
    pub(crate) status: Option<u16>,
    pub(crate) reason_name: Option<String>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) _cause: Option<wasm_bindgen::JsValue>,
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
    pub(crate) fn raw_js_cause(&self) -> Option<&wasm_bindgen::JsValue> {
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
    pub(crate) fn contract(message: String) -> Self {
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
    pub(crate) fn from_cause(cause: wasm_bindgen::JsValue) -> Self {
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
pub(crate) fn classify_capture_error(error: AdapterError) -> Error {
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
pub(crate) fn classify_display_capture_error(error: AdapterError) -> Error {
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
pub(crate) fn classify_publisher_mutation_error(error: AdapterError) -> Error {
    if is_permission_error(&error) {
        Error::PermissionDenied(error)
    } else {
        Error::PublicationFailed(error)
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn classify_subscription_permission_error(error: AdapterError) -> Error {
    if is_permission_error(&error) {
        Error::PermissionDenied(error)
    } else {
        Error::PublisherPermissionsFailed(error)
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn classify_participant_update_error(error: AdapterError) -> Error {
    if is_permission_error(&error) {
        Error::PermissionDenied(error)
    } else {
        Error::ParticipantUpdateFailed(error)
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn deserialize_statistics<T>(
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
pub(crate) fn raw_js_handle(
    result: Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>,
) -> Result<wasm_bindgen::JsValue, Error> {
    result
        .map_err(AdapterError::from_cause)
        .map_err(Error::AdapterFailure)
}

#[cfg(all(feature = "raw-js", target_arch = "wasm32"))]
pub(crate) fn raw_js_statistics(
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
pub(crate) fn raw_js_processor_operation(
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
pub(crate) fn classify_data_packet_send_error(error: AdapterError) -> Error {
    if is_permission_error(&error) {
        Error::PermissionDenied(error)
    } else {
        Error::DataPacketSendFailed(error)
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn classify_chat_message_send_error(error: AdapterError) -> Error {
    if is_permission_error(&error) {
        Error::PermissionDenied(error)
    } else {
        Error::ChatMessageSendFailed(error)
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn classify_data_stream_open_error(error: AdapterError) -> Error {
    if is_permission_error(&error) {
        Error::PermissionDenied(error)
    } else {
        Error::DataStreamOpenFailed(error)
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn participant_update_operation(
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
pub(crate) fn is_permission_error(error: &AdapterError) -> bool {
    error.status() == Some(403)
        || error.reason_name.as_deref() == Some("NOT_ALLOWED")
        || matches!(
            error.name(),
            "NotAllowedError" | "PermissionDeniedError" | "UnauthorizedError"
        )
}
