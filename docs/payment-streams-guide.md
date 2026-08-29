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

The sender may add more funds to an active stream:

```typescript
await client.topUpStream("G_SENDER...", "stream_001", 500_000n);
```

This adds more deposit without altering the rate. The extra deposit is transferred from the sender into the contract and emits `STREAM/TOPPED_UP`.

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

## 6) Operational guidance for production

### Recommended merchant flow

1. Create a stream with a generous but bounded deposit.
2. Configure a fixed destination if your app wants permissionless trigger flows.
3. Require milestone approval before sending payouts for contractor-heavy work.
4. Keep a rate floor via `min_rate_per_second` so a stream cannot be throttled below a business-safe minimum.
5. Monitor accrual and balance changes through the contract state and emitted stream events.
6. Top up before depletion if you want continuity; otherwise the stream becomes `Exhausted`.

### Event coverage

The stream contract emits these high-value events:

- `STREAM/CREATED`
- `STREAM/DESTINATION_SET`
- `STREAM/MILESTONE_APPROVED`
- `STREAM/MILESTONE_REVOKED`
- `STREAM/RATE_DECREASED`
- `STREAM/WITHDRAWN`
- `STREAM/TOPPED_UP`
- `STREAM/CANCELLED`
- `STREAM/PAUSED`
- `STREAM/RESUMED`
- `STREAM/RATE_UPDATED`

These are the primary signals for dashboards, webhooks, or off-chain settlement systems.

---

## 7) Example end-to-end flow

```typescript
const stream = await client.createStream({
  sender: "G_SENDER...",
  receiver: "G_RECEIVER...",
  token: "C_USDC_TOKEN...",
  ratePerSecond: 100n,
  deposit: 5_000_000n,
  streamId: "project_milestone_001",
});

await client.setStreamDestination("G_RECEIVER...", "project_milestone_001", "G_DESTINATION...");
await client.approveStreamMilestone("G_SENDER...", "project_milestone_001");

await client.withdrawStream("G_RECEIVER...", "project_milestone_001");

await client.topUpStream("G_SENDER...", "project_milestone_001", 1_000_000n);
await client.updateStreamRate("G_SENDER...", "project_milestone_001", 80n);

await client.pauseStream("G_SENDER...", "project_milestone_001");
await client.resumeStream("G_SENDER...", "project_milestone_001");

await client.cancelStream("G_SENDER...", "project_milestone_001");
```

This pattern is well suited for milestone-driven payouts, contractor work, and payroll schedules where the total payout is known up front but the actual release should happen over time.

---

## 8) Related docs

- [events.md](events.md) for the event catalog
- [subscription-guide.md](subscription-guide.md) for recurring billing plans
- [dispute-resolution-guide.md](dispute-resolution-guide.md) for dispute handling
- [../sdk/README.md](../sdk/README.md) for SDK and client examples
