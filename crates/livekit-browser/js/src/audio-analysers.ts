export type AudioAnalyserId = number;

export interface AudioAnalyserOwner {
  readonly __audioAnalyserOwner: unique symbol;
}

type TerminalCallback = () => void;

interface AudioAnalyserRecord {
  id: AudioAnalyserId;
  owner: AudioAnalyserOwner;
  sourceTrack: MediaStreamTrack;
  isCurrent: () => boolean;
  sourceNode: MediaStreamAudioSourceNode;
  analyserNode: AnalyserNode;
  silentSink: GainNode;
  context: AudioContext;
  terminal: boolean;
  terminalCallback: TerminalCallback | null;
  endedListener: TerminalCallback | null;
  closeOperation: Promise<void> | null;
}

let nextAudioAnalyserId: AudioAnalyserId = 1;
const audioAnalysers = new Map<AudioAnalyserId, AudioAnalyserRecord>();
const ownerAnalysers = new Map<AudioAnalyserOwner, Set<AudioAnalyserId>>();

export function createAudioAnalyserOwner(): AudioAnalyserOwner {
  return {} as AudioAnalyserOwner;
}

function audioContextConstructor() {
  const AudioContext = globalThis.AudioContext || globalThis.webkitAudioContext;
  if (typeof AudioContext !== "function") {
    throw new TypeError("Web Audio AudioContext is unavailable");
  }
  return AudioContext;
}

export function mediaStreamAudioSource(track: { mediaStreamTrack?: MediaStreamTrack }) {
  const source = track?.mediaStreamTrack;
  if (!(source instanceof MediaStreamTrack) || source.kind !== "audio") {
    throw new TypeError("Media Track does not expose a browser audio source");
  }
  if (source.readyState === "ended") throw new Error("Media Track audio source has ended");
  return source;
}

export function createAudioAnalyser(
  owner: AudioAnalyserOwner,
  sourceTrack: MediaStreamTrack,
  isCurrent: () => boolean,
  onTerminal: TerminalCallback,
) {
  const AudioContext = audioContextConstructor();
  const context = new AudioContext();
  let sourceNode: MediaStreamAudioSourceNode;
  let analyserNode: AnalyserNode;
  let silentSink: GainNode;
  try {
    sourceNode = context.createMediaStreamSource(new MediaStream([sourceTrack]));
    analyserNode = context.createAnalyser();
    analyserNode.fftSize = 2048;
    silentSink = context.createGain();
    silentSink.gain.value = 0;
    sourceNode.connect(analyserNode);
    analyserNode.connect(silentSink);
    silentSink.connect(context.destination);
  } catch (error: unknown) {
    void context.close().catch(() => {});
    throw error;
  }

  const id = nextAudioAnalyserId++;
  const record: AudioAnalyserRecord = {
    id,
    owner,
    sourceTrack,
    isCurrent,
    sourceNode,
    analyserNode,
    silentSink,
    context,
    terminal: false,
    terminalCallback: onTerminal,
    endedListener: null,
    closeOperation: null,
  };
  record.endedListener = () => void terminateAudioAnalyser(record).catch(() => {});
  sourceTrack.addEventListener("ended", record.endedListener);
  audioAnalysers.set(id, record);
  let analyserIds = ownerAnalysers.get(owner);
  if (!analyserIds) {
    analyserIds = new Set();
    ownerAnalysers.set(owner, analyserIds);
  }
  analyserIds.add(id);
  if (context.state === "suspended") void context.resume().catch(() => {});
  return id;
}

function terminateAudioAnalyser(record: AudioAnalyserRecord): Promise<void> {
  if (record.terminal) return record.closeOperation || Promise.resolve();
  record.terminal = true;
  if (record.endedListener) {
    record.sourceTrack.removeEventListener("ended", record.endedListener);
    record.endedListener = null;
  }
  try {
    record.sourceNode.disconnect();
  } catch {}
  try {
    record.analyserNode.disconnect();
  } catch {}
  try {
    record.silentSink.disconnect();
  } catch {}
  const analyserIds = ownerAnalysers.get(record.owner);
  analyserIds?.delete(record.id);
  if (analyserIds?.size === 0) ownerAnalysers.delete(record.owner);
  audioAnalysers.delete(record.id);
  const callback = record.terminalCallback;
  record.terminalCallback = null;
  let callbackError: unknown = null;
  try {
    callback?.();
  } catch (error: unknown) {
    callbackError = error;
  }
  try {
    record.closeOperation =
      record.context.state === "closed" ? Promise.resolve() : record.context.close();
  } catch (error: unknown) {
    record.closeOperation = Promise.reject(error);
  }
  if (callbackError != null) {
    record.closeOperation = record.closeOperation.then(() => {
      throw callbackError;
    });
  }
  return record.closeOperation;
}

export function terminateAudioAnalyserOwner(owner: AudioAnalyserOwner) {
  const analyserIds = ownerAnalysers.get(owner);
  if (!analyserIds) return;
  for (const analyserId of [...analyserIds]) {
    const analyser = audioAnalysers.get(analyserId);
    if (!analyser) continue;
    try {
      void terminateAudioAnalyser(analyser).catch(() => {});
    } catch {
      // One terminal callback must not prevent cleanup of its owner's remaining analysers.
    }
  }
}

export function readAudioAnalyser(analyserId: AudioAnalyserId) {
  const record = audioAnalysers.get(analyserId);
  if (!record || record.terminal) throw new Error("Audio Analyser is no longer available");
  if (record.sourceTrack.readyState === "ended" || !record.isCurrent()) {
    void terminateAudioAnalyser(record).catch(() => {});
    throw new Error("Audio Analyser source is no longer available");
  }
  const waveform = new Float32Array(record.analyserNode.fftSize);
  record.analyserNode.getFloatTimeDomainData(waveform);
  return waveform;
}

export function closeAudioAnalyser(analyserId: AudioAnalyserId) {
  const record = audioAnalysers.get(analyserId);
  if (!record) return Promise.resolve();
  return terminateAudioAnalyser(record);
}

export function dropAudioAnalyser(analyserId: AudioAnalyserId) {
  const record = audioAnalysers.get(analyserId);
  if (record) void terminateAudioAnalyser(record).catch(() => {});
}
