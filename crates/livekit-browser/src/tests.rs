#[cfg(feature = "tracing")]
use crate::client::emit_tracing_record;
use crate::client::{finish_initialization, ClientInner, RuntimeInspection};
use crate::error::{
    classify_capture_error, classify_chat_message_send_error, classify_data_packet_send_error,
    classify_display_capture_error, classify_participant_update_error,
    classify_publisher_mutation_error, classify_subscription_permission_error,
};
use crate::room::{AdapterRoomSnapshot, AdapterTranscriptionSegment, RpcHandlerCapacity};
use crate::*;

use futures_executor::block_on;
use futures_util::{FutureExt, StreamExt};

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
            emit_tracing_record(&LiveKitLogRecord::test_record(
                level,
                "connected",
                serde_json::json!({ "room": "RM_test" }),
            ));
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
    let Some(TranscriptionSegmentStreamItem::Segment(segment)) = block_on(transcriptions.next())
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
        block_on(local_participant.send_data_packet(vec![0, 255], DataPacketDelivery::Reliable,)),
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
        block_on(local_participant.open_text_stream(TextStreamOptions::new("student", "lesson"))),
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
        serde_json::from_str(r#"{"type":"remoteParticipantLeft","identity":"student"}"#).unwrap();

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

    let first = block_on(track.publish(&first_session, VideoPublishOptions::default())).unwrap();
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

    let first = block_on(track.publish(&first_session, AudioPublishOptions::default())).unwrap();
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
    let video =
        LocalVideoTrack::test_track_with_source(Rc::new(Cell::new(0)), TrackSource::ScreenShare);
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
    let video =
        LocalVideoTrack::test_track_with_source(Rc::new(Cell::new(0)), TrackSource::ScreenShare);
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
    let video =
        LocalVideoTrack::test_track_with_source(Rc::new(Cell::new(0)), TrackSource::ScreenShare);
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
