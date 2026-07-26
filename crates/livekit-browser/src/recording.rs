use crate::*;

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
