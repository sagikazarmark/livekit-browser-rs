//! Executable acceptance harness for the browser LiveKit crates.

use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    time::{Duration, UNIX_EPOCH},
};

use dioxus::prelude::*;
use futures_util::{
    future::{AbortHandle, Abortable},
    StreamExt,
};
use gloo_timers::future::TimeoutFuture;
use livekit_browser::{
    AudioAnalyser, AudioCodec, AudioPlaybackState, AudioPublishOptions, AudioTrackAttachment,
    AudioTrackSource, BrowserCapabilities, BrowserClient, ByteStreamHandlerRegistration,
    ByteStreamOptions, CameraCaptureOptions, CameraResolution, ChatMessageStreamItem,
    ConnectionCredentials, ConnectionQuality, DataPacketDelivery, DataPacketStreamItem,
    Error as LiveKitError, FacingMode, InitializeOptions, LiveKitLogLevel, LocalAudioTrack,
    LocalAudioTrackPublication, LocalVideoTrack, LocalVideoTrackPublication, MediaDeviceInfo,
    MediaDeviceKind, MicrophoneCaptureOptions, PublicationSubscriptionPermissions,
    RemoteAudioTrack, RemoteAudioTrackAttachment, RemoteMediaTrack, RemoteParticipant,
    RemoteTrackPublication, RemoteVideoTrack, RemoteVideoTrackAttachment, RoomConnectionState,
    RoomEventKind, RoomOptions, RoomSession, RpcApplicationError, RpcFailureKind, RpcInvocation,
    RpcMethodRegistration, RpcOptions, RpcPayload, ScreenCapture, ScreenCaptureOptions,
    TextStreamHandlerRegistration, TextStreamOptions, TrackKind, TrackSource,
    TranscriptionSegmentStreamItem, VideoCodec, VideoEncoding, VideoPublishOptions,
    VideoTrackAttachment,
};
use livekit_browser_dioxus::{
    use_audio_playback, use_browser_capabilities, use_participants, use_room_actions,
    use_room_session, use_room_snapshot, use_track_publications, LocalAudio, LocalVideo,
    RemoteAudio, RemoteTrackPublicationSubscription, RemoteVideo, RoomSessionProvider,
};
use livekit_browser_recording::{
    LocalClip, LocalClipLimits, LocalClipOptions, LocalClipRecorder, LocalClipRecording,
    LocalClipRecordingState, LocalClipStopReason, LocalClipTrack, RevocableObjectUrl,
};
use serde::Deserialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CredentialsResponse {
    participant_token: String,
    server_url: Option<String>,
}

struct RpcDelayCancellation {
    cancelled: Signal<bool>,
    completed: bool,
}

#[derive(Clone)]
struct ObservedChatMessage {
    id: String,
    content: String,
    sender_identity: String,
    timestamp_millis: u128,
    edit_timestamp_millis: Option<u128>,
}

#[derive(Clone)]
struct ObservedTranscriptionSegment {
    id: String,
    participant_identity: String,
    track_publication_sid: String,
    text: String,
    start_time: u64,
    end_time: u64,
    language: String,
    is_final: bool,
    first_received_at_millis: u128,
    last_received_at_millis: u128,
}

#[derive(Clone, Default)]
struct AudioAnalysisObservation {
    level: f32,
    waveform_length: usize,
    waveform_fingerprint: String,
}

fn livekit_log_level_label(level: LiveKitLogLevel) -> &'static str {
    match level {
        LiveKitLogLevel::Trace => "trace",
        LiveKitLogLevel::Debug => "debug",
        LiveKitLogLevel::Info => "info",
        LiveKitLogLevel::Warn => "warn",
        LiveKitLogLevel::Error => "error",
        _ => "unknown",
    }
}

impl AudioAnalysisObservation {
    fn read(analyser: &AudioAnalyser) -> Result<Self, LiveKitError> {
        let analysis = analyser.read()?;
        Ok(Self {
            level: analysis.level(),
            waveform_length: analysis.waveform().len(),
            waveform_fingerprint: waveform_fingerprint(analysis.waveform()),
        })
    }
}

fn install_audio_analyser(
    created: Option<Result<AudioAnalyser, LiveKitError>>,
    mut analyser: Signal<Option<AudioAnalyser>>,
    mut observation: Signal<AudioAnalysisObservation>,
    mut error: Signal<String>,
) {
    match created {
        Some(Ok(created)) => {
            observation.set(AudioAnalysisObservation::default());
            analyser.set(Some(created));
        }
        Some(Err(analyser_error)) => error.set(analyser_error.to_string()),
        None => {}
    }
}

fn install_audio_analysis(
    analysis: Option<Result<AudioAnalysisObservation, LiveKitError>>,
    mut observation: Signal<AudioAnalysisObservation>,
    mut error: Signal<String>,
) {
    match analysis {
        Some(Ok(analysis)) => observation.set(analysis),
        Some(Err(analyser_error)) => error.set(analyser_error.to_string()),
        None => {}
    }
}

async fn close_audio_analyser(
    analyser: Option<AudioAnalyser>,
    mut stored: Signal<Option<AudioAnalyser>>,
    mut error: Signal<String>,
) {
    let Some(analyser) = analyser else { return };
    if let Err(analyser_error) = analyser.close().await {
        error.set(analyser_error.to_string());
    }
    stored.set(Some(analyser));
}

type RpcOperation = Pin<Box<dyn Future<Output = Result<RpcPayload, LiveKitError>> + 'static>>;

#[derive(Clone, Copy)]
struct RpcResultSignals {
    response_kind: Signal<String>,
    response_text: Signal<String>,
    response_bytes: Signal<String>,
    error_kind: Signal<String>,
    error_code: Signal<String>,
    error_message: Signal<String>,
    error_data: Signal<String>,
}

#[derive(Clone, Copy)]
struct LocalClipSignals {
    recording: Signal<Option<LocalClipRecording>>,
    completion_abort: Signal<Option<AbortHandle>>,
    clip: Signal<Option<LocalClip>>,
    object_url: Signal<Option<RevocableObjectUrl>>,
    stop_returned: Signal<bool>,
    error_kind: Signal<String>,
    error: Signal<String>,
}

impl RpcDelayCancellation {
    fn complete(&mut self) {
        self.completed = true;
    }
}

impl Drop for RpcDelayCancellation {
    fn drop(&mut self) {
        if !self.completed {
            let mut cancelled = self.cancelled;
            cancelled.set(true);
        }
    }
}

const LARGE_STREAM_CHUNKS: usize = 128;
const BYTE_STREAM_CHUNK_SIZE: usize = 12_000;
const LOCAL_CLIP_ACCEPTANCE_SIZE_BYTES: u64 = 10;
const LOCAL_CLIP_MIME_PREFERENCES: &[&str] = &[
    "video/x-livekit-unsupported",
    "video/webm;codecs=vp8,opus",
    "video/webm;codecs=vp8",
    "video/webm",
    "video/mp4;codecs=avc1.42E01E,mp4a.40.2",
    "video/mp4",
];

fn text_stream_chunk() -> String {
    "LiveKit text stream λ|".repeat(500)
}

fn byte_stream_chunk(index: usize) -> Vec<u8> {
    (0..BYTE_STREAM_CHUNK_SIZE)
        .map(|offset| ((index + offset) % 251) as u8)
        .collect()
}

fn payload_checksum(payload: &[u8]) -> u64 {
    payload.iter().map(|byte| u64::from(*byte)).sum()
}

fn first_remote_identity(session: &RoomSession) -> Option<String> {
    session
        .snapshot()
        .remote_participants()
        .keys()
        .next()
        .cloned()
}

fn rpc_operation(
    session: Option<RoomSession>,
    method: &'static str,
    payload: RpcPayload,
    response_timeout: Option<Duration>,
) -> Option<RpcOperation> {
    let session = session?;
    let destination = first_remote_identity(&session)?;
    let options = RpcOptions::new(destination, method, payload);
    let options = response_timeout
        .map(|timeout| options.clone().with_response_timeout(timeout))
        .unwrap_or(options);
    Some(session.local_participant().invoke_rpc(options))
}

pub fn launch() {
    dioxus::launch(App);
}

fn query_value(name: &str) -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    let value = web_sys::UrlSearchParams::new_with_str(&search)
        .ok()?
        .get(name)?;
    (!value.trim().is_empty()).then_some(value)
}

fn generated_demo_identity(prefix: &str) -> String {
    let timestamp = js_sys::Date::now() as u64 % 1_000_000;
    let random = (js_sys::Math::random() * 10_000.0) as u64;
    format!("{prefix}-{timestamp:06}-{random:04}")
}

fn second_participant_url(room: &str, identity: &str) -> Option<String> {
    let href = web_sys::window()?.location().href().ok()?;
    let url = web_sys::Url::new(&href).ok()?;
    url.search_params().set("room", room);
    url.search_params().set("identity", identity);
    url.set_hash("");
    Some(url.href())
}

fn format_byte_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn format_duration(duration: Duration) -> String {
    format!("{:.2} s", duration.as_secs_f64())
}

#[derive(Clone, Default, PartialEq)]
struct DemoProgress {
    remote_participant_seen: bool,
    local_media_captured: bool,
    local_media_published: bool,
    local_media_unpublished: bool,
    local_media_stopped: bool,
    chat_message_exchanged: bool,
    local_clip_created: bool,
    local_clip_artifact_released: bool,
}

async fn request_credentials(room: &str, identity: &str) -> Result<ConnectionCredentials, String> {
    use wasm_bindgen::JsCast;

    let body = serde_json::json!({ "room": room, "identity": identity }).to_string();
    let options = web_sys::RequestInit::new();
    options.set_method("POST");
    options.set_body(&JsValue::from_str(&body));
    let request =
        web_sys::Request::new_with_str_and_init("/api/token", &options).map_err(js_error)?;
    request
        .headers()
        .set("content-type", "application/json")
        .map_err(js_error)?;
    let window = web_sys::window().ok_or_else(|| "browser window is unavailable".to_string())?;
    let response = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(js_error)?
        .dyn_into::<web_sys::Response>()
        .map_err(js_error)?;
    let json = JsFuture::from(response.text().map_err(js_error)?)
        .await
        .map_err(js_error)?
        .as_string()
        .ok_or_else(|| "credential response was not a string".to_string())?;
    if !response.ok() {
        return Err(format!(
            "Token service returned {}: {json}",
            response.status()
        ));
    }
    let response: CredentialsResponse =
        serde_json::from_str(&json).map_err(|error| error.to_string())?;
    let server_url = if let Some(server_url) = response.server_url {
        server_url
    } else {
        let protocol = if window.location().protocol().map_err(js_error)? == "https:" {
            "wss:"
        } else {
            "ws:"
        };
        let host = window.location().host().map_err(js_error)?;
        format!("{protocol}//{host}")
    };
    Ok(ConnectionCredentials::new(
        server_url,
        response.participant_token,
    ))
}

fn js_error(value: JsValue) -> String {
    js_sys::Reflect::get(&value, &JsValue::from_str("message"))
        .ok()
        .and_then(|message| message.as_string())
        .or_else(|| value.as_string())
        .unwrap_or_else(|| "unknown browser failure".into())
}

fn connection_label(state: RoomConnectionState) -> &'static str {
    match state {
        RoomConnectionState::Connecting => "connecting",
        RoomConnectionState::Connected => "connected",
        RoomConnectionState::Reconnecting => "reconnecting",
        RoomConnectionState::SignalReconnecting => "signal reconnecting",
        RoomConnectionState::Disconnecting => "disconnecting",
        RoomConnectionState::Disconnected => "disconnected",
        _ => "unknown",
    }
}

fn connection_quality_label(quality: ConnectionQuality) -> &'static str {
    match quality {
        ConnectionQuality::Excellent => "excellent",
        ConnectionQuality::Good => "good",
        ConnectionQuality::Poor => "poor",
        ConnectionQuality::Lost => "lost",
        ConnectionQuality::Unknown => "unknown",
        _ => "unknown",
    }
}

fn track_kind_label(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Audio => "audio",
        TrackKind::Video => "video",
        TrackKind::Unknown => "unknown",
        _ => "unknown",
    }
}

fn track_source_label(source: TrackSource) -> &'static str {
    match source {
        TrackSource::Camera => "camera",
        TrackSource::Microphone => "microphone",
        TrackSource::ScreenShare => "screenShare",
        TrackSource::ScreenShareAudio => "screenShareAudio",
        TrackSource::Unknown => "unknown",
        _ => "unknown",
    }
}

fn microphone_publish_options() -> AudioPublishOptions {
    AudioPublishOptions::default()
        .with_source(AudioTrackSource::Microphone)
        .with_name("microphone-capture")
        .with_codec(AudioCodec::Opus)
        .with_dtx(true)
        .with_red(false)
}

fn camera_capture_options(device_id: &str, facing_mode: &str) -> CameraCaptureOptions {
    let facing_mode = match facing_mode {
        "environment" => FacingMode::Environment,
        "left" => FacingMode::Left,
        "right" => FacingMode::Right,
        _ => FacingMode::User,
    };
    let options = CameraCaptureOptions::default()
        .with_facing_mode(facing_mode)
        .with_resolution(CameraResolution::new(640, 360))
        .with_frame_rate(30.0);
    if device_id.is_empty() {
        options
    } else {
        options.with_device_id(device_id)
    }
}

fn microphone_capture_options(device_id: &str) -> MicrophoneCaptureOptions {
    let options = MicrophoneCaptureOptions::default()
        .with_auto_gain_control(true)
        .with_channel_count(1)
        .with_echo_cancellation(true)
        .with_noise_suppression(true)
        .with_sample_rate(48_000);
    if device_id.is_empty() {
        options
    } else {
        options.with_device_id(device_id)
    }
}

fn media_operation_error_kind(error: &LiveKitError) -> &'static str {
    match error {
        LiveKitError::PermissionDenied(_) => "permissionDenied",
        LiveKitError::DisplayCaptureNotAllowed(_) => "displayCaptureNotAllowed",
        LiveKitError::DisplayCaptureRequiresActivation(_) => "displayCaptureRequiresActivation",
        LiveKitError::DeviceUnavailable(_) => "deviceUnavailable",
        LiveKitError::DeviceInUse(_) => "deviceInUse",
        LiveKitError::PublisherPermissionsFailed(_) => "publisherPermissionsFailed",
        LiveKitError::ParticipantUpdateFailed(_) => "participantUpdateFailed",
        LiveKitError::DataPacketSendFailed(_) => "dataPacketSendFailed",
        LiveKitError::ChatMessageSendFailed(_) => "chatMessageSendFailed",
        LiveKitError::DataStreamOpenFailed(_) => "dataStreamOpenFailed",
        LiveKitError::DataStreamWriteFailed(_) => "dataStreamWriteFailed",
        LiveKitError::DataStreamCompletionFailed(_) => "dataStreamCompletionFailed",
        LiveKitError::DataStreamReadFailed(_) => "dataStreamReadFailed",
        LiveKitError::DataStreamHandlerFailed(_) => "dataStreamHandlerFailed",
        LiveKitError::UnsupportedBrowser { .. } => "unsupportedBrowser",
        LiveKitError::InvalidState { .. } => "invalidState",
        _ => "captureFailed",
    }
}

fn apply_publisher_permissions(
    session: Option<RoomSession>,
    permissions: PublicationSubscriptionPermissions,
    next_state: &'static str,
    mut publisher_permissions_state: Signal<String>,
    mut media_error_kind: Signal<String>,
    mut error: Signal<String>,
) -> impl Future<Output = ()> {
    error.set(String::new());
    media_error_kind.set(String::new());
    let operation =
        session.map(|session| session.set_publication_subscription_permissions(permissions));

    async move {
        let Some(operation) = operation else { return };
        match operation.await {
            Ok(()) => publisher_permissions_state.set(next_state.into()),
            Err(permission_error) => {
                media_error_kind.set(media_operation_error_kind(&permission_error).into());
                error.set(permission_error.to_string());
            }
        }
    }
}

fn apply_local_participant_operation<F>(
    operation: Option<F>,
    mut media_error_kind: Signal<String>,
    mut error: Signal<String>,
) -> impl Future<Output = ()>
where
    F: Future<Output = Result<(), LiveKitError>>,
{
    error.set(String::new());
    media_error_kind.set(String::new());

    async move {
        let Some(operation) = operation else { return };
        if let Err(operation_error) = operation.await {
            media_error_kind.set(media_operation_error_kind(&operation_error).into());
            error.set(operation_error.to_string());
        }
    }
}

fn rpc_failure_label(kind: RpcFailureKind) -> &'static str {
    match kind {
        RpcFailureKind::UnsupportedMethod => "unsupportedMethod",
        RpcFailureKind::RemoteParticipantNotFound => "remoteParticipantNotFound",
        RpcFailureKind::RequestPayloadTooLarge => "requestPayloadTooLarge",
        RpcFailureKind::UnsupportedServer => "unsupportedServer",
        RpcFailureKind::UnsupportedVersion => "unsupportedVersion",
        RpcFailureKind::ApplicationError => "applicationError",
        RpcFailureKind::ConnectionTimeout => "connectionTimeout",
        RpcFailureKind::ResponseTimeout => "responseTimeout",
        RpcFailureKind::RemoteParticipantDisconnected => "remoteParticipantDisconnected",
        RpcFailureKind::ResponsePayloadTooLarge => "responsePayloadTooLarge",
        RpcFailureKind::SendFailed => "sendFailed",
        RpcFailureKind::ApplicationDefined => "applicationDefined",
        _ => "unknown",
    }
}

fn apply_rpc_operation<F>(
    operation: Option<F>,
    signals: RpcResultSignals,
) -> impl Future<Output = ()>
where
    F: Future<Output = Result<RpcPayload, LiveKitError>>,
{
    let RpcResultSignals {
        mut response_kind,
        mut response_text,
        mut response_bytes,
        mut error_kind,
        mut error_code,
        mut error_message,
        mut error_data,
    } = signals;
    response_kind.set(String::new());
    response_text.set(String::new());
    response_bytes.set(String::new());
    error_kind.set(String::new());
    error_code.set(String::new());
    error_message.set(String::new());
    error_data.set(String::new());

    async move {
        let Some(operation) = operation else { return };
        match operation.await {
            Ok(RpcPayload::Text(value)) => {
                response_kind.set("text".into());
                response_text.set(value);
            }
            Ok(RpcPayload::Bytes(value)) => {
                response_kind.set("bytes".into());
                response_bytes.set(
                    value
                        .iter()
                        .map(u8::to_string)
                        .collect::<Vec<_>>()
                        .join(","),
                );
            }
            Ok(_) => response_kind.set("unknown".into()),
            Err(LiveKitError::RpcInvocationFailed(failure)) => {
                error_kind.set(rpc_failure_label(failure.kind()).into());
                error_code.set(
                    failure
                        .code()
                        .map(|code| code.to_string())
                        .unwrap_or_default(),
                );
                error_message.set(failure.message().into());
                error_data.set(failure.data().unwrap_or_default().into());
            }
            Err(LiveKitError::InvalidState { .. }) => {
                error_kind.set("terminalRoomSession".into());
            }
            Err(operation_error) => {
                error_kind.set("other".into());
                error_message.set(operation_error.to_string());
            }
        }
    }
}

fn record_rpc_invocation(
    invocation: &RpcInvocation,
    mut handler_count: Signal<u64>,
    mut handler_caller: Signal<String>,
    mut handler_request_id: Signal<String>,
    mut handler_payload_kind: Signal<String>,
    mut handler_payload: Signal<String>,
) {
    handler_count += 1;
    handler_caller.set(invocation.caller_identity().into());
    handler_request_id.set(invocation.request_id().into());
    match invocation.payload() {
        RpcPayload::Text(value) => {
            handler_payload_kind.set("text".into());
            handler_payload.set(value.clone());
        }
        RpcPayload::Bytes(value) => {
            handler_payload_kind.set("bytes".into());
            handler_payload.set(
                value
                    .iter()
                    .map(u8::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        _ => handler_payload_kind.set("unknown".into()),
    }
}

fn local_clip_options(
    mime_preferences: &[&str],
    limits: LocalClipLimits,
) -> Result<LocalClipOptions, LiveKitError> {
    LocalClipOptions::new(
        mime_preferences
            .iter()
            .map(|preference| (*preference).to_string())
            .collect(),
        limits,
    )
}

fn local_clip_error_kind(error: &LiveKitError) -> &'static str {
    match error {
        LiveKitError::UnsupportedBrowser { .. } => "unsupportedBrowser",
        LiveKitError::UnsupportedLocalClipRecordingFormat { .. } => "unsupportedMimeType",
        LiveKitError::InvalidLocalClipRecordingOptions { .. }
        | LiveKitError::InvalidState { .. } => "invalidState",
        LiveKitError::EmptyLocalClip => "emptyArtifact",
        LiveKitError::LocalClipObjectUrlFailed(_) => "objectUrlFailed",
        _ => "recordingFailed",
    }
}

fn local_clip_state_label(state: LocalClipRecordingState) -> &'static str {
    match state {
        LocalClipRecordingState::Recording => "recording",
        LocalClipRecordingState::Paused => "paused",
        LocalClipRecordingState::Stopping => "stopping",
        LocalClipRecordingState::Stopped => "completed",
        _ => "unknown",
    }
}

fn local_clip_stop_reason_label(reason: LocalClipStopReason) -> &'static str {
    match reason {
        LocalClipStopReason::Explicit => "explicit",
        LocalClipStopReason::DurationLimit => "durationLimit",
        LocalClipStopReason::SizeLimit => "sizeLimit",
        LocalClipStopReason::SourceEnded => "sourceEnded",
        _ => "unknown",
    }
}

fn begin_local_clip(
    capabilities: Option<BrowserCapabilities>,
    video_track: Option<LocalVideoTrack>,
    audio_track: Option<LocalAudioTrack>,
    mime_preferences: &[&str],
    limits: LocalClipLimits,
    signals: LocalClipSignals,
) {
    let LocalClipSignals {
        mut recording,
        mut completion_abort,
        mut clip,
        mut object_url,
        mut stop_returned,
        mut error_kind,
        mut error,
    } = signals;
    let started = (|| {
        let capabilities = capabilities.ok_or(LiveKitError::UnsupportedTarget)?;
        let recorder = LocalClipRecorder::new(capabilities)?;
        let options = local_clip_options(mime_preferences, limits)?;
        let mut tracks = Vec::new();
        if let Some(video_track) = video_track {
            tracks.push(LocalClipTrack::from(video_track));
        }
        if let Some(audio_track) = audio_track {
            tracks.push(LocalClipTrack::from(audio_track));
        }
        recorder.start(tracks, options)
    })();

    match started {
        Ok(started) => {
            if let Some(previous) = completion_abort.write().take() {
                previous.abort();
            }
            error.set(String::new());
            error_kind.set(String::new());
            object_url.set(None);
            clip.set(None);
            stop_returned.set(false);
            let finished = started.finished();
            let (abort, registration) = AbortHandle::new_pair();
            completion_abort.set(Some(abort));
            recording.set(Some(started));
            wasm_bindgen_futures::spawn_local(async move {
                match Abortable::new(finished, registration).await {
                    Ok(Ok(completed)) => {
                        let created_url = completed.create_object_url();
                        clip.set(Some(completed));
                        match created_url {
                            Ok(created_url) => object_url.set(Some(created_url)),
                            Err(url_error) => {
                                error_kind.set(local_clip_error_kind(&url_error).into());
                                error.set(url_error.to_string());
                            }
                        }
                    }
                    Ok(Err(recording_error)) => {
                        error_kind.set(local_clip_error_kind(&recording_error).into());
                        error.set(recording_error.to_string());
                    }
                    Err(_) => {}
                }
            });
        }
        Err(recording_error) => {
            error_kind.set(local_clip_error_kind(&recording_error).into());
            error.set(recording_error.to_string());
        }
    }
}

#[component]
fn App() -> Element {
    let runtime = use_signal(|| {
        BrowserClient::initialize(
            InitializeOptions::default().with_log_callback(move |record| {
                let Some(element) = web_sys::window()
                    .and_then(|window| window.document())
                    .and_then(|document| document.get_element_by_id("livekit-log-record"))
                else {
                    return;
                };
                let _ =
                    element.set_attribute("data-level", livekit_log_level_label(record.level()));
                let _ = element.set_attribute("data-message", record.message());
                let _ = element.set_attribute("data-context", &record.context().to_string());
            }),
        )
    });
    let mut room_name = use_signal(|| query_value("room").unwrap_or_else(|| "wasm-lab".into()));
    let mut identity =
        use_signal(|| query_value("identity").unwrap_or_else(|| generated_demo_identity("guest")));
    let second_identity = use_signal(|| generated_demo_identity("guest-remote-participant"));
    let mut session = use_signal(|| None::<RoomSession>);
    let mut retained_session = use_signal(|| None::<RoomSession>);
    let raw_js_contract = use_signal(|| {
        if cfg!(feature = "raw-js-contract") {
            "pending"
        } else {
            "disabled"
        }
        .to_string()
    });
    let mut dioxus_event_capacity = use_signal(|| 64_usize);
    let mut snapshot_refresh = use_signal(|| 0_u64);
    let mut event_count = use_signal(|| 0_u64);
    let mut event_observing = use_signal(|| false);
    let mut event_revision = use_signal(|| 0_u64);
    let mut event_snapshot_revision = use_signal(|| 0_u64);
    let mut event_type = use_signal(String::new);
    let mut event_types = use_signal(Vec::<String>::new);
    let mut event_identity = use_signal(String::new);
    let mut event_quality = use_signal(String::new);
    let mut held_remote_participant = use_signal(|| None::<RemoteParticipant>);
    let mut held_remote_publication = use_signal(|| None::<RemoteTrackPublication>);
    let mut held_remote_media = use_signal(|| None::<RemoteMediaTrack>);
    let mut held_remote_mutation = use_signal(String::new);
    let mut connection = use_signal(|| RoomConnectionState::Disconnected);
    let mut data_packet_observer_abort = use_signal(|| None::<AbortHandle>);
    let mut data_packet_observing = use_signal(|| false);
    let mut data_packet_stream_closed = use_signal(|| false);
    let mut reliable_packet_count = use_signal(|| 0_u64);
    let mut reliable_packet_sender = use_signal(String::new);
    let mut reliable_packet_payload = use_signal(String::new);
    let mut lossy_packet_count = use_signal(|| 0_u64);
    let mut lossy_packet_sender = use_signal(String::new);
    let mut lossy_packet_payload = use_signal(String::new);
    let mut data_packet_dropped = use_signal(|| 0_u64);
    let mut chat_message_observing = use_signal(|| false);
    let mut chat_message_stream_closed = use_signal(|| false);
    let mut observed_chat_messages = use_signal(Vec::<ObservedChatMessage>::new);
    let mut chat_message_dropped = use_signal(|| 0_u64);
    let mut sent_chat_message_count = use_signal(|| 0_u64);
    let mut chat_message_send_error = use_signal(String::new);
    let mut chat_message_input = use_signal(String::new);
    let mut chat_message_sending = use_signal(|| false);
    let mut transcription_observing = use_signal(|| false);
    let mut transcription_stream_closed = use_signal(|| false);
    let mut observed_transcription_segments = use_signal(Vec::<ObservedTranscriptionSegment>::new);
    let mut transcription_dropped = use_signal(|| 0_u64);
    let mut text_stream_handler = use_signal(|| None::<TextStreamHandlerRegistration>);
    let mut byte_stream_handler = use_signal(|| None::<ByteStreamHandlerRegistration>);
    let mut text_stream_abort = use_signal(|| None::<AbortHandle>);
    let mut byte_stream_abort = use_signal(|| None::<AbortHandle>);
    let mut text_stream_sender = use_signal(String::new);
    let mut text_stream_bytes = use_signal(|| 0_u64);
    let mut text_stream_checksum = use_signal(|| 0_u64);
    let mut text_stream_completed = use_signal(|| false);
    let mut text_stream_cancelled = use_signal(|| false);
    let mut text_stream_error = use_signal(String::new);
    let mut text_stream_read_pending = use_signal(|| false);
    let mut byte_stream_sender = use_signal(String::new);
    let mut byte_stream_bytes = use_signal(|| 0_u64);
    let mut byte_stream_checksum = use_signal(|| 0_u64);
    let mut byte_stream_completed = use_signal(|| false);
    let mut byte_stream_cancelled = use_signal(|| false);
    let mut byte_stream_error = use_signal(String::new);
    let mut byte_stream_read_pending = use_signal(|| false);
    let mut data_stream_reads_paused = use_signal(|| false);
    let mut sent_text_stream_bytes = use_signal(|| 0_u64);
    let mut sent_text_stream_writes = use_signal(|| 0_u64);
    let mut sent_text_stream_completed = use_signal(|| false);
    let mut sent_byte_stream_bytes = use_signal(|| 0_u64);
    let mut sent_byte_stream_writes = use_signal(|| 0_u64);
    let mut sent_byte_stream_completed = use_signal(|| false);
    let mut rpc_echo_registration = use_signal(|| None::<RpcMethodRegistration>);
    let mut rpc_method_registrations = use_signal(Vec::<RpcMethodRegistration>::new);
    let rpc_handler_count = use_signal(|| 0_u64);
    let rpc_handler_caller = use_signal(String::new);
    let rpc_handler_request_id = use_signal(String::new);
    let rpc_handler_payload_kind = use_signal(String::new);
    let rpc_handler_payload = use_signal(String::new);
    let mut rpc_delay_started = use_signal(|| false);
    let mut rpc_delay_completed = use_signal(|| false);
    let mut rpc_delay_cancelled = use_signal(|| false);
    let mut rpc_abandoned_count = use_signal(|| 0_u64);
    let rpc_response_kind = use_signal(String::new);
    let rpc_response_text = use_signal(String::new);
    let rpc_response_bytes = use_signal(String::new);
    let rpc_error_kind = use_signal(String::new);
    let rpc_error_code = use_signal(String::new);
    let rpc_error_message = use_signal(String::new);
    let rpc_error_data = use_signal(String::new);
    let rpc_result_signals = RpcResultSignals {
        response_kind: rpc_response_kind,
        response_text: rpc_response_text,
        response_bytes: rpc_response_bytes,
        error_kind: rpc_error_kind,
        error_code: rpc_error_code,
        error_message: rpc_error_message,
        error_data: rpc_error_data,
    };
    let mut local_video_track = use_signal(|| None::<LocalVideoTrack>);
    let mut local_video_publication = use_signal(|| None::<LocalVideoTrackPublication>);
    let mut local_video_attachment = use_signal(|| None::<VideoTrackAttachment>);
    let mut local_video_element = use_signal(|| None::<web_sys::HtmlVideoElement>);
    let mut local_audio_track = use_signal(|| None::<LocalAudioTrack>);
    let mut local_audio_publication = use_signal(|| None::<LocalAudioTrackPublication>);
    let mut local_audio_analyser = use_signal(|| None::<AudioAnalyser>);
    let local_audio_analysis = use_signal(AudioAnalysisObservation::default);
    let mut local_clip_recording = use_signal(|| None::<LocalClipRecording>);
    let mut local_clip_completion_abort = use_signal(|| None::<AbortHandle>);
    let local_clip = use_signal(|| None::<LocalClip>);
    let mut local_clip_object_url = use_signal(|| None::<RevocableObjectUrl>);
    let mut local_clip_error = use_signal(String::new);
    let mut local_clip_stop_returned = use_signal(|| false);
    let mut local_clip_downloaded = use_signal(|| false);
    let mut demo_progress = use_signal(DemoProgress::default);
    let mut screen_capture = use_signal(|| None::<ScreenCapture>);
    let mut screen_video_publication = use_signal(|| None::<LocalVideoTrackPublication>);
    let mut screen_audio_publication = use_signal(|| None::<LocalAudioTrackPublication>);
    let mut local_audio_statistics_timestamp = use_signal(|| 0.0_f64);
    let mut local_audio_statistics_packets_sent = use_signal(|| 0_u64);
    let mut local_audio_statistics_bytes_sent = use_signal(|| 0_u64);
    let mut local_video_statistics_layers = use_signal(|| 0_usize);
    let mut local_video_statistics_frames_sent = use_signal(|| 0_u64);
    let mut screen_video_attachment = use_signal(|| None::<VideoTrackAttachment>);
    let mut screen_audio_attachment = use_signal(|| None::<AudioTrackAttachment>);
    let mut screen_video_element = use_signal(|| None::<web_sys::HtmlVideoElement>);
    let mut screen_state_refresh = use_signal(|| 0_u64);
    let mut request_system_audio = use_signal(|| true);
    let mut camera_devices = use_signal(Vec::<MediaDeviceInfo>::new);
    let mut microphone_devices = use_signal(Vec::<MediaDeviceInfo>::new);
    let mut selected_camera_device = use_signal(String::new);
    let mut selected_microphone_device = use_signal(String::new);
    let mut active_camera_device = use_signal(String::new);
    let mut active_microphone_device = use_signal(String::new);
    let mut camera_facing_mode = use_signal(|| "user".to_string());
    let mut allowed_participant_identity = use_signal(String::new);
    let mut publisher_permissions_state = use_signal(|| "allParticipants".to_string());
    let mut participant_metadata_input = use_signal(String::new);
    let mut participant_attribute_key = use_signal(String::new);
    let mut participant_attribute_value = use_signal(String::new);
    let mut media_error_kind = use_signal(String::new);
    let mut error = use_signal(String::new);
    let local_clip_signals = LocalClipSignals {
        recording: local_clip_recording,
        completion_abort: local_clip_completion_abort,
        clip: local_clip,
        object_url: local_clip_object_url,
        stop_returned: local_clip_stop_returned,
        error_kind: local_clip_error,
        error,
    };

    use_effect(move || {
        let _ = snapshot_refresh.read();
        let _ = screen_state_refresh.read();
        let remote_participant_seen = session
            .read()
            .as_ref()
            .is_some_and(|session| !session.snapshot().remote_participants().is_empty());
        let video_track = local_video_track.read();
        let audio_track = local_audio_track.read();
        let screen_capture_handle = screen_capture.read();
        let local_media_captured =
            video_track.is_some() || audio_track.is_some() || screen_capture_handle.is_some();
        let local_media_stopped = video_track
            .as_ref()
            .is_some_and(LocalVideoTrack::is_stopped)
            || audio_track
                .as_ref()
                .is_some_and(LocalAudioTrack::is_stopped)
            || screen_capture_handle
                .as_ref()
                .is_some_and(|capture| capture.display_video().is_stopped());
        drop(video_track);
        drop(audio_track);
        drop(screen_capture_handle);
        let local_media_published = local_video_publication
            .read()
            .as_ref()
            .is_some_and(|publication| !publication.is_terminal())
            || local_audio_publication
                .read()
                .as_ref()
                .is_some_and(|publication| !publication.is_terminal())
            || screen_video_publication
                .read()
                .as_ref()
                .is_some_and(|publication| !publication.is_terminal())
            || screen_audio_publication
                .read()
                .as_ref()
                .is_some_and(|publication| !publication.is_terminal());
        let chat_message_exchanged =
            !observed_chat_messages.read().is_empty() || *sent_chat_message_count.read() > 0;
        let local_clip_created = local_clip.read().is_some();
        let local_clip_artifact_released = *local_clip_downloaded.read()
            || local_clip_object_url
                .read()
                .as_ref()
                .is_some_and(|url| !url.is_active());

        let mut next = demo_progress.peek().clone();
        next.remote_participant_seen |= remote_participant_seen;
        next.local_media_captured |= local_media_captured;
        next.local_media_published |= local_media_published;
        next.local_media_stopped |= local_media_stopped;
        next.chat_message_exchanged |= chat_message_exchanged;
        next.local_clip_created |= local_clip_created;
        next.local_clip_artifact_released |= local_clip_artifact_released;
        if next != *demo_progress.peek() {
            demo_progress.set(next);
        }
    });

    let runtime_read = runtime.read();
    let runtime_ready = runtime_read.is_ok();
    let runtime_error = runtime_read
        .as_ref()
        .err()
        .map(ToString::to_string)
        .unwrap_or_default();
    let runtime_capabilities = runtime_read.as_ref().ok().map(BrowserClient::capabilities);
    let runtime_sdk_version = runtime_read
        .as_ref()
        .ok()
        .map(BrowserClient::sdk_version)
        .unwrap_or_default();
    drop(runtime_read);

    let _snapshot_refresh = snapshot_refresh.read();
    let session_handle = session.read().clone();
    let snapshot = session_handle.as_ref().map(RoomSession::snapshot);
    let terminal = session_handle
        .as_ref()
        .map(RoomSession::is_terminal)
        .unwrap_or(false);
    let retained_session_handle = retained_session.read().clone();
    let retained_terminal = retained_session_handle
        .as_ref()
        .map(RoomSession::is_terminal)
        .unwrap_or(false);
    let retained_connection = retained_session_handle
        .as_ref()
        .map(|session| connection_label(session.snapshot().connection()))
        .unwrap_or("absent");
    let connection_state = *connection.read();
    let connection_text = connection_label(connection_state).to_string();
    let connected = connection_state == RoomConnectionState::Connected;
    let busy = matches!(
        connection_state,
        RoomConnectionState::Connecting
            | RoomConnectionState::Disconnecting
            | RoomConnectionState::Reconnecting
            | RoomConnectionState::SignalReconnecting
    );
    let remote_participant_count = snapshot
        .as_ref()
        .map(|snapshot| snapshot.remote_participants().len())
        .unwrap_or_default();
    let participant_count = usize::from(connected) + remote_participant_count;
    let audio_playback = snapshot
        .as_ref()
        .map(|snapshot| snapshot.audio_playback())
        .unwrap_or(AudioPlaybackState::Allowed);
    let camera_stopped = local_video_track
        .read()
        .as_ref()
        .is_none_or(LocalVideoTrack::is_stopped);
    let camera_active = !camera_stopped;
    let preview_attached = local_video_attachment
        .read()
        .as_ref()
        .is_some_and(VideoTrackAttachment::is_active);
    let video_published = local_video_publication
        .read()
        .as_ref()
        .is_some_and(|publication| !publication.is_terminal());
    let video_publication_terminal = local_video_publication
        .read()
        .as_ref()
        .is_some_and(LocalVideoTrackPublication::is_terminal);
    let video_publication_sid = local_video_publication
        .read()
        .as_ref()
        .and_then(LocalVideoTrackPublication::sid)
        .unwrap_or_default();
    let microphone_stopped = local_audio_track
        .read()
        .as_ref()
        .is_none_or(LocalAudioTrack::is_stopped);
    let microphone_active = !microphone_stopped;
    let audio_published = local_audio_publication
        .read()
        .as_ref()
        .is_some_and(|publication| !publication.is_terminal());
    let audio_publication_muted = local_audio_publication
        .read()
        .as_ref()
        .is_some_and(LocalAudioTrackPublication::is_muted);
    let audio_publication_terminal = local_audio_publication
        .read()
        .as_ref()
        .is_some_and(LocalAudioTrackPublication::is_terminal);
    let audio_publication_sid = local_audio_publication
        .read()
        .as_ref()
        .and_then(LocalAudioTrackPublication::sid)
        .unwrap_or_default();
    let local_audio_analyser_present = local_audio_analyser.read().is_some();
    let local_audio_analyser_active = local_audio_analyser
        .read()
        .as_ref()
        .is_some_and(AudioAnalyser::is_active);
    let local_audio_analysis_observation = local_audio_analysis.read().clone();
    let local_clip_supported = runtime_capabilities
        .map(|capabilities| capabilities.local_clip_recording())
        .unwrap_or(false);
    let local_clip_recording_handle = local_clip_recording.read().clone();
    let local_clip_handle = local_clip.read().clone();
    let local_clip_state = local_clip_recording_handle
        .as_ref()
        .map(|recording| local_clip_state_label(recording.state()))
        .unwrap_or("idle");
    let local_clip_selected_mime = local_clip_recording_handle
        .as_ref()
        .map(LocalClipRecording::mime_type)
        .unwrap_or_default();
    let local_clip_mime = local_clip_handle
        .as_ref()
        .map(LocalClip::mime_type)
        .unwrap_or_default();
    let local_clip_blob_type = local_clip_handle
        .as_ref()
        .map(|clip| clip.blob().type_())
        .unwrap_or_default();
    let local_clip_blob_size = local_clip_handle
        .as_ref()
        .map(LocalClip::size_bytes)
        .unwrap_or_default();
    let local_clip_duration_ms = local_clip_handle
        .as_ref()
        .map(|clip| clip.duration().as_millis())
        .unwrap_or_default();
    let local_clip_stop_reason = local_clip_handle
        .as_ref()
        .map(|clip| local_clip_stop_reason_label(clip.stop_reason()))
        .unwrap_or_default();
    let local_clip_filename = local_clip_handle
        .as_ref()
        .map(LocalClip::suggested_filename)
        .unwrap_or_default();
    let local_clip_url_active = local_clip_object_url
        .read()
        .as_ref()
        .is_some_and(RevocableObjectUrl::is_active);
    let local_clip_url = local_clip_object_url
        .read()
        .as_ref()
        .filter(|url| url.is_active())
        .map(|url| url.as_str().to_string())
        .unwrap_or_default();
    let local_clip_size = format_byte_size(local_clip_blob_size);
    let local_clip_duration = local_clip_handle
        .as_ref()
        .map(|clip| format_duration(clip.duration()))
        .unwrap_or_default();
    let local_clip_artifact_ready = local_clip_handle.is_some();
    let local_clip_url_created = local_clip_object_url.read().is_some();
    let _screen_state_refresh = screen_state_refresh.read();
    let screen_capture_handle = screen_capture.read().clone();
    let screen_video_stopped = screen_capture_handle
        .as_ref()
        .is_none_or(|capture| capture.display_video().is_stopped());
    let screen_video_active = !screen_video_stopped;
    let screen_audio_present = screen_capture_handle
        .as_ref()
        .and_then(ScreenCapture::system_audio)
        .is_some();
    let screen_audio_stopped = screen_capture_handle
        .as_ref()
        .and_then(ScreenCapture::system_audio)
        .is_none_or(LocalAudioTrack::is_stopped);
    let screen_preview_attached = screen_video_attachment
        .read()
        .as_ref()
        .is_some_and(VideoTrackAttachment::is_active);
    let screen_audio_preview_attached = screen_audio_attachment
        .read()
        .as_ref()
        .is_some_and(AudioTrackAttachment::is_active);
    let screen_video_published = screen_video_publication
        .read()
        .as_ref()
        .is_some_and(|publication| !publication.is_terminal());
    let screen_audio_published = screen_audio_publication
        .read()
        .as_ref()
        .is_some_and(|publication| !publication.is_terminal());
    let camera_device_count = camera_devices.read().len();
    let microphone_device_count = microphone_devices.read().len();
    let active_camera_device_id = active_camera_device.read().clone();
    let active_microphone_device_id = active_microphone_device.read().clone();
    let held_participant_terminal = held_remote_participant
        .read()
        .as_ref()
        .is_some_and(RemoteParticipant::is_terminal);
    let held_publication_terminal = held_remote_publication
        .read()
        .as_ref()
        .is_some_and(RemoteTrackPublication::is_terminal);
    let held_media_terminal = held_remote_media
        .read()
        .as_ref()
        .is_some_and(RemoteMediaTrack::is_terminal);
    let visible_chat_messages = observed_chat_messages.read().clone();
    let chat_message_count = visible_chat_messages.len();
    let chat_message_contents = visible_chat_messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("|");
    let chat_message_senders = visible_chat_messages
        .iter()
        .map(|message| message.sender_identity.as_str())
        .collect::<Vec<_>>()
        .join("|");
    let chat_message_ids = visible_chat_messages
        .iter()
        .map(|message| message.id.as_str())
        .collect::<Vec<_>>()
        .join("|");
    let chat_message_timestamps = visible_chat_messages
        .iter()
        .map(|message| message.timestamp_millis.to_string())
        .collect::<Vec<_>>()
        .join("|");
    let chat_message_edit_timestamps = visible_chat_messages
        .iter()
        .map(|message| {
            message
                .edit_timestamp_millis
                .map(|timestamp| timestamp.to_string())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join("|");
    let second_participant_href =
        second_participant_url(room_name.read().trim(), second_identity.read().trim())
            .unwrap_or_default();
    let demo_progress_snapshot = demo_progress.read().clone();
    let transcription_segments_read = observed_transcription_segments.read();
    let transcription_segment_count = transcription_segments_read.len();
    let transcription_segment_ids = transcription_segments_read
        .iter()
        .map(|segment| segment.id.as_str())
        .collect::<Vec<_>>()
        .join("|");
    let transcription_segment_participants = transcription_segments_read
        .iter()
        .map(|segment| segment.participant_identity.as_str())
        .collect::<Vec<_>>()
        .join("|");
    let transcription_segment_track_publication_sids = transcription_segments_read
        .iter()
        .map(|segment| segment.track_publication_sid.as_str())
        .collect::<Vec<_>>()
        .join("|");
    let transcription_segment_texts = transcription_segments_read
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join("|");
    let transcription_segment_start_times = transcription_segments_read
        .iter()
        .map(|segment| segment.start_time.to_string())
        .collect::<Vec<_>>()
        .join("|");
    let transcription_segment_end_times = transcription_segments_read
        .iter()
        .map(|segment| segment.end_time.to_string())
        .collect::<Vec<_>>()
        .join("|");
    let transcription_segment_languages = transcription_segments_read
        .iter()
        .map(|segment| segment.language.as_str())
        .collect::<Vec<_>>()
        .join("|");
    let transcription_segment_finals = transcription_segments_read
        .iter()
        .map(|segment| segment.is_final.to_string())
        .collect::<Vec<_>>()
        .join("|");
    let transcription_segment_first_received_times = transcription_segments_read
        .iter()
        .map(|segment| segment.first_received_at_millis.to_string())
        .collect::<Vec<_>>()
        .join("|");
    let transcription_segment_last_received_times = transcription_segments_read
        .iter()
        .map(|segment| segment.last_received_at_millis.to_string())
        .collect::<Vec<_>>()
        .join("|");
    drop(transcription_segments_read);

    let revision = snapshot
        .as_ref()
        .map(|snapshot| snapshot.revision().to_string())
        .unwrap_or_else(|| "0".into());
    let room_metadata = snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.room_metadata())
        .unwrap_or_default();
    let active_speakers = snapshot
        .as_ref()
        .map(|snapshot| snapshot.active_speakers().join("|"))
        .unwrap_or_default();
    let participant_identity = snapshot
        .as_ref()
        .map(|snapshot| snapshot.local_participant().identity())
        .unwrap_or_default();
    let participant_sid = snapshot
        .as_ref()
        .map(|snapshot| snapshot.local_participant().sid())
        .unwrap_or_default();
    let participant_connection_quality = snapshot
        .as_ref()
        .map(|snapshot| connection_quality_label(snapshot.local_participant().connection_quality()))
        .unwrap_or("unknown");
    let participant_metadata = snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.local_participant().metadata())
        .unwrap_or_default();
    let attributes_count = snapshot
        .as_ref()
        .map(|snapshot| snapshot.local_participant().attributes().len())
        .unwrap_or_default();
    let publication_count = snapshot
        .as_ref()
        .map(|snapshot| snapshot.local_participant().track_publications().len())
        .unwrap_or_default();
    let can_publish = snapshot
        .as_ref()
        .map(|snapshot| snapshot.local_participant().grants().can_publish())
        .unwrap_or(false);
    let can_subscribe = snapshot
        .as_ref()
        .map(|snapshot| snapshot.local_participant().grants().can_subscribe())
        .unwrap_or(false);
    let can_publish_data = snapshot
        .as_ref()
        .map(|snapshot| snapshot.local_participant().grants().can_publish_data())
        .unwrap_or(false);
    let can_publish_source_count = snapshot
        .as_ref()
        .map(|snapshot| {
            snapshot
                .local_participant()
                .grants()
                .can_publish_sources()
                .len()
        })
        .unwrap_or_default();
    let can_update_metadata = snapshot
        .as_ref()
        .map(|snapshot| snapshot.local_participant().grants().can_update_metadata())
        .unwrap_or(false);
    let hidden = snapshot
        .as_ref()
        .map(|snapshot| snapshot.local_participant().grants().hidden())
        .unwrap_or(false);
    let recorder = snapshot
        .as_ref()
        .map(|snapshot| snapshot.local_participant().grants().recorder())
        .unwrap_or(false);
    let agent = snapshot
        .as_ref()
        .map(|snapshot| snapshot.local_participant().grants().agent())
        .unwrap_or(false);
    let can_subscribe_metrics = snapshot
        .as_ref()
        .map(|snapshot| {
            snapshot
                .local_participant()
                .grants()
                .can_subscribe_metrics()
        })
        .unwrap_or(false);
    let can_manage_agent_session = snapshot
        .as_ref()
        .map(|snapshot| {
            snapshot
                .local_participant()
                .grants()
                .can_manage_agent_session()
        })
        .unwrap_or(false);

    rsx! {
        main { class: "shell",
            if let (Some(provided), Some(capabilities)) =
                (session_handle.clone(), runtime_capabilities)
            {
                RoomSessionProvider {
                    session: provided,
                    capabilities,
                    event_capacity: *dioxus_event_capacity.read(),
                    RoomSessionIntegration {
                        connection,
                        refresh: snapshot_refresh,
                        local_video_track,
                        local_audio_track,
                        error,
                    }
                }
            }
            button {
                id: "change-dioxus-event-capacity",
                hidden: true,
                r#type: "button",
                onclick: move |_| dioxus_event_capacity.set(1),
                "Change Dioxus event capacity"
            }
            div {
                id: "retained-room-session",
                hidden: true,
                "data-present": retained_session_handle.is_some(),
                "data-terminal": retained_terminal,
                "data-connection": retained_connection,
            }
            button {
                id: "release-retained-room-session",
                hidden: true,
                r#type: "button",
                onclick: move |_| retained_session.set(None),
                "Release retained Room Session"
            }
            button {
                id: "recover-local-publication-handles",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    local_video_publication.set(
                        local_video_track.read().as_ref().and_then(LocalVideoTrack::publication),
                    );
                    local_audio_publication.set(
                        local_audio_track.read().as_ref().and_then(LocalAudioTrack::publication),
                    );
                },
                "Recover Local Track Publication handles"
            }
            div {
                id: "browser-runtime",
                hidden: true,
                "data-state": if runtime_ready { "ready" } else { "failed" },
                "data-sdk-version": runtime_sdk_version,
                "data-webrtc": runtime_capabilities.map(|capabilities| capabilities.web_rtc()).unwrap_or(false),
                "data-secure-context": runtime_capabilities.map(|capabilities| capabilities.secure_context()).unwrap_or(false),
                "data-user-media": runtime_capabilities.map(|capabilities| capabilities.user_media()).unwrap_or(false),
                "data-display-capture": runtime_capabilities.map(|capabilities| capabilities.display_capture()).unwrap_or(false),
                "data-audio-playback": runtime_capabilities.map(|capabilities| capabilities.audio_playback()).unwrap_or(false),
                "data-audio-output-selection": runtime_capabilities.map(|capabilities| capabilities.audio_output_selection()).unwrap_or(false),
                "data-transcription": runtime_capabilities.map(|capabilities| capabilities.transcription()).unwrap_or(false),
                "data-local-clip-recording": local_clip_supported,
            }
            div {
                id: "raw-js-contract",
                hidden: true,
                "data-state": raw_js_contract,
            }
            div {
                id: "livekit-log-record",
                hidden: true,
                "data-level": "",
                "data-message": "",
                "data-context": "",
            }
            div {
                id: "room-session",
                hidden: true,
                "data-revision": revision,
                "data-room-metadata": room_metadata,
                "data-active-speakers": active_speakers,
                "data-identity": participant_identity,
                "data-participant-sid": participant_sid,
                "data-connection-quality": participant_connection_quality,
                "data-metadata": participant_metadata,
                "data-attribute-count": attributes_count,
                "data-publication-count": publication_count,
                "data-can-publish": can_publish,
                "data-can-subscribe": can_subscribe,
                "data-can-publish-data": can_publish_data,
                "data-can-publish-source-count": can_publish_source_count,
                "data-can-update-metadata": can_update_metadata,
                "data-hidden": hidden,
                "data-recorder": recorder,
                "data-agent": agent,
                "data-can-subscribe-metrics": can_subscribe_metrics,
                "data-can-manage-agent-session": can_manage_agent_session,
                "data-terminal": terminal,
                "data-remote-participant-count": remote_participant_count,
                if let Some(snapshot) = snapshot.as_ref() {
                    for (key, value) in snapshot.local_participant().attributes() {
                        div {
                            class: "local-participant-attribute",
                            "data-key": key.as_str(),
                            "data-value": value.as_str(),
                        }
                    }
                    for (publication_sid, publication) in snapshot.local_participant().track_publications() {
                        div {
                            class: "local-track-publication",
                            "data-publication-sid": publication_sid.as_str(),
                            "data-name": publication.name(),
                            "data-source": track_source_label(publication.source()),
                            "data-kind": track_kind_label(publication.kind()),
                            "data-muted": publication.muted(),
                        }
                    }
                }
            }
            div {
                id: "publisher-permissions",
                hidden: true,
                "data-state": publisher_permissions_state,
            }
            div { id: "remote-participants", hidden: true, "data-count": remote_participant_count,
                if let Some(snapshot) = snapshot.as_ref() {
                    for (remote_identity, participant) in snapshot.remote_participants() {
                        div {
                            class: "remote-participant",
                            "data-identity": remote_identity.as_str(),
                            "data-participant-sid": participant.sid(),
                            "data-connection-quality": connection_quality_label(participant.connection_quality()),
                            "data-metadata": participant.metadata().unwrap_or_default(),
                            "data-attribute-count": participant.attributes().len(),
                            "data-publication-count": participant.track_publications().len(),
                            for (key, value) in participant.attributes() {
                                div {
                                    class: "participant-attribute",
                                    "data-key": key.as_str(),
                                    "data-value": value.as_str(),
                                }
                            }
                            for (publication_sid, publication) in participant.track_publications() {
                                div {
                                    class: "remote-track-publication",
                                    "data-publication-sid": publication_sid.as_str(),
                                    "data-kind": track_kind_label(publication.kind()),
                                    "data-muted": publication.muted(),
                                    "data-subscribed": publication.subscribed(),
                                    "data-media-available": publication.media_available(),
                                }
                            }
                            for (data_track_sid, data_track) in participant.data_tracks() {
                                div {
                                    class: "remote-data-track",
                                    "data-track-sid": data_track_sid.as_str(),
                                    "data-name": data_track.name(),
                                    "data-uses-e2ee": data_track.uses_e2ee(),
                                }
                            }
                        }
                    }
                }
            }
            div {
                id: "room-events",
                hidden: true,
                "data-count": event_count,
                "data-observing": event_observing,
                "data-event-revision": event_revision,
                "data-snapshot-revision": event_snapshot_revision,
                "data-event-type": event_type,
                "data-event-types": event_types.read().join("|"),
                "data-event-identity": event_identity,
                "data-event-quality": event_quality,
            }
            div {
                id: "local-media-statistics",
                hidden: true,
                "data-audio-timestamp": local_audio_statistics_timestamp,
                "data-audio-packets-sent": local_audio_statistics_packets_sent,
                "data-audio-bytes-sent": local_audio_statistics_bytes_sent,
                "data-video-layer-count": local_video_statistics_layers,
                "data-video-frames-sent": local_video_statistics_frames_sent,
            }
            div {
                id: "held-remote-handles",
                hidden: true,
                "data-participant-terminal": held_participant_terminal,
                "data-publication-terminal": held_publication_terminal,
                "data-media-terminal": held_media_terminal,
                "data-mutation": held_remote_mutation,
            }
            div {
                id: "audio-playback",
                hidden: true,
                "data-state": match audio_playback {
                    AudioPlaybackState::Blocked => "blocked",
                    _ => "allowed",
                },
            }
            div {
                id: "local-video-track",
                hidden: true,
                "data-present": local_video_track.read().is_some(),
                "data-stopped": camera_stopped,
                "data-preview-attached": preview_attached,
                "data-publication-terminal": video_publication_terminal,
                "data-publication-sid": video_publication_sid,
            }
            div {
                id: "local-audio-track",
                hidden: true,
                "data-present": local_audio_track.read().is_some(),
                "data-stopped": microphone_stopped,
                "data-publication-muted": audio_publication_muted,
                "data-publication-terminal": audio_publication_terminal,
                "data-publication-sid": audio_publication_sid,
                "data-analyser-present": local_audio_analyser_present,
                "data-analyser-active": local_audio_analyser_active,
                "data-analyser-level": local_audio_analysis_observation.level,
                "data-analyser-waveform-length": local_audio_analysis_observation.waveform_length,
                "data-analyser-waveform-fingerprint": local_audio_analysis_observation.waveform_fingerprint,
            }
            div {
                id: "local-clip-recording",
                hidden: true,
                "data-supported": local_clip_supported,
                "data-state": local_clip_state,
                "data-selected-mime": local_clip_selected_mime,
                "data-mime-type": local_clip_mime,
                "data-blob-present": local_clip_handle.is_some(),
                "data-blob-type": local_clip_blob_type,
                "data-blob-size": local_clip_blob_size,
                "data-duration-ms": local_clip_duration_ms,
                "data-stop-reason": local_clip_stop_reason,
                "data-filename": local_clip_filename,
                "data-object-url-active": local_clip_url_active,
                "data-error-kind": local_clip_error,
                "data-stop-returned": local_clip_stop_returned,
                "data-size-limit-bytes": LOCAL_CLIP_ACCEPTANCE_SIZE_BYTES,
            }
            div {
                id: "screen-capture",
                hidden: true,
                "data-present": screen_capture_handle.is_some(),
                "data-video-stopped": screen_video_stopped,
                "data-system-audio-present": screen_audio_present,
                "data-system-audio-stopped": screen_audio_stopped,
                "data-preview-attached": screen_preview_attached,
                "data-audio-preview-attached": screen_audio_preview_attached,
                "data-video-published": screen_video_published,
                "data-audio-published": screen_audio_published,
            }
            div {
                id: "media-devices",
                hidden: true,
                "data-camera-count": camera_device_count,
                "data-microphone-count": microphone_device_count,
                "data-active-camera-id": active_camera_device_id,
                "data-active-microphone-id": active_microphone_device_id,
            }
            div {
                id: "media-operation-error",
                hidden: true,
                "data-kind": media_error_kind,
            }
            div {
                id: "data-packets",
                hidden: true,
                "data-observing": data_packet_observing,
                "data-closed": data_packet_stream_closed,
                "data-reliable-count": reliable_packet_count,
                "data-reliable-sender": reliable_packet_sender,
                "data-reliable-payload": reliable_packet_payload,
                "data-lossy-count": lossy_packet_count,
                "data-lossy-sender": lossy_packet_sender,
                "data-lossy-payload": lossy_packet_payload,
                "data-dropped": data_packet_dropped,
            }
            div {
                id: "data-streams",
                hidden: true,
                "data-text-handler-active": text_stream_handler.read().as_ref().is_some_and(TextStreamHandlerRegistration::is_active),
                "data-text-sender": text_stream_sender,
                "data-text-bytes": text_stream_bytes,
                "data-text-checksum": text_stream_checksum,
                "data-text-completed": text_stream_completed,
                "data-text-cancelled": text_stream_cancelled,
                "data-text-error": text_stream_error,
                "data-text-read-pending": text_stream_read_pending,
                "data-byte-handler-active": byte_stream_handler.read().as_ref().is_some_and(ByteStreamHandlerRegistration::is_active),
                "data-byte-sender": byte_stream_sender,
                "data-byte-bytes": byte_stream_bytes,
                "data-byte-checksum": byte_stream_checksum,
                "data-byte-completed": byte_stream_completed,
                "data-byte-cancelled": byte_stream_cancelled,
                "data-byte-error": byte_stream_error,
                "data-byte-read-pending": byte_stream_read_pending,
                "data-sent-text-bytes": sent_text_stream_bytes,
                "data-sent-text-writes": sent_text_stream_writes,
                "data-sent-text-completed": sent_text_stream_completed,
                "data-sent-byte-bytes": sent_byte_stream_bytes,
                "data-sent-byte-writes": sent_byte_stream_writes,
                "data-sent-byte-completed": sent_byte_stream_completed,
                "data-reads-paused": data_stream_reads_paused,
            }
            div {
                id: "chat-messages",
                hidden: true,
                "data-observing": chat_message_observing,
                "data-closed": chat_message_stream_closed,
                "data-count": chat_message_count,
                "data-contents": chat_message_contents,
                "data-senders": chat_message_senders,
                "data-ids": chat_message_ids,
                "data-timestamps": chat_message_timestamps,
                "data-edit-timestamps": chat_message_edit_timestamps,
                "data-dropped": chat_message_dropped,
                "data-sent-count": sent_chat_message_count,
                "data-send-error": chat_message_send_error,
            }
            div {
                id: "transcription-segments",
                hidden: true,
                "data-observing": transcription_observing,
                "data-closed": transcription_stream_closed,
                "data-count": transcription_segment_count,
                "data-ids": transcription_segment_ids,
                "data-participants": transcription_segment_participants,
                "data-track-publication-sids": transcription_segment_track_publication_sids,
                "data-texts": transcription_segment_texts,
                "data-start-times": transcription_segment_start_times,
                "data-end-times": transcription_segment_end_times,
                "data-languages": transcription_segment_languages,
                "data-finals": transcription_segment_finals,
                "data-first-received-times": transcription_segment_first_received_times,
                "data-last-received-times": transcription_segment_last_received_times,
                "data-dropped": transcription_dropped,
            }
            div {
                id: "participant-rpc",
                hidden: true,
                "data-echo-registered": rpc_echo_registration.read().as_ref().is_some_and(RpcMethodRegistration::is_active),
                "data-method-count": rpc_method_registrations.read().len(),
                "data-handler-count": rpc_handler_count,
                "data-handler-caller": rpc_handler_caller,
                "data-handler-request-id": rpc_handler_request_id,
                "data-handler-payload-kind": rpc_handler_payload_kind,
                "data-handler-payload": rpc_handler_payload,
                "data-delay-started": rpc_delay_started,
                "data-delay-completed": rpc_delay_completed,
                "data-delay-cancelled": rpc_delay_cancelled,
                "data-abandoned-count": rpc_abandoned_count,
                "data-response-kind": rpc_response_kind,
                "data-response-text": rpc_response_text,
                "data-response-bytes": rpc_response_bytes,
                "data-error-kind": rpc_error_kind,
                "data-error-code": rpc_error_code,
                "data-error-message": rpc_error_message,
                "data-error-data": rpc_error_data,
            }
            button {
                id: "invoke-text-rpc",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let operation = rpc_operation(
                        session.read().as_ref().cloned(),
                        "prototype.echo",
                        RpcPayload::Text("hello λ".into()),
                        None,
                    );
                    apply_rpc_operation(operation, rpc_result_signals)
                },
                "Invoke text Participant RPC"
            }
            button {
                id: "invoke-byte-rpc",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let operation = rpc_operation(
                        session.read().as_ref().cloned(),
                        "prototype.echo",
                        RpcPayload::Bytes(vec![0, 255, 128, 65]),
                        None,
                    );
                    apply_rpc_operation(operation, rpc_result_signals)
                },
                "Invoke byte Participant RPC"
            }
            button {
                id: "invoke-failing-rpc",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let operation = rpc_operation(
                        session.read().as_ref().cloned(),
                        "prototype.fail",
                        RpcPayload::Text("fail".into()),
                        None,
                    );
                    apply_rpc_operation(operation, rpc_result_signals)
                },
                "Invoke failing Participant RPC"
            }
            button {
                id: "invoke-timeout-rpc",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let operation = rpc_operation(
                        session.read().as_ref().cloned(),
                        "prototype.delay",
                        RpcPayload::Text("wait".into()),
                        Some(Duration::from_millis(50)),
                    );
                    apply_rpc_operation(operation, rpc_result_signals)
                },
                "Invoke timeout Participant RPC"
            }
            button {
                id: "abandon-rpc-invocation",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    drop(rpc_operation(
                        session.read().as_ref().cloned(),
                        "prototype.abandoned",
                        RpcPayload::Text("abandoned".into()),
                        None,
                    ));
                },
                "Abandon Participant RPC invocation"
            }
            button {
                id: "unregister-echo-rpc",
                hidden: true,
                r#type: "button",
                onclick: move |_| rpc_echo_registration.set(None),
                "Unregister echo Participant RPC"
            }
            button {
                id: "pause-data-stream-reads",
                hidden: true,
                r#type: "button",
                onclick: move |_| data_stream_reads_paused.set(true),
                "Pause incoming Data Stream reads"
            }
            button {
                id: "resume-data-stream-reads",
                hidden: true,
                r#type: "button",
                onclick: move |_| data_stream_reads_paused.set(false),
                "Resume incoming Data Stream reads"
            }
            button {
                id: "send-large-text-stream",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let current = session.read().as_ref().cloned();
                    async move {
                        let Some(current) = current else { return };
                        let Some(destination_identity) = current
                            .snapshot()
                            .remote_participants()
                            .keys()
                            .next()
                            .cloned()
                        else { return };
                        let chunk = text_stream_chunk();
                        let total_size = chunk.len() * LARGE_STREAM_CHUNKS;
                        sent_text_stream_bytes.set(0);
                        sent_text_stream_writes.set(0);
                        sent_text_stream_completed.set(false);
                        let result = async {
                            let writer = current
                                .local_participant()
                                .open_text_stream(
                                    TextStreamOptions::new(destination_identity, "large-text")
                                        .with_total_size(total_size as u64),
                                )
                                .await?;
                            for _ in 0..LARGE_STREAM_CHUNKS {
                                writer.write(chunk.clone()).await?;
                                sent_text_stream_bytes += chunk.len() as u64;
                                sent_text_stream_writes += 1;
                            }
                            writer.complete().await
                        }
                        .await;
                        match result {
                            Ok(()) => sent_text_stream_completed.set(true),
                            Err(stream_error) => {
                                media_error_kind.set(media_operation_error_kind(&stream_error).into());
                                error.set(stream_error.to_string());
                            }
                        }
                    }
                },
                "Send large text stream"
            }
            button {
                id: "send-chat-messages",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let participant = session
                        .read()
                        .as_ref()
                        .map(RoomSession::local_participant);
                    async move {
                        let Some(participant) = participant else { return };
                        sent_chat_message_count.set(0);
                        chat_message_send_error.set(String::new());
                        for content in ["first chat λ", "second chat", "third chat"] {
                            match participant.send_chat_message(content).await {
                                Ok(_) => sent_chat_message_count += 1,
                                Err(send_error) => {
                                    chat_message_send_error.set(send_error.to_string());
                                    media_error_kind.set(
                                        media_operation_error_kind(&send_error).into(),
                                    );
                                    error.set(send_error.to_string());
                                    break;
                                }
                            }
                        }
                    }
                },
                "Send chat messages"
            }
            button {
                id: "send-large-byte-stream",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let current = session.read().as_ref().cloned();
                    async move {
                        let Some(current) = current else { return };
                        let Some(destination_identity) = current
                            .snapshot()
                            .remote_participants()
                            .keys()
                            .next()
                            .cloned()
                        else { return };
                        let total_size = BYTE_STREAM_CHUNK_SIZE * LARGE_STREAM_CHUNKS;
                        sent_byte_stream_bytes.set(0);
                        sent_byte_stream_writes.set(0);
                        sent_byte_stream_completed.set(false);
                        let result = async {
                            let writer = current
                                .local_participant()
                                .open_byte_stream(
                                    ByteStreamOptions::new(destination_identity, "large-bytes")
                                        .with_total_size(total_size as u64),
                                )
                                .await?;
                            for index in 0..LARGE_STREAM_CHUNKS {
                                let chunk = byte_stream_chunk(index);
                                writer.write(chunk).await?;
                                sent_byte_stream_bytes += BYTE_STREAM_CHUNK_SIZE as u64;
                                sent_byte_stream_writes += 1;
                            }
                            writer.complete().await
                        }
                        .await;
                        match result {
                            Ok(()) => sent_byte_stream_completed.set(true),
                            Err(stream_error) => {
                                media_error_kind.set(media_operation_error_kind(&stream_error).into());
                                error.set(stream_error.to_string());
                            }
                        }
                    }
                },
                "Send large byte stream"
            }
            button {
                id: "cancel-incoming-text-stream",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    if let Some(abort) = text_stream_abort.write().take() {
                        abort.abort();
                        text_stream_read_pending.set(false);
                        text_stream_cancelled.set(true);
                    }
                },
                "Cancel incoming text stream"
            }
            button {
                id: "cancel-incoming-byte-stream",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    if let Some(abort) = byte_stream_abort.write().take() {
                        abort.abort();
                        byte_stream_read_pending.set(false);
                        byte_stream_cancelled.set(true);
                    }
                },
                "Cancel incoming byte stream"
            }
            button {
                id: "send-reliable-data-packet",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let operation = session.read().as_ref().map(|current| {
                        current.local_participant().send_data_packet(
                            vec![0, 255, 128, 65],
                            DataPacketDelivery::Reliable,
                        )
                    });
                    apply_local_participant_operation(operation, media_error_kind, error)
                },
                "Send reliable native data packet"
            }
            button {
                id: "send-lossy-data-packet",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let operation = session.read().as_ref().map(|current| {
                        current.local_participant().send_data_packet(
                            vec![7, 0, 254, 66],
                            DataPacketDelivery::Lossy,
                        )
                    });
                    apply_local_participant_operation(operation, media_error_kind, error)
                },
                "Send lossy native data packet"
            }
            button {
                id: "abandon-reliable-data-packet-send",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    if let Some(current) = session.read().as_ref() {
                        drop(current.local_participant().send_data_packet(
                            vec![9, 0, 253, 67],
                            DataPacketDelivery::Reliable,
                        ));
                    }
                },
                "Abandon reliable native data packet send"
            }
            if local_audio_track.read().is_some() {
                button {
                    id: "create-local-audio-analyser",
                    hidden: true,
                    r#type: "button",
                    onclick: move |_| {
                        let created = local_audio_track
                            .read()
                            .as_ref()
                            .map(LocalAudioTrack::create_analyser);
                        install_audio_analyser(
                            created,
                            local_audio_analyser,
                            local_audio_analysis,
                            error,
                        );
                    },
                    "Create Local Media Track audio analyser"
                }
                button {
                    id: "read-local-audio-analyser",
                    hidden: true,
                    r#type: "button",
                    onclick: move |_| {
                        let analysis = local_audio_analyser
                            .read()
                            .as_ref()
                            .map(AudioAnalysisObservation::read);
                        install_audio_analysis(analysis, local_audio_analysis, error);
                    },
                    "Read Local Media Track audio analyser"
                }
                button {
                    id: "close-local-audio-analyser",
                    hidden: true,
                    r#type: "button",
                    onclick: move |_| {
                        let analyser = local_audio_analyser.read().as_ref().cloned();
                        close_audio_analyser(analyser, local_audio_analyser, error)
                    },
                    "Close Local Media Track audio analyser"
                }
                button {
                    id: "drop-local-audio-analyser",
                    hidden: true,
                    r#type: "button",
                    onclick: move |_| local_audio_analyser.set(None),
                    "Drop Local Media Track audio analyser"
                }
                button {
                    id: "drop-local-audio-track",
                    hidden: true,
                    r#type: "button",
                    onclick: move |_| {
                        local_audio_publication.set(None);
                        local_audio_track.set(None);
                    },
                    "Drop Local Audio Track owner"
                }
            }
            if local_audio_publication.read().is_some() {
                button {
                    id: "unmute-local-audio-publication",
                    hidden: true,
                    r#type: "button",
                    onclick: move |_| {
                        let publication = local_audio_publication.read().as_ref().cloned();
                        async move {
                            let Some(publication) = publication else { return };
                            if let Err(unmute_error) = publication.set_muted(false).await {
                                error.set(unmute_error.to_string());
                            }
                        }
                    },
                    "Unmute Local Audio Track Publication"
                }
                button {
                    id: "republish-local-audio-track",
                    hidden: true,
                    r#type: "button",
                    onclick: move |_| {
                        let publication = local_audio_publication.read().as_ref().cloned();
                        let track = local_audio_track.read().as_ref().cloned();
                        let room = session.read().as_ref().cloned();
                        async move {
                            let (Some(publication), Some(track), Some(room)) =
                                (publication, track, room)
                            else {
                                return;
                            };
                            let unpublish = publication.unpublish();
                            let publish = track.publish(&room, microphone_publish_options());
                            let unpublish_result = unpublish.await;
                            let publish_result = publish.await;
                            match (unpublish_result, publish_result) {
                                (Ok(()), Ok(publication)) => {
                                    local_audio_publication.set(Some(publication));
                                }
                                (Err(unpublish_error), _) => {
                                    error.set(unpublish_error.to_string());
                                }
                                (_, Err(publish_error)) => {
                                    error.set(publish_error.to_string());
                                }
                            }
                        }
                    },
                    "Republish Local Audio Track"
                }
                button {
                    id: "replace-and-mute-old-audio-publication",
                    hidden: true,
                    r#type: "button",
                    onclick: move |_| {
                        let publication = local_audio_publication.read().as_ref().cloned();
                        let track = local_audio_track.read().as_ref().cloned();
                        let room = session.read().as_ref().cloned();
                        async move {
                            let (Some(publication), Some(track), Some(room)) =
                                (publication, track, room)
                            else {
                                return;
                            };
                            let unpublish = publication.unpublish();
                            let publish = track.publish(&room, microphone_publish_options());
                            let stale_mute = publication.set_muted(true);
                            let unpublish_result = unpublish.await;
                            let publish_result = publish.await;
                            let stale_mute_result = stale_mute.await;
                            match (unpublish_result, publish_result, stale_mute_result) {
                                (Ok(()), Ok(publication), Err(_)) => {
                                    local_audio_publication.set(Some(publication));
                                }
                                (Ok(()), Ok(_), Ok(())) => {
                                    error.set("stale publication muted its replacement".into());
                                }
                                (Err(unpublish_error), _, _) => {
                                    error.set(unpublish_error.to_string());
                                }
                                (_, Err(publish_error), _) => {
                                    error.set(publish_error.to_string());
                                }
                            }
                        }
                    },
                    "Replace and mute old Local Audio Track Publication"
                }
                button {
                    id: "abandon-local-audio-mute",
                    hidden: true,
                    r#type: "button",
                    onclick: move |_| {
                        if let Some(publication) = local_audio_publication.read().as_ref() {
                            drop(publication.set_muted(true));
                        }
                    },
                    "Abandon Local Audio Track Publication mute"
                }
            }
            if local_video_track.read().is_some() {
                button {
                    id: "retry-local-video-stop",
                    hidden: true,
                    r#type: "button",
                    onclick: move |_| {
                        let track = local_video_track.read().as_ref().cloned();
                        async move {
                            let Some(track) = track else { return };
                            if let Err(stop_error) = track.stop().await {
                                error.set(stop_error.to_string());
                            } else {
                                error.set(String::new());
                            }
                        }
                    },
                    "Retry Local Video Track stop"
                }
                button {
                    id: "drop-local-video-track",
                    hidden: true,
                    r#type: "button",
                    onclick: move |_| {
                        local_video_publication.set(None);
                        local_video_track.set(None);
                    },
                    "Drop Local Video Track owner"
                }
            }
            if local_video_attachment.read().is_some() {
                button {
                    id: "close-local-video-preview",
                    hidden: true,
                    r#type: "button",
                    onclick: move |_| {
                        let attachment = local_video_attachment.write().take();
                        if let Some(attachment) = attachment {
                            if let Err(close_error) = attachment.close() {
                                error.set(close_error.to_string());
                            }
                            local_video_attachment.set(Some(attachment));
                        }
                    },
                    "Close local video preview attachment"
                }
                button {
                    id: "drop-local-video-preview",
                    hidden: true,
                    r#type: "button",
                    onclick: move |_| {
                        local_video_attachment.set(None);
                    },
                    "Drop local video preview attachment"
                }
            }
            if local_video_publication.read().is_some() {
                button {
                    id: "unmute-local-video-publication",
                    hidden: true,
                    r#type: "button",
                    onclick: move |_| {
                        let publication = local_video_publication.read().as_ref().cloned();
                        async move {
                            let Some(publication) = publication else { return };
                            if let Err(unmute_error) = publication.set_muted(false).await {
                                error.set(unmute_error.to_string());
                            }
                        }
                    },
                    "Unmute Local Video Track Publication"
                }
            }
            button {
                id: "start-duration-limited-local-clip",
                hidden: true,
                r#type: "button",
                onclick: move |_| begin_local_clip(
                    runtime_capabilities,
                    local_video_track.read().as_ref().cloned(),
                    local_audio_track.read().as_ref().cloned(),
                    LOCAL_CLIP_MIME_PREFERENCES,
                    LocalClipLimits::Duration(Duration::from_millis(1_200)),
                    local_clip_signals,
                ),
                "Start duration-limited Local Clip Recording"
            }
            button {
                id: "start-size-limited-local-clip",
                hidden: true,
                r#type: "button",
                onclick: move |_| begin_local_clip(
                    runtime_capabilities,
                    local_video_track.read().as_ref().cloned(),
                    local_audio_track.read().as_ref().cloned(),
                    LOCAL_CLIP_MIME_PREFERENCES,
                    LocalClipLimits::Size(LOCAL_CLIP_ACCEPTANCE_SIZE_BYTES),
                    local_clip_signals,
                ),
                "Start size-limited Local Clip Recording"
            }
            button {
                id: "start-unsupported-mime-local-clip",
                hidden: true,
                r#type: "button",
                onclick: move |_| begin_local_clip(
                    runtime_capabilities,
                    local_video_track.read().as_ref().cloned(),
                    local_audio_track.read().as_ref().cloned(),
                    &["video/x-livekit-unsupported"],
                    LocalClipLimits::Duration(Duration::from_secs(1)),
                    local_clip_signals,
                ),
                "Start unsupported-MIME Local Clip Recording"
            }
            if local_clip_recording.read().is_some() {
                button {
                    id: "drop-local-clip-recording",
                    hidden: true,
                    r#type: "button",
                    onclick: move |_| {
                        if let Some(abort) = local_clip_completion_abort.write().take() {
                            abort.abort();
                        }
                        local_clip_recording.set(None);
                    },
                    "Drop Local Clip Recording owner"
                }
            }
            if local_clip_object_url.read().is_some() {
                button {
                    id: "drop-local-clip-object-url",
                    hidden: true,
                    r#type: "button",
                    onclick: move |_| local_clip_object_url.set(None),
                    "Drop Local Clip Recording object URL owner"
                }
            }
            if screen_capture.read().is_some() {
                button {
                    id: "drop-screen-capture",
                    hidden: true,
                    r#type: "button",
                    onclick: move |_| {
                        screen_video_publication.set(None);
                        screen_audio_publication.set(None);
                        screen_video_attachment.set(None);
                        screen_audio_attachment.set(None);
                        screen_capture.set(None);
                    },
                    "Drop Screen Capture owner"
                }
            }
            button {
                id: "abandon-room-join",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let client = runtime.read().as_ref().ok().cloned();
                    let room = room_name.read().trim().to_string();
                    let participant = identity.read().trim().to_string();
                    async move {
                        let Some(client) = client else { return };
                        let Ok(credentials) = request_credentials(&room, &participant).await else {
                            return;
                        };
                        let (abort, registration) = AbortHandle::new_pair();
                        wasm_bindgen_futures::spawn_local(async move {
                            let _ = Abortable::new(
                                client.join(credentials, RoomOptions::default()),
                                registration,
                            )
                            .await;
                        });
                        TimeoutFuture::new(50).await;
                        abort.abort();
                    }
                },
                "Abandon Room Session join"
            }
            button {
                id: "drop-unprovided-room-session",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let client = runtime.read().as_ref().ok().cloned();
                    let room = room_name.read().trim().to_string();
                    let participant = format!("{}-unprovided", identity.read().trim());
                    async move {
                        let Some(client) = client else { return };
                        let Ok(credentials) = request_credentials(&room, &participant).await else {
                            return;
                        };
                        if let Ok(unprovided) = client
                            .join(credentials, RoomOptions::default())
                            .await
                        {
                            drop(unprovided);
                        }
                    }
                },
                "Drop an unprovided Room Session"
            }
            button {
                id: "abandon-camera-capture",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let client = runtime.read().as_ref().ok().cloned();
                    async move {
                        let Some(client) = client else { return };
                        let (abort, registration) = AbortHandle::new_pair();
                        wasm_bindgen_futures::spawn_local(async move {
                            let _ = Abortable::new(client.create_camera_video_track(), registration).await;
                        });
                        TimeoutFuture::new(50).await;
                        abort.abort();
                    }
                },
                "Abandon camera capture"
            }
            button {
                id: "abandon-screen-capture",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let client = runtime.read().as_ref().ok().cloned();
                    async move {
                        let Some(client) = client else { return };
                        let (abort, registration) = AbortHandle::new_pair();
                        wasm_bindgen_futures::spawn_local(async move {
                            let options = ScreenCaptureOptions::default().with_system_audio(true);
                            let _ = Abortable::new(client.create_screen_capture(options), registration).await;
                        });
                        TimeoutFuture::new(50).await;
                        abort.abort();
                    }
                },
                "Abandon screen capture"
            }
            button {
                id: "refresh-room-session",
                hidden: true,
                r#type: "button",
                onclick: move |_| snapshot_refresh += 1,
                "Refresh Room Session snapshot"
            }
            button {
                id: "refresh-local-media-statistics",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let audio = local_audio_publication.read().as_ref().cloned();
                    let video = local_video_publication.read().as_ref().cloned();
                    async move {
                        if let Some(audio) = audio {
                            match audio.sender_statistics().await {
                                Ok(Some(statistics)) => {
                                    local_audio_statistics_timestamp.set(statistics.timestamp());
                                    local_audio_statistics_packets_sent.set(
                                        statistics.packets_sent().unwrap_or(0),
                                    );
                                    local_audio_statistics_bytes_sent.set(
                                        statistics.bytes_sent().unwrap_or(0),
                                    );
                                }
                                Ok(None) => {}
                                Err(statistics_error) => {
                                    error.set(statistics_error.to_string());
                                    return;
                                }
                            }
                        }
                        if let Some(video) = video {
                            match video.sender_statistics().await {
                                Ok(statistics) => {
                                    local_video_statistics_frames_sent.set(
                                        statistics.iter().filter_map(|sample| sample.frames_sent()).sum(),
                                    );
                                    local_video_statistics_layers.set(statistics.len());
                                }
                                Err(statistics_error) => error.set(statistics_error.to_string()),
                            }
                        }
                    }
                },
                "Refresh local media statistics"
            }
            button {
                id: "abandon-participant-updates",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let current = session.read().as_ref().cloned();
                    let metadata = participant_metadata_input.read().clone();
                    let attributes = BTreeMap::from([(
                        participant_attribute_key.read().clone(),
                        participant_attribute_value.read().clone(),
                    )]);
                    if let Some(current) = current {
                        let participant = current.local_participant();
                        drop(participant.set_metadata(metadata));
                        drop(participant.update_attributes(attributes));
                    }
                },
                "Abandon Local Participant updates"
            }
            button {
                id: "observe-room-events",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let current = session.read().as_ref().cloned();
                    async move {
                        let Some(current) = current else { return };
                        event_types.write().clear();
                        let mut events = current.subscribe_events(8);
                        event_observing.set(true);
                        wasm_bindgen_futures::spawn_local(async move {
                            while let Some(event) = events.next().await {
                                event_count += 1;
                                event_revision.set(event.revision());
                                event_snapshot_revision.set(current.snapshot().revision());
                                event_quality.set(String::new());
                                match event.kind() {
                                    RoomEventKind::LocalParticipantChanged => {
                                        event_type.set("localParticipantChanged".into());
                                        event_identity.set(String::new());
                                    }
                                    RoomEventKind::RemoteParticipantJoined { identity } => {
                                        event_type.set("remoteParticipantJoined".into());
                                        event_identity.set(identity.clone());
                                    }
                                    RoomEventKind::RemoteParticipantChanged { identity } => {
                                        event_type.set("remoteParticipantChanged".into());
                                        event_identity.set(identity.clone());
                                    }
                                    RoomEventKind::ParticipantConnectionQualityChanged {
                                        participant_identity,
                                        quality,
                                    } => {
                                        event_type.set("participantConnectionQualityChanged".into());
                                        event_identity.set(participant_identity.clone());
                                        event_quality.set(connection_quality_label(*quality).into());
                                    }
                                    RoomEventKind::ActiveSpeakersChanged => {
                                        event_type.set("activeSpeakersChanged".into());
                                        event_identity.set(String::new());
                                    }
                                    RoomEventKind::RemoteParticipantLeft { identity } => {
                                        event_type.set("remoteParticipantLeft".into());
                                        event_identity.set(identity.clone());
                                    }
                                    RoomEventKind::RemoteTrackPublicationsChanged {
                                        participant_identity,
                                    } => {
                                        event_type.set("remoteTrackPublicationsChanged".into());
                                        event_identity.set(participant_identity.clone());
                                    }
                                    RoomEventKind::RemoteDataTracksChanged {
                                        participant_identity,
                                    } => {
                                        event_type.set("remoteDataTracksChanged".into());
                                        event_identity.set(participant_identity.clone());
                                    }
                                    RoomEventKind::RoomMetadataChanged => {
                                        event_type.set("roomMetadataChanged".into());
                                        event_identity.set(String::new());
                                    }
                                    _ => {
                                        event_type.set("other".into());
                                        event_identity.set(String::new());
                                    }
                                }
                                event_types.write().push(event_type.read().clone());
                            }
                        });
                    }
                },
                "Observe Room Events"
            }
            button {
                id: "hold-first-remote-handles",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let current = session.read().as_ref().cloned();
                    let handles = current.and_then(|current| {
                        let snapshot = current.snapshot();
                        let (identity, publication_sid) = snapshot
                            .remote_participants()
                            .iter()
                            .find_map(|(identity, participant)| {
                                participant
                                    .track_publications()
                                    .iter()
                                    .find(|(_, publication)| publication.media_available())
                                    .map(|(sid, _)| (identity.clone(), sid.clone()))
                            })?;
                        let participant = current.remote_participant(&identity)?;
                        let publication = participant.track_publication(&publication_sid)?;
                        let media = publication.media_track().ok().flatten()?;
                        Some((participant, publication, media))
                    });
                    if let Some((participant, publication, media)) = handles {
                        held_remote_participant.set(Some(participant));
                        held_remote_publication.set(Some(publication));
                        held_remote_media.set(Some(media));
                        held_remote_mutation.set(String::new());
                    }
                },
                "Hold first Remote Participant, Track Publication, and Media Track"
            }
            button {
                id: "mutate-held-remote-publication",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let operation = held_remote_publication
                        .read()
                        .as_ref()
                        .map(|publication| publication.set_subscribed(false));
                    async move {
                        let Some(operation) = operation else { return };
                        match operation.await {
                            Err(LiveKitError::InvalidState { .. }) => {
                                held_remote_mutation.set("terminal".into());
                            }
                            Err(mutation_error) => {
                                error.set(mutation_error.to_string());
                            }
                            Ok(()) => {
                                held_remote_mutation.set("retargeted".into());
                                error.set("stale remote publication controlled its replacement".into());
                            }
                        }
                    }
                },
                "Mutate held Remote Track Publication"
            }
            button {
                id: "abandon-room-leave",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    if let Some(current) = session.read().as_ref().cloned() {
                        drop(current.leave());
                    }
                },
                "Abandon Room Session leave"
            }

            header { class: "masthead",
                div {
                    p { class: "eyebrow", "LIVEKIT BROWSER CORE" }
                    h1 { "Dioxus meets LiveKit" }
                    p { class: "lede",
                        "Join a Room Session without opening a camera or microphone. Media remains an explicit, separate lifecycle."
                    }
                }
                div { class: "stack-badge", "RUST / WASM / WEBRTC" }
            }

            section { class: "status-rail", "aria-label": "Live connection status",
                StatusItem {
                    label: "Connection",
                    value: connection_text.clone(),
                    active: connected,
                    test_id: "connection-state",
                }
                StatusItem {
                    label: "Camera",
                    value: if camera_active { String::from("on") } else { String::from("off") },
                    active: camera_active,
                    test_id: "camera-state",
                }
                StatusItem {
                    label: "Microphone",
                    value: if microphone_active { String::from("on") } else { String::from("off") },
                    active: microphone_active,
                    test_id: "microphone-state",
                }
                StatusItem {
                    label: "Participants",
                    value: participant_count.to_string(),
                    active: connected,
                    test_id: "participant-count",
                }
            }

            section { class: "demo-overview", "aria-label": "Demo readiness and lifecycle",
                article {
                    id: "capability-panel",
                    class: "demo-panel capability-panel",
                    "data-ready": runtime_ready,
                    div { class: "demo-panel-heading",
                        div {
                            span { class: "card-kicker", "BROWSER CONTRACT" }
                            h2 { "Capabilities before permission" }
                        }
                        span { class: "count-chip", "SDK {runtime_sdk_version}" }
                    }
                    p { class: "demo-panel-copy",
                        "Detected without opening a device, starting playback, or joining a Room Session."
                    }
                    div { class: "capability-grid",
                        CapabilityItem {
                            label: "WebRTC",
                            supported: runtime_capabilities.map(BrowserCapabilities::web_rtc).unwrap_or(false),
                        }
                        CapabilityItem {
                            label: "Secure context",
                            supported: runtime_capabilities.map(BrowserCapabilities::secure_context).unwrap_or(false),
                        }
                        CapabilityItem {
                            label: "Camera + microphone",
                            supported: runtime_capabilities.map(BrowserCapabilities::user_media).unwrap_or(false),
                        }
                        CapabilityItem {
                            label: "Display capture",
                            supported: runtime_capabilities.map(BrowserCapabilities::display_capture).unwrap_or(false),
                        }
                        CapabilityItem {
                            label: "Audio playback",
                            supported: runtime_capabilities.map(BrowserCapabilities::audio_playback).unwrap_or(false),
                        }
                        CapabilityItem {
                            label: "Audio output selection",
                            supported: runtime_capabilities.map(BrowserCapabilities::audio_output_selection).unwrap_or(false),
                        }
                        CapabilityItem {
                            label: "Transcription",
                            supported: runtime_capabilities.map(BrowserCapabilities::transcription).unwrap_or(false),
                        }
                        CapabilityItem {
                            label: "Local Clip Recording",
                            supported: local_clip_supported,
                        }
                    }
                }
                article {
                    id: "lifecycle-guide",
                    class: "demo-panel lifecycle-panel",
                    div { class: "demo-panel-heading",
                        div {
                            span { class: "card-kicker", "GUIDED RUN" }
                            h2 { "One explicit lifecycle" }
                        }
                        span { class: "count-chip", "11 steps" }
                    }
                    ol { class: "lifecycle-list",
                        GuideItem { step: "runtime", label: "Runtime checked", complete: runtime_ready }
                        GuideItem { step: "joined", label: "Join without capture", complete: session_handle.is_some() }
                        GuideItem { step: "remote-participant", label: "Observe a Remote Participant", complete: demo_progress_snapshot.remote_participant_seen }
                        GuideItem { step: "capture", label: "Capture a Local Media Track", complete: demo_progress_snapshot.local_media_captured }
                        GuideItem { step: "publish", label: "Create a Track Publication", complete: demo_progress_snapshot.local_media_published }
                        GuideItem { step: "chat", label: "Exchange a Chat Message", complete: demo_progress_snapshot.chat_message_exchanged }
                        GuideItem { step: "record", label: "Create a Local Clip Recording", complete: demo_progress_snapshot.local_clip_created }
                        GuideItem { step: "artifact", label: "Download or revoke its URL", complete: demo_progress_snapshot.local_clip_artifact_released }
                        GuideItem { step: "unpublish", label: "Unpublish; keep capture", complete: demo_progress_snapshot.local_media_unpublished }
                        GuideItem { step: "stop", label: "Stop the Local Media Track", complete: demo_progress_snapshot.local_media_stopped }
                        GuideItem { step: "leave", label: "Leave deterministically", complete: terminal }
                    }
                }
            }

            if !runtime_error.is_empty() {
                aside { id: "browser-runtime-error", class: "notice error", role: "alert",
                    "{runtime_error}"
                }
            }
            if !error.read().is_empty() {
                aside { class: "notice error", role: "alert", "{error}" }
            }

            section { class: "workspace",
                div { class: "media-column",
                    article { class: "video-card local-card",
                        div { class: "card-heading",
                            div {
                                span { class: "card-kicker", "LOCAL PARTICIPANT" }
                                h2 {
                                    if participant_identity.is_empty() { "Not joined" } else { "{participant_identity}" }
                                }
                            }
                            span { class: if connected { "live-chip active" } else { "live-chip" },
                                if connected { "JOINED" } else { "OFFLINE" }
                            }
                        }
                        div { class: "video-frame",
                            video {
                                id: "local-video",
                                autoplay: true,
                                muted: true,
                                "playsinline": true,
                                "aria-label": "Local camera preview",
                                onmounted: move |event| {
                                    use wasm_bindgen::JsCast;

                                    let video = event
                                        .data()
                                        .downcast::<web_sys::Element>()
                                        .cloned()
                                        .and_then(|element| element.dyn_into().ok());
                                    local_video_element.set(video);
                                },
                            }
                            if !preview_attached {
                                div { class: "video-placeholder",
                                    span { class: "aperture", "LK" }
                                    p { "Camera preview is private until explicitly published" }
                                }
                            }
                        }
                    }
                    article { class: "video-card screen-card",
                        div { class: "card-heading",
                            div {
                                span { class: "card-kicker", "DISPLAY CAPTURE" }
                                h2 { "Selected screen" }
                            }
                            span { class: if screen_video_active { "live-chip active" } else { "live-chip" },
                                if screen_video_active { "SHARING" } else { "PRIVATE" }
                            }
                        }
                        div { class: "video-frame screen-frame",
                            video {
                                id: "screen-video",
                                autoplay: true,
                                muted: true,
                                "playsinline": true,
                                "aria-label": "Local screen preview",
                                onmounted: move |event| {
                                    use wasm_bindgen::JsCast;

                                    let video = event
                                        .data()
                                        .downcast::<web_sys::Element>()
                                        .cloned()
                                        .and_then(|element| element.dyn_into().ok());
                                    screen_video_element.set(video);
                                },
                            }
                            if !screen_preview_attached {
                                div { class: "video-placeholder",
                                    span { class: "aperture", "DS" }
                                    p { "Display preview stays local until explicitly published" }
                                }
                            }
                        }
                        if screen_audio_present {
                            audio {
                                id: "screen-audio",
                                class: "screen-audio-preview",
                                controls: true,
                                "aria-label": "Local system audio preview",
                                onmounted: move |event| {
                                    use wasm_bindgen::JsCast;

                                    let element = event
                                        .data()
                                        .downcast::<web_sys::Element>()
                                        .cloned()
                                        .and_then(|element| element.dyn_into().ok());
                                    let track = screen_capture
                                        .read()
                                        .as_ref()
                                        .and_then(ScreenCapture::system_audio)
                                        .cloned();
                                    if let (Some(element), Some(track)) = (element, track) {
                                        match track.attach(&element) {
                                            Ok(attachment) => screen_audio_attachment.set(Some(attachment)),
                                            Err(attach_error) => error.set(attach_error.to_string()),
                                        }
                                    }
                                },
                            }
                        }
                    }
                    article { class: "remote-card",
                        div { class: "card-heading compact",
                            div {
                                span { class: "card-kicker", "ROOM FEED" }
                                h2 { "Remote participants" }
                            }
                            span { class: "count-chip", "{remote_participant_count} remote" }
                        }
                        div { id: "remote-media", class: "remote-grid",
                            if let Some(snapshot) = snapshot.as_ref() {
                                for (remote_identity, participant) in snapshot.remote_participants() {
                                    for (publication_sid, publication) in participant.track_publications() {
                                        if publication.kind() == TrackKind::Video {
                                            RemoteVideoPublicationNode {
                                                key: "{remote_identity}:{publication_sid}",
                                                session,
                                                participant_identity: remote_identity.clone(),
                                                publication_sid: publication_sid.clone(),
                                                subscribed: publication.subscribed(),
                                                media_available: publication.media_available(),
                                                refresh: snapshot_refresh,
                                                error,
                                            }
                                        } else if publication.kind() == TrackKind::Audio {
                                            RemoteAudioPublicationNode {
                                                key: "{remote_identity}:{publication_sid}",
                                                session,
                                                participant_identity: remote_identity.clone(),
                                                publication_sid: publication_sid.clone(),
                                                subscribed: publication.subscribed(),
                                                media_available: publication.media_available(),
                                                refresh: snapshot_refresh,
                                                error,
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if remote_participant_count == 0 {
                            p { class: "empty-room", "Use Open second participant to prefill this room in another tab, then join there." }
                        }
                    }
                    article { id: "demo-chat", class: "chat-card",
                        div { class: "card-heading compact",
                            div {
                                span { class: "card-kicker", "DEDICATED DELIVERY" }
                                h2 { "Chat Messages" }
                            }
                            span { class: "count-chip", "{chat_message_count} messages" }
                        }
                        div { class: "chat-log", "aria-live": "polite",
                            if visible_chat_messages.is_empty() {
                                p { class: "chat-empty",
                                    if connected {
                                        "Chat Messages stay independent from the Room Event queue."
                                    } else {
                                        "Join a Room Session to exchange typed Chat Messages."
                                    }
                                }
                            }
                            for (index, message) in visible_chat_messages.iter().enumerate() {
                                div {
                                    key: "{message.id}:{index}",
                                    class: if message.sender_identity == participant_identity { "chat-message local" } else { "chat-message" },
                                    span { class: "chat-sender",
                                        if message.sender_identity.is_empty() { "Unknown participant" } else { "{message.sender_identity}" }
                                    }
                                    p { "{message.content}" }
                                    if message.edit_timestamp_millis.is_some() {
                                        small { "edited" }
                                    }
                                }
                            }
                        }
                        div { class: "chat-composer",
                            label { r#for: "chat-message-input", "Message" }
                            div {
                                input {
                                    id: "chat-message-input",
                                    value: "{chat_message_input}",
                                    disabled: !connected || *chat_message_sending.read(),
                                    maxlength: 2_000,
                                    autocomplete: "off",
                                    placeholder: "Send a typed string payload",
                                    oninput: move |event| chat_message_input.set(event.value()),
                                }
                                button {
                                    id: "send-chat-message",
                                    class: "primary-button",
                                    r#type: "button",
                                    disabled: !connected
                                        || *chat_message_sending.read()
                                        || chat_message_input.read().trim().is_empty(),
                                    onclick: move |_| {
                                        let content = chat_message_input.read().trim().to_string();
                                        let participant = session
                                            .read()
                                            .as_ref()
                                            .map(RoomSession::local_participant);
                                        async move {
                                            let Some(participant) = participant else { return };
                                            chat_message_sending.set(true);
                                            chat_message_send_error.set(String::new());
                                            match participant.send_chat_message(content).await {
                                                Ok(_) => {
                                                    sent_chat_message_count += 1;
                                                    chat_message_input.set(String::new());
                                                }
                                                Err(send_error) => {
                                                    chat_message_send_error.set(send_error.to_string());
                                                    media_error_kind.set(
                                                        media_operation_error_kind(&send_error).into(),
                                                    );
                                                    error.set(send_error.to_string());
                                                }
                                            }
                                            chat_message_sending.set(false);
                                        }
                                    },
                                    if *chat_message_sending.read() { "Sending..." } else { "Send" }
                                }
                            }
                            if !chat_message_send_error.read().is_empty() {
                                small { class: "chat-error", "{chat_message_send_error}" }
                            } else if *chat_message_dropped.read() > 0 {
                                small { class: "chat-error", "Recovered after dropping {chat_message_dropped} buffered messages" }
                            }
                        }
                    }
                }

                aside { class: "control-panel",
                    div { class: "panel-heading",
                        span { "ROOM SESSION" }
                        small { "Credentials stay application-owned" }
                    }
                    label { class: "field",
                        span { "Room name" }
                        input {
                            value: "{room_name}",
                            disabled: connected || busy,
                            autocomplete: "off",
                            oninput: move |event| room_name.set(event.value()),
                        }
                    }
                    label { class: "field",
                        span { "Your identity" }
                        input {
                            value: "{identity}",
                            disabled: connected || busy,
                            autocomplete: "off",
                            oninput: move |event| identity.set(event.value()),
                        }
                    }
                    if !second_participant_href.is_empty() {
                        a {
                            id: "open-second-participant",
                            class: "secondary-button second-participant-link",
                            href: second_participant_href,
                            target: "_blank",
                            rel: "noopener",
                            "Open second participant"
                        }
                        p { class: "help-copy second-participant-help",
                            "Prefills this room with a unique identity; joining still captures no media."
                        }
                    }

                    if !connected {
                        button {
                            class: "primary-button",
                            r#type: "button",
                            disabled: !runtime_ready || busy || room_name.read().trim().is_empty() || identity.read().trim().is_empty(),
                            onclick: move |_| {
                                let client = runtime.read().as_ref().ok().cloned();
                                let room = room_name.read().trim().to_string();
                                let participant = identity.read().trim().to_string();
                                async move {
                                    let Some(client) = client else { return };
                                    connection.set(RoomConnectionState::Connecting);
                                    error.set(String::new());
                                    let result = async {
                                        let credentials = request_credentials(&room, &participant).await?;
                                        client
                                            .join(credentials, RoomOptions::default())
                                            .await
                                            .map_err(|error| error.to_string())
                                    }
                                    .await;
                                    match result {
                                        Ok(joined) => {
                                            let text_registration = joined.register_text_stream_handler(
                                                "large-text",
                                                move |mut incoming| {
                                                    if let Some(abort) = text_stream_abort.write().take() {
                                                        abort.abort();
                                                    }
                                                    text_stream_sender.set(
                                                        incoming
                                                            .info()
                                                            .sender_identity()
                                                            .unwrap_or_default()
                                                            .to_string(),
                                                    );
                                                    text_stream_bytes.set(0);
                                                    text_stream_checksum.set(0);
                                                    text_stream_completed.set(false);
                                                    text_stream_cancelled.set(false);
                                                    text_stream_error.set(String::new());
                                                    let (abort, registration) = AbortHandle::new_pair();
                                                    wasm_bindgen_futures::spawn_local(async move {
                                                        let receive = async move {
                                                            loop {
                                                                while *data_stream_reads_paused.read() {
                                                                    TimeoutFuture::new(10).await;
                                                                }
                                                                text_stream_read_pending.set(true);
                                                                let next = incoming.next().await;
                                                                text_stream_read_pending.set(false);
                                                                let Some(item) = next else {
                                                                    break;
                                                                };
                                                                match item {
                                                                    Ok(chunk) => {
                                                                        let bytes = chunk.as_bytes();
                                                                        text_stream_bytes += bytes.len() as u64;
                                                                        text_stream_checksum += payload_checksum(bytes);
                                                                    }
                                                                    Err(stream_error) => {
                                                                        text_stream_error.set(stream_error.to_string());
                                                                        return;
                                                                    }
                                                                }
                                                            }
                                                            text_stream_completed.set(true);
                                                        };
                                                        let _ = Abortable::new(receive, registration).await;
                                                    });
                                                    text_stream_abort.set(Some(abort));
                                                },
                                            );
                                            let byte_registration = joined.register_byte_stream_handler(
                                                "large-bytes",
                                                move |mut incoming| {
                                                    if let Some(abort) = byte_stream_abort.write().take() {
                                                        abort.abort();
                                                    }
                                                    byte_stream_sender.set(
                                                        incoming
                                                            .info()
                                                            .sender_identity()
                                                            .unwrap_or_default()
                                                            .to_string(),
                                                    );
                                                    byte_stream_bytes.set(0);
                                                    byte_stream_checksum.set(0);
                                                    byte_stream_completed.set(false);
                                                    byte_stream_cancelled.set(false);
                                                    byte_stream_error.set(String::new());
                                                    let (abort, registration) = AbortHandle::new_pair();
                                                    wasm_bindgen_futures::spawn_local(async move {
                                                        let receive = async move {
                                                            loop {
                                                                while *data_stream_reads_paused.read() {
                                                                    TimeoutFuture::new(10).await;
                                                                }
                                                                byte_stream_read_pending.set(true);
                                                                let next = incoming.next().await;
                                                                byte_stream_read_pending.set(false);
                                                                let Some(item) = next else {
                                                                    break;
                                                                };
                                                                match item {
                                                                    Ok(chunk) => {
                                                                        byte_stream_bytes += chunk.len() as u64;
                                                                        byte_stream_checksum += payload_checksum(&chunk);
                                                                    }
                                                                    Err(stream_error) => {
                                                                        byte_stream_error.set(stream_error.to_string());
                                                                        return;
                                                                    }
                                                                }
                                                            }
                                                            byte_stream_completed.set(true);
                                                        };
                                                        let _ = Abortable::new(receive, registration).await;
                                                    });
                                                    byte_stream_abort.set(Some(abort));
                                                },
                                            );
                                            let (text_registration, byte_registration) =
                                                match (text_registration, byte_registration) {
                                                    (Ok(text), Ok(bytes)) => (text, bytes),
                                                    (Err(registration_error), _)
                                                    | (_, Err(registration_error)) => {
                                                        let _ = joined.leave().await;
                                                        connection.set(RoomConnectionState::Disconnected);
                                                        error.set(registration_error.to_string());
                                                        return;
                                                    }
                                            };
                                            text_stream_handler.set(Some(text_registration));
                                            byte_stream_handler.set(Some(byte_registration));
                                            let rpc_registrations = (|| -> Result<_, LiveKitError> {
                                                let echo = joined.register_rpc_method(
                                                    "prototype.echo",
                                                    move |invocation| {
                                                        record_rpc_invocation(
                                                            &invocation,
                                                            rpc_handler_count,
                                                            rpc_handler_caller,
                                                            rpc_handler_request_id,
                                                            rpc_handler_payload_kind,
                                                            rpc_handler_payload,
                                                        );
                                                        let response = invocation.payload().clone();
                                                        async move {
                                                            Ok::<RpcPayload, RpcApplicationError>(response)
                                                        }
                                                    },
                                                )?;
                                                let failure = joined.register_rpc_method(
                                                    "prototype.fail",
                                                    move |invocation| {
                                                        record_rpc_invocation(
                                                            &invocation,
                                                            rpc_handler_count,
                                                            rpc_handler_caller,
                                                            rpc_handler_request_id,
                                                            rpc_handler_payload_kind,
                                                            rpc_handler_payload,
                                                        );
                                                        async move {
                                                            Err(RpcApplicationError::new(
                                                                2001,
                                                                "remote application failure",
                                                            )
                                                            .with_data("failure-details"))
                                                        }
                                                    },
                                                )?;
                                                let delay = joined.register_rpc_method_with_capacity(
                                                    "prototype.delay",
                                                    1,
                                                    move |invocation| {
                                                        record_rpc_invocation(
                                                            &invocation,
                                                            rpc_handler_count,
                                                            rpc_handler_caller,
                                                            rpc_handler_request_id,
                                                            rpc_handler_payload_kind,
                                                            rpc_handler_payload,
                                                        );
                                                        rpc_delay_started.set(true);
                                                        rpc_delay_completed.set(false);
                                                        rpc_delay_cancelled.set(false);
                                                        async move {
                                                            let mut cancellation = RpcDelayCancellation {
                                                                cancelled: rpc_delay_cancelled,
                                                                completed: false,
                                                            };
                                                            TimeoutFuture::new(10_000).await;
                                                            cancellation.complete();
                                                            rpc_delay_completed.set(true);
                                                            Ok::<RpcPayload, RpcApplicationError>(
                                                                RpcPayload::Text("late".into()),
                                                            )
                                                        }
                                                    },
                                                )?;
                                                let abandoned = joined.register_rpc_method(
                                                    "prototype.abandoned",
                                                    move |invocation| {
                                                        record_rpc_invocation(
                                                            &invocation,
                                                            rpc_handler_count,
                                                            rpc_handler_caller,
                                                            rpc_handler_request_id,
                                                            rpc_handler_payload_kind,
                                                            rpc_handler_payload,
                                                        );
                                                        rpc_abandoned_count += 1;
                                                        async move {
                                                            Ok::<RpcPayload, RpcApplicationError>(
                                                                RpcPayload::Text("observed".into()),
                                                            )
                                                        }
                                                    },
                                                )?;
                                                Ok((echo, vec![failure, delay, abandoned]))
                                            })();
                                            let (echo_registration, method_registrations) =
                                                match rpc_registrations {
                                                    Ok(registrations) => registrations,
                                                    Err(registration_error) => {
                                                        text_stream_handler.set(None);
                                                        byte_stream_handler.set(None);
                                                        let _ = joined.leave().await;
                                                        connection.set(RoomConnectionState::Disconnected);
                                                        error.set(registration_error.to_string());
                                                        return;
                                                    }
                                            };
                                            rpc_echo_registration.set(Some(echo_registration));
                                            rpc_method_registrations.set(method_registrations);
                                            if let Some(abort) = data_packet_observer_abort.write().take() {
                                                abort.abort();
                                            }
                                            reliable_packet_count.set(0);
                                            reliable_packet_sender.set(String::new());
                                            reliable_packet_payload.set(String::new());
                                            lossy_packet_count.set(0);
                                            lossy_packet_sender.set(String::new());
                                            lossy_packet_payload.set(String::new());
                                            data_packet_dropped.set(0);
                                            data_packet_observing.set(true);
                                            data_packet_stream_closed.set(false);
                                            let reliable = joined.subscribe_data_packets(
                                                DataPacketDelivery::Reliable,
                                                8,
                                            );
                                            let lossy = joined.subscribe_data_packets(
                                                DataPacketDelivery::Lossy,
                                                8,
                                            );
                                            let mut packets = futures_util::stream::select(reliable, lossy);
                                            let (abort, registration) = AbortHandle::new_pair();
                                            wasm_bindgen_futures::spawn_local(async move {
                                                let observe = async move {
                                                    while let Some(item) = packets.next().await {
                                                        match item {
                                                            DataPacketStreamItem::Packet(packet) => {
                                                                let payload = packet
                                                                    .payload()
                                                                    .iter()
                                                                    .map(u8::to_string)
                                                                    .collect::<Vec<_>>()
                                                                    .join(",");
                                                                let sender = packet
                                                                    .sender_identity()
                                                                    .unwrap_or_default()
                                                                    .to_string();
                                                                match packet.delivery() {
                                                                    DataPacketDelivery::Reliable => {
                                                                        reliable_packet_count += 1;
                                                                        reliable_packet_sender.set(sender);
                                                                        reliable_packet_payload.set(payload);
                                                                    }
                                                                    DataPacketDelivery::Lossy => {
                                                                        lossy_packet_count += 1;
                                                                        lossy_packet_sender.set(sender);
                                                                        lossy_packet_payload.set(payload);
                                                                    }
                                                                    _ => {}
                                                                }
                                                            }
                                                            DataPacketStreamItem::Lagged { dropped } => {
                                                                data_packet_dropped += dropped as u64;
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                    data_packet_observing.set(false);
                                                    data_packet_stream_closed.set(true);
                                                };
                                                let _ = Abortable::new(observe, registration).await;
                                            });
                                            data_packet_observer_abort.set(Some(abort));
                                            observed_chat_messages.write().clear();
                                            chat_message_dropped.set(0);
                                            sent_chat_message_count.set(0);
                                            chat_message_send_error.set(String::new());
                                            chat_message_observing.set(true);
                                            chat_message_stream_closed.set(false);
                                            let mut chat_messages = joined.subscribe_chat_messages(8);
                                            wasm_bindgen_futures::spawn_local(async move {
                                                while let Some(item) = chat_messages.next().await {
                                                    match item {
                                                        ChatMessageStreamItem::Message(message) => {
                                                            let timestamp = message
                                                                .timestamp()
                                                                .duration_since(UNIX_EPOCH)
                                                                .unwrap_or_default()
                                                                .as_millis();
                                                            let edit_timestamp = message
                                                                .edit_timestamp()
                                                                .and_then(|timestamp| {
                                                                    timestamp
                                                                        .duration_since(UNIX_EPOCH)
                                                                        .ok()
                                                                })
                                                                .map(|timestamp| {
                                                                    timestamp.as_millis()
                                                                });
                                                            observed_chat_messages.write().push(
                                                                ObservedChatMessage {
                                                                    id: message.id().into(),
                                                                    content: message.message().into(),
                                                                    sender_identity: message
                                                                        .sender_identity()
                                                                        .unwrap_or_default()
                                                                        .into(),
                                                                    timestamp_millis: timestamp,
                                                                    edit_timestamp_millis: edit_timestamp,
                                                                },
                                                            );
                                                        }
                                                        ChatMessageStreamItem::Lagged { dropped } => {
                                                            chat_message_dropped += dropped as u64;
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                                chat_message_observing.set(false);
                                                chat_message_stream_closed.set(true);
                                            });
                                            observed_transcription_segments.write().clear();
                                            transcription_dropped.set(0);
                                            transcription_observing.set(true);
                                            transcription_stream_closed.set(false);
                                            let mut transcriptions =
                                                joined.subscribe_transcription_segments(8);
                                            wasm_bindgen_futures::spawn_local(async move {
                                                while let Some(item) = transcriptions.next().await {
                                                    match item {
                                                        TranscriptionSegmentStreamItem::Segment(
                                                            segment,
                                                        ) => {
                                                            let first_received_at_millis = segment
                                                                .first_received_at()
                                                                .duration_since(UNIX_EPOCH)
                                                                .unwrap_or_default()
                                                                .as_millis();
                                                            let last_received_at_millis = segment
                                                                .last_received_at()
                                                                .duration_since(UNIX_EPOCH)
                                                                .unwrap_or_default()
                                                                .as_millis();
                                                            observed_transcription_segments
                                                                .write()
                                                                .push(ObservedTranscriptionSegment {
                                                                    id: segment.id().into(),
                                                                    participant_identity: segment
                                                                        .participant_identity()
                                                                        .unwrap_or_default()
                                                                        .into(),
                                                                    track_publication_sid: segment
                                                                        .track_publication_sid()
                                                                        .unwrap_or_default()
                                                                        .into(),
                                                                    text: segment.text().into(),
                                                                    start_time: segment.start_time(),
                                                                    end_time: segment.end_time(),
                                                                    language: segment.language().into(),
                                                                    is_final: segment.is_final(),
                                                                    first_received_at_millis,
                                                                    last_received_at_millis,
                                                                });
                                                        }
                                                        TranscriptionSegmentStreamItem::Lagged {
                                                            dropped,
                                                        } => {
                                                            transcription_dropped += dropped as u64;
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                                transcription_observing.set(false);
                                                transcription_stream_closed.set(true);
                                            });
                                            connection.set(joined.snapshot().connection());
                                            publisher_permissions_state.set("allParticipants".into());
                                            allowed_participant_identity.set(String::new());
                                            #[cfg(feature = "raw-js-contract")]
                                            {
                                                let contract_ready = joined
                                                    .raw_js_room()
                                                    .ok()
                                                    .and_then(|room| {
                                                        js_sys::Reflect::get(
                                                            &room,
                                                            &JsValue::from_str("localParticipant"),
                                                        )
                                                        .ok()
                                                    })
                                                    .zip(
                                                        joined
                                                            .local_participant()
                                                            .raw_js_participant()
                                                            .ok(),
                                                    )
                                                    .is_some_and(|(room_participant, raw_participant)| {
                                                        room_participant == raw_participant
                                                    });
                                                raw_js_contract.set(
                                                    if contract_ready { "ready" } else { "failed" }
                                                        .into(),
                                                );
                                            }
                                            session.set(Some(joined));
                                        }
                                        Err(message) => {
                                            connection.set(RoomConnectionState::Disconnected);
                                            error.set(message);
                                        }
                                    }
                                }
                            },
                            if busy { "Connecting..." } else { "Join room" }
                        }
                    } else {
                        button {
                            class: "secondary-button full-width",
                            r#type: "button",
                            onclick: move |_| {
                                let current = session.read().as_ref().cloned();
                                text_stream_handler.set(None);
                                byte_stream_handler.set(None);
                                if let Some(abort) = text_stream_abort.write().take() {
                                    abort.abort();
                                }
                                if let Some(abort) = byte_stream_abort.write().take() {
                                    abort.abort();
                                }
                                async move {
                                    let Some(current) = current else { return };
                                    connection.set(RoomConnectionState::Disconnecting);
                                    if let Err(leave_error) = current.leave().await {
                                        error.set(leave_error.to_string());
                                    }
                                    connection.set(RoomConnectionState::Disconnected);
                                }
                            },
                            "Leave room"
                        }
                    }
                    if audio_playback == AudioPlaybackState::Blocked {
                        button {
                            class: "audio-button",
                            r#type: "button",
                            onclick: move |_| {
                                let operation = session
                                    .read()
                                    .as_ref()
                                    .map(RoomSession::start_audio_playback);
                                async move {
                                    if let Some(operation) = operation {
                                        if let Err(playback_error) = operation.await {
                                            error.set(playback_error.to_string());
                                        }
                                    }
                                }
                            },
                            "Enable remote audio"
                        }
                    }

                    if connected {
                        div { class: "divider" }
                        p { class: "group-label", "PARTICIPANT STATE" }
                        label { class: "field",
                            span { "Participant metadata" }
                            input {
                                value: "{participant_metadata_input}",
                                autocomplete: "off",
                                oninput: move |event| participant_metadata_input.set(event.value()),
                            }
                        }
                        button {
                            id: "update-participant-metadata",
                            class: "secondary-button full-width",
                            r#type: "button",
                            onclick: move |_| {
                                let metadata = participant_metadata_input.read().clone();
                                let operation = session
                                    .read()
                                    .as_ref()
                                    .map(|session| session.local_participant().set_metadata(metadata));
                                apply_local_participant_operation(operation, media_error_kind, error)
                            },
                            "Update participant metadata"
                        }
                        label { class: "field",
                            span { "Participant attribute key" }
                            input {
                                value: "{participant_attribute_key}",
                                autocomplete: "off",
                                oninput: move |event| participant_attribute_key.set(event.value()),
                            }
                        }
                        label { class: "field",
                            span { "Participant attribute value" }
                            input {
                                value: "{participant_attribute_value}",
                                autocomplete: "off",
                                oninput: move |event| participant_attribute_value.set(event.value()),
                            }
                        }
                        button {
                            id: "update-participant-attribute",
                            class: "secondary-button full-width",
                            r#type: "button",
                            disabled: participant_attribute_key.read().is_empty(),
                            onclick: move |_| {
                                let attributes = BTreeMap::from([(
                                    participant_attribute_key.read().clone(),
                                    participant_attribute_value.read().clone(),
                                )]);
                                let operation = session
                                    .read()
                                    .as_ref()
                                    .map(|session| session.local_participant().update_attributes(attributes));
                                apply_local_participant_operation(operation, media_error_kind, error)
                            },
                            "Update participant attribute"
                        }
                        button {
                            id: "remove-participant-attribute",
                            class: "secondary-button full-width",
                            r#type: "button",
                            disabled: participant_attribute_key.read().is_empty(),
                            onclick: move |_| {
                                let attributes = BTreeMap::from([(
                                    participant_attribute_key.read().clone(),
                                    String::new(),
                                )]);
                                let operation = session
                                    .read()
                                    .as_ref()
                                    .map(|session| session.local_participant().update_attributes(attributes));
                                apply_local_participant_operation(operation, media_error_kind, error)
                            },
                            "Remove participant attribute"
                        }
                        div { class: "divider" }
                        p { class: "group-label", "PUBLISHER ACCESS" }
                        label { class: "field",
                            span { "Allowed participant identity" }
                            input {
                                value: "{allowed_participant_identity}",
                                autocomplete: "off",
                                oninput: move |event| allowed_participant_identity.set(event.value()),
                            }
                        }
                        button {
                            class: "secondary-button full-width",
                            r#type: "button",
                            disabled: allowed_participant_identity.read().is_empty(),
                            onclick: move |_| {
                                let identity = allowed_participant_identity.read().clone();
                                apply_publisher_permissions(
                                    session.read().as_ref().cloned(),
                                    PublicationSubscriptionPermissions::participant_specific([identity]),
                                    "participantSpecific",
                                    publisher_permissions_state,
                                    media_error_kind,
                                    error,
                                )
                            },
                            "Allow only participant"
                        }
                        button {
                            class: "secondary-button full-width",
                            r#type: "button",
                            onclick: move |_| apply_publisher_permissions(
                                session.read().as_ref().cloned(),
                                PublicationSubscriptionPermissions::participant_specific(
                                    std::iter::empty::<String>(),
                                ),
                                "noParticipants",
                                publisher_permissions_state,
                                media_error_kind,
                                error,
                            ),
                            "Deny all participants"
                        }
                        button {
                            class: "secondary-button full-width",
                            r#type: "button",
                            onclick: move |_| apply_publisher_permissions(
                                session.read().as_ref().cloned(),
                                PublicationSubscriptionPermissions::all_participants(),
                                "allParticipants",
                                publisher_permissions_state,
                                media_error_kind,
                                error,
                            ),
                            "Allow all participants"
                        }
                    }

                    div { class: "divider" }
                    button {
                        class: "secondary-button full-width",
                        r#type: "button",
                        disabled: !runtime_ready,
                        onclick: move |_| {
                            let client = runtime.read().as_ref().ok().cloned();
                            async move {
                                let Some(client) = client else { return };
                                error.set(String::new());
                                media_error_kind.set(String::new());
                                let camera_request = client.enumerate_media_devices(
                                    MediaDeviceKind::Camera,
                                    true,
                                );
                                let microphone_request = client.enumerate_media_devices(
                                    MediaDeviceKind::Microphone,
                                    true,
                                );
                                let (cameras, microphones) =
                                    futures_util::future::join(camera_request, microphone_request).await;
                                match cameras {
                                    Ok(devices) => {
                                        let selected = selected_camera_device.read().clone();
                                        if !devices.iter().any(|device| device.device_id() == selected) {
                                            selected_camera_device.set(
                                                devices
                                                    .first()
                                                    .map(|device| device.device_id().to_string())
                                                    .unwrap_or_default(),
                                            );
                                        }
                                        camera_devices.set(devices);
                                    }
                                    Err(device_error) => {
                                        media_error_kind.set(
                                            media_operation_error_kind(&device_error).into(),
                                        );
                                        error.set(device_error.to_string());
                                    }
                                }
                                match microphones {
                                    Ok(devices) => {
                                        let selected = selected_microphone_device.read().clone();
                                        if !devices.iter().any(|device| device.device_id() == selected) {
                                            selected_microphone_device.set(
                                                devices
                                                    .first()
                                                    .map(|device| device.device_id().to_string())
                                                    .unwrap_or_default(),
                                            );
                                        }
                                        microphone_devices.set(devices);
                                    }
                                    Err(device_error) => {
                                        media_error_kind.set(
                                            media_operation_error_kind(&device_error).into(),
                                        );
                                        error.set(device_error.to_string());
                                    }
                                }
                            }
                        },
                        "Refresh input devices"
                    }

                    div { class: "divider" }
                    p { class: "group-label", "LOCAL CAMERA" }
                    label { class: "field",
                        span { "Camera input" }
                        select {
                            value: "{selected_camera_device}",
                            oninput: move |event| selected_camera_device.set(event.value()),
                            option { value: "", "Browser default" }
                            for device in camera_devices.read().iter() {
                                option {
                                    value: device.device_id(),
                                    if device.label().is_empty() {
                                        "Camera ({device.device_id()})"
                                    } else {
                                        "{device.label()}"
                                    }
                                }
                            }
                        }
                    }
                    label { class: "field",
                        span { "Camera facing mode" }
                        select {
                            value: "{camera_facing_mode}",
                            oninput: move |event| camera_facing_mode.set(event.value()),
                            option { value: "user", "User" }
                            option { value: "environment", "Environment" }
                            option { value: "left", "Left" }
                            option { value: "right", "Right" }
                        }
                    }
                    if camera_active {
                        button {
                            class: "secondary-button full-width",
                            r#type: "button",
                            disabled: selected_camera_device.read().is_empty(),
                            onclick: move |_| {
                                let current = local_video_track.read().as_ref().cloned();
                                let selected = selected_camera_device.read().clone();
                                async move {
                                    let Some(current) = current else { return };
                                    error.set(String::new());
                                    media_error_kind.set(String::new());
                                    match current.switch_device(selected).await {
                                        Ok(()) => {
                                            let device_id = current
                                                .device_id()
                                                .await
                                                .ok()
                                                .flatten()
                                                .unwrap_or_default();
                                            active_camera_device.set(device_id);
                                        }
                                        Err(device_error) => {
                                            media_error_kind.set(
                                                media_operation_error_kind(&device_error).into(),
                                            );
                                            error.set(device_error.to_string());
                                        }
                                    }
                                }
                            },
                            "Switch camera"
                        }
                        button {
                            class: "secondary-button full-width",
                            r#type: "button",
                            onclick: move |_| {
                                let current = local_video_track.read().as_ref().cloned();
                                let selected = selected_camera_device.read().clone();
                                let facing_mode = camera_facing_mode.read().clone();
                                async move {
                                    let Some(current) = current else { return };
                                    error.set(String::new());
                                    media_error_kind.set(String::new());
                                    let options = camera_capture_options(&selected, &facing_mode);
                                    match current.restart_with_options(options).await {
                                        Ok(()) => {
                                            let device_id = current
                                                .device_id()
                                                .await
                                                .ok()
                                                .flatten()
                                                .unwrap_or_default();
                                            active_camera_device.set(device_id);
                                        }
                                        Err(device_error) => {
                                            media_error_kind.set(
                                                media_operation_error_kind(&device_error).into(),
                                            );
                                            error.set(device_error.to_string());
                                        }
                                    }
                                }
                            },
                            "Restart camera"
                        }
                        button {
                            class: "secondary-button full-width",
                            r#type: "button",
                            onclick: move |_| {
                                let current = local_video_track.read().as_ref().cloned();
                                async move {
                                    let Some(current) = current else { return };
                                    let stop = current.stop();
                                    local_video_track.set(Some(current));
                                    if let Err(stop_error) = stop.await {
                                        error.set(stop_error.to_string());
                                    } else {
                                        active_camera_device.set(String::new());
                                    }
                                }
                            },
                            "Stop camera"
                        }
                    } else {
                        button {
                            class: "primary-button",
                            r#type: "button",
                            disabled: !runtime_ready || local_video_element.read().is_none(),
                            onclick: move |_| {
                                let client = runtime.read().as_ref().ok().cloned();
                                let element = local_video_element.read().clone();
                                let selected = selected_camera_device.read().clone();
                                let facing_mode = camera_facing_mode.read().clone();
                                async move {
                                    let (Some(client), Some(element)) = (client, element) else { return };
                                    error.set(String::new());
                                    media_error_kind.set(String::new());
                                    let options = camera_capture_options(&selected, &facing_mode);
                                    match client.create_camera_video_track_with_options(options).await {
                                        Ok(track) => match track.attach(&element) {
                                            Ok(attachment) => {
                                                let device_id = track
                                                    .device_id()
                                                    .await
                                                    .ok()
                                                    .flatten()
                                                    .unwrap_or_else(|| selected.clone());
                                                active_camera_device.set(device_id);
                                                local_video_attachment.set(Some(attachment));
                                                local_video_track.set(Some(track));
                                            }
                                            Err(attach_error) => error.set(attach_error.to_string()),
                                        },
                                        Err(capture_error) => {
                                            media_error_kind.set(
                                                media_operation_error_kind(&capture_error).into(),
                                            );
                                            error.set(capture_error.to_string());
                                        }
                                    }
                                }
                            },
                            "Start camera"
                        }
                    }
                    if connected {
                        if video_published {
                            button {
                                class: "secondary-button full-width",
                                r#type: "button",
                                onclick: move |_| {
                                    let publication = local_video_publication.read().as_ref().cloned();
                                    async move {
                                        let Some(publication) = publication else { return };
                                        if let Err(mute_error) = publication.set_muted(true).await {
                                            error.set(mute_error.to_string());
                                        }
                                    }
                                },
                                "Mute camera publication"
                            }
                            button {
                                class: "secondary-button full-width",
                                r#type: "button",
                                onclick: move |_| {
                                    let publication = local_video_publication.read().as_ref().cloned();
                                    async move {
                                        let Some(publication) = publication else { return };
                                        match publication.unpublish().await {
                                            Ok(()) => {
                                                local_video_publication.set(None);
                                                demo_progress.write().local_media_unpublished = true;
                                            }
                                            Err(unpublish_error) => error.set(unpublish_error.to_string()),
                                        }
                                    }
                                },
                                "Unpublish camera"
                            }
                        } else if camera_active {
                            button {
                                class: "secondary-button full-width",
                                r#type: "button",
                                onclick: move |_| {
                                    error.set(String::new());
                                    media_error_kind.set(String::new());
                                    let track = local_video_track.read().as_ref().cloned();
                                    let room = session.read().as_ref().cloned();
                                    async move {
                                        let (Some(track), Some(room)) = (track, room) else { return };
                                        let options = VideoPublishOptions::default()
                                            .with_source(TrackSource::Camera)
                                            .with_name("camera-preview")
                                            .with_codec(VideoCodec::Vp8)
                                            .with_simulcast(true)
                                            .with_encoding(VideoEncoding::new(800_000, 30));
                                        match track.publish(&room, options).await {
                                            Ok(publication) => local_video_publication.set(Some(publication)),
                                            Err(publish_error) => {
                                                media_error_kind.set(
                                                    media_operation_error_kind(&publish_error).into(),
                                                );
                                                error.set(publish_error.to_string());
                                            }
                                        }
                                    }
                                },
                                "Publish camera"
                            }
                        }
                    }

                    div { class: "divider" }
                    p { class: "group-label", "LOCAL MICROPHONE" }
                    label { class: "field",
                        span { "Microphone input" }
                        select {
                            value: "{selected_microphone_device}",
                            oninput: move |event| selected_microphone_device.set(event.value()),
                            option { value: "", "Browser default" }
                            for device in microphone_devices.read().iter() {
                                option {
                                    value: device.device_id(),
                                    if device.label().is_empty() {
                                        "Microphone ({device.device_id()})"
                                    } else {
                                        "{device.label()}"
                                    }
                                }
                            }
                        }
                    }
                    if microphone_active {
                        button {
                            class: "secondary-button full-width",
                            r#type: "button",
                            disabled: selected_microphone_device.read().is_empty(),
                            onclick: move |_| {
                                let current = local_audio_track.read().as_ref().cloned();
                                let selected = selected_microphone_device.read().clone();
                                async move {
                                    let Some(current) = current else { return };
                                    error.set(String::new());
                                    media_error_kind.set(String::new());
                                    match current.switch_device(selected).await {
                                        Ok(()) => {
                                            let device_id = current
                                                .device_id()
                                                .await
                                                .ok()
                                                .flatten()
                                                .unwrap_or_default();
                                            active_microphone_device.set(device_id);
                                        }
                                        Err(device_error) => {
                                            media_error_kind.set(
                                                media_operation_error_kind(&device_error).into(),
                                            );
                                            error.set(device_error.to_string());
                                        }
                                    }
                                }
                            },
                            "Switch microphone"
                        }
                        button {
                            class: "secondary-button full-width",
                            r#type: "button",
                            onclick: move |_| {
                                let current = local_audio_track.read().as_ref().cloned();
                                let selected = selected_microphone_device.read().clone();
                                async move {
                                    let Some(current) = current else { return };
                                    error.set(String::new());
                                    media_error_kind.set(String::new());
                                    let options = microphone_capture_options(&selected);
                                    match current.restart_with_options(options).await {
                                        Ok(()) => {
                                            let device_id = current
                                                .device_id()
                                                .await
                                                .ok()
                                                .flatten()
                                                .unwrap_or_default();
                                            active_microphone_device.set(device_id);
                                        }
                                        Err(device_error) => {
                                            media_error_kind.set(
                                                media_operation_error_kind(&device_error).into(),
                                            );
                                            error.set(device_error.to_string());
                                        }
                                    }
                                }
                            },
                            "Restart microphone"
                        }
                        button {
                            class: "secondary-button full-width",
                            r#type: "button",
                            onclick: move |_| {
                                let current = local_audio_track.read().as_ref().cloned();
                                async move {
                                    let Some(current) = current else { return };
                                    let stop = current.stop();
                                    local_audio_track.set(Some(current));
                                    if let Err(stop_error) = stop.await {
                                        error.set(stop_error.to_string());
                                    } else {
                                        active_microphone_device.set(String::new());
                                    }
                                }
                            },
                            "Stop microphone"
                        }
                    } else {
                        button {
                            class: "primary-button",
                            r#type: "button",
                            disabled: !runtime_ready,
                            onclick: move |_| {
                                let client = runtime.read().as_ref().ok().cloned();
                                let selected = selected_microphone_device.read().clone();
                                async move {
                                    let Some(client) = client else { return };
                                    error.set(String::new());
                                    media_error_kind.set(String::new());
                                    let options = microphone_capture_options(&selected);
                                    match client.create_microphone_audio_track_with_options(options).await {
                                        Ok(track) => {
                                            let device_id = track
                                                .device_id()
                                                .await
                                                .ok()
                                                .flatten()
                                                .unwrap_or_else(|| selected.clone());
                                            active_microphone_device.set(device_id);
                                            local_audio_track.set(Some(track));
                                        }
                                        Err(capture_error) => {
                                            media_error_kind.set(
                                                media_operation_error_kind(&capture_error).into(),
                                            );
                                            error.set(capture_error.to_string());
                                        }
                                    }
                                }
                            },
                            "Start microphone"
                        }
                    }
                    if connected {
                        if audio_published {
                            button {
                                class: "secondary-button full-width",
                                r#type: "button",
                                onclick: move |_| {
                                    let publication = local_audio_publication.read().as_ref().cloned();
                                    async move {
                                        let Some(publication) = publication else { return };
                                        if let Err(mute_error) = publication.set_muted(true).await {
                                            error.set(mute_error.to_string());
                                        }
                                    }
                                },
                                "Mute microphone publication"
                            }
                            button {
                                class: "secondary-button full-width",
                                r#type: "button",
                                onclick: move |_| {
                                    let publication = local_audio_publication.read().as_ref().cloned();
                                    async move {
                                        let Some(publication) = publication else { return };
                                        match publication.unpublish().await {
                                            Ok(()) => {
                                                local_audio_publication.set(None);
                                                demo_progress.write().local_media_unpublished = true;
                                            }
                                            Err(unpublish_error) => error.set(unpublish_error.to_string()),
                                        }
                                    }
                                },
                                "Unpublish microphone"
                            }
                        } else if microphone_active {
                            button {
                                class: "secondary-button full-width",
                                r#type: "button",
                                onclick: move |_| {
                                    let track = local_audio_track.read().as_ref().cloned();
                                    let room = session.read().as_ref().cloned();
                                    async move {
                                        let (Some(track), Some(room)) = (track, room) else { return };
                                        match track.publish(&room, microphone_publish_options()).await {
                                            Ok(publication) => local_audio_publication.set(Some(publication)),
                                            Err(publish_error) => error.set(publish_error.to_string()),
                                        }
                                    }
                                },
                                "Publish microphone"
                            }
                        }
                    }

                    div { class: "divider" }
                    p { class: "group-label", "LOCAL CLIP RECORDING" }
                    p { class: "help-copy",
                        "Encode selected camera and microphone Local Media Tracks into a bounded browser Blob."
                    }
                    button {
                        id: "start-local-clip",
                        class: "record-button",
                        r#type: "button",
                        disabled: !local_clip_supported
                            || (!camera_active && !microphone_active)
                            || matches!(local_clip_state, "recording" | "paused" | "stopping"),
                        onclick: move |_| {
                            local_clip_downloaded.set(false);
                            begin_local_clip(
                                runtime_capabilities,
                                local_video_track.read().as_ref().cloned(),
                                local_audio_track.read().as_ref().cloned(),
                                LOCAL_CLIP_MIME_PREFERENCES,
                                LocalClipLimits::DurationOrSize {
                                    max_duration: Duration::from_secs(30),
                                    max_bytes: 64 * 1024 * 1024,
                                },
                                local_clip_signals,
                            );
                        },
                        "Start local clip"
                    }
                    button {
                        id: "pause-local-clip",
                        class: "secondary-button full-width",
                        r#type: "button",
                        disabled: local_clip_state != "recording",
                        onclick: move |_| {
                            let current = local_clip_recording.read().as_ref().cloned();
                            if let Some(current) = current {
                                match current.pause() {
                                    Ok(()) => local_clip_recording.set(Some(current)),
                                    Err(recording_error) => {
                                        local_clip_error.set(local_clip_error_kind(&recording_error).into());
                                        error.set(recording_error.to_string());
                                    }
                                }
                            }
                        },
                        "Pause local clip"
                    }
                    button {
                        id: "resume-local-clip",
                        class: "secondary-button full-width",
                        r#type: "button",
                        disabled: local_clip_state != "paused",
                        onclick: move |_| {
                            let current = local_clip_recording.read().as_ref().cloned();
                            if let Some(current) = current {
                                match current.resume() {
                                    Ok(()) => local_clip_recording.set(Some(current)),
                                    Err(recording_error) => {
                                        local_clip_error.set(local_clip_error_kind(&recording_error).into());
                                        error.set(recording_error.to_string());
                                    }
                                }
                            }
                        },
                        "Resume local clip"
                    }
                    button {
                        id: "stop-local-clip",
                        class: "record-button stop",
                        r#type: "button",
                        disabled: !matches!(local_clip_state, "recording" | "paused"),
                        onclick: move |_| {
                            let current = local_clip_recording.read().as_ref().cloned();
                            if let Some(current) = current {
                                match current.stop() {
                                    Ok(finished) => {
                                        local_clip_recording.set(Some(current));
                                        wasm_bindgen_futures::spawn_local(async move {
                                            match finished.await {
                                                Ok(_) => local_clip_stop_returned.set(true),
                                                Err(recording_error) => {
                                                    local_clip_error.set(local_clip_error_kind(&recording_error).into());
                                                    error.set(recording_error.to_string());
                                                }
                                            }
                                        });
                                    }
                                    Err(recording_error) => {
                                        local_clip_error.set(local_clip_error_kind(&recording_error).into());
                                        error.set(recording_error.to_string());
                                    }
                                }
                            }
                        },
                        "Stop local clip"
                    }
                    if local_clip_artifact_ready {
                        div {
                            id: "local-clip-artifact",
                            class: "artifact-card",
                            "data-url-state": if local_clip_url_active {
                                "active"
                            } else if local_clip_url_created {
                                "revoked"
                            } else {
                                "unavailable"
                            },
                            div { class: "artifact-heading",
                                span { "BROWSER-OWNED BLOB" }
                                strong { "{local_clip_size}" }
                            }
                            dl {
                                div { dt { "Format" } dd { "{local_clip_mime}" } }
                                div { dt { "Duration" } dd { "{local_clip_duration}" } }
                                div { dt { "Stopped by" } dd { "{local_clip_stop_reason}" } }
                                div { dt { "Filename" } dd { "{local_clip_filename}" } }
                                div {
                                    dt { "Object URL" }
                                    dd {
                                        if local_clip_url_active {
                                            "active"
                                        } else if local_clip_url_created {
                                            "revoked"
                                        } else {
                                            "unavailable"
                                        }
                                    }
                                }
                            }
                        }
                    }
                    button {
                        id: "revoke-local-clip-url",
                        class: "secondary-button full-width",
                        r#type: "button",
                        disabled: !local_clip_url_active,
                        onclick: move |_| {
                            let current = local_clip_object_url.write().take();
                            if let Some(current) = current {
                                if let Err(url_error) = current.revoke() {
                                    local_clip_error.set(local_clip_error_kind(&url_error).into());
                                    error.set(url_error.to_string());
                                }
                                local_clip_object_url.set(Some(current));
                            }
                        },
                        "Revoke local clip URL"
                    }
                    if local_clip_url_active {
                        a {
                            id: "local-clip-download",
                            class: "download-link",
                            href: local_clip_url,
                            download: local_clip_filename,
                            onclick: move |_| local_clip_downloaded.set(true),
                            "Download local clip"
                        }
                    }

                    div { class: "divider" }
                    p { class: "group-label", "LOCAL SCREEN SHARE" }
                    label { class: "check-field",
                        input {
                            r#type: "checkbox",
                            checked: *request_system_audio.read(),
                            disabled: screen_video_active,
                            onchange: move |event| request_system_audio.set(event.checked()),
                        }
                        span { "Request system audio when available" }
                    }
                    if screen_video_active {
                        button {
                            class: "secondary-button full-width",
                            r#type: "button",
                            onclick: move |_| {
                                let current = screen_capture.read().as_ref().cloned();
                                async move {
                                    let Some(current) = current else { return };
                                    error.set(String::new());
                                    if let Err(stop_error) = current.stop().await {
                                        error.set(stop_error.to_string());
                                    }
                                    screen_state_refresh += 1;
                                }
                            },
                            "Stop screen share"
                        }
                    } else {
                        button {
                            class: "primary-button",
                            r#type: "button",
                            disabled: !runtime_ready
                                || screen_video_element.read().is_none()
                                || screen_video_published
                                || screen_audio_published,
                            onclick: move |_| {
                                let client = runtime.read().as_ref().ok().cloned();
                                let video_element = screen_video_element.read().clone();
                                let include_audio = *request_system_audio.read();
                                async move {
                                    let (Some(client), Some(video_element)) = (client, video_element) else {
                                        return;
                                    };
                                    error.set(String::new());
                                    media_error_kind.set(String::new());
                                    let options = ScreenCaptureOptions::default()
                                        .with_system_audio(include_audio);
                                    let capture_request = client.create_screen_capture(options);
                                    match capture_request.await {
                                        Ok(capture) => {
                                            let video = capture.display_video().clone();
                                            match video.attach(&video_element) {
                                                Ok(attachment) => {
                                                    let video_ended = video.ended();
                                                    wasm_bindgen_futures::spawn_local(async move {
                                                        let _ = video_ended.await;
                                                        screen_state_refresh += 1;
                                                    });
                                                    if let Some(audio) = capture.system_audio().cloned() {
                                                        let audio_ended = audio.ended();
                                                        wasm_bindgen_futures::spawn_local(async move {
                                                            let _ = audio_ended.await;
                                                            screen_state_refresh += 1;
                                                        });
                                                    }
                                                    screen_video_publication.set(None);
                                                    screen_audio_publication.set(None);
                                                    screen_video_attachment.set(Some(attachment));
                                                    screen_audio_attachment.set(None);
                                                    screen_capture.set(Some(capture));
                                                }
                                                Err(attach_error) => {
                                                    let _ = capture.stop().await;
                                                    error.set(attach_error.to_string());
                                                }
                                            }
                                        }
                                        Err(capture_error) => {
                                            media_error_kind.set(
                                                media_operation_error_kind(&capture_error).into(),
                                            );
                                            error.set(capture_error.to_string());
                                        }
                                    }
                                }
                            },
                            "Start screen share"
                        }
                    }
                    if connected {
                        if screen_video_published {
                            button {
                                class: "secondary-button full-width",
                                r#type: "button",
                                onclick: move |_| {
                                    let publication = screen_video_publication.read().as_ref().cloned();
                                    async move {
                                        let Some(publication) = publication else { return };
                                        match publication.unpublish().await {
                                            Ok(()) => {
                                                screen_video_publication.set(None);
                                                demo_progress.write().local_media_unpublished = true;
                                            }
                                            Err(unpublish_error) => error.set(unpublish_error.to_string()),
                                        }
                                    }
                                },
                                "Unpublish screen video"
                            }
                        } else if screen_video_active {
                            button {
                                class: "secondary-button full-width",
                                r#type: "button",
                                onclick: move |_| {
                                    let track = screen_capture
                                        .read()
                                        .as_ref()
                                        .map(|capture| capture.display_video().clone());
                                    let room = session.read().as_ref().cloned();
                                    async move {
                                        let (Some(track), Some(room)) = (track, room) else { return };
                                        let options = VideoPublishOptions::default()
                                            .with_source(TrackSource::ScreenShare)
                                            .with_name("display-capture")
                                            .with_codec(VideoCodec::Vp8)
                                            .with_simulcast(true)
                                            .with_encoding(VideoEncoding::new(1_500_000, 15));
                                        match track.publish(&room, options).await {
                                            Ok(publication) => screen_video_publication.set(Some(publication)),
                                            Err(publish_error) => error.set(publish_error.to_string()),
                                        }
                                    }
                                },
                                "Publish screen video"
                            }
                        }
                        if screen_audio_published {
                            button {
                                class: "secondary-button full-width",
                                r#type: "button",
                                onclick: move |_| {
                                    let publication = screen_audio_publication.read().as_ref().cloned();
                                    async move {
                                        let Some(publication) = publication else { return };
                                        match publication.unpublish().await {
                                            Ok(()) => {
                                                screen_audio_publication.set(None);
                                                demo_progress.write().local_media_unpublished = true;
                                            }
                                            Err(unpublish_error) => error.set(unpublish_error.to_string()),
                                        }
                                    }
                                },
                                "Unpublish system audio"
                            }
                        } else if screen_audio_present && !screen_audio_stopped {
                            button {
                                class: "secondary-button full-width",
                                r#type: "button",
                                onclick: move |_| {
                                    let track = screen_capture
                                        .read()
                                        .as_ref()
                                        .and_then(ScreenCapture::system_audio)
                                        .cloned();
                                    let room = session.read().as_ref().cloned();
                                    async move {
                                        let (Some(track), Some(room)) = (track, room) else { return };
                                        let options = AudioPublishOptions::default()
                                            .with_source(AudioTrackSource::ScreenShareAudio)
                                            .with_name("display-system-audio")
                                            .with_codec(AudioCodec::Opus)
                                            .with_dtx(false)
                                            .with_red(false);
                                        match track.publish(&room, options).await {
                                            Ok(publication) => screen_audio_publication.set(Some(publication)),
                                            Err(publish_error) => error.set(publish_error.to_string()),
                                        }
                                    }
                                },
                                "Publish system audio"
                            }
                        }
                    }

                    if session_handle.is_some() {
                        button {
                            id: "retain-and-unmount-room-provider",
                            hidden: true,
                            r#type: "button",
                            onclick: move |_| {
                                retained_session.set(session.read().as_ref().cloned());
                                session.set(None);
                            },
                            "Retain Room Session and unmount provider"
                        }
                        button {
                            id: "drop-room-session",
                            hidden: true,
                            r#type: "button",
                            onclick: move |_| {
                                session.set(None);
                                connection.set(RoomConnectionState::Disconnected);
                            },
                            "Drop Room Session handle"
                        }
                    }
                }
            }

            details { class: "state-panel", open: true,
                summary {
                    span { "CURRENT ROOM SNAPSHOT" }
                    small { "Revision {revision}" }
                }
                pre { id: "prototype-state",
                    "connection: {connection_text}\nidentity: {participant_identity}\nparticipant sid: {participant_sid}\npublications: {publication_count}\nterminal: {terminal}"
                }
            }
        }
    }
}

#[component]
fn RoomSessionIntegration(
    mut connection: Signal<RoomConnectionState>,
    mut refresh: Signal<u64>,
    local_video_track: Signal<Option<LocalVideoTrack>>,
    local_audio_track: Signal<Option<LocalAudioTrack>>,
    mut error: Signal<String>,
) -> Element {
    let room = use_room_session();
    let snapshot = use_room_snapshot();
    let participants = use_participants();
    let publications = use_track_publications();
    let capabilities = use_browser_capabilities();
    let playback = use_audio_playback();
    let actions = use_room_actions();
    let mut show_media_nodes = use_signal(|| false);

    use_effect(move || {
        connection.set(snapshot.read().connection());
        refresh += 1;
    });

    let snapshot_read = snapshot.read();
    let revision = snapshot_read.revision();
    let connected = snapshot_read.connection() == RoomConnectionState::Connected;
    drop(snapshot_read);
    let participant_read = participants.read();
    let remote_participant_count = participant_read.remote().len();
    drop(participant_read);
    let publication_read = publications.read();
    let local_publication_count = publication_read.local().len();
    let remote_publication_count = publication_read
        .remote()
        .values()
        .map(BTreeMap::len)
        .sum::<usize>();
    let remote_subscription_requests = publication_read
        .remote()
        .iter()
        .flat_map(|(identity, publications)| {
            publications.iter().map(move |(sid, publication)| {
                (
                    RemoteTrackPublicationSubscription::new(
                        identity,
                        sid,
                        !publication.subscribed(),
                    ),
                    track_kind_label(publication.kind()),
                )
            })
        })
        .collect::<Vec<_>>();
    let remote_media = publication_read
        .remote()
        .iter()
        .flat_map(|(identity, publications)| {
            publications.keys().filter_map(|sid| {
                room.remote_track_publication(identity, sid)
                    .and_then(|publication| publication.media_track().ok().flatten())
            })
        })
        .collect::<Vec<_>>();
    drop(publication_read);
    let local_video = local_video_track.read().clone();
    let local_audio = local_audio_track.read().clone();
    let media_nodes_visible = *show_media_nodes.read();
    let playback_label = match *playback.read() {
        AudioPlaybackState::Blocked => "blocked",
        _ => "allowed",
    };
    let mut leave_action = actions.leave;
    let mut playback_action = actions.start_audio_playback;
    let mut remote_subscription_action = actions.set_remote_track_publication_subscription;
    let mut permissions_action = actions.set_publication_subscription_permissions;
    let mut metadata_action = actions.set_local_participant_metadata;
    let mut attributes_action = actions.update_local_participant_attributes;
    let leave_action_state = dioxus_action_state(&leave_action);
    let playback_action_state = dioxus_action_state(&playback_action);
    let remote_subscription_action_state = dioxus_action_state(&remote_subscription_action);
    let permissions_action_state = dioxus_action_state(&permissions_action);
    let metadata_action_state = dioxus_action_state(&metadata_action);
    let attributes_action_state = dioxus_action_state(&attributes_action);

    rsx! {
        div {
            id: "dioxus-room-session",
            hidden: true,
            "data-revision": revision,
            "data-connected": connected,
            "data-remote-participant-count": remote_participant_count,
            "data-local-publication-count": local_publication_count,
            "data-remote-publication-count": remote_publication_count,
            "data-webrtc": capabilities.web_rtc(),
            "data-playback": playback_label,
            "data-leave-action": leave_action_state,
            "data-playback-action": playback_action_state,
            "data-remote-subscription-action": remote_subscription_action_state,
            "data-permissions-action": permissions_action_state,
            "data-metadata-action": metadata_action_state,
            "data-attributes-action": attributes_action_state,
            button {
                id: "toggle-dioxus-media-nodes",
                r#type: "button",
                onclick: move |_| show_media_nodes.toggle(),
                "Toggle Dioxus media nodes"
            }
            button {
                id: "dioxus-start-audio-playback",
                r#type: "button",
                onclick: move |_| {
                    playback_action.call();
                },
                "Start audio through Dioxus action"
            }
            button {
                id: "dioxus-leave-room",
                r#type: "button",
                onclick: move |_| {
                    leave_action.call();
                },
                "Leave through Dioxus action"
            }
            button {
                id: "dioxus-allow-all-subscriptions",
                r#type: "button",
                onclick: move |_| {
                    permissions_action.call(PublicationSubscriptionPermissions::all_participants());
                },
                "Allow all subscriptions through Dioxus action"
            }
            button {
                id: "dioxus-set-metadata",
                r#type: "button",
                onclick: move |_| {
                    metadata_action.call("dioxus-action-metadata".to_string());
                },
                "Set metadata through Dioxus action"
            }
            button {
                id: "dioxus-update-attributes",
                r#type: "button",
                onclick: move |_| {
                    attributes_action.call(BTreeMap::from([(
                        "dioxus-action".to_string(),
                        "complete".to_string(),
                    )]));
                },
                "Update attributes through Dioxus action"
            }
            for (request, kind) in remote_subscription_requests {
                button {
                    class: "dioxus-toggle-remote-subscription",
                    "data-kind": kind,
                    "data-target-subscribed": request.subscribed(),
                    r#type: "button",
                    onclick: move |_| {
                        remote_subscription_action.call(request.clone());
                    },
                    "Toggle Remote Track Publication subscription through Dioxus action"
                }
            }
            if media_nodes_visible {
                if let Some(track) = local_video {
                    LocalVideo {
                        track,
                        class: "dioxus-local-video",
                        autoplay: true,
                        muted: true,
                        "playsinline": true,
                        on_error: move |attach_error: LiveKitError| error.set(attach_error.to_string()),
                    }
                }
                if let Some(track) = local_audio {
                    LocalAudio {
                        track,
                        class: "dioxus-local-audio",
                        on_error: move |attach_error: LiveKitError| error.set(attach_error.to_string()),
                    }
                }
                for media in remote_media {
                    match media {
                        RemoteMediaTrack::Video(track) => rsx! {
                            RemoteVideo {
                                key: "{track.participant_identity()}:{track.publication_sid()}",
                                track,
                                class: "dioxus-remote-video",
                                autoplay: true,
                                "playsinline": true,
                                on_error: move |attach_error: LiveKitError| error.set(attach_error.to_string()),
                            }
                        },
                        RemoteMediaTrack::Audio(track) => rsx! {
                            RemoteAudio {
                                key: "{track.participant_identity()}:{track.publication_sid()}",
                                track,
                                class: "dioxus-remote-audio",
                                autoplay: true,
                                on_error: move |attach_error: LiveKitError| error.set(attach_error.to_string()),
                            }
                        },
                        _ => rsx! {},
                    }
                }
            }
        }
    }
}

fn dioxus_action_state<Input: 'static, Output: 'static>(
    action: &Action<Input, Output>,
) -> &'static str {
    if action.pending() {
        "pending"
    } else {
        match action.value() {
            Some(Ok(_)) => "ready",
            Some(Err(_)) => "error",
            None => "idle",
        }
    }
}

#[component]
fn RemoteVideoPublicationNode(
    session: Signal<Option<RoomSession>>,
    participant_identity: String,
    publication_sid: String,
    subscribed: bool,
    media_available: bool,
    refresh: Signal<u64>,
    mut error: Signal<String>,
) -> Element {
    let mut element = use_signal(|| None::<web_sys::HtmlVideoElement>);
    let mut track = use_signal(|| None::<RemoteVideoTrack>);
    let mut attachment = use_signal(|| None::<RemoteVideoTrackAttachment>);
    let mut sibling_track = use_signal(|| None::<RemoteVideoTrack>);
    let mut sibling_attachment = use_signal(|| None::<RemoteVideoTrackAttachment>);
    let mut attachment_enabled = use_signal(|| true);
    let mut statistics_timestamp = use_signal(|| 0.0_f64);
    let mut statistics_frames_received = use_signal(|| 0_u64);

    use_effect({
        let participant_identity = participant_identity.clone();
        let publication_sid = publication_sid.clone();
        move || {
            let _refresh = *refresh.read();
            let enabled = *attachment_enabled.read();
            if !media_available {
                attachment.set(None);
                track.set(None);
                sibling_attachment.set(None);
                sibling_track.set(None);
                return;
            }
            if !enabled {
                attachment.set(None);
                track.set(None);
                return;
            }
            if attachment
                .read()
                .as_ref()
                .is_some_and(RemoteVideoTrackAttachment::is_active)
            {
                return;
            }
            let element = element.read().clone();
            let current = session.read().as_ref().cloned();
            let (Some(element), Some(current)) = (element, current) else {
                return;
            };
            let Some(publication) =
                current.remote_track_publication(&participant_identity, &publication_sid)
            else {
                return;
            };
            match publication.media_track() {
                Ok(Some(RemoteMediaTrack::Video(remote))) => match remote.attach(&element) {
                    Ok(guard) => {
                        track.set(Some(remote));
                        attachment.set(Some(guard));
                    }
                    Err(attach_error) => error.set(attach_error.to_string()),
                },
                Ok(None) => {}
                Ok(Some(_)) => {
                    error.set("Remote Track Publication resolved the wrong media kind".into())
                }
                Err(resolve_error) => error.set(resolve_error.to_string()),
            }
        }
    });

    let attached = attachment
        .read()
        .as_ref()
        .is_some_and(RemoteVideoTrackAttachment::is_active);
    let terminal = track
        .read()
        .as_ref()
        .is_some_and(RemoteVideoTrack::is_terminal);
    let sibling_attached = sibling_attachment
        .read()
        .as_ref()
        .is_some_and(RemoteVideoTrackAttachment::is_active);
    let subscription_identity = participant_identity.clone();
    let subscription_sid = publication_sid.clone();
    let sibling_identity = participant_identity.clone();
    let sibling_sid = publication_sid.clone();
    let subscription_label = if subscribed {
        "Unsubscribe"
    } else {
        "Subscribe"
    };

    rsx! {
        div {
            class: "remote-tile remote-media-publication",
            "data-participant-identity": participant_identity.as_str(),
            "data-publication-sid": publication_sid.as_str(),
            "data-kind": "video",
            "data-subscribed": subscribed,
            "data-media-available": media_available,
            "data-attached": attached,
            "data-sibling-attached": sibling_attached,
            "data-terminal": terminal,
            "data-statistics-timestamp": statistics_timestamp,
            "data-statistics-frames-received": statistics_frames_received,
            if media_available {
                video {
                    autoplay: true,
                    "playsinline": true,
                    "aria-label": "Remote video from {participant_identity}",
                    onmounted: move |event| {
                        use wasm_bindgen::JsCast;

                        let video = event
                            .data()
                            .downcast::<web_sys::Element>()
                            .cloned()
                            .and_then(|element| element.dyn_into().ok());
                        element.set(video);
                    },
                }
            }
            span { "{participant_identity}" }
            button {
                class: "remote-subscription-button",
                r#type: "button",
                onclick: move |_| {
                    let operation = session
                        .read()
                        .as_ref()
                        .and_then(|current| {
                            current.remote_track_publication(
                                &subscription_identity,
                                &subscription_sid,
                            )
                        })
                        .map(|publication| publication.set_subscribed(!subscribed));
                    async move {
                        if let Some(operation) = operation {
                            if let Err(subscription_error) = operation.await {
                                error.set(subscription_error.to_string());
                            }
                        }
                    }
                },
                "{subscription_label} {participant_identity} video"
            }
            button {
                class: "attach-sibling-remote-video",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let element = element.read().clone();
                    let current = session.read().as_ref().cloned();
                    let Some((element, current)) = element.zip(current) else {
                        return;
                    };
                    let Some(publication) = current
                        .remote_track_publication(&sibling_identity, &sibling_sid)
                    else {
                        return;
                    };
                    match publication.media_track() {
                        Ok(Some(RemoteMediaTrack::Video(remote))) => match remote.attach(&element) {
                            Ok(guard) => {
                                sibling_track.set(Some(remote));
                                sibling_attachment.set(Some(guard));
                            }
                            Err(attach_error) => error.set(attach_error.to_string()),
                        },
                        Ok(None) => {}
                        Ok(Some(_)) => {
                            error.set("Remote Track Publication resolved the wrong media kind".into())
                        }
                        Err(resolve_error) => error.set(resolve_error.to_string()),
                    }
                },
                "Attach sibling remote video guard"
            }
            button {
                class: "refresh-remote-statistics",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let current = track.read().as_ref().cloned();
                    async move {
                        let Some(current) = current else { return };
                        match current.receiver_statistics().await {
                            Ok(Some(statistics)) => {
                                statistics_timestamp.set(statistics.receiver().timestamp());
                                statistics_frames_received.set(statistics.frames_received().unwrap_or(0));
                            }
                            Ok(None) => {}
                            Err(statistics_error) => error.set(statistics_error.to_string()),
                        }
                    }
                },
                "Refresh remote video statistics"
            }
            button {
                class: "close-remote-video-attachment",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    attachment_enabled.set(false);
                    let guard = attachment.write().take();
                    if let Some(guard) = guard {
                        if let Err(close_error) = guard.close() {
                            error.set(close_error.to_string());
                        }
                        attachment.set(Some(guard));
                    }
                },
                "Close remote video attachment"
            }
            button {
                class: "drop-sibling-remote-video-attachment",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    sibling_attachment.set(None);
                    sibling_track.set(None);
                },
                "Drop sibling remote video attachment"
            }
            button {
                class: "drop-remote-video-attachment",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    attachment_enabled.set(false);
                    attachment.set(None);
                },
                "Drop remote video attachment"
            }
        }
    }
}

#[component]
fn RemoteAudioPublicationNode(
    session: Signal<Option<RoomSession>>,
    participant_identity: String,
    publication_sid: String,
    subscribed: bool,
    media_available: bool,
    refresh: Signal<u64>,
    mut error: Signal<String>,
) -> Element {
    let mut element = use_signal(|| None::<web_sys::HtmlAudioElement>);
    let mut track = use_signal(|| None::<RemoteAudioTrack>);
    let mut attachment = use_signal(|| None::<RemoteAudioTrackAttachment>);
    let mut attachment_enabled = use_signal(|| true);
    let mut analyser = use_signal(|| None::<AudioAnalyser>);
    let analysis_observation = use_signal(AudioAnalysisObservation::default);
    let mut statistics_timestamp = use_signal(|| 0.0_f64);
    let mut statistics_packets_received = use_signal(|| 0_u64);
    let mut statistics_bytes_received = use_signal(|| 0_u64);
    let mut statistics_total_audio_energy = use_signal(|| 0.0_f64);
    let mut statistics_total_samples_duration = use_signal(|| 0.0_f64);

    use_effect({
        let participant_identity = participant_identity.clone();
        let publication_sid = publication_sid.clone();
        move || {
            let _refresh = *refresh.read();
            let enabled = *attachment_enabled.read();
            if !media_available || !enabled {
                attachment.set(None);
                track.set(None);
                return;
            }
            if attachment
                .read()
                .as_ref()
                .is_some_and(RemoteAudioTrackAttachment::is_active)
            {
                return;
            }
            let element = element.read().clone();
            let current = session.read().as_ref().cloned();
            let (Some(element), Some(current)) = (element, current) else {
                return;
            };
            let Some(publication) =
                current.remote_track_publication(&participant_identity, &publication_sid)
            else {
                return;
            };
            match publication.media_track() {
                Ok(Some(RemoteMediaTrack::Audio(remote))) => match remote.attach(&element) {
                    Ok(guard) => {
                        track.set(Some(remote));
                        attachment.set(Some(guard));
                    }
                    Err(attach_error) => error.set(attach_error.to_string()),
                },
                Ok(None) => {}
                Ok(Some(_)) => {
                    error.set("Remote Track Publication resolved the wrong media kind".into())
                }
                Err(resolve_error) => error.set(resolve_error.to_string()),
            }
        }
    });

    let attached = attachment
        .read()
        .as_ref()
        .is_some_and(RemoteAudioTrackAttachment::is_active);
    let terminal = track
        .read()
        .as_ref()
        .is_some_and(RemoteAudioTrack::is_terminal);
    let analyser_present = analyser.read().is_some();
    let analyser_active = analyser
        .read()
        .as_ref()
        .is_some_and(AudioAnalyser::is_active);
    let observed_analysis = analysis_observation.read().clone();
    let subscription_identity = participant_identity.clone();
    let subscription_sid = publication_sid.clone();
    let subscription_label = if subscribed {
        "Unsubscribe"
    } else {
        "Subscribe"
    };

    rsx! {
        div {
            class: "remote-audio remote-media-publication",
            "data-participant-identity": participant_identity.as_str(),
            "data-publication-sid": publication_sid.as_str(),
            "data-kind": "audio",
            "data-subscribed": subscribed,
            "data-media-available": media_available,
            "data-attached": attached,
            "data-terminal": terminal,
            "data-analyser-present": analyser_present,
            "data-analyser-active": analyser_active,
            "data-analyser-level": observed_analysis.level,
            "data-analyser-waveform-length": observed_analysis.waveform_length,
            "data-analyser-waveform-fingerprint": observed_analysis.waveform_fingerprint,
            "data-statistics-timestamp": statistics_timestamp,
            "data-statistics-packets-received": statistics_packets_received,
            "data-statistics-bytes-received": statistics_bytes_received,
            "data-statistics-total-audio-energy": statistics_total_audio_energy,
            "data-statistics-total-samples-duration": statistics_total_samples_duration,
            onmounted: move |event| {
                use wasm_bindgen::JsCast;

                let audio = event
                    .data()
                    .downcast::<web_sys::Element>()
                    .and_then(|wrapper| wrapper.query_selector("audio").ok().flatten())
                    .and_then(|element| element.dyn_into().ok());
                element.set(audio);
            },
            audio {
                autoplay: true,
                "aria-label": "Remote audio from {participant_identity}",
            }
            button {
                class: "remote-subscription-button",
                r#type: "button",
                onclick: move |_| {
                    let operation = session
                        .read()
                        .as_ref()
                        .and_then(|current| {
                            current.remote_track_publication(
                                &subscription_identity,
                                &subscription_sid,
                            )
                        })
                        .map(|publication| publication.set_subscribed(!subscribed));
                    async move {
                        if let Some(operation) = operation {
                            if let Err(subscription_error) = operation.await {
                                error.set(subscription_error.to_string());
                            }
                        }
                    }
                },
                "{subscription_label} {participant_identity} audio"
            }
            button {
                class: "close-remote-audio-attachment",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    attachment_enabled.set(false);
                    let guard = attachment.write().take();
                    if let Some(guard) = guard {
                        if let Err(close_error) = guard.close() {
                            error.set(close_error.to_string());
                        }
                        attachment.set(Some(guard));
                    }
                },
                "Close remote audio attachment"
            }
            button {
                class: "refresh-remote-statistics",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let current = track.read().as_ref().cloned();
                    async move {
                        let Some(current) = current else { return };
                        match current.receiver_statistics().await {
                            Ok(Some(statistics)) => {
                                statistics_timestamp.set(statistics.receiver().timestamp());
                                statistics_packets_received.set(
                                    statistics.receiver().packets_received().unwrap_or(0),
                                );
                                statistics_bytes_received.set(
                                    statistics.receiver().bytes_received().unwrap_or(0),
                                );
                                statistics_total_audio_energy.set(
                                    statistics.total_audio_energy().unwrap_or(0.0),
                                );
                                statistics_total_samples_duration.set(
                                    statistics.total_samples_duration().unwrap_or(0.0),
                                );
                            }
                            Ok(None) => {}
                            Err(statistics_error) => error.set(statistics_error.to_string()),
                        }
                    }
                },
                "Refresh remote audio statistics"
            }
            button {
                class: "create-remote-audio-analyser",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let created = track.read().as_ref().map(RemoteAudioTrack::create_analyser);
                    install_audio_analyser(created, analyser, analysis_observation, error);
                },
                "Create Remote Media Track audio analyser"
            }
            button {
                class: "read-remote-audio-analyser",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let analysis = analyser
                        .read()
                        .as_ref()
                        .map(AudioAnalysisObservation::read);
                    install_audio_analysis(analysis, analysis_observation, error);
                },
                "Read Remote Media Track audio analyser"
            }
            button {
                class: "close-remote-audio-analyser",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    let current = analyser.read().as_ref().cloned();
                    close_audio_analyser(current, analyser, error)
                },
                "Close Remote Media Track audio analyser"
            }
            button {
                class: "drop-remote-audio-analyser",
                hidden: true,
                r#type: "button",
                onclick: move |_| analyser.set(None),
                "Drop Remote Media Track audio analyser"
            }
            button {
                class: "drop-remote-audio-attachment",
                hidden: true,
                r#type: "button",
                onclick: move |_| {
                    attachment_enabled.set(false);
                    attachment.set(None);
                },
                "Drop remote audio attachment"
            }
        }
    }
}

fn waveform_fingerprint(waveform: &[f32]) -> String {
    let fingerprint = waveform
        .iter()
        .enumerate()
        .map(|(index, sample)| f64::from(*sample) * ((index % 31) + 1) as f64)
        .sum::<f64>();
    format!("{fingerprint:.8}")
}

#[component]
fn CapabilityItem(label: &'static str, supported: bool) -> Element {
    rsx! {
        div {
            class: if supported { "capability-item supported" } else { "capability-item" },
            "data-supported": supported,
            span { class: "capability-mark", if supported { "YES" } else { "NO" } }
            strong { "{label}" }
        }
    }
}

#[component]
fn GuideItem(step: &'static str, label: &'static str, complete: bool) -> Element {
    rsx! {
        li {
            class: if complete { "guide-item complete" } else { "guide-item" },
            "data-step": step,
            "data-complete": complete,
            span { class: "guide-mark", if complete { "OK" } else { "--" } }
            strong { "{label}" }
        }
    }
}

#[component]
fn StatusItem(label: &'static str, value: String, active: bool, test_id: &'static str) -> Element {
    rsx! {
        div { class: "status-item",
            span { class: if active { "status-dot active" } else { "status-dot" } }
            div {
                small { "{label}" }
                strong { id: "{test_id}", "{value}" }
            }
        }
    }
}
