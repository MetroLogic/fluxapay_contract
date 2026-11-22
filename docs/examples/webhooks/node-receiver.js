"use strict";

/**
 * FluxaPay webhook receiver — idempotent reference implementation.
 *
 * Verifies the HMAC-SHA256 signature, then records the event's stable
 * `event_id` and applies the side effect in a single SQLite transaction.
 * A replayed delivery therefore either sees the committed event (and is
 * acknowledged as a duplicate) or sees nothing (and is processed fully).
 *
 * Requires Node.js >= 22 for the built-in `node:sqlite` module.
 *
 *   npm install express
 *   FLUXAPAY_WEBHOOK_SECRET=whsec_... node node-receiver.js
 *
 * For Postgres, keep the same ordering: `BEGIN`, claim with
 * `INSERT ... ON CONFLICT (event_id) DO NOTHING`, apply the side effect, then
 * `COMMIT` — one transaction, so a crash rolls back both.
 */

const { createHmac, timingSafeEqual } = require("node:crypto");
const { DatabaseSync } = require("node:sqlite");

const SIGNATURE_HEADER = "x-fluxapay-signature";
const TIMESTAMP_HEADER = "x-fluxapay-timestamp";
const TOLERANCE_SECONDS = 300;
const RETENTION_SECONDS = 7 * 24 * 60 * 60;

/** One row per event that has been applied. The primary key is the dedup. */
const SCHEMA = `
  CREATE TABLE IF NOT EXISTS processed_webhook_events (
    event_id    TEXT PRIMARY KEY,
    event_type  TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    received_at INTEGER NOT NULL
  );
`;

function migrate(db) {
  db.exec(SCHEMA);
}

function headerValue(headers, name) {
  if (!headers) return undefined;
  if (typeof headers.get === "function") return headers.get(name) ?? undefined;
  const wanted = name.toLowerCase();
  for (const [key, value] of Object.entries(headers)) {
    if (key.toLowerCase() === wanted) {
      return Array.isArray(value) ? value[0] : value;
    }
  }
  return undefined;
}

/**
 * Verify `sha256=<hex>` over `${timestamp}.${rawBody}`.
 *
 * Returns false rather than throwing so the caller can answer 401 for every
 * malformed header. Ed25519 endpoints should use the SDK's
 * `verifyWebhookSignature`, which understands the `ed25519=` prefix.
 */
function verifySignature(secret, signatureHeader, timestampHeader, rawBody, nowSeconds) {
  if (typeof signatureHeader !== "string" || !signatureHeader.startsWith("sha256=")) {
    return false;
  }
  const timestamp = Number(timestampHeader);
  if (!Number.isFinite(timestamp) || Math.abs(nowSeconds - timestamp) > TOLERANCE_SECONDS) {
    return false;
  }

  const provided = Buffer.from(signatureHeader.slice("sha256=".length), "utf8");
  const expected = Buffer.from(
    createHmac("sha256", secret).update(`${timestamp}.`).update(rawBody).digest("hex"),
    "utf8",
  );
  return provided.length === expected.length && timingSafeEqual(provided, expected);
}

/** The payment/refund/dispute the event is about, for correlation only. */
function resourceIdOf(data) {
  if (!data || typeof data !== "object") return null;
  return data.payment_id ?? data.refund_id ?? data.dispute_id ?? null;
}

/**
 * Build the request handler.
 *
 * `db` is an open `node:sqlite` database, `secret` the endpoint's signing
 * secret (or FLUXAPAY_WEBHOOK_SECRET), and `applyEvent(db, event)` your side
 * effect. `applyEvent` runs inside the claim transaction, so throwing undoes
 * the claim as well and FluxaPay's retry can process the event cleanly.
 */
function createWebhookHandler({ db, secret, applyEvent, now = () => Date.now() }) {
  migrate(db);
  const claim = db.prepare(`
    INSERT INTO processed_webhook_events (event_id, event_type, resource_id, received_at)
    VALUES (?, ?, ?, ?)
    ON CONFLICT (event_id) DO NOTHING
  `);
  const prune = db.prepare(
    `DELETE FROM processed_webhook_events WHERE received_at < ?`,
  );

  return function handle(rawBody, headers) {
    const raw = Buffer.isBuffer(rawBody) ? rawBody : Buffer.from(String(rawBody), "utf8");
    const nowSeconds = Math.floor(now() / 1000);

    if (
      !verifySignature(
        secret,
        headerValue(headers, SIGNATURE_HEADER),
        headerValue(headers, TIMESTAMP_HEADER),
        raw,
        nowSeconds,
      )
    ) {
      return { status: 401, body: { error: "invalid signature" } };
    }

    let event;
    try {
      event = JSON.parse(raw.toString("utf8"));
    } catch {
      return { status: 400, body: { error: "invalid JSON" } };
    }

    // The envelope's `event_id` is stable across retries; `id` is the
    // delivery id and `data.payment_id` is not unique per event.
    if (typeof event?.event_id !== "string" || !event.event_id) {
      return { status: 400, body: { error: "missing event_id" } };
    }
    const resourceId = resourceIdOf(event.data);
    if (!resourceId) {
      return { status: 400, body: { error: "missing payment_id/refund_id/dispute_id" } };
    }

    // Ordering that survives a crash: claim and side effect commit together.
    // - Crash before COMMIT: both roll back, the retry processes normally.
    // - Crash after COMMIT: the retry finds the row and is acknowledged
    //   as a duplicate instead of applying the side effect twice.
    db.exec("BEGIN IMMEDIATE");
    try {
      const inserted = claim.run(event.event_id, String(event.type ?? ""), String(resourceId), nowSeconds);
      if (Number(inserted.changes) === 0) {
        db.exec("COMMIT");
        return { status: 200, body: { received: true, duplicate: true } };
      }

      // Test deliveries are signed like real ones but must not move money.
      if (event.livemode !== false) {
        applyEvent(db, event);
      }

      db.exec("COMMIT");
    } catch (error) {
      db.exec("ROLLBACK");
      // A 5xx leaves the delivery unacknowledged so FluxaPay retries it.
      throw error;
    }

    prune.run(nowSeconds - RETENTION_SECONDS);
    return { status: 200, body: { received: true, duplicate: false } };
  };
}

/** Example side effect: mark the payment confirmed in your own tables. */
function markPaymentConfirmed(db, event) {
  if (event.type !== "payment.confirmed" || !event.data?.payment_id) return;
  db.prepare(`
    INSERT INTO confirmed_payments (payment_id, amount, currency, confirmed_at)
    VALUES (?, ?, ?, ?)
    ON CONFLICT (payment_id) DO NOTHING
  `).run(
    event.data.payment_id,
    String(event.data.amount ?? ""),
    String(event.data.currency ?? ""),
    event.createdAt ?? new Date().toISOString(),
  );
}

async function startServer() {
  const secret = process.env.FLUXAPAY_WEBHOOK_SECRET;
  if (!secret) throw new Error("FLUXAPAY_WEBHOOK_SECRET is required");

  const { default: express } = await import("express");
  const db = new DatabaseSync(process.env.FLUXAPAY_DB ?? "webhooks.db");
  db.exec(`
    CREATE TABLE IF NOT EXISTS confirmed_payments (
      payment_id   TEXT PRIMARY KEY,
      amount       TEXT NOT NULL,
      currency     TEXT NOT NULL,
      confirmed_at TEXT NOT NULL
    );
  `);

  const handle = createWebhookHandler({
    db,
    secret,
    applyEvent: markPaymentConfirmed,
  });

  const app = express();
  app.post(
    "/webhooks/fluxapay",
    express.raw({ type: "application/json" }),
    (req, res) => {
      try {
        const { status, body } = handle(req.body, req.headers);
        res.status(status).json(body);
      } catch (error) {
        console.error("webhook processing failed", error);
        res.status(500).json({ error: "processing failed" });
      }
    },
  );
  app.listen(Number(process.env.PORT ?? 3000));
}

if (require.main === module) {
  startServer().catch((error) => {
    console.error(error);
    process.exit(1);
  });
}

module.exports = {
  createWebhookHandler,
  verifySignature,
  resourceIdOf,
  migrate,
  markPaymentConfirmed,
};
