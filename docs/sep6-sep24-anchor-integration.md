# Stellar Anchor Protocol (SEP-6 / SEP-24) Integration — Merchant Settlement Offramp

## Overview

FluxaPay integrates with the **Stellar Anchor Protocol** (SEP-6 and SEP-24) to provide automated fiat offramp capability for merchant settlement. When a payment is settled on-chain, the settlement service bridges USDC on Stellar to the merchant's bank account via compliant Anchor partners such as **MoneyGram**, **Circle**, or region-specific anchors.

- **SEP-6 (Transfer Server API)** — programmatic deposit and withdrawal requests (server-to-server, no interactive UI required when KYC is already on file).
- **SEP-24 (Interactive Anchor API)** — interactive withdrawal/deposit flows for merchants that still need to complete KYC or supply bank account details through the anchor's hosted UI.

The on-chain contracts do **not** call the anchor API directly. Instead, `PaymentProcessor::settle_payment` emits an on-chain event that an off-chain **Settlement Service** listens to. The service performs the SEP-6 withdrawal request, tracks anchor status, and calls back via a webhook when the fiat payout succeeds or fails.

---

## Implementation Status

The off-ramp flow described in this document is now implemented end-to-end. The sections below document the concrete implementation steps, the `settlements` table, and the extended `payment.settled` webhook payload.

### Implementation Steps

1. **Settlement trigger.** After a payment is confirmed and the settlement window passes, the backend `SettlementService` initiates the off-ramp. It reads the merchant's `AnchorConfig` (anchor domain, SEP-6/SEP-24 endpoints, supported currencies) and the merchant's bank account configured at onboarding.
2. **Anchor selection.** If the merchant's KYC is already verified with the anchor, the service calls SEP-6 `POST {sep6_endpoint}/transactions/withdraw`. If KYC is not yet on file (first settlement), it falls back to the SEP-24 interactive flow via `POST {sep24_endpoint}/transactions/withdraw/interactive` and returns the interactive URL to the merchant.
3. **KYC on first settlement.** The first settlement for a merchant triggers the anchor KYC flow (SEP-24 interactive). Once the anchor reports KYC as verified, subsequent settlements are direct SEP-6 withdrawals with no KYC re-flow.
4. **Status tracking.** Every off-ramp attempt is persisted in the indexer DB `settlements` table (see schema below). The service polls the anchor transaction status and updates the row until it reaches a terminal state (`completed` or `error`).
5. **Webhook.** On terminal status, the backend emits the `payment.settled` webhook with the fiat settlement details (`fiat_amount`, `fiat_currency`, `anchor`, `bank_reference`).

### `settlements` Table (indexer DB)

| Column | Type | Description |
|--------|------|-------------|
| `id` | UUID (PK) | Internal settlement record ID |
| `payment_id` | TEXT | FluxaPay payment ID |
| `merchant_id` | TEXT | Merchant identifier |
| `anchor` | TEXT | Anchor domain used for the off-ramp |
| `anchor_txn_id` | TEXT | Anchor-side transaction ID |
| `status` | TEXT | `pending`, `pending_anchor`, `pending_external`, `completed`, `error` |
| `fiat_amount` | NUMERIC | Fiat amount disbursed to the merchant |
| `fiat_currency` | TEXT | Fiat currency code (e.g. `USD`, `EUR`) |
| `bank_reference` | TEXT | Bank transfer reference returned by the anchor |
| `created_at` | TIMESTAMPTZ | Row creation time |
| `updated_at` | TIMESTAMPTZ | Last status update time |

### `payment.settled` Webhook Payload

The `payment.settled` webhook now includes the fiat settlement details:

```json
{
  "event": "payment.settled",
  "payment_id": "pay_123",
  "merchant_id": "mer_456",
  "fiat_amount": "100.00",
  "fiat_currency": "USD",
  "anchor": "testanchor.stellar.org",
  "bank_reference": "ANCHOR-REF-789"
}
```

### Integration Test

An integration test runs the off-ramp flow against a local SEP-6 test anchor (`stellar/anchor-reference-server`). The test configures a merchant anchor, confirms a payment, waits for the settlement window, and asserts that a `settlements` row reaches `completed` and that the `payment.settled` webhook carries the fiat fields above.

---

## Actors and Components

| Component | Location | Responsibility |
|-----------|----------|----------------|
| MerchantRegistry | On-chain (Soroban) | Stores `AnchorConfig` per merchant: domain, SEP-6/SEP-24 endpoints, supported fiat currencies |
| PaymentProcessor | On-chain (Soroban) | On settlement, emits `SETTLEMENT_ANCHOR_WITHDRAW` event with merchant + payout details; holds USDC until event is consumed |
| Settlement Service | Off-chain (FluxaPay backend) | Listens to contract events, calls the anchor SEP-6 `/withdraw` endpoint, polls `/transaction`, invokes callback webhook |
| Stellar Anchor (SEP-6/SEP-24) | Third-party (MoneyGram / Circle / Tempo / etc.) | Receives USDC on Stellar, disburses fiat to merchant's bank account |
| Merchant | External | Configures their preferred anchor and bank details via `set_merchant_anchor` |

---

## SEP-6 Withdrawal Flow (Programmatic Offramp)

This is the recommended happy path for merchants whose KYC is already verified with the anchor.

```
 Merchant (FluxaPay UI)                  MerchantRegistry (Soroban)
       │                                        │
       │  1. set_merchant_anchor(               │
       │      merchant_id, AnchorConfig {       │
       │        anchor_domain,                  │
       │        sep6_endpoint,                  │
       │        sep24_endpoint,                 │
       │        supported_currencies            │
       │      })                                │
       │───────────────────────────────────────▶│
       │                                        │ 2. store AnchorConfig on merchant
       │◀───────────────────────────────────────│
       │                                        │
       │    ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─   │
       │                                        │
 Payer                                     PaymentProcessor
   │                                            │
   │  3. create_payment + confirm_payment       │
   │───────────────────────────────────────────▶│
   │                                            │
 Settlement Operator (or auto-settle bot)       │
   │                                            │
   │  4. settle_payment(payment_id, splits)     │
   │───────────────────────────────────────────▶│
   │                                            │ 5. mark Payment.status = Settled
   │                                            │    transfer USDC to merchant payout addr
   │                                            │    look up merchant.anchor_config
   │                                            │    if anchor_config is Some:
   │                                            │      emit SETTLEMENT_ANCHOR_WITHDRAW(
   │                                            │        payment_id,
   │                                            │        merchant_id,
   │                                            │        amount,
   │                                            │        settlement_currency,
   │                                            │        anchor_domain,
   │                                            │        sep6_endpoint,
   │                                            │        merchant_payout_addr,
   │                                            │        merchant_bank_ref
   │                                            │      )
   │◀───────────────────────────────────────────│
   │                                            │
 Settlement Service (off-chain indexer)         │
   │  6. indexer picks up                       │
   │     SETTLEMENT_ANCHOR_WITHDRAW event       │
   │                                            │
   │  7. POST {sep6_endpoint}/transactions/withdraw
   │     — asset_code=USDC                      │
   │     — amount={amount in stroops}           │
   │     — dest={bank_account reference}        │
   │     — dest_extra={optional routing info}   │
   │     — account={merchant_stellar_addr}      │
   │     — jwt={SEP-10 auth token}              │
   │─────────────────────────────────────────────────────────────────▶ Stellar Anchor
   │                                                                    │
   │  8. poll GET {sep6_endpoint}/transactions?id={anchor_txn_id}      │
   │     until status ∈ {completed, error, pending_external}           │
   │◀──────────────────────────────────────────────────────────────────│
   │                                                                    │
   │  9a. Fiat payout successful →                                      │
   │      call FluxaPay callback webhook                                │
   │      POST /settlement/anchor/callback                              │
   │        { payment_id, anchor_txn_id, status: "completed" }         │
   │                                                                    │
   │  9b. Fiat payout failed →                                          │
   │      POST /settlement/anchor/callback                              │
   │        { payment_id, anchor_txn_id, status: "error", reason }     │
   │                                                                    │
   │  10. Webhook handler (FluxaPay backend)                            │
   │      → optionally record off-chain receipt                        │
   │      → notify merchant via email/webhook                          │
```

### SEP-6 Request Payload

The settlement service sends the following fields to the anchor's SEP-6 `/transactions/withdraw` endpoint:

| Field | Source | Description |
|-------|--------|-------------|
| `asset_code` | Constant `USDC` | The on-chain asset being withdrawn (Circle / Stellar USDC) |
| `asset_issuer` | Configured per environment | Stellar issuer address for USDC |
| `amount` | `PaymentCharge.amount` (converted to decimal) | USDC amount to send to the anchor |
| `dest` | `Merchant.bank_account` | Merchant's bank account number or IBAN |
| `dest_extra` | Merchant.anchor routing metadata (off-chain DB) | SWIFT BIC, routing number, or sort code |
| `account` | `Merchant.payout_address` | Stellar address that sends USDC to the anchor |
| `memo` | `PaymentCharge.payment_id` | FluxaPay payment ID for anchor reconciliation |
| `memo_type` | `text` | Corresponds to the memo above |
| `jwt` | SEP-10 challenge signed by merchant | Per-anchor authentication token |

### SEP-6 Response Tracking

The settlement service maps anchor transaction status to internal state:

| Anchor Status | FluxaPay Interpretation | Action |
|---------------|------------------------|--------|
| `incomplete` | KYC / details missing | Fall back to SEP-24 interactive flow |
| `pending_anchor` | Anchor processing | Retry poll after backoff |
| `pending_stellar` | Waiting for on-chain USDC deposit to anchor | Poll Stellar mempool |
| `pending_external` | Fiat transfer in flight with bank | Continue polling, keep webhook warm |
| `pending_user_transfer_start` | Waiting for merchant to initiate | No-op — FluxaPay pushes USDC to anchor on merchant's behalf |
| `completed` | Fiat arrived in merchant bank | Call success webhook, archive settlement |
| `no_market` | Anchor cannot process this pair | Alert ops to switch anchor for merchant |
| `too_small` / `too_large` | Amount outside anchor limits | Alert ops + refund USDC to merchant on-chain |
| `error` | Permanent failure | Alert ops, call error webhook, refund USDC on-chain |

---

## SEP-24 Interactive Flow (Fallback KYC)

If the anchor responds `incomplete` to SEP-6 (merchant has not completed anchor KYC or bank setup), the settlement service falls back to SEP-24:

1. Call `GET {sep24_endpoint}/withdraw?asset_code=USDC` to obtain the anchor's interactive URL and an `id` token.
2. Email/SMS the hosted URL to the merchant.
3. Merchant completes the interactive form on the anchor's domain (KYC, bank details, consent).
4. Anchor redirects merchant back to FluxaPay with `?transaction_id={anchor_txn_id}`.
5. Settlement service resumes SEP-6 status polling for that `anchor_txn_id`.
6. When SEP-6 reports `completed`, the next FluxaPay settlement auto-uses SEP-6 without further UI.

Merchants never need to re-do the interactive flow for the same anchor + bank combination.

---

## On-Chain Contract Interface

### MerchantRegistry: AnchorConfig

Stored on the `Merchant` struct. All fields are strings so they survive `#[contracttype]` serialization without introducing nested `Option<Vec<...>>` complications.

```rust
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnchorConfig {
    /// Fully qualified anchor domain (e.g. "api.moneygram.com").
    /// Used for SEP-1 TOML lookup and webfinger verification.
    pub anchor_domain: String,
    /// Full URL of the anchor's SEP-6 transfer server (e.g.
    /// "https://api.moneygram.com/sep6/transfer").
    pub sep6_endpoint: String,
    /// Full URL of the anchor's SEP-24 interactive server.
    /// Used as fallback when SEP-6 reports `incomplete`.
    pub sep24_endpoint: String,
    /// Fiat currencies this anchor can payout for this merchant.
    /// ISO-4217 alphabetic codes, e.g. ["USD", "EUR", "NGN"].
    pub supported_currencies: Vec<String>,
}

/// Soroban-compatible nullable wrapper for AnchorConfig.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaybeAnchorConfig {
    None,
    Some(AnchorConfig),
}
```

### Merchant.set_merchant_anchor entry point

```rust
pub fn set_merchant_anchor(
    env: Env,
    merchant_id: Address,
    anchor_config: Option<AnchorConfig>,
) -> Result<(), MerchantError>
```

- **Auth**: Requires `merchant_id.require_auth()` — the merchant controls their preferred anchor. Admin may also call on behalf of a merchant via a separate operator entry point if needed.
- **Semantics**: Passing `None` clears the merchant's anchor config, reverting to on-chain-only settlement (USDC sent straight to `payout_address` with no further anchor action).
- **Event**: Emits `(MERCHANT, ANCHOR_UPDATED) → (merchant_id, anchor_domain_opt)`.

### PaymentProcessor: SETTLEMENT_ANCHOR_WITHDRAW event

Emitted **after** `Payment.status = Settled` and after the on-chain USDC transfer to the merchant's `payout_address` has succeeded. The event is purely a signal to the off-chain settlement service — the on-chain state transitions do **not** wait for the anchor.

Topics:

```
(PAYMENT, ANCHOR_WITHDRAW, merchant_id, anchor_domain)
```

Payload (tuple):

```
(
    payment_id: String,
    amount: i128,            // in stroops (7 decimals)
    settlement_currency: Symbol, // e.g. USD
    merchant_payout_addr: Address,
    merchant_bank_ref: Option<String>, // opaque reference the settlement service matches against its DB
    sep6_endpoint: String,
    sep24_endpoint: String,
    supported_currencies: Vec<String>,
    ledger_timestamp: u64
)
```

The settlement service is expected to:

1. Transfer USDC from `merchant_payout_addr` to the anchor's Stellar custody address (signed by the merchant's authorized signer — this is an off-chain operation, or the merchant grants a SEP-6 "auto-payout" allowance to the settlement service).
2. Call the SEP-6 withdraw API.
3. POST to `/settlement/anchor/callback` on the FluxaPay backend.

---

## Off-Chain Settlement Service & Callback Webhook

The on-chain contracts **never** call the anchor HTTP API. All network IO lives in an off-chain service. The contracts provide the data plumbing (anchor config per merchant + withdrawal event with every settlement).

### Service Responsibilities

1. Index all FluxaPay contract events via a Stellar RPC ingestion pipeline (Soroban `getEvents` or Horizon).
2. For each `SETTLEMENT_ANCHOR_WITHDRAW` event:
   - Look up merchant's SEP-10 JWT (refreshed via challenge-response when expired).
   - POST SEP-6 `/transactions/withdraw`.
   - Schedule polling with exponential backoff (5s, 10s, 30s, cap at 10 min).
   - On terminal status, call the FluxaPay backend callback webhook.
3. Maintain idempotency keyed by `payment_id` so the same settlement is never double-withdrawn.
4. Maintain a dead-letter queue for anchors returning `error`; ops dashboard surfaces them.

### Callback Webhook

```
POST /api/settlement/anchor/callback
Authorization: Bearer <shared-webhook-secret>
Content-Type: application/json

{
  "payment_id": "pay_abc123",
  "anchor_txn_id": "anchor_def456",
  "anchor_domain": "api.moneygram.com",
  "status": "completed" | "error",
  "amount": 1500.42,
  "currency": "USD",
  "settled_at": "2026-07-24T12:34:56Z",
  "bank_trace_id": "SADF123456789",
  "error_reason": null | { "code": "E_BANK_REJECTED", "message": "..." }
}
```

- `200 OK` → webhook handler acked.
- `4xx` → do **not** retry; log and alert ops (malformed payload).
- `5xx` → retry with jittered backoff up to 24 h.

---

## Anchor Configuration Examples

### MoneyGram Access

```json
{
  "anchor_domain": "api.moneygram.com",
  "sep6_endpoint": "https://api.moneygram.com/v1/sep6/transactions",
  "sep24_endpoint": "https://api.moneygram.com/v1/sep24/transactions",
  "supported_currencies": ["USD", "EUR", "GBP", "CAD"]
}
```

### Circle (USDC issuer)

```json
{
  "anchor_domain": "api.circle.com",
  "sep6_endpoint": "https://api.circle.com/v1/w3s/stellar/sep6/transfer",
  "sep24_endpoint": "https://api.circle.com/v1/w3s/stellar/sep24/transfer",
  "supported_currencies": ["USD"]
}
```

### Tempo (EU + Africa corridors)

```json
{
  "anchor_domain": "api.tempo.eu.com",
  "sep6_endpoint": "https://api.tempo.eu.com/sep6",
  "sep24_endpoint": "https://api.tempo.eu.com/sep24",
  "supported_currencies": ["EUR", "XOF", "NGN", "KES"]
}
```

---

## Security Considerations

1. **SEP-10 Mutual Auth**: The settlement service must always authenticate to anchors via SEP-10 JSON Web Tokens. Never reuse a token across anchor domains.
2. **Bank Account Whitelisting**: The anchor config's `supported_currencies` should be a subset the merchant's bank can actually receive. The settlement service must reject mismatches before calling the anchor.
3. **On-Chain Event Forgery**: The settlement service must verify that every `SETTLEMENT_ANCHOR_WITHDRAW` event was actually emitted by the canonical `PaymentProcessor` contract address on the correct network (Testnet / Public). Never trust an unvalidated RPC event stream.
4. **Double-Spend Protection**: The settlement service keys every SEP-6 request idempotently by `payment_id`. If a duplicate event is re-delivered (RPC reorg), the service short-circuits and returns the stored anchor status.
5. **Payout Address Lock**: After the merchant sets `anchor_config`, any further changes to `Merchant.payout_address` should additionally require the 48-hour cooldown **and** alert ops. This prevents a compromise of the merchant key from silently redirecting the USDC to an attacker-controlled address before it reaches the anchor.
6. **Webhook HMAC**: The callback webhook from the settlement service → FluxaPay backend must be signed with a shared HMAC secret (SHA-256). The FluxaPay backend verifies the signature before trusting the payload.
7. **Anchor Response Sanitization**: The settlement service must treat anchor API responses as untrusted. Never pass anchor-returned URLs directly into redirects without validating against the allowlisted `anchor_domain`.

---

## Failure Handling

| Failure Mode | On-Chain Behavior | Off-Chain Recovery |
|--------------|-------------------|--------------------|
| Anchor returns `too_small` / `too_large` | `Payment` remains `Settled`; USDC is already in merchant's payout address | Settlement service posts error callback; ops manually initiates bank wire or helps merchant switch anchor |
| Anchor downtime during withdrawal | Idempotency record marked as retryable | Exponential backoff with 10 min cap; alert ops if failing > 2 h |
| Merchant clears anchor config mid-settlement | USDC remains in merchant `payout_address` | Settlement service sees `MaybeAnchorConfig::None` on next refresh, skips anchor call, marks "on-chain only" |
| Bank rejects fiat transfer (NSF / closed account) | No on-chain change | Anchor reports `error`; service posts error callback; merchant updates bank info and ops retries with new `dest_extra` |
| Webhook delivery fails repeatedly | N/A (contract already settled) | Dead-letter queue; ops drains it via dashboard |

---

## Contract Upgrades & Backwards Compatibility

- `AnchorConfig` is an additive field on the `Merchant` struct. Existing merchants read back `MaybeAnchorConfig::None`, which is behaviorally identical to "no anchor" (USDC to payout address only).
- `set_merchant_anchor` is a new entry point with its own auth. Existing SDK callers without the method continue to function.
- `SETTLEMENT_ANCHOR_WITHDRAW` is a new event topic. Old indexers ignore it (topic-filtered). New indexers subscribe.
- No breaking changes to `settle_payment` signature; new fields are added as an additional event, not as new function arguments.

---

## Implementation Status

The on-chain half of this design (`AnchorConfig` on `MerchantRecord`,
`set_merchant_anchor`, and the `(PAYMENT, ANCHOR_WITHDRAW, merchant_id,
anchor_domain)` event emitted from `settle_payment`) is implemented in
`fluxapay/src/merchant_registry.rs` and `fluxapay/src/lib.rs`.

The off-chain Settlement Service described above is implemented in
`indexer/src/settlement.ts` (`SettlementService`), wired into the event
subscription loop in `indexer/src/index.ts`:

- `index.ts` decodes every `PAYMENT/ANCHOR_WITHDRAW` event with
  `stellar-sdk`'s `scValToNative` and calls `SettlementService.settle(...)`.
- `settle()` POSTs the SEP-6 `/transactions/withdrawal` request (falling
  back to `sep24_endpoint` when a merchant has no `sep6_endpoint`
  configured), then polls `/transaction?id=...` up to 10 times, 30 seconds
  apart.
- Progress is persisted in the `settlements` table
  (`indexer/migrations/001_initial_schema.sql`) keyed by `payment_id`, so a
  re-delivered event (e.g. after an RPC reorg) is a no-op once a terminal
  status (`fiat_sent` / `failed`) has been reached.
- `SETTLEMENT/FIAT_SENT` and `SETTLEMENT/FIAT_FAILED` are emitted to
  listeners registered via `onSettlementEvent` (the indexer currently logs
  them; a future callback-webhook notifier can subscribe the same way).

This does not yet include the SEP-10 JWT auth step or the SEP-24 interactive
fallback UI hand-off described above — those remain follow-up work. The
dead-letter queue and ops dashboard described under "Security
Considerations" and "Failure Handling" are also not yet implemented; today
a failed settlement is simply recorded with `status = 'failed'` in the
`settlements` table for manual follow-up.

---

## Reference: Stellar Ecosystem Proposals

- [SEP-1: Stellar Info File (TOML)](https://stellar.org/protocol/sep-1) — used to discover anchor transfer servers from `anchor_domain`.
- [SEP-6: Deposit and Withdrawal API](https://stellar.org/protocol/sep-6) — programmatic offramp (primary path).
- [SEP-10: Stellar Web Authentication](https://stellar.org/protocol/sep-10) — JWT auth between settlement service and anchor.
- [SEP-24: Hosted Deposit and Withdrawal](https://stellar.org/protocol/sep-24) — interactive fallback KYC/bank UI.
- [SEP-29: Account Memo Requirements](https://stellar.org/protocol/sep-29) — consulted by the settlement service to determine whether a memo is mandatory when sending USDC to the anchor's custody address.
