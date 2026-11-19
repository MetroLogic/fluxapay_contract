# ADR-0005: DEX Simulation-to-Real (Soroswap) Integration Migration

- Status: Accepted
- Date: 2026-11-19
- Related issues: #1226 (this ADR), #173, #303, #304, #434, #436, #437, #856, #1033

## Context

FluxaPay currently ships a **simulated** DEX router (`fluxapay/src/dex_router.rs`).
`get_amounts_out` applies a hardcoded 1% per-hop haircut and `swap_exact_tokens_for_tokens`
moves tokens locally rather than routing through an on-chain liquidity source (see the
`TODO: delegate token transfers to the real Soroswap router contract` marker). Swaps are
selected via `SwapAndPayArgs` / `SwapRoute` and an admin allowlist (`AllowedRoutersList`).

Swaps are not isolated to one file. Real Soroswap integration touches several concerns that
each have their own open issue, and they must land in a consistent order or the contract will
ship a half-migrated router (real quotes behind a simulated transfer path, or vice versa). This
ADR coordinates those issues so they land as one migration.

## Decision

Adopt a **simulation/production toggle** plus a **sequenced migration** that moves each concern
from simulated to real Soroswap in dependency order, with the toggle gating the live path.

### Simulation vs production toggle

The build feature `contract-dex-router` (in `fluxapay/Cargo.toml`) and the
`#[cfg_attr(any(not(target_arch = "wasm32"), feature = "contract-dex-router"), contractimpl)]`
guard on the `DexRouter` impl already provide a compile-time toggle: the router impl is compiled
for native/test builds but is excluded from the default `contract-payment-processor` WASM build.
The migration keeps this as the canonical toggle:

- **Simulation mode (default, today):** the local `DexRouter` simulates quotes and transfers.
  Used in tests and as the fallback until every step below is verified on testnet.
- **Production mode:** the router is either replaced by an external Soroswap router address
  (configured via the existing `DexRouterAddress` / `AllowedRoutersList` allowlist) or recompiled
  with `--features contract-dex-router` once its internals delegate to Soroswap.

A single runtime/storage flag (`DataKey::DexSimulationMode: bool`, default `true`) is added so
operators can disable simulation without a redeploy. When `false`, `swap_exact_tokens_for_tokens`
MUST delegate to the configured Soroswap router and reject if none is configured/allowlisted.

### Migration sequence (issue → step)

| Step | Issue | Concern | Migration action | Toggle requirement |
|------|-------|---------|------------------|--------------------|
| 1 | #437 | Router allowlist | Allowlist enforcement already exists; extend it to require the configured Soroswap router address be present in `AllowedRoutersList` before `DexSimulationMode = false` is accepted. | simulation ok |
| 2 | #434 | XLM wrapping (WXLM) | `WrappedXlmContract` storage exists. Wire `swap_*` to wrap native XLM into WXLM (and unwrap on the refund path) via the configured WXLM token contract. | simulation ok |
| 3 | #303 / #304 / #1033 | FX oracle rate validation | `swap_and_pay` already sanitizes quotes against `FXOracle`. Keep the oracle as the price truth source; real Soroswap quotes that deviate beyond `MaxRateDeviation` / `OracleStalenessThreshold` are rejected unchanged. | simulation ok |
| 4 | #856 | Slippage / price-impact guards | `execute_swap` already caps `max_slippage_bps ≤ 5000` and `price_impact_guard` rejects > 500 bps. Keep these guards wrapping the real router call. | simulation ok |
| 5 | #173 | Routing + token transfers + refund path | Replace the simulated `swap_exact_tokens_for_tokens` transfer with a call to the allowlisted Soroswap router, and record `original_token` / `swap_path` so refund routing (#173) can reverse the hop. | requires #437, #434 |
| 6 | #436 | Multi-route aggregation | `SwapRoute` / route splitting must compose real router calls and validate aggregate output ≥ `amount_out_min`. | requires #173 |

Steps 1–4 are additive and safe to land while `DexSimulationMode = true`. Steps 5–6 flip the
transfer path to the real router and must land together behind `DexSimulationMode = false`.

## Alternatives considered

- **Big-bang rewrite of `dex_router.rs`.** Rejected: it bundles the allowlist, wrapping, oracle,
  slippage, and transfer concerns into one unreviewable change and offers no rollback path.
- **A second, parallel `RealDexRouter` contract.** Rejected: it duplicates routing/refund-path
  logic and forces `swap_and_pay` to branch on which router to call. The feature + runtime toggle
  keeps a single router surface.

## Consequences

- `DexSimulationMode` becomes a deployed, admin-gated configuration that operators flip only after
  testnet verification of steps 5–6.
- Indexers and SDK callers see no topic/ABI change; the `swap_and_pay` entry point signature is
  unchanged — only the execution path behind it changes.
- Each open DEX issue (#437, #434, #303/#304/#1033, #856, #173, #436) can be closed against its
  step in this sequence rather than as an isolated change.

## Security notes

- The allowlist (#437) is the trust root: only allowlisted Soroswap router addresses may receive
  token transfers in production mode. `DexSimulationMode = false` with an empty allowlist must fail
  closed (reject swaps), never fall back to simulation.
- WXLM wrapping (#434) must use the configured `WrappedXlmContract`; an unset value must reject
  XLM-involving swaps rather than skipping the wrap.
- Slippage/impact guards (#856) and oracle validation (#303/#304/#1033) are non-optional wrappers
  around the real router call and must not be bypassed in production mode.
