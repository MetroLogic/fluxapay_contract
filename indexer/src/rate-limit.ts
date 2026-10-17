/**
 * Issue #817 / #1032: sliding-window rate limiter for the indexer REST API.
 * Issue #963: optional Redis-backed store so limits are shared across replicas.
 *
 * - Auth routes (`/v1/auth/*`): per-IP, default 10 req/min (`RATE_LIMIT_AUTH_RPM`)
 * - Payment-creation-adjacent routes (see `PAYMENT_CREATE_ROUTES` below):
 *   always per-IP (these are pre-auth, no API key is required), default
 *   30 req/60s (`RATE_LIMIT_PAYMENT_CREATE_RPW` / `RATE_LIMIT_PAYMENT_CREATE_WINDOW_SECS`),
 *   matching the contract's `CREATE_PAYMENT_MAX_PER_WINDOW` /
 *   `CREATE_PAYMENT_WINDOW_SECS` constants. Note: this indexer has no
 *   `POST /v1/payments` route — payment creation is an on-chain
 *   `PaymentProcessor::create_payment` contract call, not a REST endpoint.
 *   The closest pre-auth, payment-related, spammable POST route is
 *   `/v1/payment-links/:id/view`, which is what this bucket protects.
 * - All other routes: per-account (API key / auth subject) falling back to IP,
 *   default 200 req/min (`RATE_LIMIT_API_RPM`)
 *
 * When `REDIS_URL` is set, counters live in Redis (shared across replicas);
 * otherwise the in-memory store is used.
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

/**
 * Issue #963: minimal Redis surface used by the limiter. Kept structural so
 * tests can inject a mock without pulling in the real `redis` client.
 */
export interface RedisLikeClient {
  incr(key: string): Promise<number>;
  pexpire(key: string, ms: number): Promise<unknown>;
  pttl(key: string): Promise<number>;
}

/**
 * Issue #963: Redis-backed fixed-window counter store. Uses `INCR` + `PEXPIRE`
 * so the counter is atomic and shared across all replicas.
 */
export class RedisRateLimitStore implements RateLimitStore {
  constructor(private readonly client: RedisLikeClient) {}

  /**
   * Synchronous interface is preserved for the middleware; the Redis path is
   * driven through `hitAsync` (see `createRateLimitMiddleware`). This method
   * exists only to satisfy the interface and is not used on the Redis path.
   */
  hit(): number {
    throw new Error("RedisRateLimitStore requires the async middleware path");
  }

  async hitAsync(key: string, windowMs: number): Promise<number> {
    const count = await this.client.incr(key);
    if (count === 1) {
      await this.client.pexpire(key, windowMs);
    }
    return count;
  }

  async resetMs(key: string): Promise<number> {
    const ttl = await this.client.pttl(key);
    return ttl > 0 ? ttl : 0;
  }

  clear(): void {
    // Redis keys expire on their own; nothing to do here.
  }
}

export interface RateLimitConfig {
  authRpm: number;
  apiRpm: number;
  /** Issue #1032: limit for payment-creation-adjacent routes, per IP. */
  paymentCreateLimit: number;
  /** Issue #1032: window (ms) for `paymentCreateLimit`. */
  paymentCreateWindowMs: number;
  windowMs?: number;
  store?: RateLimitStore;
  /** Issue #963: Redis client; when set, counters are shared across replicas. */
  redis?: RedisLikeClient;
}

/**
 * Issue #1032: routes that sit in front of on-chain payment creation and are
 * reachable without an API key. Matched by method + a path regexp so Express
 * route params (`:id`) are handled without needing the app's own router.
 */
const PAYMENT_CREATE_ROUTES: { method: string; pattern: RegExp }[] = [
  { method: "POST", pattern: /^\/v1\/payment-links\/[^/]+\/view$/ },
  // If/when a direct POST /v1/payments creation endpoint is added, it
  // belongs here too so it inherits the same per-IP budget.
  { method: "POST", pattern: /^\/v1\/payments\/?$/ },
];

function isPaymentCreateRoute(req: Request): boolean {
  const method = req.method || "GET";
  const path = req.path || req.url || "";
  return PAYMENT_CREATE_ROUTES.some(
    (route) => route.method === method && route.pattern.test(path),
  );
}

export function loadRateLimitConfig(
  env: NodeJS.ProcessEnv = process.env,
): RateLimitConfig {
  const authRpm = parseInt(env.RATE_LIMIT_AUTH_RPM || "10", 10);
  const apiRpm = parseInt(env.RATE_LIMIT_API_RPM || "200", 10);
  // Issue #1032: defaults mirror the contract's CREATE_PAYMENT_MAX_PER_WINDOW
  // (30) / CREATE_PAYMENT_WINDOW_SECS (60) so API-level and on-chain spam
  // controls agree.
  const paymentCreateLimit = parseInt(
    env.RATE_LIMIT_PAYMENT_CREATE_RPW || "30",
    10,
  );
  const paymentCreateWindowSecs = parseInt(
    env.RATE_LIMIT_PAYMENT_CREATE_WINDOW_SECS || "60",
    10,
  );
  return {
    authRpm: Number.isFinite(authRpm) && authRpm > 0 ? authRpm : 10,
    apiRpm: Number.isFinite(apiRpm) && apiRpm > 0 ? apiRpm : 200,
    paymentCreateLimit:
      Number.isFinite(paymentCreateLimit) && paymentCreateLimit > 0
        ? paymentCreateLimit
        : 30,
    paymentCreateWindowMs:
      (Number.isFinite(paymentCreateWindowSecs) && paymentCreateWindowSecs > 0
        ? paymentCreateWindowSecs
        : 60) * 1000,
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
  const paymentCreateLimit = config?.paymentCreateLimit ?? loaded.paymentCreateLimit;
  const paymentCreateWindowMs =
    config?.paymentCreateWindowMs ?? loaded.paymentCreateWindowMs;
  const windowMs = config?.windowMs ?? 60_000;
  const store = config?.store ?? new MemorySlidingWindowStore();
  const redisStore = config?.redis ? new RedisRateLimitStore(config.redis) : undefined;

  return function rateLimitMiddleware(
    req: Request,
    res: Response,
    next: NextFunction,
  ): void {
    const path = req.path || req.url || "";
    const isAuth = path.startsWith("/v1/auth/") || path === "/v1/auth";
    const isPaymentCreate = !isAuth && isPaymentCreateRoute(req);

    let limit: number;
    let key: string;
    let effectiveWindowMs: number;
    if (isAuth) {
      limit = authRpm;
      key = `auth:${clientIp(req)}`;
      effectiveWindowMs = windowMs;
    } else if (isPaymentCreate) {
      // Issue #1032: always per-IP, regardless of API key, since a spammer
      // can mint fresh keys but not fresh IPs as cheaply.
      limit = paymentCreateLimit;
      key = `payment_create:${clientIp(req)}`;
      effectiveWindowMs = paymentCreateWindowMs;
    } else {
      limit = apiRpm;
      key = `api:${accountKey(req)}`;
      effectiveWindowMs = windowMs;
    }

    const now = Date.now();

    const finish = (count: number, resetMs: number): void => {
      res.setHeader("X-RateLimit-Limit", String(limit));
      res.setHeader("X-RateLimit-Remaining", String(Math.max(0, limit - count)));
      res.setHeader(
        "X-RateLimit-Reset",
        String(Math.ceil((now + resetMs) / 1000)),
      );

      if (count > limit) {
        const retryAfterSec = Math.ceil(resetMs / 1000);
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

    if (redisStore) {
      // Issue #963: shared counter across replicas.
      const redisKey = `ratelimit:${key}`;
      redisStore
        .hitAsync(redisKey, effectiveWindowMs)
        .then(async (count) => {
          const resetMs = await redisStore.resetMs(redisKey);
          finish(count, resetMs || effectiveWindowMs);
        })
        .catch(() => {
          // Fail open on Redis errors so a Redis outage does not take down
          // the API; fall back to the in-memory store for this request.
          const count = store.hit(key, now, effectiveWindowMs);
          finish(count, effectiveWindowMs);
        });
      return;
    }

    const count = store.hit(key, now, effectiveWindowMs);
    finish(count, effectiveWindowMs);
  };
}

/** Test helper: expose a fresh store + middleware pair. */
export function createTestRateLimiter(overrides?: Partial<RateLimitConfig>) {
  const store = overrides?.store ?? new MemorySlidingWindowStore();
  const middleware = createRateLimitMiddleware({ ...overrides, store });
  return { store, middleware };
}
