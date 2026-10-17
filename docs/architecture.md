# FluxaPay Contract Architecture

## Overview

FluxaPay is a multi-contract Soroban dApp that enables merchants to accept payments, create payment links, manage refunds, and handle disputes on the Stellar network. The system is built around three core contracts that coordinate payment processing, refund management, and merchant registry.

---

## Contract Responsibilities

### PaymentProcessor

The **PaymentProcessor** contract is the primary contract that orchestrates payment creation, settlement, and lifecycle management. It maintains:
- Payment records with status tracking (PENDING, SETTLED, DISPUTED, REFUNDED)
- Rate limiting per merchant and payer to prevent abuse
- Idempotency keys to prevent duplicate payment creation
- Fee configuration and split logic (treasury, developer, merchant)
- Integration with the FX oracle for multi-currency settlements
- Payment link management via PaymentLinkManager
- Dispute tracking and arbitration voting
- Subscription plan and tick-based recurring payments

### RefundManager

The **RefundManager** contract handles all refund operations, including:
- Refund creation and status tracking
- Cooldown periods to prevent rapid refund abuse
- Collaborative settlement signatures for operator/merchant agreement
- Refund routing (direct to payer or via DEX swap to original token)
- Reentrancy protection during concurrent refund processing

### MerchantRegistry

The **MerchantRegistry** contract maintains merchant data and verification:
- Merchant registration with KYC tier levels
- Merchant verification and status management
- Monthly and cumulative volume tracking for tier auto-upgrades
- Dispute counts for merchant reputation scoring
- Merchant-specific rate limiting configuration

---

## Cross-Contract Call Diagram

```
PaymentProcessor
  ├─ calls MerchantRegistry
  │   ├─ verify_merchant(merchant_id) → ok/error
  │   └─ get_merchant(merchant_id) → Merchant
  │
  ├─ calls DexRouter (for swap_and_pay)
  │   └─ swap_exact_tokens_for_tokens() → Vec<amounts>
  │
  ├─ calls FXOracle (optional, for rate validation)
  │   └─ get_rate(pair) → (rate, timestamp)
  │
  ├─ calls RefundManager
  │   ├─ process_refund(refund_id) → ok/error
  │   └─ create_refund(...) → refund_id
  │
  └─ calls PaymentLinkManager
      ├─ create_link(...) → link_id
      └─ use_link(link_id) → payment_id

RefundManager
  ├─ calls DexRouter (for swap-based refunds)
  │   └─ swap_exact_tokens_for_tokens() → amounts
  │
  └─ calls FXOracle (optional, for refund rate validation)
      └─ get_rate(pair) → (rate, timestamp)

PaymentLinkManager
  └─ stores links independently (no external calls)
```

## Subscription Metrics (#1083)

The indexer retains subscription plan ownership/pricing and lifecycle events
from `SUBSCRIPTION` contract events. The merchant-scoped
`GET /v1/subscriptions/metrics?merchant_id=...&period=YYYY-MM` endpoint uses
that history to calculate active subscribers at the start of the month, churn,
trial conversions, and dunning recoveries; current subscription state and plan
amounts supply active subscriber count, past-due count, and MRR. API responses
are cached for five minutes and require a merchant-bound API key with the
`read:analytics` scope (admin keys may query any merchant).

### Key Call Flows

**Payment Creation Flow:**
1. Payer calls `PaymentProcessor::create_payment()`
2. PaymentProcessor validates payer rate limit
3. PaymentProcessor fetches merchant from MerchantRegistry
4. PaymentProcessor stores payment with PENDING status
5. Payment event is emitted
6. Return payment_id to payer

**Payment Settlement Flow:**
1. Operator calls `PaymentProcessor::settle_payment(payment_id)`
2. PaymentProcessor acquires reentrancy lock
3. **(Issue #1031)** If the merchant's `settlement_schedule` is `Daily` or
   `Weekly`, PaymentProcessor rejects the call with
   `Error::SettlementIntervalNotElapsed` unless at least one interval has
   passed since `last_settlement_at`, and with `Error::SettlementBelowMinimum`
   if the merchant's pending balance (including this payment) is still below
   `SETTLEMENT_MIN_AMOUNT`. `Manual`-schedule merchants are unaffected and
   settle immediately, matching `trigger_settlement`'s existing semantics for
   the aggregate sweep.
4. PaymentProcessor updates payment status to SETTLED
5. Fees are calculated and split
6. **(Issue #1017)** If the merchant has a `reserve_policy`, the configured
   `reserve_bps` share of the net settlement amount is withheld in a rolling
   reserve bucket (`PAYMENT/RESERVE_HELD`) instead of being credited to
   pending settlement; it becomes available after `holding_period_secs` via
   `release_merchant_reserve`.
7. Settlement event is emitted
8. Reentrancy lock is released

**Refund Processing Flow:**
1. Operator/Payer calls `RefundManager::process_refund(refund_id)`
2. RefundManager validates cooldown period
3. If swap-based refund: calls DexRouter to exchange tokens
4. Refund status updated to PROCESSED
5. Refund event emitted
6. Funds transferred to original payer

**Swap and Pay Protection:**
`PaymentProcessor::swap_and_pay()` applies a default 50 bps (0.5%) minimum
output floor relative to the on-chain DEX quote, in addition to the caller's
`amount_out_min`. If an FX oracle and pair are provided, the quoted output is
also checked against that independent reference and rejected when adverse
price impact exceeds 100 bps (1%). Swaps without an oracle still receive the
minimum-output and volume protections, but cannot be checked against a
reference-market price.

The PaymentProcessor tracks successful `amount_in` swap volume for the current
ledger sequence in instance storage. A swap that would exceed the default
10,000,000,000,000-unit per-ledger cap is rejected before calling the DEX.
Admin accounts can adjust all three defaults with `set_swap_limits`; the
volume counter starts fresh when the ledger sequence changes.

---

## Role Model

Access control uses a fixed **role registry** (`KNOWN_ROLES` in
`fluxapay/src/access_control.rs`). `grant_role` and `revoke_role` reject any
`Symbol` not in this list with `AccessControlError::UnknownRole`, preventing
phantom permissions from typo'd role names.

| Symbol | Constant helper |
|--------|-----------------|
| `ADMIN` | `role_admin()` |
| `ORACLE` | `role_oracle()` |
| `MERCHANT` | `role_merchant()` |
| `SETTLEMENT_OPERATOR` | `role_settlement_operator()` |
| `ARBITRATOR` | `role_arbitrator()` |

When adding a new system role, update `KNOWN_ROLES` first — it is the single
source of truth.

### Admin Role

**Permissions:**
- Grant/revoke roles to other accounts
- Initialize contract and set settlement operator
- Claim admin from pending admin (two-step admin transfer)
- Set MerchantRegistry address
- Allow/blacklist tokens
- Set global rate limits and fee configuration
- Propose and finalize fee changes (7-day maturity)
- Set KYC tier limits

**How Granted:** Set at initialization; transferred via `claim_admin()` two-step process

### Merchant Role

**Permissions:**
- Create payments, payment links, and payment-link invoices as the address holding the role
- Request refunds for its own payments
- View their own payments and refunds
- Receive payment settlements

**How Granted:** Admin grants or revokes the role via `grant_merchant_role()` / `revoke_merchant_role()` on the relevant contract. Payment creation and invoice-linked payment links also validate the merchant's configured registry record when a MerchantRegistry is configured. PaymentLinkManager maintains its own role state, administered by its initialized contract admin. A role never authorizes acting for another merchant address.

### Oracle Role

**Permissions:**
- Provide FX rate data to PaymentProcessor
- Update current rate and timestamp
- Used to validate swap prices during DEX execution

**How Granted:** Admin grants via `grant_role()` to FXOracle contract or operator

### Settlement Operator Role

**Permissions:**
- Call `settle_payment()` to mark payments as settled
- Call `process_refund()` to execute refunds
- View payment and refund records
- Sign collaborative settlements with merchants
- Propose and finalize fee changes (7-day maturity)

**How Granted:** Admin sets via `set_settlement_operator()` at init; revoked via `revoke_role()`

### Arbitrator Role

**Permissions:**
- Vote on disputes via `vote_on_dispute()`
- Lock stake for dispute arbitration
- View dispute details and voting tally

**How Granted:** Admin grants via `grant_role()` when adding to arbitrator pool

---

## Key DataKey Variants

| DataKey | Type | Purpose |
|---------|------|---------|
| `Payment(id)` | PaymentCharge | Core payment record with amount, merchant, status, fees |
| `Refund(id)` | RefundRecord | Refund request with original token, swap path, status |
| `Dispute(id)` | Dispute | Dispute record with evidence, resolution, voting state |
| `Stream(id)` | PaymentStream | Recurring payment plan with tick interval and balance |
| `MerchantPayments(addr)` | Vec<String> | Index of payment IDs for a merchant (pagination) |
| `MerchantRateLimit(addr)` | (count, reset_time) | Per-merchant creation rate limit tracker |
| `PayerRateLimit(addr)` | (count, reset_time) | Per-payer creation rate limit tracker |
| `GlobalRateLimit` | (count, reset_time) | Global creation rate limit across all users |
| `MerchantMonthlyVolume(addr, month)` | i128 | Cumulative payment volume in current month (for KYC limits) |
| `MerchantCumulativeVolume(addr)` | i128 | All-time volume for tier auto-upgrade eligibility |
| `IdempotencyKey(key)` | String | Stores payment_id to prevent duplicate creation |
| `AllowedToken(addr)` | bool | Whitelist of supported payment tokens |
| `FeeSplitConfig` | FeeSplit | Treasury %, developer %, and addresses for fee distribution |
| `CurrentFee` | i128 | Current payment processing fee in basis points |
| `KycTierLimitsConfig` | Map<tier, limit> | Max monthly volume per KYC tier |
| `DisputeArbitratorVotes(id)` | Vec<Address> | List of arbitrators who voted on a dispute |
| `DisputeVoteTally(id)` | (for_count, against_count) | Vote counts for dispute resolution |
| `ReentrancyLock` | bool | Prevents concurrent settle_payment/process_refund calls |

---

## Payment Lifecycle

### 1. **Creation** (Payer initiates)
   - Payer calls `PaymentProcessor::create_payment()` with merchant and amount
   - PaymentProcessor checks rate limits, validates merchant, creates Payment with status=PENDING
   - Event: `PAYMENT/CREATED(payment_id, payer, merchant, amount)`

### 2. **Settlement** (Operator confirms)
   - Operator calls `PaymentProcessor::settle_payment(payment_id)`
   - PaymentProcessor acquires reentrancy lock, updates status to SETTLED, calculates and splits fees
   - Operator receives settlement confirmation
   - Event: `PAYMENT/SETTLED(payment_id, amount, fee)`

### 3. **Dispute** (Payer/Merchant challenges)
   - Payer calls `PaymentProcessor::create_dispute(payment_id, evidence_hash)` within dispute window
   - PaymentProcessor creates Dispute, emits event, starts arbitration
   - Arbitrators lock stake and vote
   - Event: `DISPUTE/CREATED(dispute_id, payment_id, initiator)`

### 4a. **Refund (Approval)** (Dispute resolved in favor of refund)
   - After dispute resolution or within cooldown period, `RefundManager::process_refund()` is called
   - RefundManager checks cooldown, routes refund (direct or via DEX swap to original token), transfers funds
   - Refund status changes to PROCESSED, original payment status to REFUNDED
   - Event: `REFUND/PROCESSED(refund_id, original_amount, routed_amount)`

### 4b. **Finalization** (Dispute resolved, payment confirmed)
   - Arbitrators vote settlement, tally exceeds threshold
   - Dispute status changes to RESOLVED
   - Payment status remains SETTLED or transitions to DISPUTED then back to SETTLED if arbitration favors payment
   - Event: `DISPUTE/RESOLVED(dispute_id, outcome, final_status)`

### Optional: **Recurring Payments** (Subscription tick)
   - Recurring payments tick at interval; PaymentProcessor calls `process_due_subscriptions()`
   - For each active subscription past its tick time, creates a new payment (child of subscription)
   - Updates subscription balance and next tick time
   - Event: `SUBSCRIPTION/TICK(subscription_id, new_payment_id, remaining_balance)`

---

## Integration Points

- **DEX Router**: Atomic token swaps for `swap_and_pay()` and swap-based refunds
- **FX Oracle**: Optional multi-currency rate validation to prevent price slippage abuse
- **Merchant Registry**: Verification, KYC tier tracking, merchant lookup (see [MerchantRegistry API Reference](merchant-registry-api-reference.md))
- **Payment Link Manager**: Independent links with direct transfers and metadata validation

---

## MerchantRegistry API Reference

For detailed entry point documentation, parameters, return types, authorization requirements, and emitted events for merchant management, see the dedicated reference document:

👉 **[MerchantRegistry API Reference](merchant-registry-api-reference.md)**

Key operations documented include:
- `register_merchant` — Registration & initial tier assignment
- `update_merchant` — Payout address and profile updates
- `verify_merchant` / `verify_merchant_with_signature` — Verification flows
- `set_kyc_tier` / `auto_upgrade_kyc_tier` — Tier management & automated upgrades
- `set_fee_config` / `calculate_platform_fee` — Platform fee configurations
- `add_to_whitelist` / `is_address_whitelisted` — Payer whitelist controls
- `suspend_merchant` / `reinstate_merchant` — Lifecycle & suspension management
- `get_all_merchants` — Paginated merchant catalog queries

---

## FX & Settlement: Stellar Anchor Protocol (SEP-6 / SEP-24) Fiat Offramp

FluxaPay's settlement layer bridges on-chain USDC on Stellar to merchants' off-chain bank accounts via the Stellar Anchor Protocol. The integration supports both **SEP-6** (programmatic, no UI — for merchants with existing KYC) and **SEP-24** (interactive hosted UI — for merchants that still need to complete KYC or bank details). Compliant anchor partners include MoneyGram, Circle (USDC issuer), Tempo, and region-specific anchors.

### Design Rationale

Soroban contracts cannot make HTTP calls to anchor APIs from on-chain. The architecture therefore uses an **on-chain event + off-chain callback** pattern:

1. On-chain: `PaymentProcessor::settle_payment` transfers USDC to the merchant's `payout_address`, marks the `Payment.status = Settled`, and **emits** a `PAYMENT / ANCHOR_WITHDRAW` event containing all parameters the off-chain service needs (anchor endpoints, amount, currency, merchant payout address, etc.).
2. Off-chain: a **Settlement Service** (indexer) picks up the `ANCHOR_WITHDRAW` event and calls the anchor's SEP-6 `/transactions/withdraw` endpoint. It polls for terminal status and posts a callback webhook (`POST /settlement/anchor/callback`) to FluxaPay's backend.

This keeps the on-chain contracts simple (no HTTP, no async), and moves the inherently-asynchronous fiat-bank-network logic to the service layer where it belongs. Full protocol details, request/response mappings, failure handling, and reference anchor configs live in the companion document: [sep6-sep24-anchor-integration.md](sep6-sep24-anchor-integration.md).

### Merchant Anchor Config (MerchantRegistry)

Merchants opt in to anchor offramp via `MerchantRegistry::set_merchant_anchor`, which stores an `AnchorConfig` struct on their Merchant record:

| Field | Purpose |
|-------|---------|
| `anchor_domain` | Fully qualified anchor domain (e.g. `api.moneygram.com`) for SEP-1 TOML discovery |
| `sep6_endpoint` | Full URL to the anchor's SEP-6 programmatic transfer server |
| `sep24_endpoint` | Full URL to the anchor's SEP-24 interactive transfer server (fallback) |
| `supported_currencies` | ISO-4217 codes this anchor can payout for this merchant (USD, EUR, NGN, …) |

- SDK surface: `MerchantRegistryClient.setMerchantAnchor({ merchantId, anchorConfig })`. Pass `null` to clear the anchor and revert to on-chain-only settlement.
- Auth: `set_merchant_anchor` requires the merchant's own signature — only the merchant controls their anchor.
- Backwards compatibility: the `Merchant.anchor_config` field is a `MaybeAnchorConfig` enum (`None` / `Some(AnchorConfig)`). Existing merchants read back `None` — same behavior as before (no anchor, USDC to payout address directly).

### Settlement → Anchor Flow

```
PaymentProcessor::settle_payment
  │
  ├─ Transfer USDC → merchant.payout_address        (on-chain)
  ├─ Payment.status = Settled                       (on-chain)
  ├─ emit PAYMENT / SETTLED
  │
  └─ if merchant.anchor_config.is_some:
       emit PAYMENT / ANCHOR_WITHDRAW               (off-chain trigger)
         Topics:   (PAYMENT, ANCHOR_WITHDRAW, merchant_id, anchor_domain)
         Payload:  (payment_id, amount, currency,
                    merchant_payout_addr,
                    sep6_endpoint, sep24_endpoint,
                    supported_currencies, ledger_ts)
          │
          ▼
  Off-chain Settlement Service
    ├─ Consume event (Soroban RPC / Horizon getEvents)
    ├─ Authenticated via SEP-10 JWT per anchor
    ├─ POST {sep6_endpoint}/transactions/withdraw
    │    → body: { asset_code=USDC, amount,
    │              dest=bank_account, account=payout_addr,
    │              memo=payment_id }
    ├─ Poll /transactions/{id} until terminal
    │    - completed  → webhook success
    │    - incomplete → fall back to SEP-24 URL → notify merchant
    │    - error      → webhook error + alert ops
    └─ POST /api/settlement/anchor/callback  (FluxaPay backend)
         → webhook HMAC-signed; 200 OK = ack; 5xx = retry w/ backoff
```

### Updated Cross-Contract Diagram

```
PaymentProcessor
  ├─ calls MerchantRegistry
  │   ├─ verify_merchant(merchant_id) → ok/error
  │   ├─ get_merchant(merchant_id) → Merchant
  │   │                          ├─ .fee_config       → % + fixed fee calc in settle_payment
  │   │                          ├─ .anchor_config    → ANCHOR_WITHDRAW event payload
  │   │                          └─ ...
  │   └─ set_merchant_anchor(merchant_id, AnchorConfig | None)
  │
  ├─ calls DexRouter (for swap_and_pay)
  │   └─ swap_exact_tokens_for_tokens() → Vec<amounts>
  │
  ├─ calls FXOracle (optional, for rate validation)
  │   └─ get_rate(pair) → (rate, timestamp)
  │
  ├─ calls RefundManager
  │   ├─ process_refund(refund_id) → ok/error
  │   └─ create_refund(...) → refund_id
  │
  ├─ calls PaymentLinkManager
  │   ├─ create_link(...) → link_id
  │   └─ use_link(link_id) → payment_id
  │
  └─ emits events → Off-chain Settlement Service
       └─ PAYMENT / ANCHOR_WITHDRAW  ──┐
                                       ▼
                              Stellar Anchor (SEP-6 / SEP-24)
                                 MoneyGram / Circle / Tempo
                                       │
                                       ▼
                              Merchant Bank Account (fiat)
                                       │
                                       ▼
                         POST /settlement/anchor/callback
```

### Key Events

| On-Chain Event | Consumer | Purpose |
|----------------|----------|---------|
| `(MERCHANT, ANCHOR_UPDATED)` | Indexer / audit | Records whenever a merchant sets or clears their anchor config |
| `(PAYMENT, ANCHOR_WITHDRAW, merchant_id, anchor_domain)` | Off-chain Settlement Service | Triggers SEP-6 withdrawal with the anchor partner on successful settlement |
| `(PAYMENT, SETTLED)` | General indexer | Canonical settlement event (fires regardless of whether an anchor is configured) |

### Failure & Security Notes

- **Anchor API unreachable**: settlement on-chain already succeeded; the off-chain Settlement Service retries with exponential backoff and alerts ops if failing for >2 h. USDC is already safely in the merchant's on-chain payout address, so the merchant is never at risk of losing funds — worst case the fiat payout is delayed.
- **KYC / bank missing at anchor**: SEP-6 responds `incomplete`. The service redirects the merchant to the SEP-24 interactive URL; once the merchant completes the flow the next settlement auto-uses SEP-6.
- **Double-withdraw protection**: the off-chain service keys every request idempotently by `payment_id`.
- **SEP-10 mutual auth**: every call to the anchor must use a fresh SEP-10 JWT signed by the merchant / FluxaPay operator key for that anchor.
- **Payout address 48h delay**: if the merchant also changes their on-chain `payout_address` after configuring an anchor, the existing 48-hour change cooldown applies, preventing a compromised merchant key from silently redirecting USDC before it reaches the anchor.

Full SEP-6 / SEP-24 protocol integration details, request payloads, anchor status mapping, and callback webhook schema are documented in [sep6-sep24-anchor-integration.md](sep6-sep24-anchor-integration.md).

---

### DEX Router & Slippage Tolerance (#856)

FluxaPay integrates with Soroswap-compatible DEX routers (`DexRouter`) to support atomic token swaps prior to payment settlement (`swap_and_pay`) or for swap-based refunds.

#### `execute_swap` Entry Point

```rust
pub fn execute_swap(
    env: Env,
    caller: Address,
    token_in: Address,
    token_out: Address,
    amount_in: i128,
    min_amount_out: i128,
    max_slippage_bps: u32,
) -> Result<i128, DexRouterError>
```

- **Slippage Enforcement**: `min_amount_out` specifies the absolute minimum token output the caller is willing to receive. If the actual swap output is less than `min_amount_out`, the swap reverts with `DexRouterError::SlippageExceeded` (code 4).
- **Secondary Quoted Price Guard**: `max_slippage_bps` enforces a relative tolerance in basis points against the simulated quoted price (`get_amounts_out`). The parameter is strictly capped at `≤ 5000` bps (50% max). If actual execution deviates beyond this range or if `max_slippage_bps > 5000`, the transaction is rejected with `SlippageExceeded`.
- **Price Impact Guard**: Swaps also enforce an internal price impact guard to protect against pool imbalance.

---

## Security Considerations

- **Threat Model**: See the [FluxaPay threat model](threat-model.md) for assets,
  attack surfaces, mitigations, and residual risks.
- **Reentrancy Protection**: `ReentrancyLock` guards concurrent settle/refund operations
- **Rate Limiting**: Per-merchant, per-payer, and global limits prevent spam
- **Idempotency**: Duplicate payment requests detected via idempotency keys
- **Cooldown Periods**: Refund requests blocked within cooldown window (default 7 days)
- **Collaborative Settlement**: Operator and merchant must both sign refund agreements
- **Arbitration Voting**: Stake-locked voting with threshold ensures fair dispute resolution
- **Two-Step Admin Transfer**: Prevents accidental admin loss via `pending_admin` + `claim_admin()`
- **Metadata Validation**: Payment links enforce max key count (20) and value length (256 chars)

## Indexer Event Replay

The indexer can recover contract events missed during downtime through
`POST /v1/admin/replay`, which accepts `from_ledger` and `to_ledger` in the
JSON request body. The inclusive ledger range is limited to 10,000 ledgers and
the caller must provide an API key with the `admin` scope. The legacy
`POST /admin/replay` route remains available for existing operator clients.

Replay reads contract events from the configured Soroban RPC endpoint, then
passes each event through the normal persistence pipeline. `event_id` is
unique in `contract_events`; an already-stored event is skipped without
reapplying its payment, refund, or dispute update. Newly stored events update
the typed index tables and dispatch matching enabled merchant webhooks. Replay
webhook envelopes include `replayed: true` at the top level and in `data`, so
consumers can distinguish recovery deliveries from live notifications.

The endpoint streams progress and a final summary as Server-Sent Events. Each
update reports processed events, newly stored events, and skipped duplicates.
RPC or persistence errors are surfaced as an SSE error rather than reported as
a successful replay.

## Error Codes

Every `#[contracterror]` enum across all contracts (`Error`, `AccessControlError`,
`StreamError`, `FXOracleError`, `MerchantError`, `MerchantAuthError`,
`DexRouterError`, `AccountAbstractionError`) is documented in a single
reference table, including common causes and remediation: see
[error-codes.md](error-codes.md).

## Architecture Decision Records

- [ADR-0001: Access Control Split](ADR-0001-access-control-split.md)
- [ADR-0002: Payment Stream Design](ADR-0002-payment-stream-design.md)
- [ADR-0003: KYC Tier System](ADR-0003-kyc-tier-system.md)
- [ADR-0004: Multi-Sig Admin Governance](ADR-0004-multisig-admin-governance.md)
