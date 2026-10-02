/**
 * Issue #817: sliding-window rate limiter for the indexer REST API.
 *
 * - Auth routes (`/v1/auth/*`): per-IP, default 10 req/min (`RATE_LIMIT_AUTH_RPM`)
 * - All other routes: per-account (API key / auth subject) falling back to IP,
 *   default 200 req/min (`RATE_LIMIT_API_RPM`)
 *
 * On breach: HTTP 429 with a `Retry-After` header (seconds).
 */

import type { Request, Response, NextFunction } from "express";

export interface RateLimitStore {
  /** Record a hit at `nowMs` for `key`; return hits in the last `windowMs`. */
  hit(key: string, nowMs: number, windowMs: number): number;
  clear(): void;
}

/** In-memory sliding-window store (suitable for single-process / tests). */
export class MemorySlidingWindowStore implements RateLimitStore {
  private readonly buckets = new Map<string, number[]>();

  hit(key: string, nowMs: number, windowMs: number): number {
    const cutoff = nowMs - windowMs;
    let timestamps = this.buckets.get(key) ?? [];
    timestamps = timestamps.filter((t) => t > cutoff);
    timestamps.push(nowMs);
    this.buckets.set(key, timestamps);
    return timestamps.length;
  }

  clear(): void {
    this.buckets.clear();
  }
}

export interface RateLimitConfig {
  authRpm: number;
  apiRpm: number;
  windowMs?: number;
  store?: RateLimitStore;
}

export function loadRateLimitConfig(
  env: NodeJS.ProcessEnv = process.env,
): RateLimitConfig {
  const authRpm = parseInt(env.RATE_LIMIT_AUTH_RPM || "10", 10);
  const apiRpm = parseInt(env.RATE_LIMIT_API_RPM || "200", 10);
  return {
    authRpm: Number.isFinite(authRpm) && authRpm > 0 ? authRpm : 10,
    apiRpm: Number.isFinite(apiRpm) && apiRpm > 0 ? apiRpm : 200,
  };
}

function clientIp(req: Request): string {
  const forwarded = req.headers["x-forwarded-for"];
  if (typeof forwarded === "string" && forwarded.trim()) {
    return forwarded.split(",")[0]!.trim();
  }
  return req.ip || req.socket.remoteAddress || "unknown";
}

function accountKey(req: Request): string {
  const apiKey =
    (typeof req.headers["x-api-key"] === "string" && req.headers["x-api-key"].trim()) ||
    (typeof req.headers.authorization === "string" &&
    req.headers.authorization.startsWith("Bearer ")
      ? req.headers.authorization.slice("Bearer ".length).trim()
      : "");
  if (apiKey) return `acct:${apiKey}`;
  const sub = (req as any).auth?.sub;
  if (typeof sub === "string" && sub) return `acct:${sub}`;
  return `ip:${clientIp(req)}`;
}

export function createRateLimitMiddleware(config?: Partial<RateLimitConfig>) {
  const loaded = loadRateLimitConfig();
  const authRpm = config?.authRpm ?? loaded.authRpm;
  const apiRpm = config?.apiRpm ?? loaded.apiRpm;
  const windowMs = config?.windowMs ?? 60_000;
  const store = config?.store ?? new MemorySlidingWindowStore();

  return function rateLimitMiddleware(
    req: Request,
    res: Response,
    next: NextFunction,
  ): void {
    const path = req.path || req.url || "";
    const isAuth = path.startsWith("/v1/auth/") || path === "/v1/auth";
    const limit = isAuth ? authRpm : apiRpm;
    const key = isAuth ? `auth:${clientIp(req)}` : `api:${accountKey(req)}`;

    const now = Date.now();
    const count = store.hit(key, now, windowMs);

    res.setHeader("X-RateLimit-Limit", String(limit));
    res.setHeader("X-RateLimit-Remaining", String(Math.max(0, limit - count)));

    if (count > limit) {
      const retryAfterSec = Math.ceil(windowMs / 1000);
      res.setHeader("Retry-After", String(retryAfterSec));
      res.status(429).json({
        error: "Too Many Requests",
        retry_after: retryAfterSec,
        limit,
      });
      return;
    }

    next();
  };
}

/** Test helper: expose a fresh store + middleware pair. */
export function createTestRateLimiter(overrides?: Partial<RateLimitConfig>) {
  const store = overrides?.store ?? new MemorySlidingWindowStore();
  const middleware = createRateLimitMiddleware({ ...overrides, store });
  return { store, middleware };
}
