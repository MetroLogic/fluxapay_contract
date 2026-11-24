# FluxaPay Webhook Integration Guide

This guide explains how merchants consume **off-chain webhook** notifications for payment lifecycle events. On-chain event catalogs live in [`docs/events.md`](events.md) and [`fluxapay/EVENTS.md`](../fluxapay/EVENTS.md). Webhooks are the REST-facing projection of those events for merchant backends.

## Endpoint management API

Webhook endpoints are merchant-scoped. Authenticate with a merchant API key that
has the `manage:webhooks` scope.

### Create

```http
POST /v1/webhooks
Content-Type: application/json

{
  "url": "https://merchant.example.com/webhooks",
  "event_types": ["payment.confirmed", "refund.completed"],
  "signing_algorithm": "hmac_sha256"
}
```

`event_types` may be omitted or set to `[]` to receive all supported event
types. The response includes `signing_secret` once, when FluxaPay generates it.
Never store or return the secret as part of later list or update responses.

### List and update

```http
GET /v1/webhooks
PATCH /v1/webhooks/{endpointId}
{
  "url": "https://merchant.example.com/new-webhooks",
  "event_types": ["payment.confirmed"],
  "enabled": true
}
```

List and normal update responses redact signing secrets. `PUT` is also accepted
as an alias for `PATCH`.

### Disable and delete

```http
POST /v1/webhooks/{endpointId}/disable
DELETE /v1/webhooks/{endpointId}
```

Disable may also be performed by setting `"enabled": false` in an update.

### Rotate the signing secret

```http
POST /v1/webhooks/{endpointId}/rotate-secret
```

The replacement secret is returned only in this rotation response. `/rotate`
is supported as a shorter alias. After rotating, update the merchant's secret
store before processing deliveries signed with the previous secret.

---

## Overview

When a payment (or refund/dispute) transitions state, FluxaPay’s off-chain indexer delivers an HTTPS `POST` to your registered webhook URL. You should:

1. Verify the HMAC-SHA256 (or Ed25519) signature over the raw body
2. Claim the event's `event_id` and record its payment/refund/dispute id in one transaction with your side effect (see [Idempotency](#idempotency))
3. Return `2xx` quickly; queue slow work from an outbox row written in the same transaction

---

## Event types

### Payment lifecycle

| Webhook event | Trigger | Typical on-chain source |
|---------------|---------|-------------------------|
| `payment.created` | Charge created | `PAYMENT/CREATED` |
| `payment.pending` | Awaiting on-chain confirmation | payment still `Pending` |
| `payment.confirmed` | Deposit verified | `PAYMENT/CONFIRMED` / verify |
| `payment.expired` | Payment TTL elapsed before funding | `PAYMENT/EXPIRED` |
| `payment.failed` | Failed or invalid | failed status |
| `payment.settled` | Merchant settled | `PAYMENT/SETTLED` |

### Refund events (`REFUND/*`)

| Webhook event | Trigger | On-chain source |
|---------------|---------|-----------------|
| `refund.requested` | Refund filed | `REFUND/REQUESTED` |
| `refund.processed` | Refund completed | `REFUND/PROCESSED` / `COMPLETED` |
| `refund.rejected` | Refund rejected | `REFUND/REJECTED` |

### Dispute events (`DISPUTE/*`)

| Webhook event | Trigger | On-chain source |
|---------------|---------|-----------------|
| `dispute.created` | Dispute opened | `DISPUTE/CREATED` |
| `dispute.reviewed` | Moved under review | `DISPUTE/REVIEWED` |
| `dispute.resolved` | Resolution applied | `DISPUTE/RESOLVED` |
| `dispute.rejected` | Dispute rejected | `DISPUTE/REJECTED` |
| `dispute.escalated` | Deadline / escalation | `DISPUTE/ESCALATED` |
| `dispute.batch_created` | Bulk filing result | `DISPUTE/BATCH_CREATED` |

---

## Payload schema

All webhooks share a common envelope:

```json
{
  "id": "whd_0f1c2b3a-...",
  "event_id": "evt_9f1c2b3a-...",
  "type": "payment.confirmed",
  "createdAt": "2026-11-22T12:00:00.000Z",
  "livemode": true,
  "data": {
    "payment_id": "pay_abc123",
    "merchant_id": "G...",
    "amount": "10000000",
    "currency": "USDC",
    "status": "confirmed",
    "metadata": {
      "order_id": "ORD-9"
    }
  }
}
```

### Field reference

| Field | Type | Description |
|-------|------|-------------|
| `id` | string | Delivery id (`whd_...`). Also sent as `X-FluxaPay-Delivery`; stable when one delivery is retried |
| `event_id` | string | **Idempotency key** — opaque, stable across retries of the same event; use it as-is to deduplicate, never parse it |
| `type` | string | Event name (see tables above) |
| `createdAt` | string | ISO 8601 timestamp (UTC) |
| `livemode` | boolean | `false` for test deliveries from the dashboard; do not apply side effects |
| `replayed` | boolean | Present and `true` when the delivery was replayed from the delivery log |
| `data.payment_id` | string | Payment the event is about. Unique per payment, **not** per event |
| `data.refund_id` / `data.dispute_id` | string | Resource id on refund and dispute events |
| `data.merchant_id` | string | Merchant Stellar address |
| `data.amount` | string | Amount in minor units (string to avoid JSON number precision issues) |
| `data.currency` | string | e.g. `USDC` |
| `data.status` | string | Current status snapshot |
| `data.metadata` | object\|null | Merchant metadata from create |

### Payment expired payload (`payment.expired`)

```json
{
  "id": "whd_01HEXP1234567890",
  "event_id": "evt_2ab7d9e4-...",
  "type": "payment.expired",
  "createdAt": "2026-11-22T13:00:00.000Z",
  "livemode": true,
  "data": {
    "payment_id": "pay_abc123",
    "merchant_id": "GA7NQQNLFQC7OQF6...MERCHANT",
    "amount": "10000000",
    "currency": "USDC",
    "status": "expired",
    "expires_at": 1710003600
  }
}
```

### Refund payload extras

```json
{
  "type": "refund.processed",
  "data": {
    "refund_id": "ref_...",
    "payment_id": "pay_...",
    "amount": "5000000",
    "status": "completed"
  }
}
```

Deduplicate on `event_id`; store `refund_id` (falling back to `payment_id`) for correlation. Never dedup refunds on `payment_id` alone — one payment can have several refunds.

### Dispute payload extras

```json
{
  "type": "dispute.created",
  "data": {
    "dispute_id": "dsp_...",
    "payment_id": "pay_...",
    "amount": "10000000",
    "status": "open",
    "reason": "Item not received"
  }
}
```

Deduplicate on `event_id`; store `dispute_id` for correlation. One dispute emits several events (`dispute.opened`, `dispute.resolved`, …), so `dispute_id` alone would drop every event after the first.

---

## Webhook Signature Verification (HMAC-SHA256 & Ed25519)

FluxaPay supports two signing algorithms for webhooks:
- **HMAC-SHA256** (`hmac_sha256`, default): Uses a merchant-specific shared secret.
- **Ed25519** (`ed25519`): Platform signs raw payloads with a dedicated Ed25519 keypair. Fast to verify, compact, and natively aligns with Stellar keypairs.

The `X-FluxaPay-Signature` header is prefixed with the signing algorithm:
- `sha256=<hex_signature>`
- `ed25519=<hex_signature>`

Every request includes:

| Header | Description |
|--------|-------------|
| `X-FluxaPay-Signature` | Algorithm-prefixed signature (`sha256=...` or `ed25519=...`) |
| `X-FluxaPay-Timestamp` | Unix seconds when the webhook was delivered |
| `X-FluxaPay-Event` | Same as JSON `type` (convenience) |
| `X-FluxaPay-Delivery` | Unique delivery attempt ID |

---

### Algorithm 1: HMAC-SHA256

1. Read the **raw request body** (do not re-serialize JSON).
2. Strip prefix `sha256=` from `X-FluxaPay-Signature`.
3. Compute `HMAC-SHA256(webhook_secret, signed_payload)` where `signed_payload = ${timestamp}.${rawBody}`.
4. Compare using constant-time comparison (`crypto.timingSafeEqual`).
5. Reject if `|now - timestamp| > 300` seconds (replay window).

---

### Algorithm 2: Ed25519

1. Retrieve the platform's active Ed25519 public key from `GET /webhooks/public-key`.
2. Read the **raw request body**.
3. Strip prefix `ed25519=` from `X-FluxaPay-Signature`.
4. Verify the 64-byte Ed25519 signature over the raw payload buffer using the platform public key.
5. Check `X-FluxaPay-Timestamp` to ensure the delivery is within the 300-second window.

#### Active Public Key Endpoint

```http
GET /webhooks/public-key
```

Response:
```json
{
  "algorithm": "ed25519",
  "public_key": "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60"
}
```

---

### Key Rotation Procedure

To rotate the platform's Ed25519 signing key without downtime:
1. **Pre-publish new key**: Deploy the new public key alongside the existing active key in key management.
2. **Grace period**: Backends can cache the public key with a TTL (e.g., 1 hour). When a signature verification fails, backends should re-fetch `GET /webhooks/public-key`.
3. **Switch active key**: Set `WEBHOOK_ED25519_PRIVATE_KEY` and `WEBHOOK_ED25519_PUBLIC_KEY` in the indexer environment.
4. **Verification fallback**: Verifiers should fall back to querying `/webhooks/public-key` on cache misses.

---

### Verification with the FluxaPay SDK

The FluxaPay SDK provides `verifyWebhookSignature` which automatically detects the algorithm from the header prefix:

```typescript
import { verifyWebhookSignature } from "@fluxapay/sdk";

// Verifies either HMAC-SHA256 or Ed25519 seamlessly:
const isValid = verifyWebhookSignature(
  rawBodyString,
  req.headers["x-fluxapay-signature"],
  secretOrPublicKey,
);
```

## Retry policy

Each webhook endpoint has its own retry policy. If your endpoint does not return HTTP `2xx`, FluxaPay retries using that endpoint’s configured policy.

### Default policy

The default policy matches the historical fixed behaviour:

| Field | Default |
|-------|---------|
| `max_retries` | `3` |
| `initial_delay_ms` | `1000` |
| `backoff_multiplier` | `2.0` |
| `max_delay_ms` | `300000` |

With the defaults, delivery attempts are spaced as follows:

| Attempt | Delay before retry |
|---------|--------------------|
| 1 (initial) | immediate |
| 2 | ~1s |
| 3 | ~2s |
| 4 (final) | ~4s |

- **Max retries:** 3 retries after the first delivery (**4 total attempts**).
- **Backoff:** exponential (base ~1s, multiplier 2.0).
- After exhaustion, the event is marked failed; you can replay from the dashboard or indexer.

### Customizing the retry policy

Update an endpoint’s policy with `PATCH /v1/webhooks/{endpoint_id}`:

```json
{
  "max_retries": 10,
  "initial_delay_ms": 1000,
  "backoff_multiplier": 2.0,
  "max_delay_ms": 300000
}
```

| Field | Type | Constraints | Description |
|-------|------|-------------|-------------|
| `max_retries` | integer | `1`–`20` | Retries after the first delivery |
| `initial_delay_ms` | integer | `500`–`60000` | Delay before the first retry, in milliseconds |
| `backoff_multiplier` | number | `1.0`–`3.0` | Multiplier applied to the delay after each attempt |
| `max_delay_ms` | integer | `>= 1` | Upper bound on any single retry delay, in milliseconds |

Values outside these ranges are rejected with a descriptive `400` error. The delay before retry `n` is `min(initial_delay_ms * backoff_multiplier^(n-1), max_delay_ms)`.

Return `200` as soon as the event is durably queued; do heavy work out-of-band.

---

## Idempotency

Deliveries can repeat (retries, at-least-once semantics, manual replays), so a receiver must apply each event exactly once.

**Dedup key:** the envelope's `event_id`. It is opaque, unique per event and stable across retries, while `data.payment_id` is reused by every event about the same payment. Store the resource id beside it (`payment_id` for payments, `refund_id` for refunds, `dispute_id` for disputes) so you can trace, query and reconcile.

### Ordering: claim before you act

The safe order is to claim the key and apply the side effect in **one transaction**:

```
BEGIN
INSERT INTO processed_webhook_events (event_id, event_type, resource_id, received_at)
     VALUES (?, ?, ?, ?)
     ON CONFLICT (event_id) DO NOTHING
if row was not inserted:
    COMMIT
    return 200            -- duplicate: the effect is already committed
apply_side_effect(event)  -- same transaction
COMMIT
return 200
```

Why not "process, then mark processed"? A crash between those two steps leaves the side effect applied but the key unrecorded, so the retry applies it again. With the claim and the effect committed together:

| Crash point | Result | Delivery retry |
|-------------|--------|----------------|
| Before `COMMIT` | Claim and side effect both roll back | Processes normally |
| After `COMMIT`, before the `200` | Both are durable | Finds the claim, returns `200` without re-applying |

If your side effect lives in another system (email, ledger API), commit the claim first, then drain an outbox row written in the same transaction; make that downstream call idempotent on `event_id`.

Return `200` for duplicates — a non-`2xx` makes FluxaPay retry a delivery you have already handled. Store processed keys for at least **7 days** (longer than the retry window) and prune older rows.

The runnable examples below implement this flow; their tests are in [`docs/examples/webhooks/`](examples/webhooks/).

---

## Mapping on-chain → webhook

| On-chain `(namespace, action)` | Webhook `type` |
|--------------------------------|----------------|
| `PAYMENT/CREATED` | `payment.created` |
| (indexer pending state) | `payment.pending` |
| `PAYMENT/CONFIRMED` | `payment.confirmed` |
| `PAYMENT/EXPIRED` / failed | `payment.failed` |
| `PAYMENT/SETTLED` | `payment.settled` |
| `REFUND/REQUESTED` | `refund.requested` |
| `REFUND/PROCESSED` | `refund.processed` |
| `REFUND/REJECTED` | `refund.rejected` |
| `DISPUTE/CREATED` | `dispute.created` |
| `DISPUTE/REVIEWED` | `dispute.reviewed` |
| `DISPUTE/RESOLVED` | `dispute.resolved` |
| `DISPUTE/REJECTED` | `dispute.rejected` |
| `DISPUTE/ESCALATED` | `dispute.escalated` |
| `DISPUTE/BATCH_CREATED` | `dispute.batch_created` |

---

## Node.js (Express) example

Requires Node.js >= 22 for the built-in `node:sqlite` module (swap the two SQL
statements for `pg` / your database; the ordering is what matters). The complete
runnable file is [`docs/examples/webhooks/node-receiver.js`](examples/webhooks/node-receiver.js).

```javascript
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
```

Verify the duplicate path with the included test:

```bash
node --test docs/examples/webhooks/node-receiver.test.js
```

The test signs one event, delivers it twice and asserts your side effect ran
once, then repeats the check for a crash between the claim and the commit.

---

## Python (FastAPI) example

The complete runnable file is [`docs/examples/webhooks/python_receiver.py`](examples/webhooks/python_receiver.py).

```bash
pip install fastapi uvicorn
FLUXAPAY_WEBHOOK_SECRET=whsec_... uvicorn python_receiver:app --port 3000
```

```python
"""FluxaPay webhook receiver — idempotent reference implementation.

Verifies the HMAC-SHA256 signature, then records the event's stable
``event_id`` and applies the side effect in a single SQLite transaction.
A replayed delivery therefore either sees the committed event (and is
acknowledged as a duplicate) or sees nothing (and is processed fully).

Run:

    pip install fastapi uvicorn
    FLUXAPAY_WEBHOOK_SECRET=whsec_... uvicorn python_receiver:app --port 3000

For Postgres, keep the same ordering and replace the insert with
``INSERT ... ON CONFLICT (event_id) DO NOTHING`` inside one
``BEGIN ... COMMIT`` block.
"""

import hashlib
import hmac
import json
import os
import sqlite3
import time

from fastapi import FastAPI, Header, Request
from fastapi.responses import JSONResponse

TOLERANCE_SECONDS = 300
RETENTION_SECONDS = 7 * 24 * 60 * 60

SCHEMA = """
CREATE TABLE IF NOT EXISTS processed_webhook_events (
    event_id    TEXT PRIMARY KEY,
    event_type  TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    received_at INTEGER NOT NULL
);
"""


def connect(path: str = ":memory:") -> sqlite3.Connection:
    """Open a SQLite connection in autocommit mode for explicit transactions."""
    conn = sqlite3.connect(path, isolation_level=None, check_same_thread=False)
    conn.execute(SCHEMA)
    return conn


def verify_signature(
    secret: bytes,
    signature_header: str | None,
    timestamp_header: str | None,
    raw_body: bytes,
    now_seconds: int | None = None,
) -> bool:
    """Verify ``sha256=<hex>`` over ``<timestamp>.<raw_body>``."""
    if not signature_header or not signature_header.startswith("sha256="):
        return False
    if not timestamp_header:
        return False
    try:
        timestamp = int(timestamp_header)
    except ValueError:
        return False

    now = int(time.time()) if now_seconds is None else now_seconds
    if abs(now - timestamp) > TOLERANCE_SECONDS:
        return False

    expected = hmac.new(
        secret, f"{timestamp}.".encode() + raw_body, hashlib.sha256
    ).hexdigest()
    return hmac.compare_digest(expected, signature_header[len("sha256=") :])


def resource_id_of(data: dict) -> str | None:
    """The payment/refund/dispute the event is about, for correlation only."""
    return data.get("payment_id") or data.get("refund_id") or data.get("dispute_id")


class WebhookReceiver:
    """Signature check + atomic dedup + side effect.

    ``apply_event(conn, event)`` runs inside the claim transaction, so raising
    an exception undoes the claim too and FluxaPay's retry can process the
    event cleanly. It may be a regular function.
    """

    def __init__(self, conn: sqlite3.Connection, secret: bytes, apply_event):
        self.conn = conn
        self.secret = secret
        self.apply_event = apply_event

    def handle(
        self,
        raw_body: bytes,
        headers: dict[str, str],
        now_seconds: int | None = None,
    ) -> tuple[int, dict]:
        now = int(time.time()) if now_seconds is None else now_seconds
        lowered = {key.lower(): value for key, value in headers.items()}

        if not verify_signature(
            self.secret,
            lowered.get("x-fluxapay-signature"),
            lowered.get("x-fluxapay-timestamp"),
            raw_body,
            now,
        ):
            return 401, {"error": "invalid signature"}

        try:
            event = json.loads(raw_body)
        except ValueError:
            return 400, {"error": "invalid JSON"}

        # The envelope's ``event_id`` is stable across retries; ``id`` is the
        # delivery id and ``data.payment_id`` is not unique per event.
        event_id = event.get("event_id") if isinstance(event, dict) else None
        if not isinstance(event_id, str) or not event_id:
            return 400, {"error": "missing event_id"}

        data = event.get("data") or {}
        resource_id = resource_id_of(data)
        if not resource_id:
            return 400, {"error": "missing payment_id/refund_id/dispute_id"}

        # Ordering that survives a crash: claim and side effect commit together.
        # - Crash before COMMIT: both roll back and the retry processes normally.
        # - Crash after COMMIT: the retry finds the row and is acknowledged as a
        #   duplicate instead of applying the side effect twice.
        self.conn.execute("BEGIN IMMEDIATE")
        try:
            cursor = self.conn.execute(
                """
                INSERT INTO processed_webhook_events
                    (event_id, event_type, resource_id, received_at)
                VALUES (?, ?, ?, ?)
                ON CONFLICT (event_id) DO NOTHING
                """,
                (event_id, str(event.get("type", "")), str(resource_id), now),
            )
            if cursor.rowcount == 0:
                self.conn.execute("COMMIT")
                return 200, {"received": True, "duplicate": True}

            # Test deliveries are signed like real ones but must not move money.
            if event.get("livemode") is not False:
                self.apply_event(self.conn, event)

            self.conn.execute("COMMIT")
        except Exception:
            self.conn.execute("ROLLBACK")
            # Propagate so the HTTP layer answers 5xx and FluxaPay retries.
            raise

        self.conn.execute(
            "DELETE FROM processed_webhook_events WHERE received_at < ?",
            (now - RETENTION_SECONDS,),
        )
        return 200, {"received": True, "duplicate": False}


def mark_payment_confirmed(conn: sqlite3.Connection, event: dict) -> None:
    """Example side effect: mark the payment confirmed in your own tables."""
    if event.get("type") != "payment.confirmed":
        return
    data = event.get("data") or {}
    payment_id = data.get("payment_id")
    if not payment_id:
        return
    conn.execute(
        """
        INSERT INTO confirmed_payments
            (payment_id, amount, currency, confirmed_at)
        VALUES (?, ?, ?, ?)
        ON CONFLICT (payment_id) DO NOTHING
        """,
        (
            payment_id,
            str(data.get("amount", "")),
            str(data.get("currency", "")),
            str(event.get("createdAt", "")),
        ),
    )


def create_app(secret: bytes, conn: sqlite3.Connection) -> FastAPI:
    app = FastAPI()
    receiver = WebhookReceiver(conn, secret, mark_payment_confirmed)

    @app.post("/webhooks/fluxapay")
    async def fluxapay_webhook(
        request: Request,
        x_fluxapay_signature: str = Header(default=""),
        x_fluxapay_timestamp: str = Header(default=""),
    ):
        status, body = receiver.handle(
            await request.body(),
            {
                "x-fluxapay-signature": x_fluxapay_signature,
                "x-fluxapay-timestamp": x_fluxapay_timestamp,
            },
        )
        return JSONResponse(status_code=status, content=body)

    return app


def _default_app() -> FastAPI:
    secret = os.environ.get("FLUXAPAY_WEBHOOK_SECRET", "")
    if not secret:
        raise RuntimeError("FLUXAPAY_WEBHOOK_SECRET is required")
    conn = connect(os.environ.get("FLUXAPAY_DB", "webhooks.db"))
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS confirmed_payments (
            payment_id   TEXT PRIMARY KEY,
            amount       TEXT NOT NULL,
            currency     TEXT NOT NULL,
            confirmed_at TEXT NOT NULL
        )
        """
    )
    return create_app(secret.encode(), conn)


app = _default_app()
```

Verify the duplicate path with the included test:

```bash
cd docs/examples/webhooks && python3 -m unittest test_python_receiver -v
```

The test delivers the same signed payload twice and asserts the side effect ran
once, then checks that a failure inside the transaction rolls the claim back so
the retry can process the event.

---

## Testing webhooks locally

Use a tunnel (ngrok, cloudflared) to expose your local server, register the URL in the sandbox dashboard, and trigger events from the sandbox. Verify signatures with the sandbox secret before going live.
