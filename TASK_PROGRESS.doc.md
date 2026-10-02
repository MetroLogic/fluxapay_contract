# Fluxapay Contract — Task Progress (Decentralized Milestones)

## Current status

Repository investigation completed for the following milestone items:

- **[76/90] XLM Native Routing Wrappers**
  - Checked swap flow (`PaymentProcessor::swap_and_pay`) and DEX router wrapper (`dex_router.rs`).
  - **Result:** No implementation found for automatic XLM → WXLM wrapping; implementing it requires code/state changes.

- **[78/90] Multi-DEX Aggregation**
  - Inspected swap execution interfaces and call sites.
  - **Result:** No multi-router, multi-path aggregation/splitting support found; implementing it requires code changes.

- **[69/90] Decentralized Directory Governance**
  - Inspected merchant directory governance (`merchant_registry.rs`).
  - **Result:** Merchant suspension exists but is **admin-only**; no stakeholder voting-to-suspend mechanism found.

- **[72/90] DEX Router Registry**
  - Inspected swap execution and DEX router wrapper.
  - **Result:** No allowlist/registry for approved router addresses; `swap_and_pay` uses `args.dex_router` directly.

## Important constraint conflicts

Multiple milestones’ acceptance criteria require adding or changing contract logic.

- User constraints stated in the conversation included:
  - “without make any change to the previous code”
  - “don’t run any test”

Given those constraints, acceptance criteria that require new functionality could not be implemented.

## Repo observations (where the behavior lives)

- `fluxapay/src/lib.rs`
  - `PaymentProcessor::swap_and_pay` is the swap entrypoint; it uses `args.dex_router` and `args.path` directly.
- `fluxapay/src/dex_router.rs`
  - Provides a simple DEX router wrapper interface; no registry/allowlist.
- `fluxapay/src/merchant_registry.rs`
  - Contains merchant suspension/reinstatement and related metadata.

## Next steps (if constraints are relaxed)

If you approve code changes for the desired milestones, the following is typically required:

1. Add contract storage for router allowlist/registry (for milestone **[72/90]**).
2. Add admin entrypoints to add/remove/update approved router addresses.
3. Validate `args.dex_router` against allowlist inside `swap_and_pay`.
4. Add unit tests for allowlist behavior (without running them automatically).

## Notes

- No code changes were applied during this investigation.
- No tests were executed.
