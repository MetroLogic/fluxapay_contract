"use strict";

/**
 * Duplicate-delivery test for node-receiver.js.
 *
 *   node --test docs/examples/webhooks/node-receiver.test.js
 */

const assert = require("node:assert/strict");
const { createHmac } = require("node:crypto");
const { DatabaseSync } = require("node:sqlite");
const test = require("node:test");

const { createWebhookHandler } = require("./node-receiver");

const SECRET = "whsec_test_secret";
const NOW_MS = 1_767_225_600_000;
const NOW_S = Math.floor(NOW_MS / 1000);

function sign(body, timestamp = NOW_S) {
  return `sha256=${createHmac("sha256", SECRET).update(`${timestamp}.${body}`).digest("hex")}`;
}

function envelope(overrides = {}) {
  return {
    id: "whd_00000000-0000-0000-0000-000000000001",
    event_id: "evt_test_payment_confirmed",
    type: "payment.confirmed",
    createdAt: "2026-01-01T00:00:00.000Z",
    livemode: true,
    data: {
      payment_id: "pay_1",
      merchant_id: "GA7NQQNLFQC7OQF6MERCHANT",
      amount: "1000000",
      currency: "USDC",
      status: "confirmed",
    },
    ...overrides,
  };
}

function setup(applyEvent) {
  const db = new DatabaseSync(":memory:");
  db.exec(`
    CREATE TABLE applied_side_effects (event_id TEXT NOT NULL);
  `);
  const handler = createWebhookHandler({
    db,
    secret: SECRET,
    now: () => NOW_MS,
    applyEvent:
      applyEvent ??
      ((conn, event) => {
        conn.prepare(`INSERT INTO applied_side_effects (event_id) VALUES (?)`).run(event.event_id);
      }),
  });
  const delivered = (event) => {
    const raw = Buffer.from(JSON.stringify(event), "utf8");
    return handler(raw, {
      "X-FluxaPay-Signature": sign(raw),
      "X-FluxaPay-Timestamp": String(NOW_S),
      "X-FluxaPay-Event": event.type,
      "X-FluxaPay-Delivery": event.id,
    });
  };
  const appliedCount = () =>
    Number(db.prepare(`SELECT COUNT(*) AS n FROM applied_side_effects`).get().n);
  const recordedCount = () =>
    Number(db.prepare(`SELECT COUNT(*) AS n FROM processed_webhook_events`).get().n);
  return { delivered, appliedCount, recordedCount, db };
}

test("duplicate delivery is acknowledged without applying the side effect twice", () => {
  const { delivered, appliedCount, recordedCount } = setup();
  const event = envelope();

  const first = delivered(event);
  assert.equal(first.status, 200);
  assert.deepEqual(first.body, { received: true, duplicate: false });

  const second = delivered(event);
  assert.equal(second.status, 200);
  assert.deepEqual(second.body, { received: true, duplicate: true });

  assert.equal(appliedCount(), 1);
  assert.equal(recordedCount(), 1);
});

test("distinct events for the same payment are both applied", () => {
  const { delivered, appliedCount } = setup();

  assert.equal(delivered(envelope()).status, 200);
  assert.equal(
    delivered(
      envelope({
        event_id: "evt_test_payment_created",
        type: "payment.created",
        data: { ...envelope().data, status: "created" },
      }),
    ).status,
    200,
  );

  assert.equal(appliedCount(), 2);
});

test("a refund event dedups on its event_id and never on the payment", () => {
  const { delivered, appliedCount, recordedCount } = setup();
  const refund = envelope({
    event_id: "evt_test_refund_completed",
    type: "refund.completed",
    data: { refund_id: "ref_1", payment_id: "pay_1", amount: "500000", status: "completed" },
  });

  assert.equal(delivered(refund).status, 200);
  assert.equal(delivered(refund).body.duplicate, true);
  assert.equal(appliedCount(), 1);
  assert.equal(recordedCount(), 1);
});

test("a crash while applying the side effect rolls back the claim", () => {
  let failNext = true;
  const { delivered, appliedCount, recordedCount } = setup((conn, event) => {
    if (failNext) {
      failNext = false;
      throw new Error("simulated crash");
    }
    conn.prepare(`INSERT INTO applied_side_effects (event_id) VALUES (?)`).run(event.event_id);
  });

  assert.throws(() => delivered(envelope()), /simulated crash/);
  assert.equal(appliedCount(), 0);
  assert.equal(recordedCount(), 0, "claim must not survive the failed transaction");

  const retry = delivered(envelope());
  assert.equal(retry.status, 200);
  assert.equal(retry.body.duplicate, false);
  assert.equal(appliedCount(), 1);
});

test("a tampered body or stale timestamp is rejected before dedup", () => {
  const { db, appliedCount } = setup();
  const raw = Buffer.from(JSON.stringify(envelope()), "utf8");
  const handler = createWebhookHandler({ db, secret: SECRET, now: () => NOW_MS, applyEvent: () => {
    throw new Error("must not run");
  } });

  const tampered = handler(Buffer.concat([raw, Buffer.from(" ")]), {
    "x-fluxapay-signature": sign(raw),
    "x-fluxapay-timestamp": String(NOW_S),
  });
  assert.equal(tampered.status, 401);

  const stale = handler(raw, {
    "x-fluxapay-signature": sign(raw, NOW_S - 301),
    "x-fluxapay-timestamp": String(NOW_S - 301),
  });
  assert.equal(stale.status, 401);
  assert.equal(appliedCount(), 0);
});
