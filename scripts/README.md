# FluxaPay Deployment Scripts

## `deploy.sh`

Deploys all six FluxaPay contracts to a Stellar network in dependency order,
initialises each contract with the deployer as admin, and writes the resulting
contract IDs to `.env.testnet`.

### Usage
# Scripts

Operational scripts for deployment, SDK generation, local development, and CI checks.

## Script Catalog

| Script | Purpose | Required Env Vars | When to Run |
|--------|---------|-------------------|-------------|
| `deploy-testnet.sh` | Deploy all 6 contracts to testnet/mainnet in dependency order, initialize each, and write contract IDs to `.env.testnet`. Optionally seeds test data. | `STELLAR_SECRET_KEY`, `STELLAR_NETWORK` | After Rust contract changes |
| `deploy_testnet.sh` | Legacy deployment script — builds and deploys 5 contracts (no PaymentLinkManager). | `STELLAR_SECRET_KEY`, `STELLAR_NETWORK` | After Rust contract changes (prefer `deploy-testnet.sh`) |
| `fund-accounts.js` | Fund Stellar accounts via Friendbot (testnet) or local faucet (sandbox). Accepts addresses as CLI args or a `--config` JSON file. | None | Local dev setup |
| `subscription-daemon.js` | Long-running daemon that polls for due subscriptions and calls `process_due_subscriptions` on the RefundManager contract. | `CONTRACT_ID`, `OPERATOR_SECRET` | Always running in prod |
| `htlc-sweep.js` | Long-running daemon that polls a local HTLC index for expired (`timeout_ledger` passed) entries and calls the permissionless `claim_htlc_timeout` on the `Htlc` contract to refund the sender. Pass `--once` for a single cycle. | `HTLC_CONTRACT_ID`, `SWEEP_SECRET_KEY` | Always running in prod |
| `fx-oracle-updater.js` | Long-running updater that polls an off-chain price feed, converts quotes to 7-decimal fixed-point, and pushes them to the FXOracle contract via a single atomic `set_rates_batch` call. Skips the cycle (and alerts) on feed outage; logs + alerts on `RateDeviationExceeded`. Pass `--once` for a single cycle. | `ORACLE_SECRET`, `FX_ORACLE_CONTRACT_ID` | Always running in prod |
| `check-doc-links.js` | Verifies every relative link and `#anchor` in Markdown docs resolves (default `docs/faq.md`); exits 1 listing broken links. Run `node scripts/check-doc-links.js [file.md ...]`. | None | After editing docs |
| `generate-sdk.sh` | Generate TypeScript SDK bindings from compiled WASM using `stellar contract bindings typescript`. | None | After contract changes |
| `sandbox-init.sh` | Bootstrap local Stellar sandbox — builds WASM, generates local identities, funds admin via friendbot. | `STELLAR_RPC_URL`, `STELLAR_NETWORK` | Local dev first-time setup |
| `check-mainnet-contract-ids.js` | CI informational check — warns when `sdk/src/network-profiles.ts` mainnet still has `UNSET_CONTRACT_ID` placeholders. Always exits 0. | None | CI |
| `check-error-map-sync.ts` | CI gate — verifies `FLUXAPAY_CONTRACT_ERROR_MAP` in `sdk/src/index.ts` matches the `Error` enum in `fluxapay/src/lib.rs`. Fails CI (exit 1) on drift **or when an expected source file cannot be found** (the missing path is printed). | None | CI |

## CI Jobs

### `check-error-map-sync`

The `check-error-map-sync` CI job runs `npx tsx scripts/check-error-map-sync.ts` on every
pull request and push to `main`. It is a **hard gate**: the job fails (non-zero exit)
when the SDK error map drifts from the contract `Error` enum, and also when any expected
source file (e.g. `fluxapay/src/lib.rs` or `sdk/src/index.ts`) is missing — the error
message includes the missing path so the failure is actionable in CI logs.

To reproduce locally:

```bash
npx tsx scripts/check-error-map-sync.ts
```

To verify the fail-loud behavior, temporarily rename a source file and re-run — the
script must exit non-zero and print the missing path.

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

### `htlc-sweep.js`

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `STELLAR_RPC_URL` | No | `https://soroban-testnet.stellar.org` | Soroban RPC endpoint |
| `HTLC_CONTRACT_ID` | Yes | — | Deployed `Htlc` contract address |
| `SWEEP_SECRET_KEY` | Yes | — | Funding keypair that pays the sweep transaction fee (no HTLC-specific role needed - `claim_htlc_timeout` is permissionless) |
| `POLL_INTERVAL_MS` | No | `60000` | Poll interval in milliseconds |
| `NETWORK_PASSPHRASE` | No | `Networks.TESTNET` | Stellar network passphrase |
| `HTLC_INDEX_PATH` | No | `/tmp/fluxapay_htlcs.json` | Local JSON index of `{ id, timeout_ledger, status }` entries to check for expiry |

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
| `MAX_STALENESS_SECS` | No | `86400` | On-chain rate freshness window; warns and alerts at 80% of this age |
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
./deploy.sh [network]
```

The network may be supplied as an optional positional argument or via the
`STELLAR_NETWORK` environment variable. The positional argument takes
precedence over the environment variable. When neither is provided the network
defaults to `testnet`.

Accepted networks: `testnet` | `futurenet` | `standalone`.

### Examples

```bash
# Deploy to testnet (default)
./deploy.sh testnet

# Deploy to futurenet via the environment variable
STELLAR_NETWORK=futurenet ./deploy.sh

# Deploy to a standalone sandbox
./deploy.sh standalone

# Seed test data after deploy
SEED_DATA=true ./deploy.sh testnet

# Skip the cargo build step
SKIP_BUILD=true ./deploy.sh testnet
```

An unknown network value prints usage and exits non-zero.

### Environment variables

| Variable            | Required | Description                                              |
| ------------------- | -------- | -------------------------------------------------------- |
| `STELLAR_SECRET_KEY`| yes      | Deployer secret key (starts with `S`)                    |
| `STELLAR_NETWORK`   | no       | Target network (`testnet` \| `futurenet` \| `standalone`)|
| `STELLAR_RPC_URL`   | no       | Override the RPC endpoint                                |
| `SEED_DATA`         | no       | Set to `true` to seed test data after deploy             |
| `SKIP_BUILD`        | no       | Set to `true` to skip the cargo build step               |

## `deploy-testnet.sh`

Deprecated. Kept for backward compatibility; it prints a warning and delegates
to `deploy.sh`, defaulting to `testnet` when no network is supplied.
