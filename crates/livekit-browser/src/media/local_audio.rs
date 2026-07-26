use super::*;

use crate::room::{SessionInner, SessionLifecycle};

/// A browser-thread-local local audio capture owned independently of any Room Session.
///
/// Clones share one underlying browser capture. Explicit stop is terminal for every clone, and
/// dropping the final clone stops capture as a fallback.
#[derive(Clone)]
pub struct LocalAudioTrack {
    pub(crate) inner: Rc<LocalAudioTrackInner>,
}

impl PartialEq for LocalAudioTrack {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for LocalAudioTrack {}

impl LocalAudioTrack {
    /// Returns the authoritative source that created this track.
    pub fn source(&self) -> AudioTrackSource {
        self.inner.source
    }

    /// Whether capture has been stopped for every clone.
    pub fn is_stopped(&self) -> bool {
        self.inner.lifecycle.get() == LocalAudioTrackLifecycle::Stopped
    }

    /// Returns the unstable `livekit-client` Local Audio Track object.
    #[cfg(feature = "raw-js")]
    #[cfg_attr(docsrs, doc(cfg(feature = "raw-js")))]
    pub fn raw_js_media_track(&self) -> Result<wasm_bindgen::JsValue, Error> {
        if self.is_stopped() {
            return Err(Error::InvalidState {
                operation: "access raw Local Audio Track",
                reason: "capture has stopped",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        raw_js_handle(adapter::raw_local_media_track(self.inner.track_id, "audio"))
    }

    /// Installs an unstable JavaScript audio processor through the track mutation queue.
    #[cfg(feature = "raw-js")]
    #[cfg_attr(docsrs, doc(cfg(feature = "raw-js")))]
    pub fn set_raw_js_processor(
        &self,
        _processor: wasm_bindgen::JsValue,
        _show_processed_stream_locally: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if self.is_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "set raw Local Audio Track processor",
                    reason: "capture has stopped",
                })
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });
        #[cfg(target_arch = "wasm32")]
        raw_js_processor_operation(adapter::set_local_media_track_processor(
            self.inner.track_id,
            "audio",
            _processor,
            _show_processed_stream_locally,
        ))
    }

    /// Returns the currently installed unstable JavaScript audio processor.
    #[cfg(feature = "raw-js")]
    #[cfg_attr(docsrs, doc(cfg(feature = "raw-js")))]
    pub fn raw_js_processor(&self) -> Result<Option<wasm_bindgen::JsValue>, Error> {
        if self.is_stopped() {
            return Err(Error::InvalidState {
                operation: "access raw Local Audio Track processor",
                reason: "capture has stopped",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        {
            let value = adapter::raw_local_media_track_processor(self.inner.track_id, "audio")
                .map_err(AdapterError::from_cause)
                .map_err(Error::MediaControlFailed)?;
            Ok((!value.is_null() && !value.is_undefined()).then_some(value))
        }
    }

    /// Stops the currently installed unstable JavaScript audio processor.
    #[cfg(feature = "raw-js")]
    #[cfg_attr(docsrs, doc(cfg(feature = "raw-js")))]
    pub fn stop_raw_js_processor(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if self.is_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "stop raw Local Audio Track processor",
                    reason: "capture has stopped",
                })
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });
        #[cfg(target_arch = "wasm32")]
        raw_js_processor_operation(adapter::stop_local_media_track_processor(
            self.inner.track_id,
            "audio",
        ))
    }

    /// Returns the browser device ID currently producing this track.
    pub fn device_id(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, Error>> + 'static>> {
        if self.source() != AudioTrackSource::Microphone {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "inspect Local Audio Track device",
                    reason: "system audio has no selectable microphone device",
                })
            });
        }
        if self.is_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "inspect Local Audio Track device",
                    reason: "capture has stopped",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            local_media_device_id(
                adapter::local_audio_track_device_id(self.inner.track_id),
                "microphone device query returned a non-string ID",
            )
        }
    }

    /// Restarts this track with its currently committed microphone constraints.
    pub fn restart(&self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if let Err(error) = self.ensure_active_for_device_change("restart Local Audio Track") {
            return Box::pin(async move { Err(error) });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let inner = Rc::clone(&self.inner);
            start_local_media_device_change(
                move || {
                    inner.mark_stopped();
                },
                |callback| adapter::restart_local_audio_track(self.inner.track_id, callback),
            )
        }
    }

    /// Restarts this track with new typed microphone constraints.
    ///
    /// The existing Room Session, Track Publication, and mute state are kept. If applying the new
    /// constraints fails, the adapter restores the previously committed input.
    pub fn restart_with_options(
        &self,
        _options: MicrophoneCaptureOptions,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if let Err(error) = self.ensure_active_for_device_change("restart Local Audio Track") {
            return Box::pin(async move { Err(error) });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let raw_options = raw_js_options!(_options, raw_js_options);
            let options = match serde_wasm_bindgen::to_value(&_options) {
                Ok(options) => options,
                Err(error) => {
                    return Box::pin(async move {
                        Err(Error::AdapterFailure(AdapterError::contract(
                            error.to_string(),
                        )))
                    });
                }
            };
            let inner = Rc::clone(&self.inner);
            start_local_media_device_change(
                move || {
                    inner.mark_stopped();
                },
                |callback| {
                    adapter::restart_local_audio_track_with_options(
                        self.inner.track_id,
                        options,
                        raw_options,
                        callback,
                    )
                },
            )
        }
    }

    /// Switches this track to one exact microphone while retaining its other constraints.
    pub fn switch_device(
        &self,
        device_id: impl Into<String>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if let Err(error) = self.ensure_active_for_device_change("switch Local Audio Track device")
        {
            return Box::pin(async move { Err(error) });
        }
        let _device_id = device_id.into();

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let inner = Rc::clone(&self.inner);
            start_local_media_device_change(
                move || {
                    inner.mark_stopped();
                },
                |callback| {
                    adapter::switch_local_audio_track_device(
                        self.inner.track_id,
                        &_device_id,
                        callback,
                    )
                },
            )
        }
    }

    fn ensure_active_for_device_change(&self, operation: &'static str) -> Result<(), Error> {
        if self.source() != AudioTrackSource::Microphone {
            Err(Error::InvalidState {
                operation,
                reason: "system audio cannot restart or switch microphone devices",
            })
        } else if self.is_stopped() {
            Err(Error::InvalidState {
                operation,
                reason: "capture has stopped",
            })
        } else {
            Ok(())
        }
    }

    /// Returns why this capture ended, if it is already terminal.
    pub fn end_reason(&self) -> Option<LocalMediaTrackEndReason> {
        self.inner.end_state.reason.get()
    }

    /// Resolves when this capture becomes terminal.
    pub fn ended(&self) -> LocalMediaTrackEnded {
        LocalMediaTrackEnded {
            state: Rc::clone(&self.inner.end_state),
            waiter_id: None,
        }
    }

    /// Creates a typed level and waveform analyser for this exact audio source.
    pub fn create_analyser(&self) -> Result<AudioAnalyser, Error> {
        if self.is_stopped() {
            return Err(Error::InvalidState {
                operation: "create Local Media Track audio analyser",
                reason: "capture has stopped",
            });
        }

        let _source = AudioAnalyserSource::Local(Rc::downgrade(&self.inner));
        #[cfg(target_arch = "wasm32")]
        return create_audio_analyser(_source, |callback| {
            adapter::create_local_audio_analyser(self.inner.track_id, callback)
        });

        #[cfg(all(not(target_arch = "wasm32"), test))]
        return Ok(AudioAnalyser::test_analyser(_source));

        #[cfg(all(not(target_arch = "wasm32"), not(test)))]
        Err(Error::UnsupportedTarget)
    }

    /// Makes every clone terminal and stops the shared microphone capture.
    ///
    /// Terminal state is installed when this method is called. Await the returned local future for
    /// deterministic browser cleanup.
    pub fn stop(&self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        let _newly_stopped = self.inner.mark_stopped();
        #[cfg(target_arch = "wasm32")]
        if _newly_stopped || self.inner.stop_operation.borrow().is_none() {
            self.inner.start_stop();
        }

        let _inner = Rc::clone(&self.inner);
        Box::pin(async move {
            #[cfg(target_arch = "wasm32")]
            _inner
                .await_stop()
                .await
                .map_err(Error::MediaControlFailed)?;

            Ok(())
        })
    }

    /// Attaches this capture to one browser audio element without publishing it.
    pub fn attach(
        &self,
        _element: &web_sys::HtmlAudioElement,
    ) -> Result<AudioTrackAttachment, Error> {
        if self.is_stopped() {
            return Err(Error::InvalidState {
                operation: "attach Local Audio Track",
                reason: "capture has stopped",
            });
        }

        #[cfg(target_arch = "wasm32")]
        let attachment_id = adapter::attach_local_audio_track(self.inner.track_id, _element)
            .map_err(AdapterError::from_cause)
            .map_err(Error::MediaControlFailed)?;
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        {
            let inner = Rc::new(AudioTrackAttachmentInner {
                active: Cell::new(true),
                track: Rc::downgrade(&self.inner),
                track_id: self.inner.track_id,
                attachment_id,
            });
            self.inner
                .attachments
                .borrow_mut()
                .push(Rc::downgrade(&inner));
            Ok(AudioTrackAttachment { inner })
        }
    }

    /// Returns this track's current publication command handle, including an in-flight publish.
    ///
    /// This lets callers observe and recover a publication whose Rust operation future was
    /// abandoned even though its underlying JavaScript Promise continued.
    pub fn publication(&self) -> Option<LocalAudioTrackPublication> {
        self.inner.clear_terminal_publication();
        let inner = self
            .inner
            .replacement_publication
            .borrow()
            .as_ref()
            .cloned()
            .or_else(|| self.inner.publication.borrow().as_ref().cloned());
        inner.map(|inner| LocalAudioTrackPublication { inner })
    }

    /// Publishes this capture into one active Room Session.
    ///
    /// A Local Audio Track can have at most one active or in-flight publication association.
    pub fn publish(
        &self,
        session: &RoomSession,
        options: AudioPublishOptions,
    ) -> Pin<Box<dyn Future<Output = Result<LocalAudioTrackPublication, Error>> + 'static>> {
        if self.is_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "publish Local Audio Track",
                    reason: "capture has stopped",
                })
            });
        }
        if session.inner.lifecycle.get() != SessionLifecycle::Active {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "publish Local Audio Track",
                    reason: "Room Session is terminal",
                })
            });
        }
        if options.source != self.source() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "publish Local Audio Track",
                    reason: "publication source does not match captured media",
                })
            });
        }

        self.inner.clear_terminal_publication();
        let is_replacement = self
            .inner
            .publication
            .borrow()
            .as_ref()
            .map(|publication| publication.lifecycle.get())
            == Some(LocalAudioTrackPublicationLifecycle::Unpublishing);
        if self.inner.replacement_publication.borrow().is_some()
            || (self.inner.publication.borrow().is_some() && !is_replacement)
        {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "publish Local Audio Track",
                    reason: "track is already associated with a Room Session",
                })
            });
        }

        #[cfg(all(not(target_arch = "wasm32"), test))]
        {
            let publication = Rc::new(LocalAudioTrackPublicationInner {
                lifecycle: Cell::new(LocalAudioTrackPublicationLifecycle::Published),
                muted: Cell::new(self.inner.muted.get()),
                sid: RefCell::new(Some("TR_audio_test".into())),
                session: Rc::downgrade(&session.inner),
                track: Rc::downgrade(&self.inner),
            });
            self.inner
                .publication
                .borrow_mut()
                .replace(Rc::clone(&publication));
            Box::pin(async move { Ok(LocalAudioTrackPublication { inner: publication }) })
        }

        #[cfg(all(not(target_arch = "wasm32"), not(test)))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            self.start_publish(session, options, is_replacement)
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn start_publish(
        &self,
        session: &RoomSession,
        options: AudioPublishOptions,
        is_replacement: bool,
    ) -> Pin<Box<dyn Future<Output = Result<LocalAudioTrackPublication, Error>> + 'static>> {
        use wasm_bindgen::JsCast;

        let raw_options = raw_js_options!(options, raw_js_options);
        let options =
            match serde_wasm_bindgen::to_value(&AdapterAudioPublishOptions::from(&options)) {
                Ok(options) => options,
                Err(error) => {
                    return Box::pin(async move {
                        Err(Error::AdapterFailure(AdapterError::contract(
                            error.to_string(),
                        )))
                    });
                }
            };
        let publication = Rc::new(LocalAudioTrackPublicationInner {
            lifecycle: Cell::new(LocalAudioTrackPublicationLifecycle::Publishing),
            muted: Cell::new(self.inner.muted.get()),
            sid: RefCell::new(None),
            session: Rc::downgrade(&session.inner),
            track: Rc::downgrade(&self.inner),
            publication_id: self.inner.allocate_publication_id(),
            track_id: self.inner.track_id,
            room_id: session.inner.room_id,
        });
        if is_replacement {
            self.inner
                .replacement_publication
                .borrow_mut()
                .replace(Rc::clone(&publication));
        } else {
            self.inner
                .publication
                .borrow_mut()
                .replace(Rc::clone(&publication));
        }

        let callback_track = Rc::clone(&self.inner);
        let callback_publication = Rc::clone(&publication);
        let callback = wasm_bindgen::closure::Closure::once_into_js(
            move |success: bool, value: wasm_bindgen::JsValue| {
                if success {
                    if let Some(sid) = value.as_string() {
                        callback_track.clear_sdk_replacement_source();
                        if is_replacement {
                            callback_track.promote_replacement_publication(&callback_publication);
                        }
                        callback_publication.sid.borrow_mut().replace(sid);
                        if callback_publication.lifecycle.get()
                            == LocalAudioTrackPublicationLifecycle::Publishing
                        {
                            callback_publication
                                .lifecycle
                                .set(LocalAudioTrackPublicationLifecycle::Published);
                        }
                        return;
                    }
                }
                callback_publication
                    .lifecycle
                    .set(LocalAudioTrackPublicationLifecycle::Unpublished);
                callback_track.clear_publication(&callback_publication);
            },
        );
        let callback: js_sys::Function = callback.unchecked_into();
        let replacement_callback: &js_sys::Function = self
            .inner
            .publication_replacement_callback()
            .as_ref()
            .unchecked_ref();
        let operation = adapter::publish_local_audio_track(
            session.inner.room_id,
            self.inner.track_id,
            publication.publication_id,
            options,
            raw_options,
            replacement_callback,
            &callback,
        )
        .map_err(AdapterError::from_cause)
        .map_err(classify_publisher_mutation_error);
        let operation = match operation {
            Ok(operation) => operation,
            Err(error) => {
                publication
                    .lifecycle
                    .set(LocalAudioTrackPublicationLifecycle::Unpublished);
                self.inner.clear_publication(&publication);
                return Box::pin(async move { Err(error) });
            }
        };

        Box::pin(async move {
            let sid = wasm_bindgen_futures::JsFuture::from(operation)
                .await
                .map_err(AdapterError::from_cause)
                .map_err(classify_publisher_mutation_error)?
                .as_string()
                .ok_or_else(|| {
                    Error::AdapterFailure(AdapterError::contract(
                        "audio publication returned an invalid SID".into(),
                    ))
                })?;
            if publication.sid.borrow().as_deref() != Some(sid.as_str()) {
                return Err(Error::AdapterFailure(AdapterError::contract(
                    "audio publication completion did not install its SID".into(),
                )));
            }
            Ok(LocalAudioTrackPublication { inner: publication })
        })
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn test_track(stop_count: Rc<Cell<usize>>) -> Self {
        Self::test_track_with_source(stop_count, AudioTrackSource::Microphone)
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn test_track_with_source(
        stop_count: Rc<Cell<usize>>,
        source: AudioTrackSource,
    ) -> Self {
        Self {
            inner: Rc::new(LocalAudioTrackInner {
                lifecycle: Cell::new(LocalAudioTrackLifecycle::Active),
                source,
                end_state: LocalMediaTrackEndState::new(),
                muted: Cell::new(false),
                attachments: RefCell::new(Vec::new()),
                publication: RefCell::new(None),
                replacement_publication: RefCell::new(None),
                analysers: RefCell::new(Vec::new()),
                stop_count,
            }),
        }
    }
}

impl fmt::Debug for LocalAudioTrack {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalAudioTrack")
            .field("source", &self.source())
            .field("stopped", &self.is_stopped())
            .finish()
    }
}

pub(crate) struct LocalAudioTrackInner {
    pub(crate) lifecycle: Cell<LocalAudioTrackLifecycle>,
    source: AudioTrackSource,
    end_state: Rc<LocalMediaTrackEndState>,
    #[cfg(any(target_arch = "wasm32", test))]
    muted: Cell<bool>,
    pub(crate) attachments: RefCell<Vec<Weak<AudioTrackAttachmentInner>>>,
    publication: RefCell<Option<Rc<LocalAudioTrackPublicationInner>>>,
    replacement_publication: RefCell<Option<Rc<LocalAudioTrackPublicationInner>>>,
    pub(crate) analysers: RefCell<Vec<Weak<AudioAnalyserState>>>,
    #[cfg(target_arch = "wasm32")]
    next_publication_id: Cell<u32>,
    #[cfg(target_arch = "wasm32")]
    publication_replacement_callback: OnceCell<PublicationReplacementCallback>,
    #[cfg(target_arch = "wasm32")]
    sdk_replacement_source: RefCell<Option<Rc<LocalAudioTrackPublicationInner>>>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) track_id: u32,
    #[cfg(target_arch = "wasm32")]
    stop_operation: RefCell<Option<Result<js_sys::Promise, AdapterError>>>,
    #[cfg(target_arch = "wasm32")]
    ended_callback: RefCell<Option<wasm_bindgen::closure::Closure<dyn Fn()>>>,
    #[cfg(all(test, not(target_arch = "wasm32")))]
    stop_count: Rc<Cell<usize>>,
}

impl LocalAudioTrackInner {
    fn remove_attachment(&self, target: *const AudioTrackAttachmentInner) {
        self.attachments
            .borrow_mut()
            .retain(|attachment| attachment.as_ptr() != target && attachment.strong_count() > 0);
    }

    #[cfg(target_arch = "wasm32")]
    fn allocate_publication_id(&self) -> u32 {
        let publication_id = self.next_publication_id.get();
        self.next_publication_id
            .set(publication_id.checked_add(1).unwrap_or(1));
        publication_id
    }

    #[cfg(target_arch = "wasm32")]
    fn reconcile_sdk_publication(
        self: &Rc<Self>,
        expected_publication_id: u32,
        sid: Option<String>,
        muted: bool,
    ) -> u32 {
        let previous = if let Some(sid) = sid.as_ref() {
            if sid.is_empty() || self.publication.borrow().is_some() {
                return 0;
            }
            let Some(previous) = self.sdk_replacement_source.borrow_mut().take() else {
                return 0;
            };
            if previous.publication_id != expected_publication_id
                || previous
                    .session
                    .upgrade()
                    .is_none_or(|session| session.lifecycle.get() != SessionLifecycle::Active)
            {
                self.sdk_replacement_source.borrow_mut().replace(previous);
                return 0;
            }
            previous
        } else {
            let Some(previous) = self.publication.borrow_mut().take() else {
                return 0;
            };
            if previous.publication_id != expected_publication_id || previous.is_terminal() {
                self.publication.borrow_mut().replace(previous);
                return 0;
            }
            previous
                .lifecycle
                .set(LocalAudioTrackPublicationLifecycle::Unpublished);
            self.sdk_replacement_source.borrow_mut().replace(previous);
            return 0;
        };
        let publication_id = self.allocate_publication_id();
        let replacement = Rc::new(LocalAudioTrackPublicationInner {
            lifecycle: Cell::new(LocalAudioTrackPublicationLifecycle::Published),
            muted: Cell::new(muted),
            sid: RefCell::new(sid),
            session: previous.session.clone(),
            track: Rc::downgrade(self),
            publication_id,
            track_id: previous.track_id,
            room_id: previous.room_id,
        });
        self.publication.borrow_mut().replace(replacement);
        publication_id
    }

    #[cfg(target_arch = "wasm32")]
    fn clear_sdk_replacement_source(&self) {
        self.sdk_replacement_source.borrow_mut().take();
    }

    #[cfg(target_arch = "wasm32")]
    fn publication_replacement_callback(
        self: &Rc<Self>,
    ) -> &wasm_bindgen::closure::Closure<dyn Fn(u32, Option<String>, bool) -> u32> {
        self.publication_replacement_callback.get_or_init(|| {
            let track = Rc::downgrade(self);
            wasm_bindgen::closure::Closure::wrap(Box::new(move |publication_id, sid, muted| {
                track
                    .upgrade()
                    .map(|track| track.reconcile_sdk_publication(publication_id, sid, muted))
                    .unwrap_or(0)
            })
                as Box<dyn Fn(u32, Option<String>, bool) -> u32>)
        })
    }

    fn clear_terminal_publication(&self) {
        #[cfg(target_arch = "wasm32")]
        if self
            .sdk_replacement_source
            .borrow()
            .as_ref()
            .is_some_and(|publication| {
                publication
                    .session
                    .upgrade()
                    .is_none_or(|session| session.lifecycle.get() != SessionLifecycle::Active)
            })
        {
            self.sdk_replacement_source.borrow_mut().take();
        }
        let replacement_terminal = self
            .replacement_publication
            .borrow()
            .as_ref()
            .is_some_and(|publication| publication.association_released());
        if replacement_terminal {
            if let Some(publication) = self.replacement_publication.borrow_mut().take() {
                publication
                    .lifecycle
                    .set(LocalAudioTrackPublicationLifecycle::Unpublished);
            }
        }

        let terminal = self
            .publication
            .borrow()
            .as_ref()
            .is_some_and(|publication| publication.association_released());
        if terminal {
            if let Some(publication) = self.publication.borrow_mut().take() {
                publication
                    .lifecycle
                    .set(LocalAudioTrackPublicationLifecycle::Unpublished);
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn clear_publication(&self, publication: &Rc<LocalAudioTrackPublicationInner>) {
        let pending_matches = self
            .replacement_publication
            .borrow()
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, publication));
        if pending_matches {
            self.replacement_publication.borrow_mut().take();
        }
        let matches = self
            .publication
            .borrow()
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, publication));
        if matches {
            self.publication.borrow_mut().take();
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn promote_replacement_publication(&self, publication: &Rc<LocalAudioTrackPublicationInner>) {
        let matches = self
            .replacement_publication
            .borrow()
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, publication));
        if matches {
            self.replacement_publication.borrow_mut().take();
            self.publication
                .borrow_mut()
                .replace(Rc::clone(publication));
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn start_stop(self: &Rc<Self>) {
        let operation =
            adapter::stop_local_audio_track(self.track_id).map_err(AdapterError::from_cause);
        self.stop_operation.borrow_mut().replace(operation);
    }

    #[cfg(target_arch = "wasm32")]
    async fn await_stop(&self) -> Result<(), AdapterError> {
        let operation = self.stop_operation.borrow().clone();
        let Some(operation) = operation else {
            return Ok(());
        };
        let result = match operation {
            Ok(operation) => wasm_bindgen_futures::JsFuture::from(operation)
                .await
                .map(|_| ())
                .map_err(AdapterError::from_cause),
            Err(error) => Err(error),
        };
        if result.is_err() {
            self.stop_operation.borrow_mut().take();
        }
        result
    }

    fn mark_stopped(&self) -> bool {
        if self.lifecycle.replace(LocalAudioTrackLifecycle::Stopped)
            == LocalAudioTrackLifecycle::Stopped
        {
            return false;
        }

        self.end_state.finish(LocalMediaTrackEndReason::Stopped);
        terminate_audio_analysers(&self.analysers);
        for attachment in self.attachments.borrow_mut().drain(..) {
            if let Some(attachment) = attachment.upgrade() {
                attachment.active.set(false);
            }
        }
        #[cfg(all(test, not(target_arch = "wasm32")))]
        self.stop_count.set(self.stop_count.get() + 1);
        true
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn mark_browser_ended(&self) {
        if self.lifecycle.replace(LocalAudioTrackLifecycle::Stopped)
            == LocalAudioTrackLifecycle::Stopped
        {
            return;
        }
        self.end_state
            .finish(LocalMediaTrackEndReason::BrowserEnded);
        terminate_audio_analysers(&self.analysers);
        for attachment in self.attachments.borrow_mut().drain(..) {
            if let Some(attachment) = attachment.upgrade() {
                attachment.active.set(false);
            }
        }
        if let Some(publication) = self.replacement_publication.borrow_mut().take() {
            publication
                .lifecycle
                .set(LocalAudioTrackPublicationLifecycle::Unpublished);
        }
        if let Some(publication) = self.publication.borrow_mut().take() {
            publication
                .lifecycle
                .set(LocalAudioTrackPublicationLifecycle::Unpublished);
        }
    }
}

impl Drop for LocalAudioTrackInner {
    fn drop(&mut self) {
        self.mark_stopped();
        #[cfg(target_arch = "wasm32")]
        adapter::drop_local_audio_track(self.track_id);
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum LocalAudioTrackLifecycle {
    Active,
    Stopped,
}

/// A typed local-audio preview attachment with explicit and drop cleanup.
pub struct AudioTrackAttachment {
    inner: Rc<AudioTrackAttachmentInner>,
}

impl AudioTrackAttachment {
    /// Whether this guard still owns an active browser element attachment.
    pub fn is_active(&self) -> bool {
        self.inner.active.get()
            && self
                .inner
                .track
                .upgrade()
                .is_some_and(|track| track.lifecycle.get() == LocalAudioTrackLifecycle::Active)
    }

    /// Detaches the audio element without stopping or publishing capture.
    pub fn close(&self) -> Result<(), Error> {
        if !self.inner.active.get() {
            return Ok(());
        }

        #[cfg(target_arch = "wasm32")]
        adapter::detach_local_audio_track(self.inner.track_id, self.inner.attachment_id)
            .map_err(AdapterError::from_cause)
            .map_err(Error::MediaControlFailed)?;

        self.inner.active.set(false);
        if let Some(track) = self.inner.track.upgrade() {
            track.remove_attachment(Rc::as_ptr(&self.inner));
        }
        Ok(())
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn test_attachment(track: &LocalAudioTrack) -> Self {
        let inner = Rc::new(AudioTrackAttachmentInner {
            active: Cell::new(true),
            track: Rc::downgrade(&track.inner),
        });
        track
            .inner
            .attachments
            .borrow_mut()
            .push(Rc::downgrade(&inner));
        Self { inner }
    }
}

impl fmt::Debug for AudioTrackAttachment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AudioTrackAttachment")
            .field("active", &self.is_active())
            .finish()
    }
}

pub(crate) struct AudioTrackAttachmentInner {
    active: Cell<bool>,
    track: Weak<LocalAudioTrackInner>,
    #[cfg(target_arch = "wasm32")]
    track_id: u32,
    #[cfg(target_arch = "wasm32")]
    attachment_id: u32,
}

impl Drop for AudioTrackAttachmentInner {
    fn drop(&mut self) {
        if self.active.replace(false) {
            #[cfg(target_arch = "wasm32")]
            adapter::drop_local_audio_track_attachment(self.track_id, self.attachment_id);
        }
        if let Some(track) = self.track.upgrade() {
            track.remove_attachment(self as *const _);
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn parse_local_audio_track_id(value: wasm_bindgen::JsValue) -> Result<u32, AdapterError> {
    value
        .as_f64()
        .filter(|id| id.is_finite() && id.fract() == 0.0 && *id > 0.0)
        .map(|id| id as u32)
        .ok_or_else(|| {
            AdapterError::contract(
                "microphone capture returned an invalid Local Audio Track ID".into(),
            )
        })
}

#[cfg(target_arch = "wasm32")]
fn create_local_audio_track(track_id: u32, source: AudioTrackSource) -> LocalAudioTrack {
    LocalAudioTrack {
        inner: Rc::new(LocalAudioTrackInner {
            lifecycle: Cell::new(LocalAudioTrackLifecycle::Active),
            source,
            end_state: LocalMediaTrackEndState::new(),
            muted: Cell::new(false),
            attachments: RefCell::new(Vec::new()),
            publication: RefCell::new(None),
            replacement_publication: RefCell::new(None),
            analysers: RefCell::new(Vec::new()),
            next_publication_id: Cell::new(1),
            publication_replacement_callback: OnceCell::new(),
            sdk_replacement_source: RefCell::new(None),
            track_id,
            stop_operation: RefCell::new(None),
            ended_callback: RefCell::new(None),
        }),
    }
}

#[cfg(target_arch = "wasm32")]
fn finish_microphone_capture(result: Result<u32, AdapterError>) -> Result<LocalAudioTrack, Error> {
    result
        .map(|track_id| create_local_audio_track(track_id, AudioTrackSource::Microphone))
        .map_err(classify_capture_error)
}

#[cfg(target_arch = "wasm32")]
fn drop_captured_audio(track_id: u32) {
    adapter::drop_local_audio_track(track_id);
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn capture_microphone_audio_track(
    options: MicrophoneCaptureOptions,
) -> Pin<Box<dyn Future<Output = Result<LocalAudioTrack, Error>> + 'static>> {
    use wasm_bindgen::JsCast;

    let state = BrowserCaptureState::new();
    let callback_state = Rc::clone(&state);
    let callback = wasm_bindgen::closure::Closure::once_into_js(
        move |success: bool, value: wasm_bindgen::JsValue| {
            let result = if success {
                parse_local_audio_track_id(value)
            } else {
                Err(AdapterError::from_cause(value))
            };
            complete_browser_capture(&callback_state, result, drop_captured_audio);
        },
    );
    let callback: js_sys::Function = callback.unchecked_into();
    let raw_options = raw_js_options!(options, raw_js_options);
    let options = match serde_wasm_bindgen::to_value(&options) {
        Ok(options) => options,
        Err(error) => {
            state
                .result
                .borrow_mut()
                .replace(Err(AdapterError::contract(error.to_string())));
            return Box::pin(BrowserCaptureFuture {
                state,
                finish: finish_microphone_capture,
                cleanup: drop_captured_audio,
            });
        }
    };
    if let Err(cause) = adapter::create_microphone_audio_track(options, raw_options, &callback) {
        state
            .result
            .borrow_mut()
            .replace(Err(AdapterError::from_cause(cause)));
    }

    Box::pin(BrowserCaptureFuture {
        state,
        finish: finish_microphone_capture,
        cleanup: drop_captured_audio,
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn capture_microphone_audio_track(
    _options: MicrophoneCaptureOptions,
) -> Pin<Box<dyn Future<Output = Result<LocalAudioTrack, Error>> + 'static>> {
    Box::pin(async { Err(Error::UnsupportedTarget) })
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Copy, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
pub(crate) struct AdapterScreenCapture {
    video_track_id: u32,
    audio_track_id: Option<u32>,
}

#[cfg(target_arch = "wasm32")]
fn finish_screen_capture(
    result: Result<AdapterScreenCapture, AdapterError>,
) -> Result<ScreenCapture, Error> {
    match result {
        Ok(capture) => create_screen_capture(capture).map_err(Error::AdapterFailure),
        Err(error) => Err(classify_display_capture_error(error)),
    }
}

#[cfg(target_arch = "wasm32")]
fn drop_captured_screen(capture: AdapterScreenCapture) {
    adapter::drop_local_video_track(capture.video_track_id);
    if let Some(track_id) = capture.audio_track_id {
        adapter::drop_local_audio_track(track_id);
    }
}

#[cfg(target_arch = "wasm32")]
fn create_screen_capture(capture: AdapterScreenCapture) -> Result<ScreenCapture, AdapterError> {
    use wasm_bindgen::JsCast;

    let display_video = create_local_video_track(capture.video_track_id, TrackSource::ScreenShare);
    let video_inner = Rc::clone(&display_video.inner);
    let video_weak = Rc::downgrade(&video_inner);
    let video_callback = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
        if let Some(inner) = video_weak.upgrade() {
            inner.mark_browser_ended();
        }
    }) as Box<dyn Fn()>);
    adapter::observe_local_video_track_ended(
        capture.video_track_id,
        video_callback.as_ref().unchecked_ref(),
    )
    .map_err(AdapterError::from_cause)?;
    video_inner
        .ended_callback
        .borrow_mut()
        .replace(video_callback);

    let system_audio = capture
        .audio_track_id
        .map(|track_id| {
            let track = create_local_audio_track(track_id, AudioTrackSource::ScreenShareAudio);
            let audio_inner = Rc::clone(&track.inner);
            let audio_weak = Rc::downgrade(&audio_inner);
            let callback = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
                if let Some(inner) = audio_weak.upgrade() {
                    inner.mark_browser_ended();
                }
            }) as Box<dyn Fn()>);
            adapter::observe_local_audio_track_ended(track_id, callback.as_ref().unchecked_ref())
                .map_err(AdapterError::from_cause)?;
            audio_inner.ended_callback.borrow_mut().replace(callback);
            Ok(track)
        })
        .transpose()?;

    Ok(ScreenCapture {
        display_video,
        system_audio,
    })
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn capture_screen(
    options: ScreenCaptureOptions,
) -> Pin<Box<dyn Future<Output = Result<ScreenCapture, Error>> + 'static>> {
    use wasm_bindgen::JsCast;

    let state = BrowserCaptureState::new();
    let callback_state = Rc::clone(&state);
    let callback = wasm_bindgen::closure::Closure::once_into_js(
        move |success: bool, value: wasm_bindgen::JsValue| {
            let result = if success {
                serde_wasm_bindgen::from_value::<AdapterScreenCapture>(value)
                    .map_err(|error| AdapterError::contract(error.to_string()))
            } else {
                Err(AdapterError::from_cause(value))
            };
            complete_browser_capture(&callback_state, result, drop_captured_screen);
        },
    );
    let callback: js_sys::Function = callback.unchecked_into();
    let raw_options = raw_js_options!(options, raw_js_options);
    let options = match serde_wasm_bindgen::to_value(&options) {
        Ok(options) => options,
        Err(error) => {
            state
                .result
                .borrow_mut()
                .replace(Err(AdapterError::contract(error.to_string())));
            return Box::pin(BrowserCaptureFuture {
                state,
                finish: finish_screen_capture,
                cleanup: drop_captured_screen,
            });
        }
    };
    if let Err(cause) = adapter::create_screen_capture(options, raw_options, &callback) {
        state
            .result
            .borrow_mut()
            .replace(Err(AdapterError::from_cause(cause)));
    }

    Box::pin(BrowserCaptureFuture {
        state,
        finish: finish_screen_capture,
        cleanup: drop_captured_screen,
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn capture_screen(
    _options: ScreenCaptureOptions,
) -> Pin<Box<dyn Future<Output = Result<ScreenCapture, Error>> + 'static>> {
    Box::pin(async { Err(Error::UnsupportedTarget) })
}

/// A browser-thread-local command handle for one Local Audio Track Publication.
#[derive(Clone)]
pub struct LocalAudioTrackPublication {
    inner: Rc<LocalAudioTrackPublicationInner>,
}

impl LocalAudioTrackPublication {
    /// Returns the LiveKit publication SID after publishing completes.
    pub fn sid(&self) -> Option<String> {
        self.inner.sid.borrow().clone()
    }

    /// Whether the publication has been muted without being removed.
    pub fn is_muted(&self) -> bool {
        self.inner.muted.get()
    }

    /// Whether this command handle can no longer control its original publication.
    pub fn is_terminal(&self) -> bool {
        self.inner.is_terminal()
    }

    /// Returns the unstable `livekit-client` Local Track Publication object.
    #[cfg(feature = "raw-js")]
    #[cfg_attr(docsrs, doc(cfg(feature = "raw-js")))]
    pub fn raw_js_publication(&self) -> Result<wasm_bindgen::JsValue, Error> {
        if self.is_terminal() || self.sid().is_none() {
            return Err(Error::InvalidState {
                operation: "access raw Local Audio Track Publication",
                reason: "Track Publication is pending or terminal",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        raw_js_handle(adapter::raw_local_publication(
            self.inner.track_id,
            self.inner.publication_id,
            "audio",
        ))
    }

    /// Returns the browser's unstable raw `RTCStatsReport` for this publication.
    #[cfg(feature = "raw-js")]
    #[cfg_attr(docsrs, doc(cfg(feature = "raw-js")))]
    pub fn raw_js_rtc_stats_report(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<wasm_bindgen::JsValue>, Error>> + 'static>> {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "query raw Local Audio Track RTC statistics",
                    reason: "Track Publication is terminal",
                })
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });
        #[cfg(target_arch = "wasm32")]
        raw_js_statistics(adapter::raw_local_rtc_stats_report(
            self.inner.track_id,
            self.inner.publication_id,
            "audio",
        ))
    }

    /// Queries the latest normalized sender statistics without changing publication state.
    pub fn sender_statistics(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<AudioSenderStatistics>, Error>> + 'static>> {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "query Local Audio Track sender statistics",
                    reason: "Track Publication is terminal",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            deserialize_statistics(adapter::local_audio_sender_statistics(
                self.inner.track_id,
                self.inner.publication_id,
            ))
        }
    }

    /// Mutes or unmutes while keeping the Track Publication alive.
    pub fn set_muted(
        &self,
        _muted: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if !_muted && self.inner.capture_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "unmute Track Publication",
                    reason: "Local Audio Track capture has stopped",
                })
            });
        }
        if let Err(error) = self.inner.begin_mute_control() {
            return Box::pin(async move { Err(error) });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;

            let publication = Rc::clone(&self.inner);
            let callback_publication = Rc::clone(&publication);
            let muted = _muted;
            let callback = wasm_bindgen::closure::Closure::once_into_js(move |success: bool| {
                if success {
                    callback_publication.muted.set(muted);
                    if let Some(track) = callback_publication.track.upgrade() {
                        track.muted.set(muted);
                    }
                }
            });
            let callback: js_sys::Function = callback.unchecked_into();
            let operation = adapter::set_local_audio_track_muted(
                publication.track_id,
                publication.publication_id,
                _muted,
                &callback,
            )
            .map_err(AdapterError::from_cause)
            .map_err(classify_publisher_mutation_error);
            let operation = match operation {
                Ok(operation) => operation,
                Err(error) => return Box::pin(async move { Err(error) }),
            };
            Box::pin(async move {
                wasm_bindgen_futures::JsFuture::from(operation)
                    .await
                    .map_err(AdapterError::from_cause)
                    .map_err(classify_publisher_mutation_error)?;
                Ok(())
            })
        }
    }

    /// Removes the Track Publication while keeping microphone capture alive.
    pub fn unpublish(&self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        let _previous_lifecycle = match self.inner.begin_unpublish() {
            Ok(previous) => previous,
            Err(error) => return Box::pin(async move { Err(error) }),
        };

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;

            let publication = Rc::clone(&self.inner);
            let callback_track = publication.track.clone();
            let callback_publication = Rc::clone(&publication);
            let callback = wasm_bindgen::closure::Closure::once_into_js(move |success: bool| {
                if success {
                    callback_publication
                        .lifecycle
                        .set(LocalAudioTrackPublicationLifecycle::Unpublished);
                    if let Some(track) = callback_track.upgrade() {
                        track.clear_publication(&callback_publication);
                    }
                } else if callback_publication.lifecycle.get()
                    != LocalAudioTrackPublicationLifecycle::Unpublished
                {
                    callback_publication
                        .lifecycle
                        .set(LocalAudioTrackPublicationLifecycle::Published);
                }
            });
            let callback: js_sys::Function = callback.unchecked_into();
            let operation = adapter::unpublish_local_audio_track(
                publication.room_id,
                publication.track_id,
                publication.publication_id,
                &callback,
            )
            .map_err(AdapterError::from_cause)
            .map_err(classify_publisher_mutation_error);
            let operation = match operation {
                Ok(operation) => operation,
                Err(error) => {
                    publication.lifecycle.set(_previous_lifecycle);
                    return Box::pin(async move { Err(error) });
                }
            };
            Box::pin(async move {
                wasm_bindgen_futures::JsFuture::from(operation)
                    .await
                    .map_err(AdapterError::from_cause)
                    .map_err(classify_publisher_mutation_error)?;
                Ok(())
            })
        }
    }
}

impl fmt::Debug for LocalAudioTrackPublication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalAudioTrackPublication")
            .field("sid", &self.sid())
            .field("muted", &self.is_muted())
            .field("terminal", &self.is_terminal())
            .finish()
    }
}

struct LocalAudioTrackPublicationInner {
    lifecycle: Cell<LocalAudioTrackPublicationLifecycle>,
    muted: Cell<bool>,
    sid: RefCell<Option<String>>,
    session: Weak<SessionInner>,
    track: Weak<LocalAudioTrackInner>,
    #[cfg(target_arch = "wasm32")]
    publication_id: u32,
    #[cfg(target_arch = "wasm32")]
    track_id: u32,
    #[cfg(target_arch = "wasm32")]
    room_id: u32,
}

impl LocalAudioTrackPublicationInner {
    fn capture_stopped(&self) -> bool {
        self.track
            .upgrade()
            .is_none_or(|track| track.lifecycle.get() == LocalAudioTrackLifecycle::Stopped)
    }

    fn is_terminal(&self) -> bool {
        self.lifecycle.get() == LocalAudioTrackPublicationLifecycle::Unpublished
            || self
                .session
                .upgrade()
                .is_none_or(|session| session.lifecycle.get() != SessionLifecycle::Active)
    }

    fn association_released(&self) -> bool {
        self.lifecycle.get() == LocalAudioTrackPublicationLifecycle::Unpublished
            || self
                .session
                .upgrade()
                .is_none_or(|session| session.lifecycle.get() == SessionLifecycle::Left)
    }

    fn begin_mute_control(&self) -> Result<(), Error> {
        if self.is_terminal() {
            return Err(Error::InvalidState {
                operation: "change Track Publication mute",
                reason: "Track Publication is terminal",
            });
        }
        Ok(())
    }

    fn begin_unpublish(&self) -> Result<LocalAudioTrackPublicationLifecycle, Error> {
        if self.is_terminal()
            || self.lifecycle.get() == LocalAudioTrackPublicationLifecycle::Unpublishing
        {
            return Err(Error::InvalidState {
                operation: "unpublish Local Audio Track",
                reason: "Track Publication is terminal or being unpublished",
            });
        }
        let previous = self
            .lifecycle
            .replace(LocalAudioTrackPublicationLifecycle::Unpublishing);
        Ok(previous)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LocalAudioTrackPublicationLifecycle {
    #[cfg(target_arch = "wasm32")]
    Publishing,
    #[cfg(any(target_arch = "wasm32", test))]
    Published,
    Unpublishing,
    Unpublished,
}
