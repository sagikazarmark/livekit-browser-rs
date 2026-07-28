# Browser Compatibility

This document is the compatibility contract for the exact `livekit-client` version wrapped by
`livekit-browser`. A passing run of the compatibility gate — `dagger check` from `demo/` — is
required before changing the browser SDK, normalized adapter, Rust interface, or Dioxus consumer.
The gate itself is the Playwright suite in `demo/e2e`; see [Demo Application](demo.md) for how to
run it, including the Firefox and WebKit projects the Dagger check does not install.

## Supported Browsers

The gate uses the browser revisions bundled with Playwright 1.61.1. The matrix describes
the workflows the wrapper gates, not every device and codec combination available in those browser
families.

| Workflow | Chromium | Firefox | WebKit |
| --- | --- | --- | --- |
| Initialization and exact SDK validation | Required | Required | Required |
| Room Session join, reconnection, leave, and cleanup | Required | Required | Required |
| Camera and microphone capture and publication | Required | Required | Required |
| Display capture | Required when the browser exposes `getDisplayMedia` | Required when the browser exposes `getDisplayMedia` | Required when the browser exposes `getDisplayMedia` |
| Remote media, attachment, and playback recovery | Required | Required | Required |
| Reliable/lossy byte delivery, Data Streams, Participant RPC, Chat Messages, and Transcription Segments | Required | Required | Required |
| Connection quality, sender/receiver statistics, and Audio Analysis | Required | Required | Required |
| Local Clip Recording | Required when `MediaRecorder` and a requested MIME type are available | Required when `MediaRecorder` and a requested MIME type are available | Required when `MediaRecorder` and a requested MIME type are available |
| Dioxus provider, hooks, actions, media nodes, and cleanup | Required | Required | Required |

Mobile-sized Chromium and WebKit projects use a `390 x 844` touch viewport. They verify responsive
consumption, Room Session state, and capability-driven optional controls. Browser emulation does not
validate real cameras, microphones, display choosers, hardware codecs, operating-system permission
prompts, backgrounding, or network transitions. Follow the physical iOS and Android guidance in
[Demo Application](demo.md) before release.

Capabilities are detected without requesting permission. Camera and microphone capture, display
capture, audio output selection, Transcription Segments, and Local Clip Recording remain conditional
on the reported browser APIs and the exact loaded SDK contract. Missing mutation capabilities reject
their workflow before a permission request, Room Session, media object, or recorder is mutated.
Unavailable observational capabilities such as Transcription Segments are reported as unavailable;
they do not prevent otherwise supported Room Sessions.

## UMD Installation

The application, rather than the Rust crate, owns the exact JavaScript dependency:

```sh
cd demo
npm install --save-exact livekit-client@2.20.1
```

Commit both `package.json` and `package-lock.json`. Copy
`node_modules/livekit-client/dist/livekit-client.umd.js` into the application's `public` directory,
which `dx` copies verbatim into the served root; for the example, `npm run assets` performs that
copy and `Dockerfile.web` performs it in the container build. Do not fetch a floating CDN artifact
at runtime.

The UMD script must load synchronously before the WASM bootstrap. The Dioxus example declares it in
`demo/Dioxus.toml`:

```toml
[web.resource]
script = ["/livekit-client.umd.min.js"]
```

`BrowserClient::initialize` checks `globalThis.LivekitClient`, requires version `2.20.1`, validates
every required exported constructor, method, event, enum, and WebRTC capability, and only then
installs client state. LiveKit defines `connect`, `disconnect`, and `startAudio` as Room-instance
fields rather than prototype methods, so the wrapper validates those immediately after Room
construction and before registering the Room, listeners, or connection state. Unexported Data
Stream reader and writer contracts are similarly validated before wrapper handles are installed.
A missing global, version mismatch, inspectable API mismatch, or unsupported WebRTC runtime is a
terminal initialization failure. The browser contracts assert that preflight failures construct no
Room, Local Media Track, `MediaStream`, or `MediaRecorder` and request no media or playback
permission.

## User Activation

- Call display capture directly from the click or equivalent trusted user interaction that starts
  screen sharing. Do not put unrelated asynchronous work before the browser chooser.
- Treat display chooser cancellation and browser policy denial as the same typed outcome when the
  browser reports `NotAllowedError`.
- Call `RoomSession::start_audio_playback` from a user interaction after blocked remote audio is
  observed. Autoplay policy varies by browser and user settings.
- Create an Audio Analyser from an active user interaction when the browser initially suspends its
  `AudioContext`; retrying an existing blocked context is preferable to creating unmanaged contexts.
- Camera and microphone permission prompts still depend on a secure context. `localhost` is special;
  physical mobile consumers must use the documented HTTPS endpoint and trusted local CA.

## Lifecycle Cleanup

- Explicitly leave each Room Session. Final-handle drop is a best-effort fallback that schedules
  disconnect and listener removal.
- Explicitly stop Local Media Tracks when capture is no longer needed. Unpublishing does not stop
  capture, and leaving a Room Session does not destroy independently owned Local Media Tracks.
- Close local and remote attachment guards when detaching elements. Attachment cleanup never changes
  publication or subscription policy.
- Close Audio Analysers to await `AudioContext` shutdown. Source termination and final drop provide
  fallback cleanup.
- Stop Local Clip Recording and await its completion before consuming the Blob. Dropping the final
  active recorder stops encoding, removes callbacks, and discards partial chunks.
- Revoke each `RevocableObjectUrl` after download. Final guard drop revokes it as a fallback.
- Drop Data Stream handlers, Participant RPC registrations, Chat Message and Transcription Segment
  subscribers, and Dioxus providers when their owning feature or subtree is removed. Room Session
  leave closes the remaining registrations and subscribers.

The wrapper's Data Stream flow control uses private `~livekit-browser/flow/v1/*` topics between two
participants running the same wrapper version. Standard LiveKit clients do not interoperate with
these private stream topics. Native SDK open, write, and close calls fail after 30 seconds without
browser transport progress, and the initial receiver handshake fails after 10 seconds. Once a
native write is accepted, waiting for the destination application to pull it has no deadline.

## Unsupported Workflows

This extraction does not provide server-side Room Recording, Egress control, Ingress, SIP, agents,
administrative APIs, native desktop/mobile clients, or a pure-Rust browser signaling/WebRTC stack.
Local Clip Recording is browser-local and is not Room Recording. End-to-end encryption remains a
separate future slice because it requires application-owned worker installation and key ownership.

The wrapper does not promise a particular display-audio source, codec, bitrate, device label, audio
output selector, or autoplay result. These are browser, operating-system, hardware, and policy
capabilities. Transcription Segments are unavailable when the exact SDK lacks its normalized event.

The optional `raw-js` feature is deliberately unstable. Raw objects can bypass mutation queues and
error classification, desynchronize Room Snapshots, replace or stop media behind Rust handles,
remove listeners, retain terminal JavaScript objects, and invalidate attachments or Audio Analysers.
Isolate raw operations and revalidate typed state afterward; no compatibility is promised beyond the
exact pinned SDK.

## Snapshot Profile

`demo/e2e/snapshot-normalization.spec.mjs` profiles the public Room Session/Dioxus consumption boundary
in each desktop engine. One joined Local Participant is combined with 99 synthetic Remote
Participants, two Track Publications per Remote Participant, two attributes per participant, and
fixed metadata. The fixture therefore represents 100 participants and 198 remote Track
Publications without adding signaling or WebRTC load to the normalization measurement.

Each sample emits two same-turn room metadata callbacks. The contract requires one new Room Snapshot
revision, two Room Events tagged with that installed revision, all 99 Remote Participants and 198
Track Publications still present, and the final metadata visible to the Dioxus consumer. Three
updates warm generation maps and rendering; 15 measured updates report p50, nearest-rank p95, and
maximum trigger-to-Dioxus-commit latency. The initial p95 target is 250 ms in every engine on the
containerized local gate. This intentionally includes full JavaScript normalization, JS-to-WASM
conversion, Rust installation, event delivery, Dioxus consumption, and DOM commit.

Results from the containerized local gate on 2026-07-20:

| Engine | p50 | p95 | Maximum | Result |
| --- | ---: | ---: | ---: | --- |
| Chromium | 15.1 ms | 15.6 ms | 15.6 ms | Pass |
| Firefox | 29.0 ms | 38.0 ms | 38.0 ms | Pass |
| WebKit | 17.0 ms | 41.0 ms | 41.0 ms | Pass |

All engines met the initial target, so an incremental reducer is not justified at this scale.

If any engine exceeds the target on a reproducible runner, profile the full snapshot walk before
introducing an incremental reducer. Do not weaken snapshot-first revision coherence to meet the
budget.

## Rust/TypeScript DTO Contracts

The room-state, runtime-inspection, media, device, statistics, messaging, participant-RPC, and
data-stream DTOs crossing `serde-wasm-bindgen` are declared in Rust and generated into
`crates/livekit-browser/js/src/generated/adapter-contracts.ts`. After changing one of those Rust
DTOs or its Serde representation, regenerate and verify the committed TypeScript contract:

```sh
cargo test -p livekit-browser contracts::adapter_contracts_are_fresh -- --exact
npm run check --prefix crates/livekit-browser/js
```

`dagger generate livekit-browser:contracts` runs the same test in a container and applies the
rewritten contract as a changeset.

The DTOs derive `ts_rs::TS` only under `cfg(test)`, which keeps the generator out of the shipped
wasm dependency graph, so a test is the only place the contract can be rendered. When the committed
file is stale, `adapter_contracts_are_fresh` rewrites it and then fails: run the tests again to
confirm the update, and commit the result. A plain `cargo test` covers this, so the command above
matters only when you want the check alone. CI fails on its single run rather than self-healing.

The npm `check` remains Node-only so adapter consumers do not need Cargo; the Rust freshness check
runs explicitly in the Rust stage of `Dockerfile.web`.

### Intentional Exclusions

Every named Serde object DTO belongs in the generated contract registry. These generic or
positional boundaries remain intentionally handwritten:

| Boundary | Rust shape | Reason |
| --- | --- | --- |
| Local Participant attribute updates | `BTreeMap<String, String>` | Serialized as a plain JavaScript object; no named DTO exists. |
| Structured SDK log context | `serde_json::Value` | Recursively normalized JSON data rather than a fixed object schema. |
| Positional data-packet callbacks | `Uint8Array`, optional Participant Identity, and delivery flag | Preserves the existing callback ABI; changing to an object requires an explicit ABI decision. |

The Rust `adapter_contract_exclusions_are_documented` test keeps this exclusion registry present.

## Atomic SDK Upgrade

Upgrade `livekit-client` as one commit; never merge an intermediate mixed-version state.

1. Read the upstream release notes and compare TypeScript declarations for Room Events, options,
   errors, statistics, worker assets, media behavior, and browser support.
2. Run `npm install --save-exact livekit-client@<version>` in `demo` to update the
   application dependency and lockfile. Run the same command in `crates/livekit-browser/js` to update
   the adapter's type dependency and lockfile. Confirm `Dockerfile.web` still copies the intended UMD
   file.
3. Change `LIVEKIT_CLIENT_VERSION` in `crates/livekit-browser/src/lib.rs` to the same exact version.
4. Update the TypeScript sources under `crates/livekit-browser/js/src` and normalized Rust types
   together for every changed SDK contract. Run
   `cargo test -p livekit-browser contracts::adapter_contracts_are_fresh -- --exact` until it
   passes, and commit the updated
   `crates/livekit-browser/js/src/generated/adapter-contracts.ts`. Run
   `npm run build --prefix crates/livekit-browser/js` (or
   `dagger generate livekit-browser:adapter`, which runs the same bundle in a container) and
   commit the generated
   `crates/livekit-browser/js/livekit-browser.js`. Update browser capability and unsupported-feature
   documentation.
5. Update adapter, runtime-failure, browser workflow, mobile capability, cleanup, and snapshot profile
   contracts for the new behavior.
6. Run `dagger check` from the repository root for the adapter and Rust checks, and from `demo/` for
   the compatibility gate. The gate always uses a pinned local LiveKit service, never Cloud, and
   covers the Chromium, mobile-sized, target-scale, and two-participant workflows. Run the Firefox
   and WebKit projects against the Docker stack as described in [Demo Application](demo.md); a Cloud
   run stays an explicit, non-deterministic extra.
7. Manually complete one two-participant physical-device acceptance pass: join, publish camera and
   microphone, subscribe and attach remote media, exchange each messaging family used by the
   application, record and revoke a Local Clip, leave, and confirm hardware/listener cleanup.

No SDK version range is accepted until multiple versions are deliberately tested and a concrete
compatibility policy replaces exact coupling.
