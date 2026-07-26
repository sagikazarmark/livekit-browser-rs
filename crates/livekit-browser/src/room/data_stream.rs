use super::*;

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
pub(crate) struct AdapterOutgoingTextStream {
    pub(super) id: u32,
    pub(super) info: TextStreamInfo,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
pub(crate) struct AdapterOutgoingByteStream {
    pub(super) id: u32,
    pub(super) info: ByteStreamInfo,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum OutgoingStreamLifecycle {
    Open,
    Writing,
    Completing,
    Terminal,
}

pub(super) struct OutgoingStreamInner {
    pub(super) lifecycle: Cell<OutgoingStreamLifecycle>,
    pub(super) session: Weak<SessionInner>,
    #[cfg(target_arch = "wasm32")]
    pub(super) stream_id: u32,
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
    pub(super) inner: Rc<OutgoingStreamInner>,
    pub(super) info: TextStreamInfo,
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
    pub(super) inner: Rc<OutgoingStreamInner>,
    pub(super) info: ByteStreamInfo,
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
pub(crate) struct AdapterTextStreamRead {
    done: bool,
    value: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
pub(crate) struct AdapterIncomingTextStream {
    pub(super) id: u32,
    pub(super) info: TextStreamInfo,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
pub(crate) struct AdapterIncomingByteStream {
    pub(super) id: u32,
    pub(super) info: ByteStreamInfo,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
pub(crate) struct AdapterByteStreamRead {
    done: bool,
    value: Option<Vec<u8>>,
}

/// A pull-driven incoming text stream.
///
/// Each successful pull acknowledges one chunk to its sender. Dropping or cancelling the stream
/// notifies the sender and drains the private SDK reader through its terminal trailer.
pub struct IncomingTextStream {
    pub(super) info: TextStreamInfo,
    pub(super) session: Weak<SessionInner>,
    pub(super) terminal: bool,
    #[cfg(target_arch = "wasm32")]
    pub(super) stream_id: u32,
    #[cfg(target_arch = "wasm32")]
    pub(super) pending: Option<Pin<Box<wasm_bindgen_futures::JsFuture>>>,
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
    pub(super) info: ByteStreamInfo,
    pub(super) session: Weak<SessionInner>,
    pub(super) terminal: bool,
    #[cfg(target_arch = "wasm32")]
    pub(super) stream_id: u32,
    #[cfg(target_arch = "wasm32")]
    pub(super) pending: Option<Pin<Box<wasm_bindgen_futures::JsFuture>>>,
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

pub(super) struct StreamHandlerRegistrationInner {
    pub(super) active: Rc<Cell<bool>>,
    pub(super) session: Weak<SessionInner>,
    #[cfg(target_arch = "wasm32")]
    pub(super) registration_id: u32,
    #[cfg(target_arch = "wasm32")]
    pub(super) _callback: wasm_bindgen::closure::Closure<dyn FnMut(wasm_bindgen::JsValue)>,
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
    pub(super) inner: Rc<StreamHandlerRegistrationInner>,
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
    pub(super) inner: Rc<StreamHandlerRegistrationInner>,
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
pub(super) fn begin_text_stream_open(
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
pub(super) fn begin_byte_stream_open(
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
