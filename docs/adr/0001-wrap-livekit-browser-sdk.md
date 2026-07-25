---
status: accepted
---

# Wrap the official LiveKit browser SDK

Browser realtime media will use an opinionated Rust/WASM interface backed by LiveKit's official `livekit-client` JavaScript SDK, rather than porting LiveKit or mirroring its JavaScript interface. The application build owns an exact `livekit-client` UMD dependency; `livekit-browser` owns a private browser orchestration adapter, verifies the loaded version at initialization, and keeps JavaScript details private. The adapter normalizes SDK state and owns browser-event state or versioned wrapper protocols when the pinned SDK cannot provide a required lifecycle or delivery contract directly.

## Considered Options

- Port LiveKit onto `web-sys`: rejected for now because browser WebRTC is only one missing layer; signaling, runtime affinity, media integration, and native-only assumptions also require a multi-crate architectural port. See [the WASM research](../livekit-rust-wasm-research.md).
- Mirror the JavaScript SDK: rejected because a one-for-one Rust surface would be shallow, inherit upstream churn, and obscure browser-specific invariants behind nominal Rust types.
- Build a Dioxus-only wrapper: rejected because Room Sessions, participants, publications, tracks, and browser media attachment do not depend on Dioxus.

## Consequences

The initial implementation requires `livekit-client` 2.20.1 to be loaded before WASM and remains browser-thread-only. Audio and video use Track Publications, while Data Tracks are separate domain objects because the pinned SDK does not expose them as publications. Application-pull Data Stream backpressure uses a private versioned acknowledgement protocol because SDK write completion does not report destination application consumption; both endpoints must therefore use the same wrapper version. Subscriber lag is represented by typed items in continuing bounded streams rather than operation errors. Browser `MediaRecorder` timing, events, and Blob ownership also remain in the private adapter while Rust owns the public Local Clip Recording lifecycle. A future pure Rust implementation may replace private internals, but there is no speculative backend trait. JavaScript escape handles are available only through an explicitly unstable `raw-js` feature so normal callers do not couple themselves to the chosen implementation.
