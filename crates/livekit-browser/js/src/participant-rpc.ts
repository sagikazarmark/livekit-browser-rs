import type { LocalParticipant, Room, RpcInvocationData } from "livekit-client";

type RpcSettlement = (success: boolean, value: unknown) => void;

export interface ParticipantRpcRoomRecord {
  id: number;
  room: Room;
  released: boolean;
}

interface Dependencies {
  getRoom(roomId: number): ParticipantRpcRoomRecord | undefined;
  requireDataPublishing(participant: LocalParticipant): void;
}

interface RpcOptions {
  destinationIdentity: string;
  method: string;
  payload: string;
  responseTimeoutMs: number;
}

interface RpcFailureSource {
  status?: unknown;
  name?: unknown;
  code?: unknown;
  message?: unknown;
  data?: unknown;
}

interface MethodRegistration {
  id: number;
  roomId: number;
  method: string;
}

interface InvocationRecord {
  id: number;
  roomId: number;
  resolve(response: string): void;
  reject(error: unknown): void;
}

interface RoomState {
  methodIds: Set<number>;
  invocationIds: Set<number>;
  pendingCalls: Set<RpcSettlement>;
}

export function createParticipantRpcSubsystem(dependencies: Dependencies) {
  let nextMethodId = 1;
  let nextInvocationId = 1;
  const methods = new Map<number, MethodRegistration>();
  const invocations = new Map<number, InvocationRecord>();
  const roomStates = new WeakMap<ParticipantRpcRoomRecord, RoomState>();

  function stateFor(room: ParticipantRpcRoomRecord) {
    let state = roomStates.get(room);
    if (!state) {
      state = { methodIds: new Set(), invocationIds: new Set(), pendingCalls: new Set() };
      roomStates.set(room, state);
    }
    return state;
  }

  function availableRoom(roomId: number) {
    const room = dependencies.getRoom(roomId);
    if (!room || room.released) throw new Error("Room Session is no longer available");
    return room;
  }

  function normalizeFailure(error: unknown) {
    const source =
      error && (typeof error === "object" || typeof error === "function")
        ? (error as RpcFailureSource)
        : {};
    return {
      terminal: false,
      permission_denied:
        source.status === 403 ||
        source.name === "NotAllowedError" ||
        source.name === "PermissionDeniedError" ||
        source.name === "UnauthorizedError",
      code:
        Number.isInteger(source.code) && (source.code as number) >= 0
          ? (source.code as number)
          : null,
      message:
        typeof source.message === "string"
          ? source.message
          : typeof error === "string"
            ? error
            : "unknown Participant RPC failure",
      data: typeof source.data === "string" ? source.data : null,
      cause: error,
    };
  }

  function invoke(roomId: number, options: RpcOptions, onComplete: RpcSettlement) {
    let room: ParticipantRpcRoomRecord | undefined;
    let callback: RpcSettlement | null = onComplete;
    const settle: RpcSettlement = (success, value) => {
      if (!callback) return;
      const current = callback;
      callback = null;
      if (room) stateFor(room).pendingCalls.delete(settle);
      current(success, value);
    };
    try {
      room = availableRoom(roomId);
      const participant = room.room.localParticipant;
      dependencies.requireDataPublishing(participant);
      const invocation = participant.performRpc({
        destinationIdentity: options.destinationIdentity,
        method: options.method,
        payload: options.payload,
        responseTimeout: options.responseTimeoutMs,
      });
      stateFor(room).pendingCalls.add(settle);
      Promise.resolve(invocation).then(
        (response) => settle(true, response),
        (error: unknown) => settle(false, normalizeFailure(error)),
      );
    } catch (error: unknown) {
      settle(false, normalizeFailure(error));
    }
  }

  function registerMethod(
    roomId: number,
    method: string,
    callback: (invocation: {
      id: number;
      request_id: string;
      caller_identity: string;
      payload: string;
      response_timeout_ms: number;
    }) => void,
  ) {
    const room = availableRoom(roomId);
    const id = nextMethodId++;
    room.room.registerRpcMethod(method, (invocation: RpcInvocationData) => {
      return new Promise<string>((resolve, reject) => {
        if (room.released || !methods.has(id)) {
          reject(
            new globalThis.LivekitClient.RpcError(
              1400,
              "Participant RPC method is no longer registered",
            ),
          );
          return;
        }
        const invocationId = nextInvocationId++;
        invocations.set(invocationId, { id: invocationId, roomId, resolve, reject });
        stateFor(room).invocationIds.add(invocationId);
        try {
          callback({
            id: invocationId,
            request_id: invocation.requestId,
            caller_identity: invocation.callerIdentity,
            payload: invocation.payload,
            response_timeout_ms: invocation.responseTimeout,
          });
        } catch {
          invocations.delete(invocationId);
          stateFor(room).invocationIds.delete(invocationId);
          reject(new globalThis.LivekitClient.RpcError(1500, "Participant RPC handler failed"));
        }
      });
    });
    methods.set(id, { id, roomId, method });
    stateFor(room).methodIds.add(id);
    return id;
  }

  function dropMethod(registrationId: number) {
    const registration = methods.get(registrationId);
    if (!registration) return;
    methods.delete(registrationId);
    const room = dependencies.getRoom(registration.roomId);
    if (room) {
      stateFor(room).methodIds.delete(registrationId);
      room.room.unregisterRpcMethod(registration.method);
    }
  }

  function completeInvocation(
    invocationId: number,
    completion:
      | { success?: unknown; payload?: unknown; code?: unknown; message?: unknown; data?: unknown }
      | null
      | undefined,
  ) {
    const invocation = invocations.get(invocationId);
    if (!invocation) return;
    invocations.delete(invocationId);
    const room = dependencies.getRoom(invocation.roomId);
    if (room) stateFor(room).invocationIds.delete(invocationId);
    if (completion?.success === true) {
      invocation.resolve(typeof completion.payload === "string" ? completion.payload : "");
    } else {
      invocation.reject(
        new globalThis.LivekitClient.RpcError(
          Number.isInteger(completion?.code) ? (completion!.code as number) : 1500,
          typeof completion?.message === "string"
            ? completion.message
            : "Participant RPC handler failed",
          typeof completion?.data === "string" ? completion.data : undefined,
        ),
      );
    }
  }

  function dropInvocation(invocationId: number) {
    const invocation = invocations.get(invocationId);
    if (!invocation) return;
    invocations.delete(invocationId);
    const room = dependencies.getRoom(invocation.roomId);
    if (room) stateFor(room).invocationIds.delete(invocationId);
    invocation.reject(new globalThis.LivekitClient.RpcError(1503, "Room Session left"));
  }

  function releaseRoom(room: ParticipantRpcRoomRecord) {
    const state = stateFor(room);
    for (const settle of [...state.pendingCalls]) {
      settle(false, {
        terminal: true,
        permission_denied: false,
        code: null,
        message: "Room Session is terminal",
        data: null,
      });
    }
    for (const id of [...state.methodIds]) dropMethod(id);
    for (const id of [...state.invocationIds]) dropInvocation(id);
    roomStates.delete(room);
  }

  return {
    invokeParticipantRpc: invoke,
    registerParticipantRpcMethod: registerMethod,
    dropParticipantRpcMethod: dropMethod,
    completeParticipantRpcInvocation: completeInvocation,
    dropParticipantRpcInvocation: dropInvocation,
    dropParticipantRpcInvocationDescriptor: (
      descriptor: { id?: unknown } | null | undefined,
    ) => {
      if (Number.isInteger(descriptor?.id)) dropInvocation(descriptor!.id as number);
    },
    releaseRoom,
  };
}
