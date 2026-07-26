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

#[cfg(target_arch = "wasm32")]
mod adapter;
mod client;
mod error;
mod media;
mod recording;
mod room;

pub use client::{
    BrowserCapabilities, BrowserCapability, BrowserClient, CameraCaptureOptions, CameraResolution,
    ConnectionCredentials, FacingMode, InitializeOptions, LiveKitLogLevel, LiveKitLogRecord,
    MediaDeviceInfo, MediaDeviceKind, MicrophoneCaptureOptions, RoomOptions, ScreenCaptureOptions,
    LIVEKIT_CLIENT_VERSION,
};
pub use error::{AdapterError, Error};
pub use media::{
    AudioAnalyser, AudioAnalysis, AudioCodec, AudioPublishOptions, AudioTrackAttachment,
    AudioTrackSource, LocalAudioTrack, LocalAudioTrackPublication, LocalMediaTrackEndReason,
    LocalMediaTrackEnded, LocalVideoTrack, LocalVideoTrackPublication, ScreenCapture, VideoCodec,
    VideoEncoding, VideoPublishOptions, VideoTrackAttachment,
};
#[doc(hidden)]
pub use recording::local_clip_recording_support;
pub use room::{
    AudioPlaybackState, AudioReceiverStatistics, AudioSenderStatistics,
    ByteStreamHandlerRegistration, ByteStreamInfo, ByteStreamOptions, ChatMessage,
    ChatMessageStream, ChatMessageStreamItem, ConnectionQuality, DataPacket, DataPacketDelivery,
    DataPacketStream, DataPacketStreamItem, DataStreamErrorReason, IncomingByteStream,
    IncomingTextStream, LocalParticipant, LocalParticipantSnapshot, LocalTrackPublicationSnapshot,
    OutgoingByteStream, OutgoingTextStream, ParticipantGrants, PublicationSubscriptionPermissions,
    ReceiverStatistics, RemoteAudioTrack, RemoteAudioTrackAttachment, RemoteDataTrackSnapshot,
    RemoteMediaTrack, RemoteParticipant, RemoteParticipantSnapshot, RemoteTrackPublication,
    RemoteTrackPublicationSnapshot, RemoteVideoTrack, RemoteVideoTrackAttachment,
    RoomConnectionState, RoomEvent, RoomEventKind, RoomEventStream, RoomSession, RoomSnapshot,
    RpcApplicationError, RpcFailure, RpcFailureKind, RpcInvocation, RpcMethodRegistration,
    RpcOptions, RpcPayload, SenderStatistics, TextStreamHandlerRegistration, TextStreamInfo,
    TextStreamOptions, TrackKind, TrackSource, TranscriptionSegment, TranscriptionSegmentStream,
    TranscriptionSegmentStreamItem, VideoReceiverStatistics, VideoSenderStatistics,
    DATA_STREAM_RECEIVE_WINDOW_CHUNKS, DEFAULT_RPC_HANDLER_CAPACITY, DEFAULT_RPC_RESPONSE_TIMEOUT,
    MAX_ACTIVE_INCOMING_DATA_STREAMS, MAX_DATA_STREAM_CHUNK_BYTES, MAX_RPC_PAYLOAD_BYTES,
    MIN_RPC_RESPONSE_TIMEOUT,
};

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
