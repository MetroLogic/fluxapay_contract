# ADR-0002: Payment Stream Design

- Status: Accepted
- Date: 2026-04-15

## Context

`stream.rs` implements continuous, per-second token streaming from a
`sender` to a `receiver` (`PaymentStream`), independent of the discrete
`create_payment` / `verify_payment` flow in `lib.rs`. As a standalone,
sizeable module, its core design decisions weren't previously written down
anywhere, leaving new contributors and auditors to reverse-engineer the
rationale from the code itself. This ADR documents those decisions.

## Decision 1: Lazy accrual, not eager

Accrual is computed on demand via a pure function rather than updated by a
recurring on-chain job:

```rust
// total = min(deposit, checkpoint + (now - last_checkpoint) * rate)
pub fn compute_total_accrued(
    accrued_at_checkpoint: i128,
    last_checkpoint_at: u64,
    now: u64,
    rate_per_second: i128,
    remaining_deposit: i128,
) -> i128
```

`PaymentStream` stores only `accrued_at_checkpoint` and
`last_checkpoint_at`; the amount accrued since the last checkpoint is
derived at read/withdraw time, never written to storage until a state
change (withdrawal, rate change, pause/resume, cancel) forces a checkpoint.

### Rationale

- **Ledger cost**: Soroban charges for storage writes, not for CPU spent on
  arithmetic. An eager model would need a per-stream, per-tick write (or a
  scheduled job iterating every active stream) to keep `accrued` current —
  that's O(active streams) storage writes per tick, most of which no one
  ever reads. The lazy model is O(1) storage writes per stream, only when
  something actually happens to that stream.
- **No scheduler dependency**: Soroban contracts can't run background jobs;
  an eager design would need an off-chain cron invoking every stream on a
  timer, which is both an availability dependency and an attack surface
  (a missed tick shouldn't cost the receiver money). Lazy accrual makes
  "amount owed right now" a pure function of ledger time, correct whether
  or not anyone has called the contract recently.
- **Saturating arithmetic, not checked**: `compute_total_accrued` uses
  `saturating_add`/`saturating_mul`/`saturating_sub` throughout instead of
  checked arithmetic that would panic on overflow. A payment stream must
  never brick (become permanently un-withdrawable) because a pathological
  `rate * elapsed` computation overflowed `i128`; saturating at `i128::MAX`
  and then clamping to `deposit` is safe because the result is always
  re-clamped to `[0, deposit]` immediately after. `proptest_stream_accrual_no_overflow`
  and `proptest_stream_accrual_monotonic` in `proptests.rs` fuzz this
  specifically.

### Trade-off accepted

Every withdrawal/rate-change/pause call pays the cost of recomputing
accrual from the last checkpoint — a handful of arithmetic ops, negligible
next to a storage write. In exchange, streams with zero activity between
checkpoints cost zero extra storage churn, and the design has no dependency
on an external clock/scheduler being reliably invoked.

## Decision 2: Milestone gating is sender-controlled, not a separate contract

`PaymentStream.milestones_approved: bool` lives directly on the stream
record. When `false`, withdrawals are blocked (checked at the top of the
withdraw path) until the **sender** calls `approve_milestones` to flip it
back to `true`. There is no separate "milestone gating contract" or
oracle-driven gate.

### Rationale

- **Streams already model a sender/receiver trust relationship.** The
  sender funded the deposit and is the only party authorized to change the
  rate (`decrease_rate_per_second`) or cancel the stream. Milestone
  approval is the same category of decision — "should money keep flowing
  to this receiver" — so it belongs with the other sender-only controls on
  the same record, not in a separate contract with its own access-control
  surface and cross-contract call overhead.
- **No new storage layout / cross-contract round-trip.** A separate gating
  contract would require an extra persistent lookup (and extra
  authorization plumbing) on every withdrawal just to ask "is this
  milestone approved?" — for a boolean that only the stream's own sender
  can ever set. Co-locating it on `PaymentStream` makes the check a single
  field read on data already being loaded for the withdrawal.
- **Consistent semantics with rate control.** Just as the sender can slow
  (never speed up) the flow rate unilaterally, the sender can pause
  withdrawals unilaterally by leaving `milestones_approved: false` after
  creation (new streams default it to `true`, i.e. unblocked, unless a
  workflow explicitly opts in to milestone-gating by setting it false and
  requiring explicit approval before the first withdrawal).

### Trade-off accepted

Because gating is sender-controlled, a receiver has no way to force
release of funds if a sender withholds milestone approval in bad faith
beyond whatever off-chain/dispute recourse exists outside this contract.
This is an intentional consequence of streams being sender-funded,
sender-authorized constructs — the same trust model already implied by
letting the sender decrease the rate or cancel the stream at will.

## Decision 3: Rate changes are unidirectional — decrease only

`decrease_rate_per_second` enforces, in order:

1. `new_rate > 0` (`InvalidRate` otherwise)
2. `new_rate < stream.rate_per_second` (`RateNotDecreased` otherwise — this
   also rejects `new_rate == rate_per_second`, a true no-op)
3. `new_rate >= stream.min_rate_per_second` (`RateBelowMinimum` otherwise)

There is no `increase_rate_per_second` function anywhere in the module.

### Security rationale

- **Bounding the receiver's exposure at creation time.** The receiver's
  maximum possible drain rate is fixed the moment the stream is created
  (`rate_per_second` at `create_stream`). If the sender could raise the
  rate later, a sender could under-fund a stream, let the receiver believe
  they're being paid at rate `R`, then spike the rate arbitrarily right
  before the deposit runs out — or grief a receiver's off-chain accounting
  (which typically assumes a monotonically non-increasing rate) by
  suddenly increasing flow. Making rate changes strictly decreasing means
  a receiver's worst case is exactly the rate they observed when the
  stream was created (or any lower rate that followed) — it can only get
  slower, never faster or reset upward, which is the direction that's safe
  for the *receiver*, not the sender.
- **Symmetric with "sender can always choose to spend less."** A sender
  who wants to pay *more* per second has a simple, safe alternative already
  available to them: cancel the stream and open a new one at the higher
  rate (or top up a second stream). That path goes through the normal
  `create_stream` validation (fresh `min_rate`, fresh deposit, fresh
  authorization) instead of silently mutating an existing receiver's
  expectations in place.

## Decision 4: `min_rate_per_second` as DoS protection

Every stream carries its own `min_rate_per_second` (defaulting to `1` if
not specified at creation), and `decrease_rate_per_second` rejects any new
rate below it with `RateBelowMinimum`.

### Rationale

Without a floor, a sender could repeatedly call
`decrease_rate_per_second` to ratchet the rate down toward (but never
exactly) zero — e.g. `1 -> 0` is rejected by `InvalidRate` (rate must stay
positive), but nothing else stops a sender from decreasing the rate in
tiny increments arbitrarily many times, each call being a full contract
invocation that does a storage read, a checkpoint computation, and a
storage write. `min_rate_per_second` bounds how far this can be pushed and
lets a stream's creator (or receiver's integration expectations) enforce
"this stream is meaningful, or it should be cancelled" — a rate that's
been driven to its floor is a signal the stream is effectively winding
down, not a way to spam cheap no-op-ish invocations indefinitely while
still nominally being "active." It also protects receivers who build
automation assuming a stream delivers at least some minimum throughput —
below that floor, the sender must cancel rather than leave a receiver
subscribed to a stream paying an economically meaningless trickle.

## Decision 5: Stream index storage schema

Streams are looked up directly by ID (`StreamDataKey::Stream(String) ->
PaymentStream`), but sender/receiver need to *enumerate* their own
streams without knowing IDs in advance. This is handled by a parallel,
append-only index:

```rust
#[contracttype]
pub enum StreamIndexKey {
    SenderStream(Address, u32),      // (sender, index) -> stream_id
    SenderStreamCount(Address),      // sender -> count
    RecipientStream(Address, u32),   // (receiver, index) -> stream_id
    RecipientStreamCount(Address),   // receiver -> count
}
```

On `create_stream`, the new `stream_id` is appended to both the sender's
and receiver's index (`append_sender_stream` / `append_recipient_stream`),
each writing at `count` and then incrementing the corresponding
`*StreamCount` key.

### Rationale

- **O(1) append, O(1) random access, no re-indexing.** A `Vec<String>`
  stored under a single key per address would need to be read and
  rewritten in full on every append — O(n) storage cost per new stream
  once an address has many streams. The `(Address, u32) -> stream_id`
  keyed scheme makes each append a single new key write plus a counter
  bump, independent of how many streams that address already has.
- **Separate sender/receiver indexes, not one shared index.** A sender and
  a receiver need different enumeration ("streams I'm paying out of" vs.
  "streams paying into me"), and a given address can be a sender on some
  streams and a receiver on others simultaneously. Two independent
  `(role, address, index)` key families avoid conflating those two very
  different queries into one structure that would need a `role` filter
  applied client-side after fetching everything.
- **Counts stored separately from entries.** Keeping `*StreamCount` as its
  own key (rather than embedding a length inside a single aggregate
  record) means checking "how many streams does this address have" is one
  cheap read, and iterating `0..count` to page through
  `SenderStream(addr, i)` reads is straightforward pagination without
  loading unrelated data.

## Consequences

- Pros: predictable, bounded per-call storage cost regardless of stream
  age or activity level; no off-chain scheduler dependency; clear,
  auditable invariants (`current_streak`-style monotonic accrual, rate
  cannot increase, minimum rate floor) that are directly fuzzed by
  `proptests.rs`.
- Cons: milestone gating has no receiver-side escape hatch short of
  external dispute resolution; a sender wanting to pay faster must open a
  new stream rather than mutate the existing one; enumerating "all streams
  for an address" costs one read per stream (acceptable given streams per
  address are expected to be small in practice).

## Revisit Trigger

If receivers routinely need a way to force resolution when a sender
withholds milestone approval, consider adding an optional third-party
arbitration hook (mirroring the dispute/arbitration model already used for
`PaymentProcessor` refunds) rather than changing the sender-controlled
default.
