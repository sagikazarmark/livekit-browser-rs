//! Rendering and drift detection for the generated TypeScript adapter contracts.
//!
//! The DTOs derive `ts_rs::TS` only under `cfg(test)`, which keeps the code generator out of the
//! shipped wasm dependency graph and makes a test the only place these declarations can be
//! rendered. [`adapter_contracts_are_fresh`] therefore both checks the committed file and, on
//! drift, rewrites it.

use crate::client::{ExactDeviceId, RuntimeInspection};
use crate::media::{AdapterAudioPublishOptions, AdapterScreenCapture, AdapterVideoPublishOptions};
use crate::room::{
    AdapterByteStreamRead, AdapterChatMessage, AdapterIncomingByteStream,
    AdapterIncomingTextStream, AdapterJoin, AdapterOutgoingByteStream, AdapterOutgoingTextStream,
    AdapterRemoteMediaTrack, AdapterRemoteParticipantSnapshot,
    AdapterRemoteTrackPublicationSnapshot, AdapterRoomSnapshot, AdapterRpcFailure,
    AdapterRpcHandlerCompletion, AdapterRpcInvocation, AdapterRpcOptions, AdapterTextStreamRead,
    AdapterTranscriptionSegment, AdapterUpdate, RemoteParticipantLocator, RemotePublicationLocator,
};
use crate::*;

use std::fs;
use std::path::{Path, PathBuf};
use ts_rs::{Config, TS};

const ADAPTER_CONTRACTS_HEADER: &str =
    "// Generated from Rust DTOs by `cargo test -p livekit-browser`. Do not edit.\n\n";

fn adapter_contracts_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("js/src/generated/adapter-contracts.ts")
}

fn append_contract<T: TS>(output: &mut String, config: &Config) {
    output.push_str("export ");
    output.push_str(&T::decl(config));
    output.push_str("\n\n");
}

fn render_adapter_contracts() -> String {
    let mut output = String::from(ADAPTER_CONTRACTS_HEADER);
    let config = Config::default();

    // Keep this registry explicit and ordered so contract additions are intentional and diffs
    // are stable across filesystems.
    append_contract::<BrowserCapabilities>(&mut output, &config);
    append_contract::<RoomConnectionState>(&mut output, &config);
    append_contract::<ConnectionQuality>(&mut output, &config);
    append_contract::<AudioPlaybackState>(&mut output, &config);
    append_contract::<TrackSource>(&mut output, &config);
    append_contract::<TrackKind>(&mut output, &config);
    append_contract::<ParticipantGrants>(&mut output, &config);
    append_contract::<LocalTrackPublicationSnapshot>(&mut output, &config);
    append_contract::<LocalParticipantSnapshot>(&mut output, &config);
    append_contract::<RemoteDataTrackSnapshot>(&mut output, &config);
    append_contract::<AdapterRemoteTrackPublicationSnapshot>(&mut output, &config);
    append_contract::<AdapterRemoteParticipantSnapshot>(&mut output, &config);
    append_contract::<AdapterRoomSnapshot>(&mut output, &config);
    append_contract::<RoomEventKind>(&mut output, &config);
    append_contract::<AdapterJoin>(&mut output, &config);
    append_contract::<AdapterUpdate>(&mut output, &config);
    append_contract::<RuntimeInspection>(&mut output, &config);
    append_contract::<AdapterScreenCapture>(&mut output, &config);
    append_contract::<RemoteParticipantLocator>(&mut output, &config);
    append_contract::<RemotePublicationLocator>(&mut output, &config);
    append_contract::<AdapterRemoteMediaTrack>(&mut output, &config);
    append_contract::<AdapterRpcOptions>(&mut output, &config);
    append_contract::<AdapterRpcFailure>(&mut output, &config);
    append_contract::<AdapterRpcInvocation>(&mut output, &config);
    append_contract::<AdapterRpcHandlerCompletion>(&mut output, &config);
    append_contract::<TextStreamOptions>(&mut output, &config);
    append_contract::<ByteStreamOptions>(&mut output, &config);
    append_contract::<TextStreamInfo>(&mut output, &config);
    append_contract::<ByteStreamInfo>(&mut output, &config);
    append_contract::<AdapterOutgoingTextStream>(&mut output, &config);
    append_contract::<AdapterOutgoingByteStream>(&mut output, &config);
    append_contract::<AdapterTextStreamRead>(&mut output, &config);
    append_contract::<AdapterIncomingTextStream>(&mut output, &config);
    append_contract::<AdapterIncomingByteStream>(&mut output, &config);
    append_contract::<AdapterByteStreamRead>(&mut output, &config);
    append_contract::<MediaDeviceKind>(&mut output, &config);
    append_contract::<MediaDeviceInfo>(&mut output, &config);
    append_contract::<SenderStatistics>(&mut output, &config);
    append_contract::<VideoSenderStatistics>(&mut output, &config);
    append_contract::<ReceiverStatistics>(&mut output, &config);
    append_contract::<AudioReceiverStatistics>(&mut output, &config);
    append_contract::<VideoReceiverStatistics>(&mut output, &config);
    append_contract::<AdapterChatMessage>(&mut output, &config);
    append_contract::<AdapterTranscriptionSegment>(&mut output, &config);
    append_contract::<ExactDeviceId>(&mut output, &config);
    append_contract::<FacingMode>(&mut output, &config);
    append_contract::<CameraResolution>(&mut output, &config);
    append_contract::<CameraCaptureOptions>(&mut output, &config);
    append_contract::<MicrophoneCaptureOptions>(&mut output, &config);
    append_contract::<ScreenCaptureOptions>(&mut output, &config);
    append_contract::<VideoCodec>(&mut output, &config);
    append_contract::<VideoEncoding>(&mut output, &config);
    append_contract::<AdapterVideoPublishOptions>(&mut output, &config);
    append_contract::<AudioCodec>(&mut output, &config);
    append_contract::<AdapterAudioPublishOptions>(&mut output, &config);
    append_contract::<PublicationSubscriptionPermissions>(&mut output, &config);
    assert_eq!(output.pop(), Some('\n'));

    output
}

/// Compares `contents` with the file at `path`, rewriting it on mismatch and then failing.
///
/// A local `cargo test` run applies the fix and passes on the next run; CI fails on the single
/// run it performs. Mirrors `ensure_file_contents` from rust-analyzer's `test-utils` crate.
#[track_caller]
fn ensure_file_contents(path: &Path, contents: &str) {
    if let Ok(committed) = fs::read_to_string(path) {
        if committed.replace("\r\n", "\n") == contents {
            return;
        }
    }

    eprintln!(
        "\n{} was not up to date and has been rewritten.",
        path.display()
    );
    if std::env::var_os("CI").is_some() {
        eprintln!("NOTE: run `cargo test -p livekit-browser` locally and commit the result.");
    }
    eprintln!();

    fs::create_dir_all(path.parent().expect("generated contracts parent"))
        .expect("create generated contracts directory");
    fs::write(path, contents).expect("write generated adapter contracts");
    panic!("adapter contracts were stale; re-run the tests to confirm the update");
}

#[test]
fn adapter_contracts_are_fresh() {
    let contracts = render_adapter_contracts();
    assert!(contracts.is_ascii(), "adapter contracts must remain ASCII");
    assert!(
        !contracts.contains('\r'),
        "adapter contracts must use LF endings"
    );
    ensure_file_contents(&adapter_contracts_path(), &contracts);
}

#[test]
fn adapter_contract_exclusions_are_documented() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/browser-compatibility.md");
    let documentation = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "read adapter contract documentation at {}: {error}",
            path.display()
        )
    });
    for marker in [
        "`BTreeMap<String, String>`",
        "`serde_json::Value`",
        "Positional data-packet callbacks",
    ] {
        assert!(
            documentation.contains(marker),
            "adapter contract exclusion {marker} is not documented"
        );
    }
}
