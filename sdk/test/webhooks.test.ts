import { createHmac } from 'crypto';
import { verifyWebhookSignature } from '../src/index';

const SECRET = 'whsec_test_secret';

function sign(payload: string, timestamp: number, secret = SECRET): string {
  const signedPayload = `${timestamp}.${payload}`;
  const digest = createHmac('sha256', secret).update(signedPayload).digest('hex');
  return `t=${timestamp},v1=${digest}`;
}

describe('verifyWebhookSignature', () => {
  const payload = JSON.stringify({ event: 'payment.succeeded', id: 'pay_123' });

  it('returns true for a valid signature', () => {
    const timestamp = Math.floor(Date.now() / 1000);
    const signature = sign(payload, timestamp);

    expect(
      verifyWebhookSignature({ payload, signature, secret: SECRET })
    ).toBe(true);
  });

  it('accepts a Buffer payload', () => {
    const timestamp = Math.floor(Date.now() / 1000);
    const signature = sign(payload, timestamp);

    expect(
      verifyWebhookSignature({
        payload: Buffer.from(payload, 'utf8'),
        signature,
        secret: SECRET,
      })
    ).toBe(true);
  });

  it('returns false for an invalid signature', () => {
    const timestamp = Math.floor(Date.now() / 1000);
    const signature = sign(payload, timestamp, 'wrong_secret');

    expect(
      verifyWebhookSignature({ payload, signature, secret: SECRET })
    ).toBe(false);
  });

  it('returns false for a tampered payload', () => {
    const timestamp = Math.floor(Date.now() / 1000);
    const signature = sign(payload, timestamp);
    const tampered = JSON.stringify({ event: 'payment.succeeded', id: 'pay_999' });

    expect(
      verifyWebhookSignature({ payload: tampered, signature, secret: SECRET })
    ).toBe(false);
  });

  it('returns false for an expired timestamp', () => {
    const timestamp = Math.floor(Date.now() / 1000) - 600;
    const signature = sign(payload, timestamp);

    expect(
      verifyWebhookSignature({ payload, signature, secret: SECRET })
    ).toBe(false);
  });

  it('honors a custom toleranceSecs', () => {
    const timestamp = Math.floor(Date.now() / 1000) - 600;
    const signature = sign(payload, timestamp);

    expect(
      verifyWebhookSignature({
        payload,
        signature,
        secret: SECRET,
        toleranceSecs: 900,
      })
    ).toBe(true);
  });

  it('returns false when the timestamp is missing', () => {
    const digest = createHmac('sha256', SECRET).update(payload).digest('hex');

    expect(
      verifyWebhookSignature({ payload, signature: `v1=${digest}`, secret: SECRET })
    ).toBe(false);
  });

  it('returns false for a malformed signature header', () => {
    expect(
      verifyWebhookSignature({ payload, signature: 'garbage', secret: SECRET })
    ).toBe(false);
  });
});
