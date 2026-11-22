# Feature-Flag Strategy for Contract Bundling

- Status: Accepted
- Date: 2026-11-19
- Related issues: #1225

## Context

`fluxapay` is a single Soroban crate (`fluxapay/src/`) that bundles every contract
together. Each contract must deploy as its own WASM, and Soroban enforces a tight
per-contract size budget, so the crate builds **exactly one contract at a time**
selected by a Cargo feature. `default = ["contract-payment-processor"]` keeps a
plain `cargo build` producing the primary contract; every other contract is an
opt-in `cargo build --no-default-features --features contract-<name>`.

## Strategy

1. **One feature per independently deployable contract.** The feature gates the
   `#[contractimpl]` block (see the `cfg_attr` gate at the top of `dex_router.rs`)
   so only the chosen contract is linked into the WASM.
2. **Shared modules (types, errors, events, utils, constants) are always compiled.**
   They carry no flag; they are linked into whichever contract is selected.
3. **`lib.rs` stays a thin wiring file** (ADR-0006): it declares modules and hosts
   `#[contractimpl]` delegations, never the bulk of the logic.
4. **Default build = PaymentProcessor.** CI and `cargo build` without flags must keep
   producing the primary contract so downstream tooling does not break.

## Deployable units

| Module | Feature flag | Independently deployable |
| --- | --- | --- |
| `payment_processor.rs` | `contract-payment-processor` (default) | yes |
| `stream.rs` / `payment_streaming.rs` | `contract-payment-streaming` | yes |
| `refund_manager.rs` | `contract-refund-manager` | yes |
| `gas_estimator.rs` | `contract-gas-estimator` | yes |
| `fx_oracle.rs` | `contract-fx-oracle` | yes |
| `merchant_registry.rs` | `contract-merchant-registry` | yes |
| `payment_link.rs` | `contract-payment-link` | yes |
| `dex_router.rs` | `contract-dex-router` | yes |
| `htlc.rs` | `contract-htlc` | yes |

## Modules that remain part of PaymentProcessor / RefundManager

The following modules compile as library code inside the primary contracts. Each
is a support module with no standalone `#[contract]` struct of its own, so they
do not need — and must not receive — an independent feature flag.

| Module | Host contract | Reason |
| --- | --- | --- |
| `account_abstraction.rs` | PaymentProcessor | Session-key and merchant-delegation helpers called by PaymentProcessor entry points; exposes only free functions and types, no standalone contract struct. |
| `meta_transaction.rs` | PaymentProcessor | Sponsored meta-transaction relay; dispatches into `PaymentProcessor::create_payment` and related methods. No standalone contract struct. |
| `dispute.rs` | PaymentProcessor | Dispute bond/bitmap helpers; dispute entry points live in PaymentProcessor's `contractimpl` block. No standalone contract struct. |
| `merkle_distributor.rs` | PaymentProcessor | Pure Merkle hashing and bitmap helpers; `create_batch` / `claim_batch_payment` entry points live in PaymentProcessor's `contractimpl` block per the module doc comment. No standalone contract struct. |

## Orphaned duplicate: `subscription.rs`

`fluxapay/src/subscription.rs` defines a `SubscriptionContract` struct with
`#[contract]` / `#[contractimpl]`, but it is **not declared as a module in
`lib.rs`** (there is no `mod subscription;` outside of `#[cfg(test)]` blocks).
The complete, production subscription implementation lives inside
`PaymentProcessor`'s `contractimpl` block in `lib.rs`, which integrates
subscriptions with payments, refunds, and the merchant registry.

Additionally, `subscription.rs` contains several compile-breaking syntax errors
(`Option(u32)` instead of `Option<u32>`, `Ok()` instead of `Ok(())`, the
undefined symbol `GRACE_CANCHL`) that make the file unshippable as-is.

**This file is an orphaned duplicate and must not be added to the deployable
feature list.** It is excluded from the build by virtue of never being declared
as a module. If a truly standalone `SubscriptionContract` WASM is ever needed in
the future, it should be derived from the production implementation in `lib.rs`,
not from this file.

## `htlc` — already covered

The originating issue listed `htlc` among the missing, but `contract-htlc`
already exists in `Cargo.toml` and `htlc.rs` already carries the matching
`cfg_attr` gate. It is covered.

## Build verification

Each deployable contract can be built in isolation with:

```bash
# Primary contract (default)
cargo build --target wasm32-unknown-unknown --release

# Any other standalone contract
cargo build --target wasm32-unknown-unknown --release \
  --no-default-features --features contract-<name>
```

Supported values for `<name>`: `payment-streaming`, `refund-manager`,
`gas-estimator`, `fx-oracle`, `merchant-registry`, `payment-link`,
`dex-router`, `htlc`.

Each build produces exactly one contract WASM. CI should assert each artifact
stays under the Soroban size budget so a regression is caught before deploy.
