use super::*;

use crate::room::{SessionInner, SessionLifecycle};

/// A browser-thread-local local video capture owned independently of any Room Session.
///
/// Clones share one underlying browser capture. Explicit stop is terminal for every clone, and
/// dropping the final clone stops capture as a fallback.
#[derive(Clone)]
pub struct LocalVideoTrack {
    pub(crate) inner: Rc<LocalVideoTrackInner>,
}

impl PartialEq for LocalVideoTrack {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for LocalVideoTrack {}

impl LocalVideoTrack {
    /// Returns the authoritative source that created this track.
    pub fn source(&self) -> TrackSource {
        self.inner.source
    }

    /// Whether capture has been stopped for every clone.
    pub fn is_stopped(&self) -> bool {
        self.inner.lifecycle.get() == LocalVideoTrackLifecycle::Stopped
    }

    /// Returns the unstable `livekit-client` Local Video Track object.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_media_track(&self) -> Result<wasm_bindgen::JsValue, Error> {
        if self.is_stopped() {
            return Err(Error::InvalidState {
                operation: "access raw Local Video Track",
                reason: "capture has stopped",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        raw_js_handle(adapter::raw_local_media_track(self.inner.track_id, "video"))
    }

    /// Installs an unstable JavaScript video processor through the track mutation queue.
    #[cfg(feature = "raw-js")]
    pub fn set_raw_js_processor(
        &self,
        _processor: wasm_bindgen::JsValue,
        _show_processed_stream_locally: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if self.is_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "set raw Local Video Track processor",
                    reason: "capture has stopped",
                })
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });
        #[cfg(target_arch = "wasm32")]
        raw_js_processor_operation(adapter::set_local_media_track_processor(
            self.inner.track_id,
            "video",
            _processor,
            _show_processed_stream_locally,
        ))
    }

    /// Returns the currently installed unstable JavaScript video processor.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_processor(&self) -> Result<Option<wasm_bindgen::JsValue>, Error> {
        if self.is_stopped() {
            return Err(Error::InvalidState {
                operation: "access raw Local Video Track processor",
                reason: "capture has stopped",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        {
            let value = adapter::raw_local_media_track_processor(self.inner.track_id, "video")
                .map_err(AdapterError::from_cause)
                .map_err(Error::MediaControlFailed)?;
            Ok((!value.is_null() && !value.is_undefined()).then_some(value))
        }
    }

    /// Stops the currently installed unstable JavaScript video processor.
    #[cfg(feature = "raw-js")]
    pub fn stop_raw_js_processor(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if self.is_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "stop raw Local Video Track processor",
                    reason: "capture has stopped",
                })
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });
        #[cfg(target_arch = "wasm32")]
        raw_js_processor_operation(adapter::stop_local_media_track_processor(
            self.inner.track_id,
            "video",
        ))
    }

    /// Returns the browser device ID currently producing this track.
    pub fn device_id(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, Error>> + 'static>> {
        if self.source() != TrackSource::Camera {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "inspect Local Video Track device",
                    reason: "display capture has no selectable camera device",
                })
            });
        }
        if self.is_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "inspect Local Video Track device",
                    reason: "capture has stopped",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            local_media_device_id(
                adapter::local_video_track_device_id(self.inner.track_id),
                "camera device query returned a non-string ID",
            )
        }
    }

    /// Restarts this track with its currently committed camera constraints.
    pub fn restart(&self) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if let Err(error) = self.ensure_active_for_device_change("restart Local Video Track") {
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
                |callback| adapter::restart_local_video_track(self.inner.track_id, callback),
            )
        }
    }

    /// Restarts this track with new typed camera constraints.
    ///
    /// The existing Room Session, Track Publication, mute state, and preview attachment are kept.
    /// If applying the new constraints fails, the adapter restores the previously committed input.
    pub fn restart_with_options(
        &self,
        _options: CameraCaptureOptions,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if let Err(error) = self.ensure_active_for_device_change("restart Local Video Track") {
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
                    adapter::restart_local_video_track_with_options(
                        self.inner.track_id,
                        options,
                        raw_options,
                        callback,
                    )
                },
            )
        }
    }

    /// Switches this track to one exact camera while retaining its other constraints.
    pub fn switch_device(
        &self,
        device_id: impl Into<String>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if let Err(error) = self.ensure_active_for_device_change("switch Local Video Track device")
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
                    adapter::switch_local_video_track_device(
                        self.inner.track_id,
                        &_device_id,
                        callback,
                    )
                },
            )
        }
    }

    fn ensure_active_for_device_change(&self, operation: &'static str) -> Result<(), Error> {
        if self.source() != TrackSource::Camera {
            Err(Error::InvalidState {
                operation,
                reason: "display capture cannot restart or switch camera devices",
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

    /// Makes every clone terminal and stops the shared camera capture.
    ///
    /// Terminal state and attachment termination are installed when this method is called. Await
    /// the returned local future for deterministic browser cleanup.
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

    /// Attaches this capture to one browser video element without publishing it.
    pub fn attach(
        &self,
        _element: &web_sys::HtmlVideoElement,
    ) -> Result<VideoTrackAttachment, Error> {
        if self.is_stopped() {
            return Err(Error::InvalidState {
                operation: "attach Local Video Track",
                reason: "capture has stopped",
            });
        }

        #[cfg(target_arch = "wasm32")]
        let attachment_id = adapter::attach_local_video_track(self.inner.track_id, _element)
            .map_err(AdapterError::from_cause)
            .map_err(Error::MediaControlFailed)?;
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        {
            let inner = Rc::new(VideoTrackAttachmentInner {
                active: Cell::new(true),
                track: Rc::downgrade(&self.inner),
                track_id: self.inner.track_id,
                attachment_id,
            });
            self.inner
                .attachments
                .borrow_mut()
                .push(Rc::downgrade(&inner));
            Ok(VideoTrackAttachment { inner })
        }
    }

    /// Returns this track's current publication command handle, including an in-flight publish.
    ///
    /// This lets callers observe and recover a publication whose Rust operation future was
    /// abandoned even though its underlying JavaScript Promise continued.
    pub fn publication(&self) -> Option<LocalVideoTrackPublication> {
        self.inner.clear_terminal_publication();
        let inner = self
            .inner
            .replacement_publication
            .borrow()
            .as_ref()
            .cloned()
            .or_else(|| self.inner.publication.borrow().as_ref().cloned());
        inner.map(|inner| LocalVideoTrackPublication { inner })
    }

    /// Publishes this capture into one active Room Session.
    ///
    /// A Local Video Track can have at most one active or in-flight publication association.
    pub fn publish(
        &self,
        session: &RoomSession,
        options: VideoPublishOptions,
    ) -> Pin<Box<dyn Future<Output = Result<LocalVideoTrackPublication, Error>> + 'static>> {
        if self.is_stopped() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "publish Local Video Track",
                    reason: "capture has stopped",
                })
            });
        }
        if session.inner.lifecycle.get() != SessionLifecycle::Active {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "publish Local Video Track",
                    reason: "Room Session is terminal",
                })
            });
        }
        if options.source != self.source() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "publish Local Video Track",
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
            == Some(LocalVideoTrackPublicationLifecycle::Unpublishing);
        if self.inner.replacement_publication.borrow().is_some()
            || (self.inner.publication.borrow().is_some() && !is_replacement)
        {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "publish Local Video Track",
                    reason: "track is already associated with a Room Session",
                })
            });
        }

        #[cfg(all(not(target_arch = "wasm32"), test))]
        {
            let publication = Rc::new(LocalVideoTrackPublicationInner {
                lifecycle: Cell::new(LocalVideoTrackPublicationLifecycle::Published),
                muted: Cell::new(self.inner.muted.get()),
                sid: RefCell::new(Some("TR_test".into())),
                session: Rc::downgrade(&session.inner),
                track: Rc::downgrade(&self.inner),
            });
            self.inner
                .publication
                .borrow_mut()
                .replace(Rc::clone(&publication));
            Box::pin(async move { Ok(LocalVideoTrackPublication { inner: publication }) })
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
        options: VideoPublishOptions,
        is_replacement: bool,
    ) -> Pin<Box<dyn Future<Output = Result<LocalVideoTrackPublication, Error>> + 'static>> {
        use wasm_bindgen::JsCast;

        let raw_options = raw_js_options!(options, raw_js_options);
        let options =
            match serde_wasm_bindgen::to_value(&AdapterVideoPublishOptions::from(&options)) {
                Ok(options) => options,
                Err(error) => {
                    return Box::pin(async move {
                        Err(Error::AdapterFailure(AdapterError::contract(
                            error.to_string(),
                        )))
                    });
                }
            };
        let publication = Rc::new(LocalVideoTrackPublicationInner {
            lifecycle: Cell::new(LocalVideoTrackPublicationLifecycle::Publishing),
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
                            == LocalVideoTrackPublicationLifecycle::Publishing
                        {
                            callback_publication
                                .lifecycle
                                .set(LocalVideoTrackPublicationLifecycle::Published);
                        }
                        return;
                    }
                }
                callback_publication
                    .lifecycle
                    .set(LocalVideoTrackPublicationLifecycle::Unpublished);
                callback_track.clear_publication(&callback_publication);
            },
        );
        let callback: js_sys::Function = callback.unchecked_into();
        let replacement_callback: &js_sys::Function = self
            .inner
            .publication_replacement_callback()
            .as_ref()
            .unchecked_ref();
        let operation = adapter::publish_local_video_track(
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
                    .set(LocalVideoTrackPublicationLifecycle::Unpublished);
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
                        "publication returned an invalid SID".into(),
                    ))
                })?;
            if publication.sid.borrow().as_deref() != Some(sid.as_str()) {
                return Err(Error::AdapterFailure(AdapterError::contract(
                    "publication completion did not install its SID".into(),
                )));
            }
            Ok(LocalVideoTrackPublication { inner: publication })
        })
    }

    #[cfg(test)]
    pub(crate) fn test_track(stop_count: Rc<Cell<usize>>) -> Self {
        Self::test_track_with_source(stop_count, TrackSource::Camera)
    }

    #[cfg(test)]
    pub(crate) fn test_track_with_source(stop_count: Rc<Cell<usize>>, source: TrackSource) -> Self {
        Self {
            inner: Rc::new(LocalVideoTrackInner {
                lifecycle: Cell::new(LocalVideoTrackLifecycle::Active),
                source,
                end_state: LocalMediaTrackEndState::new(),
                muted: Cell::new(false),
                attachments: RefCell::new(Vec::new()),
                publication: RefCell::new(None),
                replacement_publication: RefCell::new(None),
                stop_count,
            }),
        }
    }
}

impl fmt::Debug for LocalVideoTrack {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalVideoTrack")
            .field("source", &self.source())
            .field("stopped", &self.is_stopped())
            .finish()
    }
}

pub(crate) struct LocalVideoTrackInner {
    lifecycle: Cell<LocalVideoTrackLifecycle>,
    source: TrackSource,
    pub(crate) end_state: Rc<LocalMediaTrackEndState>,
    #[cfg(any(target_arch = "wasm32", test))]
    muted: Cell<bool>,
    pub(crate) attachments: RefCell<Vec<Weak<VideoTrackAttachmentInner>>>,
    publication: RefCell<Option<Rc<LocalVideoTrackPublicationInner>>>,
    replacement_publication: RefCell<Option<Rc<LocalVideoTrackPublicationInner>>>,
    #[cfg(target_arch = "wasm32")]
    next_publication_id: Cell<u32>,
    #[cfg(target_arch = "wasm32")]
    publication_replacement_callback: OnceCell<PublicationReplacementCallback>,
    #[cfg(target_arch = "wasm32")]
    sdk_replacement_source: RefCell<Option<Rc<LocalVideoTrackPublicationInner>>>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) track_id: u32,
    #[cfg(target_arch = "wasm32")]
    stop_operation: RefCell<Option<Result<js_sys::Promise, AdapterError>>>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) ended_callback: RefCell<Option<wasm_bindgen::closure::Closure<dyn Fn()>>>,
    #[cfg(test)]
    stop_count: Rc<Cell<usize>>,
}

impl LocalVideoTrackInner {
    fn remove_attachment(&self, target: *const VideoTrackAttachmentInner) {
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
                    .set(LocalVideoTrackPublicationLifecycle::Unpublished);
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
                    .set(LocalVideoTrackPublicationLifecycle::Unpublished);
            }
        }
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
                .set(LocalVideoTrackPublicationLifecycle::Unpublished);
            self.sdk_replacement_source.borrow_mut().replace(previous);
            return 0;
        };
        let publication_id = self.allocate_publication_id();
        let replacement = Rc::new(LocalVideoTrackPublicationInner {
            lifecycle: Cell::new(LocalVideoTrackPublicationLifecycle::Published),
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

    #[cfg(target_arch = "wasm32")]
    fn clear_publication(&self, publication: &Rc<LocalVideoTrackPublicationInner>) {
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
    fn promote_replacement_publication(&self, publication: &Rc<LocalVideoTrackPublicationInner>) {
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
            adapter::stop_local_video_track(self.track_id).map_err(AdapterError::from_cause);
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
        if self.lifecycle.replace(LocalVideoTrackLifecycle::Stopped)
            == LocalVideoTrackLifecycle::Stopped
        {
            return false;
        }

        self.end_state.finish(LocalMediaTrackEndReason::Stopped);
        for attachment in self.attachments.borrow_mut().drain(..) {
            if let Some(attachment) = attachment.upgrade() {
                attachment.active.set(false);
            }
        }
        #[cfg(test)]
        self.stop_count.set(self.stop_count.get() + 1);
        true
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn mark_browser_ended(&self) {
        if self.lifecycle.replace(LocalVideoTrackLifecycle::Stopped)
            == LocalVideoTrackLifecycle::Stopped
        {
            return;
        }
        self.end_state
            .finish(LocalMediaTrackEndReason::BrowserEnded);
        for attachment in self.attachments.borrow_mut().drain(..) {
            if let Some(attachment) = attachment.upgrade() {
                attachment.active.set(false);
            }
        }
        if let Some(publication) = self.replacement_publication.borrow_mut().take() {
            publication
                .lifecycle
                .set(LocalVideoTrackPublicationLifecycle::Unpublished);
        }
        if let Some(publication) = self.publication.borrow_mut().take() {
            publication
                .lifecycle
                .set(LocalVideoTrackPublicationLifecycle::Unpublished);
        }
    }
}

impl Drop for LocalVideoTrackInner {
    fn drop(&mut self) {
        self.mark_stopped();
        #[cfg(target_arch = "wasm32")]
        adapter::drop_local_video_track(self.track_id);
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LocalVideoTrackLifecycle {
    Active,
    Stopped,
}

/// A typed local-video preview attachment with explicit and drop cleanup.
pub struct VideoTrackAttachment {
    inner: Rc<VideoTrackAttachmentInner>,
}

impl VideoTrackAttachment {
    /// Whether this guard still owns an active browser element attachment.
    pub fn is_active(&self) -> bool {
        self.inner.active.get()
            && self
                .inner
                .track
                .upgrade()
                .is_some_and(|track| track.lifecycle.get() == LocalVideoTrackLifecycle::Active)
    }

    /// Detaches the video element without stopping or publishing capture.
    pub fn close(&self) -> Result<(), Error> {
        if !self.inner.active.get() {
            return Ok(());
        }

        #[cfg(target_arch = "wasm32")]
        adapter::detach_local_video_track(self.inner.track_id, self.inner.attachment_id)
            .map_err(AdapterError::from_cause)
            .map_err(Error::MediaControlFailed)?;

        self.inner.active.set(false);
        if let Some(track) = self.inner.track.upgrade() {
            track.remove_attachment(Rc::as_ptr(&self.inner));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn test_attachment(track: &LocalVideoTrack) -> Self {
        let inner = Rc::new(VideoTrackAttachmentInner {
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

impl fmt::Debug for VideoTrackAttachment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VideoTrackAttachment")
            .field("active", &self.is_active())
            .finish()
    }
}

pub(crate) struct VideoTrackAttachmentInner {
    active: Cell<bool>,
    track: Weak<LocalVideoTrackInner>,
    #[cfg(target_arch = "wasm32")]
    track_id: u32,
    #[cfg(target_arch = "wasm32")]
    attachment_id: u32,
}

impl Drop for VideoTrackAttachmentInner {
    fn drop(&mut self) {
        if self.active.replace(false) {
            #[cfg(target_arch = "wasm32")]
            adapter::drop_local_video_track_attachment(self.track_id, self.attachment_id);
        }
        if let Some(track) = self.track.upgrade() {
            track.remove_attachment(self as *const _);
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn parse_local_video_track_id(value: wasm_bindgen::JsValue) -> Result<u32, AdapterError> {
    value
        .as_f64()
        .filter(|id| id.is_finite() && id.fract() == 0.0 && *id > 0.0)
        .map(|id| id as u32)
        .ok_or_else(|| {
            AdapterError::contract("camera capture returned an invalid Local Video Track ID".into())
        })
}

#[cfg(target_arch = "wasm32")]
pub(crate) struct BrowserCaptureState<T> {
    pub(crate) result: RefCell<Option<Result<T, AdapterError>>>,
    pub(crate) abandoned: Cell<bool>,
    pub(crate) waker: RefCell<Option<Waker>>,
}

#[cfg(target_arch = "wasm32")]
impl<T> BrowserCaptureState<T> {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self {
            result: RefCell::new(None),
            abandoned: Cell::new(false),
            waker: RefCell::new(None),
        })
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) struct BrowserCaptureFuture<T, O> {
    pub(crate) state: Rc<BrowserCaptureState<T>>,
    pub(crate) finish: fn(Result<T, AdapterError>) -> Result<O, Error>,
    pub(crate) cleanup: fn(T),
}

#[cfg(target_arch = "wasm32")]
impl<T, O> Future for BrowserCaptureFuture<T, O> {
    type Output = Result<O, Error>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.as_ref().get_ref();
        let Some(result) = this.state.result.borrow_mut().take() else {
            this.state
                .waker
                .borrow_mut()
                .replace(context.waker().clone());
            return Poll::Pending;
        };
        Poll::Ready((this.finish)(result))
    }
}

#[cfg(target_arch = "wasm32")]
impl<T, O> Drop for BrowserCaptureFuture<T, O> {
    fn drop(&mut self) {
        if let Some(Ok(value)) = self.state.result.borrow_mut().take() {
            (self.cleanup)(value);
        } else {
            self.state.abandoned.set(true);
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn complete_browser_capture<T>(
    state: &BrowserCaptureState<T>,
    result: Result<T, AdapterError>,
    cleanup: fn(T),
) {
    if state.abandoned.get() {
        if let Ok(value) = result {
            cleanup(value);
        }
        return;
    }
    state.result.borrow_mut().replace(result);
    if let Some(waker) = state.waker.borrow_mut().take() {
        waker.wake();
    }
}

#[cfg(target_arch = "wasm32")]
fn finish_camera_capture(result: Result<u32, AdapterError>) -> Result<LocalVideoTrack, Error> {
    result
        .map(|track_id| create_local_video_track(track_id, TrackSource::Camera))
        .map_err(classify_capture_error)
}

#[cfg(target_arch = "wasm32")]
fn drop_captured_video(track_id: u32) {
    adapter::drop_local_video_track(track_id);
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn create_local_video_track(track_id: u32, source: TrackSource) -> LocalVideoTrack {
    LocalVideoTrack {
        inner: Rc::new(LocalVideoTrackInner {
            lifecycle: Cell::new(LocalVideoTrackLifecycle::Active),
            source,
            end_state: LocalMediaTrackEndState::new(),
            muted: Cell::new(false),
            attachments: RefCell::new(Vec::new()),
            publication: RefCell::new(None),
            replacement_publication: RefCell::new(None),
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
pub(crate) fn capture_camera_video_track(
    options: CameraCaptureOptions,
) -> Pin<Box<dyn Future<Output = Result<LocalVideoTrack, Error>> + 'static>> {
    use wasm_bindgen::JsCast;

    let state = BrowserCaptureState::new();
    let callback_state = Rc::clone(&state);
    let callback = wasm_bindgen::closure::Closure::once_into_js(
        move |success: bool, value: wasm_bindgen::JsValue| {
            let result = if success {
                parse_local_video_track_id(value)
            } else {
                Err(AdapterError::from_cause(value))
            };
            complete_browser_capture(&callback_state, result, drop_captured_video);
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
                finish: finish_camera_capture,
                cleanup: drop_captured_video,
            });
        }
    };
    if let Err(cause) = adapter::create_camera_video_track(options, raw_options, &callback) {
        state
            .result
            .borrow_mut()
            .replace(Err(AdapterError::from_cause(cause)));
    }

    Box::pin(BrowserCaptureFuture {
        state,
        finish: finish_camera_capture,
        cleanup: drop_captured_video,
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn capture_camera_video_track(
    _options: CameraCaptureOptions,
) -> Pin<Box<dyn Future<Output = Result<LocalVideoTrack, Error>> + 'static>> {
    Box::pin(async { Err(Error::UnsupportedTarget) })
}

/// A browser-thread-local command handle for one Local Video Track Publication.
#[derive(Clone)]
pub struct LocalVideoTrackPublication {
    inner: Rc<LocalVideoTrackPublicationInner>,
}

impl LocalVideoTrackPublication {
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
    pub fn raw_js_publication(&self) -> Result<wasm_bindgen::JsValue, Error> {
        if self.is_terminal() || self.sid().is_none() {
            return Err(Error::InvalidState {
                operation: "access raw Local Video Track Publication",
                reason: "Track Publication is pending or terminal",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        raw_js_handle(adapter::raw_local_publication(
            self.inner.track_id,
            self.inner.publication_id,
            "video",
        ))
    }

    /// Returns the browser's unstable raw `RTCStatsReport` for this publication.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_rtc_stats_report(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<wasm_bindgen::JsValue>, Error>> + 'static>> {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "query raw Local Video Track RTC statistics",
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
            "video",
        ))
    }

    /// Queries one normalized sender-statistics sample per active video encoding.
    pub fn sender_statistics(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<VideoSenderStatistics>, Error>> + 'static>> {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "query Local Video Track sender statistics",
                    reason: "Track Publication is terminal",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            deserialize_statistics(adapter::local_video_sender_statistics(
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
                    reason: "Local Video Track capture has stopped",
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
            let operation = adapter::set_local_video_track_muted(
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

    /// Removes the Track Publication while keeping capture and attachments alive.
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
                        .set(LocalVideoTrackPublicationLifecycle::Unpublished);
                    if let Some(track) = callback_track.upgrade() {
                        track.clear_publication(&callback_publication);
                    }
                } else if callback_publication.lifecycle.get()
                    != LocalVideoTrackPublicationLifecycle::Unpublished
                {
                    callback_publication
                        .lifecycle
                        .set(LocalVideoTrackPublicationLifecycle::Published);
                }
            });
            let callback: js_sys::Function = callback.unchecked_into();
            let operation = adapter::unpublish_local_video_track(
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

impl fmt::Debug for LocalVideoTrackPublication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalVideoTrackPublication")
            .field("sid", &self.sid())
            .field("muted", &self.is_muted())
            .field("terminal", &self.is_terminal())
            .finish()
    }
}

struct LocalVideoTrackPublicationInner {
    lifecycle: Cell<LocalVideoTrackPublicationLifecycle>,
    muted: Cell<bool>,
    sid: RefCell<Option<String>>,
    session: Weak<SessionInner>,
    track: Weak<LocalVideoTrackInner>,
    #[cfg(target_arch = "wasm32")]
    publication_id: u32,
    #[cfg(target_arch = "wasm32")]
    pub(crate) track_id: u32,
    #[cfg(target_arch = "wasm32")]
    room_id: u32,
}

impl LocalVideoTrackPublicationInner {
    fn capture_stopped(&self) -> bool {
        self.track
            .upgrade()
            .is_none_or(|track| track.lifecycle.get() == LocalVideoTrackLifecycle::Stopped)
    }

    fn is_terminal(&self) -> bool {
        self.lifecycle.get() == LocalVideoTrackPublicationLifecycle::Unpublished
            || self
                .session
                .upgrade()
                .is_none_or(|session| session.lifecycle.get() != SessionLifecycle::Active)
    }

    fn association_released(&self) -> bool {
        self.lifecycle.get() == LocalVideoTrackPublicationLifecycle::Unpublished
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

    fn begin_unpublish(&self) -> Result<LocalVideoTrackPublicationLifecycle, Error> {
        if self.is_terminal()
            || self.lifecycle.get() == LocalVideoTrackPublicationLifecycle::Unpublishing
        {
            return Err(Error::InvalidState {
                operation: "unpublish Local Video Track",
                reason: "Track Publication is terminal or being unpublished",
            });
        }
        let previous = self
            .lifecycle
            .replace(LocalVideoTrackPublicationLifecycle::Unpublishing);
        Ok(previous)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LocalVideoTrackPublicationLifecycle {
    #[cfg(target_arch = "wasm32")]
    Publishing,
    #[cfg(any(target_arch = "wasm32", test))]
    Published,
    Unpublishing,
    Unpublished,
}
