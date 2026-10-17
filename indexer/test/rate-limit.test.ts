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

function mockReq(partial: Partial<Request> & { path: string }): Request {
  return {
    headers: {},
    ip: "203.0.113.10",
    socket: { remoteAddress: "203.0.113.10" } as any,
    ...partial,
  } as Request;
}

function mockRes() {
  const headers: Record<string, string> = {};
  let statusCode = 200;
  let body: unknown;
  const res = {
    setHeader(k: string, v: string) {
      headers[k.toLowerCase()] = v;
      return res;
    },
    status(code: number) {
      statusCode = code;
      return res;
    },
    json(payload: unknown) {
      body = payload;
      return res;
    },
    getHeaders: () => headers,
    getStatus: () => statusCode,
    getBody: () => body,
  };
  return res;
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

  test("auth endpoints return 429 with Retry-After after exceeding threshold", () => {
    const store = new MemorySlidingWindowStore();
    const { middleware } = createTestRateLimiter({
      authRpm: 3,
      apiRpm: 200,
      store,
      windowMs: 60_000,
    });

    for (let i = 0; i < 3; i++) {
      const res = mockRes();
      let nextCalled = false;
      middleware(
        mockReq({ path: "/v1/auth/login", method: "POST" }),
        res as unknown as Response,
        (() => {
          nextCalled = true;
        }) as NextFunction,
      );
      assert.equal(nextCalled, true, `request ${i + 1} should pass`);
      assert.equal(res.getStatus(), 200);
    }

    const blocked = mockRes();
    let nextCalled = false;
    middleware(
      mockReq({ path: "/v1/auth/login", method: "POST" }),
      blocked as unknown as Response,
      (() => {
        nextCalled = true;
      }) as NextFunction,
    );
    assert.equal(nextCalled, false);
    assert.equal(blocked.getStatus(), 429);
    assert.ok(blocked.getHeaders()["retry-after"]);
    assert.equal((blocked.getBody() as any).error, "Too Many Requests");
  });

  test("authenticated API routes use per-account limit", () => {
    const store = new MemorySlidingWindowStore();
    const { middleware } = createTestRateLimiter({
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

    for (let i = 0; i < 2; i++) {
      const res = mockRes();
      let nextCalled = false;
      middleware(req(), res as unknown as Response, (() => {
        nextCalled = true;
      }) as NextFunction);
      assert.equal(nextCalled, true);
    }

    const blocked = mockRes();
    let nextCalled = false;
    middleware(req(), blocked as unknown as Response, (() => {
      nextCalled = true;
    }) as NextFunction);
    assert.equal(nextCalled, false);
    assert.equal(blocked.getStatus(), 429);
    assert.ok(blocked.getHeaders()["retry-after"]);
  });

  test("/v1/auth/refresh is also rate-limited per IP", () => {
    const store = new MemorySlidingWindowStore();
    const { middleware } = createTestRateLimiter({
      authRpm: 1,
      apiRpm: 200,
      store,
    });

    const first = mockRes();
    let next1 = false;
    middleware(
      mockReq({ path: "/v1/auth/refresh", method: "POST" }),
      first as unknown as Response,
      (() => {
        next1 = true;
      }) as NextFunction,
    );
    assert.equal(next1, true);

    const second = mockRes();
    let next2 = false;
    middleware(
      mockReq({ path: "/v1/auth/refresh", method: "POST" }),
      second as unknown as Response,
      (() => {
        next2 = true;
      }) as NextFunction,
    );
    assert.equal(next2, false);
    assert.equal(second.getStatus(), 429);
  });

  test("Issue #1032: loadRateLimitConfig reads payment-create overrides and defaults to 30/60s", () => {
    const cfg = loadRateLimitConfig({} as NodeJS.ProcessEnv);
    assert.equal(cfg.paymentCreateLimit, 30);
    assert.equal(cfg.paymentCreateWindowMs, 60_000);

    const overridden = loadRateLimitConfig({
      RATE_LIMIT_PAYMENT_CREATE_RPW: "5",
      RATE_LIMIT_PAYMENT_CREATE_WINDOW_SECS: "30",
    } as NodeJS.ProcessEnv);
    assert.equal(overridden.paymentCreateLimit, 5);
    assert.equal(overridden.paymentCreateWindowMs, 30_000);
  });

  test("Issue #1032: POST /v1/payment-links/:id/view is rate-limited per IP, matching the contract's 30/60s budget", () => {
    const store = new MemorySlidingWindowStore();
    const { middleware } = createTestRateLimiter({
      authRpm: 10,
      apiRpm: 200,
      paymentCreateLimit: 3,
      paymentCreateWindowMs: 60_000,
      store,
    });

    const req = () =>
      mockReq({ path: "/v1/payment-links/link_123/view", method: "POST" });

    for (let i = 0; i < 3; i++) {
      const res = mockRes();
      let nextCalled = false;
      middleware(req(), res as unknown as Response, (() => {
        nextCalled = true;
      }) as NextFunction);
      assert.equal(nextCalled, true, `request ${i + 1} should pass`);
    }

    const blocked = mockRes();
    let nextCalled = false;
    middleware(req(), blocked as unknown as Response, (() => {
      nextCalled = true;
    }) as NextFunction);
    assert.equal(nextCalled, false);
    assert.equal(blocked.getStatus(), 429);
    assert.ok(blocked.getHeaders()["retry-after"]);
  });

  test("Issue #1032: payment-create limit is per IP even with different API keys", () => {
    const store = new MemorySlidingWindowStore();
    const { middleware } = createTestRateLimiter({
      authRpm: 10,
      apiRpm: 200,
      paymentCreateLimit: 1,
      paymentCreateWindowMs: 60_000,
      store,
    });

    const reqWithKey = (apiKey: string) =>
      mockReq({
        path: "/v1/payment-links/link_abc/view",
        method: "POST",
        headers: { "x-api-key": apiKey },
      });

    const first = mockRes();
    let next1 = false;
    middleware(reqWithKey("key-a"), first as unknown as Response, (() => {
      next1 = true;
    }) as NextFunction);
    assert.equal(next1, true);

    // Same IP, different API key — still blocked, because the budget is
    // keyed on IP, not on account, for this route.
    const second = mockRes();
    let next2 = false;
    middleware(reqWithKey("key-b"), second as unknown as Response, (() => {
      next2 = true;
    }) as NextFunction);
    assert.equal(next2, false);
    assert.equal(second.getStatus(), 429);
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
