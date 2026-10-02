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
| `pending_user_transfer_