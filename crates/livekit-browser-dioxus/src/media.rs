use dioxus::{prelude::*, web::WebEventExt};
use livekit_browser::{
    Error, LocalAudioTrack, LocalVideoTrack, RemoteAudioTrack, RemoteVideoTrack,
};
use wasm_bindgen::JsCast;

fn use_media_attachment<Track, MediaElement, Attachment>(
    track: Track,
    on_error: Callback<Error>,
    attach: impl Fn(&Track, &MediaElement) -> Result<Attachment, Error> + Copy + 'static,
) -> Signal<Option<MediaElement>>
where
    Track: Clone + PartialEq + 'static,
    MediaElement: Clone + 'static,
    Attachment: 'static,
{
    let mut current_track = use_signal(|| track.clone());
    let element = use_signal(|| None::<MediaElement>);
    let mut attachment = use_signal(|| None::<Attachment>);

    if *current_track.peek() != track {
        current_track.set(track);
    }
    use_effect(move || {
        attachment.set(None);
        let track = current_track.read().clone();
        let Some(element) = element.read().clone() else {
            return;
        };
        match attach(&track, &element) {
            Ok(attached) => attachment.set(Some(attached)),
            Err(error) => on_error.call(error),
        }
    });

    element
}

/// Properties for an unstyled local video attachment node.
#[derive(Clone, Props, PartialEq)]
pub struct LocalVideoProps {
    pub track: LocalVideoTrack,
    #[props(default)]
    pub on_error: Callback<Error>,
    #[props(extends = video, extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,
}

/// Renders an unstyled `video` element attached to an existing Local Video Track.
#[allow(non_snake_case)]
pub fn LocalVideo(props: LocalVideoProps) -> Element {
    let mut element = use_media_attachment(props.track, props.on_error, LocalVideoTrack::attach);

    rsx! {
        video {
            onmounted: move |event| {
                element.set(Some(event.data().as_web_event().unchecked_into()));
            },
            ..props.attributes,
        }
    }
}

/// Properties for an unstyled local audio attachment node.
#[derive(Clone, Props, PartialEq)]
pub struct LocalAudioProps {
    pub track: LocalAudioTrack,
    #[props(default)]
    pub on_error: Callback<Error>,
    #[props(extends = audio, extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,
}

/// Renders an unstyled `audio` element attached to an existing Local Audio Track.
#[allow(non_snake_case)]
pub fn LocalAudio(props: LocalAudioProps) -> Element {
    let mut element = use_media_attachment(props.track, props.on_error, LocalAudioTrack::attach);

    rsx! {
        audio {
            onmounted: move |event| {
                element.set(Some(event.data().as_web_event().unchecked_into()));
            },
            ..props.attributes,
        }
    }
}

/// Properties for an unstyled remote video attachment node.
#[derive(Clone, Props, PartialEq)]
pub struct RemoteVideoProps {
    pub track: RemoteVideoTrack,
    #[props(default)]
    pub on_error: Callback<Error>,
    #[props(extends = video, extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,
}

/// Renders an unstyled `video` element attached to an existing Remote Video Track.
#[allow(non_snake_case)]
pub fn RemoteVideo(props: RemoteVideoProps) -> Element {
    let mut element = use_media_attachment(props.track, props.on_error, RemoteVideoTrack::attach);

    rsx! {
        video {
            onmounted: move |event| {
                element.set(Some(event.data().as_web_event().unchecked_into()));
            },
            ..props.attributes,
        }
    }
}

/// Properties for an unstyled remote audio attachment node.
#[derive(Clone, Props, PartialEq)]
pub struct RemoteAudioProps {
    pub track: RemoteAudioTrack,
    #[props(default)]
    pub on_error: Callback<Error>,
    #[props(extends = audio, extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,
}

/// Renders an unstyled `audio` element attached to an existing Remote Audio Track.
#[allow(non_snake_case)]
pub fn RemoteAudio(props: RemoteAudioProps) -> Element {
    let mut element = use_media_attachment(props.track, props.on_error, RemoteAudioTrack::attach);

    rsx! {
        audio {
            onmounted: move |event| {
                element.set(Some(event.data().as_web_event().unchecked_into()));
            },
            ..props.attributes,
        }
    }
}
