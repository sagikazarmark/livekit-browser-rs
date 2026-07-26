use super::*;

/// The state change represented by a Room Event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "camelCase")]
#[non_exhaustive]
pub enum RoomEventKind {
    /// The Room Session's connection state changed.
    ConnectionStateChanged {
        /// The connection state at the tagged snapshot revision.
        state: RoomConnectionState,
    },
    /// Local Participant identity, SID, metadata, attributes, or grants changed.
    LocalParticipantChanged,
    /// The Local Participant's Track Publications changed.
    LocalTrackPublicationsChanged,
    /// A Remote Participant became visible in this Room Session.
    RemoteParticipantJoined {
        /// The stable Participant Identity that keys this participant in a Room Snapshot.
        identity: String,
    },
    /// A Remote Participant's metadata or attributes changed.
    RemoteParticipantChanged {
        /// The stable Participant Identity that keys this participant in a Room Snapshot.
        identity: String,
    },
    /// LiveKit's connection-quality estimate changed for a participant.
    ParticipantConnectionQualityChanged {
        /// The affected participant, which may be the Local Participant.
        participant_identity: String,
        /// The estimate at the tagged snapshot revision.
        quality: ConnectionQuality,
    },
    /// LiveKit's ordered active-speaker set changed.
    ActiveSpeakersChanged,
    /// A Remote Participant ceased to be visible in this Room Session.
    RemoteParticipantLeft {
        /// The stable Participant Identity that keyed this participant before it was removed.
        identity: String,
    },
    /// A Remote Participant's Track Publications or their availability changed.
    RemoteTrackPublicationsChanged {
        /// The Remote Participant whose publications changed.
        participant_identity: String,
    },
    /// A Remote Participant's separately published Data Tracks changed.
    RemoteDataTracksChanged {
        /// The Remote Participant whose Data Tracks changed.
        participant_identity: String,
    },
    /// Room metadata changed.
    RoomMetadataChanged,
    /// Browser autoplay policy for remote audio changed.
    AudioPlaybackChanged {
        /// Whether remote audio may currently play without another user activation.
        state: AudioPlaybackState,
    },
    /// This subscriber fell behind and must recover from `RoomSession::snapshot`.
    Lagged,
}

/// A bounded state notification tagged with the snapshot installed before it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoomEvent {
    pub(super) revision: u64,
    pub(super) kind: RoomEventKind,
}

impl RoomEvent {
    /// Returns the revision of the snapshot installed before this event.
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the state change represented by this event.
    pub const fn kind(&self) -> &RoomEventKind {
        &self.kind
    }
}

/// A browser-thread-local, bounded stream of Room Events.
pub struct RoomEventStream {
    pub(super) queue: Rc<RefCell<EventQueue>>,
}

impl Stream for RoomEventStream {
    type Item = RoomEvent;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut queue = self.queue.borrow_mut();
        if let Some(revision) = queue.lagged_revision.take() {
            return Poll::Ready(Some(RoomEvent {
                revision,
                kind: RoomEventKind::Lagged,
            }));
        }
        if let Some(event) = queue.events.pop_front() {
            return Poll::Ready(Some(event));
        }
        if queue.closed {
            return Poll::Ready(None);
        }

        queue.waker = Some(context.waker().clone());
        Poll::Pending
    }
}

pub(super) struct EventQueue {
    pub(super) capacity: usize,
    pub(super) events: VecDeque<RoomEvent>,
    pub(super) lagged_revision: Option<u64>,
    pub(super) closed: bool,
    pub(super) waker: Option<Waker>,
}

impl EventQueue {
    pub(super) fn push(&mut self, event: RoomEvent) {
        if self.lagged_revision.is_some() || self.events.len() == self.capacity {
            self.events.clear();
            self.lagged_revision = Some(event.revision);
        } else {
            self.events.push_back(event);
        }

        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}
