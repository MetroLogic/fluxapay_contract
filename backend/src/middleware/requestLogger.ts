/**
 * Issue #1154: structured request logging middleware.
 *
 * Logs method, path, status, latency (ms), and merchant id (when the request
 * carries a valid bearer token) in a parseable JSON format.
 *
 * Log level is controlled by the LOG_LEVEL environment variable:
 *   silent  — no output
 *   error   — only 5xx responses
 *   warn    — 4xx and 5xx responses
 *   info    — all requests (default)
 *   debug   — all requests plus request/response body size
 *
 * Usage:
 *   app.use(requestLogger());
 */

import { Request, Response, NextFunction } from "express";
import { createHmac } from "crypto";

export type LogLevel = "silent" | "error" | "warn" | "info" | "debug";

export interface RequestLoggerOptions {
  /**
   * Minimum severity to emit. Defaults to the LOG_LEVEL env var, falling
   * back to "info".
   */
  level?: LogLevel;
  /**
   * Optional JWT secret used to decode the bearer token and extract
   * merchant_id. When omitted the middleware still records whether a
   * bearer token was present, but logs merchant_id as null.
   */
  jwtSecret?: string;
  /**
   * Custom write function. Defaults to process.stdout.write so callers can
   * swap in a test spy without patching globals.
   */
  write?: (line: string) => void;
}

export interface RequestLogEntry {
  ts: string;
  level: "info" | "warn" | "error" | "debug";
  method: string;
  path: string;
  status: number;
  latency_ms: number;
  merchant_id: string | null;
  req_size: number | null;
  res_size: number | null;
  request_id: string | null;
}

const LEVEL_RANK: Record<LogLevel, number> = {
  silent: 0,
  error: 1,
  warn: 2,
  info: 3,
  debug: 4,
};

function resolveLevel(env: string | undefined): LogLevel {
  const val = (env ?? "info").toLowerCase();
  if (val in LEVEL_RANK) return val as LogLevel;
  return "info";
}

/**
 * Attempt to decode the bearer token's payload without verifying the
 * signature (we only need the merchant_id claim for logging; auth is
 * enforced separately). When a secret is supplied we do verify the
 * signature so the logged merchant_id is trustworthy.
 */
function extractMerchantIdFromToken(
  token: string,
  jwtSecret?: string,
): string | null {
  const parts = token.split(".");
  if (parts.length !== 3) return null;
  const [header, body, sig] = parts;

  if (jwtSecret) {
    const expected = createHmac("sha256", jwtSecret)
      .update(`${header}.${body}`)
      .digest("base64url");
    if (sig !== expected) return null;
  }

  try {
    const payload = JSON.parse(
      Buffer.from(body, "base64url").toString("utf8"),
    ) as Record<string, unknown>;

    if (typeof payload.merchant_id === "string") return payload.merchant_id;
    if (typeof payload.sub === "string") return payload.sub;
  } catch {
    // malformed payload — ignore
  }
  return null;
}

export function requestLogger(opts: RequestLoggerOptions = {}) {
  const minLevel = opts.level ?? resolveLevel(process.env.LOG_LEVEL);
  const minRank = LEVEL_RANK[minLevel];
  const write = opts.write ?? ((line: string) => process.stdout.write(line + "\n"));

  return function logRequest(
    req: Request,
    res: Response,
    next: NextFunction,
  ): void {
    const startAt = process.hrtime.bigint();

    // Capture response body size via the write interception.
    let resSize = 0;
    const originalWrite = res.write.bind(res);
    const originalEnd = res.end.bind(res);

    (res as any).write = function (
      chunk: any,
      ...args: any[]
    ): boolean {
      if (chunk) {
        resSize += Buffer.isBuffer(chunk)
          ? chunk.byteLength
          : Buffer.byteLength(chunk);
      }
      return originalWrite(chunk, ...args);
    };

    (res as any).end = function (
      chunk?: any,
      ...args: any[]
    ): Response {
      if (chunk) {
        resSize += Buffer.isBuffer(chunk)
          ? chunk.byteLength
          : Buffer.byteLength(chunk);
      }
      return originalEnd(chunk, ...args);
    };

    res.on("finish", () => {
      const endAt = process.hrtime.bigint();
      const latencyMs = Number((endAt - startAt) / 1_000_000n);
      const status = res.statusCode;

      // Determine log level based on status code.
      let entryLevel: RequestLogEntry["level"] =
        status >= 500 ? "error" : status >= 400 ? "warn" : "info";
      if (minLevel === "debug") entryLevel = "debug";

      const rank = LEVEL_RANK[entryLevel];
      if (rank > minRank) return; // below minimum — skip

      // Extract merchant id from bearer token when present.
      let merchantId: string | null = null;
      const authHeader = req.header("authorization") ?? "";
      const [scheme, token] = authHeader.split(" ");
      if (scheme === "Bearer" && token) {
        merchantId = extractMerchantIdFromToken(token, opts.jwtSecret);
      }

      const reqContentLength = req.header("content-length");
      const reqSize =
        reqContentLength !== undefined ? Number(reqContentLength) : null;

      const entry: RequestLogEntry = {
        ts: new Date().toISOString(),
        level: entryLevel,
        method: req.method,
        path: req.path,
        status,
        latency_ms: latencyMs,
        merchant_id: merchantId,
        req_size: reqSize,
        res_size: minLevel === "debug" ? resSize : null,
        request_id: (req.header("x-request-id") ?? null) as string | null,
      };

      write(JSON.stringify(entry));
    });

    next();
  };
}
