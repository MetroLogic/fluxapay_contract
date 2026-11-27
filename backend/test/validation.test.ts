/**
 * Issue #1155: input validation and sanitization middleware tests.
 *
 * Run with: npx ts-node --transpile-only test/validation.test.ts
 */
import assert from "assert";
import express from "express";
import {
  validateBody,
  sanitizeStrings,
  payloadSizeGuard,
  authTokenSchema,
  createPaymentSchema,
  verifyPaymentSchema,
} from "../src/middleware/validateBody";

function createTestApp() {
  const app = express();
  app.use(payloadSizeGuard(512));
  app.use(express.json());
  app.use(sanitizeStrings());

  app.post("/auth/token", validateBody(authTokenSchema), (_req, res) =>
    res.status(200).json({ ok: true }),
  );
  app.post("/v1/payments", validateBody(createPaymentSchema), (_req, res) =>
    res.status(201).json({ ok: true }),
  );
  app.post(
    "/v1/payments/verify",
    validateBody(verifyPaymentSchema),
    (_req, res) => res.status(200).json({ ok: true }),
  );
  // Echo endpoint to test sanitizeStrings.
  app.post("/echo", (req, res) => res.status(200).json(req.body));

  return app;
}

async function request(
  app: express.Express,
  method: string,
  path: string,
  body?: unknown,
): Promise<{ status: number; body: any }> {
  const server = app.listen(0);
  const addr = server.address();
  if (!addr || typeof addr === "string") {
    server.close();
    throw new Error("failed to bind");
  }
  try {
    const res = await fetch(`http://127.0.0.1:${addr.port}${path}`, {
      method,
      headers: { "Content-Type": "application/json" },
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });
    const json = await res.json().catch(() => ({}));
    return { status: res.status, body: json };
  } finally {
    server.close();
  }
}

async function main() {
  const app = createTestApp();

  // --- authTokenSchema ---

  // Missing transaction field.
  const missingTx = await request(app, "POST", "/auth/token", { account: "GABCD" });
  assert.strictEqual(missingTx.status, 400, "missing transaction → 400");
  assert.strictEqual(missingTx.body.error, "validation_error");

  // transaction too long.
  const longTx = await request(app, "POST", "/auth/token", {
    transaction: "x".repeat(10_001),
  });
  assert.strictEqual(longTx.status, 400, "oversized transaction → 400");

  // Valid auth token body.
  const validTx = await request(app, "POST", "/auth/token", {
    transaction: "AAAA".repeat(10),
  });
  assert.strictEqual(validTx.status, 200, "valid auth body → 200");

  // --- createPaymentSchema ---

  // Missing amount.
  const noAmount = await request(app, "POST", "/v1/payments", {
    deposit_address: "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN",
  });
  assert.strictEqual(noAmount.status, 400, "missing amount → 400");

  // Invalid Stellar address.
  const badAddr = await request(app, "POST", "/v1/payments", {
    deposit_address: "not-a-stellar-key",
    amount: "10.0000000",
  });
  assert.strictEqual(badAddr.status, 400, "invalid stellar address → 400");

  // Valid payment body.
  const validPayment = await request(app, "POST", "/v1/payments", {
    deposit_address: "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN",
    amount: "10.0000000",
  });
  assert.strictEqual(validPayment.status, 201, "valid payment body → 201");

  // --- verifyPaymentSchema ---

  // tx_hash too short.
  const shortHash = await request(app, "POST", "/v1/payments/verify", {
    tx_hash: "abc123",
  });
  assert.strictEqual(shortHash.status, 400, "short tx_hash → 400");

  // tx_hash not hex.
  const nonHexHash = await request(app, "POST", "/v1/payments/verify", {
    tx_hash: "z".repeat(64),
  });
  assert.strictEqual(nonHexHash.status, 400, "non-hex tx_hash → 400");

  // Valid 64-char hex hash.
  const validHash = await request(app, "POST", "/v1/payments/verify", {
    tx_hash: "a".repeat(64),
  });
  assert.strictEqual(validHash.status, 200, "valid tx_hash → 200");

  // --- payloadSizeGuard ---

  // Oversized payload (Content-Length > 512 bytes).
  const server = app.listen(0);
  const addr = server.address() as { port: number };
  const bigRes = await fetch(`http://127.0.0.1:${addr.port}/auth/token`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "Content-Length": "1000",
    },
    body: JSON.stringify({ transaction: "x".repeat(900) }),
  });
  server.close();
  assert.strictEqual(bigRes.status, 413, "oversized payload → 413");

  // --- sanitizeStrings ---

  const dirty = await request(app, "POST", "/echo", {
    name: "  hello\x00world  ",
    nested: { value: "\x00\x00test\x00" },
  });
  assert.strictEqual(dirty.status, 200, "sanitize echoes 200");
  assert.strictEqual(dirty.body.name, "hello world", "null bytes stripped and trimmed");
  assert.strictEqual(dirty.body.nested.value, "test", "nested strings sanitized");

  console.log("validation and sanitization tests passed");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
