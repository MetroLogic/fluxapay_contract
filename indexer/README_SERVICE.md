# FluxaPay Soroban Event Indexer & REST API

The FluxaPay Indexer is a Node.js/TypeScript service that subscribes to Soroban contract events from all FluxaPay contracts, persists them to PostgreSQL, handles failed events via a Dead-Letter Queue (DLQ), and exposes a read-only HTTP REST API.

## Overview

- **Multi-Contract Event Indexing (#618)**: Subscribes to events across all 5 FluxaPay contracts (`PaymentProcessor`, `RefundManager`, `MerchantRegistry`, `FXOracle`, `PaymentLinkManager`) simultaneously.
- **Dispute Lifecycle Persistence (#615)**: Persists dispute status transitions (`RESOLVED`, `REJECTED`, `ESCALATED`) with `resolved_at` timestamps, idempotency, and safe handling for unknown disputes.
- **Dead-Letter Queue & Automatic Retry (#617)**: Prevents event loss by storing failed events in `dead_letter_events`, auto-retrying on a configurable interval, and supporting manual replay via `POST /admin/replay-dlq`.
- **Read-Only REST API Server (#616)**: Exposes endpoints for querying payments, merchant payments (with pagination & status filtering), disputes, refunds, events, and health status.

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

All protected endpoints require an API key via header `x-api-key: <API_KEY>` or `Authorization: Bearer <API_KEY>`.

| Method | Endpoint | Auth Required | Description |
|--------|----------|---------------|-------------|
| `GET` | `/health` | No | Liveness and PostgreSQL connection check. Returns 200 `{ status: "healthy", database: "connected" }` or 503 if DB is down. |
| `GET` | `/payments/:paymentId` | Yes | Get payment record by payment ID. Returns 404 if not found. |
| `GET` | `/merchants/:merchantId/payments` | Yes | Query paginated payments for a merchant. Supports query parameters `page` (default 1), `limit` (default 20), and `status`. Returns useful pagination metadata. |
| `GET` | `/merchants/:merchantId/disputes` | Yes | Query disputes belonging to a merchant. Supports optional query parameter `status`. |
| `GET` | `/refunds/:refundId` | Yes | Get refund record by refund ID. Returns 404 if not found. |
| `GET` | `/events` | Yes | Query persisted events. Supports query parameters `type` (e.g. `PAYMENT/CONFIRMED`), `from` (ledger sequence), and `to` (ledger sequence). |
| `POST` | `/admin/replay-dlq` | Yes | Manually trigger replay of failed events in the dead-letter queue. Returns `{ attempted, succeeded, failed }`. |

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
