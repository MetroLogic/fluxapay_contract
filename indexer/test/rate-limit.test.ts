import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import type { Request, Response, NextFunction } from 'express';
import {
  createRateLimiter,
  rateLimitTiers,
  resolveTier,
  type RateLimitStore,
} from '../src/middleware/rate-limit.js';
/**
 * Issue #817: rate limiting for auth and authenticated indexer routes.
 * Issue #963: Redis-backed limiter path (mocked Redis client).
 */
import assert from "node:assert/strict";
import test, { describe } from "node:test";
import type { Request, Response, NextFunction } from "express";

import {
  createTestRateLimiter,
  MemorySlidingWindowStore,
  loadRateLimitConfig,
  createRedisRateLimitStore,
  type RedisLikeClient,
} from "../src/rate-limit";

function makeReq(overrides: Partial<Request> = {}): Request {
  return {
    method: 'GET',
    path: '/v1/payments',
    ip: '203.0.113.7',
    headers: {},
    ...overrides,
  } as unknown as Request;
}

function makeRes() {
  const headers: Record<string, string> = {};
  const res = {
    statusCode: 200,
    headers,
    setHeader(name: string, value: string | number) {
      headers[name.toLowerCase()] = String(value);
      return this;
    },
    status(code: number) {
      this.statusCode = code;
      return this;
    },
    json(body: unknown) {
      this.body = body;
      return this;
    },
    body: undefined as unknown,
  };
  return res as unknown as Response & {
    statusCode: number;
    headers: Record<string, string>;
    body: unknown;
  };
}

function makeStore(): RateLimitStore {
  const buckets = new Map<string, { tokens: number; updatedAt: number }>();
  return {
    async consume(key, limit, windowMs, now) {
      const existing = buckets.get(key);
      const refillRate = limit / windowMs;
      let tokens = limit;
      if (existing) {
        const elapsed = Math.max(0, now - existing.updatedAt);
        tokens = Math.min(limit, existing.tokens + elapsed * refillRate);
      }
      const allowed = tokens >= 1;
      if (allowed) tokens -= 1;
      buckets.set(key, { tokens, updatedAt: now });
      const missing = Math.max(0, 1 - tokens);
      const resetMs = allowed ? 0 : Math.ceil(missing / refillRate);
      return { allowed, remaining: Math.floor(tokens), resetMs };
    },
  };
}
/**
 * Minimal in-memory stand-in for the `redis` client, implementing only the
 * commands the Redis rate-limit store relies on (INCR + PEXPIRE + PTTL).
 */
function mockRedisClient(): RedisLikeClient & { store: Map<string, { count: number; expiresAt: number }> } {
  const store = new Map<string, { count: number; expiresAt: number }>();
  const now = () => Date.now();
  return {
    store,
    async incr(key: string) {
      const entry = store.get(key);
      if (!entry || entry.expiresAt <= now()) {
        store.set(key, { count: 1, expiresAt: 0 });
        return 1;
      }
      entry.count += 1;
      return entry.count;
    },
    async pexpire(key: string, ms: number) {
      const entry = store.get(key);
      if (!entry) return 0;
      entry.expiresAt = now() + ms;
      return 1;
    },
    async pttl(key: string) {
      const entry = store.get(key);
      if (!entry) return -2;
      const remaining = entry.expiresAt - now();
      return remaining > 0 ? remaining : -2;
    },
  };
}

describe("indexer rate limiting (Issue #817)", () => {
  test("loadRateLimitConfig reads RATE_LIMIT_AUTH_RPM and RATE_LIMIT_API_RPM", () => {
    const cfg = loadRateLimitConfig({
      RATE_LIMIT_AUTH_RPM: "7",
      RATE_LIMIT_API_RPM: "50",
    } as NodeJS.ProcessEnv);
    assert.equal(cfg.authRpm, 7);
    assert.equal(cfg.apiRpm, 50);
  });

async function run(
  limiter: ReturnType<typeof createRateLimiter>,
  req: Request,
  res: ReturnType<typeof makeRes>,
): Promise<boolean> {
  let nextCalled = false;
  const next: NextFunction = () => {
    nextCalled = true;
  };
  await limiter(req, res, next);
  return nextCalled;
}

describe('rate limit tiers', () => {
  it('defines a tier for every documented endpoint group', () => {
    expect(rateLimitTiers).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ method: 'POST', pattern: /^\/v1\/payments\/?$/, limit: 100, windowMs: 60_000, scope: 'apiKey' }),
        expect.objectContaining({ method: 'GET', pattern: /^\/v1\/payments\//, limit: 300, windowMs: 60_000, scope: 'apiKey' }),
        expect.objectContaining({ method: 'GET', pattern: /^\/v1\/payment-links\//, limit: 60, windowMs: 60_000, scope: 'ip' }),
        expect.objectContaining({ method: 'POST', pattern: /^\/v1\/fx\/convert\/?$/, limit: 60, windowMs: 60_000, scope: 'ip' }),
        expect.objectContaining({ method: 'POST', pattern: /^\/v1\/auth\//, limit: 10, windowMs: 60_000, scope: 'ip' }),
      ]),
    );
  });

  it('resolves the correct tier for each route', () => {
    expect(resolveTier('POST', '/v1/payments')?.limit).toBe(100);
    expect(resolveTier('GET', '/v1/payments/pi_123')?.limit).toBe(300);
    expect(resolveTier('GET', '/v1/payment-links/pl_123')?.limit).toBe(60);
    expect(resolveTier('POST', '/v1/fx/convert')?.limit).toBe(60);
    expect(resolveTier('POST', '/v1/auth/login')?.limit).toBe(10);
  });

  it('gives admin endpoints a separate, higher limit', () => {
    const admin = resolveTier('GET', '/v1/admin/merchants');
    expect(admin).toBeDefined();
    expect(admin!.limit).toBeGreaterThan(300);
  });
});

describe('token bucket middleware', () => {
  let now: number;

  beforeEach(() => {
    now = 1_700_000_000_000;
    vi.spyOn(Date, 'now').mockImplementation(() => now);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('allows requests under the limit and sets rate limit headers', async () => {
    const limiter = createRateLimiter({ store: makeStore() });
    const req = makeReq({ method: 'POST', path: '/v1/payments', headers: { 'x-api-key': 'key_a' } });
    const res = makeRes();

    const nextCalled = await run(limiter, req, res);

    expect(nextCalled).toBe(true);
    expect(res.statusCode).toBe(200);
    expect(res.headers['x-ratelimit-limit']).toBe('100');
    expect(res.headers['x-ratelimit-remaining']).toBe('99');
    expect(res.headers['x-ratelimit-reset']).toBeDefined();
  });

  it('returns 429 with Retry-After once the bucket is exhausted', async () => {
    const limiter = createRateLimiter({ store: makeStore() });
    const req = makeReq({ method: 'POST', path: '/v1/auth/login' });

    for (let i = 0; i < 10; i += 1) {
      const res = makeRes();
      expect(await run(limiter, req, res)).toBe(true);
    }

    const blocked = makeRes();
    const nextCalled = await run(limiter, req, blocked);

    expect(nextCalled).toBe(false);
    expect(blocked.statusCode).toBe(429);
    expect(blocked.headers['retry-after']).toBeDefined();
    expect(Number(blocked.headers['retry-after'])).toBeGreaterThan(0);
    expect(blocked.headers['x-ratelimit-remaining']).toBe('0');
  });

  it('refills tokens over time', async () => {
    const limiter = createRateLimiter({ store: makeStore() });
    const req = makeReq({ method: 'POST', path: '/v1/auth/login' });

    for (let i = 0; i < 10; i += 1) {
      await run(limiter, req, makeRes());
    }
    expect(await run(limiter, req, makeRes())).toBe(false);

    now += 60_000;
    expect(await run(limiter, req, makeRes())).toBe(true);
  });

  it('limits auth endpoints by IP, not API key', async () => {
    const limiter = createRateLimiter({ store: makeStore() });
    const reqA = makeReq({ method: 'POST', path: '/v1/auth/login', ip: '198.51.100.1', headers: { 'x-api-key': 'key_a' } });
    const reqB = makeReq({ method: 'POST', path: '/v1/auth/login', ip: '198.51.100.1', headers: { 'x-api-key': 'key_b' } });

    for (let i = 0; i < 10; i += 1) {
      await run(limiter, reqA, makeRes());
    }

    expect(await run(limiter, reqB, makeRes())).toBe(false);
  });

  it('limits payment-links by IP for unauthenticated callers', async () => {
    const limiter = createRateLimiter({ store: makeStore() });
    const req = makeReq({ method: 'GET', path: '/v1/payment-links/pl_123' });

    for (let i = 0; i < 60; i += 1) {
      expect(await run(limiter, req, makeRes())).toBe(true);
    }

    const blocked = makeRes();
    expect(await run(limiter, req, blocked)).toBe(false);
    expect(blocked.statusCode).toBe(429);
  });

  it('isolates buckets per API key', async () => {
    const limiter = createRateLimiter({ store: makeStore() });
    const reqA = makeReq({ method: 'POST', path: '/v1/payments', headers: { 'x-api-key': 'key_a' } });
    const reqB = makeReq({ method: 'POST', path: '/v1/payments', headers: { 'x-api-key': 'key_b' } });

    for (let i = 0; i < 100; i += 1) {
      await run(limiter, reqA, makeRes());
    }
    expect(await run(limiter, reqA, makeRes())).toBe(false);
    expect(await run(limiter, reqB, makeRes())).toBe(true);
  });

  test("Issue #963: Redis-backed store enforces a shared limit across replicas", async () => {
    const redis = mockRedisClient();
    const store = createRedisRateLimitStore(redis);

    // Two independent limiters sharing the same Redis client simulate two
    // replicas: the counter must be shared, not per-process.
    const replicaA = createTestRateLimiter({
      authRpm: 10,
      apiRpm: 2,
      store,
      windowMs: 60_000,
    });
    const replicaB = createTestRateLimiter({
      authRpm: 10,
      apiRpm: 2,
      store,
      windowMs: 60_000,
    });

    const req = () =>
      mockReq({
        path: "/v1/payments",
        method: "GET",
        headers: { "x-api-key": "merchant-key-1" },
      });

    const first = mockRes();
    let next1 = false;
    replicaA.middleware(req(), first as unknown as Response, (() => {
      next1 = true;
    }) as NextFunction);
    assert.equal(next1, true);

    const second = mockRes();
    let next2 = false;
    replicaB.middleware(req(), second as unknown as Response, (() => {
      next2 = true;
    }) as NextFunction);
    assert.equal(next2, true);

    // Third request hits the shared budget regardless of which replica serves it.
    const blocked = mockRes();
    let next3 = false;
    replicaA.middleware(req(), blocked as unknown as Response, (() => {
      next3 = true;
    }) as NextFunction);
    assert.equal(next3, false);
    assert.equal(blocked.getStatus(), 429);
    assert.ok(blocked.getHeaders()["retry-after"]);
  });

  test("Issue #963: Redis-backed responses include X-RateLimit-* headers", async () => {
    const redis = mockRedisClient();
    const store = createRedisRateLimitStore(redis);
    const { middleware } = createTestRateLimiter({
      authRpm: 10,
      apiRpm: 5,
      store,
      windowMs: 60_000,
    });

    const res = mockRes();
    let nextCalled = false;
    middleware(
      mockReq({ path: "/v1/fx/convert", method: "GET" }),
      res as unknown as Response,
      (() => {
        nextCalled = true;
      }) as NextFunction,
    );
    assert.equal(nextCalled, true);
    const headers = res.getHeaders();
    assert.equal(headers["x-ratelimit-limit"], "5");
    assert.equal(headers["x-ratelimit-remaining"], "4");
    assert.ok(headers["x-ratelimit-reset"]);
  });
});
