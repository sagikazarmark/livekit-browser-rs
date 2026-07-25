# LiveKit Rust SDK support for browser WebAssembly

**Research date:** 2026-07-18
**Upstream snapshot:** [`livekit/rust-sdks` `62359c3`](https://github.com/livekit/rust-sdks/commit/62359c3598913d3c3ac07518c493c8df0bc1fcef), after the latest `livekit` release tag, [`v0.7.53` at `da3ee00`](https://github.com/livekit/rust-sdks/releases/tag/livekit%2Fv0.7.53). The relevant files are unchanged between that tag and the reviewed `main` commit.

## Bottom line

**No: LiveKit's native Rust `livekit` client does not currently support `wasm32-unknown-unknown` or browser targets.** Its manifests contain the beginning of an intended browser split, but the wasm WebRTC implementation is an empty `unimplemented!()` module, the signal transport is built around native TCP WebSockets, the runtime and callback contracts assume `Send`, and parts of the client still reference native-only APIs without wasm guards. Official support lists and CI cover Windows, macOS, Linux, iOS, and Android, not wasm or browsers.

This is **not realistically fixed by a small downstream `cfg`/dependency patch**. The existing abstraction means the LiveKit room/control logic need not be rewritten, but a useful port still needs a browser WebRTC backend, browser signaling, a wasm-local execution model, browser media capture/rendering APIs, native-only feature separation, and browser CI. That is an architectural port across several crates.

For a browser application whose Rust code already compiles to wasm, the practical short path is to use LiveKit's official **JavaScript browser SDK** (`livekit-client`) behind a narrow JS/`wasm-bindgen` bridge. That is a supported browser SDK called from wasm; it is not wasm support in the Rust `livekit` crate.

## Do not conflate these components

| Component | Purpose | Browser/wasm conclusion |
|---|---|---|
| [`livekit`](https://github.com/livekit/rust-sdks/tree/62359c3598913d3c3ac07518c493c8df0bc1fcef/livekit) | Native realtime room client: signaling, peer connections, tracks, and data | **Unsupported.** This is the crate that would have to be ported. |
| [`libwebrtc`](https://github.com/livekit/rust-sdks/tree/62359c3598913d3c3ac07518c493c8df0bc1fcef/libwebrtc) | LiveKit's safe WebRTC abstraction used by `livekit` | Selects a wasm implementation in principle, but that implementation is absent. |
| [`webrtc-sys`](https://github.com/livekit/rust-sdks/tree/62359c3598913d3c3ac07518c493c8df0bc1fcef/webrtc-sys) | Unsafe C++ bridge to a native static build of Chromium's libwebrtc | Native-only, but deliberately excluded from `libwebrtc` on wasm. It is not the immediate blocker once the target split is honored. |
| [`livekit-api`](https://github.com/livekit/rust-sdks/tree/62359c3598913d3c3ac07518c493c8df0bc1fcef/livekit-api) | Server APIs, webhook handling, and access-token signing | **Not a realtime client.** Token-only code compiling on a target would not provide room/WebRTC support, and token signing belongs on a trusted backend. |
| [`livekit-client`](https://github.com/livekit/client-sdk-js) through `wasm-bindgen` | The official JS/TypeScript browser client, invoked through JS interop | **Practical browser option.** It uses the JS SDK's implementation rather than the Rust SDK's room/client implementation. |

The Rust repository itself labels `livekit-api` as “Server APIs and auth token generation” and `livekit` as the “LiveKit real-time SDK” ([README lines 43-48](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/README.md#L43-L48)); the crate manifests likewise describe [`livekit` as the Rust Client SDK](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/livekit/Cargo.toml#L1-L8) and [`livekit-api` as the Rust Server SDK](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/livekit-api/Cargo.toml#L1-L8).

## Concrete target evidence

### 1. Official support and CI omit browsers and wasm

The Rust SDK's support checklist names only Windows, macOS, Linux, iOS, and Android ([README lines 36-42](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/README.md#L36-L42)). Its primary build matrix has those same desktop/mobile target families and no `wasm32` entry ([`builds.yml` lines 43-73](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/.github/workflows/builds.yml#L43-L73)); tests run only Windows, macOS, and Linux host targets ([`tests.yml` lines 44-57](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/.github/workflows/tests.yml#L44-L57)). The native libwebrtc artifact workflow builds Windows, macOS, Linux, Android, and iOS, again with no wasm artifact ([`webrtc-builds.yml` lines 22-84](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/.github/workflows/webrtc-builds.yml#L22-L84)).

LiveKit's official platform quickstarts classify React and Unity WebGL as web SDKs and do not list Rust as a web platform ([platform quickstarts](https://docs.livekit.io/transport/sdk-platforms/)). The connection documentation separately describes Rust among SDKs for realtime **backend** applications rather than among frontend browser SDKs ([connection guide](https://docs.livekit.io/home/client/connect/)). Absence from a list alone is not dispositive, but it agrees with the source and CI.

### 2. `webrtc-sys` is native-only, but `libwebrtc` already tries to avoid it on wasm

`webrtc-sys` is a CXX bridge that compiles many Rust/C++ bridge files, downloads a prebuilt static `libwebrtc`, and links `static=webrtc` ([build script lines 31-137](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/webrtc-sys/build.rs#L31-L137)). Its platform switch implements Windows, Linux, macOS, iOS, and Android and panics for every other target ([lines 137-380](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/webrtc-sys/build.rs#L137-L380)). Its download helper constructs a target-specific native release name and URL ([`webrtc-sys/build/src/lib.rs` lines 34-68 and 87-107](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/webrtc-sys/build/src/lib.rs#L34-L107)); CI publishes no browser build.

However, `libwebrtc` makes `webrtc-sys`, CXX, Tokio, and other native dependencies conditional on `not(target_arch = "wasm32")`, while selecting `wasm-bindgen`, `js-sys`, and `web-sys` on wasm ([`libwebrtc/Cargo.toml` lines 31-58](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/libwebrtc/Cargo.toml#L31-L58)). Its root module similarly selects `web/mod.rs` on wasm and `native/mod.rs` elsewhere ([`libwebrtc/src/lib.rs` lines 17-19](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/libwebrtc/src/lib.rs#L17-L19)). Therefore:

- Compiling `webrtc-sys` itself for wasm is unsupported.
- The intended `livekit` browser architecture does **not** require porting the C++ library; it requires implementing the browser's built-in WebRTC APIs behind `libwebrtc`'s existing abstraction.
- Merely adding a wasm artifact or another arm to `webrtc-sys/build.rs` would follow the wrong seam and would not address signaling, browser media, or execution constraints.

### 3. The selected wasm WebRTC backend is not implemented

The entire selected wasm implementation is currently [`unimplemented!();`](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/libwebrtc/src/web/mod.rs#L15). Consequently it defines none of the `imp::peer_connection`, `imp::peer_connection_factory`, `imp::data_channel`, track, sender/receiver/transceiver, session-description, candidate, stats, source, or stream modules consumed by the shared wrappers; representative dependencies can be seen in [`peer_connection.rs`](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/libwebrtc/src/peer_connection.rs#L17-L30) and [`peer_connection_factory.rs`](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/libwebrtc/src/peer_connection_factory.rs#L17-L20).

Two files under `libwebrtc/src/web` can give a misleading impression of partial support. They are not declared by `web/mod.rs`, so they are dormant. The data-channel file targets obsolete shared names such as `DataChannelTrait` and `DataState` ([web file lines 15-43](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/libwebrtc/src/web/data_channel.rs#L15-L43)), whereas the current shared API defines a concrete wrapper and `DataChannelState` ([current API lines 20-77](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/libwebrtc/src/data_channel.rs#L20-L77)). The media-stream experiment contains dozens of `unimplemented!()` methods ([example lines 63-142](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/libwebrtc/src/web/media_stream.rs#L63-L142)). These are old design experiments, not a nearly complete backend awaiting module declarations.

The `web-sys` feature list is likewise only an initial subset: it includes peer connection, SDP, ICE, and data-channel types but omits the browser media-device, `MediaStreamTrack`, RTP sender/receiver/transceiver, and HTML media-element surface needed for current client parity ([manifest lines 40-58](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/libwebrtc/Cargo.toml#L40-L58)). Adding those features is necessary but nowhere near sufficient without implementations.

### 4. The realtime client has additional native assumptions

The `livekit` crate unconditionally depends on `libwebrtc` and `livekit-api`; its default runtime is Tokio, and each runtime feature selects one of `livekit-api`'s native signal-client implementations ([`livekit/Cargo.toml` lines 10-57](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/livekit/Cargo.toml#L10-L57)). There is no wasm/browser feature.

Specific blockers beyond the empty WebRTC backend include:

- **Signaling transport:** `livekit-api` aliases its WebSocket to a Tungstenite stream over `livekit_runtime::TcpStream`, then connects with Tokio- or async-std-based Tungstenite ([`signal_stream.rs` lines 15-59 and 89-108](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/livekit-api/src/signal_client/signal_stream.rs#L15-L108)). A browser has a WebSocket API, not a raw TCP stream. The official JS client has a separate browser path: it creates a browser `WebSocket` ([`WebSocketStream.ts` lines 43-54](https://github.com/livekit/client-sdk-js/blob/5db17af4922212851238b63ff2266df7e4095b6b/src/api/WebSocketStream.ts#L43-L54)) and places the access token/join request in URL parameters ([`SignalClient.ts` lines 1175-1292](https://github.com/livekit/client-sdk-js/blob/5db17af4922212851238b63ff2266df7e4095b6b/src/api/SignalClient.ts#L1175-L1292)). A Rust browser port needs that transport/authentication behavior, not just a wasm-compatible TLS dependency.
- **Execution and thread-safety model:** the Tokio runtime adapter requires spawned futures and outputs to be `Send` ([`livekit-runtime/src/tokio.rs` lines 29-40](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/livekit-runtime/src/tokio.rs#L29-L40)), and the dispatcher backend also requires `Send` ([`dispatcher.rs` lines 33-62](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/livekit-runtime/src/dispatcher.rs#L33-L62)). WebRTC/data callbacks are declared `Send + Sync` ([peer callbacks lines 98-106](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/libwebrtc/src/peer_connection.rs#L98-L106)), while the dormant browser experiment uses `Rc<RefCell<...>>` for JS callback state ([web data channel lines 15-41](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/libwebrtc/src/web/data_channel.rs#L15-L41)). A browser backend therefore needs an explicit local-task/thread-affinity boundary or target-specific callback contracts.
- **Browser media API:** local audio/video creation only has native source arms; on wasm those arms disappear and the fallback panics as an unsupported source ([audio lines 53-74](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/livekit/src/room/track/local_audio_track.rs#L53-L74), [video lines 158-170](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/livekit/src/room/track/local_video_track.rs#L158-L170)). The browser needs capture through media devices and playback/attachment through DOM media tracks/elements rather than the native frame-source/stream model.
- **Incomplete target gating inside `livekit`:** current production modules still import native-only packet-trailer/frame-cryptor/UUID APIs without enclosing the whole use or feature in a wasm guard; examples include [`local_video_track.rs` lines 22-38](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/livekit/src/room/track/local_video_track.rs#L22-L38), [`e2ee/key_provider.rs` lines 15-23](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/livekit/src/room/e2ee/key_provider.rs#L15-L23), and [`rpc/client.rs` line 22](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/livekit/src/room/rpc/client.rs#L22). These would be follow-on compile errors even after replacing `web/mod.rs`.

## Upstream issue and pull-request history

Search covered open and closed issues and PRs in `livekit/rust-sdks` for `wasm`, `wasm32`, `WebAssembly`, `browser`, `web target`, and `wasm-bindgen`, plus organization-wide GitHub searches for Rust with those terms. Dependency-update matches and unrelated wasm products were excluded.

- [**PR #42, “feat: video publishing”**](https://github.com/livekit/rust-sdks/pull/42): opened 2023-02-18; **merged 2023-03-18**. Its own description says “Prepare webrtc abstraction ( for future wasm support )”, not that wasm support was delivered. The target-selected web module added by its merge commit was already `unimplemented!()` ([historical source](https://github.com/livekit/rust-sdks/blob/cad6d36201e11c2f2de0f883a3693dfdf82917f7/livekit-webrtc/src/web/mod.rs#L15)). This is the origin of the current scaffold.
- [**Issue #800, “State of building for WASM”**](https://github.com/livekit/rust-sdks/issues/800): opened 2025-12-01; **closed 2026-06-02 as `not_planned`** by `linear-code[bot]`; it has no comments or technical response. The automated closure should not be read as an engineering assessment, but it supplied no roadmap either.
- [**Issue #1145, “Run Livekit/WebRTC on Rust WASM Client”**](https://github.com/livekit/rust-sdks/issues/1145): opened 2026-06-08; **open as of 2026-07-18**; no comments or maintainer response.
- [**Issue #1146, “Can you use libwebrtc by itself?”**](https://github.com/livekit/rust-sdks/issues/1146): opened 2026-06-08; **open as of 2026-07-18**. It explicitly proposes writing bindings to the JavaScript WebRTC API if #1145 is not addressed, but has no LiveKit maintainer answer. It is adjacent design discussion, not an implementation PR.

No open or closed dedicated implementation PR was found beyond #42's future-support scaffold. In particular, there is **no active upstream PR implementing the wasm `libwebrtc` backend, browser signaling, or wasm CI** as of the research date.

## Minimum viable architectural work

To preserve the Rust `livekit` control/room implementation rather than delegating to JS, the minimum credible browser port is:

1. **Complete `libwebrtc::imp` for browsers.** Wrap `web_sys::RtcPeerConnection` and implement the shared peer-connection/factory, SDP, ICE, data-channel, media-track, RTP sender/receiver/transceiver/parameters, and stats contracts. Decide which native frame/source APIs are unavailable, browser-specific, or redesigned rather than filling them with panics.
2. **Add a browser signal transport to `livekit-api`.** Use browser WebSocket events and binary frames, implement LiveKit's browser-compatible join/auth URL behavior, and retain reconnect/ping/close semantics without `TcpStream`, Tungstenite, native proxy, or native TLS assumptions.
3. **Add a wasm runtime backend.** Use browser timers and local task spawning and isolate JS values on their owning thread. Refactor the unconditional `Send`/`Sync` bounds where the browser implementation cannot satisfy the native contract.
4. **Define browser media integration.** Add camera/microphone/display capture from browser `MediaStreamTrack`s and a supported way to expose/attach subscribed tracks for playback. This is a platform API decision, not only an internal binding task.
5. **Separate native-only functionality.** Gate or redesign packet trailers, native E2EE/frame cryptors, native frame streams/sources, dynamic loading, and other imports so unsupported features fail at feature selection/API use rather than preventing the crate from compiling.
6. **Add target and browser CI.** At minimum, `cargo check` for `wasm32-unknown-unknown`, `wasm-bindgen-test` or equivalent headless-browser tests for signaling/data/media, and an end-to-end LiveKit server test. A compile-only leg would not validate browser WebRTC behavior.

A data-only or subscribe-only proof of concept could defer some capture/frame APIs, but it still needs items 1-3 and enough RTP/data/event integration to establish the LiveKit session. That remains substantially larger than a downstream manifest patch.

## Practical downstream options

### Patch the native Rust client

**Verdict: not a small or low-risk patch.** The existing `imp` seam is useful and makes an upstream-quality port feasible, but the dormant browser code predates the current API and the remaining blockers span `libwebrtc`, `livekit-api`, `livekit-runtime`, and `livekit`. A downstream fork would also own browser compatibility and ongoing parity with LiveKit protocol/client changes until upstream accepts and maintains it.

### Call `client-sdk-js` from wasm

**Verdict: realistic and the recommended short path when a browser deliverable is needed.** The official JS SDK publishes ESM and UMD browser bundles ([`package.json` lines 1-29](https://github.com/livekit/client-sdk-js/blob/5db17af4922212851238b63ff2266df7e4095b6b/package.json#L1-L29)), documents browser room connection, camera/microphone publishing, DOM track attachment, and a browser support matrix ([README usage](https://github.com/livekit/client-sdk-js/blob/5db17af4922212851238b63ff2266df7e4095b6b/README.md#L68-L159), [browser support](https://github.com/livekit/client-sdk-js/blob/5db17af4922212851238b63ff2266df7e4095b6b/README.md#L485-L499)). `wasm-bindgen` officially supports importing JS modules/snippets, and `raw_module` can leave module resolution to an end application's bundler ([wasm-bindgen JS imports](https://wasm-bindgen.github.io/wasm-bindgen/reference/js-snippets.html), [`raw_module`](https://wasm-bindgen.github.io/wasm-bindgen/reference/attributes/on-js-imports/raw_module.html)).

Keep that bridge narrow: expose room lifecycle, selected events, data operations, and opaque JS track/element handles rather than attempting to mirror the entire TypeScript API in Rust. Promise-to-`Future` conversion, closure lifetime management, event unsubscription, and JS bundling still require care, but LiveKit's browser signaling/WebRTC/media/reconnect logic remains in the SDK that officially implements it.

### Use `livekit-api` in browser wasm

**Verdict: not a substitute for either client.** Its default features are server services, token signing, and webhooks, while its optional signal clients are native-runtime transports ([feature table](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/livekit-api/Cargo.toml#L10-L51)). There is some isolated wasm awareness elsewhere in the workspace, such as the `device-info` browser implementation ([manifest](https://github.com/livekit/rust-sdks/blob/62359c3598913d3c3ac07518c493c8df0bc1fcef/device-info/Cargo.toml#L28-L30)), but there is no wasm CI or support claim for `livekit-api`. Whether a minimal no-network feature set happens to cross-compile is orthogonal: it cannot join a room.

Token generation is also normally a backend concern because LiveKit tokens are signed with the API secret ([official token reference](https://docs.livekit.io/frontends/authentication/tokens/)); LiveKit's manual-generation guide explicitly places programmatic token creation on the backend ([custom token generation](https://docs.livekit.io/frontends/build/authentication/custom/)). A browser should receive a participant token from a trusted endpoint, not embed the API secret in wasm.

## Feasibility verdict

| Goal | Verdict |
|---|---|
| Build the official Rust `livekit` client for a browser today | **No; unsupported and source-incomplete.** |
| Fix it downstream with a few target `cfg`s/dependency swaps | **No.** Those only expose the missing backend, transport, runtime, and media work. |
| Port the Rust client while retaining its room/control logic | **Technically feasible, but an architectural multi-crate project.** `webrtc-sys` need not be ported if browser WebRTC is used behind `libwebrtc::imp`. |
| Ship a Rust/wasm browser app using LiveKit soon | **Yes, by bridging to the official `livekit-client` JS SDK.** This is JS SDK usage, not native Rust SDK wasm support. |
| Use `livekit-api` as the browser RTC client | **No.** It is the server/token crate and provides no media client. |
