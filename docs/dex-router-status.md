# DEX router: simulation status and limitations

> **Status: simulation only. Not production-safe.**
>
> The bundled `DexRouter` contract (`fluxapay/src/dex_router.rs`) is a stand-in for a real
> Soroswap-style router. Several of its functions perform **no on-chain token transfers at
> all** and compute outputs with a hardcoded formula instead of reading real pool reserves.
> Deploying it as the live router behind `swap_and_pay` would let callers "swap" without
> moving any tokens, or receive quotes that don't reflect real liquidity. Each gap below has
> an open issue; do not treat this file as a substitute for fixing them.

## How swap_and_pay actually uses this

`swap_and_pay` / `swap_and_pay_multi_route` (`fluxapay/src/lib.rs`) don't hardcode `DexRouter` —
they call whatever router address the caller passes in `args.dex_router`, gated by
`get_allowed_routers` / `is_router_allowed` (`DataKey::AllowedRoutersList`, issue #437). The
surrounding checks in `swap_and_pay` itself — the router allowlist, oracle price-deviation
validation (`validate_oracle_swap_rate`), price-impact guard, per-ledger volume cap, and
circular-path/arbitrage check in `validate_path_returns` — are real, already-enforced logic,
**not** simulated. The simulation problem is specifically in the reference `DexRouter`
contract that ships in this crate and is the only router most test/dev deployments actually
allowlist today.

## Per-function limitations

| Function | Behavior today | Limitation | Tracking issue |
|---|---|---|---|
| `factory()` | Returns `env.current_contract_address()` | Placeholder; a real router's `factory()` returns the Soroswap factory contract address used to look up pools. Callers cannot use this to discover real pools. | [#1145](https://github.com/MetroLogic/fluxapay_contract/issues/1145) |
| `get_amounts_out(amount_in, path)` | Applies a hardcoded 1% haircut per hop (`current * 99 / 100`) | Does not query any on-chain pool for real reserves/price. Quotes are the same regardless of actual liquidity, pool depth, or fees. | [#1144](https://github.com/MetroLogic/fluxapay_contract/issues/1144) |
| `get_amounts_in(amount_out, path)` | Inverts the same synthetic 1%-per-hop formula (`current * 100 / 99`) | Same root cause as `get_amounts_out` — a synthetic formula, not a real reverse quote. | tracked under [#1146](https://github.com/MetroLogic/fluxapay_contract/issues/1146) (consumer of this quote) |
| `check_liquidity(path)` | Always returns `true` | Never actually checks that a pool/path has liquidity. A path with zero real liquidity still passes this check. | [#1147](https://github.com/MetroLogic/fluxapay_contract/issues/1147) |
| `execute_swap(caller, token_in, token_out, amount_in, min_amount_out, max_slippage_bps)` | Validates path/liquidity/slippage/price-impact using the simulated quote, then **publishes `SWAP/EXECUTED` without calling `token::Client::transfer` for either token** | No input is pulled from `caller` and no output is paid out — the event implies a completed swap that never happened. This is the most severe gap: an indexer or integrator trusting `SWAP/EXECUTED` would believe funds moved when they did not. | [#1142](https://github.com/MetroLogic/fluxapay_contract/issues/1142) (critical) |
| `swap_exact_tokens_for_tokens(...)` / `execute_swap_internal(...)` | Validates the path and slippage/price-impact against the simulated quote, then publishes `SWAP/EXECUTED` | Same gap as `execute_swap`: neither this function nor its internal helper calls a token transfer on the success path. This is the entry point `swap_and_pay` actually invokes via `DexRouterClient`, so a production deployment pointed at this contract would create a `PaymentCharge` believing a swap settled when no tokens moved. Not yet tracked by its own issue — treat it as the same class of bug as #1142 until filed. | related to [#1142](https://github.com/MetroLogic/fluxapay_contract/issues/1142) |
| `swap_tokens_for_exact_tokens(...)` | Computes a required input via `get_amounts_in`'s synthetic formula, refunds the unspent surplus via `refund_caller`, and returns the computed amounts | Returns computed/dummy amounts without executing any real swap or pulling the required input from the caller. | [#1146](https://github.com/MetroLogic/fluxapay_contract/issues/1146) |
| `refund_caller(recipient, amount)` | Publishes a `REFUND/CALLER` event with the recipient and amount | Never calls a token transfer — no tokens are actually returned to `recipient`. Any caller relying on the refund path (e.g. the fallback branch in `swap_exact_tokens_for_tokens`, or the overpayment refund in `swap_tokens_for_exact_tokens`) does not get funds back on-chain despite the event. | [#1143](https://github.com/MetroLogic/fluxapay_contract/issues/1143) |

## Related, already-filed follow-up work

- [#1177](https://github.com/MetroLogic/fluxapay_contract/issues/1177) — add an end-to-end `swap_and_pay` test using a mock router that actually moves tokens (`fluxapay/src/mock_dex_router.rs` already does real `token::Client::transfer` calls and is a better template for a real router than `dex_router.rs` itself).
- [#1172](https://github.com/MetroLogic/fluxapay_contract/issues/1172) — circuit breaker / emergency pause specifically for the `swap_and_pay` path, independent of the simulation gaps above.
- [#955](https://github.com/MetroLogic/fluxapay_contract/issues/955) — multi-asset routing with exact-output guarantees; depends on `swap_tokens_for_exact_tokens` actually executing (#1146).

## Planned path to a real router

The sequencing for replacing the simulation with real Soroswap integration — allowlist
hardening, WXLM wrapping, oracle validation, slippage guards, then the actual transfer path
and multi-route aggregation — is coordinated in
[ADR-0005](ADR-0005-dex-simulation-to-real-migration.md). That ADR's proposed
`DataKey::DexSimulationMode` toggle is a **design, not yet implemented** — there is no such
storage flag in the code today; the only existing compile-time toggle is the
`contract-dex-router` Cargo feature gating `DexRouter`'s `#[contractimpl]` block.

## Bottom line for integrators

Do not point `swap_and_pay` at the bundled `DexRouter` in any environment where real value is
at stake. It is useful for unit tests that only need a `DexRouterClient`-shaped contract to
call, and nothing else. For integration tests that need tokens to actually move, use
`fluxapay/src/mock_dex_router.rs` instead (see #1177), and track the issues above before
considering `dex_router.rs` for a production allowlist entry.
