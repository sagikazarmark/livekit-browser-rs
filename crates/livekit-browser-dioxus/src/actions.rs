use std::collections::BTreeMap;

use dioxus::prelude::*;
use livekit_browser::Error;

use crate::room::RoomSessionContext;

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
