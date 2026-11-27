/**
 * Minimal structured logger used across the backend.
 *
 * In production wire this up to pino or winston. For now it writes
 * newline-delimited JSON to stdout/stderr so it is parseable without
 * any additional dependency.
 */

type LogLevel = "debug" | "info" | "warn" | "error";

function emit(level: LogLevel, obj: Record<string, unknown>, msg?: string): void {
  const entry = {
    ts: new Date().toISOString(),
    level,
    ...(msg ? { msg } : {}),
    ...obj,
  };
  const line = JSON.stringify(entry);
  if (level === "error" || level === "warn") {
    process.stderr.write(line + "\n");
  } else {
    process.stdout.write(line + "\n");
  }
}

export const logger = {
  debug: (obj: Record<string, unknown>, msg?: string) => emit("debug", obj, msg),
  info:  (obj: Record<string, unknown>, msg?: string) => emit("info",  obj, msg),
  warn:  (obj: Record<string, unknown>, msg?: string) => emit("warn",  obj, msg),
  error: (obj: Record<string, unknown>, msg?: string) => emit("error", obj, msg),
};
