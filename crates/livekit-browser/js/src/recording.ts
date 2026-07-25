type StopReason = "durationLimit" | "sizeLimit" | "explicit";
type RecordingEvent =
  | { kind: "stopping" }
  | { kind: "failed"; error: unknown }
  | { kind: "empty" }
  | {
      kind: "completed";
      blob: Blob;
      mimeType: string;
      durationMs: number;
      sizeBytes: number;
      stopReason: StopReason | "sourceEnded";
    };

interface LocalClipRecording {
  id: number;
  recorder: MediaRecorder;
  onEvent: (event: RecordingEvent) => void;
  maxDurationMs: number;
  maxBytes: number;
  chunks: Blob[];
  accumulatedBytes: number;
  activeDurationMs: number;
  activeStartedAt: number | null;
  durationTimer: ReturnType<typeof setTimeout> | null;
  stopReason: StopReason | null;
  terminal: boolean;
  onDataAvailable: (event: BlobEvent) => void;
  onError: (event: Event) => void;
  onStop: () => void;
}

const localClipRecordings = new Map<number, LocalClipRecording>();
let nextLocalClipRecordingId = 1;
const localClipNow = () => globalThis.performance.now();

export function localClipRecordingSupported(): boolean {
  return (
    typeof globalThis.MediaRecorder === "function" &&
    typeof globalThis.MediaRecorder.isTypeSupported === "function" &&
    typeof globalThis.MediaStream === "function" &&
    typeof globalThis.Blob === "function" &&
    typeof globalThis.URL?.createObjectURL === "function" &&
    typeof globalThis.URL?.revokeObjectURL === "function"
  );
}

export function selectLocalClipMimeType(preferences: string[]): string | null {
  if (!localClipRecordingSupported()) return null;
  return preferences.find((preference) => globalThis.MediaRecorder.isTypeSupported(preference)) ?? null;
}

function localClipActiveDuration(record: LocalClipRecording, at = localClipNow()): number {
  return record.activeDurationMs +
    (record.activeStartedAt == null ? 0 : Math.max(0, at - record.activeStartedAt));
}

function pauseLocalClipDurationClock(record: LocalClipRecording): void {
  if (record.activeStartedAt == null) return;
  record.activeDurationMs = localClipActiveDuration(record);
  record.activeStartedAt = null;
}

function scheduleLocalClipDurationLimit(record: LocalClipRecording): void {
  if (record.maxDurationMs === 0 || record.activeStartedAt == null || record.stopReason) return;
  if (record.durationTimer != null) clearTimeout(record.durationTimer);
  const remaining = Math.max(0, record.maxDurationMs - localClipActiveDuration(record));
  record.durationTimer = setTimeout(() => {
    if (localClipActiveDuration(record) >= record.maxDurationMs) {
      requestLocalClipStop(record, "durationLimit");
    } else {
      scheduleLocalClipDurationLimit(record);
    }
  }, Math.min(remaining, 2_147_483_647));
}

function releaseLocalClipRecord(record: LocalClipRecording): void {
  if (record.durationTimer != null) clearTimeout(record.durationTimer);
  record.recorder.removeEventListener("dataavailable", record.onDataAvailable);
  record.recorder.removeEventListener("error", record.onError);
  record.recorder.removeEventListener("stop", record.onStop);
  localClipRecordings.delete(record.id);
}

function emitLocalClipFailure(record: LocalClipRecording, error: unknown): void {
  if (record.terminal) return;
  record.terminal = true;
  pauseLocalClipDurationClock(record);
  releaseLocalClipRecord(record);
  try {
    if (record.recorder.state !== "inactive") record.recorder.stop();
  } catch {}
  record.chunks = [];
  record.onEvent({ kind: "failed", error });
}

function requestLocalClipStop(record: LocalClipRecording, reason: StopReason): void {
  if (record.terminal || record.stopReason) return;
  record.stopReason = reason;
  pauseLocalClipDurationClock(record);
  if (record.durationTimer != null) clearTimeout(record.durationTimer);
  record.onEvent({ kind: "stopping" });
  if (record.recorder.state !== "inactive") record.recorder.stop();
}

export function startLocalClipRecording(
  stream: MediaStream,
  selectedMimeType: string,
  maxDurationMs: number,
  maxBytes: number,
  onEvent: (event: RecordingEvent) => void,
) {
  if (!localClipRecordingSupported()) throw new Error("MediaRecorder is unavailable");

  const recorder = new MediaRecorder(stream, { mimeType: selectedMimeType });
  const id = nextLocalClipRecordingId++;
  const record = {} as LocalClipRecording;
  Object.assign(record, {
    id,
    recorder,
    onEvent,
    maxDurationMs,
    maxBytes,
    chunks: [],
    accumulatedBytes: 0,
    activeDurationMs: 0,
    activeStartedAt: localClipNow(),
    durationTimer: null,
    stopReason: null,
    terminal: false,
  });
  record.onDataAvailable = (event) => {
    if (record.terminal || !(event.data instanceof Blob) || event.data.size === 0) return;
    record.chunks.push(event.data);
    record.accumulatedBytes += event.data.size;
    if (record.maxBytes > 0 && record.accumulatedBytes >= record.maxBytes) {
      requestLocalClipStop(record, "sizeLimit");
    }
  };
  record.onError = (event) =>
    emitLocalClipFailure(record, (event as Event & { error?: unknown }).error ?? event);
  record.onStop = () => {
    if (record.terminal) return;
    record.terminal = true;
    pauseLocalClipDurationClock(record);
    const durationMs = record.activeDurationMs;
    const firstChunkType = record.chunks.find((chunk) => chunk.type)?.type || "";
    const mimeType = recorder.mimeType || firstChunkType;
    const stopReason = record.stopReason || "sourceEnded";
    releaseLocalClipRecord(record);
    if (!mimeType) {
      const error = new Error("MediaRecorder did not report the artifact MIME type");
      error.name = "ContractError";
      onEvent({ kind: "failed", error });
    } else {
      const blob = new Blob(record.chunks, { type: mimeType });
      if (blob.size === 0) onEvent({ kind: "empty" });
      else onEvent({ kind: "completed", blob, mimeType, durationMs, sizeBytes: blob.size, stopReason });
    }
    record.chunks = [];
  };

  recorder.addEventListener("dataavailable", record.onDataAvailable);
  recorder.addEventListener("error", record.onError);
  recorder.addEventListener("stop", record.onStop);
  localClipRecordings.set(id, record);
  try {
    recorder.start(250);
    scheduleLocalClipDurationLimit(record);
  } catch (error) {
    releaseLocalClipRecord(record);
    throw error;
  }
  return { id, mimeType: recorder.mimeType };
}

function localClipRecording(recordingId: number): LocalClipRecording {
  const record = localClipRecordings.get(recordingId);
  if (!record || record.terminal) throw new Error("Local Clip Recording is no longer active");
  return record;
}

export function pauseLocalClipRecording(recordingId: number): void {
  const record = localClipRecording(recordingId);
  if (record.recorder.state !== "recording") {
    throw new DOMException("Local Clip Recording is not recording", "InvalidStateError");
  }
  record.recorder.pause();
  pauseLocalClipDurationClock(record);
  if (record.durationTimer != null) clearTimeout(record.durationTimer);
}

export function resumeLocalClipRecording(recordingId: number): void {
  const record = localClipRecording(recordingId);
  if (record.recorder.state !== "paused") {
    throw new DOMException("Local Clip Recording is not paused", "InvalidStateError");
  }
  record.recorder.resume();
  record.activeStartedAt = localClipNow();
  scheduleLocalClipDurationLimit(record);
}

export function stopLocalClipRecording(recordingId: number): void {
  requestLocalClipStop(localClipRecording(recordingId), "explicit");
}

export function dropLocalClipRecording(recordingId: number): void {
  const record = localClipRecordings.get(recordingId);
  if (!record) return;
  record.terminal = true;
  pauseLocalClipDurationClock(record);
  releaseLocalClipRecord(record);
  try {
    if (record.recorder.state !== "inactive") record.recorder.stop();
  } catch {}
  record.chunks = [];
}

export function createLocalClipObjectUrl(blob: Blob): string {
  return URL.createObjectURL(blob);
}

export function revokeLocalClipObjectUrl(url: string): void {
  URL.revokeObjectURL(url);
}
