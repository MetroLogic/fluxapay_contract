# ADR-0006: Module Boundaries for `lib.rs`

- Status: Accepted
- Date: 2026-11-19
- Related issues: #1222

## Context

`fluxapay/src/lib.rs` has grown to ~14,000 lines and ~290 `pub fn` entry points, mixing contract
wiring (`#[contract]` / `#[contractimpl]` blocks), shared type/enum definitions, and large amounts
of business logic. Earlier work split some logic into submodules (`payment_processor.rs`,
`refund_manager.rs`, `merchant_registry.rs`, `dex_router.rs`, `fx_oracle.rs`, etc.), but without a
guard `lib.rs` is free to re-accumulate logic, undoing the split over time.

This ADR records the target shape for `lib.rs` and the CI guard that keeps it.

## Decision

`lib.rs` is a **thin wiring file**. It declares modules, re-exports the public API, and hosts the
`#[contract]` / `#[contractimpl]` trait delegations. Business logic, storage helpers, and large
type bodies live in submodules.

### Allowed in `lib.rs`

- `#![...]` attributes, `use` of submodule items, and `pub mod`/`pub use` wiring.
- `#[contract]` structs and `#[contractimpl]` blocks whose bodies **delegate** to submodule
  functions (e.g. `fn create_payment(...) { payment_processor::create_payment(...) }`).
- Shared `#[contracttype]`/`#[contracterror]` enums that are referenced across modules and would
  create circular module deps if moved (the `Error`, `PaymentStatus`, payment/dispute args).
- Crate-level `const` values that other modules import via `pub use constants::*`.

### Not allowed in `lib.rs`

- New business logic inside `#[contractimpl]` bodies (delegate to a submodule instead).
- New storage-access or state-machine logic (it belongs in `payment_state_machine.rs`,
  `payment_processor.rs`, `refund_manager.rs`, etc.).
- New helper functions that are not thin delegations.

### CI guard

`scripts/check_librs_boundaries.sh` (wired into `.github/workflows/ci.yml`) enforces two frozen
baselines against `fluxapay/src/lib.rs`:

- **Line count** must not exceed the baseline (`.librs-baseline`) beyond a small tolerance.
- **`pub fn` count** must not exceed the baseline (strict — new entry points are fine only if they
  are thin delegations; growth beyond baseline signals logic re-accumulation).

The baselines are the current values at the time this ADR lands. They are not the *target* — the
target is a much smaller `lib.rs` — but freezing the current size guarantees it cannot grow while
the remaining logic is migrated into submodules. The baseline file may be lowered (never raised)
as logic is moved out; lowering it is a deliberate, reviewable change.

## Alternatives considered

- **`#![warn(clippy::...)]` only.** Rejected: clippy does not measure file size or entry-point
  count, so it cannot prevent gradual re-accumulation.
- **A hard line-count cap set to the eventual target (e.g. 500 lines).** Rejected today: it would
  fail immediately since the split is not complete. The frozen-from-here model is the practical
  first step and is tightened over time.

## Consequences

- `lib.rs` cannot grow; any PR that adds logic to it must move that logic into a submodule, or it
  fails CI.
- The guard is intentionally coarse (size + entry-point count). It catches accumulation without
  dictating module structure, leaving submodule organization to review.
- Lowering `.librs-baseline` as logic is extracted becomes the visible metric of split progress.
