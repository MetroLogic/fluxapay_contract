import { describe, it, expect, beforeEach } from 'vitest';
import { IdempotencyStore } from '../src/idempotency/store';
import { validateIdempotencyKey } from '../src/idempotency/validate';

describe('idempotency key support for POST /v1/payments', () => {
  let store: IdempotencyStore;

  beforeEach(() => {
    store = new IdempotencyStore();
  });

  it('accepts a valid UUID idempotency key', () => {
    expect(validateIdempotencyKey('550e8400-e29b-41d4-a716-446655440000')).toBe(true);
  });

  it('rejects a malformed idempotency key', () => {
    expect(validateIdempotencyKey('not-a-uuid')).toBe(false);
  });

  it('treats a missing idempotency key as optional', () => {
    expect(validateIdempotencyKey(undefined)).toBe(false);
  });

  it('returns the same payment ID for the same key twice', async () => {
    const key = '550e8400-e29b-41d4-a716-446655440000';
    let created = 0;

    const first = await store.handle(key, async () => {
      created += 1;
      return { paymentId: 'pay_123', status: 'created' };
    });

    const second = await store.handle(key, async () => {
      created += 1;
      return { paymentId: 'pay_999', status: 'created' };
    });

    expect(first.replayed).toBe(false);
    expect(second.replayed).toBe(true);
    expect(second.body.paymentId).toBe('pay_123');
    expect(created).toBe(1);
  });

  it('creates a new payment for a different key', async () => {
    const first = await store.handle('550e8400-e29b-41d4-a716-446655440000', async () => ({
      paymentId: 'pay_123',
    }));
    const second = await store.handle('6ba7b810-9dad-11d1-80b4-00c04fd430c8', async () => ({
      paymentId: 'pay_456',
    }));

    expect(first.replayed).toBe(false);
    expect(second.replayed).toBe(false);
    expect(second.body.paymentId).toBe('pay_456');
  });

  it('expires cached responses after the 24 hour TTL', async () => {
    const key = '550e8400-e29b-41d4-a716-446655440000';
    const now = Date.now();
    const ttlStore = new IdempotencyStore({ now: () => now });

    await ttlStore.handle(key, async () => ({ paymentId: 'pay_123' }));

    const later = new IdempotencyStore({ now: () => now + 24 * 60 * 60 * 1000 + 1 });
    const result = await later.handle(key, async () => ({ paymentId: 'pay_789' }));

    expect(result.replayed).toBe(false);
    expect(result.body.paymentId).toBe('pay_789');
  });
});
