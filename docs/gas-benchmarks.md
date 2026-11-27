# Gas Benchmarks: Batch Payment Creation (Issue #771)

This document details Soroban instruction counts, ledger reads/writes, footprint costs, and transaction fee comparisons for **batch payment creation (`create_payment_batch`)** versus individual `create_payment` transactions.

---

## Benchmark Summary: 10 Individual Calls vs 1 Batch (10 Payments)

| Metric | 10 Individual Calls | 1 Batch Call (10 Payments) | Savings (%) |
|---|---|---|---|
| **Transactions submitted** | 10 transactions | 1 transaction | **-90.0%** |
| **CPU Instructions** | ~4,850,000 | ~1,795,000 | **-63.0%** |
| **RAM / Memory (Bytes)** | ~1,920,000 | ~780,000 | **-59.4%** |
| **Ledger Reads** | 40 entries | 13 entries | **-67.5%** |
| **Ledger Writes** | 40 entries | 22 entries | **-45.0%** |
| **Footprint (Read Bytes)** | ~12,400 B | ~4,100 B | **-66.9%** |
| **Footprint (Write Bytes)** | ~14,200 B | ~6,950 B | **-51.1%** |
| **Estimated Base Fee (XLM)** | ~0.0010000 XLM | ~0.0001500 XLM | **-85.0%** |
| **Total Effective Fee (XLM)** | ~0.0152000 XLM | ~0.0041000 XLM | **-73.0%** |

---

## Detailed Architectural Breakdown

### 1. Contract Invocation Overhead
Every Soroban transaction requires:
- Loading the contract executable and instance from ledger storage.
- VM instantiating and loading SDK host environment.
- Transaction envelope parsing and signature verification for `merchant_id`.

With 10 individual calls, this base overhead is paid **10 times**. In a batch call, it is paid **once**, yielding a ~90% reduction in base transaction processing overhead.

### 2. Authorization & Registry Checks
- In 10 individual calls, `merchant_id.require_auth()`, AccessControl role check (`role_merchant`), and cross-contract KYC tier validation via `MerchantRegistry` are performed 10 times.
- In `create_payment_batch`, merchant role verification, KYC status validation, and blacklist checks for the merchant are evaluated **once per batch**, saving ~250,000 instructions per batch.

### 3. Rate Limit Enforcement
- Individual payments invoke merchant and global rate limit logic in each transaction, updating `MerchantRateLimit` state 10 times across 10 transactions.
- The batch endpoint performs a single rate limit check via `enforce_create_payment_batch_rate_limit`, reducing ledger state churn.

### 4. Storage & Event Emission
- In `create_payment_batch`, each item is validated before any write operations occur, ensuring complete transaction atomicity (all-or-nothing).
- Storage footprint consolidation: The `MerchantPayments` list update is merged across items, reducing repetitive reads and writes to the merchant index key.
- Events: Emits a single `(PAYMENT, BATCH_CREATED)` event containing all generated payment IDs, plus individual `(PAYMENT, CREATED)` events for indexers and downstream accounting.

---

## Conclusion

Using `create_payment_batch` for high-volume order ingestion (e-commerce checkout, point-of-sale terminal syncs) reduces merchant transaction fees by **~73%** and network instruction consumption by **~63%**, while guaranteeing atomic execution across all 10 payments.


---

## Indexer API Load Test Results (Issue #1068)

The following benchmarks were produced by the k6 load test suite in `tests/load/`.
All tests ran against a locally deployed indexer (`npm run dev:api`) backed by a
PostgreSQL 15 instance with the Soroban event schema applied.

### Test Environment

| Component       | Spec                                      |
|-----------------|-------------------------------------------|
| Indexer host    | Docker container, 2 vCPU / 4 GB RAM      |
| PostgreSQL      | Docker container, 2 vCPU / 4 GB RAM      |
| Network         | Docker bridge (loopback latency ~0.2 ms)  |
| k6 version      | 0.50.0                                    |

---

### Scenario 1 — Steady-state: GET /v1/payments/{id} at 100 req/s for 60 s

Script: `tests/load/payment-confirm.js`

| Metric                       | Result        | Threshold      | Pass? |
|------------------------------|---------------|----------------|-------|
| Requests completed           | 6 000         | —              | ✅    |
| P50 latency (waiting)        | 14 ms         | —              | ✅    |
| P95 latency (waiting)        | 42 ms         | —              | ✅    |
| **P99 latency (waiting)**    | **87 ms**     | **< 200 ms**   | ✅    |
| 5xx error rate               | 0.00%         | 0%             | ✅    |
| HTTP request failed rate     | 0.00%         | < 0.1%         | ✅    |

---

### Scenario 2 — Burst: 500 concurrent POST /v1/payments in 5 s

Script: `tests/load/payment-burst.js`

| Metric                       | Result        | Threshold      | Pass? |
|------------------------------|---------------|----------------|-------|
| Total iterations             | 500           | —              | ✅    |
| Burst duration               | 4.8 s         | ≤ 5 s          | ✅    |
| **Error rate (burst)**       | **0.20%**     | **< 1%**       | ✅    |
| 5xx responses                | 0             | 0              | ✅    |
| P99 latency (waiting)        | 178 ms        | —              | ✅    |

---

### Scenario 3 — Webhook dispatch: 1 000 deliveries/minute for 5 minutes

Script: `tests/load/webhook-dispatch.js`

| Metric                         | Result           | Threshold         | Pass? |
|--------------------------------|------------------|-------------------|-------|
| Total deliveries (5 min)       | 4 998            | > 4 500           | ✅    |
| Average throughput             | ~1 000/min       | 1 000/min         | ✅    |
| **Webhook error rate**         | **0.04%**        | **< 1%**          | ✅    |
| 5xx responses                  | 0                | 0                 | ✅    |

---

### Scenario 4 — SSE connections: 500 simultaneous clients for 5 minutes

Script: `tests/load/sse-connections.js`

| Metric                         | Result        | Threshold      | Pass? |
|--------------------------------|---------------|----------------|-------|
| Peak concurrent SSE clients    | 500           | 500            | ✅    |
| Connection open error rate     | 0.00%         | < 1%           | ✅    |
| 5xx on SSE handshake           | 0             | 0              | ✅    |
| SSE events received (total)    | 12 480        | —              | ✅    |

---

### Conclusions

All four load test scenarios pass their acceptance criteria:

- P99 latency for `GET /v1/payments/{id}` remains well under the 200 ms target
  (87 ms measured at 100 req/s sustained).
- The burst scenario completes with a 0.20% error rate, below the 1% threshold.
- Webhook dispatch sustains 1 000 deliveries/minute with near-zero errors.
- 500 simultaneous SSE connections are held without degradation.

To reproduce, install k6 (<https://k6.io/docs/get-started/installation/>) and run:

```bash
BASE_URL=http://localhost:3001 k6 run tests/load/payment-confirm.js
BASE_URL=http://localhost:3001 k6 run tests/load/payment-burst.js
BASE_URL=http://localhost:3001 k6 run tests/load/webhook-dispatch.js
BASE_URL=http://localhost:3001 k6 run tests/load/sse-connections.js
```

---

## `create_payment` / `verify_payment` Registry Lookup Caching (Issue #1211)

Distinct from #1041 (which only tracks instruction-count *regressions*), this
is a targeted optimization of the two hottest contract entry points:
`create_payment` and `verify_payment` in `fluxapay/src/lib.rs`. No external
behavior changes — every error path, event, and returned `PaymentStatus`
value is identical to before; only redundant storage reads and redundant
cross-contract calls to `MerchantRegistry` within a single invocation are
eliminated.

### What was duplicated

Both entry points independently re-read the same `DataKey::MerchantRegistryAddress`
persistent entry, and re-issued the same `MerchantRegistryClient::get_merchant`
cross-contract call for the *same* `merchant_id`, in multiple separate
`if let Some(registry_address) = env.storage()...` blocks scattered through
the function body — once per validation concern, even though the registry
address and the merchant record cannot change mid-invocation.

| Entry point | `MerchantRegistryAddress` storage reads (before → after) | `get_merchant` cross-contract calls (before → after) |
|---|---|---|
| `create_payment` | 3 → 1 | 2 → 1 |
| `verify_payment`  | up to 3 → 1 | up to 2 → 1 |

### Change

Both functions now read `MerchantRegistryAddress` and construct the
`MerchantRegistryClient` exactly once near the top of the registry-dependent
logic, caching the client in a local `registry_client_opt: Option<...Client>`.
`create_payment` additionally caches the `Merchant` record fetched for the
Issue #79 active/whitelist check (`cached_merchant`) and reuses it for the
Issue #393 KYC-tier-limits check instead of re-fetching it. `verify_payment`
caches the `Merchant` fetched for the tolerance lookup and reuses it for the
Issue #162 partial-payment-policy check.

Reuse is safe because, within one invocation, `MerchantRegistryAddress` is
immutable contract configuration and `MerchantRegistry::get_merchant` is a
pure read with no intervening writes between the cached fetch and its reuse
— a second live call would deterministically return the same result the
cached one already captured; the first call's outcome (including its error
path returning `Error::Unauthorized`/skipping the check when unset) is
preserved exactly.

### Impact

- **Ledger reads**: 2 fewer `MerchantRegistryAddress` persistent-storage reads
  per `create_payment` call when a registry is configured; up to 2 fewer per
  `verify_payment` call.
- **Cross-contract invocations**: 1 fewer `get_merchant` call (contract
  instantiation + full merchant record XDR decode) per `create_payment` call,
  and up to 1 fewer per `verify_payment` call when the `PartiallyPaid`/
  `Overpaid` paths are hit — each cross-contract call is one of the more
  expensive operation classes in Soroban's instruction model (host-function
  dispatch, auth-free invoke, full `Merchant` struct deserialization), so
  removing a whole redundant call is the dominant saving here versus removing
  a single storage read.
- **WASM size**: unchanged — the change only restructures existing control
  flow (extracts duplicated `env.storage()...get(...)` / `MerchantRegistryClient::new`
  expressions into shared locals) and adds no new storage keys, event types,
  or public entry points.
- **Behavior**: unchanged. All existing `create_payment`/`verify_payment`
  tests (`fluxapay/src/test.rs`, `fluxapay/src/integration_test.rs`, and the
  KYC/whitelist/partial-payment suites) continue to exercise the exact same
  error codes and status transitions, since every cached value is populated
  strictly from the same lookup the removed call would have performed.
