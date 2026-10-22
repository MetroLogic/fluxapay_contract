# Sandbox Deployment Recipes

Quick developer setup for FluxaPay using Docker Compose templates (Issue #231).

## Prerequisites

- Docker Desktop or Docker Engine + Compose v2
- Stellar CLI (`stellar`) on the host for `scripts/sandbox-init.sh`
- Copy environment template: `cp .env.docker.example .env.docker`

## Local Standalone Sandbox

Runs a local Stellar/Soroban node via `stellar/quickstart`.

```bash
docker compose up -d
./scripts/sandbox-init.sh
```

Default endpoints:

| Service | URL |
|---------|-----|
| Soroban RPC | `http://localhost:8000/soroban/rpc` |
| Horizon | `http://localhost:8000` |
| Friendbot | `http://localhost:8000/friendbot?addr=<G-address>` |

Build WASM inside Docker (optional profile):

```bash
docker compose --profile build up contract-builder
```

## Testnet Devbox

Containerized Node.js workspace prewired for testnet RPC/HORIZON URLs.

```bash
docker compose -f docker-compose.testnet.yml up -d
docker compose -f docker-compose.testnet.yml exec devbox bash
cd sdk && npm install && npm run build
```

Run SDK build check profile:

```bash
docker compose -f docker-compose.testnet.yml --profile ci up sdk-check
```

## Environment Variables

See `.env.docker.example` for all supported variables. After deploying contracts, populate:

- `PAYMENT_PROCESSOR_ID`
- `MERCHANT_REGISTRY_ID`
- `FX_ORACLE_ID`
- `REFUND_MANAGER_ID`

Then follow invoke recipes in [`local-invoke.md`](./local-invoke.md).

## Tear Down

```bash
docker compose down -v
docker compose -f docker-compose.testnet.yml down -v
```
