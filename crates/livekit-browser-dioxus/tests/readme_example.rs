//! Compile-checks the example in this crate's README.

use dioxus::prelude::*;
use livekit_browser::{BrowserCapabilities, RoomSession};
use livekit_browser_dioxus::{use_participants, RoomSessionProvider};

#[component]
#[allow(dead_code)]
fn Call(session: RoomSession, capabilities: BrowserCapabilities) -> Element {
    rsx! {
        RoomSessionProvider { session, capabilities, Roster {} }
    }
}

#[component]
#[allow(dead_code)]
fn Roster() -> Element {
    let participants = use_participants();

    rsx! {
        for (identity, _participant) in participants().remote().iter() {
            div { key: "{identity}", "{identity}" }
        }
    }
}
