import type { LocalParticipant, RemoteParticipant, Room } from "livekit-client";

import { requireMethods } from "./runtime";

type StreamKind = "text" | "bytes";
type Completion = (success: boolean, value?: unknown) => void;
type OpenCompletion = (success: boolean, value: unknown) => void;
type StreamChunk = string | Uint8Array;

export interface DataStreamRoomRecord {
  id: number;
  room: Room;
  released: boolean;
}

interface Dependencies {
  getRoom(roomId: number): DataStreamRoomRecord | undefined;
  requireDataPublishing(participant: LocalParticipant): void;
  enqueueReliableMutation<T>(
    room: DataStreamRoomRecord,
    mutation: () => T | PromiseLike<T>,
  ): Promise<T>;
}

interface DataStreamOptions {
  destinationIdentity?: unknown;
  topic?: unknown;
  totalSize?: number;
}

interface NativeStreamInfo {
  id?: unknown;
  topic?: unknown;
  mimeType?: unknown;
  timestamp?: unknown;
  size?: unknown;
  attributes?: Record<string, string>;
  attachedStreamIds?: unknown;
  name?: unknown;
}

interface NativeWriter {
  info: NativeStreamInfo;
  write(chunk: StreamChunk): Promise<void>;
  close(): Promise<void>;
  defaultWriter?: { abort(error: unknown): Promise<void>; releaseLock(): void };
  onClose?: () => void;
}

interface NativeReader extends AsyncIterable<unknown> {
  info: NativeStreamInfo;
  withAbortSignal(signal: AbortSignal): AsyncIterable<unknown>;
}

interface NormalizedStreamInfo {
  id: string;
  topic: string;
  mime_type: string;
  timestamp: number;
  size: number | null;
  attributes: Record<string, string>;
  sender_identity: string | null;
  attached_stream_ids?: unknown[];
  name?: string;
}

interface StreamDescriptor {
  id: number;
  info: NormalizedStreamInfo;
}

type OperationSettlement = (success: boolean, error?: unknown) => void;

interface PendingAcknowledgement {
  expectedAcknowledgement: number;
  writerDone: boolean;
  settle: OperationSettlement;
}

interface OutgoingStreamRecord {
  id: number;
  roomId: number;
  writer: NativeWriter;
  kind: StreamKind;
  wireId: string;
  destinationIdentity: string;
  destinationParticipant: RemoteParticipant;
  applicationTopic: string;
  sent: number;
  acknowledged: number;
  pendingAck: PendingAcknowledgement | null;
  openResolve: (() => void) | null;
  openReject: ((error: unknown) => void) | null;
  openTimer: ReturnType<typeof setTimeout> | null;
  closing: boolean;
  terminal: boolean;
  pendingOperations: Set<OperationSettlement>;
}

interface IncomingStreamRecord {
  id: number;
  roomId: number;
  kind: StreamKind;
  wireId: string;
  senderIdentity: string;
  senderParticipant: RemoteParticipant;
  reader: NativeReader;
  iterator: AsyncIterator<unknown>;
  abortController: AbortController;
  readTail: Promise<unknown>;
  consumed: number;
  terminal: boolean;
}

interface HandlerRegistration {
  id: number;
  roomId: number;
  topic: string;
  kind: StreamKind;
  callback(descriptor: StreamDescriptor): void;
}

interface RoomState {
  outgoingIds: Set<number>;
  incomingIds: Set<number>;
  handlerIds: Set<number>;
  textHandlerInstalled: boolean;
  byteHandlerInstalled: boolean;
  pendingOpens: Set<OpenCompletion>;
}

interface FlowControlMessage {
  v: 1;
  streamId: string;
  type?: unknown;
  consumed?: unknown;
}

const FLOW_TEXT_TOPIC = "~livekit-browser/flow/v1/text";
const FLOW_BYTES_TOPIC = "~livekit-browser/flow/v1/bytes";
const FLOW_CONTROL_TOPIC = "~livekit-browser/flow/v1/control";
const FLOW_APPLICATION_TOPIC_ATTRIBUTE = "~livekit-browser.topic";
const FLOW_PROTOCOL_ATTRIBUTE = "~livekit-browser.flow";
const FLOW_PROTOCOL_VERSION = "1";
const MAX_ACTIVE_INCOMING_DATA_STREAMS = 16;
const FLOW_OPEN_TIMEOUT_MS = 10_000;
const FLOW_SDK_OPERATION_TIMEOUT_MS = 30_000;

export function createDataStreamSubsystem(dependencies: Dependencies) {
  let nextOutgoingId = 1;
  let nextIncomingId = 1;
  let nextHandlerId = 1;
  const outgoingStreams = new Map<number, OutgoingStreamRecord>();
  const incomingStreams = new Map<number, IncomingStreamRecord>();
  const handlers = new Map<number, HandlerRegistration>();
  const roomStates = new WeakMap<DataStreamRoomRecord, RoomState>();
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();

  function stateFor(room: DataStreamRoomRecord): RoomState {
    let state = roomStates.get(room);
    if (!state) {
      state = {
        outgoingIds: new Set(),
        incomingIds: new Set(),
        handlerIds: new Set(),
        textHandlerInstalled: false,
        byteHandlerInstalled: false,
        pendingOpens: new Set(),
      };
      roomStates.set(room, state);
    }
    return state;
  }

  function roomForStream(roomId: number) {
    const room = dependencies.getRoom(roomId);
    if (!room || room.released) throw new Error("Room Session is no longer available");
    return room;
  }

  function flowControlError(name: string, message: string, reasonName = name.replace(/Error$/, "")) {
    const error = new Error(message);
    error.name = name;
    (error as Error & { reasonName: string }).reasonName = reasonName;
    return error;
  }

  function terminalError() {
    return new Error("Room Session or data stream became terminal");
  }

  function enqueueBoundedMutation<T>(
    room: DataStreamRoomRecord,
    mutation: () => T | PromiseLike<T>,
    timeoutMessage: string,
  ): Promise<T> {
    return dependencies.enqueueReliableMutation(room, () => {
      let timer: ReturnType<typeof setTimeout>;
      let operation: Promise<T>;
      try {
        operation = Promise.resolve(mutation());
      } catch (error: unknown) {
        operation = Promise.reject(error);
      }
      const timeout = new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(flowControlError("FlowControlTimeoutError", timeoutMessage)),
          FLOW_SDK_OPERATION_TIMEOUT_MS,
        );
      });
      return Promise.race([operation, timeout]).finally(() => clearTimeout(timer));
    });
  }

  function wireTopic(kind: StreamKind) {
    return kind === "text" ? FLOW_TEXT_TOPIC : FLOW_BYTES_TOPIC;
  }

  function nativeOptions(options: DataStreamOptions, kind: StreamKind) {
    return {
      topic: wireTopic(kind),
      totalSize: options.totalSize,
      destinationIdentities: [options.destinationIdentity as string],
      attributes: {
        [FLOW_PROTOCOL_ATTRIBUTE]: FLOW_PROTOCOL_VERSION,
        [FLOW_APPLICATION_TOPIC_ATTRIBUTE]: options.topic as string,
      },
    };
  }

  function sendFlowControl(
    room: DataStreamRoomRecord,
    destinationIdentity: string,
    message: Record<string, unknown>,
  ) {
    if (room.released) return Promise.reject(terminalError());
    const payload = encoder.encode(JSON.stringify({ v: 1, ...message }));
    return enqueueBoundedMutation(
      room,
      () =>
        room.room.localParticipant.publishData(payload, {
          reliable: true,
          topic: FLOW_CONTROL_TOPIC,
          destinationIdentities: [destinationIdentity],
        }),
      "LiveKit did not send Data Stream flow control",
    );
  }

  function parseFlowControl(payload: Uint8Array): FlowControlMessage | null {
    try {
      const message: unknown = JSON.parse(decoder.decode(payload));
      if (!message || typeof message !== "object") return null;
      const candidate = message as Partial<FlowControlMessage>;
      if (candidate.v !== 1 || typeof candidate.streamId !== "string") return null;
      return candidate as FlowControlMessage;
    } catch {
      return null;
    }
  }

  function normalizedSize(size: unknown) {
    const normalized = Number(size);
    return Number.isSafeInteger(normalized) && normalized >= 0 ? normalized : null;
  }

  function normalizeInfo(info: NativeStreamInfo, senderIdentity: unknown): NormalizedStreamInfo {
    return {
      id: typeof info.id === "string" ? info.id : "",
      topic: typeof info.topic === "string" ? info.topic : "",
      mime_type: typeof info.mimeType === "string" ? info.mimeType : "",
      timestamp: Math.max(0, Math.trunc(Number(info.timestamp) || 0)),
      size: normalizedSize(info.size),
      attributes: { ...(info.attributes || {}) },
      sender_identity: typeof senderIdentity === "string" ? senderIdentity : null,
    };
  }

  function normalizeTextInfo(info: NativeStreamInfo, senderIdentity: unknown = null) {
    return {
      ...normalizeInfo(info, senderIdentity),
      attached_stream_ids: Array.isArray(info.attachedStreamIds) ? [...info.attachedStreamIds] : [],
    };
  }

  function normalizeByteInfo(info: NativeStreamInfo, senderIdentity: unknown = null) {
    return {
      ...normalizeInfo(info, senderIdentity),
      name: typeof info.name === "string" ? info.name : "",
    };
  }

  function beginOutgoing(
    roomId: number,
    options: DataStreamOptions,
    kind: StreamKind,
    onComplete: OpenCompletion,
  ) {
    const room = roomForStream(roomId);
    const participant = room.room.localParticipant;
    dependencies.requireDataPublishing(participant);
    const destinationIdentity = options?.destinationIdentity;
    const applicationTopic = options?.topic;
    if (typeof destinationIdentity !== "string" || destinationIdentity.length === 0) {
      throw new TypeError("Data Stream destination Participant Identity is required");
    }
    if (typeof applicationTopic !== "string" || applicationTopic.length === 0) {
      throw new TypeError("Data Stream application topic is required");
    }
    const destinationParticipant = room.room.remoteParticipants.get(destinationIdentity);
    if (!destinationParticipant) {
      throw flowControlError("DestinationUnavailableError", "Data Stream destination is unavailable");
    }
    const roomState = stateFor(room);
    let callback: OpenCompletion | null = onComplete;
    let openSettled = false;
    const settle: OpenCompletion = (success, value) => {
      if (!callback) return;
      const current = callback;
      callback = null;
      openSettled = true;
      roomState.pendingOpens.delete(settle);
      current(success, value);
    };
    roomState.pendingOpens.add(settle);
    const operation = (async () => {
      if (room.released) throw new Error("Room Session is no longer available");
      let nativeOpen: Promise<unknown>;
      const writer = await enqueueBoundedMutation(
        room,
        () => {
          nativeOpen =
            kind === "text"
              ? participant.streamText(nativeOptions(options, kind))
              : participant.streamBytes(nativeOptions(options, kind));
          void Promise.resolve(nativeOpen)
            .then((lateWriter: unknown) => {
              const writer = lateWriter as Partial<NativeWriter> | null;
              if (openSettled) void Promise.resolve(writer?.close?.()).catch(() => {});
            })
            .catch(() => {});
          return nativeOpen;
        },
        "LiveKit did not open the native Data Stream",
      ) as NativeWriter;
      try {
        requireMethods(writer, "Data Stream writer", ["write", "close"]);
      } catch (error: unknown) {
        void Promise.resolve(writer?.close?.()).catch(() => {});
        throw error;
      }
      if (typeof writer.info?.id !== "string" || writer.info.id.length === 0) {
        await writer.close().catch(() => {});
        throw new TypeError("LiveKit Data Stream writer info ID is missing");
      }
      if (
        openSettled ||
        room.released ||
        room.room.remoteParticipants.get(destinationIdentity) !== destinationParticipant
      ) {
        await writer.close().catch(() => {});
        throw flowControlError(
          "DestinationUnavailableError",
          "Data Stream destination changed while opening",
        );
      }
      const id = nextOutgoingId++;
      const record: OutgoingStreamRecord = {
        id,
        roomId,
        writer,
        kind,
        wireId: writer.info.id,
        destinationIdentity,
        destinationParticipant,
        applicationTopic,
        sent: 0,
        acknowledged: 0,
        pendingAck: null,
        openResolve: null,
        openReject: null,
        openTimer: null,
        closing: false,
        terminal: false,
        pendingOperations: new Set(),
      };
      outgoingStreams.set(id, record);
      roomState.outgoingIds.add(id);
      const info = kind === "text" ? normalizeTextInfo(writer.info) : normalizeByteInfo(writer.info);
      info.topic = applicationTopic;
      const descriptor: StreamDescriptor = { id, info };
      return new Promise<StreamDescriptor>((resolve, reject) => {
        record.openResolve = () => resolve(descriptor);
        record.openReject = reject;
        record.openTimer = setTimeout(() => {
          terminateOutgoing(
            record,
            flowControlError(
              "FlowControlTimeoutError",
              "Data Stream receiver did not acknowledge the stream",
            ),
            true,
          );
        }, FLOW_OPEN_TIMEOUT_MS);
      });
    })();
    operation.then(
      (descriptor) => settle(true, descriptor),
      (error: unknown) => settle(false, error),
    );
  }

  function outgoingRecord(streamId: number, expectedKind?: StreamKind) {
    const record = outgoingStreams.get(streamId);
    if (!record || record.terminal || record.closing) {
      throw new Error("Outgoing data stream is no longer available");
    }
    if (expectedKind && record.kind !== expectedKind) {
      throw new TypeError(`Outgoing data stream does not carry ${expectedKind}`);
    }
    return record;
  }

  function removeOutgoing(record: OutgoingStreamRecord) {
    outgoingStreams.delete(record.id);
    const room = dependencies.getRoom(record.roomId);
    if (room) stateFor(room).outgoingIds.delete(record.id);
  }

  function clearOutgoingTimer(record: OutgoingStreamRecord) {
    if (record.openTimer != null) clearTimeout(record.openTimer);
    record.openTimer = null;
  }

  function settleOutgoingOperations(record: OutgoingStreamRecord, error: unknown = terminalError()) {
    for (const settle of [...record.pendingOperations]) settle(false, error);
  }

  function releaseFailedSdkWriter(record: OutgoingStreamRecord) {
    try {
      record.writer.defaultWriter?.releaseLock();
    } catch {}
    try {
      const onClose = record.writer.onClose;
      record.writer.onClose = undefined;
      onClose?.();
    } catch {}
  }

  function terminateOutgoing(
    record: OutgoingStreamRecord,
    error: unknown = terminalError(),
    notifyRemote = false,
  ) {
    if (record.terminal) return;
    record.terminal = true;
    clearOutgoingTimer(record);
    const rejectOpen = record.openReject;
    record.openResolve = null;
    record.openReject = null;
    rejectOpen?.(error);
    settleOutgoingOperations(record, error);
    record.pendingAck = null;
    removeOutgoing(record);
    const room = dependencies.getRoom(record.roomId);
    if (notifyRemote && room) {
      void sendFlowControl(room, record.destinationIdentity, {
        type: "cancel",
        streamId: record.wireId,
      }).catch(() => {});
    }
    if (record.closing) {
      try {
        void Promise.resolve(record.writer.defaultWriter?.abort(error)).catch(() => {});
      } catch {}
      releaseFailedSdkWriter(record);
    } else if (room) {
      void enqueueBoundedMutation(
        room,
        () => record.writer.close(),
        "LiveKit did not release the native Data Stream",
      ).catch(() => releaseFailedSdkWriter(record));
    } else {
      releaseFailedSdkWriter(record);
    }
  }

  function trackOutgoingOperation(record: OutgoingStreamRecord, onComplete: Completion) {
    let callback: Completion | null = onComplete;
    const settle: OperationSettlement = (success, error) => {
      if (!callback) return;
      const current = callback;
      callback = null;
      record.pendingOperations.delete(settle);
      current(success, error);
    };
    record.pendingOperations.add(settle);
    return settle;
  }

  function beginWrite(
    streamId: number,
    expectedKind: StreamKind,
    chunk: StreamChunk,
    onComplete: Completion,
  ) {
    const record = outgoingRecord(streamId, expectedKind);
    const room = roomForStream(record.roomId);
    const settle = trackOutgoingOperation(record, onComplete);
    const expectedAcknowledgement = record.sent + 1;
    record.sent = expectedAcknowledgement;
    const pending: PendingAcknowledgement = {
      expectedAcknowledgement,
      writerDone: false,
      settle,
    };
    record.pendingAck = pending;
    let operation: Promise<void>;
    try {
      operation = enqueueBoundedMutation(
        room,
        () => {
          if (record.terminal) throw terminalError();
          return record.writer.write(chunk);
        },
        "LiveKit did not accept the native Data Stream chunk",
      );
    } catch (error: unknown) {
      terminateOutgoing(record, error, true);
      return;
    }
    operation.then(
      () => {
        pending.writerDone = true;
        if (record.pendingAck !== pending || record.acknowledged < expectedAcknowledgement) return;
        record.pendingAck = null;
        settle(true);
      },
      (error: unknown) => terminateOutgoing(record, error, true),
    );
  }

  function completeOutgoing(streamId: number, onComplete: Completion) {
    const record = outgoingRecord(streamId);
    const room = roomForStream(record.roomId);
    record.closing = true;
    const settle = trackOutgoingOperation(record, onComplete);
    let operation: Promise<void>;
    try {
      operation = enqueueBoundedMutation(
        room,
        () => record.writer.close(),
        "LiveKit did not complete the native Data Stream",
      );
    } catch (error: unknown) {
      terminateOutgoing(record, error, true);
      return;
    }
    operation.then(
      () => {
        clearOutgoingTimer(record);
        record.closing = false;
        record.terminal = true;
        removeOutgoing(record);
        settle(true);
      },
      (error: unknown) => terminateOutgoing(record, error, true),
    );
  }

  function dropOutgoing(streamId: number) {
    const record = outgoingStreams.get(streamId);
    if (record) terminateOutgoing(record, terminalError(), true);
  }

  function registrationFor(room: DataStreamRoomRecord, topic: unknown, kind: StreamKind) {
    for (const id of stateFor(room).handlerIds) {
      const registration = handlers.get(id);
      if (registration?.kind === kind && registration.topic === topic) return registration;
    }
    return null;
  }

  function removeIncoming(record: IncomingStreamRecord) {
    incomingStreams.delete(record.id);
    const room = dependencies.getRoom(record.roomId);
    if (room) stateFor(room).incomingIds.delete(record.id);
  }

  function drainIncoming(record: IncomingStreamRecord) {
    void record.readTail
      .then(async () => {
        try {
          await record.iterator.return?.();
        } catch {}
        try {
          for await (const _chunk of record.reader) {
            // Drain the admitted chunk and trailer so LiveKit releases its stream controller.
          }
        } catch {}
      })
      .catch(() => {})
      .finally(() => removeIncoming(record));
  }

  function cancelIncoming(record: IncomingStreamRecord, notifyRemote = true, force = false) {
    if (record.terminal) {
      if (force) {
        record.abortController.abort(new DOMException("Incoming data stream cancelled", "AbortError"));
        removeIncoming(record);
      }
      return;
    }
    record.terminal = true;
    const room = dependencies.getRoom(record.roomId);
    if (notifyRemote && room) {
      void sendFlowControl(room, record.senderIdentity, {
        type: "cancel",
        streamId: record.wireId,
      }).catch(() => cancelIncoming(record, false, true));
    }
    if (force) {
      record.abortController.abort(new DOMException("Incoming data stream cancelled", "AbortError"));
      removeIncoming(record);
    } else {
      drainIncoming(record);
    }
  }

  function handleIncomingNative(
    room: DataStreamRoomRecord,
    kind: StreamKind,
    nativeReader: unknown,
    participantInfo: { identity?: unknown } | undefined,
  ) {
    if (room.released) return;
    requireMethods(nativeReader as object | null | undefined, "Data Stream reader", ["withAbortSignal"]);
    const reader = nativeReader as NativeReader;
    if (typeof reader?.[Symbol.asyncIterator] !== "function") {
      throw new TypeError("LiveKit Data Stream reader is not async iterable");
    }
    const wireId = reader.info?.id;
    const applicationTopic = reader.info?.attributes?.[FLOW_APPLICATION_TOPIC_ATTRIBUTE];
    const protocol = reader.info?.attributes?.[FLOW_PROTOCOL_ATTRIBUTE];
    const senderIdentity = participantInfo?.identity;
    const senderParticipant =
      typeof senderIdentity === "string"
        ? room.room.remoteParticipants.get(senderIdentity)
        : undefined;
    const registration = registrationFor(room, applicationTopic, kind);
    const roomState = stateFor(room);
    if (
      typeof wireId !== "string" ||
      wireId.length === 0 ||
      protocol !== FLOW_PROTOCOL_VERSION ||
      typeof senderIdentity !== "string" ||
      !senderParticipant ||
      !registration ||
      roomState.incomingIds.size >= MAX_ACTIVE_INCOMING_DATA_STREAMS
    ) {
      if (typeof wireId === "string" && wireId.length > 0 && typeof senderIdentity === "string") {
        void sendFlowControl(room, senderIdentity, {
          type: "cancel",
          streamId: wireId,
        }).catch(() => {});
      }
      return;
    }
    const id = nextIncomingId++;
    const abortController = new AbortController();
    const iterator = reader.withAbortSignal(abortController.signal)[Symbol.asyncIterator]();
    requireMethods(iterator, "Data Stream iterator", ["next"]);
    const record: IncomingStreamRecord = {
      id,
      roomId: room.id,
      kind,
      wireId,
      senderIdentity,
      senderParticipant,
      reader,
      iterator,
      abortController,
      readTail: Promise.resolve(),
      consumed: 0,
      terminal: false,
    };
    incomingStreams.set(id, record);
    roomState.incomingIds.add(id);
    const info =
      kind === "text"
        ? normalizeTextInfo(reader.info, senderIdentity)
        : normalizeByteInfo(reader.info, senderIdentity);
    info.topic = applicationTopic as string;
    registration.callback({ id, info });
    void sendFlowControl(room, senderIdentity, {
      type: "ack",
      streamId: wireId,
      consumed: 0,
    }).catch(() => cancelIncoming(record, false));
  }

  function registerHandler(
    roomId: number,
    topic: string,
    kind: StreamKind,
    callback: HandlerRegistration["callback"],
  ) {
    const room = roomForStream(roomId);
    if (registrationFor(room, topic, kind)) {
      throw flowControlError(
        "HandlerAlreadyRegisteredError",
        `A ${kind} stream handler already exists for topic ${topic}`,
      );
    }
    const id = nextHandlerId++;
    const roomState = stateFor(room);
    if (kind === "text" && !roomState.textHandlerInstalled) {
      room.room.registerTextStreamHandler(FLOW_TEXT_TOPIC, (reader, participantInfo) =>
        handleIncomingNative(room, "text", reader, participantInfo),
      );
      roomState.textHandlerInstalled = true;
    } else if (kind === "bytes" && !roomState.byteHandlerInstalled) {
      room.room.registerByteStreamHandler(FLOW_BYTES_TOPIC, (reader, participantInfo) =>
        handleIncomingNative(room, "bytes", reader, participantInfo),
      );
      roomState.byteHandlerInstalled = true;
    }
    handlers.set(id, { id, roomId, topic, kind, callback });
    roomState.handlerIds.add(id);
    return id;
  }

  function dropHandler(registrationId: number) {
    const registration = handlers.get(registrationId);
    if (!registration) return;
    handlers.delete(registrationId);
    const room = dependencies.getRoom(registration.roomId);
    if (!room) return;
    const roomState = stateFor(room);
    roomState.handlerIds.delete(registrationId);
    const hasKind = [...roomState.handlerIds].some(
      (id) => handlers.get(id)?.kind === registration.kind,
    );
    if (!hasKind && registration.kind === "text" && roomState.textHandlerInstalled) {
      room.room.unregisterTextStreamHandler(FLOW_TEXT_TOPIC);
      roomState.textHandlerInstalled = false;
    } else if (!hasKind && registration.kind === "bytes" && roomState.byteHandlerInstalled) {
      room.room.unregisterByteStreamHandler(FLOW_BYTES_TOPIC);
      roomState.byteHandlerInstalled = false;
    }
  }

  function incomingRecord(streamId: number, expectedKind: StreamKind) {
    const record = incomingStreams.get(streamId);
    if (!record || record.terminal) throw new Error("Incoming data stream is no longer available");
    if (record.kind !== expectedKind) {
      throw new TypeError(`Incoming data stream does not carry ${expectedKind}`);
    }
    return record;
  }

  function readIncoming(streamId: number, expectedKind: StreamKind) {
    const record = incomingRecord(streamId, expectedKind);
    const operation = record.readTail.then(() => record.iterator.next());
    record.readTail = operation.catch(() => {});
    return operation.then(async (result) => {
      if (record.terminal) return { done: true, value: null };
      if (result.done) finishIncoming(streamId);
      let value: string | number[] | null = null;
      if (!result.done && expectedKind === "text") {
        value = typeof result.value === "string" ? result.value : "";
      } else if (!result.done) {
        value = Array.from(result.value as ArrayLike<number>);
      }
      if (!result.done) {
        record.consumed += 1;
        const room = roomForStream(record.roomId);
        await sendFlowControl(room, record.senderIdentity, {
          type: "ack",
          streamId: record.wireId,
          consumed: record.consumed,
        });
      }
      return { done: result.done === true, value };
    });
  }

  function finishIncoming(streamId: number) {
    const record = incomingStreams.get(streamId);
    if (!record) return;
    record.terminal = true;
    removeIncoming(record);
  }

  function outgoingByWireId(room: DataStreamRoomRecord, wireId: string) {
    for (const id of stateFor(room).outgoingIds) {
      const record = outgoingStreams.get(id);
      if (record?.wireId === wireId) return record;
    }
    return null;
  }

  function incomingByWireId(room: DataStreamRoomRecord, wireId: string) {
    for (const id of stateFor(room).incomingIds) {
      const record = incomingStreams.get(id);
      if (record?.wireId === wireId) return record;
    }
    return null;
  }

  function handleFlowControl(
    room: DataStreamRoomRecord,
    payload: Uint8Array,
    participant: RemoteParticipant | undefined,
    reliable: boolean,
    topic: string | undefined,
  ) {
    if (topic !== FLOW_CONTROL_TOPIC) return false;
    if (!reliable || !participant) return true;
    const message = parseFlowControl(payload);
    if (!message) return true;
    if (message.type === "ack") {
      const record = outgoingByWireId(room, message.streamId);
      if (
        !record ||
        record.destinationParticipant !== participant ||
        room.room.remoteParticipants.get(record.destinationIdentity) !== participant
      ) {
        return true;
      }
      const consumed = message.consumed;
      if (
        !Number.isSafeInteger(consumed) ||
        (consumed as number) < 0 ||
        (consumed as number) > record.sent ||
        (consumed as number) < record.acknowledged
      ) {
        terminateOutgoing(
          record,
          flowControlError("FlowControlProtocolError", "Data Stream acknowledgement is invalid"),
          true,
        );
        return true;
      }
      record.acknowledged = consumed as number;
      if (consumed === 0 && record.openResolve) {
        clearOutgoingTimer(record);
        const resolve = record.openResolve;
        record.openResolve = null;
        record.openReject = null;
        resolve();
      }
      const pending = record.pendingAck;
      if (pending && pending.writerDone && record.acknowledged >= pending.expectedAcknowledgement) {
        record.pendingAck = null;
        pending.settle(true);
      }
      return true;
    }
    if (message.type === "cancel") {
      const outgoing = outgoingByWireId(room, message.streamId);
      if (outgoing?.destinationParticipant === participant) {
        terminateOutgoing(
          outgoing,
          flowControlError("RemoteCancelledError", "Data Stream receiver cancelled the stream"),
          false,
        );
      }
      const incoming = incomingByWireId(room, message.streamId);
      if (incoming?.senderParticipant === participant) cancelIncoming(incoming, false);
    }
    return true;
  }

  function participantDisconnected(room: DataStreamRoomRecord, participant: RemoteParticipant) {
    const roomState = stateFor(room);
    for (const id of [...roomState.outgoingIds]) {
      const stream = outgoingStreams.get(id);
      if (stream?.destinationParticipant === participant) {
        terminateOutgoing(
          stream,
          flowControlError("DestinationUnavailableError", "Data Stream destination disconnected"),
          false,
        );
      }
    }
    for (const id of [...roomState.incomingIds]) {
      const stream = incomingStreams.get(id);
      if (stream?.senderParticipant === participant) cancelIncoming(stream, false, true);
    }
  }

  function releaseRoom(room: DataStreamRoomRecord) {
    const roomState = stateFor(room);
    for (const settle of [...roomState.pendingOpens]) settle(false, terminalError());
    for (const id of [...roomState.handlerIds]) dropHandler(id);
    for (const id of [...roomState.outgoingIds]) dropOutgoing(id);
    for (const id of [...roomState.incomingIds]) {
      const stream = incomingStreams.get(id);
      if (stream) cancelIncoming(stream, true, true);
    }
    roomStates.delete(room);
  }

  return {
    openTextStream: (roomId: number, options: DataStreamOptions, onComplete: OpenCompletion) =>
      beginOutgoing(roomId, options, "text", onComplete),
    openByteStream: (roomId: number, options: DataStreamOptions, onComplete: OpenCompletion) =>
      beginOutgoing(roomId, options, "bytes", onComplete),
    writeTextStream: (streamId: number, chunk: string, onComplete: Completion) =>
      beginWrite(streamId, "text", chunk, onComplete),
    writeByteStream: (streamId: number, chunk: ArrayLike<number>, onComplete: Completion) =>
      beginWrite(streamId, "bytes", Uint8Array.from(chunk), onComplete),
    completeOutgoingDataStream: completeOutgoing,
    dropOutgoingDataStream: dropOutgoing,
    dropOutgoingDataStreamDescriptor: (descriptor: { id?: unknown } | null | undefined) => {
      if (Number.isInteger(descriptor?.id)) dropOutgoing(descriptor!.id as number);
    },
    registerTextStreamHandler: (
      roomId: number,
      topic: string,
      callback: HandlerRegistration["callback"],
    ) => registerHandler(roomId, topic, "text", callback),
    registerByteStreamHandler: (
      roomId: number,
      topic: string,
      callback: HandlerRegistration["callback"],
    ) => registerHandler(roomId, topic, "bytes", callback),
    dropDataStreamHandler: dropHandler,
    readTextStream: (streamId: number) => readIncoming(streamId, "text"),
    readByteStream: (streamId: number) => readIncoming(streamId, "bytes"),
    finishIncomingDataStream: finishIncoming,
    dropIncomingDataStream: (streamId: number) => {
      const record = incomingStreams.get(streamId);
      if (record) cancelIncoming(record);
    },
    dropIncomingDataStreamDescriptor: (descriptor: { id?: unknown } | null | undefined) => {
      if (Number.isInteger(descriptor?.id)) {
        const record = incomingStreams.get(descriptor!.id as number);
        if (record) cancelIncoming(record);
      }
    },
    handleFlowControl,
    participantDisconnected,
    releaseRoom,
  };
}
