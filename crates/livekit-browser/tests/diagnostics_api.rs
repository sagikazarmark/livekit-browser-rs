use livekit_browser::{InitializeOptions, LiveKitLogLevel, LiveKitLogRecord};

#[test]
fn typed_log_callback_api_is_available_without_javascript_values() {
    let options = InitializeOptions::default().with_log_callback(|record| {
        let _: LiveKitLogLevel = record.level();
        let _: &str = record.message();
        let _: &serde_json::Value = record.context();
    });

    let _ = options;
    let _ = std::mem::size_of::<LiveKitLogRecord>();
}

#[cfg(feature = "tracing")]
#[test]
fn tracing_can_be_selected_at_initialization() {
    let _ = InitializeOptions::default().with_tracing();
}

#[cfg(feature = "raw-js")]
#[test]
fn raw_js_feature_covers_the_documented_escape_hatches() {
    use livekit_browser::{
        AudioPublishOptions, CameraCaptureOptions, Error, LocalAudioTrack,
        LocalAudioTrackPublication, LocalParticipant, LocalVideoTrack, LocalVideoTrackPublication,
        MicrophoneCaptureOptions, RemoteAudioTrack, RemoteMediaTrack, RemoteParticipant,
        RemoteTrackPublication, RemoteVideoTrack, RoomOptions, RoomSession, ScreenCaptureOptions,
        VideoPublishOptions,
    };
    use wasm_bindgen::JsValue;

    #[allow(clippy::too_many_arguments)]
    fn compile_raw_api(
        raw: JsValue,
        error: &Error,
        session: &RoomSession,
        local_participant: &LocalParticipant,
        remote_participant: &RemoteParticipant,
        remote_publication: &RemoteTrackPublication,
        local_video: &LocalVideoTrack,
        local_audio: &LocalAudioTrack,
        local_video_publication: &LocalVideoTrackPublication,
        local_audio_publication: &LocalAudioTrackPublication,
        remote_video: &RemoteVideoTrack,
        remote_audio: &RemoteAudioTrack,
        remote_media: &RemoteMediaTrack,
    ) {
        let _ = RoomOptions::default()
            .with_raw_js_room_options(raw.clone())
            .with_raw_js_connect_options(raw.clone());
        let _ = CameraCaptureOptions::default().with_raw_js_options(raw.clone());
        let _ = MicrophoneCaptureOptions::default().with_raw_js_options(raw.clone());
        let _ = ScreenCaptureOptions::default().with_raw_js_options(raw.clone());
        let _ = VideoPublishOptions::default().with_raw_js_options(raw.clone());
        let _ = AudioPublishOptions::default().with_raw_js_options(raw.clone());

        let _ = error.raw_js_cause();
        let _ = session.raw_js_room();
        let _ = local_participant.raw_js_participant();
        let _ = remote_participant.raw_js_participant();
        let _ = remote_publication.raw_js_publication();
        let _ = local_video.raw_js_media_track();
        let _ = local_audio.raw_js_media_track();
        let _ = local_video_publication.raw_js_publication();
        let _ = local_audio_publication.raw_js_publication();
        let _ = remote_video.raw_js_media_track();
        let _ = remote_audio.raw_js_media_track();
        let _ = remote_media.raw_js_media_track();

        let _ = local_video.set_raw_js_processor(raw.clone(), false);
        let _ = local_video.raw_js_processor();
        let _ = local_video.stop_raw_js_processor();
        let _ = local_audio.set_raw_js_processor(raw, false);
        let _ = local_audio.raw_js_processor();
        let _ = local_audio.stop_raw_js_processor();

        let _ = local_video_publication.raw_js_rtc_stats_report();
        let _ = local_audio_publication.raw_js_rtc_stats_report();
        let _ = remote_video.raw_js_rtc_stats_report();
        let _ = remote_audio.raw_js_rtc_stats_report();
    }

    let _ = compile_raw_api;
}
