import type {
  MediaDeviceInfo,
  MediaDeviceKind as AdapterMediaDeviceKind,
} from "./generated/adapter-contracts";

type MediaInputKind = "audioinput" | "videoinput";

function normalizedDeviceKind(kind: MediaDeviceKind): AdapterMediaDeviceKind {
  switch (kind) {
    case "videoinput":
      return "camera";
    case "audioinput":
      return "microphone";
    default:
      throw new TypeError(`unsupported media input kind: ${kind}`);
  }
}

export async function enumerateMediaDevices(
  kind: MediaInputKind,
  requestPermissions: boolean,
): Promise<MediaDeviceInfo[]> {
  const devices = await globalThis.LivekitClient.Room.getLocalDevices(kind, requestPermissions);
  return devices.map((device) => ({
    device_id: device.deviceId || "",
    group_id: device.groupId || "",
    label: device.label || "",
    kind: normalizedDeviceKind(device.kind),
  } satisfies MediaDeviceInfo));
}
