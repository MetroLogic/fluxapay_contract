# FluxaPay Soroban Event Indexer

This directory contains the FluxaPay Soroban Event Indexer service that syncs events from all FluxaPay smart contracts into PostgreSQL and provides a read-only REST API.

## Features

- **Multi-Contract Event Indexing (#618)**: Subscribes simultaneously to events across all 5 FluxaPay contracts (`PaymentProcessor`, `RefundManager`, `MerchantRegistry`, `FXOracle`, `PaymentLinkManager`).
- **REST API Server (#616)**: HTTP API for querying payments, merchant payments (with pagination & status filter), disputes, refunds, events, and health status. Protected with API key authentication (`x-api-key` header or `Authorization: Bearer <API_KEY>`).
- **Dead-Letter Queue & Automatic Retry (#617)**: Persists failed events to `dead_letter_events` with error tracking and retry counters. Includes an automatic retry worker and an authenticated `POST /admin/replay-dlq` manual replay endpoint.
- **Dispute Lifecycle Persistence (#615)**: Persists dispute status transitions (`RESOLVED`, `REJECTED`, `ESCALATED`) with `resolved_at` timestamps, idempotency, and safe unknown dispute handling.

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
