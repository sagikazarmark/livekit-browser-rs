# livekit-browser-dioxus

[![crates.io](https://img.shields.io/crates/v/livekit-browser-dioxus?style=flat-square)](https://crates.io/crates/livekit-browser-dioxus)
[![docs.rs](https://img.shields.io/docsrs/livekit-browser-dioxus?style=flat-square)](https://docs.rs/livekit-browser-dioxus)

**Headless [Dioxus](https://dioxuslabs.com) integration for
[`livekit-browser`](https://crates.io/crates/livekit-browser) Room Sessions.**

This crate is unstyled and renders no UI of its own. It provides a subtree-scoped Room Session
provider, reactive state hooks, typed actions, and bare media attachment nodes; how any of it looks
is entirely yours.

## Usage

Fetch credentials and join through `livekit-browser`, then hand the joined Room Session to
`RoomSessionProvider`. The provider owns only its subtree's observation and cleanup — it never joins,
leaves, captures, or publishes on its own.

```rust
use dioxus::prelude::*;
use livekit_browser::{BrowserCapabilities, RoomSession};
use livekit_browser_dioxus::{RemoteVideo, RoomSessionProvider, use_participants};

#[component]
fn Call(session: RoomSession, capabilities: BrowserCapabilities) -> Element {
    rsx! {
        RoomSessionProvider { session, capabilities, Roster {} }
    }
}

#[component]
fn Roster() -> Element {
    let participants = use_participants();

    rsx! {
        for (identity, _participant) in participants().remote().iter() {
            div { key: "{identity}", "{identity}" }
        }
    }
}
```

## Reading state

The provider subscribes to bounded Room Events and republishes the coherent `RoomSnapshot` installed
before each one, so consumers always read a self-consistent revision instead of reassembling state
from individual events.

- `use_room_snapshot` for the whole snapshot
- `use_participants`, `use_track_publications`, `use_audio_playback` for memoized slices
- `use_browser_capabilities` for permission-free capability gating
- `use_room_session` to reach the underlying handle for anything not covered above

These hooks panic when called outside a `RoomSessionProvider`, matching Dioxus context hooks.

## Changing state

`use_room_actions` returns `RoomSessionActions`, exposing Dioxus pending/result/error state for
common mutations. Each action starts its core operation synchronously before the returned waiter is
polled, which preserves browser user-activation requirements — important for `start_audio_playback`,
which must run inside a real user gesture.

## Attaching media

`LocalVideo`, `LocalAudio`, `RemoteVideo`, and `RemoteAudio` render bare `video` and `audio` elements
and attach an existing Media Track to them. Attachment is display only: mounting, unmounting, or
swapping a track never captures, publishes, mutes, or changes subscription policy. Attachment
failures go to `on_error` rather than panicking.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
