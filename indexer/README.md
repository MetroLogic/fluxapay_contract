# Mercury Indexer Configurations

This directory contains the configurations for the Mercury Indexer to sync FluxaPay smart contract events and data to the database.

## Configs
- `sync.yml`: Defines the DB sync configuration profiles (e.g., testnet_quick_sync, mainnet_full_sync, sandbox_local) and supports quick mappings for events to DB tables.

## Quick Mappings
Quick mappings allow mapping contract events to database tables automatically.
Enable `enable_quick_mappings` in the settings to use this feature.

## REST API Authentication (Issue #672)

The indexer's REST API (`src/server.ts`, run via `npm run dev:api` /
`npm run start:api`) authenticates requests with SEP-10 (Stellar Web
Authentication) JWTs instead of a static API key — see
[`README_SERVICE.md`](./README_SERVICE.md#authentication) for the full
endpoint list and configuration.

Quick summary:
- Clients authenticate against `STELLAR_WEB_AUTH_ENDPOINT` (SEP-10 challenge
  flow implemented in `sdk/src/sep10.ts`) to obtain a JWT, then send
  `Authorization: Bearer <jwt>` on every request.
- `GET /merchants/:merchantId/*` endpoints only return data for the
  merchant the JWT's `sub` claim matches (or an admin account).
- `GET /admin/*` endpoints require the caller's account to be listed in
  `INDEXER_ADMIN_ACCOUNTS`.
- Configure via `STELLAR_WEB_AUTH_ENDPOINT`, `STELLAR_HOME_DOMAIN`,
  `STELLAR_SERVER_PUBLIC_KEY`, and `INDEXER_ADMIN_ACCOUNTS` (see
  `.env.example`).
