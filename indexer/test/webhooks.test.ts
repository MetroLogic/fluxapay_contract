import { createHmac } from 'crypto';
import { describe, it, expect } from 'vitest';

/**
 * Unit tests for FluxaPay webhook signature verification.
 *
 * Mirrors the scheme documented in docs/webhooks.md:
 *   signedPayload = `${timestamp}.${rawBody}`
 *   v1            = HMAC-SHA256(secret, signedPayload) as lowercase hex
 *   header        = `t=${timestamp},v1=${v1}`
 */

const TOLERANCE_SECONDS = 300;

function sign(secret: string, timestamp: number, rawBody: string): string {
  return createHmac('sha256', secret).update(`${timestamp}.${rawBody}`, 'utf8').digest('hex');
}

function parseHeader(header: string): { timestamp: number; signature: string } | null {
  const parts = header.split(',');
  let timestamp: number | null = null;
  let signature: string | null = null;
  for (const part of parts) {
    const [key, value] = part.split('=');
    if (key === 't') timestamp = Number(value);
    if (key === 'v1') signature = value;
  }
  if (timestamp === null || signature === null || Number.isNaN(timestamp)) return null;
  return { timestamp, signature };
}

function verify(
  secret: string,
  header: string,
  rawBody: string,
  nowSeconds: number = Math.floor(Date.now() / 1000)
): boolean {
  const parsed = parseHeader(header);
  if (!parsed) return false;
  if (Math.abs(nowSeconds - parsed.timestamp) > TOLERANCE_SECONDS) return false;
  const expected = sign(secret, parsed.timestamp, rawBody);
  const a = Buffer.from(expected, 'utf8');
  const b = Buffer.from(parsed.signature, 'utf8');
  if (a.length !== b.length) return false;
  return require('crypto').timingSafeEqual(a, b);
}

const SECRET = 'whsec_test_secret';
const BODY = JSON.stringify({ type: 'payment.confirmed', data: { paymentId: 'pay_1' } });

function makeHeader(secret: string, timestamp: number, body: string): string {
  return `t=${timestamp},v1=${sign(secret, timestamp, body)}`;
}

describe('webhook signature verification', () => {
  it('accepts a correct signature', () => {
    const ts = Math.floor(Date.now() / 1000);
    const header = makeHeader(SECRET, ts, BODY);
    expect(verify(SECRET, header, BODY, ts)).toBe(true);
  });

  it('rejects a wrong signature', () => {
    const ts = Math.floor(Date.now() / 1000);
    const header = `t=${ts},v1=${'0'.repeat(64)}`;
    expect(verify(SECRET, header, BODY, ts)).toBe(false);
  });

  it('rejects an expired timestamp', () => {
    const ts = Math.floor(Date.now() / 1000) - (TOLERANCE_SECONDS + 60);
    const header = makeHeader(SECRET, ts, BODY);
    expect(verify(SECRET, header, BODY)).toBe(false);
  });

  it('rejects a signature made with the wrong key', () => {
    const ts = Math.floor(Date.now() / 1000);
    const header = makeHeader('whsec_wrong_key', ts, BODY);
    expect(verify(SECRET, header, BODY, ts)).toBe(false);
  });
});
