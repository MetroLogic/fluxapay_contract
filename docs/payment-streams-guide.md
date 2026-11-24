# Payment Streams Guide

FluxaPay payment streams lock a token deposit in the streaming contract and
release accrued amounts to the receiver as time passes. Accrual is calculated
lazily from the ledger timestamp; there is no recurring transaction.

## Create a regular stream

The SDK `createStream` method creates a stream without a vesting cliff:

```ts
const stream = await fluxapay.createStream({
  sender,
  receiver,
  token,
  ratePerSecond: 10n,
  deposit: 5_000n,
  streamId: "grant-2026-01",
});
```

The receiver must set a withdrawal destination, and the sender must approve the
stream milestone before withdrawals can be triggered. The stream stops accruing
when its deposit is exhausted. See [`stream.rs`](../fluxapay/src/stream.rs) for
the contract behavior.

## Create a vesting stream with a cliff

Set `cliff_at` to a Unix timestamp in seconds. Until the Soroban ledger timestamp
reaches this value, `get_accrued_amount` reports zero and withdrawal entry points
return `CliffNotReached`. At the cliff, the amount accrued from the stream start
is released on the first successful withdrawal; accrual then continues at the
configured rate.

```rust
let cliff_at = env.ledger().timestamp() + 365 * 24 * 60 * 60;
let stream = client.create_vesting_stream(
    &sender,
    &receiver,
    &token,
    &10i128,
    &1_000_000_000i128,
    &String::from_str(&env, "employee-grant-2026"),
    &cliff_at,
);
```

`PaymentProcessor::create_vesting_stream` is the cliff-enabled entry point;
existing `create_stream` calls remain unchanged. Callers using the standalone
streaming contract can use `PaymentStreaming::create_stream_with_cliff`
directly, which also accepts the existing optional minimum-rate argument.

The contract emits `STREAM/CLIFF_REACHED` with the stream ID and withdrawal
timestamp once, when the first post-cliff withdrawal succeeds. If the sender
cancels a cliff stream before its cliff, the unvested deposit is returned to
the sender and no accrued amount is paid to the receiver. A cancellation after
the cliff follows the normal accrued-amount payout behavior.
