# Dioxus camera and audio library research

**Research date:** 2026-07-19

**Scope:** Upstream crates, documentation, source, manifests, examples, CI, commits, releases, and issues only. This report does not assess this repository's implementation.

**Primary-source snapshots:** [`dioxus-cameras` 0.3.2 / `cameras` commit `64ade44`](https://github.com/matthewjberger/cameras/tree/64ade44809ba6f215c981dfbe8be7d16a6bae08c), the exact published [`dioxus-chimeras` 0.2.0 source](https://docs.rs/crate/dioxus-chimeras/0.2.0/source/), and [`dioxus-audio` 0.1.1 package commit `a3bbab0`](https://github.com/sagikazarmark/dioxus-audio/tree/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905). Activity was checked through camera commit [`64ade44`](https://github.com/matthewjberger/cameras/commit/64ade44809ba6f215c981dfbe8be7d16a6bae08c) and audio `main` commit [`3ba72b8`](https://github.com/sagikazarmark/dioxus-audio/commit/3ba72b804220655b42df06f781173706e2d638b2).

## Bottom line

| Library | What it is | Current disposition |
|---|---|---|
| [`dioxus-cameras`](https://crates.io/crates/dioxus-cameras) | Dioxus Desktop glue around the native `cameras` crate: camera/device hooks, frame pump, loopback preview server, and WebGL2 canvas renderer | The maintained camera option of these three, but young and specifically desktop/native. Latest release is 0.3.2, requires Rust 1.85 and Dioxus 0.7, and has no browser-wasm or mobile backend. |
| [`dioxus-chimeras`](https://crates.io/crates/dioxus-chimeras) | The same camera integration under its former `chimeras` name | Superseded, not complementary. Its only release and every release of its `chimeras` dependency are yanked; the repository URL now resolves to `cameras`. Do not begin new work on it. |
| [`dioxus-audio`](https://crates.io/crates/dioxus-audio) | Browser audio recording, playback, device selection, analysis, and Dioxus UI components | Orthogonal to camera video, but extremely new. Release 0.1.1 requires Rust 1.92 and Dioxus 0.7. Recording/playback work only in browser `wasm32-unknown-unknown`; non-wasm targets expose analysis/UI but return unsupported from controllers. |

`dioxus-cameras` and `dioxus-chimeras` overlap almost completely because the latter is the former name and implementation lineage. `dioxus-audio` does not duplicate camera functionality. It is conceptually composable with camera video, and all maintained crates accept Dioxus 0.7, but the complete implementations do **not** currently share a target: camera capture is native Dioxus Desktop, whereas audio capture/playback is selected only for browser wasm. In a single native desktop build, only `dioxus-audio`'s pure analysis and visual components remain usable; its controllers report `UnsupportedPlatform` ([audio platform contract](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/README.md#L118-L129), [non-wasm recorder](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/recorder.rs#L355-L419), [non-wasm player](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/playback.rs#L170-L214)).

## Compatibility matrix

| Library/version | Dioxus requirement | Rust / edition | Functional targets | Feature defaults |
|---|---|---|---|---|
| `dioxus-cameras` 0.3.2 | `dioxus = "^0.7"` with `desktop` | Rust 1.85, edition 2024 | Native Dioxus Desktop on macOS, Windows, and Linux; WebGL2 is required inside the desktop webview | No default features; optional `rtsp`; `discover` implies `rtsp` |
| `dioxus-chimeras` 0.2.0 | `dioxus = "^0.7.3"` with `desktop` | Rust 1.85, edition 2024 | Intended native desktop on macOS, Windows, and Linux; default RTSP is only implemented on macOS/Windows and breaks the docs.rs Linux all-feature build | `rtsp` on by default |
| `dioxus-audio` 0.1.1 | `dioxus = "^0.7"`, defaults off, `lib` only | Rust 1.92, edition 2024 | Recording, playback, input devices, and live Web Audio analysis on browser `wasm32-unknown-unknown`; pure analysis and renderable neutral UI on other targets | No crate features |

The version and feature requirements come from the published manifests: [`dioxus-cameras`](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/Cargo.toml#L32-L41), [`dioxus-chimeras`](https://docs.rs/crate/dioxus-chimeras/0.2.0/source/Cargo.toml.orig), and [`dioxus-audio`](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/Cargo.toml#L1-L50). The camera core documents the OS backend split as AVFoundation on macOS, Media Foundation on Windows, and V4L2 on Linux, with RTSP unavailable on Linux ([platform table](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/cameras/README.md#L56-L63)). The camera CI checks all three desktop operating systems and has no wasm or mobile job ([CI matrix](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/.github/workflows/ci.yml#L12-L69)).

## `dioxus-cameras`

### Purpose and architecture

`dioxus-cameras` is intentionally a thin Dioxus-specific layer over `cameras`. The core crate owns device enumeration, camera opening, source types, frame capture, conversion, controls, RTSP, discovery, and the background pump. The integration crate contributes:

- Dioxus hooks for one stream, device enumeration, stream-id management, and optional RTSP discovery.
- A `Registry` of latest frames and a loopback HTTP `PreviewServer`.
- `StreamPreview`, which renders a canvas, and `PreviewScript`, which injects a polling WebGL2 renderer.
- A `pub use cameras` re-export so consumers can use compatible core types through one dependency.

This division is explicit in the [crate-level documentation and exports](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/src/lib.rs#L1-L102) and the [workspace README](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/README.md#L15-L35). The preview server binds an ephemeral `127.0.0.1` port, serves a 24-byte header plus raw frame planes, and shuts down when its final handle is dropped ([server implementation](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/src/server.rs#L20-L91), [listener](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/src/server.rs#L115-L179)). JavaScript fetches that endpoint roughly every 28 ms and uploads NV12, BGRA, or RGBA data to WebGL2 textures ([renderer](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/assets/preview.js#L43-L49), [fetch/upload loop](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/assets/preview.js#L112-L244), [animation loop](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/assets/preview.js#L297-L319)).

### Public API

The generated documentation reports 100% API documentation and lists the complete root surface ([docs.rs API](https://docs.rs/dioxus-cameras/0.3.2/dioxus_cameras/)):

| Surface | Public items and contract |
|---|---|
| Core re-export | `pub use cameras`, giving access to `CameraSource`, `Device`, `Frame`, `StreamConfig`, `PixelFormat`, core errors, controls, conversion, pump APIs, and enabled optional modules. |
| Setup/server | `start_preview_server() -> io::Result<PreviewServer>`; `register_with(&PreviewServer, LaunchBuilder) -> LaunchBuilder`; `PreviewServer { pub port }`. |
| Frame registry | `Registry`, `LatestFrame`, `get_or_create_sink`, `publish_frame`, and `remove_sink` for custom-pump use. |
| Stream hook | `use_camera_stream(id, Signal<Option<CameraSource>>, StreamConfig) -> UseCameraStream`. Its public fields are `status: Signal<StreamStatus>`, `active: Signal<bool>`, and `capture_frame: Callback<(), Option<Frame>>`. `StreamStatus` is `Idle`, `Connecting { label }`, `Streaming { label }`, or `Failed { error }` ([definitions](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/src/camera_stream.rs#L21-L114), [hook signature](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/src/camera_stream.rs#L116-L162)). |
| Device hook | `use_devices() -> UseDevices`, exposing public `devices`, `ready`, and `refresh` fields ([source](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/src/devices.rs#L10-L76)). |
| Stream-list hook | `use_streams() -> UseStreams`, exposing public `ids`, `add`, and `remove` fields; ids are monotonically allocated and not reused within a hook instance ([source](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/src/streams.rs#L3-L62)). |
| Preview UI | `StreamPreview { id }`, `PreviewScript {}`, and raw `PREVIEW_JS` ([components](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/src/component.rs#L13-L61)). |
| Conditional discovery | On macOS/Windows with `discover`: `use_discovery() -> UseDiscovery`, whose signals/callbacks cover discovered cameras, unmatched hosts, scan progress, running/error state, start, cancel, and clear ([feature/export gate](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/src/lib.rs#L75-L90), [documented handle](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/README.md#L131-L146)). |

### Dependencies and native footprint

The integration's direct normal dependencies are `bytes 1`, `cameras ^0.3`, `dioxus ^0.7` with `desktop`, and `futures-timer 3` ([crates.io dependency metadata](https://crates.io/api/v1/crates/dioxus-cameras/0.3.2/dependencies)). Material transitive and native dependencies come from two branches:

- **Camera backend:** Common Rust dependencies include `crossbeam-channel`, `thiserror`, and `zune-jpeg`. Linux selects `v4l`; Windows selects the `windows` crate with Media Foundation and COM APIs; macOS selects `objc2` bindings for AVFoundation, CoreMedia/CoreVideo, and VideoToolbox plus `dispatch2`/`block2` ([core manifest](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/cameras/Cargo.toml#L39-L73)).
- **Optional RTSP/discovery:** `rtsp` pulls `retina`, Tokio, `url`, and `futures-util`; `discover` also pulls `ipnet` and Tokio I/O. RTSP decoding uses VideoToolbox on macOS and Media Foundation on Windows, not FFmpeg; Linux RTSP is unsupported ([features](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/cameras/Cargo.toml#L32-L49), [platform behavior](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/cameras/README.md#L190-L205)).
- **Desktop webview:** Dioxus Desktop brings the native webview/window stack. The project's Linux CI installs `libv4l-dev`, Clang/libclang, GTK 3, libsoup 3, JavaScriptCoreGTK 4.1, WebKitGTK 4.1, and `libxdo-dev`, documenting the practical Linux build surface ([CI packages](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/.github/workflows/ci.yml#L35-L48)). macOS and Windows use their platform camera frameworks and OS webview stack.

The crate has no wasm-specific dependency section. Its use of `std::net::TcpListener`, OS camera backends, threads, and Dioxus's `desktop` feature is substantive architecture, not just missing target metadata.

### Examples

The [quick start](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/README.md#L20-L65) demonstrates starting and registering the preview server, creating a camera source signal and stream configuration, rendering `StreamPreview`, toggling pump activity, capturing one frame, and injecting `PreviewScript`. The runnable [`apps/dioxus-demo`](https://github.com/matthewjberger/cameras/tree/64ade44809ba6f215c981dfbe8be7d16a6bae08c/apps/dioxus-demo) is much broader: multi-stream USB/RTSP selection, snapshots, controls, autofocus/sharpness, device refresh, and conditional discovery. The core `cameras` crate also ships snapshot, pump, monitor, sharpness, autofocus, and discovery examples ([example manifest entries](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/cameras/Cargo.toml#L74-L96)).

### Known limitations and behavioral traps

- **Desktop only:** The documented product is "Dioxus desktop," and the implementation requires native camera APIs, a local TCP server, and WebGL2 in the embedded webview. There is no browser-wasm or mobile backend.
- **WebGL2 failure is silent:** `canvas.getContext("webgl2")` returning false simply exits setup, so a preview can remain blank without a surfaced Rust error ([renderer setup](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/assets/preview.js#L43-L49)). `PreviewScript` must appear exactly once or canvases remain blank ([component contract](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/src/component.rs#L47-L61)).
- **Required launch wiring can panic:** `StreamPreview` and `use_camera_stream` panic at render time if `register_with` did not inject their context ([preview](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/src/component.rs#L24-L36), [hook](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/src/camera_stream.rs#L154-L166)).
- **Snapshots block their caller:** `capture_frame` synchronously waits about one frame interval and, when paused, up to another 20 ms; calling repeatedly on the UI thread can stall it ([contract](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/src/camera_stream.rs#L96-L113)).
- **Reconnects are not cancellable:** Every signal write reconnects, even to an equal source. Rapid changes start one thread per request; stale results are discarded, but old `open_source` calls consume resources until completion ([semantics](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/src/camera_stream.rs#L134-L152)).
- **Device enumeration errors are hidden:** `use_devices` swallows `cameras::devices()` errors and retains the previous list; its handle has no error field ([implementation](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/src/devices.rs#L24-L28), [worker](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/src/devices.rs#L57-L67)).
- **Feature/platform asymmetry:** RTSP and discovery work only on macOS/Windows. Discovery is currently Axis-only, anonymous-only, IPv4 RTSP probing; it has no ONVIF, WS-Discovery, or mDNS and limits a scan to 65,536 targets ([documented limits](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/cameras/README.md#L207-L258)).

### Maintenance and maturity

crates.io records ten releases from 0.1.0 on 2026-04-17 through 0.3.2 on 2026-05-16, Rust 1.85, 268 total downloads, and no yanks for the current release line ([crate API](https://crates.io/api/v1/crates/dioxus-cameras)). The repository had 53 commits between 2026-04-15 and 2026-05-16, but no later source activity by the research date ([commit history](https://github.com/matthewjberger/cameras/commits/main/)). It has no GitHub Releases entries ([releases](https://github.com/matthewjberger/cameras/releases)) and crates.io reports no reverse dependencies ([reverse-dependency API](https://crates.io/api/v1/crates/dioxus-cameras/reverse_dependencies?page=1&per_page=100)).

The project did respond to its only filed issue: a Windows NV12 panic reported in [issue #1](https://github.com/matthewjberger/cameras/issues/1) was fixed and closed by [`756de5b`](https://github.com/matthewjberger/cameras/commit/756de5b0b44a7f1f5c9b6fcd45c9b9aa6af6b050), followed by 0.3.2. CI checks/clippies/tests the full workspace with all features on Linux, Windows, and macOS ([workflow](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/.github/workflows/ci.yml#L50-L69)), and docs.rs builds now succeed ([build history](https://docs.rs/crate/dioxus-cameras/0.3.2/builds)).

**Maturity assessment:** functioning, documented, and cross-platform-tested, but still an early pre-1.0, single-owner crate with a short activity history, tiny downstream footprint, no formal release notes/tags, and several pre-1.0 minor-version breaks in its first month.

## `dioxus-chimeras`

### Purpose and relationship to `dioxus-cameras`

The published crate describes exactly the same architecture as `dioxus-cameras`: a Dioxus Desktop bridge around `chimeras::Camera`, with a loopback frame server, registry, camera pump hook, and WebGL2 canvas renderer ([published README](https://docs.rs/crate/dioxus-chimeras/0.2.0/source/README.md)). Its public source exports the same server/registry/hooks/components and `PREVIEW_JS`, except it lacks the later `cameras` re-export and discovery hook ([published root](https://docs.rs/crate/dioxus-chimeras/0.2.0/source/src/lib.rs)).

This is historical succession, not two competing designs. The repository history contains an explicit [`refactor: rename chimeras to cameras across workspace`](https://github.com/matthewjberger/cameras/commit/843f2af0feb5019205c9730d44dbb3929ee279e7) commit on 2026-04-17. Requests to the old `matthewjberger/chimeras` repository now resolve to `matthewjberger/cameras`, and `dioxus-cameras` 0.1.0 was published later that same day ([current crate history](https://crates.io/api/v1/crates/dioxus-cameras)).

### Public API, compatibility, and dependencies

The 0.2.0 public surface is:

- `start_preview_server`, `register_with`, `PreviewServer`.
- `Registry`, `LatestFrame`, `get_or_create_sink`, `publish_frame`, `remove_sink`.
- `use_camera_stream`, `UseCameraStream`, and `StreamStatus`.
- `use_devices`/`UseDevices` and `use_streams`/`UseStreams`.
- `StreamPreview`, `PreviewScript`, and `PREVIEW_JS`.

The [published API source](https://docs.rs/crate/dioxus-chimeras/0.2.0/source/src/lib.rs) and [published quick start](https://docs.rs/crate/dioxus-chimeras/0.2.0) are the usable documentation because rustdoc failed to build. Unlike the successor, its README tells consumers to add a separate matching `chimeras` dependency rather than re-exporting it.

Its manifest requires Rust 1.85, edition 2024, Dioxus `^0.7.3` with `desktop`, and direct dependencies `bytes`, `chimeras ^0.2`, and `futures-timer`; `rtsp` is enabled by default ([manifest](https://docs.rs/crate/dioxus-chimeras/0.2.0/source/Cargo.toml.orig), [crates.io dependencies](https://crates.io/api/v1/crates/dioxus-chimeras/0.2.0/dependencies)). Through `chimeras`, its native and optional dependency surface is materially the same as the camera list above: V4L2/`v4l` on Linux, Media Foundation/`windows` on Windows, AVFoundation/VideoToolbox/`objc2` on macOS, and `retina` plus Tokio for RTSP ([`chimeras` 0.2 dependency metadata](https://crates.io/api/v1/crates/chimeras/0.2.0/dependencies)).

### Examples and limitations

The published README contains the same complete quick start and pause/snapshot example as the successor and points to a multi-stream demo in its parent repository ([docs.rs-rendered README](https://docs.rs/crate/dioxus-chimeras/0.2.0)). Those repository links now redirect into the renamed project, so they describe the successor rather than a maintained 0.2 branch.

All `dioxus-cameras` desktop, WebGL2, required-context, blocking-snapshot, reconnect, hidden-device-error, and RTSP platform limitations apply. In addition:

- **Every release is yanked:** `dioxus-chimeras` has only 0.2.0, published 2026-04-17 and yanked 2026-04-19 ([crate API](https://crates.io/api/v1/crates/dioxus-chimeras)). All four `chimeras` core releases are also yanked ([core crate API](https://crates.io/api/v1/crates/chimeras)).
- **Default Linux build defect:** The crate enables `rtsp` by default, but the core's RTSP module is gated to macOS/Windows while another type still referred to it. docs.rs's Linux all-feature build failed with `cannot find rtsp in crate` ([build log](https://docs.rs/crate/dioxus-chimeras/0.2.0/builds/3149436)). Disabling default features avoids that RTSP path, but it does not change the crate's superseded status.
- **No generated rustdoc:** docs.rs reports that its sole build failed ([build history](https://docs.rs/crate/dioxus-chimeras/0.2.0/builds)).
- **No discovery hook or parent-crate re-export:** those are successor improvements rather than part of 0.2.0 ([old exports](https://docs.rs/crate/dioxus-chimeras/0.2.0/source/src/lib.rs), [current exports](https://github.com/matthewjberger/cameras/blob/64ade44809ba6f215c981dfbe8be7d16a6bae08c/crates/dioxus-cameras/src/lib.rs#L64-L90)).

### Maintenance and maturity

crates.io records one release, 14 total downloads, one owner, no reverse dependencies, and a yank two days after publication ([metadata](https://crates.io/api/v1/crates/dioxus-chimeras), [reverse dependencies](https://crates.io/api/v1/crates/dioxus-chimeras/reverse_dependencies?page=1&per_page=100)). Maintenance continued under the `cameras`/`dioxus-cameras` names rather than this package.

**Maturity assessment:** discontinued migration artifact. Its code informed the maintained successor, but the published package itself is yanked, has a known default-feature target failure, and should not be selected alongside or instead of `dioxus-cameras` for new work.

## `dioxus-audio`

### Purpose and architecture

`dioxus-audio` combines four layers:

- Browser microphone input discovery and permission handling.
- Browser recording through `getUserMedia`, `MediaRecorder`, and Web Audio analysis.
- Browser playback through an `HTMLAudioElement` backed by a Blob object URL.
- Platform-neutral value types, lifecycle state machines, sample/peak helpers, and Dioxus controls/visualizations.

The released README lists recording, playback, input-device, analysis, component, and scoped-style capabilities ([feature list](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/README.md#L8-L19)). The root exports five modules plus `AudioData`, `AudioInputId`, `AudioInputDevice`, `RecordedAudio`, `AudioError`, and `AudioErrorKind` ([crate root](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/lib.rs#L1-L12)).

### Public API

docs.rs exposes the complete item index but reports only 15.95% documentation coverage ([docs.rs](https://docs.rs/dioxus-audio/0.1.1/dioxus_audio/)). Reading the released source gives this effective API:

| Module | Public surface |
|---|---|
| Root values/errors | `AudioData { bytes, mime_type }` with `new`; `AudioInputId` with `new`/`as_str`/`Display`; `AudioInputDevice { id, label, is_default }` with `new`/`display_label`; `RecordedAudio { audio, duration, peaks, input_device }`; `AudioError` and non-exhaustive `AudioErrorKind` covering unsupported, permission, device, configuration, recorder, playback, and backend failures ([types](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/types.rs#L1-L71), [errors](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/error.rs#L1-L54)). |
| `analysis` | `AnalysisDomain::{Waveform,Spectrum}`; `AudioAnalyser::read/level`; `downsample_peaks`; `peak_amplitude`; normalized `WaveformSelection` and builders; generic `trim_interleaved_pcm` ([source](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/analysis.rs#L1-L182)). On non-wasm, `AudioAnalyser` reads empty data and level zero. |
| `devices` | `MicrophonePermission`, `DeviceListStatus`, `AudioInputDevices` getters and `select`/`refresh`/`request_permission`, `use_audio_input_devices`, and pure `reconcile_selected_device` ([source](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/devices.rs#L10-L190)). |
| `recorder` | Non-exhaustive `RecorderStatus`; `RecorderOptions` and validation; `MicrophoneStatus`; `AudioRecorder` signal getters and `start`/`pause`/`resume`/`stop`/`cancel`/`completed`/`take_completed`/`clear_completed`; `use_audio_recorder`; public `RecorderLifecycle`, `RecordingSessionId`, `CompletionDisposition`, and `RecorderCommandError` for state-machine-level use/testing ([lifecycle](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/recorder.rs#L16-L228), [options/controller/hook](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/recorder.rs#L230-L370)). |
| `playback` | Non-exhaustive `PlaybackStatus`; `PlaybackLifecycle`; `PlaybackCommandError`; `clamp_seek`; `AudioPlayerController` status/position/duration/rate and play/pause/seek/skip/rate commands; `use_audio_player` ([source](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/playback.rs#L14-L184)). |
| `components` | `AudioStyles`, `STYLESHEET`, `AudioScrubber`, `AudioInputSelector`, `MicrophoneStatusIndicator`, `AudioPlayer`, `RecorderControls`, `LiveWaveform`, `SpectrumVisualizer`, `LevelMeter`, `WaveformPreview`, and `WaveformRangeSelector` ([exports/base components](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/components.rs#L1-L74)). Component inputs expose controllers/signals and optional labels, processing states, bar counts, dimensions, cancellation callbacks, and selection/seek handlers ([player props](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/components/player.rs#L12-L23), [device props](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/components/devices.rs#L7-L12), [visualizer props](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/components/visualizer.rs#L9-L39), [waveform props](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/components/waveform.rs#L5-L78)). |

The default recorder configuration uses a 256-point FFT, 0.8 smoothing, 100 ms peaks, and preference order `audio/webm;codecs=opus`, `audio/webm`, then `audio/mp4`; the browser selects the first supported type ([defaults and validation](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/recorder.rs#L230-L278), [selection](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/recorder/web.rs#L373-L411)).

### Dependencies and native footprint

The portable dependency layer is deliberately small: `dioxus ^0.7` with only `lib`, plus `dioxus-icons 0.1`. Browser wasm additionally pulls `gloo-timers`, `js-sys`, `wasm-bindgen`, `wasm-bindgen-futures`, and `web-sys`; `dioxus-ssr` and `wasm-bindgen-test` are dev dependencies ([published manifest](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/Cargo.toml#L13-L54), [crates.io dependency metadata](https://crates.io/api/v1/crates/dioxus-audio/0.1.1/dependencies)).

There is no CPAL, ALSA, PulseAudio, PipeWire, CoreAudio, AVFoundation, Media Foundation, or other native audio backend. The platform dependency is the browser itself, specifically Web Audio, Media Capture and Streams, MediaRecorder, Blob/Object URL, and HTML media element APIs represented by the `web-sys` feature list. Consequently it avoids native system-library installation but cannot perform native desktop/mobile audio I/O.

### Examples and test evidence

The released [recording example](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/README.md#L54-L90) composes device selection, recording, microphone status, and a live waveform. The [playback example](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/README.md#L92-L116) lazily supplies `AudioData` to `AudioPlayer`. The repository's [web demo](https://github.com/sagikazarmark/dioxus-audio/tree/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/demo) adds pages for devices, recorder, playback, analysis, waveform/range selection, and visualizers.

This is not just compile-only coverage. Playwright runs the built web demo in Chromium with a fake audio input and tests permission/device discovery, recording pause/resume/stop/cancel, generated playback, retry after autoplay rejection, and playback rates ([device tests](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/demo/e2e/devices.spec.ts), [recorder tests](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/demo/e2e/recorder.spec.ts), [playback tests](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/demo/e2e/playback.spec.ts)). The test/deploy pipeline builds Dioxus Web and runs Playwright Chromium ([Dagger demo pipeline](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/demo/dagger.dang#L71-L127)). This validates Chromium behavior, not a cross-browser support matrix or real hardware diversity.

### Known limitations and behavioral traps

- **Browser wasm only for I/O:** Recording, playback, device enumeration, and live analyzer access require exactly `cfg(all(target_arch = "wasm32", target_os = "unknown"))`. Native Dioxus Desktop and server targets deliberately return unsupported; they do not route through the desktop webview ([manifest target gate](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/Cargo.toml#L17-L50), [README](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/README.md#L118-L129)).
- **Secure context and browser policy:** Microphone capture requires HTTPS or a browser-trusted loopback origin. Permission UX, MIME support, labels, and background capture remain browser/OS policy ([recording notes](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/README.md#L78-L90), [platform notes](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/README.md#L118-L125)).
- **No active-device switching:** The selected input is snapshotted by `start`; changing selection does not switch an active recording ([README](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/README.md#L88-L90), [capture constraints](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/recorder/web.rs#L707-L725)).
- **Memory-buffered media:** Recording accumulates Blob chunks and then materializes one `Vec<u8>`; playback likewise copies a complete `Vec<u8>` into a Blob/object URL ([recording collection](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/recorder/web.rs#L425-L465), [final collection](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/recorder/web.rs#L873-L899), [playback allocation](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/playback/web.rs#L244-L265)). There is no streaming upload, file-backed recording, URL source, or bounded-memory long-recording API.
- **Analysis is lightweight, not an audio-processing engine:** Recorded `peaks` are periodic `u8` maxima. `trim_interleaved_pcm` can trim caller-provided decoded PCM but does not decode, edit, or re-encode the compressed `RecordedAudio`. `WaveformPreview` normalizes each waveform independently, so it is not an absolute loudness comparison ([waveform note](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/README.md#L50-L52), [analysis helpers](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/analysis.rs#L77-L182)).
- **Completed recordings are intentionally backpressured:** A new recording cannot start until the previous completed value is cleared or taken. This protects unsaved data but must be handled by the consumer ([lifecycle](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/recorder.rs#L78-L103), [README guidance](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/README.md#L78-L83)).
- **Browser coverage is narrow:** First-party E2E coverage is Chromium-based. No Safari/Firefox support table or mobile-browser CI is published.
- **Sparse rustdoc:** docs.rs reports 15.95% documented public API despite the broad public surface ([docs.rs](https://docs.rs/dioxus-audio/0.1.1/dioxus_audio/)).

### Maintenance and maturity

The repository was created on 2026-07-16. crates.io shows 0.1.0 that day and 0.1.1 on 2026-07-17, 28 total downloads, no yanks, and no reverse dependencies ([crate API](https://crates.io/api/v1/crates/dioxus-audio), [reverse dependencies](https://crates.io/api/v1/crates/dioxus-audio/reverse_dependencies?page=1&per_page=100)). There is one formal GitHub release, [`v0.1.1`](https://github.com/sagikazarmark/dioxus-audio/releases/tag/v0.1.1). The published crate identifies source commit [`a3bbab0`](https://github.com/sagikazarmark/dioxus-audio/commit/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905); by the research date, `main` had advanced [12 commits to `3ba72b8`](https://github.com/sagikazarmark/dioxus-audio/compare/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905...3ba72b804220655b42df06f781173706e2d638b2), primarily adding and refining styling through merged [PR #5](https://github.com/sagikazarmark/dioxus-audio/pull/5). Those later changes are unreleased.

The only open issue is owner-filed [issue #4](https://github.com/sagikazarmark/dioxus-audio/issues/4), concerning export of failed E2E artifacts rather than an audio behavior defect. CI, browser E2E, SSR component tests, lifecycle tests, and docs.rs all exist, and the docs.rs build succeeds ([build](https://docs.rs/crate/dioxus-audio/0.1.1/builds)).

**Maturity assessment:** actively built and unusually well tested for its age, but still a days-old 0.1 API with one owner, one functional release, no downstream adoption signal, one-browser CI, sparse API documentation, and a very high Rust MSRV of 1.92. It is evidence of a serious prototype/early library, not evidence of a stabilized ecosystem dependency.

## Overlap and composition

| Pair | Overlap | Composition verdict |
|---|---|---|
| `dioxus-cameras` + `dioxus-chimeras` | Near-total: same camera hook/server/registry/WebGL architecture and direct rename lineage | Do not combine. Choose `dioxus-cameras`; `dioxus-chimeras` is yanked and superseded. |
| `dioxus-cameras` + `dioxus-audio` | Domain-adjacent but functionally distinct: video camera capture/preview versus microphone recording, audio playback, analysis, and controls | Dioxus 0.7 requirements align, but full runtime features are target-incompatible. Native desktop can use camera plus audio's pure helpers/visual UI, not audio recording/playback. Browser wasm can use audio I/O, not `dioxus-cameras`. A complete camera+microphone product would need an additional backend or a split architecture. |
| `dioxus-chimeras` + `dioxus-audio` | Same relationship as above, with the obsolete camera package | No reason to choose this pairing over the maintained camera successor, and the same target mismatch remains. |

There is no direct crate dependency between the camera and audio projects, no shared media-stream abstraction, no synchronization API, and no muxing/encoding layer. Composition means placing independent Dioxus components and state in one application; it does not provide synchronized audiovisual capture, WebRTC tracks, container muxing, echo cancellation configuration, or a common device/permission model.

## Decision-oriented summary

- Use `dioxus-cameras` only when the intended runtime is a native Dioxus Desktop application on macOS, Windows, or Linux and accepting the native camera/webview dependency surface is reasonable.
- Treat `dioxus-chimeras` as historical evidence only. Its crates are yanked and its maintained continuation is `dioxus-cameras`.
- Evaluate `dioxus-audio` only for a browser wasm Dioxus client if recording/playback is needed, or for its pure analysis/rendering pieces elsewhere. Do not infer native Dioxus Desktop audio support from the fact that its UI components compile there.
- Do not infer a ready-made audiovisual stack from these libraries. The maintained camera and audio implementations currently meet on Dioxus 0.7 APIs, but not on a common full-featured compilation target.

## Application to this repository

### Recommendation

Do not add any of the three libraries to `livekit-proto` now.

| Library | Recommendation | Reason |
|---|---|---|
| `dioxus-cameras` | Reject for the current project | It is a native Dioxus Desktop capture and preview stack. This project is deliberately client-side Dioxus Web on `wasm32-unknown-unknown`, and its browser camera tracks must remain the same LiveKit-owned objects used for preview, restart, publication, and cleanup. |
| `dioxus-chimeras` | Reject | It has the same target mismatch and is the yanked, superseded predecessor of `dioxus-cameras`. |
| `dioxus-audio` | Do not adopt; use as design input only | It targets browser wasm, but its useful I/O APIs are Dioxus-owned workflows that acquire a separate microphone stream or play copied audio bytes. They do not accept this project's existing Local or Remote Audio Tracks and conflict with the recording artifact and ownership contracts. |

The target mismatch is architectural rather than incidental. The accepted ADR rejects a Dioxus-only core and keeps browser media behavior behind `livekit-browser` ([ADR 0001](adr/0001-wrap-livekit-browser-sdk.md#considered-options)). The plan explicitly excludes Dioxus Desktop and native renderers and requires the Dioxus adapter to remain client-side Web only ([crate plan](livekit-browser-crate-plan.md#dioxus-adapter)).

### Already implemented media ownership

`livekit-browser` already owns the behavior that the camera and audio libraries would otherwise duplicate:

- Typed browser camera/microphone devices and capture constraints are in [`livekit-browser/src/lib.rs`](../crates/livekit-browser/src/lib.rs#L451-L664).
- Local Video Tracks retain one shared LiveKit capture across clones, device restarts, preview attachment, publication, and deterministic stop ([video track](../crates/livekit-browser/src/lib.rs#L1204-L1463)).
- Local Audio Tracks provide the equivalent microphone/system-audio lifecycle ([audio track](../crates/livekit-browser/src/lib.rs#L2301-L2560)).
- Local and Remote Tracks attach to caller-owned browser elements through explicit guards rather than UI-framework-owned playback objects ([remote tracks](../crates/livekit-browser/src/lib.rs#L4410-L4542), [guard cleanup](../crates/livekit-browser/src/lib.rs#L4676-L4768)).
- Capture and enumeration deliberately use LiveKit's `Room.getLocalDevices`, `createLocalVideoTrack`, `createLocalAudioTrack`, and `createLocalScreenTracks` so publication continues to operate on the same LiveKit track objects ([adapter](../crates/livekit-browser/js/livekit-browser.js#L30-L49), [camera](../crates/livekit-browser/js/livekit-browser.js#L693-L701), [audio and screen](../crates/livekit-browser/js/livekit-browser.js#L843-L917)).

Replacing that path with either camera crate is impossible on browser wasm. Letting `dioxus-audio` call `getUserMedia` would create a second microphone capture outside the existing shared-track lifecycle.

### Open-ticket fit

#### [Issue #20: Analyze Local and Remote Audio Tracks](https://github.com/sagikazarmark/klassroom-proto.orig/issues/20)

`dioxus-audio` is not reusable for the ticket's core analyser handle. Its public `AudioAnalyser` constructor is crate-private and the browser implementation creates it only while its own recorder acquires a new microphone stream ([analyser constructor](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/analysis.rs#L11-L47), [recorder setup](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/recorder/web.rs#L267-L323)). It cannot analyse an existing LiveKit Local Audio Track or subscribed Remote Audio Track. Depending on it from core would also violate the intentional rule that core has no Dioxus dependency.

The small sample-conversion and level calculation routines are useful reference implementations, but are too small to justify importing a Dioxus UI/controller crate into the framework-neutral core.

#### [Issue #21: Create bounded Local Clip Recordings](https://github.com/sagikazarmark/klassroom-proto.orig/issues/21)

`dioxus-audio` does not meet the ticket's input or artifact contracts:

- It records a newly acquired microphone stream, not selected existing Local Audio and Video Tracks.
- It is audio-only, while the ticket requires selected camera and microphone tracks.
- It accumulates Blob chunks and then copies the final Blob into `Vec<u8>` in WASM ([collection](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/recorder/web.rs#L873-L899)); the ticket explicitly requires retaining a browser Blob without copying it into WASM memory.
- It has no duration or accumulated-size auto-stop limits and no public revocable object-URL guard.

Its recorder lifecycle and MIME preference ordering are good design references, but adopting the crate would still leave the difficult ticket requirements to implement and would improperly introduce Dioxus into `livekit-browser-recording`.

#### [Issue #22: Complete headless Dioxus Room Session integration](https://github.com/sagikazarmark/klassroom-proto.orig/issues/22)

None of the libraries removes the relevant boilerplate. The project needs unstyled components that obtain Dioxus-mounted `HtmlAudioElement`/`HtmlVideoElement` values and retain existing LiveKit attachment guards. The repeated code is currently visible in the demo's remote nodes ([video](../demo/src/client.rs#L4722-L4959), [audio](../demo/src/client.rs#L4962-L5206)), while `livekit-browser-dioxus` is still an empty shell ([crate root](../crates/livekit-browser-dioxus/src/lib.rs)).

`dioxus-cameras` renders native-camera frames through its own loopback server and canvas. `dioxus-audio` creates a private `HtmlAudioElement` from copied `AudioData`; it does not attach externally owned LiveKit tracks to a mounted element ([player implementation](https://github.com/sagikazarmark/dioxus-audio/blob/a3bbab0cd921f2f1f39acb5b25dcd6b2dac04905/src/playback/web.rs#L207-L265)). Its visual components are styled application UI, which issue #22 explicitly excludes from the headless adapter.

The smallest implementation remains an in-repository provider/hooks layer plus four thin local/remote audio/video attachment nodes over the existing core methods. That removes the example's Dioxus plumbing without introducing another capture, playback, or lifecycle authority.

#### [Issue #24: Browser compatibility gate](https://github.com/sagikazarmark/klassroom-proto.orig/issues/24)

The camera libraries provide no browser evidence. `dioxus-audio` has first-party Chromium Playwright coverage but not this project's required Chromium, Firefox, WebKit, and mobile-sized matrix. Adopting it would add cross-browser behavior that this project would still need to validate and potentially patch itself.

### Reconsideration triggers

Re-evaluate `dioxus-cameras` only if the product adds a separate native Dioxus Desktop client whose camera frames do not need to be LiveKit browser tracks.

Re-evaluate `dioxus-audio` if it gains a framework-neutral lower layer that can consume externally owned `MediaStreamTrack`/`MediaStream` inputs, exposes analyser and recorder construction independently of `getUserMedia`, retains Blob artifacts outside WASM memory, supports bounded recording, and demonstrates Firefox/WebKit behavior. Even then, keep any Dioxus components in `livekit-browser-dioxus`; core and recording should remain Dioxus-free.
