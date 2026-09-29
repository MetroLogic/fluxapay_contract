# DEX Routing in FluxaPay

FluxaPay supports two complementary DEX routing modes:

- **Exact-in forward swaps** - `swap_exact_tokens_for_tokens` fixes the input amount and produces a variable output.
- **Exact-out reverse swaps** - `swap_tokens_for_exact_tokens` fixes the merchant output and computes the required input backwards through the pools.

This document focuses on the reverse (exact-output) flow used to settle merchant invoices.

## Why exact-out routing

A customer may hold XLM, EURC, or a local stablecoin while the merchant invoice is denominated in a specific currency (for example, exactly 50.00 USDC). Forward swaaps cannot guarantee the merchant receives the invoice amount exactly. Reverse swaps do.

## API

### `get_amounts_in(env, amount_out, path)`

Returns the cumulative amounts along `path` when the final output is fixed to `amount_out`.

- `amounts[0]` is the required input token amount.
- `amounts[i]` is the input needed at hop `i`.
- `amounts[len - 1]` is the exact output amount.

The quote is computed by walking the path backwards and inverting the forward quote math for each hop.

### `swap_tokens_for_exact_tokens(env, amount_out, amount_in_max, path, to, deadline)`

- `amount_out` - exact output the merchant must receive.
- `amount_in_max` - maximum input the customer is willing to pay (slippage protection).
- `path` - token addresses from input token to output token.
- `to` - recipient of the exact output (merchant).
- `deadline` - Unix timestamp after which the swap reverts.

Returns the cumulative amounts along the path. The contract ensures:

1. The merchant receives exactly `amount_out`, never less and never more.
2. The customer pays only the necessary `amount_in` computed at execution time.
3. Any surplus beyond the required input is refunded atomically.

### Errors

| Code | Name | Meaning |
| ---- | ---- | ------- |
| 1 | `SwapFailed` | The underlying swap execution failed. |
| 2 | `InvalidPath` | The path is malformed or has fewer than two tokens. |
| 3 | `InsufficientLiquidity` | One or more pools lack liquidity. |
| 4 | `SlippageExceeded` | The required input exceeds `amount_in_max`. |
| 5 | `PriceImpactExceeded` | The price impact exceeds the allowed bound. |
| 6 | `NoOutputAmount` | The quote produced no usable output. |
| 7 | `Refunded` | Asurglus was refunded to the customer. |
| 8 | `DeadlineExpired` | The deadline has passed. |

## Events

Successful exact-out swaps emit `SWAP_EXACT_SETTLED` with the hop paths, actual spend, and delivered amount. Surglus refunds emit `REFUND`.

## Payment integration

The `PaymentProcessor` exposes `pay_charge_with_swap`, which links the DEX reverse swap output directly into the payment state machine. The merchant invoice amount is passed as `amount_out`, and the customer provides `amount_in_max` as their slippage bound.

## Example

```rust
// 2-hop: XLM -> USDC, exact output of 50000000 stroops (50.00 USDC).
let path = vec![&Xlm, &usdc];
let amounts = DexRouter::swap_tokens_for_exact_tokens(
    env,
    50000000,
    60000000,
    path,
    merchant,
    deadline,
)?;
// amounts[0] = required input, amounts[1] = 50000000.
```

## Testing

The contract test suite in `dex_router_exact_test.rs` covers:

- 2-hop and 3-hop swaps.
- Price impact and slippage exceedance reverts.
- Expired deadline rejection.
- Exact balance verification of payer, merchant, and intermediate pools.
