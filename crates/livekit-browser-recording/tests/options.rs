//! Host-target coverage for Local Clip Recording option validation.

use std::time::Duration;

use livekit_browser::Error;
use livekit_browser_recording::{LocalClipLimits, LocalClipOptions};

#[test]
fn local_clip_options_preserve_ordered_mime_preferences() {
    let options = LocalClipOptions::new(
        vec![
            "video/unsupported".to_string(),
            "video/webm;codecs=vp8,opus".to_string(),
            "video/mp4".to_string(),
        ],
        LocalClipLimits::DurationOrSize {
            max_duration: Duration::from_secs(30),
            max_bytes: 8_000_000,
        },
    )
    .expect("valid bounded options");

    assert_eq!(
        options.mime_preferences(),
        [
            "video/unsupported",
            "video/webm;codecs=vp8,opus",
            "video/mp4",
        ]
    );
    assert_eq!(options.max_duration(), Some(Duration::from_secs(30)));
    assert_eq!(options.max_bytes(), Some(8_000_000));
}

#[test]
fn local_clip_options_reject_empty_mime_preferences() {
    assert!(matches!(
        LocalClipOptions::new(
            Vec::new(),
            LocalClipLimits::Duration(Duration::from_secs(1)),
        ),
        Err(Error::InvalidLocalClipRecordingOptions { .. })
    ));
}

#[test]
fn local_clip_options_reject_zero_limits() {
    for limits in [
        LocalClipLimits::Duration(Duration::ZERO),
        LocalClipLimits::Size(0),
        LocalClipLimits::DurationOrSize {
            max_duration: Duration::ZERO,
            max_bytes: 1,
        },
        LocalClipLimits::DurationOrSize {
            max_duration: Duration::from_secs(1),
            max_bytes: 0,
        },
    ] {
        assert!(matches!(
            LocalClipOptions::new(vec!["video/webm".to_string()], limits),
            Err(Error::InvalidLocalClipRecordingOptions { .. })
        ));
    }
}

#[test]
fn local_clip_options_reject_empty_mime_values() {
    assert!(matches!(
        LocalClipOptions::new(
            vec![String::new()],
            LocalClipLimits::Duration(Duration::from_secs(1)),
        ),
        Err(Error::InvalidLocalClipRecordingOptions { .. })
    ));
}
