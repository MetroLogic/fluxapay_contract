# Subscription Guide

Recurring billing is a first-class FluxaPay flow for SaaS, memberships, media subscriptions, and other services where the same customer is charged repeatedly on a schedule.

This guide documents the **actual on-chain subscription lifecycle, dunning, grace, and trial behavior** as implemented in `fluxapay/src/lib.rs` (`PaymentProcessor` contract) and mirrored in `fluxapay/src/refund_manager.rs` (`RefundManager` contract). The TypeScript SDK (`sdk/src/index.ts`) talks to the `PaymentProcessor` entry points described here.

> **Implementation note.** `fluxapay/src/subscription.rs` (and its `subscription_test.rs` / `subscription_lifecycle_test.rs` companions) is a separate, **unwired** subscription module. It is not declared with `mod subscription;` anywhere in `lib.rs`, so it never compiles into either deployed contract, and `subscription_lifecycle_test.rs` isn't registered as a test module at all (it also references undefined identifiers like `tenv` and a typo'd `get_plam`). Earlier drafts of this guide were written against that dead module's API shape (`create_plan`, `cancel_within_grace_period`, a configurable `grace_period_secs`, `max_cycles`, etc.). None of those function names or fields exist in the live contracts — everything below has been re-verified against the code that actually ships.

## 1) When to use subscriptions

Use subscriptions when you want to charge the same customer repeatedly over time, such as:

- SaaS and platform memberships
- Monthly retainers or service fees
- Digital media and content products
- Access tiers or premium features
- Usage-metered bundles billed per cycle

Each plan has a `billing_interval` (`Daily`, `Weekly`, `Monthly`, `Annually` — converted internally to seconds via `BillingInterval::to_secs()`), an optional `trial_days`, and an optional per-subscription `max_payments` cap.

## 2) Lifecycle states

`SubscriptionStatus` (`fluxapay/src/lib.rs`) has exactly **four** variants:

- `Active` — billing is enabled. This also covers a subscription that has a failed charge and is waiting on a dunning retry (there is no separate "past due" status; check `retry_count` / `next_retry_at` on the `Subscription` record to detect that condition).
- `Paused` — suspended by the subscriber, either indefinitely (`pause_subscription`) or until a scheduled timestamp (`pause_with_resume_date`).
- `Cancelled` — terminal. Reached by an explicit `cancel_subscription` call *or* automatically when dunning retries are exhausted. The two cases emit different events (`SUBSCRIPTION/CANCELLED` vs `SUBSCRIPTION/CANCELLED_MAX_RETRIES`) but share the same `Cancelled` status value — there is no distinct `CancelledDueToPaymentFailure` status.
- `Expired` — terminal. Reached when `total_payments` reaches the subscription's `max_payments` cap. There is no separate `Completed` status; `Expired` is the terminal state for a subscription that ran its full course.

A cancelled subscription can be brought back to `Active` by an admin via `admin_reactivate_subscription` (this resets `retry_count` and reschedules `next_payment_at`; it does not restore a free trial).

## 3) Creating a subscription plan

A merchant creates a plan with `create_subscription_plan`. It requires the `MERCHANT` role and validates `amount > 0` and `trial_days <= MAX_TRIAL_DAYS` (90).

### TypeScript SDK

```typescript
await client.createSubscriptionPlan({
  merchant: "GMERCHANT...",
  planId: "pro_monthly",
  name: "Pro",
  description: "Monthly SaaS access",
  amount: 5_000_000n,
  currency: "USDC",
  billingInterval: "Monthly",
  trialDays: 14, // optional, omit or null for no trial
});
```

### Soroban CLI

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $MERCHANT_SECRET \
  -- create_subscription_plan \
  --merchant $MERCHANT_ADDRESS \
  --plan_id "pro_monthly" \
  --name "Pro" \
  --description "Monthly SaaS access" \
  --amount 5000000 \
  --currency USDC \
  --billing_interval Monthly \
  --trial_days 14
```

Fetch a plan with `get_subscription_plan`, and retire one (without deleting it) with `deactivate_subscription_plan`, which flips `active = false` so no new subscribers can join while existing subscriptions keep billing.

## 4) Free trial configuration

Trials are a plan-level setting, not a separate "trial" lifecycle state:

- A plan's `trial_days: Option<u32>` is capped at `MAX_TRIAL_DAYS = 90`; `create_subscription_plan` returns `Error::TrialTooLong` above that.
- When a customer subscribes to a plan with `trial_days` set, the contract computes `trial_ends_at = now + trial_days * 86_400` and sets the subscription's first `next_payment_at` to that timestamp (instead of `now + interval_secs`).
- Attempting to charge a subscription before `trial_ends_at` returns `Error::TrialActive` and does **not** bill — this is the trial's enforcement mechanism, there is no dedicated `Trial` status.
- `SUBSCRIPTION/TRIAL_STARTED` is emitted at subscribe time when a trial applies; `SUBSCRIPTION/TRIAL_ENDED` is emitted the first time a post-trial charge is attempted (`total_payments == 0` and `trial_ends_at.is_some()`).
- A plan without `trial_days` behaves exactly as before — `trial_ends_at` is `None` and the first charge is due after one full `interval_secs`.

## 5) Subscribing

Two entry points create a `Subscription` record:

- **`subscribe(payer, plan_id, max_payments, affiliate, affiliate_fee_bps)`** — the contract generates the `subscription_id` (`sub_<counter>`). Supports an optional lifetime cap (`max_payments`) and an optional affiliate revenue share taken out of each charge.
- **`subscribe_to_plan(payer, subscription_id, plan_id)`** — the caller supplies the `subscription_id` (useful when tying a subscription to a pre-existing payment/order ID); fails with `Error::PaymentAlreadyExists` if that ID is already used, and does not support `max_payments` or affiliates.

Both reject subscribing to an inactive (`deactivate_subscription_plan`'d) plan with `Error::PaymentAlreadyProcessed`, and both emit `SUBSCRIPTION/CREATED`.

```typescript
const subscriptionId = await client.subscribe({
  payer: "GCUSTOMER...",
  planId: "pro_monthly",
  maxPayments: 12, // optional
});
```

## 6) Charging and dunning (grace period)

There is **no separate configurable "grace period" field** on a plan or subscription — the constants `GRACE_PERIOD_SECS`, `MAX_GRACE_PERIOD_SECS`, `MAX_DUNNING_RETRIES`, and `DUNNING_RETRY_BACKOFF_SECS` in `fluxapay/src/constants.rs` are defined but are **dead code**: they are referenced only by the unwired `subscription.rs` module, never by `lib.rs` or `refund_manager.rs`. The grace/dunning behavior that actually ships is driven by two different constants:

- `SUBSCRIPTION_MAX_RETRIES = 3`
- `SUBSCRIPTION_RETRY_INTERVAL_SECS = 2 days` (172,800 seconds)

An operator (holding the `oracle` or `settlement_operator` role) triggers a charge with `process_subscription` (checks the charge is actually due, then delegates to `charge_subscription`) or `charge_subscription` directly:

1. If the subscription is `Paused` and its scheduled `resume_at` has passed, it auto-resumes first.
2. If not `Active`, or still inside a trial, the call is a no-op (`Ok(status)`) or returns `Error::TrialActive` — no charge is attempted.
3. If the due date (`next_retry_at` if retrying, else `next_payment_at`) hasn't arrived, it's a no-op.
4. The contract pulls the full amount from the payer via `token::Client::try_transfer`.
   - **Success**: resets `retry_count` to 0, advances `next_payment_at` by `interval_secs`, emits `SUBSCRIPTION/CHARGED`. If `total_payments` now meets `max_payments`, status becomes `Expired` and `SUBSCRIPTION/EXPIRED` is also emitted.
   - **Failure**: increments `retry_count`. If `retry_count >= SUBSCRIPTION_MAX_RETRIES`, the subscription is set to `Cancelled`, `SUBSCRIPTION/CANCELLED_MAX_RETRIES` is emitted, and the call returns `Error::SubscriptionRetryExhausted`. Otherwise `next_retry_at = now + SUBSCRIPTION_RETRY_INTERVAL_SECS`, `SUBSCRIPTION/PAYMENT_FAILED` is emitted, and the call returns `Error::SubscriptionInGracePeriod`.

So the effective maximum "grace window" before cancellation is `SUBSCRIPTION_MAX_RETRIES * SUBSCRIPTION_RETRY_INTERVAL_SECS` = 6 days.

### A second, separate batch path: `process_due_subscriptions`

`process_due_subscriptions(operator)` is a distinct batch entry point that scans the `ActiveSubscriptions` index and, for every subscription whose `next_payment_at` has passed, **creates a `Pending` `PaymentCharge` record** (to be paid into the contract's deposit address) and emits `SUBSCRIPTION/TICKED` — it does **not** call `token::try_transfer` and has **no retry/dunning logic**. If `max_payments` is reached it sets the subscription straight to `Cancelled` (not `Expired`) and emits both `SUBSCRIPTION/COMPLETED` and `SUBSCRIPTION/CANCELLED`. This is a different billing model (deposit-and-settle) from `charge_subscription`'s direct-pull model, and the two paths are not interchangeable — pick one per deployment, don't mix them for the same subscription.

The daemon script [scripts/subscription-daemon.js](../scripts/subscription-daemon.js) is the recommended operational pattern for polling due subscriptions and invoking `process_subscription`/`charge_subscription`.

## 7) Usage-metered subscriptions

For metered billing, `submit_usage_metrics(operator, subscription_id, units_used, unit_price, token)` overrides the subscription's `amount` with `units_used * unit_price`, appends the record to a per-subscription usage log, emits `SUBSCRIPTION/USAGE_RECORDED`, and immediately triggers `charge_subscription` at the new amount. It's rejected with `Error::InvalidStatusTransition` for a `Cancelled` or `Expired` subscription. Query history with `get_usage_metrics(subscription_id, from_timestamp, to_timestamp)`.

## 8) Pause and resume

```typescript
await client.pauseSubscription("GCUSTOMER...", subscriptionId); // indefinite pause
await client.resumeSubscription("GCUSTOMER...", subscriptionId); // manual resume
```

- `pause_subscription(payer, subscription_id)` pauses indefinitely; a matching `resume_subscription(payer, subscription_id)` call is required to bill again, and resets `next_payment_at` to `now + interval_secs` (the paused duration is not back-billed).
- `pause_with_resume_date(payer, subscription_id, resume_timestamp)` schedules an automatic resume; the next `charge_subscription`/`process_subscription` call at or after `resume_timestamp` auto-resumes the subscription before attempting the charge.
- Only the subscriber (`payer_address`) may pause/resume their own subscription; mismatched callers get `Error::Unauthorized`.

Events: `SUBSCRIPTION/PAUSED`, `SUBSCRIPTION/RESUMED`.

## 9) Cancellation and proration

```typescript
await client.cancelSubscription("GCUSTOMER...", subscriptionId);
```

`cancel_subscription(payer_or_merchant, subscription_id, refund_remaining)` may be called by either the subscriber or the merchant. There is **no** `cancel_within_grace_period` function (an earlier draft of this guide referenced one that does not exist in the live code).

Proration is opt-in and admin-gated:

- An admin enables it platform-wide with `set_allow_prorated_refunds(admin, true)`.
- When proration is enabled **and** the caller passes `refund_remaining: true` **and** the subscription was charged within the still-open billing period, the contract computes `prorated = amount * days_remaining / period_days` (whole days) and opens a pending refund for that amount via the refund flow.
- If proration is disabled, or the cancel-time conditions above aren't met, cancellation is immediate with no refund.

Cancelling always emits `SUBSCRIPTION/CANCELLED` and removes the subscription from the `ActiveSubscriptions` index so it's skipped by `process_due_subscriptions`/dunning going forward.

## 10) Admin reactivation

`admin_reactivate_subscription(admin, subscription_id)` (requires the `admin` role) can only be called on a `Cancelled` subscription. It resets `retry_count` to 0, clears `next_retry_at`, reschedules `next_payment_at` to `now + interval_secs`, re-adds the subscription to the active index, and emits `SUBSCRIPTION/REACTIVATED`. It does not distinguish *why* the subscription was cancelled (manual vs. dunning-exhausted) and does not reinstate a trial.

## 11) Event sequence reference

All subscription events use the two-level topic `(SUBSCRIPTION, <ACTION>)`. The actions that are actually emitted by the live contracts are:

| Event | Emitted by | Notes |
|---|---|---|
| `SUBSCRIPTION/PLAN_CREATED` | `create_subscription_plan` | |
| `SUBSCRIPTION/PLAN_DEACTIVATED` | `deactivate_subscription_plan` | |
| `SUBSCRIPTION/CREATED` | `subscribe` / `subscribe_to_plan` | |
| `SUBSCRIPTION/TRIAL_STARTED` | `subscribe` / `subscribe_to_plan` | only when the plan has `trial_days` |
| `SUBSCRIPTION/TRIAL_ENDED` | `charge_subscription` | first post-trial charge attempt |
| `SUBSCRIPTION/CHARGED` | `charge_subscription` | successful charge |
| `SUBSCRIPTION/EXPIRED` | `charge_subscription` | `max_payments` cap reached on a successful charge |
| `SUBSCRIPTION/PAYMENT_FAILED` | `charge_subscription` | charge failed, retry scheduled |
| `SUBSCRIPTION/CANCELLED_MAX_RETRIES` | `charge_subscription` | dunning retries exhausted |
| `SUBSCRIPTION/PAUSED` | `pause_subscription` / `pause_with_resume_date` | |
| `SUBSCRIPTION/RESUMED` | `resume_subscription` / auto-resume in `charge_subscription` | |
| `SUBSCRIPTION/CANCELLED` | `cancel_subscription`, or `process_due_subscriptions` on cap | |
| `SUBSCRIPTION/REACTIVATED` | `admin_reactivate_subscription` | |
| `SUBSCRIPTION/USAGE_RECORDED` | `submit_usage_metrics` | |
| `SUBSCRIPTION/TICKED` | `process_due_subscriptions` | batch "create a pending payment" path only |
| `SUBSCRIPTION/COMPLETED` | `process_due_subscriptions` | batch-path cap reached (note: emitted alongside `CANCELLED`, not `EXPIRED`, in this path) |

A typical **normal** lifecycle: `PLAN_CREATED` → `CREATED` → `CHARGED` (repeating) → `CANCELLED` (manual) or `EXPIRED` (cap reached via `charge_subscription`).

A typical **trial** lifecycle: `PLAN_CREATED` → `CREATED` + `TRIAL_STARTED` → (charge attempts return `TrialActive` and emit nothing) → `TRIAL_ENDED` + `CHARGED` → `CHARGED` (repeating).

A typical **dunning** lifecycle: `CHARGED` → `PAYMENT_FAILED` → `PAYMENT_FAILED` → `PAYMENT_FAILED` → `CANCELLED_MAX_RETRIES` (after `SUBSCRIPTION_MAX_RETRIES` consecutive failures), optionally followed later by `REACTIVATED` if an admin steps in.

## 12) Webhook events to listen for

For merchant integrations, the events most worth wiring a webhook handler to are:

- `SUBSCRIPTION/CREATED`
- `SUBSCRIPTION/CHARGED`
- `SUBSCRIPTION/PAYMENT_FAILED`
- `SUBSCRIPTION/CANCELLED_MAX_RETRIES`
- `SUBSCRIPTION/CANCELLED`
- `SUBSCRIPTION/EXPIRED`
- `SUBSCRIPTION/TRIAL_STARTED` / `SUBSCRIPTION/TRIAL_ENDED`
- `SUBSCRIPTION/PLAN_DEACTIVATED`

A webhook consumer should key events by `subscription_id` and treat retries/redelivery as idempotent, the same way you would for standard payment webhooks.

## 13) TypeScript SDK example

```typescript
import { FluxapayClient } from "@fluxapay/sdk";

const client = new FluxapayClient("testnet");

async function createMonthlyPlan() {
  await client.createSubscriptionPlan({
    merchant: "GMERCHANT...",
    planId: "studio-monthly",
    name: "Studio",
    description: "Monthly access to studio tools",
    amount: 25_000_000n,
    currency: "USDC",
    billingInterval: "Monthly",
    trialDays: 14,
  });

  const plan = await client.getSubscriptionPlan("studio-monthly");
  console.log("Plan:", plan);
}

async function subscribeCustomer() {
  const subscriptionId = await client.subscribe({
    payer: "GCUSTOMER...",
    planId: "studio-monthly",
  });

  await client.pauseSubscription("GCUSTOMER...", subscriptionId);
  await client.resumeSubscription("GCUSTOMER...", subscriptionId);
}
```

This example demonstrates the full merchant journey: create plan → subscribe (optionally with a trial) → pause/resume → track charge cycle via webhooks and the daemon.

## Operational guidance for production

To operate subscriptions reliably:

- Keep the daemon running continuously using the repo script at [scripts/subscription-daemon.js](../scripts/subscription-daemon.js).
- Decide up front whether you're using the direct-pull path (`charge_subscription`/`process_subscription`, with dunning) or the batch deposit path (`process_due_subscriptions`, no dunning) — they are not interchangeable for the same subscription.
- Monitor `SUBSCRIPTION/PAYMENT_FAILED` and `SUBSCRIPTION/CANCELLED_MAX_RETRIES` events to proactively notify customers before/after dunning exhausts.
- Use `SUBSCRIPTION/EXPIRED` to trigger renewal campaigns or access revocation when a `max_payments`-capped plan completes.
- The effective maximum grace window before automatic cancellation is `SUBSCRIPTION_MAX_RETRIES * SUBSCRIPTION_RETRY_INTERVAL_SECS` (currently 3 × 2 days = 6 days); there is no per-plan override for this today.

### Daemon RPC Timeout & Retry Handling

During periods of Stellar network congestion, calls to `charge_subscription`/`process_subscription` may encounter RPC timeouts while the transaction is awaiting inclusion in a closed ledger. To avoid silently skipping billing cycles:

1. **Retry Queue Persistence**:
   - On an RPC timeout (or missing transaction receipt within the poll timeout window), the daemon immediately persists the transaction record (`transaction_hash`, `subscription_id`, attempt count, and timestamps) to a local retry queue (`subscription_retry_queue.json`).
   - The cycle is never marked as a definitive failure or discarded on timeout.

2. **Horizon Status Poller**:
   - A dedicated poller queries the Horizon endpoint (`GET /transactions/{hash}`) before scheduling a re-submission.
   - If Horizon reports `successful: true`, the charge was included; the record is removed from the queue and marked successful.
   - If Horizon returns `404 Not Found` and the ledger close window (~30 seconds) has elapsed, the daemon prepares a re-submission with the same operator keypair and fresh sequence guard.

3. **Exponential Backoff**:
   - Re-submissions follow exponential backoff: base delay of 1s, doubling per retry (1s, 2s, 4s).
   - The daemon allows a maximum of **3 retry attempts** per timeout cycle. This is a daemon-side retry safeguard for RPC flakiness, separate from the contract's own `SUBSCRIPTION_MAX_RETRIES` dunning counter described in section 6.

4. **Retry Exhaustion Alerting**:
   - If all 3 retry attempts are exhausted without confirmation, the daemon:
     - Emits a `charge.failed` webhook to the configured `WEBHOOK_URL` containing the failure context (`subscription_id`, `transaction_hash`, attempt count, and reason).
     - Logs a high-priority `ERROR` to system logs for operational intervention.
     - Removes the failed record from the active retry queue to prevent unbounded retries.
