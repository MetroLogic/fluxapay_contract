# FluxaPay Soroban Event Indexer

This directory contains the FluxaPay Soroban Event Indexer service that syncs events from all FluxaPay smart contracts into PostgreSQL and provides a read-only REST API.

## Features

- **Multi-Contract Event Indexing (#618)**: Subscribes simultaneously to events across all 5 FluxaPay contracts (`PaymentProcessor`, `RefundManager`, `MerchantRegistry`, `FXOracle`, `PaymentLinkManager`).
- **REST API Server (#616)**: HTTP API for querying payments, merchant payments (with pagination & status filter), disputes, refunds, events, and health status. Protected with API key authentication (`x-api-key` header or `Authorization: Bearer <API_KEY>`).
- **Merchant Onboarding Checklist (#1074)**: SEP-10-authenticated checklist endpoint derived from indexed merchant registration and verification data.
- **Reconciliation Report Export (#1081)**: Merchant-scoped date-range reports with settlement, refund, dispute adjustment, fiat conversion, and CSV output.
- **Dead-Letter Queue & Automatic Retry (#617)**: Persists failed events to `dead_letter_events` with error tracking and retry counters. Includes an automatic retry worker and an authenticated `POST /admin/replay-dlq` manual replay endpoint.
- **Dispute Lifecycle Persistence (#615)**: Persists dispute status transitions (`RESOLVED`, `REJECTED`, `ESCALATED`) with `resolved_at` timestamps, idempotency, and safe unknown dispute handling.
- **Reconciliation CSV Export (#840)**: `GET /v1/reports/reconciliation?from=&to=` streams a CSV of payments, refunds, and settlements for accounting tools (QuickBooks, Wave, Excel). Authenticated via SEP-10 JWT; large reports use a PostgreSQL cursor so rows are not buffered in memory.
- **Drizzle ORM (#842)**: All queries in `src/database.ts` go through Drizzle against the typed schema in `src/schema.ts`, so column/type mismatches fail at TypeScript compile time.

## Drizzle ORM setup (Issue #842)

The indexer uses [Drizzle ORM](https://orm.drizzle.team/) on top of `pg`:

1. Schema lives in `src/schema.ts` and must stay aligned with `migrations/*.sql`.
2. Runtime queries use the Drizzle query builder (`drizzle-orm/node-postgres`).
3. Historical SQL migrations under `migrations/` are still applied with `npm run migrate` (sequentially numbered; do not rewrite applied files).
4. Optional local helpers:
   ```bash
   npm run drizzle:generate   # emit drafts from schema.ts into ./drizzle
   npm run drizzle:studio     # browse the DB
   ```
5. After pulling schema changes, run `npm install` so `drizzle-orm` / `drizzle-kit` are present, then `npm run build`.

## Reconciliation CSV (`GET /v1/reports/reconciliation`)

```bash
curl -H "Authorization: Bearer <SEP10_JWT>" \
  -H "Accept: text/csv" \
  "http://localhost:3000/v1/reports/reconciliation?from=2026-09-01&to=2026-09-30" \
  -o reconciliation_2026-09.csv
```

Headers:
- `Content-Type: text/csv`
- `Content-Disposition: attachment; filename=reconciliation_2026-09.csv`

Columns: `payment_id,type,amount_usdc,fiat_amount,fiat_currency,status,created_at,settled_at,customer_ref,tags`

## Real-Time SSE Stream (`GET /v1/events/stream`)

Subscribe to real-time events for a merchant dashboard:

```bash
curl -N -H "Authorization: Bearer <JWT_OR_API_KEY>" \
  "http://localhost:3000/v1/events/stream?merchant_id=G_MERCHANT&event_types=payment.confirmed,payment.failed"
```

Headers returned:
- `Content-Type: text/event-stream`
- `Cache-Control: no-cache`
- `Connection: keep-alive`

Keepalive pings (`event: ping`) are dispatched every 30 seconds.

## Quick Start

1. Install dependencies:
   ```bash
   npm install
   ```

2. Configure environment:
   ```bash
   cp .env.example .env
   ```

3. Run migrations:
   ```bash
   npm run migrate
   ```

4. Start indexer & REST API:
   ```bash
   npm start
   ```

5. Run test suite:
   ```bash
   npm test
   ```

See [`README_SERVICE.md`](./README_SERVICE.md) for full configuration, API endpoint documentation, DLQ operations, and multi-contract routing details.
