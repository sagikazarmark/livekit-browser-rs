//! Headless Dioxus integration for LiveKit browser Room Sessions.
//!
//! Applications fetch credentials and join through `livekit-browser`, then provide the joined
//! Room Session to [`RoomSessionProvider`]. The provider owns only its subtree's Room Session
//! observation and cleanup. Media nodes attach existing Media Tracks to their mounted browser
//! elements; they never capture, publish, mute, or change subscription policy.

use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use dioxus::{core::Task, prelude::*, web::WebEventExt};
use futures_util::StreamExt;
use livekit_browser::{
    AudioPlaybackState, BrowserCapabilities, Error, LocalAudioTrack, LocalParticipantSnapshot,
    LocalTrackPublicationSnapshot, LocalVideoTrack, RemoteAudioTrack, RemoteParticipantSnapshot,
    RemoteTrackPublicationSnapshot, RemoteVideoTrack, RoomSession, RoomSnapshot,
};
use wasm_bindgen::JsCast;

const DEFAULT_EVENT_CAPACITY: usize = 64;

#[derive(Clone)]
struct RoomSessionContext {
    session: Signal<RoomSession>,
    snapshot: Signal<Rc<RoomSnapshot>>,
    capabilities: Signal<BrowserCapabilities>,
}

struct RoomSessionObserver {
    session: RoomSession,
    capacity: usize,
    task: Task,
}

fn observe_snapshots(
    observed: RoomSession,
    capacity: usize,
    mut snapshot: Signal<Rc<RoomSnapshot>>,
) -> Task {
    let mut events = observed.subscribe_events(capacity);
    spawn(async move {
        while events.next().await.is_some() {
            snapshot.set(observed.snapshot());
        }
    })
}

/// Properties for [`RoomSessionProvider`].
#[derive(Clone, Props, PartialEq)]
pub struct RoomSessionProviderProps {
    /// An already joined Room Session owned by this component subtree.
    pub session: RoomSession,
    /// Permission-free capabilities from the `BrowserClient` that created the Room Session.
    pub capabilities: BrowserCapabilities,
    /// Capacity of the provider's bounded Room Event subscription.
    #[props(default = DEFAULT_EVENT_CAPACITY)]
    pub event_capacity: usize,
    /// Components that consume this provider.
    pub children: Element,
}

/// Provides one already joined Room Session reactively to a Dioxus subtree.
///
/// The provider observes bounded Room Events and publishes the coherent snapshot installed before
/// each event. Unmounting or replacement cancels only this subtree's observation and drops the
/// provider-owned Room Session clones; explicit terminal leave remains application-controlled.
#[allow(non_snake_case)]
pub fn RoomSessionProvider(props: RoomSessionProviderProps) -> Element {
    let mut session = use_signal(|| props.session.clone());
    let mut snapshot = use_signal(|| props.session.snapshot());
    let mut capabilities = use_signal(|| props.capabilities);
    let observer = use_hook({
        let observed = props.session.clone();
        move || {
            let task = observe_snapshots(observed.clone(), props.event_capacity, snapshot);
            Rc::new(RefCell::new(RoomSessionObserver {
                session: observed,
                capacity: props.event_capacity,
                task,
            }))
        }
    });

    let (session_changed, observer_changed) = {
        let observer = observer.borrow();
        let session_changed = observer.session != props.session;
        (
            session_changed,
            session_changed || observer.capacity != props.event_capacity,
        )
    };
    if observer_changed {
        let mut observer = observer.borrow_mut();
        observer.task.cancel();
        observer.session = props.session.clone();
        observer.capacity = props.event_capacity;
        observer.task = observe_snapshots(props.session.clone(), props.event_capacity, snapshot);

        if session_changed {
            session.set(props.session.clone());
            snapshot.set(props.session.snapshot());
        }
    }
    if *capabilities.peek() != props.capabilities {
        capabilities.set(props.capabilities);
    }

    use_context_provider(|| RoomSessionContext {
        session,
        snapshot,
        capabilities,
    });

    use_drop(move || {
        observer.borrow().task.cancel();
    });

    props.children
}

/// Returns the nearest provider's exact Room Session handle.
///
/// This hook panics when called outside a [`RoomSessionProvider`], matching Dioxus context hooks.
pub fn use_room_session() -> RoomSession {
    use_context::<RoomSessionContext>().session.read().clone()
}

/// Returns a reactive read-only signal for the nearest provider's current coherent snapshot.
pub fn use_room_snapshot() -> ReadSignal<Rc<RoomSnapshot>> {
    let context = use_context::<RoomSessionContext>();
    use_hook(move || ReadSignal::new(context.snapshot))
}

/// Returns the nearest provider's permission-free browser capabilities.
pub fn use_browser_capabilities() -> BrowserCapabilities {
    *use_context::<RoomSessionContext>().capabilities.read()
}

/// A coherent Local Participant and Remote Participant view derived from one Room Snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParticipantSnapshots {
    local: LocalParticipantSnapshot,
    remote: BTreeMap<String, RemoteParticipantSnapshot>,
}

impl ParticipantSnapshots {
    /// Returns the Local Participant snapshot.
    pub const fn local(&self) -> &LocalParticipantSnapshot {
        &self.local
    }

    /// Returns Remote Participants keyed by Participant Identity.
    pub const fn remote(&self) -> &BTreeMap<String, RemoteParticipantSnapshot> {
        &self.remote
    }
}

/// Returns reactive participant state derived from the nearest provider's current snapshot.
pub fn use_participants() -> Memo<ParticipantSnapshots> {
    let snapshot = use_room_snapshot();
    use_memo(move || {
        let snapshot = snapshot.read();
        ParticipantSnapshots {
            local: snapshot.local_participant().clone(),
            remote: snapshot.remote_participants().clone(),
        }
    })
}

/// A coherent view of all Track Publications derived from one Room Snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrackPublicationSnapshots {
    local: BTreeMap<String, LocalTrackPublicationSnapshot>,
    remote: BTreeMap<String, BTreeMap<String, RemoteTrackPublicationSnapshot>>,
}

impl TrackPublicationSnapshots {
    /// Returns Local Participant Track Publications keyed by publication SID.
    pub const fn local(&self) -> &BTreeMap<String, LocalTrackPublicationSnapshot> {
        &self.local
    }

    /// Returns Remote Participant Track Publications keyed by Participant Identity and then SID.
    pub const fn remote(
        &self,
    ) -> &BTreeMap<String, BTreeMap<String, RemoteTrackPublicationSnapshot>> {
        &self.remote
    }
}

/// Returns reactive Track Publication state from the nearest provider's current snapshot.
pub fn use_track_publications() -> Memo<TrackPublicationSnapshots> {
    let snapshot = use_room_snapshot();
    use_memo(move || {
        let snapshot = snapshot.read();
        TrackPublicationSnapshots {
            local: snapshot.local_participant().track_publications().clone(),
            remote: snapshot
                .remote_participants()
                .iter()
                .map(|(identity, participant)| {
                    (identity.clone(), participant.track_publications().clone())
                })
                .collect(),
        }
    })
}

/// Returns reactive browser autoplay state from the nearest provider's current snapshot.
pub fn use_audio_playback() -> Memo<AudioPlaybackState> {
    let snapshot = use_room_snapshot();
    use_memo(move || snapshot.read().audio_playback())
}

/// A typed request to change one Remote Track Publication's subscription policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteTrackPublicationSubscription {
    participant_identity: String,
    publication_sid: String,
    subscribed: bool,
}

impl RemoteTrackPublicationSubscription {
    /// Creates a subscription request keyed by Participant Identity and publication SID.
    pub fn new(
        participant_identity: impl Into<String>,
        publication_sid: impl Into<String>,
        subscribed: bool,
    ) -> Self {
        Self {
            participant_identity: participant_identity.into(),
            publication_sid: publication_sid.into(),
            subscribed,
        }
    }

    /// Returns the Remote Participant Identity.
    pub fn participant_identity(&self) -> &str {
        &self.participant_identity
    }

    /// Returns the Track Publication SID.
    pub fn publication_sid(&self) -> &str {
        &self.publication_sid
    }

    /// Returns the requested subscription state.
    pub const fn subscribed(&self) -> bool {
        self.subscribed
    }
}

/// Dioxus actions for common Room Session mutations.
///
/// Each action exposes Dioxus's pending/result/error state. Calling an action starts the core
/// operation synchronously before the returned waiter is polled, preserving browser activation
/// requirements and the core's abandoned-future semantics.
#[derive(Clone, Copy)]
pub struct RoomSessionActions {
    pub leave: Action<(), ()>,
    pub start_audio_playback: Action<(), ()>,
    pub set_remote_track_publication_subscription:
        Action<(RemoteTrackPublicationSubscription,), ()>,
    pub set_publication_subscription_permissions:
        Action<(livekit_browser::PublicationSubscriptionPermissions,), ()>,
    pub set_local_participant_metadata: Action<(String,), ()>,
    pub update_local_participant_attributes: Action<(BTreeMap<String, String>,), ()>,
}

/// Returns typed Dioxus actions bound to the nearest Room Session.
pub fn use_room_actions() -> RoomSessionActions {
    let context = use_context::<RoomSessionContext>();
    let room = context.session.read().clone();
    let mut current_snapshot = context.snapshot;
    let leave_room = room.clone();
    let completed_leave_room = room.clone();
    let playback_room = room.clone();
    let subscription_room = room.clone();
    let permissions_room = room.clone();
    let metadata_participant = room.local_participant();
    let attributes_participant = room.local_participant();

    let leave = use_action(move || {
        let operation = leave_room.leave();
        let completed_leave_room = completed_leave_room.clone();
        current_snapshot.set(leave_room.snapshot());
        async move {
            let result = operation.await;
            current_snapshot.set(completed_leave_room.snapshot());
            result
        }
    });
    let start_audio_playback = use_action(move || {
        let operation = playback_room.start_audio_playback();
        async move { operation.await }
    });
    let set_remote_track_publication_subscription =
        use_action(move |request: RemoteTrackPublicationSubscription| {
            let operation = subscription_room
                .remote_track_publication(request.participant_identity(), request.publication_sid())
                .map(|publication| publication.set_subscribed(request.subscribed()));
            async move {
                let operation = operation.ok_or(Error::InvalidState {
                    operation: "change Remote Track Publication subscription",
                    reason: "Track Publication is not current",
                })?;
                operation.await
            }
        });
    let set_publication_subscription_permissions = use_action(
        move |permissions: livekit_browser::PublicationSubscriptionPermissions| {
            let operation = permissions_room.set_publication_subscription_permissions(permissions);
            async move { operation.await }
        },
    );
    let set_local_participant_metadata = use_action(move |metadata: String| {
        let operation = metadata_participant.set_metadata(metadata);
        async move { operation.await }
    });
    let update_local_participant_attributes =
        use_action(move |attributes: BTreeMap<String, String>| {
            let operation = attributes_participant.update_attributes(attributes);
            async move { operation.await }
        });

    RoomSessionActions {
        leave,
        start_audio_playback,
        set_remote_track_publication_subscription,
        set_publication_subscription_permissions,
        set_local_participant_metadata,
        update_local_participant_attributes,
    }
}

fn use_media_attachment<Track, MediaElement, Attachment>(
    track: Track,
    on_error: Callback<Error>,
    attach: impl Fn(&Track, &MediaElement) -> Result<Attachment, Error> + Copy + 'static,
) -> Signal<Option<MediaElement>>
where
    Track: Clone + PartialEq + 'static,
    MediaElement: Clone + 'static,
    Attachment: 'static,
{
    let mut current_track = use_signal(|| track.clone());
    let element = use_signal(|| None::<MediaElement>);
    let mut attachment = use_signal(|| None::<Attachment>);

    if *current_track.peek() != track {
        current_track.set(track);
    }
    use_effect(move || {
        attachment.set(None);
        let track = current_track.read().clone();
        let Some(element) = element.read().clone() else {
            return;
        };
        match attach(&track, &element) {
            Ok(attached) => attachment.set(Some(attached)),
            Err(error) => on_error.call(error),
        }
    });

    element
}

/// Properties for an unstyled local video attachment node.
#[derive(Clone, Props, PartialEq)]
pub struct LocalVideoProps {
    pub track: LocalVideoTrack,
    #[props(default)]
    pub on_error: Callback<Error>,
    #[props(extends = video, extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,
}

/// Renders an unstyled `video` element attached to an existing Local Video Track.
#[allow(non_snake_case)]
pub fn LocalVideo(props: LocalVideoProps) -> Element {
    let mut element = use_media_attachment(props.track, props.on_error, LocalVideoTrack::attach);

    rsx! {
        video {
            onmounted: move |event| {
                element.set(Some(event.data().as_web_event().unchecked_into()));
            },
            ..props.attributes,
        }
    }
}

/// Properties for an unstyled local audio attachment node.
#[derive(Clone, Props, PartialEq)]
pub struct LocalAudioProps {
    pub track: LocalAudioTrack,
    #[props(default)]
    pub on_error: Callback<Error>,
    #[props(extends = audio, extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,
}

/// Renders an unstyled `audio` element attached to an existing Local Audio Track.
#[allow(non_snake_case)]
pub fn LocalAudio(props: LocalAudioProps) -> Element {
    let mut element = use_media_attachment(props.track, props.on_error, LocalAudioTrack::attach);

    rsx! {
        audio {
            onmounted: move |event| {
                element.set(Some(event.data().as_web_event().unchecked_into()));
            },
            ..props.attributes,
        }
    }
}

/// Properties for an unstyled remote video attachment node.
#[derive(Clone, Props, PartialEq)]
pub struct RemoteVideoProps {
    pub track: RemoteVideoTrack,
    #[props(default)]
    pub on_error: Callback<Error>,
    #[props(extends = video, extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,
}

/// Renders an unstyled `video` element attached to an existing Remote Video Track.
#[allow(non_snake_case)]
pub fn RemoteVideo(props: RemoteVideoProps) -> Element {
    let mut element = use_media_attachment(props.track, props.on_error, RemoteVideoTrack::attach);

    rsx! {
        video {
            onmounted: move |event| {
                element.set(Some(event.data().as_web_event().unchecked_into()));
            },
            ..props.attributes,
        }
    }
}

/// Properties for an unstyled remote audio attachment node.
#[derive(Clone, Props, PartialEq)]
pub struct RemoteAudioProps {
    pub track: RemoteAudioTrack,
    #[props(default)]
    pub on_error: Callback<Error>,
    #[props(extends = audio, extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,
}

/// Renders an unstyled `audio` element attached to an existing Remote Audio Track.
#[allow(non_snake_case)]
pub fn RemoteAudio(props: RemoteAudioProps) -> Element {
    let mut element = use_media_attachment(props.track, props.on_error, RemoteAudioTrack::attach);

    rsx! {
        audio {
            onmounted: move |event| {
                element.set(Some(event.data().as_web_event().unchecked_into()));
            },
            ..props.attributes,
        }
    }
}
