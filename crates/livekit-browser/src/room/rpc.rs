use super::*;

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

pub(super) fn encode_rpc_payload(payload: &RpcPayload) -> String {
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
pub(super) fn decode_rpc_payload(payload: String) -> RpcPayload {
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
    pub(super) request_id: String,
    pub(super) caller_identity: String,
    pub(super) payload: RpcPayload,
    pub(super) response_timeout: Duration,
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
    pub(super) code: u32,
    pub(super) message: String,
    pub(super) data: Option<String>,
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
    pub(crate) code: Option<u32>,
    pub(crate) message: String,
    pub(crate) data: Option<String>,
    #[allow(dead_code)]
    pub(crate) cause: Option<Box<AdapterError>>,
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
    pub(crate) fn raw_js_cause(&self) -> Option<&wasm_bindgen::JsValue> {
        self.cause.as_deref().and_then(AdapterError::raw_js_cause)
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
pub(crate) struct AdapterRpcOptions {
    destination_identity: String,
    method: String,
    payload: String,
    response_timeout_ms: u32,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
pub(crate) struct AdapterRpcFailure {
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
pub(crate) struct AdapterRpcInvocation {
    pub(super) id: u32,
    pub(super) request_id: String,
    pub(super) caller_identity: String,
    pub(super) payload: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub(super) response_timeout_ms: u64,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdapterRpcHandlerCompletion {
    pub(super) success: bool,
    pub(super) payload: Option<String>,
    pub(super) code: Option<u32>,
    pub(super) message: Option<String>,
    pub(super) data: Option<String>,
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
pub(super) fn start_rpc_invocation(
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
                        cause: raw_cause.map(Box::new),
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
                cause: Some(Box::new(error)),
            })
        }));
    }
    Box::pin(RpcInvocationFuture { state })
}

#[cfg(target_arch = "wasm32")]
pub(super) struct RpcHandlerTask {
    pub(super) invocation_id: u32,
    pub(super) abort: futures_util::future::AbortHandle,
}

#[cfg(any(target_arch = "wasm32", test))]
pub(crate) struct RpcHandlerCapacity {
    limit: usize,
    in_flight: Rc<Cell<usize>>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl RpcHandlerCapacity {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            in_flight: Rc::new(Cell::new(0)),
        }
    }

    pub(crate) fn try_acquire(&self) -> Option<RpcHandlerPermit> {
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
pub(crate) struct RpcHandlerPermit {
    in_flight: Rc<Cell<usize>>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl Drop for RpcHandlerPermit {
    fn drop(&mut self) {
        self.in_flight.set(self.in_flight.get().saturating_sub(1));
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn complete_rpc_handler(invocation_id: u32, completion: AdapterRpcHandlerCompletion) {
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

pub(super) struct RpcMethodRegistrationInner {
    pub(super) active: Rc<Cell<bool>>,
    pub(super) session: Weak<SessionInner>,
    #[cfg(target_arch = "wasm32")]
    pub(super) registration_id: u32,
    #[cfg(target_arch = "wasm32")]
    pub(super) _callback: wasm_bindgen::closure::Closure<dyn FnMut(wasm_bindgen::JsValue)>,
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
    pub(super) inner: Rc<RpcMethodRegistrationInner>,
}

impl RpcMethodRegistration {
    pub fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    pub fn unregister(&self) {
        self.inner.unregister();
    }
}
