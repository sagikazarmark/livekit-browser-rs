//! Compile-checks the example in this crate's README.

use std::time::Duration;

use livekit_browser::{BrowserClient, Error, LocalAudioTrack, LocalVideoTrack};
use livekit_browser_recording::{LocalClipLimits, LocalClipOptions, LocalClipRecorder};

#[allow(dead_code)]
async fn readme_example(
    client: BrowserClient,
    camera: LocalVideoTrack,
    microphone: LocalAudioTrack,
) -> Result<(), Error> {
    let recorder = LocalClipRecorder::new(client.capabilities())?;

    let options = LocalClipOptions::new(
        vec!["video/webm;codecs=vp9,opus".into(), "video/webm".into()],
        LocalClipLimits::Duration(Duration::from_secs(30)),
    )?;

    let recording = recorder.start([camera.into(), microphone.into()], options)?;

    let clip = recording.stop()?.await?;

    let url = clip.create_object_url()?;
    println!("{} ({})", url.as_str(), clip.suggested_filename());
    Ok(())
}
