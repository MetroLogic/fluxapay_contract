import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import type { Request, Response, NextFunction } from 'express';
import {
  createRateLimiter,
  rateLimitTiers,
  resolveTier,
  type RateLimitStore,
} from '../src/middleware/rate-limit.js';

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
});
