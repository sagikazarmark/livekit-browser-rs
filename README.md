# livekit-browser-rs

Rust and WebAssembly bindings for [LiveKit](https://livekit.io)'s official browser SDK.

LiveKit's Rust SDK targets native platforms and does not build for `wasm32`. This workspace takes
the other route: it wraps the `livekit-client` browser SDK behind a typed, framework-neutral Rust
interface, so a Rust/WASM application can join rooms, capture and publish media, exchange data, and
observe room state without writing JavaScript.

## Crates

| Crate | Description |
| --- | --- |
| [`livekit-browser`](crates/livekit-browser) | Framework-neutral core: Room Sessions, media, state, data, RPC |
| [`livekit-browser-dioxus`](crates/livekit-browser-dioxus) | Headless [Dioxus](https://dioxuslabs.com) provider, hooks, actions, and media nodes |
| [`livekit-browser-recording`](crates/livekit-browser-recording) | Local Clip Recording of Local Media Tracks into browser-owned artifacts |

## Requirements

- The `wasm32-unknown-unknown` target. These crates are browser-only; they compile for host targets
  so tests and tooling work, but every operation returns `Error::UnsupportedTarget` there.
- A secure context for media capture: HTTPS, or the `localhost` origin browsers treat specially.
- **A host page that loads `livekit-client` 2.20.1 as a UMD bundle.** See below.

## Installation

```sh
cargo add livekit-browser
```

This crate does **not** bundle JavaScript. It calls into a `livekit-client` UMD build that your page
loads, and the version must match exactly. Add the SDK as an application dependency and serve its
UMD artifact:

```sh
npm install livekit-client@2.20.1
```

```html
<!-- Must load before the WASM module. -->
<script src="/assets/livekit-client.umd.min.js"></script>
```

`BrowserClient::initialize` validates that global and the required WebRTC support before any Room
Session or media state can exist, so a missing or mismatched SDK surfaces as `Error::SdkMissing` or
`Error::SdkVersionMismatch` rather than failing deep inside a later call. The exact required version
is exported as `LIVEKIT_CLIENT_VERSION`.

See [Browser Compatibility](docs/browser-compatibility.md) for UMD setup and load order, the browser
capability matrix, and the atomic SDK upgrade procedure.

## Quick start

Joining, capturing, and publishing are separate steps. Joining captures and publishes nothing,
capture works before or after joining, and unpublishing does not stop capture.

```rust
use livekit_browser::{
    BrowserClient, ConnectionCredentials, InitializeOptions, RoomOptions, TrackSource,
    VideoPublishOptions,
};

// Validates the loaded SDK and browser support. Requests no permissions.
let client = BrowserClient::initialize(InitializeOptions::default())?;

// Joining captures and publishes no media.
let credentials = ConnectionCredentials::new("wss://example.livekit.cloud", token);
let session = client.join(credentials, RoomOptions::default()).await?;

// Capture is independent of the Room Session and of publication.
let camera = client.create_camera_video_track().await?;
let publication = camera
    .publish(
        &session,
        VideoPublishOptions::default().with_source(TrackSource::Camera),
    )
    .await?;

// Unpublishing keeps the local capture and preview alive.
publication.unpublish().await?;
session.leave().await?;
```

The browser should receive only a signaling URL and a participant token minted by your own server.
Never ship a LiveKit API secret to WASM.

## What it provides

### Room Sessions and state

Join, leave, and automatic reconnection stay within one Room Session and one underlying browser Room
object. `RoomSession::snapshot` returns an immutable, internally coherent `RoomSnapshot` carrying a
monotonic revision: room and participant metadata and attributes, publisher grants, connection
quality, active speakers, and Track Publications.

`RoomSession::subscribe_events` delivers bounded `RoomEvent`s tagged with the revision installed
before each one. A subscriber that falls behind receives `RoomEventKind::Lagged` and recovers from
the current snapshot, so it never silently misses changes, and lag on one subscriber does not affect
others.

### Media

Camera, microphone, and display capture have independent lifecycles, each with typed capture
constraints and actionable errors. Permission denied, unavailable device, and device-in-use remain
distinguishable, and failed mutations roll back rather than leaving partial state.

Active inputs can restart or switch devices without replacing their Room Session or Track
Publication. Muting preserves the publication; unpublishing preserves capture; stopping releases the
browser resource. Each Local Media Track can publish into at most one Room Session, and mutations are
serialized per track.

Display capture returns required screen video plus optional system or tab audio — optional even when
requested, since browsers and display sources differ in what they provide. It must start from an
active, focused user interaction. Chooser cancellation and policy denial share the browser's
`NotAllowedError` and are reported as one typed outcome.

Remote handles are replacement-safe: when LiveKit replaces the underlying object, the old handle
becomes terminal instead of silently retargeting the new one, even when the logical ID is reused.
Attachment to browser elements is display only and never changes subscription policy.

### Data, messaging, and RPC

**Data Streams** route by application topic and retain native string or byte payloads without
imposing a codec. Each stream targets one Remote Participant, and each write is limited to LiveKit's
15,000-byte protocol chunk. A private reliable acknowledgement resolves a write only after the
destination pulls that exact chunk, giving real end-to-end backpressure.

> Both endpoints must run the same version of this crate. The flow-control contract rides on private
> versioned topics, so standard `livekit-client` Data Stream handlers are not wire-compatible.

**Participant RPC** routes one request to a named handler on one Remote Participant and returns a
native string or byte payload without entering the Room Event queue. Requests and responses are
capped at 15,360 encoded bytes; larger content belongs on a Data Stream. Unsupported methods,
unavailable participants, timeouts, remote application errors, and transport failures stay
distinguishable through `RpcFailureKind`.

**Chat Messages** and **Transcription Segments** each have a dedicated per-subscriber bounded stream
that reports exact dropped counts, never competes with Room Event capacity, and closes when the Room
Session begins leaving.

### Diagnostics

Participant connection quality is coherent revisioned snapshot state. Detailed media health is on
demand: published local publications expose typed sender statistics, subscribed remote tracks expose
typed receiver statistics, and neither is copied into every snapshot. Local and Remote Media Tracks
carrying audio can create cloneable Audio Analysers returning owned waveform and RMS level frames.

`InitializeOptions::with_log_callback` installs an application-owned callback receiving typed
`LiveKitLogRecord` values. The optional `tracing` feature adds `InitializeOptions::with_tracing`,
mapping SDK levels to `tracing` events under the `livekit_client` target.

## Feature flags

| Feature | Default | Description |
| --- | --- | --- |
| `tracing` | off | Routes SDK logs to `tracing` under the `livekit_client` target |
| `raw-js` | off | Exposes the underlying `livekit-client` JavaScript objects |

`raw-js` has **no compatibility guarantee** beyond the exact pinned SDK. Calling methods on raw
handles can bypass mutation queues and error classification, desynchronize snapshots, replace or stop
media without updating Rust lifecycle state, remove listeners, invalidate analysers or attachments,
and prevent deterministic cleanup. Isolate those operations and revalidate wrapper state afterward.
Raw option values are additive, and typed wrapper fields win on collisions.

## Architecture

```text
Dioxus Rust UI (WASM)
        |
        | provider, hooks, actions, attachment nodes
        v
livekit-browser-dioxus
        |
        | framework-neutral Rust interface
        v
livekit-browser
        |
        | private narrow wasm-bindgen API
        v
LiveKit browser SDK + private normalization adapter
        |
        | HTTPS/WSS
        v
LiveKit server
```

The application, not the crate, owns the JavaScript dependency. `livekit-browser` talks to the SDK
only through a private normalization adapter written in TypeScript and bundled into
`crates/livekit-browser/js/livekit-browser.js`. Its data contracts are generated from the Rust DTOs,
so the Rust and TypeScript sides cannot drift silently.

## Demo application

The repository ships a Dioxus full-stack demo covering every supported workflow, with a local
LiveKit server, a Caddy TLS gateway for phone testing, and a Playwright compatibility gate. It needs
only Docker:

```sh
./run-prototype.sh
```

It also runs directly with `dx serve --fullstack`, and deploys to Cloudflare Workers against a
LiveKit Cloud project. `demo/` is a Dagger module, so the build, the local server, and the Worker
build, dev server, and deployment all run from `dagger call` without a host toolchain.

See [Demo Application](docs/demo.md) for the guided checklist, LiveKit Cloud setup, running it
locally with `dx`, deploying it to Cloudflare Workers, phone testing with a local certificate
authority, network ports, and the automated check.

The default setup is deliberately insecure for production: it uses LiveKit's public development key
pair and an unauthenticated token endpoint. Do not expose it beyond a trusted development network.

## Documentation

- [Browser Compatibility](docs/browser-compatibility.md) — supported browsers, capability matrix,
  UMD setup, unsupported workflows, and the SDK upgrade procedure
- [Demo Application](docs/demo.md) — running the demo stack and the compatibility gate
- [Domain language](CONTEXT.md) — the vocabulary these crates use, and the terms they avoid
- [ADR 0001](docs/adr/0001-wrap-livekit-browser-sdk.md) — why this wraps the browser SDK

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT)
at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this
project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
