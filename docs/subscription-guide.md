# Subscription Guide

Recurring billing is a first-class FluxaPay flow for SaaS, memberships, media subscriptions, and other services where the same customer is charged repeatedly on a schedule.

This guide explains how merchants create plans, onboard customers, process recurring charges, and react to lifecycle events in production.

## 1) When to use subscriptions

Use subscriptions when you want to charge the same customer repeatedly over time, such as:

- SaaS and platform memberships
- Monthly retainers or service fees
- Digital media and content products
- Access tiers or premium features
- Usage bundles with fixed-period renewal

FluxaPay supports recurring billing via subscription plans, each with a configurable `interval_secs` (in seconds), an optional `max_cycles` limit, and a configurable grace period.

Subscription lifecycle states are:

- `Active` — billing is enabled
- `Paused` — temporarily suspended by the subscriber- `PastDue` — a charge failed and the contract is waiting to retry
- `Cancelled` — terminal stop; no future billing
- `CancelledDueToPaymentFailure` — terminal stop after exceeding dunning retries
- `Completed` — reached the configured `max_cycles` limit

---

## 2) Creating a subscription plan

A merchant first creates a plan. Each plan defines the recurring charge, interval, and lifetime.

### API shape

```typescript
await client.createSubscriptionPlan({
  merchant: "GMERCHANT...",
  planId: "pro_monthly",
  name: "Pro",
  description: "Monthly SaaS access",
  amount: 5_000_000n,
  currency: "USDC",
  intervalSecs: 2592000,
  maxCycles: 12,
  gracePeriodSecs: 86400,
});
```

The on-chain contract uses `create_plal`, which stores the plan and emits a `SUBSCRIPTION/PLAN_CREATED` event for indexer visibility.

### Soroban CLI example

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $MERCHANT_SECRET \
  -- create_plan \
  --merchant $MERCHANT_ADDRESS \
  --plan_id "pro_monthly" \
  --name "Pro" \
  --amount 5000000 \
  --interval_secs 2592000 \
  --max_cycles 12 \
  --grace_period_secs 86400
```

You can then fetch a plan by Id:

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  -- get_plan \
  --plan_id "pro_monthly"
```

---

## 3) Customer subscription flow

The standard customer flow is:

1. Merchant creates a plan.
2. Customer subscribes to the plan.
3. The contract creates a `Subscription` object with `current_cycle = 0`.
4. The first charge occurs after the billing interval elapses.
5. Future charges are triggered by the operator daemon or explicit processing calls.

### Subscribe a customer

```typescript
await client.subscribeToPlan({
  payer: "G_CUSTOMER...",
  planId: "pro_monthly",
});
```

This creates a subscription with `status: Active`, stores the `current_cycle` counter, and emits `SUBSCRIPTION/CREATED`.

### Soroban CLI example

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $CUSTOMER_SECRET \
  -- subscribe \
  --subscriber $CUSTOMER_ADDRESS \
  --plan_id "pro_monthly"
```

### First charge behavior

The contract does not bill immediately when the subscription is created. It waits until the next billing window is due, which is calculated from the plan `interval_secs` and the subscription's `last_charge_at` timestamp.

---

## 4) Charge cycle and charge_subscription

Recurring charges are handled by the operator/settlement flow. A due subscription is processed when its billing date is reached.

FluxaPay implements automated processing through `charge_subscription`, which checks active subscriptions and executes each due charge in sequence. The daemon script in [scripts/subscription-daemon.js](../scripts/subscription-daemon.js) is the recommended operational pattern for polling and invoking this flow.

### How the cycle works

- The daemon scans tracked active subscriptions.
- It looks for subscriptions whose `last_charge_at + interval_secs` is now or in the past.
- It invokes `charge_subscription` from the operator account.
- The contract attempts each eligible subscription charge.
- Successful charges update `last_charge_at`, `current_cycle`, and open a fresh grace window.
- Failed charges move the subscription into the dunning retry path.

### Manual operator-triggered charge

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $OPERATOR_SECRET \
  -- charge_subscription \
  --plan_id "pro_monthly" \
  --subscriber $CUSTOMER_ADDRESS \
  --charge_succeeded true
```

The function returns an error if the interval has not elapsed, the subscription is paused, or the max cycle limit has been reached.

---

## 5) Dunning retry logic

If a subscription charge fails, FluxaPay moves the customer into a dunning state rather than immediately cancelling the subscription.

The contract enforces:

- `MAX_DUNNING_RETRIES = 3`
- `DUNNING_BACKOFF_SECS = 86400` (1 day minimum retry backoff)

This means:

1. A payment fails.
2. The subscription enters the `PastDue` state with `next_retry_at` set.
3. The daemon retries after the backoff window.
4. If the same subscription fails through all 3 retry windows, the system cancels it with `CancelledDueToPaymentFailure`.

Relevant lifecycle events include:

- `SUBSCRIPTION/CHARGE_FAILED`
- `SUBSCRIPTION/DUNNING_TRIGGERED`
- `SUBSCRIPTION/CHARGED` on success

A failed charge does not silently disappear: merchants should watch for retry and cancellation events so they can notify the customer or prompt for updated payment credentials.

### Daemon RPC Timeout & Retry Handling

During periods of Stellar network congestion, calls to `charge_subscription` may encounter RPC timeouts while the transaction is awaiting inclusion in a closed ledger. To avoid silently skipping billing cycles:

1. **Retry Queue Persistence**:
   - On an RPC timeout (or missing transaction receipt within the poll timeout window), the daemon immediately persists the transaction record (`transaction_hash`, `subscription_id`, attempt count, and timestamps) to a local retry queue (`subscription_retry_queue.json`).
   - The cycle is never marked as a definitive failure or discarded on timeout.

2. **Horizon Status Poller**:
   - A dedicated poller queries the Horizon endpoint (`GET /transactions/{hash}`) before scheduling a re-submission.
   - If Horizon reports `successful: true`, the charge was included; the record is removed from the queue and marked successful.
   - If Horizon returns `404 Not Found` and the ledger close window (~30 seconds) has elapsed, the daemon prepares a re-submission with the same operator keypair and fresh sequence guard.

3. **Exponential Backoff**:
   - Re-submissions follow exponential backoff: base delay of 1s, doubling per retry (1s, 2s, 4s).
   - The daemon allows a maximum of **3 retry attempts** per timeout cycle.

4. **Retry Exhaustion Alerting**:
   - If all 3 retry attempts are exhausted without confirmation, the daemon:
     - Emits a `charge.failed` webhook to the configured `WEBHOOK_URL` containing the failure context (`subscription_id`, `transaction_hash`, attempt count, and reason).
     - Logs a high-priority `ERROR` to system logs for operational intervention.
     - Removes the failed record from the active retry queue to prevent unbounded retries.

---

## 6) Pause and resume

Subscribers may pause a subscription when the service is temporarily suspended or a customer is taking a break.

### Pause immediately

```typescript
await client.pauseSubscription({
  subscriber: "G_CUSTOMER...",
  planId: "pro_monthly",
});
```

This updates the subscription to `Paused` and emits `SUBSCRIPTION/PAUSED`. While paused, `charge_subscription` returns `SubscriptionPaused`.

### Resume manually

```typescript
await client.resumeSubscription({
  subscriber: "G_CUSTOMER...",
  planId: "pro_monthly",
});
```

Resuming resets the billing baseline (`last_charge_at`) to the current timestamp so the paused duration is not back-billed. The contract emits `SUBSCRIPTION/RESUMED`.

### CLI examples

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $CUSTOMER_SECRET \
  -- pause_subscription \
  --plan_id "pro_monthly" \
  --subscriber $CUSTOMER_ADDRESS
```

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $CUSTOMER_SECRET \
  -- resume_subscription \
  --plan_id "pro_monthly" \
  --subscriber $CUSTOMER_ADDRESS
```

Relevant events:

- `SUBSCRIPTION/PAUSED`
- `SUBSCRIPTION/RESUMED`

---

## 7) Cancellation and prorated refunds

A customer or merchant can cancel a subscription at any time with `cancel_subscription`.

```typescript
await client.cancelSubscription({
  subscriber: "G_CUSTOMER...",
  planId: "pro_monthly",
});
```

Within the grace window, `cancel_within_grace_period` cancels the subscription and issues a refund for the most recent charge. Outside the grace window, no refund is issued.

---

## 8) Webhook events to listen for

For merchant integrations, the most important subscription events are:

- `SUBSCRIPTION/CREATED`
- `SUBSCRIPTION/CHARGED`
- `SUBSCRIPTION/CHARGE_FAILED`
- `SUBSCRIPTION/DUNNING_TRIGGERED`
- `SUBSCRIPTION/PAUSED`
- `SUBSCRIPTION/RESUMED`
- `SUBSCRIPTION/COMPLETED`
- `SUBSCRIPTION/CANCELLED`
- `SUBSCRIPTION/GRACE_CANCEL`
- `SUBSCRIPTION/PLAN_CREATED`
- `SUBSCRIPTION/PLAN_ARCHIVED`
- `SUBSCRIPTION/PLAN_RESTORED`

A webhook consumer should key events by `subscription_id` or `refund_id` and treat retries as idempotent in the same way you would for standard payment webhooks.

---

## 9) TypeScript SDK example

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
    intervalSecs: 2592000,
    maxCycles: 12,
    gracePeriodSecs: 86400,
  });

  const plan = await client.getSubscriptionPlan("studio-monthly");
  console.log("Plan:", plan);
}

async function subscribeCustomer() {
  await client.subscribeToPlan({
    subscriber: "GCUSTOMER...",
    planId: "studio-monthly",
  });

  await client.pauseSubscription({
    subscriber: "GCUSTOMER...",
    planId: "studio-monthly",
  });

  await client.resumeSubscription({
    subscriber: "GCUSTOMER...",
    planId: "studio-monthly",
  });
}
```

This example demonstrates the full merchant journey: create plan → subscribe → pause/resume → track charge cycle via webhooks and the daemon.

---

## Operational guidance for production

To operate subscriptions reliably:

- Keep the daemon running continuously using the repo script at [scripts/subscription-daemon.js](../scripts/subscription-daemon.js).
- Monitor `SUBSCRIPTION/CHARGE_FAILED` and `SUBSCRIPTION/DUNNING_TRIGGERED` events to proactively notify customers.
- Use `SUBSCRIPTION/COMPLETED` to trigger renewal campaigns or access revocation.
- Keep the grace period configured at or below the maximum of 3 days.
