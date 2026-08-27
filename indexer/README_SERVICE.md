# FluxaPay Soroban Event Indexer

The FluxaPay Indexer is a Node.js/TypeScript service that subscribes to Soroban contract events and persists them to a PostgreSQL database for the merchant dashboard.

## Overview

The indexer:
- Subscribes to all contract events via stellar-sdk EventSubscription API
- Handles event types: PAYMENT/, REFUND/, DISPUTE/, MERCHANT/, STREAM/, SUBSCRIPTION/, INVOICE/
- Writes events to PostgreSQL with schema matching event data
- Implements at-least-once delivery with idempotency (event_id dedup)
- Provides REST API for querying indexed events
- Runs in Docker Compose for local development

## Architecture

```
Soroban Contract → RPC Endpoint → Indexer Service → PostgreSQL
                                      ↓
                               Merchant Dashboard
```

The service polls the Soroban RPC endpoint for new events every 5 seconds (configurable), deduplicates using `event_id` (ledger-txHash-eventIndex), and stores typed events in separate tables.

## Setup

### Prerequisites
- Node.js 18+
- Docker & Docker Compose (optional, recommended)
- PostgreSQL 13+ (if not using Docker)
- Stellar CLI (for contract deployment)

### Local Development (with Docker)

1. Clone and navigate to the indexer directory:
```bash
cd indexer
```

2. Copy environment file:
```bash
cp .env.example .env
```

3. Update `.env` with your contract ID:
```bash
# .env
FLUXAPAY_CONTRACT_ID=<your-contract-id>
SOROBAN_RPC_URL=http://localhost:8000/soroban/rpc  # for local sandbox
```

4. Start services:
```bash
docker compose up -d
```

5. Check logs:
```bash
docker compose logs -f indexer
```

### Local Development (without Docker)

1. Install dependencies:
```bash
npm install
```

2. Copy environment file and configure:
```bash
cp .env.example .env
```

3. Create PostgreSQL database:
```bash
createdb fluxapay
```

4. Run migrations:
```bash
npm run migrate
```

5. Start in development mode:
```bash
npm run dev
```

## Configuration

Environment variables (see `.env.example`):

| Variable | Default | Description |
|----------|---------|-------------|
| `SOROBAN_RPC_URL` | `http://localhost:8000/soroban/rpc` | Soroban RPC endpoint |
| `FLUXAPAY_CONTRACT_ID` | Required | Deployed contract ID |
| `DATABASE_URL` | `postgres://postgres:password@localhost:5432/fluxapay` | PostgreSQL connection string |
| `POLL_INTERVAL_MS` | 5000 | Event polling interval in milliseconds |
| `START_LEDGER` | 1 | Starting ledger for initial sync |

## Database Schema

### Core Tables

#### contract_events (dedup table)
Stores all contract events for idempotency tracking:
- `event_id`: Unique identifier (ledger-txHash-index)
- `event_type`: PAYMENT, REFUND, DISPUTE, MERCHANT, STREAM, SUBSCRIPTION, INVOICE
- `ledger`: Ledger sequence number
- `tx_hash`: Transaction hash
- `data`: JSON payload

#### typed event tables
- `payments`: Payment events (payment_id, merchant_id, amount, status)
- `refunds`: Refund events (refund_id, payment_id, amount, status)
- `disputes`: Dispute events (dispute_id, payment_id, amount, status)
- `merchants`: Merchant events (merchant_id, status)
- `streams`: Stream events (stream_id, sender, receiver, amount, status)
- `subscriptions`: Subscription events (subscription_id, payer, status)
- `invoices`: Invoice events (invoice_id, merchant_id, total_amount, status)

#### indexer_checkpoint
Tracks sync progress:
- `contract_id`: Contract being indexed
- `last_processed_ledger`: Most recent ledger processed

## Event Flow

### 1. Event Subscription
```typescript
// Subscribes to contract events via stellar-sdk
const request: GetEventsRequest = {
  filters: [{ type: "contract", contractIds: [contractId] }],
  startLedger: currentLedger,
  limit: 100,
};
```

### 2. Event Deduplication
```sql
-- Check if event_id already exists
SELECT id FROM contract_events WHERE event_id = ?

-- Insert with ON CONFLICT to handle race conditions
INSERT INTO contract_events (event_id, ...) VALUES (?)
ON CONFLICT (event_id) DO NOTHING
```

### 3. Event Persistence
Events are parsed and routed to type-specific tables with idempotent upserts:
```sql
INSERT INTO payments (payment_id, merchant_id, amount, ...)
VALUES (?, ?, ?, ...)
ON CONFLICT (payment_id) DO UPDATE SET status = ?
```

## Running in Production

### Docker Compose (Recommended)

1. Build and push image:
```bash
docker build -t fluxapay-indexer:latest .
docker tag fluxapay-indexer:latest registry.example.com/fluxapay-indexer:latest
docker push registry.example.com/fluxapay-indexer:latest
```

2. Use environment-specific docker-compose:
```bash
# For testnet
FLUXAPAY_CONTRACT_ID=<testnet-id> \
SOROBAN_RPC_URL=https://soroban-testnet.stellar.org \
DATABASE_URL=postgres://user:pass@prod-db:5432/fluxapay_testnet \
docker compose up -d
```

### Kubernetes (Alternative)

```bash
kubectl apply -f k8s/indexer-deployment.yaml
kubectl logs -f deployment/fluxapay-indexer
```

### Manual Systemd Service

```ini
# /etc/systemd/system/fluxapay-indexer.service
[Unit]
Description=FluxaPay Indexer
After=network.target postgres.service

[Service]
Type=simple
User=fluxapay
WorkingDirectory=/opt/fluxapay-indexer
EnvironmentFile=/etc/fluxapay-indexer.env
ExecStart=/usr/bin/node dist/index.js
Restart=on-failure
RestartSec=10s

[Install]
WantedBy=multi-user.target
```

## Monitoring and Maintenance

### Check Service Status
```bash
docker compose ps
docker compose logs indexer
```

### Reset Event Sync
To restart event sync from a specific ledger:
```bash
docker compose exec postgres psql -U postgres fluxapay
DELETE FROM contract_events WHERE ledger > 12345;
DELETE FROM indexer_checkpoint;
```

### Performance Tuning
- Increase `POLL_INTERVAL_MS` if RPC is rate-limited
- Batch size is configured in code (default: 100 events/poll)
- Add database indexes for frequently queried fields

## Troubleshooting

### Connection Issues
```bash
# Test database connection
docker compose exec postgres psql -U postgres -d fluxapay -c "SELECT 1"

# Test RPC connection
curl -X POST http://localhost:8000/soroban/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getLatestLedger","id":1}'
```

### Event Sync Failures
- Check logs: `docker compose logs indexer`
- Verify contract ID is correct in environment
- Ensure RPC endpoint is accessible
- Check database permissions

### Missing Events
- Verify indexer is running: `docker compose ps`
- Check `last_processed_ledger` in `indexer_checkpoint` table
- For recovery, delete records for affected ledgers and restart

## Development

### Build
```bash
npm run build
```

### Watch Mode
```bash
npm run watch
```

### Test
```bash
npm test
```

### Code Structure
```
src/
  index.ts           # Main event subscriber
  types.ts           # Event type definitions
  database.ts        # PostgreSQL adapter
  migrations/
    run.ts           # Migration runner
migrations/
  001_initial_schema.sql  # Initial database schema
```

## Authentication

The REST API (`src/server.ts`) requires a SEP-10 (Stellar Web Authentication)
JWT on every request except `GET /health`:

```
Authorization: Bearer <jwt>
```

Clients obtain the JWT by completing the SEP-10 challenge/response flow
against `STELLAR_WEB_AUTH_ENDPOINT` (see `SEP10Authenticator` in
`sdk/src/sep10.ts` for the client-side implementation). The JWT's `sub`
claim is the authenticated Stellar account.

| Variable | Default | Description |
|----------|---------|-------------|
| `STELLAR_WEB_AUTH_ENDPOINT` | `https://testanchor.stellar.org/auth` | SEP-10 challenge endpoint |
| `STELLAR_HOME_DOMAIN` | `fluxapay.stellar.org` | Home domain asserted in the SEP-10 challenge |
| `STELLAR_SERVER_PUBLIC_KEY` | _(required)_ | Stellar account used to verify issued JWTs |
| `INDEXER_ADMIN_ACCOUNTS` | _(empty)_ | Comma-separated Stellar accounts allowed on `/admin/*` |
| `INDEXER_API_PORT` | `3001` | Port for `npm run dev:api` / `start:api` |

### Endpoints

| Endpoint | Auth | Notes |
|----------|------|-------|
| `GET /health` | none | Liveness check |
| `GET /merchants/:merchantId/payments` | SEP-10, merchant-scoped | 403 if JWT `sub` ≠ `:merchantId` and caller isn't an admin |
| `GET /merchants/:merchantId/refunds` | SEP-10, merchant-scoped | Same scoping as above |
| `GET /merchants/:merchantId/disputes` | SEP-10, merchant-scoped | Same scoping as above |
| `GET /admin/stats` | SEP-10, admin-only | 403 unless caller is in `INDEXER_ADMIN_ACCOUNTS` |

Run it standalone:
```bash
npm run dev:api    # ts-node, for local development
npm run start:api  # compiled dist/server.js
```

## API (Future)

The merchant dashboard connects to this indexer. Query examples:

```sql
-- Get payment events for a merchant
SELECT * FROM payments
WHERE merchant_id = 'merchant_123'
AND created_at > NOW() - INTERVAL '30 days'
ORDER BY created_at DESC;

-- Get invoice status
SELECT status, COUNT(*) FROM invoices
WHERE merchant_id = 'merchant_123'
GROUP BY status;

-- Track refund patterns
SELECT SUM(amount) as total_refunded FROM refunds
WHERE payment_id IN (
  SELECT payment_id FROM payments
  WHERE merchant_id = 'merchant_123'
)
```

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for guidelines.

## License

MIT - See [LICENSE](../LICENSE)
