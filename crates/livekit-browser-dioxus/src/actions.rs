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
    /// Leaves the Room Session and refreshes the provided snapshot around the operation.
    pub leave: Action<(), ()>,
    /// Retries browser-blocked remote audio playback.
    ///
    /// Call this from a user activation, such as a click handler; browsers reject it otherwise.
    pub start_audio_playback: Action<(), ()>,
    /// Subscribes to or unsubscribes from one Remote Track Publication.
    ///
    /// Fails with [`Error::InvalidState`] when the addressed publication is no longer current in
    /// the snapshot, which happens when the participant left or the publication was replaced.
    pub set_remote_track_publication_subscription:
        Action<(RemoteTrackPublicationSubscription,), ()>,
    /// Replaces the publisher-side policy for who may subscribe to local Track Publications.
    ///
    /// Applies to current and future publications; it does not stop capture or unpublish anything.
    pub set_publication_subscription_permissions:
        Action<(livekit_browser::PublicationSubscriptionPermissions,), ()>,
    /// Replaces Local Participant metadata.
    ///
    /// The value must be non-empty: LiveKit treats an empty metadata string as no update.
    pub set_local_participant_metadata: Action<(String,), ()>,
    /// Patches Local Participant attributes.
    ///
    /// Follows LiveKit's patch semantics, where an empty value deletes that attribute.
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
    let start_audio_playback = use_action(move || playback_room.start_audio_playback());
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
            permissions_room.set_publication_subscription_permissions(permissions)
        },
    );
    let set_local_participant_metadata =
        use_action(move |metadata: String| metadata_participant.set_metadata(metadata));
    let update_local_participant_attributes =
        use_action(move |attributes: BTreeMap<String, String>| {
            attributes_participant.update_attributes(attributes)
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
