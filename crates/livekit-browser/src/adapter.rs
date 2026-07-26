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
    pub fn raw_local_media_track_processor(track_id: u32, kind: &str) -> Result<JsValue, JsValue>;

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
