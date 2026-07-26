use super::*;

/// The delivery mode LiveKit uses for a data packet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DataPacketDelivery {
    /// Retransmit lost packets and preserve ordering at the cost of head-of-line blocking.
    Reliable,
    /// Prefer low latency by allowing packets to be lost.
    Lossy,
}

/// One native byte packet received from another participant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataPacket {
    pub(crate) payload: Vec<u8>,
    pub(crate) sender_identity: Option<String>,
    pub(crate) delivery: DataPacketDelivery,
}

impl DataPacket {
    /// Returns the packet payload without applying a codec or text conversion.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Returns the sending Participant Identity when LiveKit supplied one.
    pub fn sender_identity(&self) -> Option<&str> {
        self.sender_identity.as_deref()
    }

    /// Returns the packet's LiveKit delivery mode.
    pub const fn delivery(&self) -> DataPacketDelivery {
        self.delivery
    }
}

/// One observation from a bounded data-packet subscription.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DataPacketStreamItem {
    /// A native byte packet received in the subscription's delivery mode.
    Packet(DataPacket),
    /// This subscriber fell behind and the oldest buffered packets were dropped.
    Lagged {
        /// The exact number of packets dropped since the previous stream item.
        dropped: usize,
    },
}

pub(super) struct BoundedQueue<T> {
    #[cfg(any(target_arch = "wasm32", test))]
    pub(super) capacity: usize,
    pub(super) items: VecDeque<T>,
    pub(super) dropped: usize,
    pub(super) closed: bool,
    pub(super) waker: Option<Waker>,
}

impl<T> BoundedQueue<T> {
    fn poll_next_mapped<U>(
        &mut self,
        context: &mut Context<'_>,
        item: impl FnOnce(T) -> U,
        lagged: impl FnOnce(usize) -> U,
    ) -> Poll<Option<U>> {
        if self.dropped > 0 {
            let dropped = std::mem::take(&mut self.dropped);
            return Poll::Ready(Some(lagged(dropped)));
        }
        if let Some(value) = self.items.pop_front() {
            return Poll::Ready(Some(item(value)));
        }
        if self.closed {
            return Poll::Ready(None);
        }

        self.waker = Some(context.waker().clone());
        Poll::Pending
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(super) fn push(&mut self, item: T) {
        if self.items.len() == self.capacity {
            self.items.pop_front();
            self.dropped += 1;
        }
        self.items.push_back(item);

        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }

    fn close(&mut self) {
        self.closed = true;
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

pub(super) fn close_bounded_queues<T>(
    queues: impl IntoIterator<Item = Weak<RefCell<BoundedQueue<T>>>>,
) {
    for queue in queues {
        if let Some(queue) = queue.upgrade() {
            queue.borrow_mut().close();
        }
    }
}

pub(super) type DataPacketQueue = BoundedQueue<DataPacket>;

/// A browser-thread-local, bounded stream of packets for one delivery mode.
pub struct DataPacketStream {
    pub(super) queue: Rc<RefCell<DataPacketQueue>>,
}

impl Stream for DataPacketStream {
    type Item = DataPacketStreamItem;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.queue
            .borrow_mut()
            .poll_next_mapped(context, DataPacketStreamItem::Packet, |dropped| {
                DataPacketStreamItem::Lagged { dropped }
            })
    }
}

pub(super) struct DataPacketSubscription {
    #[cfg(any(target_arch = "wasm32", test))]
    pub(super) delivery: DataPacketDelivery,
    pub(super) queue: Weak<RefCell<DataPacketQueue>>,
}

/// One chat message sent through LiveKit's participant messaging workflow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatMessage {
    pub(crate) id: String,
    pub(crate) message: String,
    pub(crate) sender_identity: Option<String>,
    pub(crate) timestamp: SystemTime,
    pub(crate) edit_timestamp: Option<SystemTime>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
pub(crate) struct AdapterChatMessage {
    id: String,
    message: String,
    sender_identity: Option<String>,
    #[cfg_attr(test, ts(type = "number"))]
    timestamp_millis: u64,
    #[cfg_attr(test, ts(type = "number | null"))]
    edit_timestamp_millis: Option<u64>,
}

#[cfg(target_arch = "wasm32")]
impl From<AdapterChatMessage> for ChatMessage {
    fn from(message: AdapterChatMessage) -> Self {
        Self {
            id: message.id,
            message: message.message,
            sender_identity: message.sender_identity,
            timestamp: std::time::UNIX_EPOCH + Duration::from_millis(message.timestamp_millis),
            edit_timestamp: message
                .edit_timestamp_millis
                .map(|timestamp| std::time::UNIX_EPOCH + Duration::from_millis(timestamp)),
        }
    }
}

impl ChatMessage {
    /// Returns LiveKit's stable identifier for this message.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the message content as its native string.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the sending Participant Identity when LiveKit supplied one.
    pub fn sender_identity(&self) -> Option<&str> {
        self.sender_identity.as_deref()
    }

    /// Returns the time at which LiveKit says the message was created.
    pub const fn timestamp(&self) -> SystemTime {
        self.timestamp
    }

    /// Returns the time of the latest LiveKit chat edit, when present.
    pub const fn edit_timestamp(&self) -> Option<SystemTime> {
        self.edit_timestamp
    }
}

/// One observation from a bounded chat-message subscription.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ChatMessageStreamItem {
    /// A chat message sent by the Local Participant or a Remote Participant.
    Message(ChatMessage),
    /// This subscriber fell behind and the oldest buffered messages were dropped.
    Lagged {
        /// The exact number of messages dropped since the previous stream item.
        dropped: usize,
    },
}

/// A browser-thread-local, bounded stream of chat messages.
pub struct ChatMessageStream {
    pub(super) queue: Rc<RefCell<ChatMessageQueue>>,
}

impl Stream for ChatMessageStream {
    type Item = ChatMessageStreamItem;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.queue.borrow_mut().poll_next_mapped(
            context,
            ChatMessageStreamItem::Message,
            |dropped| ChatMessageStreamItem::Lagged { dropped },
        )
    }
}

pub(super) type ChatMessageQueue = BoundedQueue<ChatMessage>;

pub(super) struct ChatMessageSubscription {
    pub(super) queue: Weak<RefCell<ChatMessageQueue>>,
}

/// One LiveKit-normalized update to a speech transcription segment.
///
/// Successive interim and final updates retain the same segment ID. Participant Identity and
/// Track Publication SID are optional because LiveKit may not resolve their browser objects.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranscriptionSegment {
    pub(crate) id: String,
    pub(crate) participant_identity: Option<String>,
    pub(crate) track_publication_sid: Option<String>,
    pub(crate) text: String,
    pub(crate) start_time: u64,
    pub(crate) end_time: u64,
    pub(crate) language: String,
    pub(crate) is_final: bool,
    pub(crate) first_received_at: SystemTime,
    pub(crate) last_received_at: SystemTime,
}

impl TranscriptionSegment {
    /// Returns LiveKit's stable identity for this segment across interim and final updates.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the transcribed Participant Identity when LiveKit resolved it.
    pub fn participant_identity(&self) -> Option<&str> {
        self.participant_identity.as_deref()
    }

    /// Returns the transcribed Track Publication SID when LiveKit resolved it.
    pub fn track_publication_sid(&self) -> Option<&str> {
        self.track_publication_sid.as_deref()
    }

    /// Returns the latest text for this segment.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns LiveKit's normalized start-time integer without inventing units not specified by
    /// the pinned SDK.
    pub const fn start_time(&self) -> u64 {
        self.start_time
    }

    /// Returns LiveKit's normalized end-time integer without inventing units not specified by the
    /// pinned SDK.
    pub const fn end_time(&self) -> u64 {
        self.end_time
    }

    /// Returns the BCP-47 language tag supplied by LiveKit, or an empty string when absent.
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Whether this update completes the segment rather than replacing interim text.
    pub const fn is_final(&self) -> bool {
        self.is_final
    }

    /// Returns when LiveKit first normalized this segment in the local browser.
    pub const fn first_received_at(&self) -> SystemTime {
        self.first_received_at
    }

    /// Returns when LiveKit normalized this specific update in the local browser.
    pub const fn last_received_at(&self) -> SystemTime {
        self.last_received_at
    }
}

/// One observation from a bounded transcription-segment subscription.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TranscriptionSegmentStreamItem {
    /// An interim or final transcription segment update.
    Segment(TranscriptionSegment),
    /// This subscriber fell behind and the oldest buffered updates were dropped.
    Lagged {
        /// The exact number of updates dropped since the previous stream item.
        dropped: usize,
    },
}

pub(super) type TranscriptionSegmentQueue = BoundedQueue<TranscriptionSegment>;

/// A browser-thread-local, bounded stream of transcription segment updates.
pub struct TranscriptionSegmentStream {
    pub(super) queue: Rc<RefCell<TranscriptionSegmentQueue>>,
}

impl Stream for TranscriptionSegmentStream {
    type Item = TranscriptionSegmentStreamItem;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.queue.borrow_mut().poll_next_mapped(
            context,
            TranscriptionSegmentStreamItem::Segment,
            |dropped| TranscriptionSegmentStreamItem::Lagged { dropped },
        )
    }
}

pub(super) struct TranscriptionSegmentSubscription {
    pub(super) queue: Weak<RefCell<TranscriptionSegmentQueue>>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, allow(dead_code))]
pub(crate) struct AdapterTranscriptionSegment {
    id: String,
    participant_identity: Option<String>,
    track_publication_sid: Option<String>,
    text: String,
    #[cfg_attr(test, ts(type = "number"))]
    start_time: u64,
    #[cfg_attr(test, ts(type = "number"))]
    end_time: u64,
    language: String,
    is_final: bool,
    #[cfg_attr(test, ts(type = "number"))]
    first_received_at_millis: u64,
    #[cfg_attr(test, ts(type = "number"))]
    last_received_at_millis: u64,
}

#[cfg(any(target_arch = "wasm32", test))]
impl AdapterTranscriptionSegment {
    pub(crate) fn into_segment(self) -> Option<TranscriptionSegment> {
        Some(TranscriptionSegment {
            id: self.id,
            participant_identity: self.participant_identity,
            track_publication_sid: self.track_publication_sid,
            text: self.text,
            start_time: self.start_time,
            end_time: self.end_time,
            language: self.language,
            is_final: self.is_final,
            first_received_at: std::time::UNIX_EPOCH
                .checked_add(Duration::from_millis(self.first_received_at_millis))?,
            last_received_at: std::time::UNIX_EPOCH
                .checked_add(Duration::from_millis(self.last_received_at_millis))?,
        })
    }
}
