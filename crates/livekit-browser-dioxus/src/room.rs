use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use dioxus::{core::Task, prelude::*};
use futures_util::StreamExt;
use livekit_browser::{
    AudioPlaybackState, BrowserCapabilities, LocalParticipantSnapshot,
    LocalTrackPublicationSnapshot, RemoteParticipantSnapshot, RemoteTrackPublicationSnapshot,
    RoomSession, RoomSnapshot,
};

const DEFAULT_EVENT_CAPACITY: usize = 64;

#[derive(Clone)]
pub(crate) struct RoomSessionContext {
    pub(crate) session: Signal<RoomSession>,
    pub(crate) snapshot: Signal<Rc<RoomSnapshot>>,
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
