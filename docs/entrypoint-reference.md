# Contract Entrypoint Reference

This is a generated-and-curated reference of every public entrypoint (`pub fn` inside a contract's `#[contractimpl] impl` block) across the FluxaPay contracts, with its signature, authorization requirement, the events it emits, and the `Error` variants it can directly return.

**Scope note:** `fluxapay/src/payment_processor.rs` and `fluxapay/src/refund_manager.rs` each declare their own `#[contract] pub struct PaymentProcessor`/`RefundManager` plus a parallel `impl` block. Those module-local structs are a *different* Rust type from the identically-named `PaymentProcessor`/`RefundManager` declared at the crate root in `lib.rs` (Rust module paths, not a redefinition), and the crate-root versions are the ones registered by every test (`env.register(PaymentProcessor, ())` / `env.register(RefundManager, ())`), re-exported as `PaymentProcessorClient`/`RefundManagerClient`, and deployed. This reference therefore documents the crate-root `impl` blocks in `lib.rs`. `fluxapay/src/payment_streaming.rs` additionally declares a `PaymentStreaming` contract that is never referenced by `pub mod` in `lib.rs` and so is not part of the compiled crate at all; the real `PaymentStreaming` contract lives in `fluxapay/src/stream.rs`.

**Auth** lists which argument(s) the function calls `.require_auth()` on, and which access-control role(s) (see `fluxapay/src/access_control.rs`) it checks via `AccessControl::has_role`. "None" means the function performs no authorization itself — it is either a public read, or it authorizes implicitly through a caller-supplied address argument elsewhere in the call chain.

**Events** are the event names this function emits directly, written as `NAMESPACE/EVENT` for the legacy `env.events().publish((Symbol, Symbol), data)` topic-tuple style (see `#[allow(deprecated)]` on the surrounding `impl` block), or as the `#[contractevent]` struct name for newer modules that have migrated (see `fluxapay/src/events.rs`). Events emitted by a function this entrypoint calls internally (not inline in its own body) are not listed here — see the callee's own entry.

**Error codes** list the `Error` variants referenced directly in the function body. Where a function delegates most of its validation to an internal `*_internal` helper, only the variants visible in the public function's own body are listed; consult `fluxapay/src/errors.rs` (or `docs/error-codes.md`) and the referenced helper for the complete set.

> **Keeping this in sync:** this file is curated from the contract source by parsing each `impl <Contract>` block for `pub fn` signatures, `require_auth`/role checks, event emissions, and `Error::` references, then hand-reviewing the result. Per [CONTRIBUTING.md](../CONTRIBUTING.md#74-entrypoint-reference), any PR that adds, removes, or changes the signature/auth/events of a `pub fn` entrypoint must update the corresponding section of this file in the same PR.

## Contracts

- [`RefundManager`](#refundmanager) — `fluxapay/src/lib.rs` — Refund lifecycle: creation, approval, processing, cancellation, disputes, refund-fee and refund-policy configuration, pause controls, access-control admin.
- [`PaymentProcessor`](#paymentprocessor) — `fluxapay/src/lib.rs` — Payment lifecycle: creation, verification/settlement, expiry, KYC tiers, merchant fee config, swap-and-pay, subscriptions wiring, treasury, access-control admin.
- [`MerchantRegistry`](#merchantregistry) — `fluxapay/src/merchant_registry.rs` — Merchant onboarding, verification, fee/anchor config, rolling reserves, suspension, analytics/ranking.
- [`DexRouter`](#dexrouter) — `fluxapay/src/dex_router.rs` — Allowlisted DEX router registry used by swap-and-pay.
- [`FXOracle`](#fxoracle) — `fluxapay/src/fx_oracle.rs` — Exchange-rate oracle: rate submission, aggregation, staleness/deviation checks.
- [`Htlc`](#htlc) — `fluxapay/src/htlc.rs` — Hash Time-Locked Contracts for cross-chain/conditional payments.
- [`GasEstimator`](#gasestimator) — `fluxapay/src/gas_estimator.rs` — Static operation cost/fee estimates for client-side gas budgeting.
- [`PaymentLinkManager`](#paymentlinkmanager) — `fluxapay/src/payment_link.rs` — Shareable payment-link creation, analytics, and direct-transfer tracking.
- [`PaymentStreaming`](#paymentstreaming) — `fluxapay/src/stream.rs` — Continuous/vesting payment streams, including multi-payee streams.
- [`SubscriptionContract`](#subscriptioncontract) — `fluxapay/src/subscription.rs` — Recurring subscription plans, billing, dunning.

## RefundManager

Source: `fluxapay/src/lib.rs` (112 public entrypoints)

Refund lifecycle: creation, approval, processing, cancellation, disputes, refund-fee and refund-policy configuration, pause controls, access-control admin.

#### `version`

```rust
pub fn version(env: Env) -> String
```

Returns the current contract version string from persistent storage.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_version`

```rust
pub fn set_version(env: Env, admin: Address, version: String) -> Result<(), Error>
```

Admin-only: update the stored contract version after a WASM upgrade.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `CONTRACT/VERSION_UPDATED`
- **Error codes:** `InvalidVersion`, `Unauthorized`

#### `get_dispute_bond_amount`

```rust
pub fn get_dispute_bond_amount(env: Env) -> i128
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_contract_metadata`

```rust
pub fn set_contract_metadata(env: Env, admin: Address, key: Symbol, value: String) -> Result<(), Error>
```

Admin-only: set an arbitrary on-chain metadata entry (issue #667), e.g. a description, deployment notes, or audit commit hash. Stored in instance storage under a caller-chosen key, with the instance TTL bumped to `LONG_LIVE_TTL` so metadata survives archival.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_contract_metadata`

```rust
pub fn get_contract_metadata(env: Env, key: Symbol) -> Option<String>
```

Public read of an on-chain metadata entry set via `set_contract_metadata` (issue #667). Returns `None` if the key was never set.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `initialize_refund_manager`

```rust
pub fn initialize_refund_manager(env: Env, admin: Address, usdc_token_address: Address) -> Result<(), Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `set_refund_fee_bps`

```rust
pub fn set_refund_fee_bps(env: Env, admin: Address, bps: i128) -> Result<(), Error>
```

Admin-only: set the refund processing fee in basis points (0–1000, max 10%).

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidAmount`, `Unauthorized`

#### `get_refund_fee_bps`

```rust
pub fn get_refund_fee_bps(env: Env) -> i128
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_dex_router_address`

```rust
pub fn set_dex_router_address(env: Env, admin: Address, dex_router: Address) -> Result<(), Error>
```

Admin: configure the DEX router used to route swap_and_pay refunds back to the payer's original token (Issue #173).

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `set_refund_policy`

```rust
pub fn set_refund_policy(env: Env, admin: Address, require_receipt_hash: bool) -> Result<(), Error>
```

Admin: require a `receipt_hash` on every refund before `process_refund` will execute it (Issue #176).

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_refund_policy`

```rust
pub fn get_refund_policy(env: Env) -> RefundPolicy
```

Issue #676: read-only view of all refund configuration in one call — `require_receipt_hash` (Issue #176), `refund_expiry_secs` (Issue #170), `refund_fee_bps`, and `cooldown_secs`. Lets integrators check policy before submitting a refund without having to know every individual storage key / setter.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `grant_role`

```rust
pub fn grant_role(env: Env, admin: Address, role: Symbol, account: Address) -> Result<(), Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `grant_merchant_role`

```rust
pub fn grant_merchant_role(env: Env, admin: Address, merchant_id: Address) -> Result<(), Error>
```

- **Auth:** checks role(s) `merchant`
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `revoke_role`

```rust
pub fn revoke_role(env: Env, admin: Address, role: Symbol, account: Address) -> Result<(), Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `revoke_merchant_role`

```rust
pub fn revoke_merchant_role(env: Env, admin: Address, merchant_id: Address) -> Result<(), Error>
```

- **Auth:** checks role(s) `merchant`
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `sync_grant_role_with_processor`

```rust
pub fn sync_grant_role_with_processor(env: Env, admin: Address, payment_processor_address: Address, role: Symbol, account: Address) -> Result<(), Error>
```

Synchronize a role grant across PaymentProcessor and RefundManager.  If either grant fails, the transaction aborts and no changes are persisted.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `ACCESS_CONTROL/SYNC_GRANT`
- **Error codes:** `AccessControlError`, `Unauthorized`

#### `sync_revoke_role_with_processor`

```rust
pub fn sync_revoke_role_with_processor(env: Env, admin: Address, payment_processor_address: Address, role: Symbol, account: Address) -> Result<(), Error>
```

Synchronize a role revoke across PaymentProcessor and RefundManager.  If either revoke fails, the transaction aborts and no changes are persisted.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `ACCESS_CONTROL/SYNC_REVOKE`
- **Error codes:** `AccessControlError`, `Unauthorized`

#### `has_role`

```rust
pub fn has_role(env: Env, role: Symbol, account: Address) -> bool
```

- **Auth:** checks an access-control role
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `renounce_role`

```rust
pub fn renounce_role(env: Env, account: Address, role: Symbol) -> Result<(), Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `propose_admin`

```rust
pub fn propose_admin(env: Env, current_admin: Address, new_admin: Address) -> Result<(), Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `claim_admin`

```rust
pub fn claim_admin(env: Env, new_admin: Address) -> Result<(), Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `accept_admin`

```rust
pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `cancel_admin_transfer`

```rust
pub fn cancel_admin_transfer(env: Env, current_admin: Address) -> Result<(), Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `transfer_admin`

```rust
pub fn transfer_admin(env: Env, current_admin: Address, new_admin: Address) -> Result<(), Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `accept_admin_transfer`

```rust
pub fn accept_admin_transfer(env: Env, new_admin: Address) -> Result<(), Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_admin`

```rust
pub fn get_admin(env: Env) -> Option<Address>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `add_to_blacklist`

```rust
pub fn add_to_blacklist(env: Env, admin: Address, address: Address) -> Result<(), Error>
```

Add an address to the global blacklist (admin only).

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `remove_from_blacklist`

```rust
pub fn remove_from_blacklist(env: Env, admin: Address, address: Address) -> Result<(), Error>
```

Remove an address from the global blacklist (admin only).

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `is_blacklisted`

```rust
pub fn is_blacklisted(env: Env, address: Address) -> bool
```

Returns true when an address is globally blacklisted.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `register_swap_payment`

```rust
pub fn register_swap_payment(env: Env, payment_id: String, merchant_id: Address, amount: i128, currency: Symbol, original_token: Address, swap_path: Vec<Address>) -> Result<(), Error>
```

Like `register_payment`, but also records the original token and swap path used by a `swap_and_pay` payment, so `process_refund` can route the refund back through the DEX to the payer's original token (Issue #173).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `configure_fee_split`

```rust
pub fn configure_fee_split(env: Env, admin: Address, treasury_bps: u32, developer_bps: u32, treasury_address: Address, developer_address: Address) -> Result<(), Error>
```

Issue #168: Configure fee split destinations for platform fees. Admin can set allocation ratios and destination addresses. `treasury_bps + developer_bps` must be ≤ 10 000; any remainder goes to treasury.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `FEE_SPLIT/CONFIGURED`
- **Error codes:** `InvalidAmount`, `Unauthorized`

#### `set_fee_split_config`

```rust
pub fn set_fee_split_config(env: Env, admin: Address, config: FeeSplitConfig) -> Result<(), Error>
```

Struct-based setter for the platform fee split config (alias for `configure_fee_split`). Admin only. `config.treasury_bps + config.developer_bps` must be ≤ 10 000.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `FEE_SPLIT/CONFIGURED`
- **Error codes:** `InvalidAmount`, `Unauthorized`

#### `get_fee_split_config`

```rust
pub fn get_fee_split_config(env: Env) -> Option<FeeSplitConfig>
```

Get the current fee split configuration.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_merchant_registry_address`

```rust
pub fn set_merchant_registry_address(env: Env, admin: Address, registry_address: Address) -> Result<(), Error>
```

Issue #1016: Wire RefundManager to the MerchantRegistry contract so that `process_refund` can look up a merchant's KYC tier and apply the tier-specific refund fee (Basic/Unverified 100 bps, Full 80 bps, Business 50 bps) instead of always falling back to the flat default. Admin only.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_merchant_registry_address`

```rust
pub fn get_merchant_registry_address(env: Env) -> Option<Address>
```

Get the MerchantRegistry contract address used for tiered refund fee lookups.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_role_members`

```rust
pub fn get_role_members(env: Env, role: Symbol) -> Vec<Address>
```

Returns all addresses currently holding the given role (issue #37).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `propose_fee_update`

```rust
pub fn propose_fee_update(env: Env, admin: Address, new_fee: i128) -> Result<(), Error>
```

- **Auth:** Requires `admin.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `finalize_fee_update`

```rust
pub fn finalize_fee_update(env: Env, admin: Address) -> Result<(), Error>
```

- **Auth:** Requires `admin.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `FeeProposalNotReady`, `NoFeeProposal`, `Unauthorized`

#### `register_payment`

```rust
pub fn register_payment(env: Env, payment_id: String, merchant_id: Address, amount: i128, currency: Symbol)
```

Register a payment with the refund manager so refund amounts can be validated.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `register_payment_with_payer`

```rust
pub fn register_payment_with_payer(env: Env, payment_id: String, merchant_id: Address, payer: Address, amount: i128, currency: Symbol)
```

Helper for tests to register a confirmed payment with an explicit payer address.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `queue_auto_refund`

```rust
pub fn queue_auto_refund(env: Env, caller: Address, registry_address: Address, payment_id: String, refund_amount: i128, requester: Address, reason: String) -> Result<String, Error>
```

- **Auth:** Requires `caller.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `create_refund`

```rust
pub fn create_refund(env: Env, payment_id: String, refund_amount: i128, reason: String, requester: Address) -> Result<String, Error>
```

- **Auth:** Requires `requester.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `create_refund_idempotent`

```rust
pub fn create_refund_idempotent(env: Env, payment_id: String, refund_amount: i128, reason: String, requester: Address, idempotency_key: Option<String>) -> Result<String, Error>
```

Issue #638: Create a refund request with an optional idempotency key.  When `idempotency_key` is `Some`, the key is persisted for 30 days: * Retrying with the same key **and** the same `(payment_id, refund_amount, reason)` returns the original `refund_id` without creating a duplicate. * Reusing the key with different parameters returns `Error::DuplicateIdempotencyKey`.  Passing `None` is exactly equivalent to `create_refund` (backward compatible).

- **Auth:** Requires `requester.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `create_refund_with_receipt`

```rust
pub fn create_refund_with_receipt(env: Env, payment_id: String, refund_amount: i128, reason: String, requester: Address, receipt_hash: Option<BytesN<32>>, idempotency_key: Option<String>) -> Result<String, Error>
```

Create a refund request with optional receipt hash metadata.  Issue #638: also accepts an optional `idempotency_key` with the same semantics as `create_refund_idempotent`.

- **Auth:** Requires `requester.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `process_refund`

```rust
pub fn process_refund(env: Env, operator: Address, refund_id: String) -> Result<(), Error>
```

- **Auth:** Requires `operator.require_auth()`; checks role(s) `oracle`, `settlement_operator`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `batch_process_refunds`

```rust
pub fn batch_process_refunds(env: Env, operator: Address, refund_ids: Vec<String>, max_batch: u32) -> Result<Vec<RefundBatchItemResult>, Error>
```

Batch-process up to `max_batch` pending refunds in a single call.  Issue #1199: there were batch entrypoints for payments (`batch_expire`, `create_payments_batch`) and disputes (`batch_create_disputes`), but refund processing still required one invocation per refund. This reduces per-refund invocation overhead for operators settling many approved refunds at once.  Processes up to `max_batch` items (hard cap [`MAX_REFUND_BATCH`]). Each `refund_id` is authorized exactly like [`Self::process_refund`] (the caller must hold the settlement-operator or oracle role, or be the refund's own requester with an already-approved refund) and is then processed independently via `process_refund_internal`; a failure on one item does not revert previously-succeeded items in the same batch. Returns a per-item [`RefundBatchItemResult`] so callers can tell which refunds completed and which failed (and why). Emits `REFUND/BATCH_PROCESSED` with `(success_count, fail_count)`.

- **Auth:** Requires `operator.require_auth()`; checks role(s) `oracle`, `settlement_operator`
- **Events:** `REFUND/BATCH_PROCESSED`
- **Error codes:** `BatchTooLarge`, `Unauthorized`

#### `get_treasury_balance`

```rust
pub fn get_treasury_balance(env: Env) -> i128
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_treasury_withdrawal_history`

```rust
pub fn get_treasury_withdrawal_history(env: Env, offset: u32, limit: u32) -> Vec<TreasuryWithdrawal>
```

Return a page of treasury withdrawal history (newest-first). `offset` skips the first N records; `limit` caps the page size (max 100).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `withdraw_treasury`

```rust
pub fn withdraw_treasury(env: Env, admin: Address, amount: i128, destination: Address) -> Result<(), Error>
```

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `TREASURY/WITHDRAWN`
- **Error codes:** `InsufficientTreasuryBalance`, `InvalidAmount`, `Unauthorized`

#### `reject_refund`

```rust
pub fn reject_refund(env: Env, operator: Address, refund_id: String) -> Result<(), Error>
```

Reject a pending refund (operator only). Emits REFUND/REJECTED (issue #27).

- **Auth:** Requires `operator.require_auth()`; checks role(s) `oracle`, `settlement_operator`
- **Events:** `REFUND/REJECTED`
- **Error codes:** `RefundAlreadyProcessed`, `Unauthorized`

#### `expire_refund`

```rust
pub fn expire_refund(env: Env, operator: Address, refund_id: String) -> Result<(), Error>
```

Clean up a pending refund that has passed its `expiry_at` deadline (Issue #170). Marks it `Rejected` so it no longer blocks the payment's refundable balance. Callable by the same roles as `process_refund`/`reject_refund`.

- **Auth:** Requires `operator.require_auth()`; checks role(s) `oracle`, `settlement_operator`
- **Events:** `REFUND/EXPIRED`
- **Error codes:** `RefundAlreadyProcessed`, `RefundExpired`, `Unauthorized`

#### `approve_refund`

```rust
pub fn approve_refund(env: Env, operator: Address, refund_id: String) -> Result<(), Error>
```

Issue #171: Approve a pending refund, allowing customer to claim it. Operator marks the refund as approved without processing it immediately. Customer can then call process_refund to claim the approved refund.

- **Auth:** Requires `operator.require_auth()`; checks role(s) `oracle`, `settlement_operator`
- **Events:** `REFUND/APPROVED`
- **Error codes:** `RefundAlreadyProcessed`, `Unauthorized`

#### `claim_refund`

```rust
pub fn claim_refund(env: Env, requester: Address, refund_id: String) -> Result<(), Error>
```

Issue #450: Customer self-serves an operator-approved refund.  Callable only by the original refund requester, and only once an operator has called `approve_refund`. `process_refund` remains available for operators/oracles who need to execute a refund directly without waiting for the customer to claim it.

- **Auth:** Requires `requester.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `RefundAlreadyProcessed`, `RefundNotApproved`, `Unauthorized`

#### `set_refund_expiry`

```rust
pub fn set_refund_expiry(env: Env, admin: Address, secs: u64) -> Result<(), Error>
```

Admin-configurable refund expiry window in seconds (Issue #170). Applies to refunds created after this call.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidAmount`, `Unauthorized`

#### `refund_instantly`

```rust
pub fn refund_instantly(env: Env, merchant_id: Address, payment_id: String, refund_amount: i128, reason: String, registry_address: Address) -> Result<String, Error>
```

Cancel a pending refund. Caller must be the refund requester (merchant) or contract admin. Removes the refund from the payment's pending list and emits REFUND/CANCELLED. Instantly refund a payment without operator approval.  Only merchants with KYC tier `Full` or `Business` may call this. The merchant must be the `merchant_id` on the original payment. Executes the USDC transfer immediately (no `Pending` state).

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** `REFUND/COMPLETED`
- **Error codes:** `DirectTransferNotDisputable`, `PaymentAlreadyProcessed`, `PaymentNotFound`, `Unauthorized`

#### `cancel_refund`

```rust
pub fn cancel_refund(env: Env, caller: Address, refund_id: String) -> Result<(), Error>
```

- **Auth:** Requires `caller.require_auth()`; checks role(s) `admin`
- **Events:** `REFUND/CANCELLED`
- **Error codes:** `RefundAlreadyProcessed`, `RefundCancelled`, `Unauthorized`

#### `get_refund`

```rust
pub fn get_refund(env: Env, refund_id: String) -> Result<Refund, Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_payment_refunds`

```rust
pub fn get_payment_refunds(env: Env, payment_id: String) -> Result<Vec<Refund>, Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `create_dispute`

```rust
pub fn create_dispute(env: Env, payment_id: String, amount: i128, reason: String, evidence: String, disputer: Address, payout_splits: Vec<SettlementSplit>) -> Result<String, Error>
```

- **Auth:** Requires `disputer.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `batch_create_disputes`

```rust
pub fn batch_create_disputes(env: Env, disputes: Vec<CreateDisputeArgs>, max_batch: u32) -> Result<Vec<DisputeBatchItemResult>, Error>
```

Batch-create disputes for marketplace bulk filing.  Processes up to `max_batch` items (hard cap 20). Each item is handled identically to `create_dispute`; failures do not revert successes. Bonds are deducted only for successful disputes. Emits `DISPUTE/BATCH_CREATED` with `(success_count, fail_count)`.

- **Auth:** Requires `disputer.require_auth()`
- **Events:** `DISPUTE/BATCH_CREATED`
- **Error codes:** `BatchTooLarge`

#### `review_dispute`

```rust
pub fn review_dispute(env: Env, operator: Address, dispute_id: String) -> Result<(), Error>
```

- **Auth:** Requires `operator.require_auth()`; checks role(s) `oracle`, `settlement_operator`
- **Events:** `DISPUTE/REVIEWED`
- **Error codes:** `DisputeAlreadyResolved`, `Unauthorized`

#### `set_dispute_rate_limits`

```rust
pub fn set_dispute_rate_limits(env: Env, admin: Address, per_payer: u32, global_per_hour: u32) -> Result<(), Error>
```

Configure dispute creation rate limits (admin only).  * `per_payer` — max concurrent open/under-review disputes per disputer * `global_per_hour` — max dispute creations per hour across all disputers

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `set_require_evidence_cid`

```rust
pub fn set_require_evidence_cid(env: Env, admin: Address, require_cid: bool) -> Result<(), Error>
```

Toggle whether non-empty dispute evidence must be a valid IPFS CID. Set `false` for testnet/dev to accept arbitrary evidence strings.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `set_dispute_threshold`

```rust
pub fn set_dispute_threshold(env: Env, admin: Address, amount: i128) -> Result<(), Error>
```

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `DISPUTE/THRESHOLD_SET`
- **Error codes:** `InvalidAmount`, `Unauthorized`

#### `set_dispute_deadline`

```rust
pub fn set_dispute_deadline(env: Env, operator: Address, dispute_id: String, deadline: u64) -> Result<(), Error>
```

Operator-only: set or update the review deadline for an open or under-review dispute. Emits DISPUTE/DEADLINE_SET. If the current ledger time already exceeds the deadline, the dispute is also flagged as escalated and DISPUTE/ESCALATED is emitted.

- **Auth:** Requires `operator.require_auth()`; checks role(s) `oracle`, `settlement_operator`
- **Events:** `DISPUTE/DEADLINE_SET`, `DISPUTE/ESCALATED`
- **Error codes:** `DisputeAlreadyResolved`, `Unauthorized`

#### `set_dispute_payout_splits`

```rust
pub fn set_dispute_payout_splits(env: Env, operator: Address, dispute_id: String, splits: Vec<SettlementSplit>) -> Result<(), Error>
```

Operator: configure multi-party payout splits for a marketplace dispute. Splits must sum to exactly `dispute.amount`; validated again at resolution time in case the dispute amount changes. (Issue #446)

- **Auth:** Requires `operator.require_auth()`; checks role(s) `oracle`, `settlement_operator`
- **Events:** _none observed in this function body_
- **Error codes:** `DisputeAlreadyResolved`, `InvalidSplitSum`, `Unauthorized`

#### `check_dispute_deadline`

```rust
pub fn check_dispute_deadline(env: Env, dispute_id: String) -> Result<(), Error>
```

Anyone may call this to trigger escalation after a dispute review deadline elapses.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `escalate_expired_disputes`

```rust
pub fn escalate_expired_disputes(env: Env, dispute_ids: Vec<String>) -> u32
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `resolve_dispute_with_refund`

```rust
pub fn resolve_dispute_with_refund(env: Env, operator: Address, dispute_id: String, resolution_notes: String, operator_signature: String) -> Result<String, Error>
```

- **Auth:** Requires `operator.require_auth()`; checks role(s) `oracle`, `settlement_operator`
- **Events:** `DISPUTE/ADJUSTMENT`, `DISPUTE/OPERATOR_NOTE`, `DISPUTE/RESOLVED`, `DISPUTE/SPLIT_RESOLVED`
- **Error codes:** `DisputeAlreadyResolved`, `InvalidSplitSum`, `Unauthorized`

#### `batch_resolve_disputes`

```rust
pub fn batch_resolve_disputes(env: Env, operator: Address, resolutions: Vec<DisputeResolution>) -> Result<Vec<Result<(), Error>>, Error>
```

Resolve up to `MAX_DISPUTE_BATCH` disputes independently.  The favored party must be either the customer who filed the dispute or the merchant. Their bond is returned and the other party's bond is forfeited to the admin/treasury. Invalid or already resolved disputes return an item-level error without preventing later items from running.

- **Auth:** Requires `operator.require_auth()`; checks role(s) `admin`, `settlement_operator`
- **Events:** _none observed in this function body_
- **Error codes:** `BatchTooLarge`, `Unauthorized`

#### `reject_dispute`

```rust
pub fn reject_dispute(env: Env, operator: Address, dispute_id: String, resolution_notes: String, operator_signature: String) -> Result<(), Error>
```

- **Auth:** Requires `operator.require_auth()`; checks role(s) `oracle`, `settlement_operator`
- **Events:** `DISPUTE/REJECTED`
- **Error codes:** `DisputeAlreadyResolved`, `InputTooLong`, `Unauthorized`

#### `get_dispute_operator_note`

```rust
pub fn get_dispute_operator_note(env: Env, dispute_id: String) -> Result<DisputeOperatorNote, Error>
```

Retrieve the persisted operator note for a dispute.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `DisputeNotFound`

#### `settle_dispute_collaboratively`

```rust
pub fn settle_dispute_collaboratively(env: Env, dispute_id: String, settlement_amount: i128, buyer_pubkey: BytesN<32>, signature_buyer: BytesN<64>, merchant_pubkey: BytesN<32>, signature_merchant: BytesN<64>) -> Result<String, Error>
```

Close a dispute instantly when both the buyer and merchant have agreed on a settlement amount off-chain and submit their Ed25519 signatures.  The message that both parties must sign is: `SHA-256( dispute_id_bytes || settlement_amount_bytes )` where `settlement_amount_bytes` is the little-endian 16-byte encoding of the `i128` settlement amount.  # Parameters * `dispute_id`         – The dispute to settle. * `settlement_amount`  – Agreed amount to refund to the buyer (≤ disputed amount). * `buyer_pubkey`       – Ed25519 public key of the buyer (32 bytes). * `signature_buyer`    – Ed25519 signature from the buyer (64 bytes). * `merchant_pubkey`    – Ed25519 public key of the merchant (32 bytes). * `signature_merchant` – Ed25519 signature from the merchant (64 bytes).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** `DISPUTE/COLLABORATIVE_SETTLED`
- **Error codes:** `DisputeAlreadyResolved`, `InvalidAmount`

#### `get_collaborative_settlement`

```rust
pub fn get_collaborative_settlement(env: Env, dispute_id: String) -> Result<CollaborativeSettlement, Error>
```

Retrieve the collaborative settlement record for a dispute.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `DisputeNotFound`

#### `get_merchant_dispute_count`

```rust
pub fn get_merchant_dispute_count(env: Env, merchant_id: Address) -> u64
```

Issue #184: Get the current dispute count for a merchant.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_merchant_payment_count`

```rust
pub fn get_merchant_payment_count(env: Env, merchant_id: Address) -> u64
```

Issue #184: Get the current confirmed payment count for a merchant.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `lock_stake`

```rust
pub fn lock_stake(env: Env, arbitrator: Address, dispute_id: String, token: Address, amount: i128) -> Result<(), Error>
```

Lock a governance-token stake to participate in dispute voting.  The arbitrator transfers `amount` tokens into the contract as a stake. The stake is slashed if the arbitrator votes against the majority.  # Parameters * `arbitrator`  – Address locking the stake; must sign. * `dispute_id`  – Dispute to vote on. * `token`       – Governance token contract address. * `amount`      – Amount to lock (must be > 0).

- **Auth:** Requires `arbitrator.require_auth()`; checks role(s) `arbitrator`
- **Events:** `DISPUTE/STAKE_LOCKED`
- **Error codes:** `DisputeAlreadyResolved`, `InvalidAmount`, `Unauthorized`

#### `cast_vote`

```rust
pub fn cast_vote(env: Env, arbitrator: Address, dispute_id: String, choice: VoteChoice) -> Result<(), Error>
```

Cast a stake-weighted vote on a dispute.  The arbitrator must have locked a stake first via `lock_stake`. Each arbitrator may only vote once per dispute.  # Parameters * `arbitrator` – Voting arbitrator; must sign. * `dispute_id` – Dispute to vote on. * `choice`     – `VoteChoice::Favour` or `VoteChoice::Against`.

- **Auth:** Requires `arbitrator.require_auth()`; checks role(s) `arbitrator`
- **Events:** `DISPUTE/VOTE_CAST`
- **Error codes:** `DisputeAlreadyResolved`, `Unauthorized`

#### `finalize_dispute_vote`

```rust
pub fn finalize_dispute_vote(env: Env, operator: Address, dispute_id: String, token: Address, arbitrators: Vec<Address>) -> Result<(), Error>
```

Finalize a dispute based on stake-weighted votes.  The majority side wins. Arbitrators who voted against the majority lose 10% of their stake (slashed to the contract admin). Winners receive their stake back.  # Parameters * `operator`    – Settlement operator or oracle; must sign. * `dispute_id`  – Dispute to finalize. * `token`       – Governance token used for stakes. * `arbitrators` – List of all arbitrators who participated.

- **Auth:** Requires `operator.require_auth()`; checks role(s) `oracle`, `settlement_operator`
- **Events:** `DISPUTE/VOTE_FINALIZED`
- **Error codes:** `DisputeAlreadyResolved`, `Unauthorized`

#### `vote_dispute`

```rust
pub fn vote_dispute(env: Env, arbitrator: Address, dispute_id: String, choice: ArbitratorVoteChoice) -> Result<(), Error>
```

Cast a role-gated vote on a dispute. Unlike [`Self::cast_vote`] (which is stake-weighted), this flow simply counts one vote per `ARBITRATOR`-role address and auto-executes the resolution as soon as either side reaches [`ARBITRATOR_VOTING_THRESHOLD`].  # Parameters * `arbitrator` – Must hold the `ARBITRATOR` role; must sign. * `dispute_id` – Dispute to vote on; must currently be `UnderReview`. * `choice`     – `ArbitratorVoteChoice::Approve` or `::Reject`.

- **Auth:** Requires `arbitrator.require_auth()`; checks role(s) `arbitrator`
- **Events:** `DISPUTE/VOTE_CAST`
- **Error codes:** `AlreadyVoted`, `DisputeAlreadyResolved`, `Unauthorized`

#### `get_vote_tally`

```rust
pub fn get_vote_tally(env: Env, dispute_id: String) -> VoteTally
```

Get the current vote tally for a dispute.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_dispute`

```rust
pub fn get_dispute(env: Env, dispute_id: String) -> Result<Dispute, Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `submit_evidence`

```rust
pub fn submit_evidence(env: Env, admin: Address, dispute_id: String, submitter: Address, evidence_hash: BytesN<32>) -> Result<(), Error>
```

Anchor the SHA-256 hash of uploaded evidence for a dispute. An admin relayer may submit only on behalf of a recorded participant.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `DISPUTE/EVIDENCE_SUBMITTED`
- **Error codes:** `Unauthorized`

#### `get_dispute_evidence`

```rust
pub fn get_dispute_evidence(env: Env, dispute_id: String) -> Vec<BytesN<32>>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_payment_disputes`

```rust
pub fn get_payment_disputes(env: Env, payment_id: String) -> Result<Vec<Dispute>, Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_payment_disputes_by_status`

```rust
pub fn get_payment_disputes_by_status(env: Env, payment_id: String, status: DisputeStatus) -> Result<Vec<Dispute>, Error>
```

Issue #575: Get disputes for a payment filtered by dispute status.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_dispute_summary`

```rust
pub fn get_dispute_summary(env: Env) -> DisputeSummary
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `submit_arbitrator_vote`

```rust
pub fn submit_arbitrator_vote(env: Env, dispute_id: String, arbitrator: Address, vote: ArbitratorVoteChoice) -> Result<(), Error>
```

Issue #178: Submit an arbitrator vote on a dispute resolution.

- **Auth:** Requires `arbitrator.require_auth()`; checks role(s) `arbitrator`
- **Events:** `DISPUTE/ARBITRATOR_VOTE`
- **Error codes:** `DisputeAlreadyResolved`, `InvalidAmount`, `Unauthorized`

#### `check_arbitration_threshold`

```rust
pub fn check_arbitration_threshold(env: Env, dispute_id: String) -> Result<bool, Error>
```

Issue #178: Check arbitrator voting threshold and auto-resolve if met.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `ArbitrationVotingThresholdNotMet`

#### `create_subscription_plan`

```rust
pub fn create_subscription_plan(env: Env, merchant: Address, plan_id: String, name: String, description: String, amount: i128, currency: Symbol, billing_interval: BillingInterval, trial_days: Option<u32>) -> Result<(), Error>
```

- **Auth:** Requires `merchant.require_auth()`; checks role(s) `merchant`
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidAmount`, `TrialTooLong`, `Unauthorized`

#### `create_plan`

```rust
pub fn create_plan(env: Env, merchant: Address, plan_id: String, name: String, description: String, amount: i128, currency: Symbol, interval_secs: u64) -> Result<(), Error>
```

Create a billing plan with an explicit interval in seconds.

- **Auth:** Requires `merchant.require_auth()`; checks role(s) `merchant`
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidAmount`, `Unauthorized`

#### `get_subscription_plan`

```rust
pub fn get_subscription_plan(env: Env, plan_id: String) -> Result<SubscriptionPlan, Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `PaymentNotFound`

#### `get_plan`

```rust
pub fn get_plan(env: Env, plan_id: String) -> Result<SubscriptionPlan, Error>
```

Alias for `get_subscription_plan`.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `deactivate_subscription_plan`

```rust
pub fn deactivate_subscription_plan(env: Env, merchant: Address, plan_id: String) -> Result<(), Error>
```

- **Auth:** Requires `merchant.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `PaymentNotFound`, `Unauthorized`

#### `subscribe`

```rust
pub fn subscribe(env: Env, payer: Address, plan_id: String, max_payments: Option<u32>, affiliate: Option<Address>, affiliate_fee_bps: Option<u32>) -> Result<String, Error>
```

- **Auth:** Requires `payer.require_auth()`
- **Events:** `SUBSCRIPTION/CREATED`, `SUBSCRIPTION/TRIAL_STARTED`
- **Error codes:** `PaymentAlreadyProcessed`, `PaymentNotFound`

#### `subscribe_to_plan`

```rust
pub fn subscribe_to_plan(env: Env, payer: Address, subscription_id: String, plan_id: String) -> Result<(), Error>
```

Subscribe to a plan using a caller-supplied subscription identifier.

- **Auth:** Requires `payer.require_auth()`
- **Events:** `SUBSCRIPTION/CREATED`, `SUBSCRIPTION/TRIAL_STARTED`
- **Error codes:** `PaymentAlreadyExists`, `PaymentAlreadyProcessed`, `PaymentNotFound`

#### `get_subscription`

```rust
pub fn get_subscription(env: Env, subscription_id: String) -> Result<Subscription, Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_payer_subscriptions`

```rust
pub fn get_payer_subscriptions(env: Env, payer: Address) -> Vec<Subscription>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_plan_subscribers`

```rust
pub fn get_plan_subscribers(env: Env, plan_id: String, offset: u32, limit: u32, include_cancelled: bool) -> Vec<Subscription>
```

Issue #633: List subscribers to a plan, paginated.  Returns up to `limit` `Subscription` records (hard-capped at 100 per call) starting at `offset` within the plan's subscriber index, in subscription order. When `include_cancelled` is `false`, subscriptions with `status == Cancelled` are filtered out *before* pagination, so the page always contains `limit` live subscribers when that many remain.  Used by merchants for plan-level analytics and bulk notifications.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `pause_subscription`

```rust
pub fn pause_subscription(env: Env, payer: Address, subscription_id: String) -> Result<(), Error>
```

- **Auth:** Requires `payer.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `PaymentAlreadyProcessed`, `Unauthorized`

#### `pause_with_resume_date`

```rust
pub fn pause_with_resume_date(env: Env, payer: Address, subscription_id: String, resume_timestamp: u64) -> Result<(), Error>
```

- **Auth:** Requires `payer.require_auth()`
- **Events:** `SUBSCRIPTION/PAUSED`
- **Error codes:** `InvalidResumeTimestamp`, `PaymentAlreadyProcessed`, `Unauthorized`

#### `charge_subscription`

```rust
pub fn charge_subscription(env: Env, operator: Address, subscription_id: String, token: Address) -> Result<SubscriptionStatus, Error>
```

Attempt to charge a subscription.  Handles the full lifecycle including: - Auto-resuming a paused subscription whose `resume_at` has passed. - Pulling the due amount via a pre-authorization (if one exists) or directly via the token contract. - On insufficient balance: entering a grace period with up to `SUBSCRIPTION_MAX_RETRIES` retries spaced `SUBSCRIPTION_RETRY_INTERVAL_SECS` apart before marking the subscription as `Cancelled`.  # Parameters * `operator`        – Oracle or settlement operator; must sign. * `subscription_id` – Subscription to charge. * `token`           – Token contract to pull payment from.

- **Auth:** Requires `operator.require_auth()`; checks role(s) `oracle`, `settlement_operator`
- **Events:** `SUBSCRIPTION/CANCELLED_MAX_RETRIES`, `SUBSCRIPTION/CHARGED`, `SUBSCRIPTION/EXPIRED`, `SUBSCRIPTION/PAYMENT_FAILED`, `SUBSCRIPTION/RESUMED`, `SUBSCRIPTION/TRIAL_ENDED`
- **Error codes:** `SubscriptionInGracePeriod`, `SubscriptionRetryExhausted`, `TrialActive`, `Unauthorized`

#### `process_subscription`

```rust
pub fn process_subscription(env: Env, operator: Address, subscription_id: String) -> Result<SubscriptionStatus, Error>
```

Trigger a recurring subscription charge when the billing date is due.

- **Auth:** Requires `operator.require_auth()`; checks role(s) `oracle`, `settlement_operator`
- **Events:** _none observed in this function body_
- **Error codes:** `PaymentAlreadyProcessed`, `PaymentNotFound`, `Unauthorized`

#### `resume_subscription`

```rust
pub fn resume_subscription(env: Env, payer: Address, subscription_id: String) -> Result<(), Error>
```

- **Auth:** Requires `payer.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `PaymentAlreadyProcessed`, `Unauthorized`

#### `cancel_subscription`

```rust
pub fn cancel_subscription(env: Env, payer_or_merchant: Address, subscription_id: String, refund_remaining: bool) -> Result<(), Error>
```

Cancel a subscription and optionally create a prorated pending refund.  When `refund_remaining` is true, a payment was made in the current billing period, and the admin policy `allow_prorated_refunds` is enabled, a pending refund is created for the unused portion of the period (by whole days).

- **Auth:** Requires `payer_or_merchant.require_auth()`
- **Events:** `SUBSCRIPTION/CANCELLED`
- **Error codes:** `PaymentAlreadyProcessed`, `Unauthorized`

#### `set_allow_prorated_refunds`

```rust
pub fn set_allow_prorated_refunds(env: Env, admin: Address, allow: bool) -> Result<(), Error>
```

Admin policy: enable or disable prorated refunds on subscription cancel.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_allow_prorated_refunds`

```rust
pub fn get_allow_prorated_refunds(env: Env) -> bool
```

Query whether prorated subscription refunds are allowed.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `admin_reactivate_subscription`

```rust
pub fn admin_reactivate_subscription(env: Env, admin: Address, subscription_id: String) -> Result<(), Error>
```

Admin override to reactivate a subscription that was cancelled due to max retries. Resets retry_count to 0 and reschedules the next payment.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `SUBSCRIPTION/REACTIVATED`
- **Error codes:** `PaymentAlreadyProcessed`, `Unauthorized`

#### `submit_usage_metrics`

```rust
pub fn submit_usage_metrics(env: Env, operator: Address, subscription_id: String, units_used: i128, unit_price: i128, token: Address) -> Result<SubscriptionStatus, Error>
```

Submit usage metrics for a metered subscription.  Operators call this to record usage units consumed since the last billing cycle. The subscription amount is scaled by `units_used * unit_price` and charged immediately via `charge_subscription`.  # Parameters * `operator`         – Must hold oracle or settlement_operator role. * `subscription_id`  – Target subscription. * `units_used`       – Number of usage units consumed this period. * `unit_price`       – Price per unit in the subscription token's smallest unit. * `token`            – Token contract address used for the charge.

- **Auth:** Requires `operator.require_auth()`; checks role(s) `oracle`, `settlement_operator`
- **Events:** `SUBSCRIPTION/USAGE_RECORDED`
- **Error codes:** `InvalidAmount`, `InvalidStatusTransition`, `Unauthorized`

#### `get_usage_metrics`

```rust
pub fn get_usage_metrics(env: Env, subscription_id: String, from_timestamp: u64, to_timestamp: u64) -> Vec<UsageMetrics>
```

Issue #664: Return usage-metric records for `subscription_id` recorded within `[from_timestamp, to_timestamp]` (inclusive), oldest first. Returns an empty vector if none were recorded, or if the subscription itself doesn't exist.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `process_due_subscriptions`

```rust
pub fn process_due_subscriptions(env: Env, operator: Address) -> Result<u32, Error>
```

Issue #302: Process due subscriptions - iterate ActiveSubscriptions and create a payment record for each due subscription.

- **Auth:** Requires `operator.require_auth()`; checks role(s) `oracle`, `settlement_operator`
- **Events:** `PAYMENT/CREATED`, `SUBSCRIPTION/CANCELLED`, `SUBSCRIPTION/COMPLETED`, `SUBSCRIPTION/TICKED`
- **Error codes:** `Unauthorized`

#### `upgrade_contract`

```rust
pub fn upgrade_contract(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<String, Error>
```

Queue a contract WASM upgrade via the timelock.  Issue #624: `upgrade_contract` no longer takes effect immediately.  The upgrade is queued as a `PendingTimelockAction` and can only be executed after the configured delay (default 48 hours) via `execute_timelocked_action`.  Returns the action ID.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

---

## PaymentProcessor

Source: `fluxapay/src/lib.rs` (180 public entrypoints)

Payment lifecycle: creation, verification/settlement, expiry, KYC tiers, merchant fee config, swap-and-pay, subscriptions wiring, treasury, access-control admin.

#### `version`

```rust
pub fn version(env: Env) -> String
```

Returns the current contract version string from persistent storage. Falls back to INITIAL_CONTRACT_VERSION if not set.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_version`

```rust
pub fn set_version(env: Env, admin: Address, version: String) -> Result<(), Error>
```

Admin-only: update the stored contract version after a WASM upgrade.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `CONTRACT/VERSION_UPDATED`
- **Error codes:** `InvalidVersion`, `Unauthorized`

#### `get_version`

```rust
pub fn get_version(env: Env) -> String
```

Alias for version() — returns the current contract version string.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_contract_health`

```rust
pub fn get_contract_health(env: Env) -> ContractHealth
```

Issue #683: Return a summary of key contract metrics for dashboards and monitoring. No authentication required — this is a public read.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_contract_metadata`

```rust
pub fn set_contract_metadata(env: Env, admin: Address, key: Symbol, value: String) -> Result<(), Error>
```

Admin-only: set an arbitrary on-chain metadata entry (issue #667), e.g. a description, deployment notes, or audit commit hash. Stored in instance storage under a caller-chosen key, with the instance TTL bumped to `LONG_LIVE_TTL` so metadata survives archival.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_contract_metadata`

```rust
pub fn get_contract_metadata(env: Env, key: Symbol) -> Option<String>
```

Public read of an on-chain metadata entry set via `set_contract_metadata` (issue #667). Returns `None` if the key was never set.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `initialize_payment_processor`

```rust
pub fn initialize_payment_processor(env: Env, admin: Address) -> Result<(), Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `set_merchant_registry_address`

```rust
pub fn set_merchant_registry_address(env: Env, admin: Address, registry_address: Address) -> Result<(), Error>
```

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `set_swap_limits`

```rust
pub fn set_swap_limits(env: Env, admin: Address, default_max_slippage_bps: u32, max_price_impact_bps: u32, max_swap_volume_per_ledger: i128) -> Result<(), Error>
```

Admin: configure swap slippage, oracle price-impact, and per-ledger volume limits.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidAmount`, `Unauthorized`

#### `get_swap_limits`

```rust
pub fn get_swap_limits(env: Env) -> SwapLimits
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_fx_oracle`

```rust
pub fn set_fx_oracle(env: Env, admin: Address, fx_oracle: Address) -> Result<(), Error>
```

Admin: configure the FX oracle used to snapshot rates during `verify_payment` (Issue #304).

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `set_fee_rate`

```rust
pub fn set_fee_rate(env: Env, admin: Address, bps: i128) -> Result<String, Error>
```

Queue a settlement fee rate change via the timelock.  Issue #624: `set_fee_rate` no longer takes effect immediately.  Instead it enqueues a `PendingTimelockAction` that can only be executed after the configured timelock delay (default 48 hours) has elapsed.  Returns the action ID assigned to the pending action.  # Arguments * `admin` – Must hold the admin role. * `bps`   – Fee in basis points (e.g. 100 = 1 %). Must be 0–10 000.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidAmount`, `Unauthorized`

#### `set_auto_refund_overpayment`

```rust
pub fn set_auto_refund_overpayment(env: Env, admin: Address, enabled: bool) -> Result<(), Error>
```

Admin-only: enable or disable automatic pending refund creation for overpaid payments. When enabled (default), any payment verified as Overpaid will automatically create a pending refund for the excess amount and emit a REFUND/AUTO_CREATED event.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_auto_refund_overpayment`

```rust
pub fn get_auto_refund_overpayment(env: &Env) -> bool
```

Check whether automatic refund creation for overpaid payments is enabled. Defaults to true if not explicitly configured.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_min_payment_duration_secs`

```rust
pub fn set_min_payment_duration_secs(env: Env, admin: Address, min_secs: u64) -> Result<(), Error>
```

Return the accumulated treasury balance collected via settlement fees and platform fees (when no custom fee_recipient).

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidAmount`, `Unauthorized`

#### `set_max_payment_duration_secs`

```rust
pub fn set_max_payment_duration_secs(env: Env, admin: Address, max_secs: u64) -> Result<(), Error>
```

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidAmount`, `Unauthorized`

#### `get_treasury_balance`

```rust
pub fn get_treasury_balance(env: Env) -> i128
```

Return the accumulated treasury balance collected via settlement fees. Uses the default USDC token for backward compatibility.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_treasury_withdrawal_history`

```rust
pub fn get_treasury_withdrawal_history(env: Env, offset: u32, limit: u32) -> Vec<TreasuryWithdrawal>
```

Return a page of treasury withdrawal history (newest-first).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_platform_fee_report`

```rust
pub fn get_platform_fee_report(env: Env, from_ts: u64, to_ts: u64) -> PlatformFeeReport
```

Issue #666: Aggregate platform fee collection over `[from_ts, to_ts]` (inclusive), for treasury reporting. Reads from the `FeeCollectionHistory` log that `settle_payment` appends to on every fee-bearing settlement.  NOTE: `FeeCollectionHistory` is capped at `FEE_COLLECTION_HISTORY_CAP` entries; queries for periods older than the retained window will undercount. A follow-up should move this to time-bucketed storage (e.g. per-day accumulator keys) if long-horizon reporting is needed.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `withdraw_treasury`

```rust
pub fn withdraw_treasury(env: Env, admin: Address, amount: i128, destination: Address) -> Result<(), Error>
```

Admin withdrawal of accumulated treasury fees. Emits `TREASURY/WITHDRAWN` with `(amount, destination)` and appends to the paginated history log.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `TREASURY/WITHDRAWN`
- **Error codes:** `InsufficientTreasuryBalance`, `InvalidAmount`, `Unauthorized`

#### `add_fee_waiver_code`

```rust
pub fn add_fee_waiver_code(env: Env, admin: Address, code: String, expires_at: u64, max_uses: u32) -> Result<(), Error>
```

Admin-only: register a reusable fee-waiver code for per-payment zero-fee promotions.  The code can be consumed at settlement via the `fee_waiver_code` field on `PaymentCharge`. Each successful consumption atomically decrements `remaining_uses`; when the counter reaches zero or `expires_at` is in the past, the code is treated as invalid and normal fees apply.  To immediately revoke a live code without waiting for expiry, pass `max_uses = 0` (which will set `remaining_uses = 0` on overwrite).  # Arguments * `admin` – Must hold the admin role. * `code`  – Arbitrary case-sensitive code string (e.g. "LAUNCH2026"). * `expires_at` – Unix ledger timestamp after which the code is invalid. * `max_uses` – Maximum total payments that may use this code. Must be `>= 1` when creating a new code; may be `0` when revoking an existing one.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `FEE_WAIVER/CODE_ADDED`
- **Error codes:** `InvalidAmount`, `InvalidExpiry`, `Unauthorized`

#### `set_global_rate_limit`

```rust
pub fn set_global_rate_limit(env: Env, admin: Address, window_secs: u64, max_per_window: u32) -> Result<(), Error>
```

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `set_merchant_rate_limit`

```rust
pub fn set_merchant_rate_limit(env: Env, admin: Address, merchant_id: Address, window_secs: u64, max_per_window: u32) -> Result<(), Error>
```

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `grant_role`

```rust
pub fn grant_role(env: Env, admin: Address, role: Symbol, account: Address) -> Result<(), Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `grant_merchant_role`

```rust
pub fn grant_merchant_role(env: Env, admin: Address, merchant_id: Address) -> Result<(), Error>
```

- **Auth:** checks role(s) `merchant`
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `revoke_role`

```rust
pub fn revoke_role(env: Env, admin: Address, role: Symbol, account: Address) -> Result<(), Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `revoke_merchant_role`

```rust
pub fn revoke_merchant_role(env: Env, admin: Address, merchant_id: Address) -> Result<(), Error>
```

- **Auth:** checks role(s) `merchant`
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `has_role`

```rust
pub fn has_role(env: Env, role: Symbol, account: Address) -> bool
```

Returns whether `account` holds `role` on this contract (issue #401).

- **Auth:** checks an access-control role
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_role_members`

```rust
pub fn get_role_members(env: Env, role: Symbol) -> Vec<Address>
```

Returns all addresses currently holding `role` on this contract (issue #401).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_global_pause`

```rust
pub fn set_global_pause(env: Env, admin: Address, paused: bool, reason: String) -> Result<(), Error>
```

Set the global paused state (admin only). When paused, create_payment, verify_payment, and cancel_payment are blocked.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `set_creation_pause`

```rust
pub fn set_creation_pause(env: Env, admin: Address, paused: bool, reason: String) -> Result<(), Error>
```

Set the creation-only paused state (admin only, issue #670).  When `paused` is true, only payment-creation entry points (`create_payment`, `create_payments_batch`, `swap_and_pay`, `swap_and_pay_multi_route`, and the creation path of `retry_payment`) are blocked with `Error::ContractPaused`. Settlement, verification, cancellation, and refund operations continue to work normally. This is narrower than `set_global_pause`, which halts all operations. Query the current state with `get_creation_pause_info`.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `pause_scope`

```rust
pub fn pause_scope(env: Env, admin: Address, scope: PauseScope, reason: String) -> Result<(), Error>
```

Pause one operation family (or the entire contract), admin only.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `CONTRACT/SCOPE_PAUSED`
- **Error codes:** `Unauthorized`

#### `unpause_scope`

```rust
pub fn unpause_scope(env: Env, admin: Address, scope: PauseScope) -> Result<(), Error>
```

Resume one operation family (or the entire contract), admin only.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `CONTRACT/SCOPE_UNPAUSED`
- **Error codes:** `Unauthorized`

#### `get_paused_scopes`

```rust
pub fn get_paused_scopes(env: Env) -> Vec<PauseScope>
```

Return all operation scopes currently paused.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_paused`

```rust
pub fn set_paused(env: Env, admin: Address, paused: bool) -> Result<(), Error>
```

Legacy wrapper for set_global_pause

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_creation_pause_info`

```rust
pub fn get_creation_pause_info(env: Env) -> PauseState
```

Get the current creation-only pause state (issue #670).  Distinct from `get_pause_info`, which returns the consolidated global + creation state. `set_creation_pause` blocks only `create_payment`, `create_payments_batch`, `swap_and_pay`, `swap_and_pay_multi_route`, and the creation path of `retry_payment`. It does NOT block `verify_payment`, `settle_payment`, `cancel_payment`, `process_refund`, `claim_refund`, or dispute resolution, so operators can halt new payment intake (e.g. during a maintenance window) while still allowing in-flight payments to be confirmed, settled, and refunded. Use `set_global_pause` instead when all operations need to be halted.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_pause_info`

```rust
pub fn get_pause_info(env: Env) -> PauseInfo
```

Get the current consolidated pause info.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `is_paused`

```rust
pub fn is_paused(env: Env) -> bool
```

Get the current global paused state.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_merchant_amount_limits`

```rust
pub fn set_merchant_amount_limits(env: Env, merchant_id: Address, min: Option<i128>, max: Option<i128>) -> Result<(), Error>
```

Set per-merchant min/max payment amount limits (merchant self-service). Pass None to clear a bound. Requires the caller to hold the MERCHANT role.

- **Auth:** Requires `merchant_id.require_auth()`; checks role(s) `merchant`
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidAmount`, `Unauthorized`

#### `get_merchant_amount_limits`

```rust
pub fn get_merchant_amount_limits(env: Env, merchant_id: Address) -> Option<AmountLimits>
```

Read per-merchant amount limits.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_global_amount_limits`

```rust
pub fn set_global_amount_limits(env: Env, admin: Address, min: Option<i128>, max: Option<i128>) -> Result<(), Error>
```

Set global min/max payment amount limits (admin only). Pass None to clear a bound.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidAmount`, `Unauthorized`

#### `get_global_amount_limits`

```rust
pub fn get_global_amount_limits(env: Env) -> Option<AmountLimits>
```

Read global amount limits.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_usdc_token`

```rust
pub fn set_usdc_token(env: Env, admin: Address, token_address: Address) -> Result<(), Error>
```

Set the USDC token address used as the default settlement token. Also adds it to the supported tokens whitelist.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `allow_token`

```rust
pub fn allow_token(env: Env, admin: Address, token_address: Address) -> Result<(), Error>
```

Allow or disallow a token address for use in payments (admin only).

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `set_token_currency`

```rust
pub fn set_token_currency(env: Env, admin: Address, token_address: Address, currency: Symbol) -> Result<(), Error>
```

Issue #483: Set the currency symbol for an allowed token (e.g., USDC, EURC, BRLT). Must be called after allow_token() to establish token-to-currency mapping.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`, `UnsupportedToken`

#### `remove_supported_token`

```rust
pub fn remove_supported_token(env: Env, admin: Address, token_address: Address) -> Result<(), Error>
```

Issue #301: Remove a token from the supported tokens list (admin only).

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `TOKEN/REMOVED`
- **Error codes:** `Unauthorized`

#### `get_supported_tokens`

```rust
pub fn get_supported_tokens(env: Env) -> Vec<Address>
```

Issue #301: Return the list of supported token addresses.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_kyc_tier_limits`

```rust
pub fn set_kyc_tier_limits(env: Env, admin: Address, tier: KycTier, max_amount: i128) -> Result<String, Error>
```

Issue #303: Set per‑tier KYC payment limits (admin only).

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `set_fx_oracle_address`

```rust
pub fn set_fx_oracle_address(env: Env, admin: Address, oracle_address: Address) -> Result<(), Error>
```

Issue #303: Set the FX oracle contract address (admin only).

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `set_oracle_staleness_threshold`

```rust
pub fn set_oracle_staleness_threshold(env: Env, admin: Address, threshold_secs: u64) -> Result<(), Error>
```

Issue #1033: Admin-configurable staleness threshold (in seconds) that `confirm_payment`/`verify_payment` enforces on the FX rate snapshot before trusting it for settlement, independent of the FX oracle contract's own internal staleness cap. Must be strictly positive.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidAmount`, `Unauthorized`

#### `get_oracle_staleness_threshold`

```rust
pub fn get_oracle_staleness_threshold(env: Env) -> u64
```

Issue #1033: Currently configured oracle staleness threshold, falling back to `MAX_ORACLE_STALENESS_SECS` when the admin has not overridden it.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `add_to_blacklist`

```rust
pub fn add_to_blacklist(env: Env, admin: Address, address: Address) -> Result<(), Error>
```

Add an address to the global blacklist (admin only).

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `remove_from_blacklist`

```rust
pub fn remove_from_blacklist(env: Env, admin: Address, address: Address) -> Result<(), Error>
```

Remove an address from the global blacklist (admin only).

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `is_blacklisted`

```rust
pub fn is_blacklisted(env: Env, address: Address) -> bool
```

Returns true when an address is globally blacklisted.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `is_token_allowed`

```rust
pub fn is_token_allowed(env: Env, token_address: Address) -> bool
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `create_payment`

```rust
pub fn create_payment(env: Env, args: CreatePaymentArgs) -> Result<PaymentCharge, Error>
```

- **Auth:** checks role(s) `merchant`
- **Events:** `PAYMENT/CREATED`, `PAYMENT/RETRY_CREATED`
- **Error codes:** `AmountAboveMax`, `AmountBelowMin`, `DepositAddressInUse`, `DuplicateIdempotencyKey`, `ExceedsTierMonthlyLimit`, `ExceedsTierSingleLimit`, `InvalidAmount`, `InvalidExpiry`, `InvalidPaymentId`, `InvalidRetryTarget`, `KycLimitExceeded`, `PayerNotWhitelisted`, `TierMonthlyLimitExceeded`, `TierSingleLimitExceeded`, `Unauthorized`, `UnsupportedToken`

#### `grant_delegation`

```rust
pub fn grant_delegation(env: Env, merchant: Address, sub_account: Address, scope: account_abstraction::DelegationScope, expires_at: u64) -> Result<(), Error>
```

Grant `sub_account` delegated authority to perform `scope` payment operations on behalf of `merchant` until `expires_at`. Requires the merchant's own authorization.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `revoke_delegation`

```rust
pub fn revoke_delegation(env: Env, merchant: Address, sub_account: Address) -> Result<(), Error>
```

Revoke a previously granted sub-account delegation. Requires the merchant's own authorization.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `list_delegations`

```rust
pub fn list_delegations(env: Env, merchant: Address) -> Vec<account_abstraction::Delegation>
```

List every sub-account delegation a merchant has ever granted (including revoked/expired entries), for audit purposes.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `create_payment_delegated`

```rust
pub fn create_payment_delegated(env: Env, sub_account: Address, args: CreatePaymentArgs) -> Result<PaymentCharge, Error>
```

Create a payment using a merchant-delegated sub-account (e.g. a POS terminal or employee key) instead of the merchant's own signature. `sub_account` signs this invocation directly; its grant must cover `DelegationScope::CreatePayment` (or `All`) for `args.merchant_id` and must not be expired or revoked. Reuses the same temporary merchant-authorization bypass as sponsored meta-transactions so `create_payment`'s existing body does not need to change.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `create_refund_delegated`

```rust
pub fn create_refund_delegated(env: Env, sub_account: Address, payment_id: String, refund_amount: i128, reason: String, receipt_hash: Option<BytesN<32>>) -> Result<String, Error>
```

Create a refund using a merchant-delegated sub-account instead of the merchant's own signature. `sub_account` signs this invocation directly; its grant must cover `DelegationScope::CreateRefund` (or `All`) for the payment's merchant, and must not be expired or revoked.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `create_payments_batch`

```rust
pub fn create_payments_batch(env: Env, args_list: Vec<CreatePaymentArgs>) -> Result<Vec<String>, Error>
```

Issue #165: Batch payment creation for optimized gas usage. Creates multiple payment charges in a single transaction. Reverts all if any element violates validation rules.

- **Auth:** Requires `merchant_id.require_auth()`; checks role(s) `merchant`
- **Events:** `PAYMENT/BATCH_CREATED`, `PAYMENT/CREATED`
- **Error codes:** `AmountAboveMax`, `BatchContainsDuplicates`, `BatchTooLarge`, `DuplicateIdempotencyKey`, `InvalidAmount`, `InvalidExpiry`, `InvalidPaymentId`, `PaymentAlreadyExists`, `Unauthorized`, `UnsupportedToken`

#### `create_payment_batch`

```rust
pub fn create_payment_batch(env: Env, merchant_id: Address, payments: Vec<PaymentRequest>) -> Result<Vec<String>, Error>
```

Issue #771: Atomic batch payment creation — create up to 10 payments in a single transaction. Emits a single PAYMENT/BATCH_CREATED event containing all payment IDs, plus individual PAYMENT/CREATED events for each created payment.

- **Auth:** Requires `merchant_id.require_auth()`; checks role(s) `merchant`
- **Events:** `PAYMENT/BATCH_CREATED`, `PAYMENT/CREATED`
- **Error codes:** `AmountAboveMax`, `BatchContainsDuplicates`, `BatchTooLarge`, `DuplicateIdempotencyKey`, `InvalidAmount`, `InvalidExpiry`, `InvalidPaymentId`, `PaymentAlreadyExists`, `Unauthorized`, `UnsupportedToken`

#### `verify_payment`

```rust
pub fn verify_payment(env: Env, oracle: Address, payment_id: String, transaction_hash: BytesN<32>, payer_address: Address, amount_received: i128, payer_muxed_id: Option<u64>, // Issue #1025: Tip/gratuity portion of `amount_received`, reported separately // so it can be validated against `MAX_TIP_BPS` and broken out for reconciliation. // Ignored (treated as no tip) when `payment.tip_enabled` is `false`. tip_amount: Option<i128>) -> Result<PaymentStatus, Error>
```

- **Auth:** Requires `oracle.require_auth()`; checks role(s) `oracle`
- **Events:** `PAYMENT/FX_RATE_UNAVAILABLE`, `PAYMENT/OVERPAID`, `PAYMENT/PARTIALLY_PAID`, `REFUND/AUTO_CREATED`, `REFUND/AUTO_REQUIRED`
- **Error codes:** `ExceedsTierMonthlyLimit`, `OracleStale`, `PaymentAlreadyProcessed`, `PaymentExpired`, `RateStale`, `TierMonthlyLimitExceeded`, `TipExceedsMax`, `Unauthorized`

#### `confirm_payment`

```rust
pub fn confirm_payment(env: Env, oracle: Address, payment_id: String, transaction_hash: BytesN<32>, payer_address: Address, amount_received: i128, payer_muxed_id: Option<u64>) -> Result<PaymentStatus, Error>
```

Issue #767: Confirm payment, supporting partial payment accumulation when `allow_partial = true`. Issue #763: Alias for verify_payment matching on-chain payment confirmation convention. Rejects confirmation if current ledger timestamp exceeds payment.expires_at, returning PaymentExpired.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `verify_payment_batch`

```rust
pub fn verify_payment_batch(env: Env, operator: Address, verifications: Vec<VerifyPaymentArgs>) -> Result<Vec<Result<PaymentStatus, Error>>, Error>
```

- **Auth:** Requires `operator.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `BatchTooLarge`

#### `accept_partial_payment`

```rust
pub fn accept_partial_payment(env: Env, merchant_id: Address, payment_id: String) -> Result<(), Error>
```

Issue #471: Allow a merchant to accept a PartiallyPaid payment at the received amount. Moves the payment from PartiallyPaid to Confirmed, using the received amount as the effective payment amount. No refund is created for the difference.

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** `PAYMENT/PARTIAL_ACCEPTED`
- **Error codes:** `PaymentAlreadyProcessed`, `Unauthorized`

#### `complete_partial_payment`

```rust
pub fn complete_partial_payment(env: Env, payer: Address, payment_id: String, top_up_amount: i128) -> Result<(), Error>
```

Issue #471: Allow a customer to complete a PartiallyPaid payment by topping up. This is a declaration that the payer will send additional funds off-chain. The payment status is moved to Pending so a new verify_payment call can confirm it with the combined amount.

- **Auth:** Requires `payer.require_auth()`
- **Events:** `PAYMENT/PARTIAL_TOPUP`
- **Error codes:** `InvalidAmount`, `PaymentAlreadyProcessed`

#### `retry_payment`

```rust
pub fn retry_payment(env: Env, merchant_id: Address, original_payment_id: String, new_expires_at: u64) -> Result<String, Error>
```

Issue #482: Create a retry payment for an expired or failed original payment. Returns the payment_id of the newly created payment, linked to the original. Maximum retry chain depth is 3.

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** `PAYMENT/RETRY_CREATED`
- **Error codes:** `InvalidExpiry`, `PaymentAlreadyProcessed`, `RetryChainTooDeep`, `Unauthorized`

#### `get_payment`

```rust
pub fn get_payment(env: Env, payment_id: String) -> Result<PaymentCharge, Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_payment_status_history`

```rust
pub fn get_payment_status_history(env: Env, payment_id: String) -> Result<Vec<PaymentStatusEvent>, Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_payment_by_metadata_hash`

```rust
pub fn get_payment_by_metadata_hash(env: Env, metadata_hash: BytesN<32>) -> Result<PaymentCharge, Error>
```

Issue #489: Reverse lookup payment by metadata_hash for order reconciliation.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `PaymentNotFound`

#### `get_customer`

```rust
pub fn get_customer(env: Env, merchant_id: Address, customer_id: Address) -> Result<CustomerProfile, Error>
```

Issue #492: Get customer profile for merchant and customer pair.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `PaymentNotFound`

#### `get_top_customers`

```rust
pub fn get_top_customers(env: Env, merchant_id: Address, limit: u32) -> Vec<CustomerProfile>
```

Issue #492: Get top customers for a merchant sorted by total_spent (descending).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_top_merchants`

```rust
pub fn get_top_merchants(env: Env, limit: u32) -> Vec<MerchantRanking>
```

Issue #628: Rank merchants by cumulative gross payment volume (descending).  Reads only the per-merchant `MerchantGrossVolume` / `MerchantPaymentCount` indexes plus the `TrackedMerchants` list — it never iterates payment records. `limit` is capped at [`TOP_MERCHANTS_MAX_LIMIT`] (100); a `limit` of 0 is treated as the cap.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `bump_payment_ttl`

```rust
pub fn bump_payment_ttl(env: Env, payment_id: String) -> Result<(), Error>
```

Issue #768: Permissionless function to allow anyone to extend a payment's TTL before it lapses.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `bump_payment_ttl_public`

```rust
pub fn bump_payment_ttl_public(env: Env, payment_id: String) -> Result<(), Error>
```

Issue #488: Permissionless public entry point for TTL maintenance.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `bulk_bump_payment_ttls`

```rust
pub fn bulk_bump_payment_ttls(env: Env, payment_ids: Vec<String>) -> Result<u32, Error>
```

Issue #488: Bulk bump payment TTLs for efficient maintenance sweeps (max 50).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `BatchTooLarge`

#### `get_merchant_payments`

```rust
pub fn get_merchant_payments(env: Env, merchant_id: Address) -> Vec<String>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_merchant_payments_paginated`

```rust
pub fn get_merchant_payments_paginated(env: Env, merchant_id: Address, offset: u32, limit: u32, status_filter: Option<PaymentStatus>) -> Vec<String>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `generate_reconciliation_report`

```rust
pub fn generate_reconciliation_report(env: Env, merchant_id: Address, from_ts: u64, to_ts: u64, offset: u32, limit: u32) -> Result<ReconciliationReport, Error>
```

Generate a reconciliation report for a merchant over a time period.  This is a read-only query that returns a structured summary of all settlements, fees, refunds, and disputes in the specified period.  # Parameters * `merchant_id` - The merchant to generate the report for * `from_ts` - Start timestamp (inclusive) * `to_ts` - End timestamp (inclusive) * `offset` - Pagination offset (number of payments to skip) * `limit` - Maximum number of payments to include (max 100)

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidExpiry`

#### `reconciliation_report_page`

```rust
pub fn reconciliation_report_page(env: Env, merchant_id: Address, from_ts: u64, to_ts: u64, offset: u32, limit: u32) -> Result<ReconciliationPage, Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `cancel_payment`

```rust
pub fn cancel_payment(env: Env, authority: Address, payment_id: String) -> Result<(), Error>
```

- **Auth:** Requires `authority.require_auth()`; checks role(s) `oracle`
- **Events:** `PAYMENT/EXPIRED`
- **Error codes:** `PaymentAlreadyProcessed`, `Unauthorized`

#### `expire_payment`

```rust
pub fn expire_payment(env: Env, payment_id: String) -> Result<(), Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** `PAYMENT/EXPIRED`
- **Error codes:** `PaymentAlreadyProcessed`, `PaymentExpired`

#### `batch_expire_payments`

```rust
pub fn batch_expire_payments(env: Env, payment_ids: Vec<String>) -> Result<u32, Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `settle_payment`

```rust
pub fn settle_payment(env: Env, operator: Address, payment_id: String, splits: Vec<SettlementSplit>) -> Result<(), Error>
```

- **Auth:** Requires `operator.require_auth()`; checks role(s) `settlement_operator`
- **Events:** `PAYMENT/ANCHOR_WITHDRAW`, `PAYMENT/FEE_COLLECTED`, `PAYMENT/FEE_SPLIT`, `PAYMENT/FEE_TOTAL`, `PAYMENT/FEE_WAIVED`, `PAYMENT/RESERVE_HELD`, `PAYMENT/SETTLED`
- **Error codes:** `InvalidSettlement`, `PaymentAlreadyProcessed`, `Reentrancy`, `SettlementBelowMinimum`, `SettlementIntervalNotElapsed`, `Unauthorized`

#### `trigger_settlement`

```rust
pub fn trigger_settlement(env: Env, operator: Address, merchant_id: Address) -> Result<i128, Error>
```

Issue #480: Trigger a settlement for a merchant's accumulated pending balance.  Sweeps the merchant's pending settlement balance to their payout address. For `Daily` and `Weekly` schedules, enforces a minimum time since the last settlement. For `Manual`, settles immediately as long as the pending balance exceeds `SETTLEMENT_MIN_AMOUNT`.

- **Auth:** Requires `operator.require_auth()`; checks role(s) `settlement_operator`
- **Events:** `MERCHANT/SETTLEMENT_TRIGGERED`
- **Error codes:** `InvalidAddress`, `InvalidAmount`, `Unauthorized`

#### `release_merchant_reserve`

```rust
pub fn release_merchant_reserve(env: Env, operator: Address, merchant_id: Address) -> Result<i128, Error>
```

Issue #1017: Release any matured rolling-reserve buckets for a merchant back into their pending settlement balance, where the next `trigger_settlement` sweep pays them out. Intended to be invoked by an off-chain release daemon on a schedule; requires the `SETTLEMENT_OPERATOR` role, matching `trigger_settlement`.

- **Auth:** Requires `operator.require_auth()`; checks role(s) `settlement_operator`
- **Events:** `MERCHANT/RESERVE_RELEASED`
- **Error codes:** `Unauthorized`

#### `prune_expired_payments`

```rust
pub fn prune_expired_payments(env: Env, operator: Address, payment_ids: Vec<String>) -> Result<u32, Error>
```

- **Auth:** Requires `operator.require_auth()`; checks role(s) `settlement_operator`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `prune_expired_invoices`

```rust
pub fn prune_expired_invoices(env: Env, operator: Address, invoice_ids: Vec<String>) -> Result<u32, Error>
```

- **Auth:** Requires `operator.require_auth()`; checks role(s) `settlement_operator`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `add_router`

```rust
pub fn add_router(env: Env, admin: Address, router: Address) -> Result<(), Error>
```

Atomic swap and pay: swap sender's token to merchant's required token and create payment. Integrates with DEX (e.g., Soroswap) for atomic asset conversion.  # Arguments * `payer` - The address making the payment * `payment_id` - Unique payment identifier * `merchant_id` - Merchant's address * `amount` - Amount in the merchant's settlement currency (after swap) * `currency` - Settlement currency symbol * `deposit_address` - Where the payment should be deposited * `token_in` - Address of the token the payer is sending * `amount_in` - Amount of token_in to swap * `amount_out_min` - Minimum amount of settlement token required * `path` - DEX swap path [token_in, ..., settlement_token] * `expires_at` - Payment expiry timestamp * `dex_router` - Address of the DEX router contract  # Returns The created PaymentCharge on success Add a router address to the allowlist (issue #437).

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `ROUTER/ADDED`
- **Error codes:** `Unauthorized`

#### `remove_router`

```rust
pub fn remove_router(env: Env, admin: Address, router: Address) -> Result<(), Error>
```

Remove a router address from the allowlist (issue #437).

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `ROUTER/REMOVED`
- **Error codes:** `Unauthorized`

#### `is_router_allowed`

```rust
pub fn is_router_allowed(env: Env, router: Address) -> bool
```

Check if a DEX router is allowed (issue #437).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_allowed_routers`

```rust
pub fn get_allowed_routers(env: Env) -> Vec<Address>
```

Get all allowlisted DEX router addresses (issue #437).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_wrapped_xlm_contract`

```rust
pub fn set_wrapped_xlm_contract(env: Env, admin: Address, wxlm: Address) -> Result<(), Error>
```

Set the Wrapped XLM (WXLM) token contract address (issue #434).

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_wrapped_xlm_contract`

```rust
pub fn get_wrapped_xlm_contract(env: Env) -> Option<Address>
```

Get the Wrapped XLM (WXLM) token contract address (issue #434).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `swap_and_pay`

```rust
pub fn swap_and_pay(env: Env, args: SwapAndPayArgs) -> Result<PaymentCharge, Error>
```

Swaps tokens via a DEX router and executes a payment (issue #434, #437).

- **Auth:** Requires `payer.require_auth()`
- **Events:** `SWAP/EXECUTED`
- **Error codes:** `ArbitrageDetected`, `InvalidAmount`, `OraclePriceDeviation`, `RouterNotAllowed`, `SwapPathInvalid`, `SwapVolumeLimitExceeded`

#### `swap_and_pay_multi_route`

```rust
pub fn swap_and_pay_multi_route(env: Env, args: SwapAndPayArgs, routes: Vec<SwapRoute>, min_output_amount: i128) -> Result<PaymentCharge, Error>
```

Issue #436: Multi-DEX route splitting / aggregation.

- **Auth:** Requires `payer.require_auth()`
- **Events:** `SWAP/AND`
- **Error codes:** `InvalidAmount`, `RouteOutputInsufficient`, `RouterNotAllowed`, `SwapPathInvalid`

#### `get_merchant_dispute_count`

```rust
pub fn get_merchant_dispute_count(env: Env, merchant_id: Address) -> u64
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_merchant_payment_count`

```rust
pub fn get_merchant_payment_count(env: Env, merchant_id: Address) -> u32
```

Issue #396: Returns the total number of payment IDs stored for a merchant. Used by pagination UIs alongside `get_merchant_payments_full`. Issue #396: Get merchant payment count for dashboard pagination (O(1) via counter).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_merchant_payment_count_dash`

```rust
pub fn get_merchant_payment_count_dash(env: Env, merchant_id: Address) -> u32
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_merchant_payments_full`

```rust
pub fn get_merchant_payments_full(env: Env, merchant_id: Address, offset: u32, limit: u32, token_address: Option<Address>) -> Vec<PaymentCharge>
```

Issue #396: Returns paginated full `PaymentCharge` structs for merchant dashboards. `limit` is capped at 50 to avoid ledger compute limits. Returns an empty vec (not an error) when `offset` exceeds the total count. When `token_address` is provided, returns only payments matching that token.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_merchant_analytics`

```rust
pub fn get_merchant_analytics(env: Env, merchant_id: Address, from_ts: u64, to_ts: u64) -> MerchantAnalytics
```

Issue #487: Query aggregate analytics for a merchant over a time range. Returns total_payments, confirmed_payments, failed_payments, total_volume, avg_payment_amount, dispute_count, refund_count, net_settled_volume. Issue #678: When from_ts/to_ts are provided, uses the DailyPaymentIndex to scan only the relevant day buckets (O(days) ledger reads).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `pre_authorize_merchant`

```rust
pub fn pre_authorize_merchant(env: Env, customer: Address, merchant: Address, token: Address, limit_per_period: i128, period_secs: u64) -> Result<MerchantAuthorization, MerchantAuthError>
```

Customer grants a merchant permission to pull up to `limit_per_period` tokens per `period_secs`-second window.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `revoke_merchant_authorization`

```rust
pub fn revoke_merchant_authorization(env: Env, customer: Address, merchant: Address) -> Result<(), MerchantAuthError>
```

Customer revokes a previously granted merchant authorization.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `pull_payment`

```rust
pub fn pull_payment(env: Env, merchant: Address, customer: Address, amount: i128) -> Result<i128, MerchantAuthError>
```

Merchant pulls `amount` tokens from the customer's account against an existing pre-authorization.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_merchant_authorization`

```rust
pub fn get_merchant_authorization(env: Env, customer: Address, merchant: Address) -> Result<MerchantAuthorization, MerchantAuthError>
```

Return the stored authorization for a (customer, merchant) pair.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `merchant_authorization_remaining`

```rust
pub fn merchant_authorization_remaining(env: Env, customer: Address, merchant: Address) -> Result<i128, MerchantAuthError>
```

Return the remaining pull budget for the current period.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `create_api_key`

```rust
pub fn create_api_key(env: Env, merchant: Address, key_hash: BytesN<32>, scopes: Vec<String>) -> Result<ApiKeyRecord, MerchantAuthError>
```

Issue #854: Merchant creates a scoped API key.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_api_key`

```rust
pub fn get_api_key(env: Env, key_hash: BytesN<32>) -> Result<ApiKeyRecord, MerchantAuthError>
```

Issue #854: Retrieve an API key record by its key hash.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `revoke_api_key`

```rust
pub fn revoke_api_key(env: Env, merchant: Address, key_hash: BytesN<32>) -> Result<(), MerchantAuthError>
```

Issue #854: Merchant revokes an active API key.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `cancel_stream`

```rust
pub fn cancel_stream(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `pause_stream`

```rust
pub fn pause_stream(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `resume_stream`

```rust
pub fn resume_stream(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `cancel_multiple_streams`

```rust
pub fn cancel_multiple_streams(env: Env, sender: Address, stream_ids: Vec<String>) -> Result<Vec<String>, StreamError>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `batch_cancel_streams`

```rust
pub fn batch_cancel_streams(env: Env, sender: Address, stream_ids: Vec<String>) -> Result<Vec<String>, StreamError>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `batch_withdraw_to`

```rust
pub fn batch_withdraw_to(env: Env, recipient: Address, withdrawals: Vec<WithdrawalRecipient>) -> Result<Vec<String>, StreamError>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `withdraw_all_for_recipient`

```rust
pub fn withdraw_all_for_recipient(env: Env, recipient: Address, max_streams: u32) -> Result<Vec<String>, StreamError>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `trigger_withdrawal`

```rust
pub fn trigger_withdrawal(env: Env, stream_id: String) -> Result<String, StreamError>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `bulk_bump_stream_ttls`

```rust
pub fn bulk_bump_stream_ttls(env: Env, stream_ids: Vec<String>) -> Result<u32, StreamError>
```

Issue #627: Bulk-extend the TTL of many stream entries in one call (permissionless). Delegates to `PaymentStreaming::bulk_bump_stream_ttls`; non-existent stream IDs are silently skipped and the count of bumped streams is returned.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `set_stream_destination`

```rust
pub fn set_stream_destination(env: Env, recipient: Address, stream_id: String, destination: Address) -> Result<(), StreamError>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `approve_stream_milestone`

```rust
pub fn approve_stream_milestone(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError>
```

Sender approves milestones for a stream, unlocking withdrawals.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `revoke_stream_milestone`

```rust
pub fn revoke_stream_milestone(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError>
```

Sender revokes a previous milestone approval, re-locking withdrawals.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `set_stream_min_rate`

```rust
pub fn set_stream_min_rate(env: Env, sender: Address, stream_id: String, min_rate_per_second: i128) -> Result<(), StreamError>
```

Sender sets a floor for `decrease_rate_per_second` on this stream.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `decrease_rate_per_second`

```rust
pub fn decrease_rate_per_second(env: Env, sender: Address, stream_id: String, new_rate: i128) -> Result<(), StreamError>
```

Reduce the flow rate of an active stream, refunding surplus deposit.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_sender_streams`

```rust
pub fn get_sender_streams(env: Env, sender: Address, page: u32, page_size: u32) -> Vec<PaymentStream>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_stream`

```rust
pub fn get_stream(env: Env, stream_id: String) -> Result<PaymentStream, StreamError>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `create_stream`

```rust
pub fn create_stream(env: Env, sender: Address, receiver: Address, token: Address, rate_per_second: i128, deposit: i128, stream_id: String) -> Result<PaymentStream, StreamError>
```

Create a new payment stream. Tokens are pulled from `sender` into the contract.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `create_vesting_stream`

```rust
pub fn create_vesting_stream(env: Env, sender: Address, receiver: Address, token: Address, rate_per_second: i128, deposit: i128, stream_id: String, cliff_at: u64) -> Result<PaymentStream, StreamError>
```

Create a vesting stream whose accrued tokens are locked until `cliff_at`.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `top_up_stream`

```rust
pub fn top_up_stream(env: Env, caller: Address, stream_id: String, amount: i128) -> Result<(), StreamError>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `top_up_multiple_streams`

```rust
pub fn top_up_multiple_streams(env: Env, sender: Address, top_ups: Vec<(String, i128)>) -> Result<(), StreamError>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `update_stream_rate`

```rust
pub fn update_stream_rate(env: Env, sender: Address, stream_id: String, new_rate: i128) -> Result<(), StreamError>
```

Update the flow rate of an active stream (increase or decrease).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `close_expired_stream`

```rust
pub fn close_expired_stream(env: Env, stream_id: String) -> Result<(), StreamError>
```

Close a terminal (Exhausted/Cancelled) stream and remove its storage entry.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `set_stream_fee_bps`

```rust
pub fn set_stream_fee_bps(env: Env, admin: Address, fee_bps: i128) -> Result<(), Error>
```

Set the platform fee in basis points applied to stream withdrawals. Only the admin may call this.

- **Auth:** Requires `admin.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `get_stream_fee_bps`

```rust
pub fn get_stream_fee_bps(env: Env) -> i128
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_stream_fee_recipient`

```rust
pub fn set_stream_fee_recipient(env: Env, admin: Address, recipient: Address) -> Result<(), Error>
```

- **Auth:** Requires `admin.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `get_stream_fee_recipient`

```rust
pub fn get_stream_fee_recipient(env: Env) -> Option<Address>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `upgrade_contract`

```rust
pub fn upgrade_contract(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<String, Error>
```

Upgrade the contract WASM and increment the contract version.  Issue #624: The upgrade is now queued via the timelock instead of executing immediately.  Returns the action ID of the pending action.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `set_timelock_delay`

```rust
pub fn set_timelock_delay(env: Env, admin: Address, secs: u64) -> Result<(), Error>
```

Set the timelock delay applied to critical admin operations.  Default is 48 hours.  Only the admin may call this.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_timelock_delay`

```rust
pub fn get_timelock_delay(env: Env) -> u64
```

Return the current timelock delay in seconds.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_pending_admin_actions`

```rust
pub fn get_pending_admin_actions(env: Env) -> Vec<PendingTimelockAction>
```

Return all pending timelocked admin actions.  Any actor may call this to inspect queued operations and their `execute_after` timestamps.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `execute_timelocked_action`

```rust
pub fn execute_timelocked_action(env: Env, admin: Address, action_id: String) -> Result<(), Error>
```

Execute a previously queued timelocked admin action.  Reverts with `TimelockNotExpired` if called before `execute_after`. Only the admin may execute.  Removes the action from the queue on success.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `CONTRACT/UPGRADED`, `TIMELOCK/ACTION_EXECUTED`
- **Error codes:** `PaymentNotFound`, `TimelockNotExpired`, `Unauthorized`

#### `set_multisig_config`

```rust
pub fn set_multisig_config(env: Env, admin: Address, threshold: u32, signers: Vec<Address>) -> Result<(), Error>
```

Configure the multi-sig threshold and signer set. Only the current admin may call this.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `get_multisig_config`

```rust
pub fn get_multisig_config(env: Env) -> (u32, Vec<Address>)
```

Returns the current multi-sig (threshold, signers) configuration.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `create_proposal`

```rust
pub fn create_proposal(env: Env, signer: Address, action: AdminAction) -> Result<u64, Error>
```

Create a new admin proposal for a contract parameter change. The calling signer must be in the multisig signer set. Returns the proposal nonce.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `vote_proposal`

```rust
pub fn vote_proposal(env: Env, signer: Address, nonce: u64) -> Result<(), Error>
```

Vote to approve an existing proposal. The calling signer must be in the multisig signer set and must not have already voted on this proposal.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`

#### `execute_proposal`

```rust
pub fn execute_proposal(env: Env, executor: Address, nonce: u64) -> Result<(), Error>
```

Execute a proposal once the multisig threshold is met.  * Proposals expire after 48 hours if the threshold is not reached. * For parameter-change actions (`SetDisputeBond`, `SetVolumeCap`, `SetRefundFeeBps`, `SetRateLimit`) the new value is written to persistent storage and a `ADMIN/PROPOSAL_EXECUTED` event is emitted.

- **Auth:** Requires `executor.require_auth()`; checks role(s) `admin`
- **Events:** `ADMIN/PROPOSAL_EXECUTED`
- **Error codes:** `AccessControlError`, `InvalidAmount`, `Unauthorized`

#### `get_proposal`

```rust
pub fn get_proposal(env: Env, nonce: u64) -> Option<AdminProposal>
```

Retrieve a pending proposal by nonce.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_dispute_bond_amount`

```rust
pub fn get_dispute_bond_amount(env: Env) -> i128
```

Read the effective dispute bond amount (configurable via multi-sig proposal). Falls back to the compile-time constant if not set via proposal.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_tier_volume_cap`

```rust
pub fn get_tier_volume_cap(env: Env, tier: KycTier) -> i128
```

Read the effective monthly volume cap for a KYC tier. Falls back to compile-time constants if not overridden by a proposal.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_refund_fee_bps`

```rust
pub fn get_refund_fee_bps(env: Env) -> i128
```

Read the effective refund fee in basis points from instance storage. Falls back to the compile-time default (100 bps) if not yet configured.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_refund_cooldown`

```rust
pub fn set_refund_cooldown(env: Env, admin: Address, secs: u64) -> Result<(), Error>
```

Set the refund cooldown period in seconds (overrides REFUND_COOLDOWN_SECS constant). Admin-only operation.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `recompute_merchant_payment_count`

```rust
pub fn recompute_merchant_payment_count(env: Env, admin: Address) -> Result<u64, Error>
```

Migration function: recompute all merchant payment counts from payment vector. Scans all merchants and rebuilds the persistent payment count index. Admin-only operation. Use this after upgrading from older contract versions.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `create_invoice`

```rust
pub fn create_invoice(env: Env, merchant_id: Address, customer_email: String, line_items: Vec<LineItem>, total_amount: i128, currency: Symbol, due_date: u64) -> Result<String, Error>
```

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidAmount`

#### `create_payment_link_invoice`

```rust
pub fn create_payment_link_invoice(env: Env, merchant_id: Address, link_manager: Address, customer_email: String, line_items: Vec<LineItem>, total_amount: i128, currency: Symbol, due_date: u64, link_args: CreateLinkArgs) -> Result<(Invoice, PaymentLink), Error>
```

Issue #632: Atomically create an invoice together with a payment link and wire them up (`invoice.payment_link_id` is set to the new link's ID).  The payment link is created first via a cross-contract call to the `link_manager` (`PaymentLinkManager`) contract. If link creation fails the call returns an error and the invoice is never persisted; if any later step fails the whole transaction reverts, so the two records are always created together or not at all.

- **Auth:** Requires `merchant_id.require_auth()`; checks role(s) `merchant`
- **Events:** `INVOICE/LINK_ATTACHED`
- **Error codes:** `InvalidAmount`, `InvalidPaymentId`, `PaymentNotFound`, `Unauthorized`

#### `mark_invoice_paid`

```rust
pub fn mark_invoice_paid(env: Env, invoice_id: String) -> Result<(), Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `PaymentAlreadyProcessed`, `PaymentNotFound`

#### `set_invoice_grace_period`

```rust
pub fn set_invoice_grace_period(env: Env, admin: Address, secs: u64) -> Result<(), Error>
```

Admin-configurable invoice overdue grace period in seconds (Issue #607).

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_invoice_grace_period`

```rust
pub fn get_invoice_grace_period(env: Env) -> u64
```

Returns the configured invoice overdue grace period in seconds (default: 0).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_invoice`

```rust
pub fn get_invoice(env: Env, invoice_id: String) -> Result<Invoice, Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `PaymentNotFound`

#### `get_merchant_invoices`

```rust
pub fn get_merchant_invoices(env: Env, merchant_id: Address) -> Vec<String>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `void_invoice`

```rust
pub fn void_invoice(env: Env, merchant_id: Address, invoice_id: String) -> Result<(), Error>
```

Issue #1067: Void an invoice (merchant must sign). Transitions Created (Pending) or Overdue → Voided (soft-delete). Paid invoices cannot be voided.

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** `INVOICE/VOIDED`
- **Error codes:** `PaymentAlreadyProcessed`, `PaymentNotFound`, `Unauthorized`

#### `mark_invoice_overdue`

```rust
pub fn mark_invoice_overdue(env: Env, invoice_id: String) -> Result<(), Error>
```

Issue #1067: Mark a past-due invoice as Overdue. Permissionless — anyone may call once `now > invoice.due_date`.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** `INVOICE/OVERDUE`
- **Error codes:** `InvalidExpiry`, `PaymentNotFound`

#### `verify_metadata_hash`

```rust
pub fn verify_metadata_hash(env: Env, payment_id: String, preimage: Bytes) -> Result<bool, Error>
```

Issue #1065: Verify that `preimage` SHA-256 hashes to the `metadata_hash` stored on a payment. Returns `true` on match, `false` when no hash was stored or hash differs. Returns `Error::PaymentNotFound` when the payment does not exist.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_fee_collection_history`

```rust
pub fn get_fee_collection_history(env: Env, offset: u32, limit: u32) -> Vec<FeeRecord>
```

Issue #1066: Return a paginated slice of fee collection records (newest first). `limit` is capped at 200 per call.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_total_fees_collected`

```rust
pub fn get_total_fees_collected(env: Env, admin: Address, token: Address) -> Result<i128, Error>
```

Issue #1066: Admin-only — lifetime total fees collected for a token.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_top_merchants_v2`

```rust
pub fn get_top_merchants_v2(env: Env, limit: u32) -> Vec<MerchantRankEntry>
```

Issue #1064: Return top merchants sorted by cumulative score descending, enriched with KYC tier. Suspended merchants are excluded. `limit` is capped at TOP_MERCHANTS_MAX_LIMIT (100).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `execute_meta_transaction`

```rust
pub fn execute_meta_transaction(env: Env, relayer: Address, payload: meta_transaction::MetaTransactionPayload, signature: BytesN<64>) -> Result<meta_transaction::MetaTxReceipt, meta_transaction::MetaTxError>
```

Submit a merchant-signed meta-transaction. The relayer pays the fee.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `rotate_webhook_key`

```rust
pub fn rotate_webhook_key(env: Env, merchant: Address, key_hash: BytesN<32>)
```

Rotate the merchant's webhook signing key. The merchant must authorize the call directly, or via `execute_meta_transaction`.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_fee_recipient`

```rust
pub fn set_fee_recipient(env: Env, merchant: Address, recipient: Address)
```

Set the address that receives this merchant's fees.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `authorize_refund`

```rust
pub fn authorize_refund(env: Env, merchant: Address, request: meta_transaction::RefundAuthRequest) -> Result<meta_transaction::RefundAuthorization, meta_transaction::MetaTxError>
```

Record the merchant's authorization of a refund.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_domain_separator`

```rust
pub fn get_domain_separator(env: Env) -> BytesN<32>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_merchant_nonce`

```rust
pub fn get_merchant_nonce(env: Env, merchant: Address) -> u64
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_webhook_key`

```rust
pub fn get_webhook_key(env: Env, merchant: Address) -> Option<BytesN<32>>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_fee_recipient`

```rust
pub fn get_fee_recipient(env: Env, merchant: Address) -> Option<Address>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_refund_authorization`

```rust
pub fn get_refund_authorization(env: Env, merchant: Address, payment_id: String) -> Option<meta_transaction::RefundAuthorization>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `create_batch`

```rust
pub fn create_batch(env: Env, creator: Address, token: Address, merkle_root: BytesN<32>, total_amount: i128, leaf_count: u32, expires_at: u64) -> Result<u64, Error>
```

Create a new Merkle-root batch distribution. Pulls `total_amount` of `token` from `creator` into contract custody. Emits `MERKLE/DISTRIBUTION_CREATED` and returns the new `distribution_id`.

- **Auth:** Requires `creator.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidAmount`, `InvalidExpiry`, `InvalidMerkleLeafCount`

#### `claim_batch_payment`

```rust
pub fn claim_batch_payment(env: Env, distribution_id: u64, index: u32, recipient: Address, amount: i128, proof: Vec<BytesN<32>>) -> Result<(), Error>
```

Claim a recipient's allocation from a batch distribution.  Verifies that `(index, recipient, amount)` is included under the distribution's stored Merkle root via `proof`, enforces one claim per `(distribution_id, index)`, and transfers `amount` to `recipient` on success. Emits `MERKLE/CLAIMED`.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidAmount`, `InvalidMerkleProof`, `MerkleAlreadyClaimed`, `MerkleDistributionAlreadyDefunded`, `MerkleDistributionExpired`

#### `defund_batch`

```rust
pub fn defund_batch(env: Env, creator: Address, distribution_id: u64) -> Result<i128, Error>
```

After `expires_at`, the distribution's creator reclaims any unclaimed balance. Emits `MERKLE/DEFUNDED` and returns the refunded amount.

- **Auth:** Requires `creator.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `MerkleDistributionAlreadyDefunded`, `MerkleDistributionNotExpired`, `Unauthorized`

#### `get_batch`

```rust
pub fn get_batch(env: Env, distribution_id: u64) -> Result<MerkleDistribution, Error>
```

View: fetch a batch distribution's metadata.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `MerkleDistributionNotFound`

#### `is_batch_claimed`

```rust
pub fn is_batch_claimed(env: Env, distribution_id: u64, index: u32) -> Result<bool, Error>
```

View: has the allocation at `index` in `distribution_id` been claimed?

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

---

## MerchantRegistry

Source: `fluxapay/src/merchant_registry.rs` (82 public entrypoints)

Merchant onboarding, verification, fee/anchor config, rolling reserves, suspension, analytics/ranking.

#### `version`

```rust
pub fn version(env: Env) -> String
```

Returns the current contract version string from persistent storage.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_version`

```rust
pub fn set_version(env: Env, admin: Address, version: String) -> Result<(), MerchantError>
```

Admin-only: update the stored contract version after a WASM upgrade.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `CONTRACT/VERSION_UPDATED`
- **Error codes:** `InvalidVersion`, `Unauthorized`

#### `initialize`

```rust
pub fn initialize(env: Env, admin: Address) -> Result<(), MerchantError>
```

Initialize the contract with an admin address

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `AdminAlreadySet`

#### `set_contract_metadata`

```rust
pub fn set_contract_metadata(env: Env, admin: Address, key: Symbol, value: String) -> Result<(), MerchantError>
```

Admin-only: set an arbitrary on-chain metadata entry (issue #667), e.g. a description, deployment notes, or audit commit hash. Stored in instance storage under a caller-chosen key, with the instance TTL bumped to `LONG_LIVE_TTL` so metadata survives archival.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_contract_metadata`

```rust
pub fn get_contract_metadata(env: Env, key: Symbol) -> Option<String>
```

Public read of an on-chain metadata entry set via `set_contract_metadata` (issue #667). Returns `None` if the key was never set.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `grant_role`

```rust
pub fn grant_role(env: Env, admin: Address, role: Symbol, account: Address) -> Result<(), MerchantError>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `set_suspension_threshold`

```rust
pub fn set_suspension_threshold(env: Env, admin: Address, threshold: u32) -> Result<(), MerchantError>
```

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `set_tier_limits`

```rust
pub fn set_tier_limits(env: Env, admin: Address, tier: KycTier, limits: crate::AmountLimits) -> Result<(), MerchantError>
```

- **Auth:** Requires `admin.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_tier_limits`

```rust
pub fn get_tier_limits(env: Env, tier: KycTier) -> crate::AmountLimits
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `register_merchant`

```rust
pub fn register_merchant(env: Env, merchant_id: Address, business_name: String, settlement_currency: String, payout_address: Option<Address>, bank_account: Option<String>, fee_config: MaybeFeeConfig) -> Result<(), MerchantError>
```

Register a new merchant

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `MerchantAlreadyExists`

#### `update_merchant`

```rust
pub fn update_merchant(env: Env, merchant_id: Address, business_name: Option<String>, settlement_currency: Option<String>, active: Option<bool>, payout_address: Option<Address>, bank_account: Option<String>, fee_config: Option<MaybeFeeConfig>) -> Result<(), MerchantError>
```

Update merchant settings

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** `MERCHANT/PAYOUT_UPDATED`, `MERCHANT/UPDATED`
- **Error codes:** `PayoutAddressNotWhitelisted`, `Unauthorized`

#### `get_payout_history`

```rust
pub fn get_payout_history(env: Env, merchant_id: Address) -> Result<Vec<Address>, MerchantError>
```

Return the ordered list of previous payout addresses for a merchant.  Each entry was the active `payout_address` immediately before it was changed; the list is in chronological order (oldest first). Returns an empty list if the address has never been changed.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `set_partial_payment_allowed`

```rust
pub fn set_partial_payment_allowed(env: Env, merchant_id: Address, partial_payment_allowed: bool) -> Result<(), MerchantError>
```

Merchant can toggle whether partial payments are accepted.

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** `MERCHANT/PARTIAL_PAYMENT_UPDATED`
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `set_merchant_payment_tolerance`

```rust
pub fn set_merchant_payment_tolerance(env: Env, merchant_id: Address, tolerance: Option<i128>) -> Result<(), MerchantError>
```

Set merchant-specific payment tolerance (merchant auth required). Tolerance is in smallest currency units. None means use global default. Max tolerance is capped at 1% of payment amount to prevent abuse.

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** `MERCHANT/TOLERANCE_UPDATED`
- **Error codes:** `Unauthorized`

#### `set_global_payment_tolerance`

```rust
pub fn set_global_payment_tolerance(env: Env, admin: Address, tolerance: i128) -> Result<(), MerchantError>
```

Set the global default payment tolerance (admin only).

- **Auth:** Requires `admin.require_auth()`
- **Events:** `MERCHANT/GLOBAL_TOLERANCE_UPDATED`
- **Error codes:** `Unauthorized`

#### `get_global_payment_tolerance`

```rust
pub fn get_global_payment_tolerance(env: Env) -> i128
```

Get the global default payment tolerance.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_merchant`

```rust
pub fn get_merchant(env: Env, merchant_id: Address) -> Result<Merchant, MerchantError>
```

Get merchant info  This function automatically reinstates merchants whose suspension has expired.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_bank_account`

```rust
pub fn get_bank_account(env: Env, caller: Address, merchant_id: Address) -> Result<Option<String>, MerchantError>
```

Get merchant bank account (restricted to admin or merchant)

- **Auth:** Requires `caller.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `verify_merchant`

```rust
pub fn verify_merchant(env: Env, admin: Address, merchant_id: Address) -> Result<(), MerchantError>
```

Verify merchant (admin only) — sets KycTier::Basic for backward compatibility. If a RefundManager address is configured, also grants the MERCHANT role there.  # Arguments * `env` - The Soroban environment * `admin` - The admin address * `merchant_id` - The merchant address to verify * `oracle_signature` - Optional oracle signature for KYC verification

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `verify_merchant_with_signature`

```rust
pub fn verify_merchant_with_signature(env: Env, admin: Address, merchant_id: Address, oracle_signature: Option<String>) -> Result<(), MerchantError>
```

Verify merchant with optional oracle signature metadata.

- **Auth:** Requires `admin.require_auth()`
- **Events:** `MERCHANT/VERIFIED`
- **Error codes:** `Unauthorized`

#### `set_kyc_tier`

```rust
pub fn set_kyc_tier(env: Env, admin: Address, merchant_id: Address, tier: KycTier) -> Result<(), MerchantError>
```

Set a specific KYC tier for a merchant (admin only).  # Arguments * `env` - The Soroban environment * `admin` - The admin address * `merchant_id` - The merchant address to update * `tier` - The KYC tier to set * `oracle_signature` - Optional oracle signature for KYC verification

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `set_kyc_tier_with_signature`

```rust
pub fn set_kyc_tier_with_signature(env: Env, admin: Address, merchant_id: Address, tier: KycTier, oracle_signature: Option<String>) -> Result<(), MerchantError>
```

Set a specific KYC tier with optional oracle signature metadata.

- **Auth:** Requires `admin.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `set_refund_manager_address`

```rust
pub fn set_refund_manager_address(env: Env, admin: Address, refund_manager: Address) -> Result<(), MerchantError>
```

Set the RefundManager contract address for automatic MERCHANT role granting

- **Auth:** Requires `admin.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_refund_manager_address`

```rust
pub fn get_refund_manager_address(env: Env) -> Option<Address>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_payment_processor_address`

```rust
pub fn set_payment_processor_address(env: Env, admin: Address, payment_processor: Address) -> Result<(), MerchantError>
```

Set the PaymentProcessor contract address for automatic KYC tier upgrades (issue #207).

- **Auth:** Requires `admin.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_payment_processor_address`

```rust
pub fn get_payment_processor_address(env: Env) -> Option<Address>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `auto_upgrade_kyc_tier`

```rust
pub fn auto_upgrade_kyc_tier(env: Env, caller: Address, merchant_id: Address, new_tier: KycTier) -> Result<(), MerchantError>
```

Automatically upgrade a merchant's KYC tier based on cumulative payment volume (issue #207). Only the registered PaymentProcessor contract may call this function. Only tier promotions are allowed (Unverified→Basic→Full→Business); demotions are rejected.

- **Auth:** Requires `caller.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `suspend_merchant`

```rust
pub fn suspend_merchant(env: Env, admin: Address, merchant_id: Address, reason: String, expiration_duration: u64) -> Result<(), MerchantError>
```

Suspend a merchant (admin only)  # Arguments * `env` - The Soroban environment * `admin` - The admin address * `merchant_id` - The merchant address to suspend * `reason` - The reason for suspension * `expiration_duration` - Duration in seconds after which suspension auto-lifts

- **Auth:** Requires `admin.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `propose_merchant_suspension`

```rust
pub fn propose_merchant_suspension(env: Env, proposer: Address, merchant_id: Address, reason: String) -> Result<u64, MerchantError>
```

- **Auth:** Requires `proposer.require_auth()`; checks role(s) `arbitrator`
- **Events:** `MERCHANT/SUSPENSION_PROPOSED`
- **Error codes:** `Unauthorized`

#### `vote_suspension`

```rust
pub fn vote_suspension(env: Env, voter: Address, proposal_id: u64, approve: bool) -> Result<(), MerchantError>
```

- **Auth:** Requires `voter.require_auth()`; checks role(s) `arbitrator`
- **Events:** `MERCHANT/SUSPENDED`, `MERCHANT/SUSPENSION_VOTED`
- **Error codes:** `DuplicateVote`, `ProposalExpired`, `ProposalNotFound`, `Unauthorized`

#### `reinstate_merchant`

```rust
pub fn reinstate_merchant(env: Env, admin: Address, merchant_id: Address) -> Result<(), MerchantError>
```

Reinstate a suspended merchant (admin only)  # Arguments * `env` - The Soroban environment * `admin` - The admin address * `merchant_id` - The merchant address to reinstate

- **Auth:** Requires `admin.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_all_merchants`

```rust
pub fn get_all_merchants(env: Env, offset: u32, limit: u32) -> Vec<Merchant>
```

Get all registered merchants with pagination support

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `list_merchants`

```rust
pub fn list_merchants(env: Env, cursor: Option<String>, limit: u32) -> Result<MerchantPage, MerchantError>
```

List registered merchants using cursor-based pagination (Issue #774).  Limits page size to max 50 and returns next_cursor for seamless iteration across pages without duplicate or missed merchants.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `PageSizeTooLarge`

#### `get_verified_merchants`

```rust
pub fn get_verified_merchants(env: Env) -> Vec<Merchant>
```

Get all verified merchants (kyc_tier != Unverified)

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_fee_config`

```rust
pub fn set_fee_config(env: Env, admin: Address, _merchant_id: Address, fee_bps: i128, _fixed_fee: i128, fee_recipient: Option<Address>) -> Result<(), MerchantError>
```

Set the platform fee configuration (admin only). `_merchant_id` and `fixed_fee` are accepted for API compatibility and reserved for future per-merchant fee support.

- **Auth:** Requires `admin.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `calculate_platform_fee`

```rust
pub fn calculate_platform_fee(env: Env, amount: i128) -> (i128, Address)
```

Calculate the platform fee for a given amount using the stored global fee config. Returns `(fee_amount, fee_recipient)`. Returns `(0, contract_address)` if no config set.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `calculate_fee`

```rust
pub fn calculate_fee(env: Env, merchant_id: Address, amount: i128) -> Result<(i128, i128), MerchantError>
```

Calculate the platform fee for a given amount based on merchant's FeeConfig. Returns (platform_fee, net_amount).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_fee_recipient`

```rust
pub fn get_fee_recipient(env: Env, merchant_id: Address) -> Result<Address, MerchantError>
```

Get the fee recipient address for a merchant. Returns the custom fee recipient if set, otherwise the admin address.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `set_metadata_hash`

```rust
pub fn set_metadata_hash(env: Env, merchant_id: Address, metadata_hash: String) -> Result<(), MerchantError>
```

Set IPFS metadata hash for merchant profile (issue #208)

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** `MERCHANT/METADATA_UPDATED`
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_metadata_hash`

```rust
pub fn get_metadata_hash(env: Env, merchant_id: Address) -> Result<Option<String>, MerchantError>
```

Get IPFS metadata hash for merchant (issue #208)

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `add_currency_payout`

```rust
pub fn add_currency_payout(env: Env, merchant_id: Address, currency: String, payout_address: Address) -> Result<(), MerchantError>
```

Add payout address for a specific currency (issue #216)

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** `MERCHANT/CURRENCY_PAYOUT_ADDED`
- **Error codes:** `PayoutAddressNotWhitelisted`

#### `get_currency_payout`

```rust
pub fn get_currency_payout(env: Env, merchant_id: Address, currency: String) -> Result<Option<Address>, MerchantError>
```

Get payout address for a specific currency (issue #216)

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_all_currency_payouts`

```rust
pub fn get_all_currency_payouts(env: Env, merchant_id: Address) -> Result<Map<String, Address>, MerchantError>
```

Get all currency payout mappings for a merchant (issue #216)

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `add_to_whitelist`

```rust
pub fn add_to_whitelist(env: Env, merchant_id: Address, payout_address: Address) -> Result<(), MerchantError>
```

Add address to payout whitelist (issue #210)

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** `MERCHANT/WHITELIST_ADDED`
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `remove_from_whitelist`

```rust
pub fn remove_from_whitelist(env: Env, merchant_id: Address, payout_address: Address) -> Result<(), MerchantError>
```

Remove address from payout whitelist (issue #210)

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** `MERCHANT/WHITELIST_REMOVED`
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_whitelist`

```rust
pub fn get_whitelist(env: Env, merchant_id: Address) -> Result<Vec<Address>, MerchantError>
```

Get payout whitelist for a merchant (issue #210)

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `is_address_whitelisted`

```rust
pub fn is_address_whitelisted(env: Env, merchant_id: Address, payout_address: Address) -> Result<bool, MerchantError>
```

Validate if a payout address is whitelisted (issue #210)

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `suspend_merchant_by_system`

```rust
pub fn suspend_merchant_by_system(env: Env, caller: Address, merchant_id: Address, reason: String, expiration_duration: u64) -> Result<(), MerchantError>
```

Issue #184: System-initiated suspension triggered by the RefundManager contract when a merchant's dispute rate exceeds the auto-suspend threshold.  Unlike `suspend_merchant`, this function does not require admin auth. It accepts calls only from the configured RefundManager or PaymentProcessor.  # Arguments * `merchant_id`          – The merchant to suspend. * `reason`               – Human-readable suspension reason. * `expiration_duration`  – Duration in seconds after which the suspension auto-lifts.

- **Auth:** Requires `caller.require_auth()`
- **Events:** `MERCHANT/AUTO_SUSPENDED`
- **Error codes:** `Unauthorized`

#### `get_merchant_dispute_count`

```rust
pub fn get_merchant_dispute_count(env: Env, merchant_id: Address) -> u64
```

Issue #184 / #753: Get the current dispute count for a merchant.  The authoritative counter is owned by the `RefundManager`, since it is the contract that opens disputes. When a `RefundManager` address is configured we forward to it; otherwise (e.g. before wiring, or if the cross-contract call fails) we fall back to the registry-local `Merchant.dispute_count`, which is incremented via [`Self::increment_merchant_dispute_count`].

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `upgrade`

```rust
pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<(), MerchantError>
```

Upgrade the contract WASM.  Only the admin can call this. Emits a `CONTRACT/UPGRADED` event on success.

- **Auth:** Requires `admin.require_auth()`
- **Events:** `CONTRACT/UPGRADED`
- **Error codes:** `Unauthorized`

#### `transfer_admin`

```rust
pub fn transfer_admin(env: Env, current_admin: Address, new_admin: Address) -> Result<(), MerchantError>
```

Issue #398: Transfer MerchantRegistry admin ownership to a new address.  Only the current admin may call this. Once transferred, the old admin loses all admin privileges and the new admin is stored under `MerchantDataKey::Admin`.  Emits a `MERCHANT_REGISTRY / ADMIN_TRANSFERRED` event with `(old_admin, new_admin)` as the payload.

- **Auth:** Requires `current_admin.require_auth()`
- **Events:** `MERCHANT_REGISTRY/ADMIN_TRANSFERRED`
- **Error codes:** `Unauthorized`

#### `get_admin`

```rust
pub fn get_admin(env: Env) -> Option<Address>
```

Issue #398: Return the current admin address.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_merchant_anchor`

```rust
pub fn set_merchant_anchor(env: Env, merchant_id: Address, anchor_config: Option<AnchorConfig>) -> Result<(), MerchantError>
```

Configure or clear the merchant's Stellar Anchor (SEP-6 / SEP-24) integration for fiat offramp during settlement.  When `anchor_config` is `Some`, the off-chain Settlement Service will call the anchor's SEP-6 withdrawal endpoint after each on-chain settlement. If `anchor_config` is `None`, the merchant reverts to on-chain-only settlement (USDC remains at their `payout_address`).  Only the merchant themselves may call this. Admin can set an anchor on the merchant's behalf by first impersonating them via their authorized signer setup (outside this contract).  Emits `(MERCHANT, ANCHOR_UPDATED) → (merchant_id, anchor_domain_opt)` where `anchor_domain_opt` is the empty string when clearing.

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** `MERCHANT/ANCHOR_UPDATED`
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `set_anchor_config`

```rust
pub fn set_anchor_config(env: Env, merchant_id: Address, config: AnchorConfig) -> Result<(), MerchantError>
```

Issue #669: Set (or replace) a merchant's SEP-6/SEP-24 anchor configuration. Thin, always-set wrapper around `set_merchant_anchor` (which also supports clearing via `None`) that additionally requires the merchant to hold at least `KycTier::Basic` before an anchor may be attached, per the SEP-6/SEP-24 integration guide.  Requires the merchant's own signature (same auth as `set_merchant_anchor`).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `NotVerified`

#### `get_anchor_config`

```rust
pub fn get_anchor_config(env: Env, merchant_id: Address) -> Option<AnchorConfig>
```

Issue #669: Public read-only accessor for a merchant's anchor configuration. Returns `None` if the merchant has not configured an anchor (or does not exist).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_merchant_whitelist_mode`

```rust
pub fn set_merchant_whitelist_mode(env: Env, merchant_id: Address, enabled: bool) -> Result<(), MerchantError>
```

Enable/disable whitelist mode for a merchant (issue #516). Only Business-tier merchants may enable whitelist mode.

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** `MERCHANT/WHITELIST_UPDATED`
- **Error codes:** `WhitelistModeRequiresBusinessTier`

#### `set_merchant_fee_waiver`

```rust
pub fn set_merchant_fee_waiver(env: Env, admin: Address, merchant_id: Address, expires_at: Option<u64>) -> Result<(), MerchantError>
```

Admin-only: apply or clear a time-based fee waiver for a merchant.  While `expires_at` is `Some(ts)`, all settlements for this merchant pay zero platform fee until `ledger.timestamp < expires_at`. This enables onboarding/promotion campaigns. Pass `None` to immediately revoke an active waiver.  Auth: only the MerchantRegistry admin.  Emits `(MERCHANT, FEE_WAIVER_SET) → (merchant_id, expires_at_opt)` where `expires_at_opt` is 0 when clearing.

- **Auth:** Requires `admin.require_auth()`
- **Events:** `MERCHANT/FEE_WAIVER_SET`
- **Error codes:** `Unauthorized`

#### `add_to_customer_whitelist`

```rust
pub fn add_to_customer_whitelist(env: Env, merchant_id: Address, customer: Address) -> Result<(), MerchantError>
```

Add a customer address to the merchant's payment whitelist (issue #516).

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** `MERCHANT/WHITELIST_UPDATED`
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `remove_from_customer_whitelist`

```rust
pub fn remove_from_customer_whitelist(env: Env, merchant_id: Address, customer: Address) -> Result<(), MerchantError>
```

Remove a customer address from the merchant's payment whitelist (issue #516).

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** `MERCHANT/WHITELIST_UPDATED`
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `is_customer_whitelisted`

```rust
pub fn is_customer_whitelisted(env: Env, merchant_id: Address, customer: Address) -> Result<bool, MerchantError>
```

Check whether a customer address is allowed to pay a merchant (issue #516). Returns true when whitelist mode is disabled, or when the address is present in the merchant's customer whitelist.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `set_settlement_schedule`

```rust
pub fn set_settlement_schedule(env: Env, merchant_id: Address, schedule: SettlementSchedule) -> Result<(), MerchantError>
```

Set the settlement schedule for a merchant. Only the merchant can change their own schedule.

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** `MERCHANT/SETTLEMENT_SCHEDULE_UPDATED`
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `set_dispute_threshold`

```rust
pub fn set_dispute_threshold(env: Env, admin: Address, threshold: u32) -> Result<(), MerchantError>
```

Issue #481: Set the global dispute threshold for auto-suspension. When a merchant's lost_disputes_count reaches or exceeds this, the merchant is automatically suspended.

- **Auth:** Requires `admin.require_auth()`
- **Events:** `MERCHANT/DISPUTE_THRESHOLD_SET`
- **Error codes:** `Unauthorized`

#### `get_pending_settlement`

```rust
pub fn get_pending_settlement(env: Env, merchant_id: Address) -> i128
```

Get the accumulated pending settlement amount for a merchant.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `add_pending_settlement`

```rust
pub fn add_pending_settlement(env: &Env, merchant_id: &Address, amount: i128)
```

Add an amount to the merchant's pending settlement balance. Called by settle_payment in PaymentProcessor after a payment is settled.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `clear_pending_settlement`

```rust
pub fn clear_pending_settlement(env: &Env, merchant_id: &Address)
```

Clear the pending settlement balance for a merchant.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_last_settlement_at`

```rust
pub fn get_last_settlement_at(env: &Env, merchant_id: &Address) -> Option<u64>
```

Get the last settlement timestamp for a merchant.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_last_settlement_at`

```rust
pub fn set_last_settlement_at(env: Env, caller: Address, merchant_id: Address, timestamp: u64) -> Result<(), MerchantError>
```

Set the last settlement timestamp for a merchant.

- **Auth:** Requires `caller.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_dispute_threshold`

```rust
pub fn get_dispute_threshold(env: Env) -> u32
```

Issue #481: Get the current global dispute threshold.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `increment_merchant_dispute_count`

```rust
pub fn increment_merchant_dispute_count(env: Env, caller: Address, merchant_id: Address) -> Result<u32, MerchantError>
```

Issue #481: Increment dispute count for a merchant and check for auto-suspension. Returns the new dispute count.

- **Auth:** Requires `caller.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `increment_resolved_against_count`

```rust
pub fn increment_resolved_against_count(env: Env, caller: Address, merchant_id: Address) -> Result<u32, MerchantError>
```

Issue #481: Increment the resolved-against count for a merchant and check auto-suspension. Returns the new resolved_against_merchant_count.

- **Auth:** Requires `caller.require_auth()`
- **Events:** `MERCHANT/AUTO_SUSPENDED`
- **Error codes:** `Unauthorized`

#### `appeal_suspension`

```rust
pub fn appeal_suspension(env: Env, merchant_id: Address, reason: String) -> Result<(), MerchantError>
```

Issue #481: Appeal a merchant suspension. Creates a review record requiring operator approval. The merchant must be currently suspended for this to succeed.

- **Auth:** Requires `merchant_id.require_auth()`
- **Events:** `MERCHANT/SUSPENSION_APPEAL`
- **Error codes:** `Unauthorized`

#### `approve_suspension_appeal`

```rust
pub fn approve_suspension_appeal(env: Env, admin: Address, merchant_id: Address) -> Result<(), MerchantError>
```

Issue #481: Admin function to approve a merchant's suspension appeal and unsuspend.

- **Auth:** Requires `admin.require_auth()`
- **Events:** `MERCHANT/SUSPENSION_APPEAL_APPROVED`
- **Error codes:** `Unauthorized`

#### `check_kyc_limit`

```rust
pub fn check_kyc_limit(env: Env, merchant_id: Address, amount: i128) -> Result<(), MerchantError>
```

Issue #777: Cross-contract function to enforce KYC tier limits during payment creation. Checks single-payment limit and calendar monthly volume cap. Resets monthly volume on the 1st of each month (UTC). Returns `KycLimitExceeded` if payment exceeds either limit.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `TierMonthlyLimitExceeded`, `TierSingleLimitExceeded`, `Unauthorized`

#### `check_monthly_limit_only`

```rust
pub fn check_monthly_limit_only(env: Env, merchant_id: Address, extra_amount: i128) -> Result<(), MerchantError>
```

Issue #1014: Cross-contract function for `verify_payment` to re-check the KYC tier monthly volume cap against an *additional* amount (e.g. the overpaid portion received on top of the originally invoiced amount, which `create_payment`'s call to `check_kyc_limit` never accounted for). On success, records `extra_amount` against the merchant's current calendar month volume.  Unlike `check_kyc_limit`, this does not re-check the per-payment single-transaction limit — that was already enforced on the original invoiced amount at `create_payment` time.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `TierMonthlyLimitExceeded`

#### `get_monthly_volume`

```rust
pub fn get_monthly_volume(env: Env, merchant_id: Address) -> i128
```

Issue #777: Query current monthly volume for a merchant in the current UTC month.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_kyc_tier_level`

```rust
pub fn get_kyc_tier_level(env: Env, merchant_id: Address) -> Result<u32, MerchantError>
```

Issue #777: Query KYC tier as numeric level (0 = Unverified, 1 = Basic, 2 = Full/Business).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_year_month`

```rust
pub fn get_year_month(timestamp: u64) -> (u32, u32)
```

Compute (year, month) in UTC from Unix epoch timestamp using Euclidean affine transform.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_reserve_policy`

```rust
pub fn set_reserve_policy(env: Env, admin: Address, merchant_id: Address, policy: Option<RollingReservePolicy>) -> Result<(), MerchantError>
```

Admin-only: set, update, or clear (pass `None`) a merchant's rolling reserve policy. When set, `PaymentProcessor::settle_payment` withholds `policy.reserve_bps` of each settlement for `policy.holding_period_secs` before it becomes available to sweep.

- **Auth:** Requires `admin.require_auth()`
- **Events:** `MERCHANT/RESERVE_POLICY_UPDATED`
- **Error codes:** `InvalidReservePolicy`, `Unauthorized`

#### `get_reserve_policy`

```rust
pub fn get_reserve_policy(env: Env, merchant_id: Address) -> Option<RollingReservePolicy>
```

Read a merchant's configured rolling reserve policy, if any.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `lock_reserve`

```rust
pub fn lock_reserve(env: Env, caller: Address, merchant_id: Address, amount: i128, unlock_at: u64) -> Result<u64, MerchantError>
```

Called by `PaymentProcessor::settle_payment` to lock `amount` of a merchant's settlement proceeds in a new reserve bucket maturing at `unlock_at`. Only the registered PaymentProcessor contract may call this.

- **Auth:** Requires `caller.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `release_reserve`

```rust
pub fn release_reserve(env: Env, caller: Address, merchant_id: Address) -> Result<i128, MerchantError>
```

Release every matured (`unlock_at <= now`) reserve bucket for a merchant, moving their total into the merchant's pending settlement balance so the next `trigger_settlement` sweep pays it out. Intended to be invoked (directly or via `PaymentProcessor::release_merchant_reserve`) on a schedule by an off-chain release daemon. Only the registered PaymentProcessor contract may call this, matching `lock_reserve`.

- **Auth:** Requires `caller.require_auth()`
- **Events:** `MERCHANT/RESERVE_RELEASED`
- **Error codes:** `Unauthorized`

#### `get_locked_reserve`

```rust
pub fn get_locked_reserve(env: Env, merchant_id: Address) -> i128
```

Total reserve currently locked (unreleased) for a merchant.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

---

## DexRouter

Source: `fluxapay/src/dex_router.rs` (6 public entrypoints)

Allowlisted DEX router registry used by swap-and-pay.

#### `factory`

```rust
pub fn factory(env: Env) -> Address
```

Get the router's factory address.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_amounts_out`

```rust
pub fn get_amounts_out(env: Env, amount_in: i128, path: Vec<Address>) -> Vec<i128>
```

Get the path length for a swap.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `swap_exact_tokens_for_tokens`

```rust
pub fn swap_exact_tokens_for_tokens(env: Env, amount_in: i128, amount_out_min: i128, path: Vec<Address>, to: Address, deadline: u64) -> Result<Vec<i128>, DexRouterError>
```

TODO: delegate token transfers to the real Soroswap router contract and pull liquidity from on-chain pools instead of simulated quotes.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `InsufficientLiquidity`, `SwapFailed`

#### `execute_swap`

```rust
pub fn execute_swap(env: Env, caller: Address, token_in: Address, token_out: Address, amount_in: i128, min_amount_out: i128, max_slippage_bps: u32) -> Result<i128, DexRouterError>
```

Execute a direct token swap with slippage tolerance and max_slippage_bps guard.  # Parameters * `caller` - The account requesting the swap (authorized) * `token_in` - The input token contract address * `token_out` - The output token contract address * `amount_in` - The exact amount of token_in to swap * `min_amount_out` - Minimum output amount acceptable to caller (reverts with SlippageExceeded if below) * `max_slippage_bps` - Maximum allowable slippage in basis points against quoted price (≤ 5000 = 50% max)

- **Auth:** Requires `caller.require_auth()`
- **Events:** `SWAP/EXECUTED`
- **Error codes:** `InsufficientLiquidity`, `NoOutputAmount`, `SlippageExceeded`

#### `get_amounts_in`

```rust
pub fn get_amounts_in(env: Env, amount_out: i128, path: Vec<Address>) -> Vec<i128>
```

Reverse quote (#888): minimum input across `path` to deliver exactly `amount_out` of the final token. Walks backwards inverting the per-hop 1% slip from get_amounts_out, so amounts[0] yields amounts[len-1] = amount_out.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `swap_tokens_for_exact_tokens`

```rust
pub fn swap_tokens_for_exact_tokens(env: Env, amount_out: i128, amount_in_max: i128, path: Vec<Address>, to: Address, deadline: u64) -> Result<Vec<i128>, DexRouterError>
```

Swap for an exact output (#888): merchant gets exactly `amount_out`; customer pays only the input computed at execution time, capped by `amount_in_max`.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `InsufficientLiquidity`, `NoOutputAmount`, `SlippageExceeded`, `SwapFailed`

---

## FXOracle

Source: `fluxapay/src/fx_oracle.rs` (25 public entrypoints)

Exchange-rate oracle: rate submission, aggregation, staleness/deviation checks.

#### `version`

```rust
pub fn version() -> u32
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `oracle_initialize`

```rust
pub fn oracle_initialize(env: Env, admin: Address, staleness_threshold: u64)
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `oracle_initialize_with_tokens`

```rust
pub fn oracle_initialize_with_tokens(env: Env, admin: Address, staleness_threshold: u64, allowed_tokens: Vec<Symbol>)
```

Issue #811: Initialise with a populated token allowlist.  Separate entry point rather than a changed signature on `oracle_initialize`, which is called from deployment scripts, tests and the SDK. Breaking all of them to add an argument most callers can supply afterwards via `add_allowed_token` would be a poor trade.  An **empty** allowlist means "not yet configured", and rate publishing is allowed through — see `require_pair_allowed` for why that default is the safe one here rather than the reverse.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `oracle_grant_role`

```rust
pub fn oracle_grant_role(env: Env, admin: Address, role: Symbol, account: Address) -> Result<(), FXOracleError>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `oracle_has_role`

```rust
pub fn oracle_has_role(env: Env, role: Symbol, account: Address) -> bool
```

- **Auth:** checks an access-control role
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_fx_admin`

```rust
pub fn get_fx_admin(env: Env) -> Option<Address>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_rate`

```rust
pub fn set_rate(env: Env, operator: Address, pair: Symbol, rate: i128, decimals: u32) -> Result<(), FXOracleError>
```

- **Auth:** Requires `operator.require_auth()`; checks role(s) `oracle`
- **Events:** `FX_RATE/UPDATED`, `ORACLE/RATE_QUORUM_REACHED`, `RATE/UPDATED`
- **Error codes:** `Unauthorized`

#### `set_oracle_quorum`

```rust
pub fn set_oracle_quorum(env: Env, admin: Address, quorum: u32) -> Result<(), FXOracleError>
```

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_oracle_submissions`

```rust
pub fn get_oracle_submissions(env: Env, pair: Symbol) -> Vec<OracleSubmission>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_rates_batch`

```rust
pub fn set_rates_batch(env: Env, operator: Address, rates: Vec<(Symbol, i128, u32)>) -> Result<u32, FXOracleError>
```

Atomically update up to 20 currency pairs. Requires the ORACLE role.  Emits `RATE/BATCH_UPDATED` with the number of pairs updated.

- **Auth:** Requires `operator.require_auth()`; checks role(s) `oracle`
- **Events:** `FX_RATE/UPDATED`, `RATE/BATCH_UPDATED`
- **Error codes:** `BatchTooLarge`, `EmptyBatch`, `Unauthorized`

#### `add_allowed_token`

```rust
pub fn add_allowed_token(env: Env, admin: Address, token: Symbol) -> Result<(), FXOracleError>
```

Issue #811: Admin adds a token to the publish allowlist.  Idempotent. An empty allowlist means enforcement is off; the first successful add turns it on.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `remove_allowed_token`

```rust
pub fn remove_allowed_token(env: Env, admin: Address, token: Symbol) -> Result<(), FXOracleError>
```

Issue #811: Admin removes a token from the publish allowlist.  Idempotent. Existing stored rates are left in place and age out through the normal staleness path.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_allowed_tokens`

```rust
pub fn get_allowed_tokens(env: Env) -> Vec<Symbol>
```

Tokens a rate may be published against. Empty means enforcement is off.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `is_token_allowed`

```rust
pub fn is_token_allowed(env: Env, token: Symbol) -> bool
```

Whether `token` is on the allowlist. Returns false when the list is empty.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_rate`

```rust
pub fn get_rate(env: Env, pair: Symbol) -> Result<RateData, FXOracleError>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `RateNotFound`

#### `check_rate_staleness`

```rust
pub fn check_rate_staleness(env: Env, pair: Symbol) -> bool
```

Permissionless staleness probe. Returns `true` when the rate is stale (or missing) and emits `RATE/STALE_ALERT` when stale data is present.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** `RATE/STALE_ALERT`
- **Error codes:** _n/a (infallible return type)_

#### `get_settlement_amount`

```rust
pub fn get_settlement_amount(env: Env, usdc_amount: i128, target_currency: Symbol) -> Result<i128, FXOracleError>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `PairNotFound`

#### `get_rate_or_inverse`

```rust
pub fn get_rate_or_inverse(env: Env, pair: Symbol) -> Result<RateData, FXOracleError>
```

Issue #636: Return the rate for `pair`, transparently falling back to the inverse pair when only the reciprocal is stored.  Lookup order: 1. Direct: `pair` (e.g. `EUR_USD`). If a rate is stored, it is returned as-is (subject to the usual staleness checks). A stored-but-stale direct rate surfaces `RateStale` — it does *not* fall through. 2. Inverse: `pair` with its `BASE`/`QUOTE` halves swapped (e.g. `USD_EUR`). If found, the returned `RateData` carries `1 / rate` scaled to `INVERSE_RATE_DECIMALS` fixed-point precision, with `pair` set to the originally-requested symbol and the timestamp/sequence copied from the inverse rate.  Returns [`FXOracleError::PairNotFound`] only when both the direct and inverse lookups find nothing (or `pair` is not a parseable `BASE_QUOTE` symbol).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `PairNotFound`, `RateNotFound`

#### `get_settlement_amount_for_pair`

```rust
pub fn get_settlement_amount_for_pair(env: Env, from: Symbol, to: Symbol, amount: i128) -> Result<i128, FXOracleError>
```

Issue #636: Convert `amount` units of `from` into `to`, using the `FROM_TO` pair rate and automatically falling back to the inverse `TO_FROM` pair via [`Self::get_rate_or_inverse`].  `result = amount * rate / 10^decimals`.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `PairNotFound`

#### `get_staleness_threshold`

```rust
pub fn get_staleness_threshold(env: Env) -> u64
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `upgrade`

```rust
pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<(), FXOracleError>
```

Upgrade the contract WASM.  Only the admin can call this. Emits a `CONTRACT/UPGRADED` event with the old and new version strings on success.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `CONTRACT/UPGRADED`
- **Error codes:** `Unauthorized`

#### `set_staleness_threshold`

```rust
pub fn set_staleness_threshold(env: Env, admin: Address, threshold: u64) -> Result<(), FXOracleError>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `set_max_staleness`

```rust
pub fn set_max_staleness(env: Env, admin: Address, threshold: u64) -> Result<(), FXOracleError>
```

Set the maximum accepted age of a rate in seconds.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidStalenessThreshold`, `Unauthorized`

#### `set_rate_deviation_limit`

```rust
pub fn set_rate_deviation_limit(env: Env, admin: Address, pair: Symbol, max_deviation_bps: u32) -> Result<(), FXOracleError>
```

Issue #478: Set maximum allowed rate deviation per currency pair in basis points. Example: 1000 = 10% max allowed deviation.

- **Auth:** Requires `admin.require_auth()`; checks role(s) `admin`
- **Events:** `RATE/DEVIATION_LIMIT_SET`
- **Error codes:** `Unauthorized`

#### `get_rate_deviation_limit`

```rust
pub fn get_rate_deviation_limit(env: Env, pair: Symbol) -> u32
```

Issue #478: Get the maximum allowed rate deviation for a pair (in basis points). Returns 0 if no limit is configured (unlimited).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

---

## Htlc

Source: `fluxapay/src/htlc.rs` (5 public entrypoints)

Hash Time-Locked Contracts for cross-chain/conditional payments.

#### `create_htlc`

```rust
pub fn create_htlc(env: Env, sender: Address, receiver: Address, token: Address, amount: i128, hashlock: BytesN<32>, timeout_ledger: u32) -> Result<String, HtlcError>
```

Locks `amount` of `token` from `sender` into escrow, payable to `receiver` on reveal of the preimage of `hashlock`, or refundable to `sender` after `timeout_ledger`. Returns the new HTLC id.

- **Auth:** Requires `sender.require_auth()`
- **Events:** `HTLC/CREATED`
- **Error codes:** `InvalidAmount`, `InvalidTimeout`

#### `claim_htlc`

```rust
pub fn claim_htlc(env: Env, htlc_id: String, claimant: Address, preimage: Bytes) -> Result<(), HtlcError>
```

Releases the escrow to `claimant`, who must be the HTLC's `receiver` and must supply a `preimage` whose SHA-256 hash matches `hashlock`. Must happen strictly before `timeout_ledger`.

- **Auth:** Requires `claimant.require_auth()`
- **Events:** `HTLC/CLAIMED`
- **Error codes:** `AlreadyClaimed`, `Expired`, `PreimageMismatch`, `Unauthorized`

#### `refund_htlc`

```rust
pub fn refund_htlc(env: Env, htlc_id: String, caller: Address) -> Result<(), HtlcError>
```

Returns the escrowed tokens to the original `sender`. Only the sender may call this, and only after `timeout_ledger` has passed.

- **Auth:** Requires `caller.require_auth()`
- **Events:** `HTLC/REFUNDED`
- **Error codes:** `AlreadyClaimed`, `NotExpired`, `Unauthorized`

#### `claim_htlc_timeout`

```rust
pub fn claim_htlc_timeout(env: Env, htlc_id: String) -> Result<(), HtlcError>
```

Issue #1021: permissionless timeout sweep. Callable by *any* account (no `require_auth` needed from the caller - the contract itself authorizes moving its own escrowed balance). Succeeds only once `env.ledger().sequence() >= htlc.timeout_ledger`, and always refunds to the original `sender`, never to the caller.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** `HTLC/TIMEOUT_REFUNDED`
- **Error codes:** `AlreadyClaimed`, `NotExpired`

#### `get_htlc`

```rust
pub fn get_htlc(env: Env, htlc_id: String) -> Result<HtlcRecord, HtlcError>
```

Read-only lookup. Returns `HtlcError::NotFound` if the id does not exist.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

---

## GasEstimator

Source: `fluxapay/src/gas_estimator.rs` (4 public entrypoints)

Static operation cost/fee estimates for client-side gas budgeting.

#### `initialize`

```rust
pub fn initialize(env: Env, payment_processor: Address)
```

Link this estimator to the PaymentProcessor that owns the global pause flag.  Must be called once after deployment so entry points can honour emergency pauses (Issue #835).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `estimate`

```rust
pub fn estimate(env: Env, op: Operation) -> Result<CostEstimate, GasEstimatorError>
```

Return the resource cost estimate for `op`.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `estimate_payment_fee`

```rust
pub fn estimate_payment_fee(env: Env, op: Operation) -> Result<CostEstimate, GasEstimatorError>
```

Estimate the payment fee, reading the token balance at most once per transaction invocation.  The balance is cached in instance storage under [`DataKey::CachedBalance`]. Because Soroban instance storage is transaction-scoped, the cached value is discarded when the invocation completes and can never be observed by a later transaction. Subsequent calls within the same invocation return the cached value instead of issuing another cross-contract call.  Issue #835: returns `ContractPaused` when the linked PaymentProcessor is under a global emergency pause, preventing cross-contract balance reads during incident response.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `operation_name`

```rust
pub fn operation_name(env: Env, op: Operation) -> Result<Symbol, GasEstimatorError>
```

Human-readable name for an operation (display helpers).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

---

## PaymentLinkManager

Source: `fluxapay/src/payment_link.rs` (27 public entrypoints)

Shareable payment-link creation, analytics, and direct-transfer tracking.

#### `version`

```rust
pub fn version() -> u32
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `initialize`

```rust
pub fn initialize(env: Env, admin: Address) -> Result<(), crate::Error>
```

Initialize the contract with an admin address.

- **Auth:** Requires `admin.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `grant_merchant_role`

```rust
pub fn grant_merchant_role(env: Env, admin: Address, merchant_id: Address) -> Result<(), crate::Error>
```

- **Auth:** Requires `admin.require_auth()`; checks role(s) `merchant`
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`, `Unauthorized`

#### `revoke_merchant_role`

```rust
pub fn revoke_merchant_role(env: Env, admin: Address, merchant_id: Address) -> Result<(), crate::Error>
```

- **Auth:** Requires `admin.require_auth()`; checks role(s) `merchant`
- **Events:** _none observed in this function body_
- **Error codes:** `AccessControlError`, `Unauthorized`

#### `upgrade`

```rust
pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<(), crate::Error>
```

Upgrade the contract WASM.  Only the admin can call this. Emits a `CONTRACT/UPGRADED` event on success.

- **Auth:** Requires `admin.require_auth()`
- **Events:** `CONTRACT/UPGRADED`
- **Error codes:** `Unauthorized`

#### `set_payment_base_url`

```rust
pub fn set_payment_base_url(env: Env, admin: Address, url: String) -> Result<(), crate::Error>
```

Set the default base URL used when `create_link` omits `base_url`.

- **Auth:** Requires `admin.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `get_payment_base_url`

```rust
pub fn get_payment_base_url(env: Env) -> Option<String>
```

Return the admin-configured default payment base URL, if any.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_payment_link_fee_bps`

```rust
pub fn set_payment_link_fee_bps(env: Env, admin: Address, link_id: Option<String>, fee_bps: Option<i128>) -> Result<(), crate::Error>
```

Issue #663: Set a per-link or contract-wide default fee override (basis points) for payment links, following the admin-gated setter pattern used by `set_fee_rate`/`set_refund_fee_bps`.  * `link_id = Some(id)` — sets (or clears, when `fee_bps` is `None`) a fee override on that specific link. Only the admin may call this, so custom per-link fees cannot be self-assigned by a merchant at link-creation time. * `link_id = None` — sets (or clears) the contract-wide default fee applied by `use_link` to any link without its own override.  `fee_bps`, when `Some`, must be in `0..=10_000` (0-100%).

- **Auth:** Requires `admin.require_auth()`
- **Events:** `LINK/FEE_BPS_SET`, `LINK/GLOBAL_FEE_BPS_SET`
- **Error codes:** `InvalidAmount`, `Unauthorized`

#### `get_effective_fee_bps`

```rust
pub fn get_effective_fee_bps(env: Env, link_id: String) -> Result<Option<i128>, crate::Error>
```

Issue #663: Returns the fee (basis points) that `use_link` would apply to `link_id` right now: the link's own override if set, otherwise the contract-wide default, otherwise `None` (no fee).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `create_link`

```rust
pub fn create_link(env: Env, merchant: Address, link_id: String, amount: Option<i128>, currency: Symbol, description: String, expires_at: Option<u64>, max_uses: Option<u32>, direct_transfer: bool, metadata: Option<Map<String, String>>, fiat: MaybeFiatConfig, base_url: Option<String>) -> Result<String, crate::Error>
```

- **Auth:** Requires `merchant.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `create_single_use_link`

```rust
pub fn create_single_use_link(env: Env, merchant: Address, link_id: String, amount: Option<i128>, currency: Symbol, description: String, expires_at: Option<u64>, direct_transfer: bool, metadata: Option<Map<String, String>>, fiat: MaybeFiatConfig, base_url: Option<String>) -> Result<String, crate::Error>
```

Issue #1061: Create a payment link that can only be redeemed once.  Identical to `create_link` but sets `single_use: true` on the resulting `PaymentLink`. A second call to `use_link` on such a link will return `Error::LinkAlreadyUsed`.

- **Auth:** Requires `merchant.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `batch_create_links`

```rust
pub fn batch_create_links(env: Env, merchant: Address, links: Vec<CreateLinkArgs>) -> Result<Vec<String>, crate::Error>
```

Issue #637: Bulk-create up to `MAX_LINKS_PER_BATCH` (50) payment links for a single merchant in one atomic call.  * Returns `BatchTooLarge` if more than 50 links are supplied. * Per-item validation (`validate_id`, metadata limits) still runs for every link; a `link_id` that already exists — on-chain or earlier in the same batch — is rejected with `PaymentAlreadyExists`. * On any failure the whole transaction reverts, so either all links are created or none are. * Returns the created link IDs in submission order.

- **Auth:** Requires `merchant.require_auth()`
- **Events:** `LINK/BATCH_CREATED`
- **Error codes:** `BatchTooLarge`, `PaymentAlreadyExists`

#### `get_merchant_links`

```rust
pub fn get_merchant_links(env: Env, merchant_id: Address, offset: u32, limit: u32, active_only: bool) -> Vec<PaymentLink>
```

Issue #634: List a merchant's payment links, paginated.  Returns up to `limit` `PaymentLink` records (hard-capped at 100 per call) starting at `offset` within the merchant's link index, in creation order. When `active_only` is `true`, links that are deactivated or expired are filtered out *before* pagination (expiry is evaluated live via the same auto-deactivation path as `get_link`).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_link_url`

```rust
pub fn get_link_url(env: Env, link_id: String) -> Option<String>
```

Return the shareable URL for a link, if one was stored.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `record_link_view`

```rust
pub fn record_link_view(env: Env, viewer: Address, link_id: String) -> Result<(), crate::Error>
```

Record a view of a payment link on behalf of `viewer`.  The `viewer` must authorize the call. Merchants (or the storefront acting on their behalf) use this to track how many people viewed the link versus how many actually paid, enabling conversion-rate analytics via `get_link_analytics`.  Issue #754: previously this was a permissionless entry point, which let any address force unbounded persistent-storage writes and event emissions for every active link. It now requires authorization and enforces a per-viewer rolling-window rate limit (see `enforce_link_view_rate_limit`), mirroring `use_link`.

- **Auth:** Requires `viewer.require_auth()`
- **Events:** `LINK/VIEWED`
- **Error codes:** `Unauthorized`

#### `use_link`

```rust
pub fn use_link(env: Env, payer: Address, link_id: String, amount: i128, usdc_token: Option<Address>) -> Result<String, crate::Error>
```

- **Auth:** Requires `payer.require_auth()`
- **Events:** `LINK/DIRECT_TRANSFER_USED`, `LINK/FEE_APPLIED`, `LINK/MAX_USES_REACHED`, `LINK/USED`, `PAYMENT_LINK/USED`
- **Error codes:** `InvalidAmount`, `LinkAlreadyUsed`, `LinkExpired`, `LinkMaxUsesReached`, `StaleOracleRate`, `Unauthorized`

#### `get_payment`

```rust
pub fn get_payment(env: Env, payment_id: String) -> Result<PaymentCharge, crate::Error>
```

Get a payment charge created from a payment link. Returns the PaymentCharge record for the given payment_id.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `PaymentNotFound`

#### `get_link_payments`

```rust
pub fn get_link_payments(env: Env, link_id: String) -> Result<Vec<String>, crate::Error>
```

Get all payment IDs generated from a specific payment link. Returns a vector of payment IDs in chronological order.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `deactivate_link`

```rust
pub fn deactivate_link(env: Env, merchant: Address, link_id: String) -> Result<(), crate::Error>
```

- **Auth:** Requires `merchant.require_auth()`
- **Events:** `LINK/DEACTIVATED`
- **Error codes:** `Unauthorized`

#### `update_link_max_uses`

```rust
pub fn update_link_max_uses(env: Env, caller: Address, link_id: String, new_max_uses: u32) -> Result<(), crate::Error>
```

Admin or merchant can update max_uses on an existing link (only increase, not decrease below use_count).

- **Auth:** Requires `caller.require_auth()`
- **Events:** `LINK/MAX_USES_UPDATED`
- **Error codes:** `InvalidAmount`, `Unauthorized`

#### `expire_link`

```rust
pub fn expire_link(env: Env, link_id: String) -> Result<(), crate::Error>
```

Permissionless entry point that deactivates an expired payment link. If the link has not expired or is already inactive, the call is idempotent (succeeds without changing state). Emits LINK/EXPIRED on actual deactivation.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** `LINK/EXPIRED`
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `batch_expire_links`

```rust
pub fn batch_expire_links(env: Env, link_ids: Vec<String>) -> Result<u32, crate::Error>
```

Batch deactivate expired payment links (max 20 per call). Iterates over the provided link IDs and calls expire_link for each. Returns the count of links that were actually deactivated.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `BatchTooLarge`

#### `get_link`

```rust
pub fn get_link(env: Env, link_id: String) -> Result<PaymentLink, crate::Error>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `get_link_analytics`

```rust
pub fn get_link_analytics(env: Env, link_id: String) -> Result<LinkAnalytics, crate::Error>
```

Retrieve analytics for a payment link.  Returns view_count, use_count, total_revenue, and conversion_rate (in basis points: `use_count * 10_000 / view_count`, or `0` if the link has not been viewed).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `verify_batch`

```rust
pub fn verify_batch(env: Env, link_ids: Vec<String>) -> Vec<(String, bool, u32, Option<u32>)>
```

Verify the status of multiple payment links in a single call. Returns a vector of (link_id, is_active, use_count, max_uses) tuples.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_link_rate_limit`

```rust
pub fn set_link_rate_limit(env: Env, admin: Address, max_uses: u32, window_secs: u64) -> Result<(), crate::Error>
```

Default per-payer rate-limit config: 5 uses per 60 seconds. Admin-only: configure the per-payer link-use rate limit.  * `max_uses`    – Maximum number of `use_link` calls allowed per payer within the window. * `window_secs` – Rolling window length in seconds.

- **Auth:** Requires `admin.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

#### `set_link_view_rate_limit`

```rust
pub fn set_link_view_rate_limit(env: Env, admin: Address, max_views: u32, window_secs: u64) -> Result<(), crate::Error>
```

Default per-viewer rate-limit config: 100 views per 60 seconds. Admin-only: configure the per-viewer payment-link view rate limit.  * `max_views`   – Maximum number of `record_link_view` calls allowed per viewer within the window. * `window_secs` – Rolling window length in seconds.

- **Auth:** Requires `admin.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `Unauthorized`

---

## PaymentStreaming

Source: `fluxapay/src/stream.rs` (32 public entrypoints)

Continuous/vesting payment streams, including multi-payee streams.

#### `version`

```rust
pub fn version() -> u32
```

Contract version bump helper.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_stream_fee_bps`

```rust
pub fn set_stream_fee_bps(env: Env, fee_bps: i128) -> Result<(), StreamError>
```

Set the platform fee in basis points applied to stream withdrawals. Admin auth is enforced by the caller (PaymentProcessor).

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidRate`

#### `get_stream_fee_bps`

```rust
pub fn get_stream_fee_bps(env: Env) -> i128
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `set_stream_fee_recipient`

```rust
pub fn set_stream_fee_recipient(env: Env, recipient: Address)
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `get_stream_fee_recipient`

```rust
pub fn get_stream_fee_recipient(env: Env) -> Option<Address>
```

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `create_stream`

```rust
pub fn create_stream(env: Env, sender: Address, receiver: Address, token: Address, rate_per_second: i128, deposit: i128, stream_id: String, min_rate: Option<i128>) -> Result<PaymentStream, StreamError>
```

Create a new payment stream.  The caller (sender) transfers `deposit` tokens from their account into this contract. Streaming begins immediately at `rate_per_second`.  # Parameters * `sender`         – Account funding the stream; must sign the transaction. * `receiver`       – Account that will receive streamed tokens. * `token`          – Token contract address. * `rate_per_second`– Tokens per second to stream. * `deposit`        – Total tokens deposited upfront. * `stream_id`      – Caller-supplied unique identifier.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `create_stream_with_cliff`

```rust
pub fn create_stream_with_cliff(env: Env, sender: Address, receiver: Address, token: Address, rate_per_second: i128, deposit: i128, stream_id: String, min_rate: Option<i128>, cliff_at: u64) -> Result<PaymentStream, StreamError>
```

Create a payment stream whose accrued tokens stay locked until `cliff_at`.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `set_stream_destination`

```rust
pub fn set_stream_destination(env: Env, recipient: Address, stream_id: String, destination: Address) -> Result<(), StreamError>
```

Configure a fixed destination address for a stream.  This lets anyone trigger a withdrawal later, while the recipient retains control of where funds are routed.  Auth: only the stream's `receiver` may call this (recipient.require_auth() + ownership check). Rejects if the stream is not Active.

- **Auth:** Requires `recipient.require_auth()`
- **Events:** `STREAM/DESTINATION_SET`
- **Error codes:** `InvalidReceiver`, `StreamNotActive`, `StreamNotFound`, `Unauthorized`

#### `approve_stream_milestone`

```rust
pub fn approve_stream_milestone(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError>
```

Sender approves milestones for a stream, unlocking distributions.

- **Auth:** Requires `sender.require_auth()`
- **Events:** `STREAM/MILESTONE_APPROVED`
- **Error codes:** `StreamNotFound`, `Unauthorized`

#### `revoke_stream_milestone`

```rust
pub fn revoke_stream_milestone(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError>
```

Sender revokes a previous milestone approval, re-locking distributions until `approve_stream_milestone` is called again.

- **Auth:** Requires `sender.require_auth()`
- **Events:** `STREAM/MILESTONE_REVOKED`
- **Error codes:** `StreamNotFound`, `Unauthorized`

#### `set_stream_min_rate`

```rust
pub fn set_stream_min_rate(env: Env, sender: Address, stream_id: String, min_rate_per_second: i128) -> Result<(), StreamError>
```

Sender sets a floor below which [`Self::decrease_rate_per_second`] will refuse to lower the stream's rate.

- **Auth:** Requires `sender.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `InvalidRate`, `StreamNotFound`, `Unauthorized`

#### `decrease_rate_per_second`

```rust
pub fn decrease_rate_per_second(env: Env, sender: Address, stream_id: String, new_rate: i128) -> Result<(), StreamError>
```

Reduce the flow rate of an active stream.  This function: 1. **Checkpoints** the `accrued_amount` and current timestamp so that earnings up to this moment are locked in before the rate changes. 2. **Refunds** any portion of the deposit that exceeds the tokens still accruing at the new (lower) rate: `surplus = remaining_deposit - accrued_since_checkpoint - new_needed` where `new_needed` is what the new rate needs to sustain for the same remaining seconds the old rate would have run. 3. **Emits** a `RateDecreased` event.  # Parameters * `sender`       – Must be the original stream sender; must sign. * `stream_id`    – Stream to update. * `new_rate`     – New rate (must be strictly less than the current rate).

- **Auth:** Requires `sender.require_auth()`
- **Events:** `STREAM/RATE_DECREASED`
- **Error codes:** `InvalidRate`, `RateBelowMinimum`, `RateNotDecreased`, `StreamNotActive`, `StreamNotFound`, `Unauthorized`

#### `get_stream`

```rust
pub fn get_stream(env: Env, stream_id: String) -> Result<PaymentStream, StreamError>
```

Return the stored stream, or `StreamNotFound`.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `StreamNotFound`

#### `get_accrued_amount`

```rust
pub fn get_accrued_amount(env: Env, stream_id: String) -> Result<i128, StreamError>
```

Compute the total tokens accrued at the current ledger timestamp.  This is a **view-only** helper and does not modify state.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `StreamNotFound`

#### `get_sender_streams`

```rust
pub fn get_sender_streams(env: Env, sender: Address, page: u32, page_size: u32) -> Vec<PaymentStream>
```

Query streams created by the given sender.  Results are paginated and capped at 100 entries per page.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** _n/a (infallible return type)_

#### `withdraw_all_for_recipient`

```rust
pub fn withdraw_all_for_recipient(env: Env, recipient: Address, max_streams: u32) -> Result<Vec<String>, StreamError>
```

Withdraw accrued funds from up to `max_streams` active streams for the recipient.  This helper scans the recipient stream index and stops after processing the configured number of active streams to avoid excessive gas usage.

- **Auth:** Requires `recipient.require_auth()`
- **Events:** `STREAM/CLIFF_REACHED`, `STREAM/WITHDRAWN`
- **Error codes:** `StreamNotFound`

#### `trigger_withdrawal`

```rust
pub fn trigger_withdrawal(env: Env, stream_id: String) -> Result<String, StreamError>
```

Trigger a permissionless withdrawal for a stream with an explicit destination.  Only streams that have a destination configured and are active may be processed through this entrypoint.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** `STREAM/CLIFF_REACHED`, `STREAM/WITHDRAWN`
- **Error codes:** `CliffNotReached`, `DestinationNotSet`, `MilestoneNotApproved`, `StreamNotActive`, `StreamNotFound`

#### `top_up_multiple_streams`

```rust
pub fn top_up_multiple_streams(env: Env, sender: Address, top_ups: Vec<(String, i128)>) -> Result<(), StreamError>
```

Top up multiple streams in a single atomic transaction.  The caller (sender) must be the sender of ALL specified streams.  # Parameters * `sender`     – Must be the sender of all streams; must sign. * `top_ups`    – Vector of (stream_id, amount) tuples.

- **Auth:** Requires `sender.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `top_up_stream`

```rust
pub fn top_up_stream(env: Env, sender: Address, stream_id: String, amount: i128) -> Result<(), StreamError>
```

- **Auth:** Requires `sender.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `cancel_stream`

```rust
pub fn cancel_stream(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError>
```

Cancel an active stream and refund any remaining un-accrued deposit to the sender.  # Parameters * `sender`    – Must be the original stream sender; must sign. * `stream_id` – Stream to cancel.

- **Auth:** Requires `sender.require_auth()`
- **Events:** `STREAM/CANCELLED`, `STREAM/CLIFF_REACHED`
- **Error codes:** `StreamNotActive`, `StreamNotFound`, `Unauthorized`

#### `pause_stream`

```rust
pub fn pause_stream(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError>
```

Pause an active stream, freezing accrual until resumed.  Accrued tokens are check-pointed at the current timestamp so that earnings up to this moment are preserved. Only the stream sender may pause.  # Parameters * `sender`    – Must be the original stream sender; must sign. * `stream_id` – Stream to pause.

- **Auth:** Requires `sender.require_auth()`
- **Events:** `STREAM/PAUSED`
- **Error codes:** `StreamNotActive`, `StreamNotFound`, `Unauthorized`

#### `resume_stream`

```rust
pub fn resume_stream(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError>
```

Resume a paused stream, restarting accrual from the current timestamp.  Only the stream sender may resume.  # Parameters * `sender`    – Must be the original stream sender; must sign. * `stream_id` – Stream to resume.

- **Auth:** Requires `sender.require_auth()`
- **Events:** `STREAM/RESUMED`
- **Error codes:** `StreamNotFound`, `StreamNotPaused`, `Unauthorized`

#### `cancel_multiple_streams`

```rust
pub fn cancel_multiple_streams(env: Env, sender: Address, stream_ids: Vec<String>) -> Result<Vec<String>, StreamError>
```

Cancel multiple streams atomically.  Each stream is cancelled in order; if any stream fails the whole call reverts.  Returns the list of cancelled stream IDs.  # Parameters * `sender`     – Must be the sender of all streams; must sign. * `stream_ids` – IDs of streams to cancel.

- **Auth:** Requires `sender.require_auth()`
- **Events:** `STREAM/CANCELLED`, `STREAM/CLIFF_REACHED`
- **Error codes:** `StreamNotActive`, `StreamNotFound`, `Unauthorized`

#### `update_stream_rate`

```rust
pub fn update_stream_rate(env: Env, sender: Address, stream_id: String, new_rate: i128) -> Result<(), StreamError>
```

Update the flow rate of an active stream (increase or decrease).  Unlike `decrease_rate_per_second`, this function allows both increases and decreases. Accrued tokens are check-pointed at the current rate before the new rate takes effect.  When the rate is **decreased**, any surplus deposit is refunded to the sender (same logic as `decrease_rate_per_second`). When the rate is **increased**, no additional deposit is required; the existing deposit simply drains faster.  # Parameters * `sender`    – Must be the original stream sender; must sign. * `stream_id` – Stream to update. * `new_rate`  – New flow rate (must be > 0).

- **Auth:** Requires `sender.require_auth()`
- **Events:** `STREAM/RATE_UPDATED`
- **Error codes:** `InvalidRate`, `StreamNotActive`, `StreamNotFound`, `Unauthorized`

#### `close_expired_stream`

```rust
pub fn close_expired_stream(env: Env, stream_id: String) -> Result<(), StreamError>
```

Close an exhausted or cancelled stream and remove its storage entry.  This releases the contract storage slot for streams that have already reached a terminal state (`Exhausted` or `Cancelled`). Any remaining accrued balance is paid out to the receiver before the key is removed.  Can be called by anyone (permissionless) once the stream is terminal, incentivising keepers to clean up storage.  # Parameters * `stream_id` – Stream to close and remove.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** `STREAM/CLOSED`
- **Error codes:** `StreamNotActive`, `StreamNotFound`

#### `batch_cancel_streams`

```rust
pub fn batch_cancel_streams(env: Env, sender: Address, stream_ids: Vec<String>) -> Result<Vec<String>, StreamError>
```

Cancel up to `MAX_BATCH_CANCEL` streams in a single transaction, skipping streams that are not active or not owned by `sender` instead of aborting. Returns the list of successfully cancelled stream IDs.

- **Auth:** Requires `sender.require_auth()`
- **Events:** `STREAM/CANCELLED`, `STREAM/CLIFF_REACHED`
- **Error codes:** _see `fluxapay/src/lib.rs` `Error` enum / delegated helper for the full set_

#### `batch_withdraw_to`

```rust
pub fn batch_withdraw_to(env: Env, recipient: Address, withdrawals: Vec<crate::WithdrawalRecipient>) -> Result<Vec<String>, StreamError>
```

Withdraw accrued amounts from multiple streams to specified destinations in a single transaction.  The caller must be the receiver of each stream.  Returns the list of stream IDs that were processed.  # Parameters * `recipient`   – Must be the receiver of all streams; must sign. * `withdrawals` – Vector of `WithdrawalRecipient` (stream_id, destination, amount).

- **Auth:** Requires `recipient.require_auth()`
- **Events:** `STREAM/CLIFF_REACHED`, `STREAM/WITHDRAWN`
- **Error codes:** `CliffNotReached`, `MilestoneNotApproved`, `StreamNotActive`, `StreamNotFound`, `Unauthorized`

#### `bulk_bump_stream_ttls`

```rust
pub fn bulk_bump_stream_ttls(env: Env, stream_ids: Vec<String>) -> Result<u32, StreamError>
```

Issue #627: Extend the persistent-storage TTL of many stream entries in a single call, so contracts with many active streams pay one batched set of TTL writes instead of one per stream interaction.  Mirrors `PaymentProcessor::bulk_bump_payment_ttls`: * Permissionless — TTL bumps only ever extend an entry's lifetime. * Stream IDs that don't resolve to a stored stream are silently skipped. * Rejects batches larger than [`MAX_BULK_TTL_BUMP`] (50).  Returns the number of streams whose TTL was actually bumped.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** `STREAM/TTL_BUMPED`
- **Error codes:** `BatchTooLarge`

#### `create_multi_stream`

```rust
pub fn create_multi_stream(env: Env, sender: Address, token: Address, deposit: i128, rate_per_second: i128, payees: Vec<PayeeAllocation>) -> Result<String, StreamError>
```

Create a stream that splits accrual across up to [`MAX_MULTI_PAYEES`] weighted payees. All `share_bps` values must sum to [`MULTI_STREAM_SHARE_TOTAL`] (10_000).  Returns the auto-generated `stream_id`.

- **Auth:** Requires `sender.require_auth()`
- **Events:** `STREAM/MULTI_CREATED`
- **Error codes:** `EmptyPayees`, `InvalidDeposit`, `InvalidPayeeShares`, `InvalidRate`, `InvalidReceiver`, `TooManyPayees`

#### `withdraw_multi_stream`

```rust
pub fn withdraw_multi_stream(env: Env, stream_id: String) -> Result<(), StreamError>
```

Withdraw accrued funds from a multi-payee stream, distributing the withdrawable amount to every payee in proportion to `share_bps`.  Transfers are atomic within the call: all payees receive their share before the function returns. Dust from integer division is given to the last payee so the full withdrawable amount is distributed.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** `STREAM/MULTI_WITHDRAWN`
- **Error codes:** `NotMultiStream`, `StreamNotActive`

#### `get_multi_stream`

```rust
pub fn get_multi_stream(env: Env, stream_id: String) -> Result<MultiPaymentStream, StreamError>
```

Read a multi-payee stream by ID.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `NotMultiStream`

#### `top_up_stream`

```rust
pub fn top_up_stream(env: Env, sender: Address, stream_id: String, additional_deposit: i128) -> Result<(), StreamError>
```

Issue #1070: Add deposit to an active or paused stream without changing the rate.  The `sender` (original stream creator) transfers `additional_deposit` tokens from their account into this contract. The stream's `remaining_deposit` is updated atomically. The rate is unchanged.  # Parameters * `sender`             – Must be the original stream sender; must sign. * `stream_id`          – ID of the stream to top up. * `additional_deposit` – Positive amount of tokens to add.  # Errors * [`StreamError::StreamNotFound`]  – No stream with that ID. * [`StreamError::Unauthorized`]    – Caller is not the stream's sender. * [`StreamError::InvalidAmount`]   – `additional_deposit` is not positive. * [`StreamError::InvalidState`]    – Stream is `Cancelled` or `Exhausted`.

- **Auth:** Requires `sender.require_auth()`
- **Events:** `STREAM/TOPPED_UP`
- **Error codes:** `InvalidAmount`, `InvalidState`, `StreamNotFound`, `Unauthorized`

---

## SubscriptionContract

Source: `fluxapay/src/subscription.rs` (12 public entrypoints)

Recurring subscription plans, billing, dunning.

#### `create_plan`

```rust
pub fn create_plan(env: Env, merchant: Address, plan_id: Symbol, name: String, amount: i128, interval_secs: u64, max_cycles: Option( u32 ), grace_period_secs: u64) -> Result<(), SubscriptionError>
```

Create a new subscription plan owned by `merchant`.

- **Auth:** Requires `merchant.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `GracePeriodOutOfRANGE`, `InvalidInterval`

#### `subscribe`

```rust
pub fn subscribe(env: Env, subscriber: Address, plan_id: Symbol) -> Result<(), SubscriptionError>
```

Subscribe `subscriber` to an existing, non-archived plan.

- **Auth:** Requires `subscriber.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `AlreadySubscribed`, `PlanArchived`, `PlanInactive`, `PlanNotFound`

#### `archive_plan`

```rust
pub fn archive_plan(env: Env, merchant: Address, plan_id: Symbol) -> Result<(), SubscriptionError>
```

Retire a plan without deleting its history. Requires merchant auth. Existing subscribers continue to bill until they cancel.

- **Auth:** Requires `merchant.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `PlanNotFound`, `Unauthorized`

#### `restore_plan`

```rust
pub fn restore_plan(env: Env, admin: Address, plan_id: Symbol) -> Result<(), SubscriptionError>
```

Reactivate an archived plan. Admin only.

- **Auth:** Requires `admin.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `PlanNotFound`, `Unauthorized`

#### `charge_subscription`

```rust
pub fn charge_subscription(env: Env, subscription_id: Symbol, subscriber: Address, charge_succeeded: bool) -> Result<(), SubscriptionError>
```

Charge a subscription for the current billing cycle.  Enforces the configured billing interval and max cycle limit, and drives the dunning state machine on failure.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `DunningBackoffActive`, `IntervalNotElapsed`, `MaxCyclesReached`, `PaymentFailed`, `SubscriptionNotFound`, `SubscriptionPaused`

#### `pause_subscription`

```rust
pub fn pause_subscription(env: Env, subscription_id: Symbol, caller: Address) -> Result<(), SubscriptionError>
```

Pause a subscription. Caller must be the subscriber. Prevents charges while paused and emits SUBSCRIPTION/PAUSED.

- **Auth:** Requires `caller.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `NotSubscriber`, `SubscriptionNotFound`, `SubscriptionPaused`

#### `resume_subscription`

```rust
pub fn resume_subscription(env: Env, subscription_id: Symbol, caller: Address) -> Result<(), SubscriptionError>
```

Resume a paused subscription. Caller must be the subscriber. Adjusts the billing interval baseline to avoid back-billing for the paused duration and emits SUBSCRIPTION/RESUMED.

- **Auth:** Requires `caller.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `NotSubscriber`, `SubscriptionNotFound`, `SubscriptionNotPaused`

#### `cancel_within_grace_period`

```rust
pub fn cancel_within_grace_period(env: Env, subscription_id: Symbol, caller: Address) -> Result<(), SubscriptionError>
```

Cancel a subscription within the grace period and refund the most recent charge.

- **Auth:** Requires `caller.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `GracePeriodClosed`, `NotSubscriber`, `SubscriptionNotFound`

#### `cancel_subscription`

```rust
pub fn cancel_subscription(env: Env, subscription_id: Symbol, caller: Address) -> Result<(), SubscriptionError>
```

Cancel a subscription outside the grace period. No refund is issued.

- **Auth:** Requires `caller.require_auth()`
- **Events:** _none observed in this function body_
- **Error codes:** `NotSubscriber`, `SubscriptionNotFound`

#### `validate_grace_period`

```rust
pub fn validate_grace_period(grace_period_secs: u64) -> Result<u64, SubscriptionError>
```

Validate and normalize a merchant-configured grace period.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `GracePeriodOutOfRANGE`

#### `get_plan`

```rust
pub fn get_plan(env: Env, plan_id: Symbol) -> Result<Plan, SubscriptionError>
```

Fetch a plan by id.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `PlanNotFound`

#### `get_subscription`

```rust
pub fn get_subscription(env: Env, plan_id: Symbol, subscriber: Address) -> Result<Subscription, SubscriptionError>
```

Fetch a subscription by plan id and subscriber.

- **Auth:** None (public read, or requester-scoped via an argument address)
- **Events:** _none observed in this function body_
- **Error codes:** `SubscriptionNotFound`

---

