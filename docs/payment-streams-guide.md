# Payment Streams Guide

This guide explains how to model continuous payouts with FluxaPay payment streams. A stream is a time-based transfer where a sender funds a deposit up front and the contract releases tokens to a receiver at a fixed `rate_per_second` until the deposit is exhausted or the stream is cancelled.

This is useful when you need:

- payroll or contractor payouts over time
- escrowed milestone settlements
- recurring payouts with a capped funding deposit
- delayed or staged delivery that is paid as work progresses

For the exact on-chain behavior, see the stream implementation in [../fluxapay/src/stream.rs](../fluxapay/src/stream.rs) and the SDK wrappers in [../sdk/src/index.ts](../sdk/src/index.ts).

---

## 1) Stream model and lifecycle

A stream is represented by a `PaymentStream` record with these core fields:

- `stream_id` — unique stream identifier
- `sender` — address that funded the stream
- `receiver` — address receiving streamed payments
- `destination` — optional fixed withdrawal destination
- `token` — token contract address
- `rate_per_second` — current flow rate
- `min_rate_per_second` — low-water floor for rate changes
- `remaining_deposit` — unused deposit remaining in the contract
- `last_checkpoint_at` — timestamp of the last accrual checkpoint
- `accrued_at_checkpoint` — cumulative amount already credited to the receiver
- `status` — `Active`, `Paused`, `Cancelled`, or `Exhausted`
- `milestones_approved` — whether distributions are unlocked

A stream is active as long as the sender has not cancelled it and the deposit has not been fully drained. The contract computes accrued value lazily at the time of each read or state mutation, so the current payout is based on:

```text
accrued = accrued_at_checkpoint + (now - last_checkpoint_at) * rate_per_second
```

with a hard clamp so it never exceeds the remaining deposit.

---

## 2) Create a stream

The sender creates a stream by transferring a deposit into the contract and choosing a rate. The rate must be positive and the deposit must be positive.

### TypeScript SDK

```typescript
const stream = await client.createStream({
  sender: "G_SENDER...",
  receiver: "G_RECEIVER...",
  token: "C_USDC_TOKEN...",
  ratePerSecond: 100n,
  deposit: 1_000_000n,
  streamId: "stream_001",
});
```

This maps to `PaymentProcessor.create_stream`, which stores the stream, appends it to the sender and receiver indexes, transfers the deposit into the contract, and emits a `STREAM/CREATED` event.

### Soroban CLI

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $SENDER_SECRET \
  -- create_stream \
  --sender $SENDER_ADDRESS \
  --receiver $RECEIVER_ADDRESS \
  --token $USDC_TOKEN_ADDRESS \
  --rate_per_second 100 \
  --deposit 1000000 \
  --stream_id "stream_001"
```

The sender may also pass `min_rate` if they want to set a floor before the rate is lowered.

---

## 3) Withdrawals and milestone gates

A stream receiver accrues tokens over time, but funds are not automatically sent except through explicit withdrawal flows.

### Milestone approval

The sender can lock or unlock withdrawals with:

- `approve_stream_milestone`
- `revoke_stream_milestone`

The `milestones_approved` field controls whether distributions are allowed. Until a sender approves milestones, recipient attempts to withdraw are ignored by the contract and return a `MilestoneNotApproved` error.

This is useful when the sender wants to grant payouts only after a milestone is accepted.

### Receiver withdrawal

A recipient can withdraw currently accrued funds via `withdraw_all_for_recipient` or a permissionless route against a configured destination.

#### Receiver-driven bulk withdrawal

```typescript
await client.withdrawStream("G_RECEIVER...", "stream_001");
```

This calls the underlying `batch_withdraw_to` flow for a single entry, and the contract will withdraw the total accrued amount up to the current timestamp, then update the stream's state.

#### Permissionless withdrawal to a fixed destination

The receiver may configure a destination for the stream:

```typescript
await client.setStreamDestination("G_RECEIVER...", "stream_001", "G_DESTINATION...");
```

Then anyone can trigger a withdrawal if the stream is active and the milestone gate is unlocked:

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $ANY_ACCOUNT \
  -- trigger_withdrawal \
  --stream_id "stream_001"
```

The destination must already be set, or the call fails with `DestinationNotSet`.

### Withdrawal semantics

On every withdrawal, the contract:

1. checkpoints accrued amount at the current timestamp,
2. computes the withdrawable amount,
3. subtracts that amount from `accrued_at_checkpoint` and `remaining_deposit`,
4. marks the stream as `Exhausted` if the deposit reaches zero,
5. applies the configured stream fee, and
6. emits `STREAM/WITHDRAWN`.

The fee configuration is admin-controlled by:

- `set_stream_fee_bps`
- `set_stream_fee_recipient`

The applied fee formula is:

```text
fee = amount * fee_bps / 10000
net = amount - fee
```

---

## 4) Top-ups and rate adjustments

### Top up

Only the original stream `sender` may add funds to a stream. `top_up_deposit` calls `env.require_auth(&stream.sender)`, so any call from a different address fails with an authorization error. This prevents a third party from forcibly extending a stream (or draining a custodial wallet) without the sender's consent.

The sender may add more funds to an active stream:

```typescript
await client.topUpStream("G_SENDER...", "stream_001", 500_000n);
```

This adds more deposit without altering the rate. The extra deposit is transferred from the sender into the contract and emits `STREAM/TOPPED_UP`.

If a third party needs to fund a stream on the sender's behalf, they must use `top_up_on_behalf`, which requires explicit authorization from the stream `sender` (the sender signs the authorization even though the caller supplies the funds). A call without that sender authorization is rejected.

### Decrease a rate

The sender may reduce the streaming rate, but only to a strictly smaller value and not below the configured minimum floor.

```typescript
await client.updateStreamRate("G_SENDER...", "stream_001", 50n);
```

The contract checkpoints accrued value before changing the rate, then refunds any surplus deposit that no longer needs to be reserved at the lower flow rate.

This refund is calculated using the old rate and the time remaining at the old rate, then subtracting the amount the new rate would require for the same time span.

The dedicated helper `decrease_rate_per_second` performs the same logic while enforcing the stricter rule that the new rate must be lower than the current rate.

### Increase a rate

`update_stream_rate` also supports rate increases without requiring a deposit top-up. The contract checkpoints the old accrual, updates the new rate, and continues streaming at the higher rate using the existing deposit.

---

## 5) Pause, resume, and cancellation

### Pause a stream

A sender can pause an active stream:

```typescript
await client.pauseStream("G_SENDER...", "stream_001");
```

The contract checkpoints accrued value, sets the status to `Paused`, and freezes accrual until resumed.

### Resume a stream

```typescript
await client.resumeStream("G_SENDER...", "stream_001");
```

This changes the status back to `Active` and resets the checkpoint to the current timestamp so accrual resumes from that moment.

### Cancel a stream

```typescript
await client.cancelStream("G_SENDER...", "stream_001");
```

Cancellation checkpoints accrued value, sends the accrued amount to the receiver, refunds the remaining unaccrued deposit to the sender, and sets the stream status to `Cancelled`.

In the contract, the `remaining_deposit` field is reduced to the accrued portion only, and the refund is computed as:

```text
refund = remaining_deposit - accrued
```

This ensures the receiver keeps what has already been earned while the sender gets back the unspent leftover.

---

## 5.1) Multi-payee streams (issue #831)

Payroll and revenue-share flows can fund **one** stream that splits accrual across
up to **10** weighted payees. All `share_bps` values must sum to exactly **10_000**
(100%). Each `withdraw_multi_stream` call distributes the currently accrued amount
proportionally to every payee in a single atomic transaction.

### Create

```typescript
// Soroban / contract call shape
create_multi_stream(
  sender,
  token,
  deposit,          // i128
  rate_per_second,  // i128
  [
    { address: payeeA, share_bps: 6000 }, // 60%
    { address: payeeB, share_bps: 4000 }, // 40%
  ],
); // → stream_id: String
```

### Withdraw

```typescript
withdraw_multi_stream(stream_id);
// Payee A receives 60% of accrued, Payee B receives 40%.
// Integer dust goes to the last payee so the full net amount is distributed.
```

Constraints enforced on-chain:

| Rule | Error |
|------|-------|
| `payees` empty | `EmptyPayees` |
| more than 10 payees | `TooManyPayees` |
| `share_bps` sum ≠ 10_000 (or any share is 0) | `InvalidPayeeShares` |

The existing single-payee `create_stream` / withdraw API is unchanged.

---

## 6) Operational guidance for production

### Recommended merchant flow

1. Create a stream with a generous but bounded deposit.
2. Configure a fixed destination if your app wants permissionless trigger flows.
3. Require milestone approval before sending payouts for contractor-heavy work.
4. Keep a rate floor via `min_rate_per_second` so a stream cannot be throttled below a business-safe minimum.
5. Monitor accrual and balance changes through the contract state and emitted stream events.
6. Top up
