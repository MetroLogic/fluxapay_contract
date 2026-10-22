# DEX Routing

`fluxapay/src/dex_router.rs` routes multi-asset payments through on-chain AMM pools. Two swap modes are supported:

- **Forward swaps** (`swap_exact_tokens_for_tokens`): the input amount is fixed and the output varies.
- **Reverse swaps** (`swap_tokens_for_exact_tokens`): the merchant output amount is fixed and the required input is computed backwards through the pools. This is the mode used to settle merchant charges where the invoice amount is exact (e.g. exactly 50.00 USDC).

## Reverse quote math (`get_amounts_in`)

Given a `path` of token addresses and a desired `amount_out`, `get_amounts_in` walks the path backwards, querying each pool's reserves and applying the constant-product formula with the pool fee:

```
For each hop (token_in -> token_out) with reserves (R_in, R_out):
    amount_in = ceil( R_in * amount_out * FEE_DENOMINATOR
                      / ((R_out - amount_out) * (FEE_DENOMINATOR - FEE_NUMERATOR)) )
```

Rounding is always up so the computed input is sufficient to deliver the exact output. The result is an array of amounts, one per hop, with the last element equal to `amount_out`.

## Exact-output swaps (`swap_tokens_for_exact_tokens`)

```rust
swap_tokens_for_exact_tokens(
    payer: Address,
    amount_out: i128,
    max_amount_in: i128,
    path: Vec<Address>,
    deadline: u64,
)
```

Guarantees:

- **Exact output delivery** — the merchant receives exactly `amount_out`, never less and never more.
- **Surplus protection** — the required `amount_in` is recomputed at execution time from live reserves. The payer is charged only that amount; any surplus the payer pre-approved or transferred is refunded atomically within the same transaction.
- **Slippage protection** — if the recomputed `amount_in` exceeds `max_amount_in`, the call reverts.
- **Deadline** — if `ledger.timestamp() > deadline`, the call reverts.

## Payment integration (`pay_charge_with_swap`)

`PaymentProcessor::pay_charge_with_swap` links the DEX swap output directly into the payment state machine: the swap delivers the merchant's settlement asset, and the delivered amount is applied to the charge. On success a `SWAP_EXACT_SETTLED` event is emitted containing the hop paths, the actual amount spent by the payer, and the amount delivered to the merchant.

## Testing

`dex_router_exact_test.rs` covers:

- 2-hop swaps (e.g. XLM -> USDC) and 3-hop swaps (e.g. BTC -> XLM -> USDC).
- Price impact and slippage exceedance reverts.
- Expired deadline rejection.
- Exact balance verification of payer, merchant, and intermediate pools.
