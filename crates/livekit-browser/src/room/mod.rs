#[cfg(all(feature = "raw-js", target_arch = "wasm32"))]
use crate::error::raw_js_handle;
#[cfg(target_arch = "wasm32")]
use crate::error::{
    classify_chat_message_send_error, classify_data_packet_send_error,
    classify_data_stream_open_error, classify_participant_update_error,
    classify_subscription_permission_error, is_permission_error, participant_update_operation,
};
#[cfg(target_arch = "wasm32")]
use crate::media::{complete_browser_capture, BrowserCaptureFuture, BrowserCaptureState};
use crate::*;

mod data_stream;
mod events;
mod messaging;
mod remote;
mod rpc;
mod snapshot;

#[cfg(target_arch = "wasm32")]
use data_stream::{
    begin_byte_stream_open, begin_text_stream_open, OutgoingStreamInner, OutgoingStreamLifecycle,
    StreamHandlerRegistrationInner,
};
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use data_stream::{
    AdapterByteStreamRead, AdapterOutgoingByteStream, AdapterOutgoingTextStream,
    AdapterTextStreamRead,
};
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) use data_stream::{AdapterIncomingByteStream, AdapterIncomingTextStream};
pub use data_stream::{
    ByteStreamHandlerRegistration, ByteStreamInfo, ByteStreamOptions, DataStreamErrorReason,
    IncomingByteStream, IncomingTextStream, OutgoingByteStream, OutgoingTextStream,
    TextStreamHandlerRegistration, TextStreamInfo, TextStreamOptions,
    DATA_STREAM_RECEIVE_WINDOW_CHUNKS, MAX_ACTIVE_INCOMING_DATA_STREAMS,
    MAX_DATA_STREAM_CHUNK_BYTES,
};
use events::EventQueue;
pub use events::{RoomEvent, RoomEventKind, RoomEventStream};
use messaging::{
    close_bounded_queues, ChatMessageQueue, ChatMessageSubscription, DataPacketQueue,
    DataPacketSubscription, TranscriptionSegmentQueue, TranscriptionSegmentSubscription,
};
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) use messaging::{AdapterChatMessage, AdapterTranscriptionSegment};
pub use messaging::{
    ChatMessage, ChatMessageStream, ChatMessageStreamItem, DataPacket, DataPacketDelivery,
    DataPacketStream, DataPacketStreamItem, TranscriptionSegment, TranscriptionSegmentStream,
    TranscriptionSegmentStreamItem,
};
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use remote::AdapterRemoteMediaTrack;
pub(crate) use remote::RemoteMediaTrackInner;
use remote::RemoteParticipantInner;
pub use remote::{
    RemoteAudioTrack, RemoteAudioTrackAttachment, RemoteMediaTrack, RemoteParticipant,
    RemoteTrackPublication, RemoteVideoTrack, RemoteVideoTrackAttachment,
};
use rpc::encode_rpc_payload;
#[cfg(target_arch = "wasm32")]
use rpc::{
    complete_rpc_handler, decode_rpc_payload, start_rpc_invocation, RpcHandlerTask,
    RpcMethodRegistrationInner,
};
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use rpc::{AdapterRpcFailure, AdapterRpcOptions};
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) use rpc::{AdapterRpcHandlerCompletion, AdapterRpcInvocation, RpcHandlerCapacity};
pub use rpc::{
    RpcApplicationError, RpcFailure, RpcFailureKind, RpcInvocation, RpcMethodRegistration,
    RpcOptions, RpcPayload, DEFAULT_RPC_HANDLER_CAPACITY, DEFAULT_RPC_RESPONSE_TIMEOUT,
    MAX_RPC_PAYLOAD_BYTES, MIN_RPC_RESPONSE_TIMEOUT,
};
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) use snapshot::AdapterRoomSnapshot;
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use snapshot::{
    AdapterRemoteParticipantSnapshot, AdapterRemoteTrackPublicationSnapshot,
};
pub use snapshot::{
    AudioPlaybackState, AudioReceiverStatistics, AudioSenderStatistics, ConnectionQuality,
    LocalParticipantSnapshot, LocalTrackPublicationSnapshot, ParticipantGrants,
    PublicationSubscriptionPermissions, ReceiverStatistics, RemoteDataTrackSnapshot,
    RemoteParticipantSnapshot, RemoteTrackPublicationSnapshot, RoomConnectionState, RoomSnapshot,
    SenderStatistics, TrackKind, TrackSource, VideoReceiverStatistics, VideoSenderStatistics,
};
pub(crate) use snapshot::{RemoteParticipantLocator, RemotePublicationLocator};

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
pub(crate) struct AdapterJoin {
    snapshot: AdapterRoomSnapshot,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
pub(crate) struct AdapterUpdate {
    snapshot: AdapterRoomSnapshot,
    events: Vec<RoomEventKind>,
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn join_room_session(
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
        #[cfg(all(test, not(target_arch = "wasm32")))]
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
    pub(crate) room_id: u32,
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
pub(crate) async fn join_room_session(
    _credentials: ConnectionCredentials,
    _options: RoomOptions,
) -> Result<RoomSession, Error> {
    Err(Error::UnsupportedTarget)
}

/// A cloneable handle to one Local Participant's room participation lifetime.
///
/// This handle is deliberately neither `Send` nor `Sync`.
#[derive(Clone)]
pub struct RoomSession {
    pub(crate) inner: Rc<SessionInner>,
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
        if options.destination_identity().is_empty() || options.method().is_empty() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "invoke participant RPC",
                    reason: "destination Participant Identity and method cannot be empty",
                })
            });
        }
        if encode_rpc_payload(options.payload()).len() > MAX_RPC_PAYLOAD_BYTES {
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
        if _options.topic().is_empty() {
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
        if _options.topic().is_empty() {
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

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn test_session(snapshot: RoomSnapshot) -> Self {
        Self::test_session_with_cleanup(snapshot, Rc::new(Cell::new(0)))
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn test_session_with_cleanup(
        snapshot: RoomSnapshot,
        cleanup_count: Rc<Cell<usize>>,
    ) -> Self {
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

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn install_test_update(&self, snapshot: RoomSnapshot, kind: RoomEventKind) {
        self.inner.install_normalized_update(snapshot, [kind]);
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn install_test_data_packet(&self, packet: DataPacket) {
        self.inner.install_data_packet(packet);
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn install_test_chat_message(&self, message: ChatMessage) {
        self.inner.install_chat_message(message);
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn install_test_transcription_segment(&self, segment: TranscriptionSegment) {
        self.inner.install_transcription_segment(segment);
    }
}

pub(crate) struct SessionInner {
    snapshot: RefCell<Rc<RoomSnapshot>>,
    subscribers: RefCell<Vec<Weak<RefCell<EventQueue>>>>,
    data_packet_subscribers: RefCell<Vec<DataPacketSubscription>>,
    chat_message_subscribers: RefCell<Vec<ChatMessageSubscription>>,
    transcription_segment_subscribers: RefCell<Vec<TranscriptionSegmentSubscription>>,
    #[cfg(target_arch = "wasm32")]
    rpc_handler_tasks: RefCell<Vec<Weak<RpcHandlerTask>>>,
    pub(crate) lifecycle: Cell<SessionLifecycle>,
    cleanup_started: Cell<bool>,
    #[cfg(all(test, not(target_arch = "wasm32")))]
    cleanup_count: Rc<Cell<usize>>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) room_id: u32,
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
    pub(crate) fn install_normalized_update(
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
            #[cfg(all(test, not(target_arch = "wasm32")))]
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
            #[cfg(all(test, not(target_arch = "wasm32")))]
            self.cleanup_count.set(self.cleanup_count.get() + 1);

            #[cfg(target_arch = "wasm32")]
            adapter::drop_room(self.room_id);
        }
        self.close_subscribers();
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum SessionLifecycle {
    Active,
    Leaving,
    Left,
}
