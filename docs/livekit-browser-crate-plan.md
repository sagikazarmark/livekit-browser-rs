# LiveKit Browser Crate Plan

**Status:** Confirmed design for private workspace extraction

This plan turns the working prototype into framework-neutral Rust/WASM modules without promising public compatibility yet. Domain terms are defined in [`CONTEXT.md`](../CONTEXT.md); the architectural rationale is recorded in [ADR 0001](adr/0001-wrap-livekit-browser-sdk.md).

## Goals

- Provide an opinionated browser conferencing interface rather than JavaScript SDK parity.
- Cover complete media, messaging, device, permission, metadata, and telemetry workflows.
- Keep browser and LiveKit lifecycle rules explicit in Rust types.
- Keep Dioxus optional and headless.
- Keep Local Clip Recording distinct from Room Recording.
- Preserve the possibility of replacing the private JavaScript implementation without introducing a speculative backend abstraction.

## Non-Goals

- Source compatibility with LiveKit's native Rust SDK.
- Token generation, LiveKit administration, SIP, Ingress, or Egress.
- Dioxus SSR or native Dioxus renderers in the initial extraction.
- Exhaustively typing browser-specific `RTCStatsReport` dictionaries.
- Typed third-party track processors in the initial interface.
- A public crates.io stability or MSRV promise.

## Workspace Shape

```text
livekit-proto/
├── crates/
│   ├── livekit-browser/
│   ├── livekit-browser-dioxus/
│   └── livekit-browser-recording/
├── examples/
│   └── dioxus-room/ (full-stack web app with server-side token issuance)
└── tests/
```

Dependency edges:

```text
livekit-browser-dioxus ───────> livekit-browser
livekit-browser-recording ────> livekit-browser
examples/dioxus-room ─────────> all three crates
livekit-browser ──────────────> application-loaded livekit-client 2.20.1
```

There is no initial `-sys` crate. The `wasm-bindgen` declarations and JavaScript adapter have one consumer and remain private implementation inside `livekit-browser`. The Dioxus and recording crates do not depend on each other.

## JavaScript Contract

The consuming application owns JavaScript installation and bundling:

1. Pin `livekit-client` exactly to 2.20.1 in the application lockfile.
2. Copy its UMD bundle from `node_modules` into locally served assets during the application build.
3. Load the UMD script synchronously before the WASM module. Runtime CDN loading is unsupported.
4. Call `BrowserClient::initialize`; initialization verifies `globalThis.LivekitClient`, exact SDK version, and browser support before creating any session or media state.

`livekit-browser` ships a private browser orchestration ES module through `wasm-bindgen`. It imports no npm modules and references the application-loaded global. It owns SDK and browser method calls, listener registration, listener cleanup, object registries, normalization into plain JavaScript values, and browser-event state or versioned wrapper protocols needed to provide contracts absent from the pinned SDK. Plain options, snapshots, and events cross with `serde-wasm-bindgen`; live room and media objects cross only as private opaque handles.

The adapter must never expose the API secret. The application obtains participant credentials elsewhere and passes only a server URL and participant token into the browser module.

## Core Interface

The following sketches communicate ownership and depth; exact names may change during tracer implementation.

```rust
let client = BrowserClient::initialize(InitializeOptions::default())?;
let capabilities = client.capabilities();

let session = client
    .join(ConnectionCredentials { server_url, participant_token }, RoomOptions::default())
    .await?;

let snapshot = session.snapshot();
let events = session.subscribe_events(128);
let local = session.local_participant();
```

`BrowserClient` is initialized once per WASM application and may create multiple Room Sessions and Local Media Tracks. It owns SDK/version/logging integration but no participant credentials. Initialization does not request browser media permissions.

`RoomSession` is a cloneable shared handle for one participation lifetime. Automatic LiveKit reconnection remains within the same session. Calling `leave` from any clone is terminal. Explicit `leave().await` provides deterministic cleanup; final drop removes listeners and schedules best-effort disconnect. Handles are `!Send` and `!Sync` and return runtime-agnostic local `Future` and `Stream` values without requiring Tokio.

Joining captures and publishes no media. Publication is always a separate operation, so a successful join cannot be obscured by partial media-publication failure.

## State And Events

`RoomSnapshot` is immutable Rust data with a monotonically increasing revision. It includes:

- Room state and metadata.
- Local Participant identity, SID, metadata, attributes, grants, and current Track Publications.
- Remote Participants keyed by Participant Identity, with server SID retained separately.
- Audio and video Track Publication state keyed by publication SID, with Data Tracks modeled separately by data-track SID.
- Active speakers, audio playback availability, and participant connection quality.

Snapshots contain no `JsValue` or live object handles. The adapter coalesces state callbacks from the same JavaScript turn, produces one normalized snapshot, installs it in Rust, and only then emits detailed Room Events tagged with that snapshot revision.

Each Room Event subscription has configurable bounded capacity. A lagging subscriber receives `Lagged` and recovers from `RoomSession::snapshot`; Room Events are not a durable journal. Data packets, text/byte streams, chat, transcription, and RPC invocations use dedicated delivery interfaces and are never routed through the lossy state-event stream.

At the initial supported scale of approximately 100 participants, correctness through coalesced full-snapshot normalization is preferred over an event reducer. Browser profiling must precede any incremental-state optimization.

## Participant And Publication Model

Local and Remote Participants are different types. Only `LocalParticipant` exposes capture, publication, metadata mutation, and publisher-side subscription-permission commands. Application roles such as instructor and student remain outside these crates; the Local Participant snapshot exposes LiveKit's source-level grants, and denied commands return typed permission failures.

Track Publications and Media Tracks remain distinct. A publication can be announced, muted, unsubscribed, or temporarily unavailable without an attachable Remote Media Track. Session lookup resolves an available Remote Media Track from a publication SID.

Participant Identity is the logical snapshot key. LiveKit SID identifies the current server object. If reconnect replaces a participant, publication, or media object, old handles become terminal and new IDs/handles arrive through snapshots and events; handles never silently retarget by participant/source.

## Local Media

`BrowserClient` creates room-independent Local Media Tracks before or after joining. Audio and video are distinct concrete handle types, with enums for heterogeneous collections. Camera, microphone, display video, and system audio are explicit sources.

Local Media Track properties:

- Cloneable shared ownership of one underlying capture.
- Explicit `stop` is terminal for all clones.
- Dropping the final owner stops capture as a fallback.
- A track may be previewed, analysed, or recorded without publication.
- A track may be published into at most one Room Session at a time.
- Unpublishing keeps capture alive; muting keeps the Track Publication alive; stopping ends capture.

Screen capture returns a local media set containing required display video and optional browser-provided system audio. Camera/microphone enumeration, labels, active-device switching, facing mode, and track restart belong to core. Common capture constraints are typed; uncommon browser constraints require the `raw-js` feature.

Publication options type source, name, simulcast, codec preference, standard encodings, DTX, and RED. Uncommon SDK options require `raw-js`. Core provides precise capture/publish/mute/unpublish/stop primitives plus common camera and microphone enable conveniences built from them.

Mutations are serialized per affected resource. Independent Local Media Tracks may proceed concurrently. Dropping a Rust operation future does not cancel its JavaScript Promise; any resulting state change remains observable. Explicit cancellation exists only where LiveKit supports it.

## Media Attachment

The generic browser crate, not the Dioxus adapter, owns browser element attachment:

```rust
let attachment = remote_video.attach(&html_video_element)?;
```

Audio and video handles accept their corresponding `web-sys` media element types and return an attachment guard that detaches on explicit close or drop. Attachment lifetime is independent of capture, subscription, and publication. Replaced/ended media handles terminate their attachments.

Dioxus media nodes only obtain mounted elements and hold attachment guards. Mounting a node never changes network subscription state.

## Subscription And Permissions

Automatic subscription is the default conferencing policy. Callers can selectively enable or disable each remote Track Publication. Core also exposes typed publisher-side subscription permissions for all participants or participant-specific access.

Metadata and attributes are present in snapshots. The Local Participant exposes permitted update operations, with LiveKit grants remaining authoritative.

## Messaging

Core includes distinct typed workflows for:

- Reliable and lossy byte data packets.
- Incoming and outgoing text streams.
- Incoming and outgoing byte streams.
- RPC registration and invocation.
- Chat send/receive conveniences.
- Transcription segment updates.

Payloads remain native bytes and strings. The crate does not impose JSON, Serde, protobuf, or a codec trait on application payloads. Handlers and streams are browser-thread-local and have explicit bounded buffering or stream backpressure rather than sharing Room Event delivery. Because `livekit-client` 2.20.1 write completion does not report destination application consumption, Data Streams use a private versioned acknowledgement protocol and require both endpoints to use the same wrapper version.

## Telemetry And Audio

The typed interface covers all normalized audio/video sender and receiver statistics exposed by `livekit-client`, while preserving raw browser `RTCStatsReport` access only under `raw-js`. Detailed stats are queried through participant/publication/track workflows rather than copied into every Room Snapshot.

Active-speaker and participant connection-quality state is included in snapshots/events. Local and Remote Audio Tracks can create typed analyser handles for level/waveform data; analysers have explicit close and drop cleanup.

An optional `tracing` feature maps LiveKit JavaScript log levels and structured context into Rust `tracing`. Applications can install their own log callback. JavaScript console logging remains the SDK fallback.

## Local Clip Recording

`livekit-browser-recording` accepts selected Local Participant audio/video tracks whether or not they are published. Remote Media Tracks are rejected; multi-participant capture belongs to Room Recording/Egress.

The recorder models start, pause, resume, automatic stop, and explicit stop. Options contain ordered MIME preferences and optional duration/accumulated-size limits. The first browser-supported format is selected and the actual MIME type is reported.

Stopping returns a `LocalClip` containing the browser `Blob`, actual MIME type, duration, stop reason, and filename suggestion. It does not copy the artifact into WASM memory, create a download link, upload, or persist data. Object URLs are created through a revocable guard.

## Dioxus Adapter

`livekit-browser-dioxus` targets client-side Dioxus Web. It declares a `0.7` dependency range but tests only the currently resolved latest 0.7 release; compatibility with older 0.7 releases is best-effort, not guaranteed.

The headless adapter provides:

- A provider that receives an already joined Room Session and distributes it within one subtree.
- Hooks for the nearest session, current snapshot, participants, publications, capabilities, playback status, and actions.
- Unstyled local/remote audio and video nodes that only manage attachment guards.
- No participant grid, controls, permission prompts, token fetching, recording UI, or CSS.

The application performs the asynchronous join and supplies the resulting session. A provider owns cleanup for its subtree, while any session clone may explicitly leave.

## Optional And Deferred Features

E2EE is planned as a later optional core feature because it affects Room Session construction and requires separately handled worker assets. Initial option types remain extensible but do not include nonfunctional placeholder encryption settings.

Track processor libraries are accepted only as raw handles under `raw-js` initially. Typed processor lifecycles wait for a concrete processor dependency.

The `raw-js` feature exposes explicitly unstable room, participant, publication, track, option, processor, and stats handles. It can violate wrapper invariants and carries no compatibility guarantee. Normal workflow interfaces contain no `JsValue`.

## Error Model

One non-exhaustive error enum supplies actionable categories such as:

- SDK missing or exact-version mismatch.
- Unsupported browser capability.
- Invalid or terminal handle.
- Browser permission denied, device unavailable, or device in use.
- Join, publication, subscription, permission, messaging, RPC, and recording failures.
- Operation invalid for current Room Session, Track Publication, Media Track, or recorder state.

Subscriber lag is a typed item in each continuing bounded delivery stream, not an operation failure in this enum. Room Event lag directs recovery through the current `RoomSnapshot`; dedicated message streams report their own dropped-item counts.

Unknown JavaScript failures retain name, message, and raw cause internally; the raw `JsValue` is exposed only with `raw-js`. Separate exhaustive error enums per operation are intentionally avoided.

## Verification

Three verification layers are required:

1. Pure Rust tests for normalized data conversion, lifecycle state machines, snapshot revisions, event lag, permissions, and error classification.
2. Lower adapter and browser contract tests against the exact UMD SDK for initialization, runtime shape inspection, raw Room and Local Participant identity, object lifetimes, listeners, snapshots, commands, attachment, recording, and cleanup. Focused hand-written SDK objects exercise adapter behavior and failure injection that cannot be isolated through the real browser workflow.
3. Playwright E2E tests against the local Docker LiveKit stack with two participants and fake media.

Chromium, Firefox, and WebKit are gating engines. Mobile-sized Chromium and WebKit scenarios verify responsive consumers and browser capability differences, but do not substitute for manual tests on physical iOS and Android devices.

## Tracer Slices

### 0. Establish The Workspace And Contract Harness

- Convert the prototype into a Cargo workspace and move the current application to `examples/dioxus-room` without changing behavior.
- Add the three private crate manifests and one application-owned npm lockfile pinned to `livekit-client` 2.20.1.
- Add one command that installs/copies the UMD artifact, builds the workspace, starts Docker, and runs browser tests.
- Preserve the existing Playwright publish/recording smoke check as the baseline.

Exit: existing prototype behavior and tests pass from the new layout before extraction begins.

### 1. Initialize And Join A Room Session

- Implement the crate-owned JS adapter, exact version check, typed capabilities, BrowserClient, participant credentials, Room Session join/leave/drop, and typed error foundation.
- Implement immutable connection/local-participant snapshots and snapshot-first bounded events.
- Move token fetching outside the room module in the example.

Exit: each browser engine joins and leaves; listener counts and terminal cleanup are verified.

### 2. Observe Remote Participants And Media

- Normalize Remote Participants and Track Publications with identity/SID semantics.
- Implement subscribed Remote Audio/Video Track handles, terminal replacement behavior, attachment guards, active speakers, playback blocking/unlock, and automatic/selective subscription.
- Use two Playwright participants to verify remote playback and reconnect replacement.

Exit: two browsers see each other and exchange attached fake audio/video over local LiveKit.

### 3. Capture And Publish Local Media

- Implement room-independent Local Audio/Video Tracks, shared ownership, explicit stop, preview attachment, and publication primitives.
- Add common camera/microphone constraints, devices, switching, grants, publish options, mute/unpublish semantics, and convenience workflows.
- Add display video/system audio capture and publisher-side subscription permissions.

Exit: join-without-media, pre-join preview, post-join capture, denied student publication, instructor camera/mic/screen, switching, unpublish-with-preview, and hardware-release scenarios pass.

### 4. Add Messaging And Application State

- Implement dedicated data packet, text/byte stream, RPC, chat, and transcription workflows with bounded delivery/backpressure.
- Add participant and room metadata/attributes plus permitted local updates.

Exit: two participants exchange each payload family, large streams obey backpressure, RPC timeouts/errors classify correctly, and none use the Room Event queue.

### 5. Add Telemetry And Local Recording

- Type LiveKit-normalized sender/receiver statistics, connection quality, and audio analyser lifecycle.
- Implement `livekit-browser-recording`, format negotiation, limits, recorder lifecycle, Blob artifacts, and object URL guards.

Exit: stats are populated for published/subscribed fake media; camera/mic recording pauses, resumes, auto-stops, and produces a non-empty downloadable Blob in all supported engines where MediaRecorder is available.

### 6. Extract The Dioxus Adapter

- Implement provider, hooks, and unstyled attachment nodes over the now-tested generic interface.
- Rewrite the example so no application code calls the JavaScript adapter or LiveKit global directly.
- Remove the throwaway polling bridge after parity is demonstrated.

Exit: the example uses only the three Rust crate interfaces, retains desktop/mobile behavior, and has no Dioxus dependency in core or recording.

### 7. Harden Compatibility And Escape Hatches

- Add `tracing` and `raw-js` features, raw processor/options/stats handles, exact-version mismatch tests, browser capability fixtures, and 100-participant snapshot profiling.
- Document browser matrix, user-activation requirements, UMD setup, cleanup rules, unsupported features, and upgrade procedure.

Exit: all three engines gate CI; unsupported capabilities fail before mutation; profile results validate full snapshot normalization at target scale.

### 8. Add E2EE Separately

- Design typed encryption configuration and key-provider ownership from a concrete application requirement.
- Add application-owned E2EE worker asset installation/version checks and browser E2E coverage.

Exit: encrypted participants exchange media/data across the supported browser matrix without weakening the unencrypted default path.

## Upgrade Procedure

An upgrade of `livekit-client` is one atomic change:

1. Update the application npm dependency and lockfile.
2. Update the expected version in `livekit-browser`.
3. Run adapter contract tests in all three browser engines.
4. Run local-LiveKit two-participant E2E tests.
5. Review upstream release notes and TypeScript declarations for events, options, errors, stats, worker assets, and behavior changes.
6. Update normalized Rust types and the adapter before accepting the upgrade.

No SDK version range is accepted until multiple versions have been deliberately tested and a concrete compatibility policy replaces exact coupling.
