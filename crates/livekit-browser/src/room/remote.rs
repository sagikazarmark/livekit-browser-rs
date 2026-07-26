use super::*;

#[cfg(target_arch = "wasm32")]
use crate::error::deserialize_statistics;
#[cfg(all(feature = "raw-js", target_arch = "wasm32"))]
use crate::error::raw_js_statistics;
#[cfg(target_arch = "wasm32")]
use crate::media::create_audio_analyser;
use crate::media::{terminate_audio_analysers, AudioAnalyserSource, AudioAnalyserState};

/// A handle for one exact Remote Participant instance.
///
/// Participant Identity remains the logical snapshot key. If LiveKit replaces the participant,
/// this handle becomes terminal instead of retargeting the replacement.
#[derive(Clone)]
pub struct RemoteParticipant {
    pub(super) inner: Rc<RemoteParticipantInner>,
}

impl RemoteParticipant {
    /// Returns the stable Participant Identity.
    pub fn identity(&self) -> &str {
        &self.inner.locator.identity
    }

    /// Returns the LiveKit participant SID captured by this handle.
    pub fn sid(&self) -> &str {
        &self.inner.locator.sid
    }

    /// Whether this exact participant instance is no longer current in its Room Session.
    pub fn is_terminal(&self) -> bool {
        self.current_snapshot().is_none()
    }

    /// Returns the unstable `livekit-client` Remote Participant object for this exact generation.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_participant(&self) -> Result<wasm_bindgen::JsValue, Error> {
        let Some(_session) = self.inner.session.upgrade() else {
            return Err(Error::InvalidState {
                operation: "access raw Remote Participant",
                reason: "Room Session is terminal",
            });
        };
        if self.is_terminal() {
            return Err(Error::InvalidState {
                operation: "access raw Remote Participant",
                reason: "Remote Participant is no longer current",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        {
            let locator = serde_wasm_bindgen::to_value(&self.inner.locator)
                .map_err(|error| AdapterError::contract(error.to_string()))
                .map_err(Error::AdapterFailure)?;
            raw_js_handle(adapter::raw_remote_participant(_session.room_id, &locator))
        }
    }

    /// Returns the current immutable state only while this exact participant remains current.
    pub fn snapshot(&self) -> Option<RemoteParticipantSnapshot> {
        self.current_snapshot()
    }

    /// Resolves a replacement-safe command handle for one current Track Publication.
    pub fn track_publication(&self, publication_sid: &str) -> Option<RemoteTrackPublication> {
        let participant = self.current_snapshot()?;
        let publication = participant.track_publications().get(publication_sid)?;
        Some(RemoteTrackPublication {
            inner: Rc::new(RemoteTrackPublicationInner {
                locator: RemotePublicationLocator {
                    participant: self.inner.locator.clone(),
                    sid: publication_sid.into(),
                    generation: publication.generation(),
                    kind: publication.kind(),
                },
                session: self.inner.session.clone(),
            }),
        })
    }

    fn current_snapshot(&self) -> Option<RemoteParticipantSnapshot> {
        let session = self.inner.session.upgrade()?;
        if session.lifecycle.get() != SessionLifecycle::Active {
            return None;
        }
        let participant = session
            .snapshot
            .borrow()
            .participant_for(&self.inner.locator)
            .cloned();
        participant
    }
}

impl fmt::Debug for RemoteParticipant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteParticipant")
            .field("identity", &self.identity())
            .field("sid", &self.sid())
            .field("terminal", &self.is_terminal())
            .finish()
    }
}

pub(super) struct RemoteParticipantInner {
    pub(super) locator: RemoteParticipantLocator,
    pub(super) session: Weak<SessionInner>,
}

/// A live command handle for one Remote Participant Track Publication.
///
/// The immutable publication state remains in `RoomSnapshot`; this handle only resolves the
/// current Remote Media Track and controls subscription policy.
#[derive(Clone)]
pub struct RemoteTrackPublication {
    inner: Rc<RemoteTrackPublicationInner>,
}

impl RemoteTrackPublication {
    /// Returns the stable Participant Identity that owns this publication.
    pub fn participant_identity(&self) -> &str {
        &self.inner.locator.participant.identity
    }

    /// Returns the publication SID.
    pub fn sid(&self) -> &str {
        &self.inner.locator.sid
    }

    /// Returns the publication's announced media kind.
    pub fn kind(&self) -> TrackKind {
        self.inner.locator.kind
    }

    /// Whether this handle no longer identifies a current publication in its Room Session.
    pub fn is_terminal(&self) -> bool {
        self.current_snapshot().is_none()
    }

    /// Returns the unstable `livekit-client` Remote Track Publication object.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_publication(&self) -> Result<wasm_bindgen::JsValue, Error> {
        let Some(_session) = self.inner.session.upgrade() else {
            return Err(Error::InvalidState {
                operation: "access raw Remote Track Publication",
                reason: "Room Session is terminal",
            });
        };
        if self.is_terminal() {
            return Err(Error::InvalidState {
                operation: "access raw Remote Track Publication",
                reason: "Track Publication is no longer current",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        {
            let locator = serde_wasm_bindgen::to_value(&self.inner.locator)
                .map_err(|error| AdapterError::contract(error.to_string()))
                .map_err(Error::AdapterFailure)?;
            raw_js_handle(adapter::raw_remote_publication(_session.room_id, &locator))
        }
    }

    /// Returns the current immutable state for this publication.
    pub fn snapshot(&self) -> Option<RemoteTrackPublicationSnapshot> {
        self.current_snapshot()
    }

    /// Enables or disables subscription without attaching or detaching browser elements.
    ///
    /// The command is sent before the returned local future is polled. Authoritative subscription
    /// and media availability continue to arrive through Room Snapshots and Room Events.
    pub fn set_subscribed(
        &self,
        _subscribed: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'static>> {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "change Remote Track Publication subscription",
                    reason: "Track Publication is terminal",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            let Some(session) = self.inner.session.upgrade() else {
                return Box::pin(async {
                    Err(Error::InvalidState {
                        operation: "change Remote Track Publication subscription",
                        reason: "Room Session is terminal",
                    })
                });
            };
            let locator = match serde_wasm_bindgen::to_value(&self.inner.locator) {
                Ok(locator) => locator,
                Err(error) => {
                    let error = Error::AdapterFailure(AdapterError::contract(error.to_string()));
                    return Box::pin(async move { Err(error) });
                }
            };
            let result =
                adapter::set_remote_track_subscription(session.room_id, &locator, _subscribed)
                    .map_err(AdapterError::from_cause)
                    .map_err(Error::SubscriptionFailed);
            Box::pin(async move { result })
        }
    }

    /// Resolves the currently available generation-specific Remote Media Track.
    ///
    /// `Ok(None)` preserves the distinction between an observable Track Publication and locally
    /// unavailable media. A later subscription or replacement must be resolved again and never
    /// retargets an older handle.
    pub fn media_track(&self) -> Result<Option<RemoteMediaTrack>, Error> {
        let Some(snapshot) = self.current_snapshot() else {
            return Err(Error::InvalidState {
                operation: "resolve Remote Media Track",
                reason: "Track Publication is terminal",
            });
        };
        if !snapshot.media_available() {
            return Ok(None);
        }
        if !matches!(self.kind(), TrackKind::Audio | TrackKind::Video) {
            return Err(Error::InvalidState {
                operation: "resolve Remote Media Track",
                reason: "Track Publication does not represent audio or video",
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        resolve_remote_media_track(self)
    }

    fn current_snapshot(&self) -> Option<RemoteTrackPublicationSnapshot> {
        let session = self.inner.session.upgrade()?;
        if session.lifecycle.get() != SessionLifecycle::Active {
            return None;
        }
        let publication = session
            .snapshot
            .borrow()
            .publication_for(&self.inner.locator)
            .cloned();
        publication
    }
}

impl fmt::Debug for RemoteTrackPublication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteTrackPublication")
            .field("participant_identity", &self.participant_identity())
            .field("sid", &self.sid())
            .field("kind", &self.kind())
            .field("terminal", &self.is_terminal())
            .finish()
    }
}

struct RemoteTrackPublicationInner {
    locator: RemotePublicationLocator,
    session: Weak<SessionInner>,
}

/// A currently available typed Remote Media Track.
#[derive(Clone)]
#[non_exhaustive]
pub enum RemoteMediaTrack {
    /// A subscribed Remote Audio Track.
    Audio(RemoteAudioTrack),
    /// A subscribed Remote Video Track.
    Video(RemoteVideoTrack),
}

impl RemoteMediaTrack {
    /// Whether this exact Remote Media Track generation has ended or been replaced.
    pub fn is_terminal(&self) -> bool {
        match self {
            Self::Audio(track) => track.is_terminal(),
            Self::Video(track) => track.is_terminal(),
        }
    }

    /// Returns the unstable `livekit-client` Remote Media Track object.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_media_track(&self) -> Result<wasm_bindgen::JsValue, Error> {
        match self {
            Self::Audio(track) => track.raw_js_media_track(),
            Self::Video(track) => track.raw_js_media_track(),
        }
    }
}

/// One generation of a locally available subscribed Remote Audio Track.
#[derive(Clone)]
pub struct RemoteAudioTrack {
    inner: Rc<RemoteMediaTrackInner>,
}

impl PartialEq for RemoteAudioTrack {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for RemoteAudioTrack {}

impl RemoteAudioTrack {
    /// Returns the Participant Identity that supplied this media.
    pub fn participant_identity(&self) -> &str {
        &self.inner.publication.participant.identity
    }

    /// Returns the Track Publication SID associated with this generation.
    pub fn publication_sid(&self) -> &str {
        &self.inner.publication.sid
    }

    /// Whether this exact media generation has ended or been replaced.
    pub fn is_terminal(&self) -> bool {
        self.inner.is_terminal()
    }

    /// Returns the unstable `livekit-client` Remote Audio Track object.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_media_track(&self) -> Result<wasm_bindgen::JsValue, Error> {
        if self.is_terminal() {
            return Err(Error::InvalidState {
                operation: "access raw Remote Audio Track",
                reason: "Remote Media Track is terminal",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        raw_js_handle(adapter::raw_remote_media_track(self.inner.track_id))
    }

    /// Returns the browser's unstable raw `RTCStatsReport` for this media generation.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_rtc_stats_report(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<wasm_bindgen::JsValue>, Error>> + 'static>> {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "query raw Remote Audio Track RTC statistics",
                    reason: "Remote Media Track is terminal",
                })
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });
        #[cfg(target_arch = "wasm32")]
        raw_js_statistics(adapter::raw_remote_rtc_stats_report(self.inner.track_id))
    }

    /// Queries the latest normalized receiver statistics for this exact media generation.
    pub fn receiver_statistics(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<AudioReceiverStatistics>, Error>> + 'static>>
    {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "query Remote Audio Track receiver statistics",
                    reason: "Remote Media Track is terminal",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            deserialize_statistics(adapter::remote_audio_receiver_statistics(
                self.inner.track_id,
            ))
        }
    }

    /// Creates a typed level and waveform analyser for this exact media generation.
    pub fn create_analyser(&self) -> Result<AudioAnalyser, Error> {
        if self.is_terminal() {
            return Err(Error::InvalidState {
                operation: "create Remote Media Track audio analyser",
                reason: "Remote Media Track is terminal",
            });
        }

        let _source = AudioAnalyserSource::Remote(Rc::downgrade(&self.inner));
        #[cfg(target_arch = "wasm32")]
        return create_audio_analyser(_source, |callback| {
            adapter::create_remote_audio_analyser(self.inner.track_id, callback)
        });

        #[cfg(all(not(target_arch = "wasm32"), test))]
        return Ok(AudioAnalyser::test_analyser(_source));

        #[cfg(all(not(target_arch = "wasm32"), not(test)))]
        Err(Error::UnsupportedTarget)
    }

    /// Attaches this Remote Audio Track without changing subscription policy.
    pub fn attach(
        &self,
        _element: &web_sys::HtmlAudioElement,
    ) -> Result<RemoteAudioTrackAttachment, Error> {
        if self.is_terminal() {
            return Err(Error::InvalidState {
                operation: "attach Remote Audio Track",
                reason: "Remote Media Track is terminal",
            });
        }

        #[cfg(target_arch = "wasm32")]
        let attachment_id = adapter::attach_remote_audio_track(self.inner.track_id, _element)
            .map_err(AdapterError::from_cause)
            .map_err(Error::MediaControlFailed)?;
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        {
            let state = Rc::new(RemoteTrackAttachmentState {
                active: Cell::new(true),
            });
            self.inner
                .attachments
                .borrow_mut()
                .push(Rc::downgrade(&state));
            Ok(RemoteAudioTrackAttachment {
                inner: Rc::new(RemoteAudioTrackAttachmentInner {
                    state,
                    track: Rc::downgrade(&self.inner),
                    track_id: self.inner.track_id,
                    attachment_id,
                }),
            })
        }
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn test_track(session: &RoomSession) -> Self {
        Self {
            inner: test_remote_media_track(
                session,
                "TR_microphone",
                "microphone",
                TrackSource::Microphone,
                TrackKind::Audio,
            ),
        }
    }
}

impl fmt::Debug for RemoteAudioTrack {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteAudioTrack")
            .field("participant_identity", &self.participant_identity())
            .field("publication_sid", &self.publication_sid())
            .field("terminal", &self.is_terminal())
            .finish()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
fn test_remote_media_track(
    session: &RoomSession,
    sid: &str,
    name: &str,
    source: TrackSource,
    kind: TrackKind,
) -> Rc<RemoteMediaTrackInner> {
    let mut snapshot = session.snapshot().as_ref().clone();
    snapshot.revision += 1;
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
                sid.into(),
                RemoteTrackPublicationSnapshot {
                    sid: sid.into(),
                    generation: 2,
                    name: name.into(),
                    source,
                    kind,
                    muted: false,
                    subscribed: true,
                    media_available: true,
                    media_generation: Some(3),
                },
            )]),
            data_tracks: BTreeMap::new(),
        },
    );
    session
        .inner
        .install_normalized_update(snapshot, std::iter::empty());
    Rc::new(RemoteMediaTrackInner {
        lifecycle: Cell::new(RemoteMediaTrackLifecycle::Active),
        publication: RemotePublicationLocator {
            participant: RemoteParticipantLocator {
                identity: "student".into(),
                sid: "PA_student".into(),
                generation: 1,
            },
            sid: sid.into(),
            generation: 2,
            kind,
        },
        media_generation: 3,
        session: Rc::downgrade(&session.inner),
        attachments: RefCell::new(Vec::new()),
        analysers: RefCell::new(Vec::new()),
    })
}

/// One generation of a locally available subscribed Remote Video Track.
#[derive(Clone)]
pub struct RemoteVideoTrack {
    pub(crate) inner: Rc<RemoteMediaTrackInner>,
}

impl PartialEq for RemoteVideoTrack {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for RemoteVideoTrack {}

impl RemoteVideoTrack {
    /// Returns the Participant Identity that supplied this media.
    pub fn participant_identity(&self) -> &str {
        &self.inner.publication.participant.identity
    }

    /// Returns the Track Publication SID associated with this generation.
    pub fn publication_sid(&self) -> &str {
        &self.inner.publication.sid
    }

    /// Whether this exact media generation has ended or been replaced.
    pub fn is_terminal(&self) -> bool {
        self.inner.is_terminal()
    }

    /// Returns the unstable `livekit-client` Remote Video Track object.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_media_track(&self) -> Result<wasm_bindgen::JsValue, Error> {
        if self.is_terminal() {
            return Err(Error::InvalidState {
                operation: "access raw Remote Video Track",
                reason: "Remote Media Track is terminal",
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_arch = "wasm32")]
        raw_js_handle(adapter::raw_remote_media_track(self.inner.track_id))
    }

    /// Returns the browser's unstable raw `RTCStatsReport` for this media generation.
    #[cfg(feature = "raw-js")]
    pub fn raw_js_rtc_stats_report(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<wasm_bindgen::JsValue>, Error>> + 'static>> {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "query raw Remote Video Track RTC statistics",
                    reason: "Remote Media Track is terminal",
                })
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });
        #[cfg(target_arch = "wasm32")]
        raw_js_statistics(adapter::raw_remote_rtc_stats_report(self.inner.track_id))
    }

    /// Queries the latest normalized receiver statistics for this exact media generation.
    pub fn receiver_statistics(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<VideoReceiverStatistics>, Error>> + 'static>>
    {
        if self.is_terminal() {
            return Box::pin(async {
                Err(Error::InvalidState {
                    operation: "query Remote Video Track receiver statistics",
                    reason: "Remote Media Track is terminal",
                })
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        return Box::pin(async { Err(Error::UnsupportedTarget) });

        #[cfg(target_arch = "wasm32")]
        {
            deserialize_statistics(adapter::remote_video_receiver_statistics(
                self.inner.track_id,
            ))
        }
    }

    /// Attaches this Remote Video Track without changing subscription policy.
    pub fn attach(
        &self,
        _element: &web_sys::HtmlVideoElement,
    ) -> Result<RemoteVideoTrackAttachment, Error> {
        if self.is_terminal() {
            return Err(Error::InvalidState {
                operation: "attach Remote Video Track",
                reason: "Remote Media Track is terminal",
            });
        }

        #[cfg(target_arch = "wasm32")]
        let attachment_id = adapter::attach_remote_video_track(self.inner.track_id, _element)
            .map_err(AdapterError::from_cause)
            .map_err(Error::MediaControlFailed)?;
        #[cfg(not(target_arch = "wasm32"))]
        return Err(Error::UnsupportedTarget);

        #[cfg(target_arch = "wasm32")]
        {
            let state = Rc::new(RemoteTrackAttachmentState {
                active: Cell::new(true),
            });
            self.inner
                .attachments
                .borrow_mut()
                .push(Rc::downgrade(&state));
            Ok(RemoteVideoTrackAttachment {
                inner: Rc::new(RemoteVideoTrackAttachmentInner {
                    state,
                    track: Rc::downgrade(&self.inner),
                    track_id: self.inner.track_id,
                    attachment_id,
                }),
            })
        }
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn test_track(session: &RoomSession) -> Self {
        Self {
            inner: test_remote_media_track(
                session,
                "TR_camera",
                "camera",
                TrackSource::Camera,
                TrackKind::Video,
            ),
        }
    }
}

impl fmt::Debug for RemoteVideoTrack {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteVideoTrack")
            .field("participant_identity", &self.participant_identity())
            .field("publication_sid", &self.publication_sid())
            .field("terminal", &self.is_terminal())
            .finish()
    }
}

pub(crate) struct RemoteMediaTrackInner {
    lifecycle: Cell<RemoteMediaTrackLifecycle>,
    publication: RemotePublicationLocator,
    media_generation: u32,
    session: Weak<SessionInner>,
    pub(crate) attachments: RefCell<Vec<Weak<RemoteTrackAttachmentState>>>,
    pub(crate) analysers: RefCell<Vec<Weak<AudioAnalyserState>>>,
    #[cfg(target_arch = "wasm32")]
    track_id: u32,
    #[cfg(target_arch = "wasm32")]
    _terminal_callback: wasm_bindgen::closure::Closure<dyn Fn()>,
}

impl RemoteMediaTrackInner {
    pub(crate) fn is_terminal(&self) -> bool {
        self.lifecycle.get() == RemoteMediaTrackLifecycle::Terminal
            || self.session.upgrade().is_none_or(|session| {
                session.lifecycle.get() != SessionLifecycle::Active
                    || !self.is_current_generation(&session.snapshot.borrow())
            })
    }

    fn is_current_generation(&self, snapshot: &RoomSnapshot) -> bool {
        snapshot
            .publication_for(&self.publication)
            .is_some_and(|publication| {
                publication.media_generation() == Some(self.media_generation)
            })
    }

    pub(crate) fn mark_terminal(&self) {
        if self.lifecycle.replace(RemoteMediaTrackLifecycle::Terminal)
            == RemoteMediaTrackLifecycle::Terminal
        {
            return;
        }
        terminate_audio_analysers(&self.analysers);
        for attachment in self.attachments.borrow_mut().drain(..) {
            if let Some(attachment) = attachment.upgrade() {
                attachment.active.set(false);
            }
        }
    }

    fn remove_attachment(&self, state: &Rc<RemoteTrackAttachmentState>) {
        self.attachments.borrow_mut().retain(|attachment| {
            attachment
                .upgrade()
                .is_some_and(|attachment| !Rc::ptr_eq(&attachment, state))
        });
    }
}

impl Drop for RemoteMediaTrackInner {
    fn drop(&mut self) {
        self.mark_terminal();
        #[cfg(target_arch = "wasm32")]
        adapter::drop_remote_media_track(self.track_id);
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum RemoteMediaTrackLifecycle {
    #[allow(dead_code)]
    Active,
    Terminal,
}

pub(crate) struct RemoteTrackAttachmentState {
    active: Cell<bool>,
}

/// A typed Remote Audio Track element attachment with explicit and drop cleanup.
pub struct RemoteAudioTrackAttachment {
    inner: Rc<RemoteAudioTrackAttachmentInner>,
}

impl RemoteAudioTrackAttachment {
    /// Whether this guard still owns an active attachment to a current media generation.
    pub fn is_active(&self) -> bool {
        self.inner.state.active.get()
            && self
                .inner
                .track
                .upgrade()
                .is_some_and(|track| !track.is_terminal())
    }

    /// Detaches the element without changing network subscription state.
    pub fn close(&self) -> Result<(), Error> {
        if !self.inner.state.active.get() {
            return Ok(());
        }
        #[cfg(target_arch = "wasm32")]
        adapter::detach_remote_audio_track(self.inner.track_id, self.inner.attachment_id)
            .map_err(AdapterError::from_cause)
            .map_err(Error::MediaControlFailed)?;
        self.inner.state.active.set(false);
        if let Some(track) = self.inner.track.upgrade() {
            track.remove_attachment(&self.inner.state);
        }
        Ok(())
    }
}

impl fmt::Debug for RemoteAudioTrackAttachment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteAudioTrackAttachment")
            .field("active", &self.is_active())
            .finish()
    }
}

struct RemoteAudioTrackAttachmentInner {
    state: Rc<RemoteTrackAttachmentState>,
    track: Weak<RemoteMediaTrackInner>,
    #[cfg(target_arch = "wasm32")]
    track_id: u32,
    #[cfg(target_arch = "wasm32")]
    attachment_id: u32,
}

impl Drop for RemoteAudioTrackAttachmentInner {
    fn drop(&mut self) {
        if self.state.active.replace(false) {
            #[cfg(target_arch = "wasm32")]
            adapter::drop_remote_audio_track_attachment(self.track_id, self.attachment_id);
        }
        if let Some(track) = self.track.upgrade() {
            track.remove_attachment(&self.state);
        }
    }
}

/// A typed Remote Video Track element attachment with explicit and drop cleanup.
pub struct RemoteVideoTrackAttachment {
    inner: Rc<RemoteVideoTrackAttachmentInner>,
}

impl RemoteVideoTrackAttachment {
    /// Whether this guard still owns an active attachment to a current media generation.
    pub fn is_active(&self) -> bool {
        self.inner.state.active.get()
            && self
                .inner
                .track
                .upgrade()
                .is_some_and(|track| !track.is_terminal())
    }

    /// Detaches the element without changing network subscription state.
    pub fn close(&self) -> Result<(), Error> {
        if !self.inner.state.active.get() {
            return Ok(());
        }
        #[cfg(target_arch = "wasm32")]
        adapter::detach_remote_video_track(self.inner.track_id, self.inner.attachment_id)
            .map_err(AdapterError::from_cause)
            .map_err(Error::MediaControlFailed)?;
        self.inner.state.active.set(false);
        if let Some(track) = self.inner.track.upgrade() {
            track.remove_attachment(&self.inner.state);
        }
        Ok(())
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn test_attachment(track: &RemoteVideoTrack) -> Self {
        let state = Rc::new(RemoteTrackAttachmentState {
            active: Cell::new(true),
        });
        track
            .inner
            .attachments
            .borrow_mut()
            .push(Rc::downgrade(&state));
        Self {
            inner: Rc::new(RemoteVideoTrackAttachmentInner {
                state,
                track: Rc::downgrade(&track.inner),
            }),
        }
    }
}

impl fmt::Debug for RemoteVideoTrackAttachment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteVideoTrackAttachment")
            .field("active", &self.is_active())
            .finish()
    }
}

struct RemoteVideoTrackAttachmentInner {
    state: Rc<RemoteTrackAttachmentState>,
    track: Weak<RemoteMediaTrackInner>,
    #[cfg(target_arch = "wasm32")]
    track_id: u32,
    #[cfg(target_arch = "wasm32")]
    attachment_id: u32,
}

impl Drop for RemoteVideoTrackAttachmentInner {
    fn drop(&mut self) {
        if self.state.active.replace(false) {
            #[cfg(target_arch = "wasm32")]
            adapter::drop_remote_video_track_attachment(self.track_id, self.attachment_id);
        }
        if let Some(track) = self.track.upgrade() {
            track.remove_attachment(&self.state);
        }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
pub(crate) struct AdapterRemoteMediaTrack {
    id: u32,
    kind: TrackKind,
    generation: u32,
}

#[cfg(target_arch = "wasm32")]
fn resolve_remote_media_track(
    publication: &RemoteTrackPublication,
) -> Result<Option<RemoteMediaTrack>, Error> {
    use wasm_bindgen::JsCast;

    let Some(session) = publication.inner.session.upgrade() else {
        return Err(Error::InvalidState {
            operation: "resolve Remote Media Track",
            reason: "Room Session is terminal",
        });
    };
    let target: Rc<RefCell<Weak<RemoteMediaTrackInner>>> = Rc::new(RefCell::new(Weak::new()));
    let callback_target = Rc::clone(&target);
    let terminal_callback = wasm_bindgen::closure::Closure::new(move || {
        if let Some(inner) = callback_target.borrow().upgrade() {
            inner.mark_terminal();
        }
    });
    let locator = serde_wasm_bindgen::to_value(&publication.inner.locator)
        .map_err(|error| Error::AdapterFailure(AdapterError::contract(error.to_string())))?;
    let descriptor = adapter::resolve_remote_media_track(
        session.room_id,
        &locator,
        publication
            .current_snapshot()
            .and_then(|snapshot| snapshot.media_generation())
            .ok_or(Error::InvalidState {
                operation: "resolve Remote Media Track",
                reason: "Remote Media Track is no longer available",
            })?,
        terminal_callback.as_ref().unchecked_ref(),
    )
    .map_err(AdapterError::from_cause)
    .map_err(Error::MediaControlFailed)?;
    if descriptor.is_null() || descriptor.is_undefined() {
        return Ok(None);
    }
    let parsed = serde_wasm_bindgen::from_value(descriptor.clone());
    let descriptor: AdapterRemoteMediaTrack = match parsed {
        Ok(descriptor) => descriptor,
        Err(error) => {
            adapter::drop_remote_media_track_descriptor(descriptor);
            return Err(Error::AdapterFailure(AdapterError::contract(
                error.to_string(),
            )));
        }
    };
    if descriptor.kind != publication.kind() {
        adapter::drop_remote_media_track(descriptor.id);
        return Err(Error::AdapterFailure(AdapterError::contract(
            "Remote Media Track kind did not match its Track Publication".into(),
        )));
    }
    let Some(snapshot) = publication.current_snapshot() else {
        adapter::drop_remote_media_track(descriptor.id);
        return Err(Error::InvalidState {
            operation: "resolve Remote Media Track",
            reason: "Track Publication is terminal",
        });
    };
    if snapshot.media_generation() != Some(descriptor.generation) {
        adapter::drop_remote_media_track(descriptor.id);
        return Err(Error::InvalidState {
            operation: "resolve Remote Media Track",
            reason: "Remote Media Track was replaced while resolving",
        });
    }
    let inner = Rc::new(RemoteMediaTrackInner {
        lifecycle: Cell::new(RemoteMediaTrackLifecycle::Active),
        publication: publication.inner.locator.clone(),
        media_generation: descriptor.generation,
        session: Rc::downgrade(&session),
        attachments: RefCell::new(Vec::new()),
        analysers: RefCell::new(Vec::new()),
        track_id: descriptor.id,
        _terminal_callback: terminal_callback,
    });
    *target.borrow_mut() = Rc::downgrade(&inner);
    Ok(Some(match descriptor.kind {
        TrackKind::Audio => RemoteMediaTrack::Audio(RemoteAudioTrack { inner }),
        TrackKind::Video => RemoteMediaTrack::Video(RemoteVideoTrack { inner }),
        _ => unreachable!("validated Remote Media Track kind"),
    }))
}
