# Scripts

Operational scripts for deployment, SDK generation, local development, and CI checks.

## Script Catalog

| Script | Purpose | Required Env Vars | When to Run |
|--------|---------|-------------------|-------------|
| `deploy-testnet.sh` | Deploy all 6 contracts to testnet/mainnet in dependency order, initialize each, and write contract IDs to `.env.testnet`. Optionally seeds test data. | `STELLAR_SECRET_KEY`, `STELLAR_NETWORK` | After Rust contract changes |
| `deploy_testnet.sh` | Legacy deployment script — builds and deploys 5 contracts (no PaymentLinkManager). | `STELLAR_SECRET_KEY`, `STELLAR_NETWORK` | After Rust contract changes (prefer `deploy-testnet.sh`) |
| `fund-accounts.js` | Fund Stellar accounts via Friendbot (testnet) or local faucet (sandbox). Accepts addresses as CLI args or a `--config` JSON file. | None | Local dev setup |
| `subscription-daemon.js` | Long-running daemon that polls for due subscriptions and calls `process_due_subscriptions` on the RefundManager contract. | `CONTRACT_ID`, `OPERATOR_SECRET` | Always running in prod |
| `fx-oracle-updater.js` | Long-running updater that polls an off-chain price feed, converts quotes to 7-decimal fixed-point, and pushes them to the FXOracle contract via a single atomic `set_rates_batch` call. Skips the cycle (and alerts) on feed outage; logs + alerts on `RateDeviationExceeded`. Pass `--once` for a single cycle. | `ORACLE_SECRET`, `FX_ORACLE_CONTRACT_ID` | Always running in prod |
| `generate-sdk.sh` | Generate TypeScript SDK bindings from compiled WASM using `stellar contract bindings typescript`. | None | After contract changes |
| `sandbox-init.sh` | Bootstrap local Stellar sandbox — builds WASM, generates local identities, funds admin via friendbot. | `STELLAR_RPC_URL`, `STELLAR_NETWORK` | Local dev first-time setup |
| `check-mainnet-contract-ids.js` | CI informational check — warns when `sdk/src/network-profiles.ts` mainnet still has `UNSET_CONTRACT_ID` placeholders. Always exits 0. | None | CI |
| `check-error-map-sync.ts` | CI gate — verifies `FLUXAPAY_CONTRACT_ERROR_MAP` in `sdk/src/index.ts` matches the `Error` enum in `fluxapay/src/lib.rs`. Fails CI on drift. | None | CI |

## Environment Variables Reference

### `deploy-testnet.sh`

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `STELLAR_SECRET_KEY` | Yes | — | Deployer secret key (starts with `S`) |
| `STELLAR_NETWORK` | Yes | — | Target network: `testnet` or `mainnet` |
| `STELLAR_RPC_URL` | No | — | Override RPC endpoint |
| `SEED_DATA` | No | `false` | Set to `"true"` to seed test merchant, FX rate, and sample payment |
| `SKIP_BUILD` | No | `false` | Set to `"true"` to skip `cargo build` |

### `deploy_testnet.sh`

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `STELLAR_SECRET_KEY` | Yes | — | Deployer secret key |
| `STELLAR_NETWORK` | Yes | — | Target network (e.g. `testnet`) |

### `subscription-daemon.js`

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `STELLAR_RPC_URL` | No | `https://soroban-testnet.stellar.org` | Soroban RPC endpoint |
| `CONTRACT_ID` | Yes | — | RefundManager contract address |
| `OPERATOR_SECRET` | Yes | — | Operator secret key (`settlement_operator` role) |
| `POLL_INTERVAL_MS` | No | `60000` | Poll interval in milliseconds |
| `NETWORK_PASSPHRASE` | No | `Networks.TESTNET` | Stellar network passphrase |

### `fx-oracle-updater.js`

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `ORACLE_SECRET` | Yes | — | Oracle keypair secret key (`S...`), must hold the `ORACLE` role on the FXOracle contract |
| `FX_ORACLE_CONTRACT_ID` | Yes | — | FXOracle contract address (`C...`). `CONTRACT_ID` is accepted as a fallback |
| `PRICE_FEED_URL` | No | `https://api.exchangerate.host/latest?base=USD` | Price feed endpoint returning JSON |
| `PRICE_FEED_RATES_PATH` | No | `rates` | Dot path to the `{ code: rate }` object inside the feed response |
| `FX_PAIRS` | No | `EUR,BRL,USD` | Comma list of `SYMBOL[:feedKey]`. Stored `Symbol` = units of that currency per 1 USD |
| `FX_RATE_DECIMALS` | No | `7` | Fixed-point decimals for stored rates (contract convention is 7) |
| `UPDATE_INTERVAL_MS` | No | `60000` | Poll interval in milliseconds |
| `MAX_FEED_RETRIES` | No | `3` | Feed fetch attempts per cycle before the cycle is skipped + alerted |
| `FEED_RETRY_BACKOFF_MS` | No | `2000` | Base linear backoff between feed retries |
| `STELLAR_RPC_URL` | No | `https://soroban-testnet.stellar.org` | Soroban RPC endpoint |
| `NETWORK_PASSPHRASE` | No | Testnet | Stellar network passphrase |
| `ALERT_WEBHOOK_URL` | No | — | Optional. Receives a JSON `POST` on every alert (feed outage, batch rejection, …) |

**Failure handling**

* **Feed outage** — after `MAX_FEED_RETRIES` failed fetches the updater pushes
  *nothing* and emits an `alert` line (`price_feed_unavailable`). The contract
  keeps its last good rates and its own staleness guard (`get_rate` →
  `RateStale`) protects downstream settlement.
* **Deviation rejection** — if the contract rejects the batch with
  `FXOracleError::RateDeviationExceeded` (#5), the updater logs and alerts
  `batch_rejected` with the offending rates and continues the loop (an operator
  must widen `set_rate_deviation_limit` or investigate the feed).
* **Structured logs** — every line on stdout is a single JSON object
  (`{ ts, level, event, ... }`); `level: "alert"` marks an operational incident.

### `fund-accounts.js`

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `FAUCET_URL` | No | `https://friendbot.stellar.org` | Faucet endpoint URL |

### `sandbox-init.sh`

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `STELLAR_RPC_URL` | Yes | — | From `.env.docker` |
| `STELLAR_NETWORK` | Yes | — | From `.env.docker` |

## Running

All scripts should be executed from the repository root:

```bash
# Deploy to testnet
STELLAR_SECRET_KEY=S... STELLAR_NETWORK=testnet bash scripts/deploy-testnet.sh

# Fund accounts
npx node scripts/fund-accounts.js GXXXXXX GYYYYYY

# Generate SDK bindings
bash scripts/generate-sdk.sh

# Bootstrap local sandbox
bash scripts/sandbox-init.sh

# Start subscription daemon
CONTRACT_ID=... OPERATOR_SECRET=S... node scripts/subscription-daemon.js

# Start the FX oracle updater (one cycle)
ORACLE_SECRET=S... FX_ORACLE_CONTRACT_ID=C... node scripts/fx-oracle-updater.js --once

# Start the FX oracle updater (long-running, every 60s)
ORACLE_SECRET=S... FX_ORACLE_CONTRACT_ID=C... node scripts/fx-oracle-updater.js

# CI checks (run automatically in CI, or manually)
npx tsx scripts/check-error-map-sync.ts
node scripts/check-mainnet-contract-ids.js
```
