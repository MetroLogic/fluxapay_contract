# Changelog

## [Unreleased]

### Added
- **Issue #671**: Added `docs/sdk-migration-guide.md` covering breaking-change migration steps between major `@fluxapay/sdk` versions, linked from `sdk/README.md` and `sdk/CHANGELOG.md`.
- **Issue #670**: `PaymentProcessor::get_creation_pause_info() -> PauseState` — read-only getter for the creation-only pause state, distinct from `get_pause_info()`'s consolidated global+creation view; clarified `set_creation_pause`/`set_global_pause` scope docs (creation pause never blocks `verify_payment`, `settle_payment`, `cancel_payment`, or refund/dispute flows).
- **Issue #668**: `PaymentCharge.payment_link_id: Option<String>` — populated by `use_link` with the source link's ID so payments can be traced back to the link that created them; `None` for payments created via `create_payment`/`swap_and_pay`. Updated the generated SDK `PaymentCharge` TypeScript type.
- **Issue #667**: `set_contract_metadata(admin, key: Symbol, value: String)` / `get_contract_metadata(key: Symbol) -> Option<String>` on `PaymentProcessor`, `RefundManager`, and `MerchantRegistry` — admin-gated on-chain metadata storage (instance storage, `LONG_LIVE_TTL`) for on-chain version/description tagging. `description`, `version`, and `deployed_at` are pre-populated on `initialize`.
- **Issue #378**: `cancel_refund` entry point — adds `RefundStatus::Cancelled`, transitions pending refunds to cancelled (requester or admin only), emits `REFUND/CANCELLED`, and excludes cancelled refunds from payment refund totals.
- **Issue #379**: Admin-configurable refund fee — `RefundFeeBps` stored in instance storage (default 100 bps at init); new `set_refund_fee_bps(admin, bps)` setter (0–1000 bps) and `get_refund_fee_bps()` getter; `process_refund` reads fee from storage.

### Fixed
- **Issue #374**: Removed unnecessary `mut` on `processed_count` and unused `now` in `process_due_subscriptions` batch processor (Clippy clean).
- **Issue #377**: `use_link` payment IDs now combine ledger timestamp with link `use_count` to prevent same-ledger collisions.

### Storage key layout changes (Issue #379)
- **Moved to instance storage**: `DataKey::RefundFeeBps` — initialized to 100 bps during `initialize_refund_manager`; previously read from persistent storage via multi-sig only.

### Added (prior) `settle_payment` correctly handles non-empty `splits` by distributing USDC according to `SettlementSplit.amount` and handles empty splits by transferring the full net amount directly to the merchant.
- **Issue #375**: `swap_and_pay` DEX integration — transfers input tokens to the router, enforces `amount_out_min` slippage and `expires_at` deadline, creates the payment after swap, and emits `SWAP/EXECUTED`.
- **Issue #376**: Subscription entry points on `RefundManager` — `create_plan`, `subscribe_to_plan`, `process_subscription`, `get_plan`, and merchant-aware `cancel_subscription`; subscription charge/cancel events documented in `EVENTS.md`.
- **Issue #373**: DexRouter stub cleanup — prefixed unused swap parameters and added TODO comments for real router integration.
- **Issue #372**: Removed unused test setup variables in `test.rs` to satisfy Clippy `-D warnings`.
- **Issue #390**: `settle_payment` correctly handles non-empty `splits` by distributing USDC according to `SettlementSplit.amount` and handles empty splits by transferring the full net amount directly to the merchant.
- **Issue #391**: Implemented `batch_expire_payments` for permissionless expiry sweeps up to 50 payments per call; fixed `expire_payment` to return `PaymentExpired` when attempting to expire a non-expired payment.
- **Issue #392**: Dispute Escalation logic: implemented `escalate_expired_disputes` to flag disputes past their `review_deadline` as escalated, processing up to 20 disputes permissionlessly.
- **Issue #393**: KYC Tier-Based Payment Limits: `MerchantRegistry` now supports `set_tier_limits` and `get_tier_limits`; `PaymentProcessor.create_payment` enforces `AmountBelowMin` and `AmountAboveMax` based on the merchant's tier limits.
- **Issue #394**: Multi-token support — added `set_usdc_token` function to configure a default settlement token and auto-whitelist it; `settle_payment` now supports per-payment token overrides via `payment.token_address`.
- **Issue #413**: Multi-currency invoicing for payment links — added `fiat: Option<FiatConfig>` to `create_link` with `FiatConfig { amount, currency, oracle }` and `MaybeFiatConfig` Soroban-compatible wrapper; `use_link` resolves the USDC equivalent via the FX oracle at exchange time.
- **Issue #395**: Contract upgrade mechanism — added `upgrade` function to `FXOracle`, `MerchantRegistry`, and `PaymentLinkManager` with admin authorization and `CONTRACT/UPGRADED` event emission; enhanced `RefundManager::upgrade_contract` with version tracking and event emission.
- **Issue #406**: Fixed rate-limit window bypass — changed `enforce_create_payment_rate_limit` and related functions from a sliding-window to a fixed-window strategy anchored to the first payment in the window, preventing burst-burst boundary bypass.
- **Issue #399** — Idempotency key TTL: `DataKey::IdempotencyKey` entries are now stored with a TTL derived from the payment expiry window (ledgers ≈ `(expires_at − now) / 5`, minimum `SHORT_LIVE_TTL`) instead of the permanent `LONG_LIVE_TTL`. A new reverse-map key `DataKey::PaymentIdempotencyToken(payment_id)` is also stored so `cancel_payment` and `expire_payment` can proactively remove the forward key, freeing the `client_token` for reuse immediately after a payment is cancelled or expired.
- **Issue #398** — `MerchantRegistry::transfer_admin(current_admin, new_admin)`: allows the current admin to hand off registry ownership atomically. Validates stored admin matches `current_admin`, updates `MerchantDataKey::Admin`, and emits `MERCHANT_REGISTRY/ADMIN_TRANSFERRED`. Added `get_admin()` getter. Old admin loses all privileges immediately after transfer.
- **Issue #397** — Stellar memo type validation in `create_payment`: `memo_type` must be one of `Text`, `Id`, `Hash`, or `Return`; Text memos are rejected if > 28 bytes; Id memos are rejected if not a valid `u64` decimal string. New error codes: `InvalidMemoType = 50`, `MemoTooLong = 51`, `InvalidMemoId = 52`. Validation is a no-op when `memo_type` is `None`.
- **Issue #396** — `get_merchant_payments_full(merchant_id, offset, limit) -> Vec<PaymentCharge>`: paginated getter returning full `PaymentCharge` structs (not just IDs) with `limit` silently capped at 50. Added `get_merchant_payment_count_for_dashboard(merchant_id) -> u32` for pagination UI total-count queries. Empty result (not error) when `offset` exceeds count.

### Storage key layout changes (Issue #399)
- **New key**: `DataKey::PaymentIdempotencyToken(String)` — reverse map from `payment_id` → `client_token`; stored and removed alongside `DataKey::IdempotencyKey`. Existing `IdempotencyKey` entries written by prior contract versions carry `LONG_LIVE_TTL` and will naturally expire; new entries use payment-scoped TTLs.
- `allow_token` unauthorized non-admin test for token allowlist enforcement (closes #328)
- `settle_payment` tests for unauthorized operators, pending/expired rejection, and `PAYMENT/SETTLED` event emission (closes #326)
- `get_merchant_payments_paginated` optional `status_filter` parameter to paginate merchant payments by `PaymentStatus` (closes #280)
- `scripts/deploy_testnet.sh`: builds all contract WASMs and deploys them to the configured Stellar network; writes resulting contract IDs to `.env.testnet`; fails fast if `STELLAR_SECRET_KEY` or `STELLAR_NETWORK` are unset (closes #294)
- `docs/local-invoke.md`: CLI recipe sections for `create_refund`, `process_refund`, `create_dispute`, `set_dispute_deadline`, `resolve_dispute_with_refund`, `verify_payment`, `settle_payment`, `set_paused`, `set_rate`, `create_link`, and `use_link` — each with full command, expected output, and error scenarios (closes #299)
- `docs/local-invoke.md`: Deployment section documenting how to run `scripts/deploy_testnet.sh` and load `.env.testnet`

- `check_dispute_deadline(dispute_id)`: public callable to trigger escalation when a dispute review deadline has elapsed; emits `DISPUTE/ESCALATED` and is a no-op if not passed/already escalated/resolved (closes #306)
- `top_up_stream(stream_id, amount)`: allows a sender to top up a single stream via direct token transfer; credits the stream deposit and emits `STREAM/TOPPED_UP` (closes #305)
- Treasury accounting: refund-time fees now accumulate in `DataKey::TreasuryBalance`; adds `get_treasury_balance()` and `withdraw_treasury(admin, amount, destination)` for admin withdrawals; emits `TREASURY/WITHDRAWN` and introduces `Error::InsufficientTreasuryBalance` (closes #291)
- `create_payments_batch`: atomic batch payment creation API with a maximum batch size of 50 and per-merchant batch rate-limit enforcement; returns payment IDs in order and emits `PAYMENT/CREATED` for each payment (closes #293)

- AccessControl edge-case tests: `renounce_role` idempotency for non-holders, self-role removal, and unauthorized `transfer_admin` validation (closes #337)
- FXOracle role management tests: `oracle_grant_role` by admin/non-admin authorization, `oracle_has_role` verification, and `get_fx_admin` initialized/uninitialized states (closes #335)
- MerchantRegistry pagination tests: `get_all_merchants` offset-based edge cases including beyond-range offsets and zero-limit handling (closes #337)
- PaymentStreaming batch operation tests: `top_up_multiple_streams` authorization and deposit updates, `cancel_multiple_streams` atomicity with invalid IDs, `batch_withdraw_to` multi-destination routing and zero-accrued stream handling (closes #336)
- PaymentLinkManager batch verification tests: `verify_batch` coverage for active/missing/deactivated links and empty input handling (closes #334)

### Changed
- **Issue #394**: `create_link` now accepts 10 parameters (fiat params bundled into `FiatConfig`); `PaymentLink` struct stores `fiat: MaybeFiatConfig`; existing `create_link` and `use_link` signatures updated accordingly.
- **All contracts**: Added `CONTRACT/UPGRADED` event emission pattern for upgradeability.

---
## Unreleased

### Added
- Propose-and-claim admin transfers with compatibility wrappers for existing admin handoff entry points
- Payer-level payment creation rate limiting to reduce spam and abusive retries
- Automatic pending refund creation for overpaid payments
- Dispute bonding so disputes now stake a bond that is returned or collected during resolution
- Role management audit logging events: ROLE_GRANTED, ROLE_REVOKED, ADMIN_TRANSFER_PROPOSED, ADMIN_TRANSFER_COMPLETED, REVOCATION_PENDING, REVOCATION_CANCELLED, ROLE_RENOUNCED, PROPOSAL_CREATED, PROPOSAL_VOTED, RECOVERY_ADMIN_TRANSFER_PROPOSED
- 24‑hour revocation cooldown periods for critical roles (ORACLE, SETTLEMENT_OPERATOR)
- emergency_revoke_role for immediate revocation bypassing cooldown
- finalize_revocation for completing revocation after cooldown
- cancel_revocation for canceling pending revocation
- get_pending_revocation to query pending revocation status
- initialize_with_recovery and set_recovery_key for emergency recovery key management
- recovery_initiate_admin_transfer for recovery‑initiated admin transfer with 30‑day lock‑in
- Multi‑signature support for critical admin functions: create_proposal, vote_proposal, execute_proposal, get_proposal
- set_multisig_config to update threshold and signer list
- get_multisig_config to query current multi‑sig config

## Contract Versions

Each contract exposes a `version() -> u32` function. Bump this value whenever a storage key or struct layout changes in a breaking way.

| Contract            | Current Version |
|---------------------|-----------------|
| `PaymentProcessor`  | 1               |
| `RefundManager`     | 1               |
| `FXOracle`          | 1               |
| `PaymentLinkManager`| 1               |
| `MerchantRegistry`  | 1               |

---

## Storage / Event Breaking Changes

### v1 — Initial release

**Storage keys (`DataKey`):**
- `Payment(String)` → `PaymentCharge`
- `MerchantPayments(Address)` → `Vec<String>`
- `MerchantRateLimit(Address)` → `MerchantCreateRateLimit`
- `Refund(String)` → `Refund`
- `PaymentRefunds(String)` → `Vec<String>`
- `RefundCounter` → `u64`
- `Dispute(String)` → `Dispute`
- `PaymentDisputes(String)` → `Vec<String>`
- `DisputeCounter` → `u64`
- `UsdcToken` → `Address`
- `Paused` → `bool`
- `MerchantRegistryAddress` → `Address`

**Oracle keys (`OracleDataKey`):**
- `Rate(Symbol)` → `RateData` (persistent)
- `StalenessThreshold` → `u64` (instance)

**Link keys (`LinkDataKey`):**
- `Link(String)` → `PaymentLink`

**Merchant keys (`MerchantDataKey`):**
- `Admin` → `Address`
- `Merchant(Address)` → `Merchant`
- `MerchantList` → `Vec<Address>`

**Events (topic tuple → data):**
- `(PAYMENT, CREATED, payment_id)` → `(merchant_id, amount)`
- `(PAYMENT, VERIFIED, payment_id)` → `(merchant_id, amount, amount_received)`
- `(PAYMENT, OVERPAID, payment_id)` → `(merchant_id, amount, amount_received)`
- `(PAYMENT, PARTIALLY_PAID, payment_id)` → `(merchant_id, amount, amount_received)`
- `(PAYMENT, CANCELLED, payment_id)` → `(merchant_id, amount)`
- `(PAYMENT, EXPIRED, payment_id)` → `(merchant_id, amount)`
- `(PAYMENT, SETTLED, payment_id)` → `(merchant_id, amount)`
- `(REFUND, CREATED)` → `(payment_id, refund_id, amount)`
- `(REFUND, COMPLETED)` → `(payment_id, refund_id, amount)`
- `(REFUND, REJECTED)` → `(payment_id, refund_id, amount)`
- `(DISPUTE, OPENED)` → `(payment_id, dispute_id, amount)`
- `(DISPUTE, UNDER_REVIEW)` → `(payment_id, dispute_id, amount)`
- `(DISPUTE, RESOLVED)` → `(payment_id, dispute_id, amount)`
- `(DISPUTE, REJECTED)` → `(payment_id, dispute_id, amount)`
- `(CONTRACT, PAUSED)` → `admin`
- `(CONTRACT, UNPAUSED)` → `admin`
- `(RATE, UPDATED)` → `pair`
- `(LINK, CREATED)` → `(link_id, merchant)`
- `(LINK, USED)` → `(link_id, payer, amount, payment_id)`
- `(LINK, DEACTIVATED)` → `link_id`

---

## Upgrade Checklist

When deploying a new contract version:

1. **Read old state** — call `get_payment`, `get_refund`, etc. on the live contract and confirm structs deserialise correctly with the new code before upgrading.
2. **Bump `version()`** — increment the constant in the relevant `impl` block.
3. **Migrate if needed** — if a `#[contracttype]` struct gains or loses fields, write a one-shot migration entry-point that reads the old layout and rewrites under the new key/struct before the upgrade goes live.
4. **Update this file** — add a new `## v<N>` section documenting every changed key, struct field, or event signature.
5. **Test** — run `cargo test --all-features` and the bounded property tests locally before opening a PR.
