# FluxaPay Soroban Event Indexer & REST API

The FluxaPay Indexer is a Node.js/TypeScript service that subscribes to Soroban contract events from all FluxaPay contracts, persists them to PostgreSQL, handles failed events via a Dead-Letter Queue (DLQ), and exposes a read-only HTTP REST API.

## Overview

- **Multi-Contract Event Indexing (#618)**: Subscribes to events across all 5 FluxaPay contracts (`PaymentProcessor`, `RefundManager`, `MerchantRegistry`, `FXOracle`, `PaymentLinkManager`) simultaneously.
- **Dispute Lifecycle Persistence (#615)**: Persists dispute status transitions (`RESOLVED`, `REJECTED`, `ESCALATED`) with `resolved_at` timestamps, idempotency, and safe handling for unknown disputes.
- **Dead-Letter Queue & Automatic Retry (#617)**: Prevents event loss by storing failed events in `dead_letter_events`, auto-retrying on a configurable interval, and supporting manual replay via `POST /admin/replay-dlq`.
- **Read-Only REST API Server (#616)**: Exposes endpoints for querying payments, merchant payments (with pagination & status filtering), disputes, refunds, events, and health status.
- **Payment Link Analytics (#1082)**: Tracks checkout views and indexed link-use payments, with owner-scoped analytics and a seven-day conversion sparkline.

---

## Configuration

Environment variables (see `.env.example`):

| Variable | Default | Description |
|----------|---------|-------------|
| `SOROBAN_RPC_URL` | `http://localhost:8000/soroban/rpc` | Soroban RPC endpoint |
| `PAYMENT_PROCESSOR_CONTRACT_ID` | _(optional)_ | PaymentProcessor contract ID |
| `REFUND_MANAGER_CONTRACT_ID` | _(optional)_ | RefundManager contract ID |
| `MERCHANT_REGISTRY_CONTRACT_ID` | _(optional)_ | MerchantRegistry contract ID |
| `FX_ORACLE_CONTRACT_ID` | _(optional)_ | FXOracle contract ID |
| `PAYMENT_LINK_MANAGER_CONTRACT_ID` | _(optional)_ | PaymentLinkManager contract ID |
| `CONTRACT_IDS` | _(optional)_ | Comma-separated contract IDs |
| `FLUXAPAY_CONTRACT_ID` | _(optional)_ | Legacy single contract ID fallback |
| `DATABASE_URL` | `postgres://postgres:password@localhost:5432/fluxapay` | PostgreSQL connection string |
| `POLL_INTERVAL_MS` | `5000` | Event polling interval in milliseconds |
| `START_LEDGER` | `1` | Starting ledger sequence |
| `PORT` / `INDEXER_API_PORT` | `3001` | REST API HTTP server port |
| `API_KEY` / `INDEXER_API_KEY` | _(required in prod)_ | Secret key for REST API authentication |
| `MERCHANT_API_KEYS` | _(required for reconciliation reports)_ | JSON object mapping each merchant API key to its merchant ID |
| `STELLAR_SERVER_PUBLIC_KEY` | _(required for onboarding endpoint)_ | SEP-10 JWT verification key |
| `DLQ_RETRY_INTERVAL_MS` | `60000` | DLQ automatic retry interval in milliseconds |
| `DLQ_MIN_AGE_SECONDS` | `60` | Minimum age in seconds before a DLQ record is retried |

---

## Multi-Contract Event Routing (#618)

The subscriber replaces single contract tracking with an array of `contractIds`. The subscriber polls for events originating from any configured contract and attaches `contractId` to the parsed event before writing to PostgreSQL.

Supported contract event routing:
- `PaymentProcessor` $\rightarrow$ `PAYMENT/`
- `RefundManager` $\rightarrow$ `REFUND/`
- `MerchantRegistry` $\rightarrow$ `MERCHANT/`
- `FXOracle` $\rightarrow$ `FX_ORACLE/` or `ORACLE/`
- `PaymentLinkManager` $\rightarrow$ `PAYMENT_LINK/` or `INVOICE/`

---

## Dispute Status Lifecycle (#615)

Disputes are tracked through their full lifecycle:
- `DISPUTE/CREATED`: Inserts dispute with status `'Open'`, `escalated = false`.
- `DISPUTE/RESOLVED`: Updates status to `'Resolved'` and sets `resolved_at` to the event timestamp.
- `DISPUTE/REJECTED`: Updates status to `'Rejected'`.
- `DISPUTE/ESCALATED`: Sets `escalated = true` and updates status to `'Escalated'`.

If an event references an unknown dispute ID, the indexer logs a warning without crashing or rolling back unrelated events. Processing duplicate events is idempotent.

---

## Dead-Letter Queue & Retry (#617)

When event processing fails (e.g. malformed JSON, temporary DB error), the raw event is stored in `dead_letter_events`:

### `dead_letter_events` Schema
- `id`: `SERIAL PRIMARY KEY`
- `event_id`: `VARCHAR(255) UNIQUE NOT NULL`
- `raw_data`: `JSONB NOT NULL`
- `error`: `TEXT NOT NULL`
- `created_at`: `TIMESTAMP DEFAULT CURRENT_TIMESTAMP`
- `retry_count`: `INTEGER DEFAULT 0`

### Replay Operations
- **Automatic Retry**: Background worker polls `dead_letter_events` every `DLQ_RETRY_INTERVAL_MS` and retries records older than `DLQ_MIN_AGE_SECONDS`. On success, the DLQ record is removed; on failure, `retry_count` is incremented.
- **Manual Replay**: Endpoint `POST /admin/replay-dlq` triggers immediate replay of DLQ events and returns `{ attempted, succeeded, failed }`.

---

## REST API Endpoints (#616)

Protected endpoints use an API key via `x-api-key` or a bearer token, except `GET /v1/merchants/me/onboarding`, which requires a SEP-10 bearer JWT. Reconciliation reports use `MERCHANT_API_KEYS` and are scoped to the merchant mapped to the presented key.

| Method | Endpoint | Auth Required | Description |
|--------|----------|---------------|-------------|
| `GET` | `/health` | No | Liveness and PostgreSQL connection check. Returns 200 `{ status: "healthy", database: "connected" }` or 503 if DB is down. |
| `GET` | `/v1/merchants/me/onboarding` | SEP-10 bearer JWT | Returns six merchant setup steps and `completion_pct`. Uses the token's `merchant_id` claim, falling back to `sub`. |
| `GET` | `/v1/reports/reconciliation?from=YYYY-MM-DD&to=YYYY-MM-DD` | Merchant API key | Returns merchant-scoped payment, settlement, refund, dispute adjustment, and FX equivalent data. The inclusive date range is limited to 31 days. Set `Accept: text/csv` for CSV output. An optional `merchant_id` query must match the merchant assigned to the key. |
| `GET` | `/payments/:paymentId` | Yes | Get payment record by payment ID. Returns 404 if not found. |
| `GET` | `/merchants/:merchantId/payments` | Yes | Query paginated payments for a merchant. Supports query parameters `page` (default 1), `limit` (default 20), and `status`. Returns useful pagination metadata. |
| `GET` | `/merchants/:merchantId/disputes` | Yes | Query disputes belonging to a merchant. Supports optional query parameter `status`. |
| `GET` | `/refunds/:refundId` | Yes | Get refund record by refund ID. Returns 404 if not found. |
| `GET` | `/events` | Yes | Query persisted events. Supports query parameters `type` (e.g. `PAYMENT/CONFIRMED`), `from` (ledger sequence), and `to` (ledger sequence). |
| `POST` | `/admin/replay-dlq` | Yes | Manually trigger replay of failed events in the dead-letter queue. Returns `{ attempted, succeeded, failed }`. |
| `POST` | `/admin/replay` | Yes (Admin) | Replay and backfill missed contract events from a ledger range (`from_ledger` to `to_ledger`). Progress streamed as SSE. |
| `POST` | `/v1/payment-links/:id/view` | No | Record a checkout-page view for an indexed link. Returns 404 for unknown links. |
| `GET` | `/v1/payment-links` | SEP-10 merchant | List the authenticated merchant's payment links and their seven-day conversion counts. |
| `GET` | `/v1/payment-links/:id/analytics` | SEP-10 merchant | Return views, attempts, conversions, conversion rate, revenue, and last use; links owned by other merchants return 404. |
| `GET` | `/v1/subscriptions/metrics?merchant_id=<id>&period=YYYY-MM` | Merchant API key (`read:analytics`) | Return monthly recurring revenue, subscriber activity, churn, trial conversions, past-due count, and dunning recovery rate. |

The checkout page should call `POST /v1/payment-links/{id}/view` once when
the link checkout loads. `LINK/USED` contract events associate the resulting
payment with the link, while confirmed/settled payment events drive conversion
and USDC revenue totals. The analytics/list endpoints use the authenticated
SEP-10 subject to scope every query to its owner.

Subscription metrics are available for a calendar month in UTC. The API key
must be registered to the requested merchant (`merchantId` in `API_KEYS` or
the key registry) and include `read:analytics`; admin-scoped keys may query
any merchant. The dashboard reads the merchant-bound key and merchant ID from
`localStorage` keys `fluxapay_api_key` and `fluxapay_merchant_id`. Results are
cached for five minutes. Indexing subscription plans and lifecycle events
requires applying
`migrations/012_subscription_metrics.sql`.

---

## Ledger Event Replay & Backfill (#858)

When the indexer experiences downtime (deployments, node restarts, outages), Stellar contract events emitted during that period can be backfilled using the admin replay endpoint:

### Replay Procedure

1. **Authenticate**: Send `x-admin-api-key: <ADMIN_API_KEY>` (or `Authorization: Bearer <ADMIN_API_KEY>`).
2. **Execute Replay**:
   ```bash
   curl -N -X POST "http://localhost:3001/admin/replay?from_ledger=100000&to_ledger=105000" \
     -H "x-admin-api-key: $ADMIN_API_KEY"
   ```
3. **Stream Progress**: The endpoint streams Server-Sent Events (SSE) reporting real-time progress:
   ```
   data: {"processed":50,"total":5001,"stored":12}
   ...
   data: {"type":"complete","processed":120,"total":5001,"stored":35}
   ```
4. **Safety & Invariants**:
   - **Range Limit**: Capped at a maximum range of 10,000 ledgers (`MAX_REPLAY_LEDGER_RANGE`) per request to prevent runaway queries.
   - **Idempotency**: Events already present in the database are skipped using `ON CONFLICT (event_id) DO NOTHING`.
   - **Non-Destructive**: Existing rows and records are never overwritten or altered; only missing entries are inserted.

### Merchant onboarding checklist

`completion_pct` is the nearest integer percentage of all six steps, including
`merchant_registered` (one completed step is 17%). Registration is read from
the indexed `merchants` table. KYC is considered complete after an indexed
`MERCHANT` event reports `status: "VERIFIED"`; its completion timestamp comes
from that event where available.

Webhook configuration, payout channels, and testnet/mainnet payment
distinctions are not represented in the current indexer schema. Those steps
therefore remain incomplete and include their `action_url`; the endpoint
cannot report 100% until those state sources are integrated. This repository
also has no dashboard application, so progress-bar and auto-dismiss behavior
must be implemented in the dashboard project.

### Reconciliation report export (#1081)

Configure `MERCHANT_API_KEYS` as a JSON object whose keys are unique API secrets
and whose values are merchant IDs, for example
`{"merchant-secret":"G...merchant-id"}`. The report endpoint does not use the
global indexer API key and rejects a requested `merchant_id` that does not
match the merchant assigned to the presented key.

Migration `009_reconciliation_reports.sql` adds payment reconciliation columns,
the `fx_rate_cache`, and dispute adjustment storage. The FXOracle emits
`FX_RATE/UPDATED` events containing each updated pair, integer fixed-point rate,
and decimals; the indexer mirrors the latest rate into `fx_rate_cache`.
The PaymentProcessor emits `PAYMENT/FEE_TOTAL` during settlement, which the
indexer uses for the fee breakdown and settlement timestamp. Payment-specific
fiat amounts, currencies, customer references, and tags are retained when
supplied by indexed payment event data. Token amounts use the contract's
seven-decimal convention; report totals are displayed to two decimal places.
Dispute adjustments are signed and represent refunds caused by lost disputes;
they are already included in the refund total and are not subtracted a second
time when calculating net settlement.

---

## Development & Testing

Run unit & integration tests:
```bash
npm test
```

Build TypeScript code:
```bash
npm run build
```

Run database migrations:
```bash
npm run migrate
```
