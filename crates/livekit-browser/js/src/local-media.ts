import { requireMethods } from "./runtime";
import {
  cameraCaptureOptions,
  cloneCaptureOptions,
  livekitTrackSource,
  microphoneCaptureOptions,
  rawOptions,
  screenCaptureOptions,
} from "./options";
import {
  normalizeSenderStatistics,
  normalizeVideoSenderStatistics,
} from "./statistics";
import type { AudioAnalyserOwner } from "./audio-analysers";
import type {
  AudioSenderStats,
  LocalAudioTrack,
  LocalParticipant,
  LocalTrackPublication,
  LocalVideoTrack,
  Room,
  TrackPublication,
  VideoSenderStats,
} from "livekit-client";
import type { RawOptions } from "./options";

type RoomId = number;
type LocalTrackId = number;
type PublicationId = number;
type AttachmentId = number;
type MediaKind = "audio" | "video";
type LocalTrackSource = "camera" | "microphone" | "screenShare" | "screenShareAudio";
type Completion = (success: boolean, value?: unknown) => void;
type TerminalCallback = () => void;
type PublicationReplacementCallback = (
  publicationId: PublicationId | null,
  sid: string | null,
  muted: boolean,
) => PublicationId;
type LocalMediaTrack = LocalAudioTrack | LocalVideoTrack;
type MediaElement = HTMLAudioElement | HTMLVideoElement;

interface LocalMediaRoom {
  readonly id: RoomId;
  readonly room: Room;
  readonly released: boolean;
}

interface AudioAnalyserSubsystem {
  createAudioAnalyserOwner(): AudioAnalyserOwner;
  createAudioAnalyser(
    owner: AudioAnalyserOwner,
    sourceTrack: MediaStreamTrack,
    isCurrent: () => boolean,
    onTerminal: TerminalCallback,
  ): number;
  mediaStreamAudioSource(track: { mediaStreamTrack?: MediaStreamTrack }): MediaStreamTrack;
  terminateAudioAnalyserOwner(owner: AudioAnalyserOwner): void;
}

export interface LocalMediaDependencies {
  getRoom(roomId: RoomId): LocalMediaRoom | undefined;
  allocateAttachmentId(): AttachmentId;
  audioAnalysers: AudioAnalyserSubsystem;
}

interface PublicationReplacementState {
  roomId: RoomId;
  publicationId: PublicationId | null;
  callback: PublicationReplacementCallback;
}

interface LocalTrackRecord<TTrack extends LocalMediaTrack, TKind extends MediaKind> {
  id: LocalTrackId;
  track: TTrack;
  mediaKind: TKind;
  source: LocalTrackSource;
  captureOptions: RawOptions;
  captureStopped: boolean;
  stopRequested: boolean;
  stopOperation: Promise<unknown> | null;
  ownerDropped: boolean;
  attachments: Map<AttachmentId, MediaElement>;
  audioAnalyserOwner?: AudioAnalyserOwner;
  publishedRoomId: RoomId | null;
  pendingPublishRoomId: RoomId | null;
  publicationId: PublicationId | null;
  pendingPublicationId: PublicationId | null;
  publication: LocalTrackPublication | null;
  publicationReplacementCallback: PublicationReplacementCallback | null;
  awaitingPublicationReplacement: PublicationReplacementState | null;
  explicitUnpublishId: PublicationId | null;
  explicitUnpublishObservedId: PublicationId | null;
  mutationTail: Promise<unknown>;
  ended: boolean;
  endedCallback: TerminalCallback | null;
  endedListener: TerminalCallback | null;
  browserEndedPeer: AnyLocalTrackRecord | null;
  endedCleanupComplete: boolean;
  endedCleanupRunning: boolean;
}

type LocalVideoTrackRecord = LocalTrackRecord<LocalVideoTrack, "video">;
type LocalAudioTrackRecord = LocalTrackRecord<LocalAudioTrack, "audio">;
type AnyLocalTrackRecord = LocalVideoTrackRecord | LocalAudioTrackRecord;
type LocalTrackRegistry = Map<LocalTrackId, AnyLocalTrackRecord>;

interface AudioPublicationInput {
  audioCodec?: unknown;
  name?: string | null;
  dtx?: boolean | null;
  red?: boolean | null;
}

interface VideoPublicationInput {
  name?: string | null;
  videoCodec?: string | null;
  simulcast?: boolean | null;
  videoEncoding?: { maxBitrate?: number; maxFramerate?: number } | null;
}

export function createLocalMediaSubsystem(dependencies: LocalMediaDependencies) {
  const { getRoom, allocateAttachmentId, audioAnalysers } = dependencies;
  let nextLocalVideoTrackId: LocalTrackId = 1;
  let nextLocalAudioTrackId: LocalTrackId = 1;
  const localVideoTracks = new Map<LocalTrackId, LocalVideoTrackRecord>();
  const localAudioTracks = new Map<LocalTrackId, LocalAudioTrackRecord>();

  function localVideoTrackRecord(trackId: LocalTrackId): LocalVideoTrackRecord {
    const record = localVideoTracks.get(trackId);
    if (!record) throw new Error("Local Video Track is no longer available");
    return record;
  }

  function localAudioTrackRecord(trackId: LocalTrackId): LocalAudioTrackRecord {
    const record = localAudioTracks.get(trackId);
    if (!record) throw new Error("Local Audio Track is no longer available");
    return record;
  }

  function localMediaTrackResources(kind: MediaKind) {
    if (kind === "video") return { records: localVideoTracks, recordFor: localVideoTrackRecord };
    if (kind === "audio") return { records: localAudioTracks, recordFor: localAudioTrackRecord };
    throw new TypeError(`unsupported Local Media Track kind: ${kind}`);
  }

  function localMediaTrackRecord(trackId: LocalTrackId, kind: MediaKind) {
    return localMediaTrackResources(kind).recordFor(trackId);
  }

  function enqueueLocalTrackMutation<T>(
    record: AnyLocalTrackRecord,
    operation: () => T | PromiseLike<T>,
  ): Promise<T> {
    const result = record.mutationTail.then(operation, operation);
    record.mutationTail = result.catch(() => {});
    return result;
  }

  function registerLocalTrack<TTrack extends LocalMediaTrack, TKind extends MediaKind>(
    id: LocalTrackId,
    track: TTrack,
    mediaKind: TKind,
    captureOptions: RawOptions,
    source: LocalTrackSource,
  ): LocalTrackRecord<TTrack, TKind> {
    return {
      id,
      track,
      mediaKind,
      source,
      captureOptions,
      captureStopped: false,
      stopRequested: false,
      stopOperation: null,
      ownerDropped: false,
      attachments: new Map(),
      audioAnalyserOwner:
        mediaKind === "audio" ? audioAnalysers.createAudioAnalyserOwner() : undefined,
      publishedRoomId: null,
      pendingPublishRoomId: null,
      publicationId: null,
      pendingPublicationId: null,
      publication: null,
      publicationReplacementCallback: null,
      awaitingPublicationReplacement: null,
      explicitUnpublishId: null,
      explicitUnpublishObservedId: null,
      mutationTail: Promise.resolve(),
      ended: false,
      endedCallback: null,
      endedListener: null,
      browserEndedPeer: null,
      endedCleanupComplete: false,
      endedCleanupRunning: false,
    };
  }

  function registerLocalVideoTrack(
    track: LocalVideoTrack,
    captureOptions: RawOptions,
    source: LocalTrackSource,
  ) {
    const id = nextLocalVideoTrackId++;
    localVideoTracks.set(id, registerLocalTrack(id, track, "video", captureOptions, source));
    return id;
  }

  function registerLocalAudioTrack(
    track: LocalAudioTrack,
    captureOptions: RawOptions,
    source: LocalTrackSource,
  ) {
    const id = nextLocalAudioTrackId++;
    localAudioTracks.set(id, registerLocalTrack(id, track, "audio", captureOptions, source));
    return id;
  }

  function removeLocalTrackEndedListener(record: AnyLocalTrackRecord) {
    if (record.endedListener) {
      record.track.off(globalThis.LivekitClient.TrackEvent.Ended, record.endedListener);
      record.endedListener = null;
    }
    record.endedCallback = null;
    record.browserEndedPeer = null;
  }

  function detachLocalElement(record: AnyLocalTrackRecord, attachmentId: AttachmentId) {
    const element = record.attachments.get(attachmentId);
    if (!element) return;
    const clearIfStillDetached = () => {
      if (![...record.attachments.values()].includes(element)) element.srcObject = null;
    };
    try {
      record.track.detach(element);
    } finally {
      record.attachments.delete(attachmentId);
      clearIfStillDetached();
      queueMicrotask(clearIfStillDetached);
      setTimeout(clearIfStillDetached, 0);
    }
  }

  function stopLocalRecord(record: AnyLocalTrackRecord) {
    let failure: unknown = null;
    if (record.audioAnalyserOwner) {
      audioAnalysers.terminateAudioAnalyserOwner(record.audioAnalyserOwner);
    }
    for (const attachmentId of [...record.attachments.keys()]) {
      try {
        detachLocalElement(record, attachmentId);
      } catch (error: unknown) {
        failure ??= error;
      }
    }
    if (!record.captureStopped) {
      try {
        record.track.stop();
        record.captureStopped = true;
      } catch (error: unknown) {
        failure ??= error;
      }
    }
    if (failure) throw failure;
  }

  function deleteFinishedBrowserEndedRecord(record: AnyLocalTrackRecord) {
    if (
      !record.ownerDropped ||
      !record.endedCleanupComplete ||
      record.publication ||
      record.publishedRoomId != null ||
      record.pendingPublishRoomId != null ||
      record.attachments.size > 0
    ) {
      return;
    }
    const records = record.mediaKind === "video" ? localVideoTracks : localAudioTracks;
    records.delete(record.id);
  }

  function finishBrowserEndedCleanup(record: AnyLocalTrackRecord, retryDelay = 0) {
    if (record.endedCleanupRunning || record.endedCleanupComplete) return;
    record.endedCleanupRunning = true;
    const operation = enqueueLocalTrackMutation(record, async () => {
      let failure: unknown = null;
      const room = record.publishedRoomId == null ? undefined : getRoom(record.publishedRoomId);
      if (record.publication && room && !room.released) {
        try {
          await room.room.localParticipant.unpublishTrack(record.track, false);
          record.publication = null;
          record.publishedRoomId = null;
          record.publicationId = null;
        } catch (error: unknown) {
          failure = error;
        }
      } else if (!room || room.released) {
        record.publication = null;
        record.publishedRoomId = null;
        record.publicationId = null;
      }
      try {
        stopLocalRecord(record);
      } catch (error: unknown) {
        failure ??= error;
      }
      if (failure) throw failure;
    });
    record.stopOperation = operation;
    void operation.then(
      () => {
        if (record.stopOperation === operation) record.stopOperation = null;
        record.endedCleanupRunning = false;
        record.endedCleanupComplete = true;
        deleteFinishedBrowserEndedRecord(record);
      },
      () => {
        if (record.stopOperation === operation) record.stopOperation = null;
        record.endedCleanupRunning = false;
        const nextDelay = Math.min(Math.max(retryDelay * 2, 10), 1_000);
        setTimeout(() => finishBrowserEndedCleanup(record, nextDelay), nextDelay);
      },
    );
  }

  function finishBrowserEnded(record: AnyLocalTrackRecord, sourceEnded: boolean) {
    if (record.ended) return;
    record.ended = true;
    record.stopRequested = true;
    record.captureStopped = sourceEnded;
    if (record.audioAnalyserOwner) {
      audioAnalysers.terminateAudioAnalyserOwner(record.audioAnalyserOwner);
    }
    for (const attachmentId of [...record.attachments.keys()]) {
      try {
        detachLocalElement(record, attachmentId);
      } catch {}
    }
    if (record.endedListener) {
      record.track.off(globalThis.LivekitClient.TrackEvent.Ended, record.endedListener);
      record.endedListener = null;
    }
    const callback = record.endedCallback;
    record.endedCallback = null;
    const peer = record.browserEndedPeer;
    record.browserEndedPeer = null;
    try {
      callback?.();
    } finally {
      try {
        if (peer) finishBrowserEnded(peer, false);
      } finally {
        finishBrowserEndedCleanup(record);
      }
    }
  }

  function installLocalTrackEndedListener(
    record: AnyLocalTrackRecord,
    onEnded: TerminalCallback | null,
  ) {
    const listener = () => finishBrowserEnded(record, true);
    record.endedListener = listener;
    record.track.on(globalThis.LivekitClient.TrackEvent.Ended, listener);
    if (onEnded) record.endedCallback = onEnded;
  }

  function observeLocalVideoTrackEnded(trackId: LocalTrackId, onEnded: TerminalCallback) {
    const record = localVideoTrackRecord(trackId);
    if (record.ended) queueMicrotask(onEnded);
    else record.endedCallback = onEnded;
  }

  function observeLocalAudioTrackEnded(trackId: LocalTrackId, onEnded: TerminalCallback) {
    const record = localAudioTrackRecord(trackId);
    if (record.ended) queueMicrotask(onEnded);
    else record.endedCallback = onEnded;
  }

  function createLocalClipMediaStream(
    audioTrackIds: LocalTrackId[],
    videoTrackIds: LocalTrackId[],
  ) {
    const source = (
      records: ReadonlyMap<LocalTrackId, AnyLocalTrackRecord>,
      trackId: LocalTrackId,
      expectedKind: MediaKind,
    ) => {
      const record = records.get(trackId);
      if (!record || record.mediaKind !== expectedKind) {
        throw new Error(`Local ${expectedKind} Track is no longer available`);
      }
      if (record.stopRequested || record.captureStopped || record.ended) {
        throw new Error(`Local ${expectedKind} Track has stopped`);
      }
      const mediaStreamTrack = record.track?.mediaStreamTrack;
      if (!(mediaStreamTrack instanceof MediaStreamTrack) || mediaStreamTrack.readyState !== "live") {
        throw new TypeError(`Local ${expectedKind} Track has no live browser source`);
      }
      return mediaStreamTrack;
    };
    return new MediaStream([
      ...audioTrackIds.map((trackId) => source(localAudioTracks, trackId, "audio")),
      ...videoTrackIds.map((trackId) => source(localVideoTracks, trackId, "video")),
    ]);
  }

  async function restartVideoTrack(record: LocalVideoTrackRecord, options: RawOptions) {
    const restartOptions = cloneCaptureOptions(options);
    delete restartOptions.resolution;
    delete restartOptions.frameRate;
    await record.track.restartTrack(restartOptions);
    const constraints: MediaTrackConstraints = {};
    if (options.resolution?.width != null) constraints.width = { ideal: options.resolution.width };
    if (options.resolution?.height != null) constraints.height = { ideal: options.resolution.height };
    if (options.frameRate != null) constraints.frameRate = { ideal: options.frameRate };
    if (Object.keys(constraints).length > 0) {
      await record.track.mediaStreamTrack.applyConstraints(constraints);
    }
    (record.track as LocalVideoTrack & { _constraints: MediaTrackConstraints })._constraints = {
      ...(record.track as LocalVideoTrack & { _constraints: MediaTrackConstraints })._constraints,
      ...constraints,
    };
  }

  function restartLocalTrack<TRecord extends AnyLocalTrackRecord>({
    record,
    nextOptions,
    normalizeOptions,
    performRestart,
    terminate,
    onComplete,
  }: {
    record: TRecord;
    nextOptions: (previous: RawOptions) => RawOptions;
    normalizeOptions: (options: RawOptions) => RawOptions;
    performRestart: (record: TRecord, options: RawOptions) => Promise<void>;
    terminate: (record: TRecord) => void;
    onComplete: (terminal: boolean) => void;
  }) {
    let becameTerminal = false;
    let operation: Promise<unknown>;
    try {
      if (typeof record.track.restartTrack !== "function") {
        throw new TypeError("LiveKit Local Track does not support restart");
      }
      operation = enqueueLocalTrackMutation(record, async () => {
        if (record.stopRequested) throw new Error("Local Media Track capture has stopped");
        const previousOptions = cloneCaptureOptions(record.captureOptions);
        const requestedOptions = normalizeOptions(nextOptions(previousOptions));
        const previousSource = record.mediaKind === "audio" ? record.track.mediaStreamTrack : null;
        try {
          await performRestart(record, cloneCaptureOptions(requestedOptions));
          record.captureOptions = requestedOptions;
        } catch (error: unknown) {
          try {
            await performRestart(record, cloneCaptureOptions(previousOptions));
          } catch {
            becameTerminal = true;
            terminate(record);
          }
          throw error;
        } finally {
          if (previousSource && record.track.mediaStreamTrack !== previousSource) {
            audioAnalysers.terminateAudioAnalyserOwner(record.audioAnalyserOwner!);
          }
        }
      });
    } catch (error: unknown) {
      operation = Promise.reject(error);
    }
    return operation.then(
      (value) => {
        onComplete(false);
        return value;
      },
      (error: unknown) => {
        onComplete(becameTerminal);
        throw error;
      },
    );
  }

  function rawLocalMediaTrack(trackId: LocalTrackId, kind: MediaKind) {
    const record = localMediaTrackRecord(trackId, kind);
    if (record.stopRequested || record.captureStopped || record.ended) {
      throw new Error("Local Media Track is no longer available");
    }
    return record.track;
  }

  function rawLocalPublication(trackId: LocalTrackId, publicationId: PublicationId, kind: MediaKind) {
    const record = localMediaTrackRecord(trackId, kind);
    if (record.publicationId !== publicationId || !record.publication) {
      throw new Error("Local Track Publication is no longer available");
    }
    return record.publication;
  }

  function setLocalMediaTrackProcessor(
    trackId: LocalTrackId,
    kind: MediaKind,
    processor: any,
    showProcessedStreamLocally: boolean,
  ) {
    const record = localMediaTrackRecord(trackId, kind);
    if (record.stopRequested || record.captureStopped || record.ended) {
      return Promise.reject(new Error("Local Media Track is no longer available"));
    }
    if (typeof record.track.setProcessor !== "function") {
      return Promise.reject(new TypeError("LiveKit Local Media Track does not support processors"));
    }
    return enqueueLocalTrackMutation(record, async () => {
      if (record.stopRequested || record.captureStopped || record.ended) {
        throw new Error("Local Media Track is no longer available");
      }
      await record.track.setProcessor(processor, showProcessedStreamLocally);
      if (record.audioAnalyserOwner) {
        audioAnalysers.terminateAudioAnalyserOwner(record.audioAnalyserOwner);
      }
    });
  }

  function rawLocalMediaTrackProcessor(trackId: LocalTrackId, kind: MediaKind) {
    const record = localMediaTrackRecord(trackId, kind);
    if (record.stopRequested || record.captureStopped || record.ended) {
      throw new Error("Local Media Track is no longer available");
    }
    if (typeof record.track.getProcessor !== "function") {
      throw new TypeError("LiveKit Local Media Track does not support processors");
    }
    return record.track.getProcessor() ?? null;
  }

  function stopLocalMediaTrackProcessor(trackId: LocalTrackId, kind: MediaKind) {
    const record = localMediaTrackRecord(trackId, kind);
    if (typeof record.track.stopProcessor !== "function") {
      return Promise.reject(new TypeError("LiveKit Local Media Track does not support processors"));
    }
    return enqueueLocalTrackMutation(record, async () => {
      if (record.stopRequested || record.captureStopped || record.ended) {
        throw new Error("Local Media Track is no longer available");
      }
      await record.track.stopProcessor(false);
      if (record.audioAnalyserOwner) {
        audioAnalysers.terminateAudioAnalyserOwner(record.audioAnalyserOwner);
      }
    });
  }

  function createLocalAudioAnalyser(trackId: LocalTrackId, onTerminal: TerminalCallback) {
    const record = localAudioTrackRecord(trackId);
    if (record.stopRequested) throw new Error("Local Audio Track capture has stopped");
    const sourceTrack = audioAnalysers.mediaStreamAudioSource(record.track);
    return audioAnalysers.createAudioAnalyser(
      record.audioAnalyserOwner!,
      sourceTrack,
      () =>
        localAudioTracks.get(record.id) === record &&
        !record.stopRequested &&
        record.track.mediaStreamTrack === sourceTrack,
      onTerminal,
    );
  }

  function queueLocalStop(record: AnyLocalTrackRecord) {
    record.stopRequested = true;
    removeLocalTrackEndedListener(record);
    if (record.stopOperation) return record.stopOperation;
    const operation = enqueueLocalTrackMutation(record, () => stopLocalRecord(record));
    record.stopOperation = operation;
    void operation.catch(() => {
      if (record.stopOperation === operation) record.stopOperation = null;
    });
    return operation;
  }

  function terminateAfterRestartFailure(record: AnyLocalTrackRecord) {
    record.stopRequested = true;
    try {
      stopLocalRecord(record);
    } catch {
      record.captureStopped = true;
    }
  }

  function requireLocalTrackInstance(track: LocalMediaTrack, name: string) {
    requireMethods(track, name, [
      "on", "off", "attach", "detach", "stop", "getDeviceId", "restartTrack", "mute",
      "unmute", "getSenderStats",
    ]);
  }

  function createCameraVideoTrack(options: RawOptions, raw: unknown, onComplete: Completion) {
    const captureOptions = cameraCaptureOptions(options, raw);
    void globalThis.LivekitClient.createLocalVideoTrack(cloneCaptureOptions(captureOptions)).then(
      (track) => {
        try {
          requireLocalTrackInstance(track, "LocalVideoTrack instance");
          onComplete(true, registerLocalVideoTrack(track, captureOptions, "camera"));
        } catch (error: unknown) {
          try { track?.stop?.(); } catch {}
          onComplete(false, error);
        }
      },
      (error: unknown) => onComplete(false, error),
    );
  }

  function localVideoTrackDeviceId(trackId: LocalTrackId) {
    const record = localVideoTrackRecord(trackId);
    return enqueueLocalTrackMutation(record, async () => {
      if (record.stopRequested) throw new Error("Local Video Track capture has stopped");
      return (await record.track.getDeviceId()) ?? null;
    });
  }

  function restartLocalVideoTrack(trackId: LocalTrackId, onComplete: (terminal: boolean) => void) {
    const record = localVideoTrackRecord(trackId);
    return restartLocalTrack({
      record,
      nextOptions: (previous) => previous,
      normalizeOptions: (requested) => cameraCaptureOptions(requested, undefined),
      performRestart: restartVideoTrack,
      terminate: terminateAfterRestartFailure,
      onComplete,
    });
  }

  function restartLocalVideoTrackWithOptions(
    trackId: LocalTrackId,
    options: RawOptions,
    raw: unknown,
    onComplete: (terminal: boolean) => void,
  ) {
    const record = localVideoTrackRecord(trackId);
    return restartLocalTrack({
      record,
      nextOptions: () => cameraCaptureOptions(options, raw),
      normalizeOptions: (requested) => requested,
      performRestart: restartVideoTrack,
      terminate: terminateAfterRestartFailure,
      onComplete,
    });
  }

  function switchLocalVideoTrackDevice(
    trackId: LocalTrackId,
    deviceId: string,
    onComplete: (terminal: boolean) => void,
  ) {
    const record = localVideoTrackRecord(trackId);
    return restartLocalTrack({
      record,
      nextOptions: (previous) => ({ ...previous, deviceId: { exact: deviceId } }),
      normalizeOptions: (requested) => cameraCaptureOptions(requested, undefined),
      performRestart: restartVideoTrack,
      terminate: terminateAfterRestartFailure,
      onComplete,
    });
  }

  function attachLocalVideoTrack(trackId: LocalTrackId, element: HTMLVideoElement) {
    const record = localVideoTrackRecord(trackId);
    if (record.stopRequested) throw new Error("Local Video Track capture has stopped");
    const attachmentId = allocateAttachmentId();
    record.track.attach(element);
    record.attachments.set(attachmentId, element);
    return attachmentId;
  }

  function detachLocalVideoTrack(trackId: LocalTrackId, attachmentId: AttachmentId) {
    detachLocalElement(localVideoTrackRecord(trackId), attachmentId);
  }

  function stopLocalVideoTrack(trackId: LocalTrackId) {
    return queueLocalStop(localVideoTrackRecord(trackId));
  }

  function dropLocalVideoTrackAttachment(trackId: LocalTrackId, attachmentId: AttachmentId) {
    const record = localVideoTracks.get(trackId);
    if (record) detachLocalElement(record, attachmentId);
  }

  function finishDroppedLocalTrack(record: AnyLocalTrackRecord, trackId: LocalTrackId, retryDelay = 0) {
    if (record.ended) {
      deleteFinishedBrowserEndedRecord(record);
      return;
    }
    void queueLocalStop(record).then(
      () => {
        if (record.attachments.size === 0 && record.publishedRoomId == null) {
          const records = record.mediaKind === "video" ? localVideoTracks : localAudioTracks;
          records.delete(trackId);
        }
      },
      () => {
        const nextDelay = Math.min(Math.max(retryDelay * 2, 10), 1_000);
        setTimeout(() => finishDroppedLocalTrack(record, trackId, nextDelay), nextDelay);
      },
    );
  }

  function dropLocalVideoTrack(trackId: LocalTrackId) {
    const record = localVideoTracks.get(trackId);
    if (!record) return;
    removeLocalTrackEndedListener(record);
    record.ownerDropped = true;
    finishDroppedLocalTrack(record, trackId);
  }

  function createMicrophoneAudioTrack(options: RawOptions, raw: unknown, onComplete: Completion) {
    const captureOptions = microphoneCaptureOptions(options, raw);
    void globalThis.LivekitClient.createLocalAudioTrack(cloneCaptureOptions(captureOptions)).then(
      (track) => {
        try {
          requireLocalTrackInstance(track, "LocalAudioTrack instance");
          onComplete(true, registerLocalAudioTrack(track, captureOptions, "microphone"));
        } catch (error: unknown) {
          try { track?.stop?.(); } catch {}
          onComplete(false, error);
        }
      },
      (error: unknown) => onComplete(false, error),
    );
  }

  function createScreenCapture(options: RawOptions, raw: unknown, onComplete: Completion) {
    const captureOptions = screenCaptureOptions(options, raw);
    void globalThis.LivekitClient.createLocalScreenTracks(cloneCaptureOptions(captureOptions)).then(
      (tracks) => {
        let videoTrackId: LocalTrackId | null = null;
        let audioTrackId: LocalTrackId | null = null;
        try {
          if (!Array.isArray(tracks)) {
            throw new TypeError("LiveKit display capture did not return a track array");
          }
          const videoTracks = tracks.filter(
            (track): track is LocalVideoTrack => track.kind === "video",
          );
          const audioTracks = tracks.filter(
            (track): track is LocalAudioTrack => track.kind === "audio",
          );
          if (videoTracks.length !== 1 || audioTracks.length > 1 || tracks.length > 2) {
            throw new TypeError(
              "LiveKit display capture must return one video track and at most one audio track",
            );
          }
          requireLocalTrackInstance(videoTracks[0], "LocalVideoTrack instance");
          videoTrackId = registerLocalVideoTrack(videoTracks[0], captureOptions, "screenShare");
          const videoRecord = localVideoTrackRecord(videoTrackId);
          installLocalTrackEndedListener(videoRecord, null);
          if (audioTracks.length === 1) {
            requireLocalTrackInstance(audioTracks[0], "LocalAudioTrack instance");
            audioTrackId = registerLocalAudioTrack(
              audioTracks[0], captureOptions, "screenShareAudio",
            );
            const audioRecord = localAudioTrackRecord(audioTrackId);
            installLocalTrackEndedListener(audioRecord, null);
            videoRecord.browserEndedPeer = audioRecord;
          }
          onComplete(true, { video_track_id: videoTrackId, audio_track_id: audioTrackId });
        } catch (error: unknown) {
          if (videoTrackId != null) {
            const record = localVideoTracks.get(videoTrackId);
            if (record) {
              removeLocalTrackEndedListener(record);
              try { stopLocalRecord(record); } catch {}
              localVideoTracks.delete(videoTrackId);
            }
          }
          if (audioTrackId != null) {
            const record = localAudioTracks.get(audioTrackId);
            if (record) {
              removeLocalTrackEndedListener(record);
              try { stopLocalRecord(record); } catch {}
              localAudioTracks.delete(audioTrackId);
            }
          }
          for (const track of Array.isArray(tracks) ? tracks : []) {
            try { track.stop(); } catch {}
          }
          onComplete(false, error);
        }
      },
      (error: unknown) => onComplete(false, error),
    );
  }

  function localAudioTrackDeviceId(trackId: LocalTrackId) {
    const record = localAudioTrackRecord(trackId);
    return enqueueLocalTrackMutation(record, async () => {
      if (record.stopRequested) throw new Error("Local Audio Track capture has stopped");
      return (await record.track.getDeviceId()) ?? null;
    });
  }

  function restartLocalAudioTrack(trackId: LocalTrackId, onComplete: (terminal: boolean) => void) {
    const record = localAudioTrackRecord(trackId);
    return restartLocalTrack({
      record,
      nextOptions: (previous) => previous,
      normalizeOptions: (requested) => microphoneCaptureOptions(requested, undefined),
      performRestart: (current, requested) => current.track.restartTrack(requested),
      terminate: terminateAfterRestartFailure,
      onComplete,
    });
  }

  function restartLocalAudioTrackWithOptions(
    trackId: LocalTrackId,
    options: RawOptions,
    raw: unknown,
    onComplete: (terminal: boolean) => void,
  ) {
    const record = localAudioTrackRecord(trackId);
    return restartLocalTrack({
      record,
      nextOptions: () => microphoneCaptureOptions(options, raw),
      normalizeOptions: (requested) => requested,
      performRestart: (current, requested) => current.track.restartTrack(requested),
      terminate: terminateAfterRestartFailure,
      onComplete,
    });
  }

  function switchLocalAudioTrackDevice(
    trackId: LocalTrackId,
    deviceId: string,
    onComplete: (terminal: boolean) => void,
  ) {
    const record = localAudioTrackRecord(trackId);
    return restartLocalTrack({
      record,
      nextOptions: (previous) => ({ ...previous, deviceId: { exact: deviceId } }),
      normalizeOptions: (requested) => microphoneCaptureOptions(requested, undefined),
      performRestart: (current, requested) => current.track.restartTrack(requested),
      terminate: terminateAfterRestartFailure,
      onComplete,
    });
  }

  function attachLocalAudioTrack(trackId: LocalTrackId, element: HTMLAudioElement) {
    const record = localAudioTrackRecord(trackId);
    if (record.stopRequested) throw new Error("Local Audio Track capture has stopped");
    const attachmentId = allocateAttachmentId();
    record.track.attach(element);
    record.attachments.set(attachmentId, element);
    return attachmentId;
  }

  function detachLocalAudioTrack(trackId: LocalTrackId, attachmentId: AttachmentId) {
    detachLocalElement(localAudioTrackRecord(trackId), attachmentId);
  }

  function stopLocalAudioTrack(trackId: LocalTrackId) {
    return queueLocalStop(localAudioTrackRecord(trackId));
  }

  function dropLocalAudioTrackAttachment(trackId: LocalTrackId, attachmentId: AttachmentId) {
    const record = localAudioTracks.get(trackId);
    if (record) detachLocalElement(record, attachmentId);
  }

  function dropLocalAudioTrack(trackId: LocalTrackId) {
    const record = localAudioTracks.get(trackId);
    if (!record) return;
    removeLocalTrackEndedListener(record);
    record.ownerDropped = true;
    finishDroppedLocalTrack(record, trackId);
  }

  function audioPublicationOptions(
    options: AudioPublicationInput,
    raw: unknown,
    record: LocalAudioTrackRecord,
  ) {
    if (options.audioCodec != null && options.audioCodec !== "opus") {
      throw new TypeError(`unsupported audio codec: ${options.audioCodec}`);
    }
    const normalized: RawOptions = { ...rawOptions(raw), source: livekitTrackSource(record.source) };
    if (options.name != null) normalized.name = options.name;
    if (options.dtx != null) normalized.dtx = options.dtx;
    if (options.red != null) normalized.red = options.red;
    return normalized;
  }

  function videoPublicationOptions(
    options: VideoPublicationInput,
    raw: unknown,
    record: LocalVideoTrackRecord,
  ) {
    const normalized: RawOptions = { ...rawOptions(raw), source: livekitTrackSource(record.source) };
    if (options.name != null) normalized.name = options.name;
    if (options.videoCodec != null) normalized.videoCodec = options.videoCodec;
    if (options.simulcast != null) normalized.simulcast = options.simulcast;
    if (options.videoEncoding != null) {
      const encoding = {
        maxBitrate: options.videoEncoding.maxBitrate,
        maxFramerate: options.videoEncoding.maxFramerate,
      };
      if (record.source === "screenShare") normalized.screenShareEncoding = encoding;
      else normalized.videoEncoding = encoding;
    }
    return normalized;
  }

  function requirePublicationSid(publication: TrackPublication): string {
    if (typeof publication.trackSid !== "string" || publication.trackSid.length === 0) {
      throw new TypeError("LiveKit returned a Track Publication without a SID");
    }
    return publication.trackSid;
  }

  function publishLocalTrack<TRecord extends AnyLocalTrackRecord, TOptions>({
    roomId,
    trackId,
    publicationId,
    options,
    raw,
    onComplete,
    onReplaced,
    recordFor,
    normalizeOptions,
    mediaName,
  }: {
    roomId: RoomId;
    trackId: LocalTrackId;
    publicationId: PublicationId;
    options: TOptions;
    raw: unknown;
    onComplete: (success: boolean, sid: string | null) => void;
    onReplaced: PublicationReplacementCallback;
    recordFor: (trackId: LocalTrackId) => TRecord;
    normalizeOptions: (options: TOptions, raw: unknown, record: TRecord) => RawOptions;
    mediaName: string;
  }) {
    let operation: Promise<string>;
    try {
      const room = getRoom(roomId);
      if (!room || room.released) throw new Error("Room Session is no longer available");
      const record = recordFor(trackId);
      if (
        record.awaitingPublicationReplacement &&
        record.awaitingPublicationReplacement.roomId !== roomId
      ) {
        throw new Error(`${mediaName} is still associated with another Room Session`);
      }
      if (record.pendingPublishRoomId != null) {
        throw new Error(`${mediaName} already has a pending publication`);
      }
      const normalizedOptions = normalizeOptions(options, raw, record);
      record.pendingPublishRoomId = roomId;
      record.pendingPublicationId = publicationId;
      operation = enqueueLocalTrackMutation(record, async () => {
        let publication: LocalTrackPublication | null = null;
        try {
          if (room.released) throw new Error("Room Session is no longer available");
          if (record.stopRequested) throw new Error(`${mediaName} capture has stopped`);
          if (record.publishedRoomId != null) {
            throw new Error(`${mediaName} is already associated with a Room Session`);
          }
          publication = await room.room.localParticipant.publishTrack(record.track, normalizedOptions);
          if (room.released) {
            try { await room.room.localParticipant.unpublishTrack(record.track, false); } catch {}
            publication = null;
            throw new Error("Room Session is no longer available");
          }
          if (record.ended) {
            try {
              await room.room.localParticipant.unpublishTrack(record.track, false);
            } catch (error: unknown) {
              record.publication = publication;
              record.publishedRoomId = roomId;
              record.publicationId = publicationId;
              publication = null;
              throw error;
            }
            publication = null;
            throw new Error(`${mediaName} capture ended while publication was completing`);
          }
          const sid = requirePublicationSid(publication);
          record.publication = publication;
          record.publishedRoomId = roomId;
          record.publicationId = publicationId;
          record.publicationReplacementCallback = onReplaced;
          record.awaitingPublicationReplacement = null;
          record.pendingPublishRoomId = null;
          record.pendingPublicationId = null;
          return sid;
        } catch (error: unknown) {
          if (publication) {
            try { await room.room.localParticipant.unpublishTrack(record.track, false); } catch {}
          }
          if (
            record.pendingPublishRoomId === roomId &&
            record.pendingPublicationId === publicationId
          ) {
            record.pendingPublishRoomId = null;
            record.pendingPublicationId = null;
          }
          throw error;
        }
      });
    } catch (error: unknown) {
      operation = Promise.reject(error);
    }
    return operation.then(
      (sid) => { onComplete(true, sid); return sid; },
      (error: unknown) => { onComplete(false, null); throw error; },
    );
  }

  function setLocalTrackMuted<TRecord extends AnyLocalTrackRecord>({
    trackId, publicationId, muted, onComplete, recordFor, mediaName,
  }: {
    trackId: LocalTrackId;
    publicationId: PublicationId;
    muted: boolean;
    onComplete: (success: boolean) => void;
    recordFor: (trackId: LocalTrackId) => TRecord;
    mediaName: string;
  }) {
    let operation: Promise<unknown>;
    try {
      const record = recordFor(trackId);
      if (!muted && record.stopRequested) throw new Error(`${mediaName} capture has stopped`);
      operation = enqueueLocalTrackMutation(record, async () => {
        if (record.publicationId !== publicationId || !record.publication) {
          throw new Error("Track Publication is no longer available");
        }
        if (muted) await record.track.mute();
        else await record.track.unmute();
      });
    } catch (error: unknown) {
      operation = Promise.reject(error);
    }
    return operation.then(
      (value) => { onComplete(true); return value; },
      (error: unknown) => { onComplete(false); throw error; },
    );
  }

  function unpublishLocalTrack<TRecord extends AnyLocalTrackRecord>({
    roomId, trackId, publicationId, onComplete, recordFor, records,
  }: {
    roomId: RoomId;
    trackId: LocalTrackId;
    publicationId: PublicationId;
    onComplete: (success: boolean) => void;
    recordFor: (trackId: LocalTrackId) => TRecord;
    records: Map<LocalTrackId, TRecord>;
  }) {
    let operation: Promise<unknown>;
    try {
      const room = getRoom(roomId);
      if (!room || room.released) throw new Error("Room Session is no longer available");
      const record = recordFor(trackId);
      operation = enqueueLocalTrackMutation(record, async () => {
        if (
          record.publishedRoomId !== roomId ||
          record.publicationId !== publicationId ||
          !record.publication
        ) {
          throw new Error("Track Publication is no longer available");
        }
        record.explicitUnpublishId = publicationId;
        try {
          try {
            await room.room.localParticipant.unpublishTrack(record.track, false);
          } catch (error: unknown) {
            if (record.explicitUnpublishObservedId !== publicationId) throw error;
          }
          const replacement = [...room.room.localParticipant.trackPublications.values()].find(
            (candidate) => candidate.track === record.track,
          );
          if (replacement) {
            try {
              await room.room.localParticipant.unpublishTrack(record.track, false);
            } catch (error: unknown) {
              const stillPublished = [...room.room.localParticipant.trackPublications.values()].some(
                (candidate) => candidate.track === record.track,
              );
              if (stillPublished) throw error;
            }
          }
        } finally {
          record.explicitUnpublishId = null;
        }
        record.publication = null;
        record.publishedRoomId = null;
        record.publicationId = null;
        record.publicationReplacementCallback = null;
        record.awaitingPublicationReplacement = null;
        record.explicitUnpublishObservedId = null;
        if (record.ownerDropped && record.pendingPublishRoomId == null) records.delete(trackId);
      });
    } catch (error: unknown) {
      operation = Promise.reject(error);
    }
    return operation.then(
      (value) => { onComplete(true); return value; },
      (error: unknown) => { onComplete(false); throw error; },
    );
  }

  function publishLocalAudioTrack(
    roomId: RoomId,
    trackId: LocalTrackId,
    publicationId: PublicationId,
    options: AudioPublicationInput,
    raw: unknown,
    onReplaced: PublicationReplacementCallback,
    onComplete: (success: boolean, sid: string | null) => void,
  ) {
    return publishLocalTrack({ roomId, trackId, publicationId, options, raw, onReplaced, onComplete,
      recordFor: localAudioTrackRecord, normalizeOptions: audioPublicationOptions,
      mediaName: "Local Audio Track" });
  }

  function setLocalAudioTrackMuted(
    trackId: LocalTrackId,
    publicationId: PublicationId,
    muted: boolean,
    onComplete: (success: boolean) => void,
  ) {
    return setLocalTrackMuted({ trackId, publicationId, muted, onComplete,
      recordFor: localAudioTrackRecord, mediaName: "Local Audio Track" });
  }

  function unpublishLocalAudioTrack(
    roomId: RoomId,
    trackId: LocalTrackId,
    publicationId: PublicationId,
    onComplete: (success: boolean) => void,
  ) {
    return unpublishLocalTrack({ roomId, trackId, publicationId, onComplete,
      recordFor: localAudioTrackRecord, records: localAudioTracks });
  }

  function publishLocalVideoTrack(
    roomId: RoomId,
    trackId: LocalTrackId,
    publicationId: PublicationId,
    options: VideoPublicationInput,
    raw: unknown,
    onReplaced: PublicationReplacementCallback,
    onComplete: (success: boolean, sid: string | null) => void,
  ) {
    return publishLocalTrack({ roomId, trackId, publicationId, options, raw, onReplaced, onComplete,
      recordFor: localVideoTrackRecord, normalizeOptions: videoPublicationOptions,
      mediaName: "Local Video Track" });
  }

  function setLocalVideoTrackMuted(
    trackId: LocalTrackId,
    publicationId: PublicationId,
    muted: boolean,
    onComplete: (success: boolean) => void,
  ) {
    return setLocalTrackMuted({ trackId, publicationId, muted, onComplete,
      recordFor: localVideoTrackRecord, mediaName: "Local Video Track" });
  }

  function unpublishLocalVideoTrack(
    roomId: RoomId,
    trackId: LocalTrackId,
    publicationId: PublicationId,
    onComplete: (success: boolean) => void,
  ) {
    return unpublishLocalTrack({ roomId, trackId, publicationId, onComplete,
      recordFor: localVideoTrackRecord, records: localVideoTracks });
  }

  function localTrackSenderStatistics(
    records: LocalTrackRegistry,
    recordFor: (trackId: LocalTrackId) => AnyLocalTrackRecord,
    trackId: LocalTrackId,
    publicationId: PublicationId,
  ): Promise<AudioSenderStats | VideoSenderStats[] | undefined>;
  async function localTrackSenderStatistics(
    records: LocalTrackRegistry,
    recordFor: (trackId: LocalTrackId) => AnyLocalTrackRecord,
    trackId: LocalTrackId,
    publicationId: PublicationId,
  ) {
    const record = recordFor(trackId);
    const publication = record.publication;
    const track = record.track;
    const roomId = record.publishedRoomId;
    const roomRecord = getRoom(roomId as RoomId);
    if (record.publicationId !== publicationId || !publication || !roomRecord || roomRecord.released) {
      throw new Error("Track Publication is no longer available");
    }
    const statistics = await track.getSenderStats();
    if (
      records.get(trackId) !== record ||
      record.publicationId !== publicationId ||
      record.publication !== publication ||
      record.track !== track ||
      record.publishedRoomId !== roomId ||
      getRoom(roomId as RoomId) !== roomRecord ||
      roomRecord.released
    ) {
      throw new Error("Track Publication is no longer available");
    }
    return statistics;
  }

  async function localAudioSenderStatistics(trackId: LocalTrackId, publicationId: PublicationId) {
    const statistics = await localTrackSenderStatistics(
      localAudioTracks, localAudioTrackRecord, trackId, publicationId,
    ) as AudioSenderStats | undefined;
    return statistics ? normalizeSenderStatistics(statistics) : null;
  }

  async function localVideoSenderStatistics(trackId: LocalTrackId, publicationId: PublicationId) {
    const statistics = await localTrackSenderStatistics(
      localVideoTracks, localVideoTrackRecord, trackId, publicationId,
    );
    if (!Array.isArray(statistics)) throw new TypeError("LiveKit returned invalid sender statistics");
    return statistics.map(normalizeVideoSenderStatistics);
  }

  async function rawLocalRtcStatsReport(
    trackId: LocalTrackId,
    publicationId: PublicationId,
    kind: MediaKind,
  ) {
    const { records, recordFor } = localMediaTrackResources(kind);
    const record = recordFor(trackId);
    const publication = record.publication;
    const track = record.track;
    const roomId = record.publishedRoomId;
    const roomRecord = getRoom(roomId as RoomId);
    if (record.publicationId !== publicationId || !publication || !roomRecord || roomRecord.released) {
      throw new Error("Track Publication is no longer available");
    }
    if (typeof track.getRTCStatsReport !== "function") {
      throw new TypeError("LiveKit Local Media Track does not expose RTCStatsReport");
    }
    const report = await track.getRTCStatsReport();
    if (
      records.get(trackId) !== record ||
      record.publicationId !== publicationId ||
      record.publication !== publication ||
      record.track !== track ||
      record.publishedRoomId !== roomId ||
      getRoom(roomId as RoomId) !== roomRecord ||
      roomRecord.released
    ) {
      throw new Error("Track Publication is no longer available");
    }
    return report ?? null;
  }

  function reconcilePublished(roomId: RoomId, publication: LocalTrackPublication) {
    const roomRecord = getRoom(roomId);
    if (!roomRecord) return;
    const reconcile = <TRecord extends AnyLocalTrackRecord>(records: Map<LocalTrackId, TRecord>) => {
      for (const record of records.values()) {
        let awaiting = record.awaitingPublicationReplacement;
        if (
          !awaiting &&
          record.publishedRoomId === roomRecord.id &&
          record.publication &&
          record.publication !== publication &&
          record.publicationReplacementCallback &&
          publication?.track === record.track
        ) {
          const publicationId = record.publicationId;
          const previousPublication = record.publication;
          const callback = record.publicationReplacementCallback;
          callback(publicationId, null, false);
          if (
            getRoom(roomId) !== roomRecord || roomRecord.released ||
            record.publication !== previousPublication ||
            record.publicationId !== publicationId ||
            record.publicationReplacementCallback !== callback
          ) continue;
          awaiting = { roomId: roomRecord.id, publicationId,
            callback };
          record.awaitingPublicationReplacement = awaiting;
          record.publication = null;
        }
        if (
          !awaiting || awaiting.roomId !== roomRecord.id ||
          record.pendingPublishRoomId === roomRecord.id || publication?.track !== record.track
        ) continue;
        const sid = requirePublicationSid(publication);
        const nextId = awaiting.callback(awaiting.publicationId, sid, publication.isMuted === true);
        if (
          getRoom(roomId) !== roomRecord || roomRecord.released ||
          record.awaitingPublicationReplacement !== awaiting
        ) continue;
        if (!Number.isInteger(nextId) || nextId <= 0) {
          throw new TypeError("Rust rejected the replacement Local Track Publication");
        }
        record.publication = publication;
        record.publicationId = nextId;
        record.publishedRoomId = roomRecord.id;
        record.publicationReplacementCallback = awaiting.callback;
        record.awaitingPublicationReplacement = null;
      }
    };
    reconcile(localVideoTracks);
    reconcile(localAudioTracks);
  }

  function reconcileUnpublished(roomId: RoomId, publication: LocalTrackPublication) {
    const roomRecord = getRoom(roomId);
    if (!roomRecord) return;
    const reconcile = <TRecord extends AnyLocalTrackRecord>(records: Map<LocalTrackId, TRecord>) => {
      for (const [trackId, record] of records) {
        if (record.publication !== publication) continue;
        if (record.explicitUnpublishId === record.publicationId) {
          record.explicitUnpublishObservedId = record.publicationId;
          continue;
        }
        const callback = record.publicationReplacementCallback;
        if (record.publishedRoomId === roomRecord.id && callback) {
          const publicationId = record.publicationId;
          callback(publicationId, null, false);
          if (
            getRoom(roomId) !== roomRecord || roomRecord.released ||
            record.publication !== publication ||
            record.publicationId !== publicationId ||
            record.publicationReplacementCallback !== callback
          ) continue;
          record.awaitingPublicationReplacement = {
            roomId: roomRecord.id, publicationId, callback,
          };
          record.publication = null;
          record.publishedRoomId = null;
          record.publicationId = null;
          record.publicationReplacementCallback = null;
          if (record.ownerDropped && record.pendingPublishRoomId == null) {
            finishDroppedLocalTrack(record, trackId);
          }
          continue;
        }
        record.publication = null;
        record.publishedRoomId = null;
        record.publicationId = null;
        record.publicationReplacementCallback = null;
        record.awaitingPublicationReplacement = null;
        if (record.ownerDropped && record.pendingPublishRoomId == null) {
          finishDroppedLocalTrack(record, trackId);
        }
      }
    };
    reconcile(localVideoTracks);
    reconcile(localAudioTracks);
  }

  function reconcileSnapshot(roomId: RoomId, participant: LocalParticipant) {
    for (const publication of participant.trackPublications.values()) {
      reconcilePublished(roomId, publication);
    }
  }

  function releaseRoom(roomId: RoomId) {
    const release = <TRecord extends AnyLocalTrackRecord>(records: Map<LocalTrackId, TRecord>) => {
      for (const [trackId, record] of records) {
        const published = record.publishedRoomId === roomId;
        const pending = record.pendingPublishRoomId === roomId;
        const awaitingReplacement = record.awaitingPublicationReplacement?.roomId === roomId;
        if (!published && !pending && !awaitingReplacement) continue;
        if (published) {
          record.publication = null;
          record.publishedRoomId = null;
          record.publicationId = null;
          record.publicationReplacementCallback = null;
          record.explicitUnpublishId = null;
          record.explicitUnpublishObservedId = null;
        }
        if (pending) {
          record.pendingPublishRoomId = null;
          record.pendingPublicationId = null;
        }
        if (awaitingReplacement) {
          record.awaitingPublicationReplacement = null;
          if (record.publishedRoomId == null) record.publicationReplacementCallback = null;
        }
        if (
          record.ownerDropped && record.publishedRoomId == null &&
          record.pendingPublishRoomId == null && !record.awaitingPublicationReplacement
        ) {
          finishDroppedLocalTrack(record, trackId);
        }
      }
    };
    release(localVideoTracks);
    release(localAudioTracks);
  }

  return {
    observeLocalVideoTrackEnded,
    observeLocalAudioTrackEnded,
    createLocalClipMediaStream,
    rawLocalMediaTrack,
    rawLocalPublication,
    setLocalMediaTrackProcessor,
    rawLocalMediaTrackProcessor,
    stopLocalMediaTrackProcessor,
    createLocalAudioAnalyser,
    createCameraVideoTrack,
    localVideoTrackDeviceId,
    restartLocalVideoTrack,
    restartLocalVideoTrackWithOptions,
    switchLocalVideoTrackDevice,
    attachLocalVideoTrack,
    detachLocalVideoTrack,
    stopLocalVideoTrack,
    dropLocalVideoTrackAttachment,
    dropLocalVideoTrack,
    createMicrophoneAudioTrack,
    createScreenCapture,
    localAudioTrackDeviceId,
    restartLocalAudioTrack,
    restartLocalAudioTrackWithOptions,
    switchLocalAudioTrackDevice,
    attachLocalAudioTrack,
    detachLocalAudioTrack,
    stopLocalAudioTrack,
    dropLocalAudioTrackAttachment,
    dropLocalAudioTrack,
    publishLocalAudioTrack,
    setLocalAudioTrackMuted,
    unpublishLocalAudioTrack,
    localAudioSenderStatistics,
    rawLocalRtcStatsReport,
    publishLocalVideoTrack,
    setLocalVideoTrackMuted,
    unpublishLocalVideoTrack,
    localVideoSenderStatistics,
    reconcilePublished,
    reconcileUnpublished,
    reconcileSnapshot,
    releaseRoom,
  };
}
