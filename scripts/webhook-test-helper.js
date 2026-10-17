#!/usr/bin/env node
'use strict';

/**
 * FluxaPay webhook signature test helper.
 *
 * Generates a valid signed webhook payload so merchants can exercise their
 * verification code end-to-end with a copy-pasteable curl command.
 *
 * Usage:
 *   npx fluxapay-webhook-test --secret whsec_... --event payment.confirmed
 *   node scripts/webhook-test-helper.js --secret whsec_... --event payment.confirmed \
 *     --payload-file ./sample.json --url https://example.com/webhooks/fluxapay
 *
 * Signature scheme (matches docs/webhooks.md):
 *   signedPayload = `${timestamp}.${rawBody}`
 *   v1            = HMAC-SHA256(secret, signedPayload) as lowercase hex
 *   header        = `t=${timestamp},v1=${v1}`
 */

const crypto = require('crypto');
const fs = require('fs');

const DEFAULT_TOLERANCE_SECONDS = 300;

function parseArgs(argv) {
  const args = { event: 'payment.confirmed', url: 'https://example.com/webhooks/fluxapay' };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!arg.startsWith('--')) continue;
    const key = arg.slice(2);
    const value = argv[i + 1];
    if (value === undefined || value.startsWith('--')) {
      args[key] = true;
    } else {
      args[key] = value;
      i += 1;
    }
  }
  return args;
}

function buildDefaultPayload(event) {
  return {
    id: `evt_test_${crypto.randomBytes(8).toString('hex')}`,
    type: event,
    created: Math.floor(Date.now() / 1000),
    data: {
      paymentId: `pay_test_${crypto.randomBytes(6).toString('hex')}`,
      amount: '10000000',
      asset: 'USDC',
      status: 'confirmed',
    },
  };
}

function signPayload(secret, timestamp, rawBody) {
  const signedPayload = `${timestamp}.${rawBody}`;
  return crypto.createHmac('sha256', secret).update(signedPayload, 'utf8').digest('hex');
}

function main() {
  const args = parseArgs(process.argv.slice(2));

  if (!args.secret || typeof args.secret !== 'string') {
    console.error('Error: --secret is required (e.g. --secret whsec_...).');
    process.exit(1);
  }

  const event = typeof args.event === 'string' ? args.event : 'payment.confirmed';

  let payload;
  if (typeof args['payload-file'] === 'string') {
    try {
      payload = JSON.parse(fs.readFileSync(args['payload-file'], 'utf8'));
    } catch (err) {
      console.error(`Error: could not read --payload-file ${args['payload-file']}: ${err.message}`);
      process.exit(1);
    }
  } else {
    payload = buildDefaultPayload(event);
  }

  const rawBody = JSON.stringify(payload);
  const timestamp = Math.floor(Date.now() / 1000);
  const signature = signPayload(args.secret, timestamp, rawBody);
  const signatureHeader = `t=${timestamp},v1=${signature}`;
  const url = typeof args.url === 'string' ? args.url : 'https://example.com/webhooks/fluxapay';

  console.log(`X-FluxaPay-Signature: ${signatureHeader}`);
  console.log(`X-FluxaPay-Timestamp: ${timestamp}`);
  console.log(`Body: ${rawBody}`);
  console.log('');
  console.log('Test with curl:');
  console.log(
    `curl -X POST '${url}' \\\n` +
      `  -H 'Content-Type: application/json' \\\n` +
      `  -H 'X-FluxaPay-Signature: ${signatureHeader}' \\\n` +
      `  -H 'X-FluxaPay-Timestamp: ${timestamp}' \\\n` +
      `  -d '${rawBody}'`
  );
  console.log('');
  console.log(
    `Note: signatures are valid for ${DEFAULT_TOLERANCE_SECONDS}s. Re-run the helper if your endpoint rejects the timestamp as expired.`
  );
}

if (require.main === module) {
  main();
}

module.exports = { signPayload, buildDefaultPayload, parseArgs, DEFAULT_TOLERANCE_SECONDS };
