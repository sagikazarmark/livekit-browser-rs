# Browser Realtime Media

This context describes a browser participant's realtime room experience and media artifacts without conflating browser-local behavior with LiveKit server capabilities.

## Language

**Room Session**:
One Local Participant's participation lifetime in a room, from joining until explicit leave or terminal disconnection. Automatic reconnection remains part of the same Room Session.
_Avoid_: Connection, Room Handle

**Track Publication**:
A participant's room-visible declaration of an audio or video source, including its identity and current availability state. A publication can exist when its media is not locally available.
_Avoid_: Track

**Media Track**:
A locally available stream of audio or video. A Local Media Track can exist without a Track Publication; a Remote Media Track is available through one.
_Avoid_: Publication, Track Publication

**Local Media Track**:
A browser-captured camera, microphone, or screen stream controlled by the Local Participant. It can exist before joining or without being published.
_Avoid_: Local Publication, Preview Track

**Remote Media Track**:
A locally available subscribed stream associated with a Remote Participant's Track Publication.
_Avoid_: Remote Publication

**Audio Analyser**:
A browser-local Web Audio graph associated with one exact Local Media Track or Remote Media Track carrying audio. Reads return owned level and waveform data; close, final drop, or source termination releases the graph.
_Avoid_: JavaScript Analyser Handle

**Audio Analysis**:
One owned Rust observation read from an Audio Analyser, containing normalized waveform samples and their root-mean-square level.
_Avoid_: JavaScript Sample Buffer

**Local Participant**:
The browser user represented in the room, with authority to publish and control local media sources.
_Avoid_: User, Current User

**Remote Participant**:
Another room participant whose state and published sources are observed by the Local Participant.
_Avoid_: Peer, Other User

**Participant RPC**:
A method-routed request and response between a Local Participant and one Remote Participant, with a bounded response timeout and typed transport or application failure. Participant RPC is not a Room Event or Data Stream.
_Avoid_: Command Event, RPC Stream

**Data Stream**:
A topic-routed sequence of text or bytes exchanged incrementally between two participants. Each sender write remains backpressured until the destination pulls that chunk. A Data Stream is not a Track Publication and retains the application's native string or byte payload without imposing a codec.
_Avoid_: Data Packet, Data Track, File Transfer

**Data Track**:
A separately published, named LiveKit data-frame source. Data Tracks are not Track Publications, Media Tracks, Data Packets, or Data Streams and have their own subscription lifetime.
_Avoid_: Data Track Publication, Data Media Track

**Chat Message**:
A LiveKit participant message with native string content, a stable message ID, creation or edit timing, and sender Participant Identity when LiveKit supplies it. Chat Messages use a dedicated bounded delivery interface rather than Room Events.
_Avoid_: Room Event, Data Packet

**Transcription Segment**:
A transient LiveKit-normalized speech-text update with a stable segment ID, source Participant Identity and Track Publication SID when available, timing and language, and interim or final state. Transcription Segments use a dedicated bounded delivery interface rather than Room Events.
_Avoid_: Caption Event, Room Event

**Local Clip Recording**:
A browser-created recording of selected local media tracks, retained only long enough to produce a downloadable artifact.
_Avoid_: Recording, Room Recording

**Room Recording**:
A server-side recording of room media produced through LiveKit Egress.
_Avoid_: Local Recording, Clip Recording
