import type {
  ReceiverStatistics,
  SenderStatistics,
  VideoSenderStatistics,
} from "./generated/adapter-contracts";

type Statistics = Record<string, unknown>;

export const optionalFiniteNumber = (value: unknown): number | null =>
  typeof value === "number" && Number.isFinite(value) ? value : null;

export const optionalUnsignedInteger = (value: unknown): number | null =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;

export const optionalSignedInteger = (value: unknown): number | null =>
  typeof value === "number" && Number.isSafeInteger(value) ? value : null;

export const optionalString = (value: unknown): string | null =>
  typeof value === "string" ? value : null;

function requiredFiniteNumber(value: unknown, field: string): number {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  throw new TypeError(`LiveKit returned invalid ${field}`);
}

function requireStatistics(
  value: unknown,
  direction: "sender" | "receiver",
): Statistics & { timestamp: number } {
  const statistics = value as Statistics | null | undefined;
  if (!statistics || !Number.isFinite(statistics.timestamp)) {
    throw new TypeError(`LiveKit returned invalid ${direction} statistics`);
  }
  return statistics as Statistics & { timestamp: number };
}

export function normalizeReceiverStatistics(value: unknown): ReceiverStatistics {
  const statistics = requireStatistics(value, "receiver");
  return {
    timestamp: statistics.timestamp,
    jitter_buffer_delay: optionalFiniteNumber(statistics.jitterBufferDelay),
    packets_lost: optionalSignedInteger(statistics.packetsLost),
    packets_received: optionalUnsignedInteger(statistics.packetsReceived),
    bytes_received: optionalUnsignedInteger(statistics.bytesReceived),
    stream_id: optionalString(statistics.streamId),
    jitter: optionalFiniteNumber(statistics.jitter),
  };
}

export function normalizeSenderStatistics(value: unknown): SenderStatistics {
  const statistics = requireStatistics(value, "sender");
  return {
    timestamp: statistics.timestamp,
    packets_sent: optionalUnsignedInteger(statistics.packetsSent),
    bytes_sent: optionalUnsignedInteger(statistics.bytesSent),
    jitter: optionalFiniteNumber(statistics.jitter),
    packets_lost: optionalSignedInteger(statistics.packetsLost),
    round_trip_time: optionalFiniteNumber(statistics.roundTripTime),
    stream_id: optionalString(statistics.streamId),
  };
}

function normalizeQualityLimitationDurations(value: unknown): Record<string, number> | null {
  if (value == null) return null;
  if (typeof value !== "object") {
    throw new TypeError("LiveKit returned invalid quality limitation durations");
  }
  const normalized: Record<string, number> = {};
  for (const [key, duration] of Object.entries(value)) {
    normalized[key] = requiredFiniteNumber(duration, "quality limitation duration");
  }
  return normalized;
}

export function normalizeVideoSenderStatistics(value: unknown): VideoSenderStatistics {
  const statistics = requireStatistics(value, "sender");
  return {
    ...normalizeSenderStatistics(statistics),
    fir_count: optionalUnsignedInteger(statistics.firCount),
    pli_count: optionalUnsignedInteger(statistics.pliCount),
    nack_count: optionalUnsignedInteger(statistics.nackCount),
    rid: optionalString(statistics.rid),
    frame_width: optionalUnsignedInteger(statistics.frameWidth),
    frame_height: optionalUnsignedInteger(statistics.frameHeight),
    frames_per_second: optionalFiniteNumber(statistics.framesPerSecond),
    frames_sent: optionalUnsignedInteger(statistics.framesSent),
    quality_limitation_reason: optionalString(statistics.qualityLimitationReason),
    quality_limitation_durations: normalizeQualityLimitationDurations(
      statistics.qualityLimitationDurations,
    ),
    quality_limitation_resolution_changes: optionalUnsignedInteger(
      statistics.qualityLimitationResolutionChanges,
    ),
    retransmitted_packets_sent: optionalUnsignedInteger(statistics.retransmittedPacketsSent),
    target_bitrate: optionalFiniteNumber(statistics.targetBitrate),
  };
}
