# livekit-browser

Framework-neutral Rust and WebAssembly bindings for [LiveKit](https://livekit.io)'s official browser
SDK.

LiveKit's Rust SDK targets native platforms and does not build for `wasm32`. This crate takes the
other route: it wraps the `livekit-client` browser SDK behind a typed Rust interface, so a Rust/WASM
application can join rooms, capture and publish media, exchange data, and observe room state without
writing JavaScript.

It is UI-framework agnostic. See [`livekit-browser-dioxus`](https://crates.io/crates/livekit-browser-dioxus)
for a Dioxus binding and [`livekit-browser-recording`](https://crates.io/crates/livekit-browser-recording)
for local clip recording.

## The host page must load the SDK

**This crate does not bundle JavaScript.** It calls into a `livekit-client` UMD build that your page
loads, and the version must match `LIVEKIT_CLIENT_VERSION` exactly:

```sh
npm install livekit-client@2.20.1
```

```html
<!-- Must load before the WASM module. -->
<script src="/assets/livekit-client.umd.min.js"></script>
```

`BrowserClient::initialize` validates that global and the required WebRTC support before any Room
Session or media state can exist, so a missing or mismatched SDK surfaces as `Error::SdkMissing` or
`Error::SdkVersionMismatch` rather than failing deep inside a later call.

Media capture additionally requires a secure context: HTTPS, or the `localhost` origin browsers treat
specially.

## Usage

Joining, capturing, and publishing are separate steps. Joining captures and publishes nothing,
capture works before or after joining, and unpublishing does not stop capture.

```rust
use livekit_browser::{
    BrowserClient, ConnectionCredentials, InitializeOptions, RoomOptions, TrackSource,
    VideoPublishOptions,
};

let client = BrowserClient::initialize(InitializeOptions::default())?;

let credentials = ConnectionCredentials::new("wss://example.livekit.cloud", token);
let session = client.join(credentials, RoomOptions::default()).await?;

let camera = client.create_camera_video_track().await?;
let publication = camera
    .publish(
        &session,
        VideoPublishOptions::default().with_source(TrackSource::Camera),
    )
    .await?;

publication.unpublish().await?;
session.leave().await?;
```

The browser should receive only a signaling URL and a participant token minted by your own server.
Never ship a LiveKit API secret to WASM.

## What it provides

- **Room Sessions** with join, leave, and automatic reconnection inside one session, plus immutable
  revisioned `RoomSnapshot`s and bounded lag-aware `RoomEvent`s that never silently drop changes.
- **Media** with independent camera, microphone, and display capture lifecycles, typed constraints,
  device restart and switching without replacing a publication, and distinguishable permission,
  unavailable-device, and device-in-use failures.
- **Replacement-safe remote handles** that become terminal instead of retargeting a replacement
  browser object, even when LiveKit reuses the logical ID.
- **Data Streams** routed by topic with real end-to-end backpressure: a write resolves only after the
  destination pulls that exact chunk.
- **Participant RPC** with native string or byte payloads and typed transport and application
  failures, outside the Room Event queue.
- **Chat Messages** and **Transcription Segments** on dedicated bounded streams reporting exact
  dropped counts.
- **Diagnostics**: connection quality in snapshots, on-demand sender and receiver statistics, and
  audio analysers returning owned waveform and RMS frames.

## Feature flags

| Feature | Default | Description |
| --- | --- | --- |
| `tracing` | off | Routes SDK logs to `tracing` under the `livekit_client` target |
| `raw-js` | off | Exposes the underlying `livekit-client` JavaScript objects |

`raw-js` has **no compatibility guarantee** beyond the exact pinned SDK. Calling methods on raw
handles can bypass mutation queues and error classification, desynchronize snapshots, replace or stop
media without updating Rust lifecycle state, remove listeners, invalidate analysers or attachments,
and prevent deterministic cleanup.

## Compatibility

Both endpoints of a Data Stream must run the same version of this crate. The flow-control contract
rides on private versioned topics, so stock `livekit-client` Data Stream handlers are not
wire-compatible with it.

See the [repository](https://github.com/sagikazarmark/livekit-browser-rs) for the browser capability
matrix and the SDK upgrade procedure.

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE) or
[MIT license](../../LICENSE-MIT) at your option.
