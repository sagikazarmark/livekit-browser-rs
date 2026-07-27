# livekit-browser-recording

[![crates.io](https://img.shields.io/crates/v/livekit-browser-recording?style=flat-square)](https://crates.io/crates/livekit-browser-recording)
[![docs.rs](https://img.shields.io/docsrs/livekit-browser-recording?style=flat-square)](https://docs.rs/livekit-browser-recording)

**Browser Local Clip Recording for
[`livekit-browser`](https://crates.io/crates/livekit-browser) Local Media Tracks.**

A Local Clip Recording encodes selected Local Media Tracks into a bounded, browser-owned artifact
using `MediaRecorder`. It is independent of LiveKit: recording needs no Room Session and no Track
Publication, and it neither starts nor stops capture.

This is deliberately **not** server-side Room Recording. Only `LocalAudioTrack` and `LocalVideoTrack`
convert into `LocalClipTrack`; Remote Media Tracks are rejected at compile time, because capturing
other participants belongs to LiveKit Egress.

## Usage

```rust
use std::time::Duration;

use livekit_browser_recording::{LocalClipLimits, LocalClipOptions, LocalClipRecorder};

// Negotiates browser capabilities. Requests no permissions and builds no MediaRecorder.
let recorder = LocalClipRecorder::new(client.capabilities())?;

// MIME types are tried in caller order; support is checked before anything is constructed.
let options = LocalClipOptions::new(
    vec!["video/webm;codecs=vp9,opus".into(), "video/webm".into()],
    LocalClipLimits::Duration(Duration::from_secs(30)),
)?;

// Takes ownership of the selected tracks for the recording's lifetime.
let recording = recorder.start([camera.into(), microphone.into()], options)?;

// `stop` returns a future resolving to the finished artifact.
let clip = recording.stop()?.await?;

// Encoded bytes stay in browser memory; the guard revokes on drop or explicit call.
let url = clip.create_object_url()?;
println!("{} ({})", url.as_str(), clip.suggested_filename());
```

## Lifecycle

`LocalClipRecorder::new` negotiates browser capabilities without requesting permissions or
constructing a `MediaRecorder`. `LocalClipRecorder::start` takes ownership of the selected tracks and
returns a `LocalClipRecording` that can be paused, resumed, and stopped.

Recording also stops on its own when a configured `LocalClipLimits` duration or size threshold is
reached, or when every selected track ends. `LocalClipStopReason` distinguishes these.

## Artifact ownership

A finished `LocalClip` wraps a browser `Blob`. Encoded bytes stay in browser memory and are never
copied into WASM linear memory. `LocalClip::create_object_url` returns a `RevocableObjectUrl` for
download, which revokes on explicit call or final drop so the browser reclaims the artifact.

## Browser support

`MediaRecorder` and its supported MIME types vary by browser. `LocalClipOptions` takes MIME types in
caller preference order, and support is checked before any recorder is constructed, so an unsupported
format fails before mutating state.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
