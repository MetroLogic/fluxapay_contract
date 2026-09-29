# DEX Routing in FluxaPay

This document describes how the FluxaPay payment processor settles merchant invoices in an exact settlement currency using the on-chain DEX router.

## Overview

Merchants specify an invoice amount in their primary settlement currency (for example, exactly 50.00 USDC). Customers however may hold diverse Stellar assets (XLM, EURC, local stablecoins). To bridge this gap, the payment processor invokes the DEX router's reverse routing entrypoint, which fixes the merchant output amount and computes the required input amount backwards through the on-chain AMM pools.

## Forward vs Reverse Routing

The router supports two directions of quote math:

- **Forward routing** (`swap_exact_tokens_for_tokens`): the input amount is fixed and the output amount is variable. This is useful when a customer knows how much they want to spend.
- **Reverse routing** (`swap_tokens_for_exact_tokens`): the output amount is fixed and the required input is computed backwards. This is useful when a merchant must receive an exact invoice amount.

## Reverse Quote Math

For a single hop with constant-product AMB reserves `reserve_in` and `reserve_out`, and a per-hop fee of `HOP_FEE_BPS` (0.3%), the output for a given input is:

```text
amount_out = (amount_in * (10000 - fee) * reserve_out) / (reserve_in * 10000 + amount_in * (10000 - fee))
```

The reverse quote inverts this formula to compute the required input for an exact output:

```text
amount_in = ceil((reserve_in * amount_out * 10000) / ((reserve_out - amount_out) * (10000 - fee)))
```

The ceiling is important: it guarantees that the exact output is always delivered and never falls short due to integer truncation.

For multi-hop paths (for example `XLM -> USDC -> EURO`), the reverse quote is applied hop by hop from the output side back to the input side. The resulting array is then reversed to produce forward-ordered amounts, where `amounts[0]` is the required input and `amounts[last]` is the exact output.

## API

### `get_amounts_in(env, amount_out, path) -> Vec<i128>`
Returns the forward-ordered amounts array for a reverse quote. The first element is the required input and the last element is the exact output. The math queries the on-chain pool reserves for each hop via `get_reserves`.

### `swap_tokens_for_exact_tokens(env, amount_out, amount_in_max, path, to, deadline)
Executes a reverse swap with the following guarantees:

- **Exact output delivery**: the merchant receives exactly `amount_out`, never less and never more.
- **Slippage protection**: if the computed required input exceeds `amount_in_max`, the call reverts with `SlippageExceeded`.
- **Surplus protection**: the customer pays only the necessary `amount_in` computed at execution time. Any excess beyond the computed input is refunded atomically.
- **Deadline enforcement**: if the ledger timestamp exceeds `deadline`, the call reverts with `DeadlineExpired`.
- **Price impact guard**: the effective execution ratio must not deviate by more than 5% (500 basis points).

## Events

The router emits a `SWAP_EXACT_SETTLED` event on every successful reverse swap. The event payload contains:

- The hop path (`Vec<Address>`)
- The actual input spent (`required_in`)
- The exact output delivered (`amount_out`)
- The recipient address (`to`)
- The ledger timestamp

This event is consumed by the payment processor to link the swap output directly into the payment state machine.

## Payment Processor Integration

The `pay_charge_with_swap` endpoint in `PaymentProcessor` links the DEX swap output directly into the payment state machine. The flow is:

1. The merchant creates a charge with an exact invoice amount in their settlement currency.
2. The customer submits a payment in a different asset along with a `max_amount_in` slippage bound.
3. The payment processor invokes `swap_tokens_for_exact_tokens` on the DEX router with the exact invoice amount as the output.
4. The router delivers the exact output to the merchant and refunds any surplus to the customer.
5. The payment state machine transitions to `Settled` using the amounts returned by the router.

## Error Codes

| Code | Name | Description |
|------|------|-------------|
| 1 | SwapFailed | The underlying swap failed for an unspecified reason. |
| 2 | InvalidPath | The path is empty, too short, or contains adjacent duplicate tokens. |
| 3 | InsufficientLiquidity | The pool reserves cannot support the requested output. |
| 4 | SlippageExceeded | The required input exceeds `max_amount_in`. |
| 5 | PriceImpactExceeded | The effective execution ratio deviates by more than 5%. |
| 6 | NoOutputAmount | The quote produced no valid output amount. |
| 7 | Refunded | An atomic refund was issued to the caller. |
| 8 | DeadlineExpired | The ledger timestamp exceeded the deadline. |
| 9 | InvalidAmount | An amount parameter was zero or negative. |

## Testing

The exact-output routing is covered by `fluxapay/src/dex_router_exact_test.rs`, which includes:

- 2-hop (XLM -> USDC) and 3-hop (BTC -> XLM -> USDC) swaps.
- Price impact and slippage exceedance reverts.
- Expired deadline rejection.
- Exact balance verification of payer, merchant, and intermediate pools.
- Event emission verification for `SWAP_EXACT_SETTLED`.
