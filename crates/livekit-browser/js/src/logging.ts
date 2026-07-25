type LogContext =
  | null
  | undefined
  | string
  | boolean
  | number
  | LogContext[]
  | { [key: string]: LogContext };
type LogCallback = (level: number, message: string, context: LogContext) => void;

let logExtensionInstalled = false;

function normalizeLogContext(value: unknown, seen = new WeakSet<object>()): LogContext {
  if (value == null || typeof value === "string" || typeof value === "boolean") return value;
  if (typeof value === "number") return Number.isFinite(value) ? value : String(value);
  if (typeof value === "bigint" || typeof value === "symbol" || typeof value === "function") {
    return String(value);
  }
  if (seen.has(value)) return "[Circular]";
  seen.add(value);
  try {
    if (Array.isArray(value)) return value.map((entry) => normalizeLogContext(entry, seen));
    const normalized: { [key: string]: LogContext } = {};
    if (value instanceof Error) {
      normalized.name = value.name;
      normalized.message = value.message;
    }
    for (const key of Object.keys(value).sort()) {
      try {
        normalized[key] = normalizeLogContext((value as Record<string, unknown>)[key], seen);
      } catch {
        normalized[key] = "[Unserializable]";
      }
    }
    return normalized;
  } finally {
    seen.delete(value);
  }
}

export function installLogExtension(onLog: LogCallback): void {
  if (logExtensionInstalled) return;
  const sdk = globalThis.LivekitClient;
  if (typeof sdk?.setLogExtension !== "function") {
    throw new TypeError("LivekitClient.setLogExtension is not a function");
  }
  sdk.setLogExtension((level, message, context) => {
    if (!Number.isInteger(level) || level < 0 || level > 4) return;
    try {
      onLog(level, String(message), normalizeLogContext(context ?? {}));
    } catch (error) {
      // Diagnostics must not interrupt LiveKit runtime behavior.
      console.error("LiveKit Rust log callback failed", error);
    }
  });
  logExtensionInstalled = true;
}
