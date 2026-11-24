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

## Missing flags

The following modules have **no** feature flag, so they are not independently
deployable today — they compile as part of the default build and cannot be selected
on their own:

- `account_abstraction.rs` — account abstraction
- `meta_transaction.rs` — meta-transactions
- `subscription.rs` — subscriptions
- `dispute.rs` — dispute resolution
- `merkle_distributor.rs` — merkle-batch payouts

> Note: the originating issue lists `htlc` among the missing, but `contract-htlc`
> already exists in `Cargo.toml`, so `htlc` is covered. The genuinely missing flags
> are the five above. Each needs a `contract-<name>` feature in `Cargo.toml` plus
> a `cfg_attr(... contract-<name>, contractimpl)` gate on its `#[contractimpl]`
> block before it can be deployed in isolation.

## Next steps

- Add a `contract-<name>` feature for each missing module and gate its
  `#[contractimpl]` so an opt-in build produces only that contract.
- Extend CI to build each contract with its feature and assert the resulting WASM
  stays under the Soroban size budget, so a regression that bloats one contract is
  caught before deploy.
