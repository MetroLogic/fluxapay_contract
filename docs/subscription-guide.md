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

FluxaPay supports recurring billing via subscription plans, each with a fixed `amount`, `currency`, and `billing_interval` (`Daily`, `Weekly`, `Monthly`, or `Annually`).

Subscription lifecycle states are:

- `Active` — billing is enabled
- `Paused` — temporarily suspended until a resume timestamp is reached
- `Cancelled` — terminal stop; no future billing
- `Expired` — reached the configured `max_payments` limit or otherwise ended by the system

---

## 2) Creating a subscription plan

A merchant first creates a plan. Each plan defines the recurring charge and interval.

### API shape

```typescript
await client.createSubscriptionPlan({
  merchant: "GMERCHANT...",
  planId: "pro_monthly",
  name: "Pro",
  description: "Monthly SaaS access",
  amount: 5_000_000n,
  currency: "USDC",
  billingInterval: "Monthly",
});
```

The on-chain contract uses `create_subscription_plan`, which stores the plan and emits a `SUBSCRIPTION/PLAN_CREATED` event for indexer visibility.

### Soroban CLI example

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
  --billing_interval Monthly
```

You can then fetch a plan by ID:

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  -- create_subscription_plan \
  --plan_id "pro_monthly"
```

If you need the read variant without writing, use the matching `get_subscription_plan` / `get_plan` call through the SDK or binding client.

---

## 3) Customer subscription flow

The standard customer flow is:

1. Merchant creates a plan.
2. Customer subscribes to the plan.
3. The contract creates a `Subscription` object.
4. The first charge occurs after the billing interval elapses.
5. Future charges are triggered by the operator daemon or explicit processing calls.

### Subscribe a customer

```typescript
await client.subscribeToPlan({
  payer: "G_CUSTOMER...",
  planId: "pro_monthly",
  paymentId: "sub_001",
});
```

This creates a subscription with `status: Active`, stores the `next_payment_at` timestamp, and emits `SUBSCRIPTION/CREATED`.

### Soroban CLI example

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $CUSTOMER_SECRET \
  -- subscribe \
  --payer $CUSTOMER_ADDRESS \
  --plan_id "pro_monthly" \
  --max_payments 12
```

`max_payments` is optional. If set, the subscription ends automatically after the configured number of successful charges.

### First charge behavior

The contract does not bill immediately when the subscription is created. It waits until the next billing window is due, which is calculated from the plan interval and the subscription's `next_payment_at` timestamp.

---

## 4) Charge cycle and `process_due_subscriptions`

Recurring charges are handled by the operator/settlement flow. A due subscription is processed when its billing date is reached.

FluxaPay implements automated processing through `process_due_subscriptions`, which checks active subscriptions and executes each due charge in sequence. The daemon script in [scripts/subscription-daemon.js](../scripts/subscription-daemon.js) is the recommended operational pattern for polling and invoking this flow.

### How the cycle works

- The daemon scans tracked active subscriptions.
- It looks for subscriptions whose `next_payment_at` or `next_retry_at` is now or in the past.
- It invokes `process_due_subscriptions` from the operator account.
- The contract attempts each eligible subscription charge.
- Successful charges update `last_payment_at`, `total_payments`, and advance `next_payment_at`.
- Failed charges move the subscription into the retry/grace path.

### Example daemon run

```bash
CONTRACT_ID=C... OPERATOR_SECRET=S... node scripts/subscription-daemon.js
```

The daemon polls on a configurable interval and is meant to run continuously in production.

### Manual operator-triggered charge

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $OPERATOR_SECRET \
  -- process_due_subscriptions \
  --operator $OPERATOR_ADDRESS
```

The function returns the count of subscriptions processed in that cycle.

---

## 5) Grace period and retry logic

If a subscription charge fails, FluxaPay moves the customer into a grace period rather than immediately cancelling the subscription.

The contract enforces:

- `SUBSCRIPTION_MAX_RETRIES = 3`
- `SUBSCRIPTION_RETRY_INTERVAL_SECS = 2 * 24 * 60 * 60` (2 days)

This means:

1. A payment fails.
2. The subscription enters the retry window with `next_retry_at` set.
3. The daemon retries again after 2 days.
4. If the same subscription fails through all 3 retry windows, the system cancels it with `SubscriptionRetryExhausted`.

Relevant lifecycle events include:

- `SUBSCRIPTION/PAYMENT_FAILED`
- `SUBSCRIPTION/CANCELLED_MAX_RETRIES`
- `SUBSCRIPTION/CHARGED` on success

A failed charge does not silently disappear: merchants should watch for retry and cancellation events so they can notify the customer or prompt for updated payment credentials.

---

## 6) Pause and resume

Merchants and customers may pause a subscription when the service is temporarily suspended or a customer is taking a break.

### Pause immediately

```typescript
await client.pauseSubscription({
  payer: "G_CUSTOMER...",
  subscriptionId: "sub_123",
});
```

### Pause with a future resume date

```typescript
await client.pauseSubscriptionWithResumeDate({
  payer: "G_CUSTOMER...",
  subscriptionId: "sub_123",
  resumeTimestamp: Math.floor(Date.now() / 1000) + 30 * 24 * 60 * 60,
});
```

This updates the subscription to `Paused` and sets `resume_at` to the future timestamp. When that time passes, `charge_subscription` auto-resumes the subscription and advances `next_payment_at` from the resume timestamp.

### Resume manually

```typescript
await client.resumeSubscription({
  payer: "G_CUSTOMER...",
  subscriptionId: "sub_123",
});
```

### CLI examples

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $CUSTOMER_SECRET \
  -- pause_with_resume_date \
  --payer $CUSTOMER_ADDRESS \
  --subscription_id "sub_123" \
  --resume_timestamp 1750000000
```

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $CUSTOMER_SECRET \
  -- resume_subscription \
  --payer $CUSTOMER_ADDRESS \
  --subscription_id "sub_123"
```

Relevant events:

- `SUBSCRIPTION/PAUSED`
- `SUBSCRIPTION/RESUMED`

---

## 7) Cancellation and prorated refunds

A customer or merchant can cancel a subscription at any time with `cancel_subscription`.

```typescript
await client.cancelSubscription({
  payerOrMerchant: "G_CUSTOMER...",
  subscriptionId: "sub_123",
  refundRemaining: true,
});
```

When `refund_remaining` is `true`, the contract may create a prorated pending refund for the unused portion of the current billing period if the policy allows it. This is a policy-driven flow and is surfaced through refund events rather than by mutating the subscription itself beyond setting `status: Cancelled`.

The contract also supports `allow_prorated_refunds` and emits `REFUND/AUTO_CREATED` when the cancellation generates a refund.

This is useful for SaaS subscriptions where a customer cancels mid-cycle and should receive credit for the unused time.

---

## 8) Webhook events to listen for

For merchant integrations, the most important subscription events are:

- `SUBSCRIPTION/CREATED`
- `SUBSCRIPTION/CHARGED`
- `SUBSCRIPTION/PAUSED`
- `SUBSCRIPTION/RESUMED`
- `SUBSCRIPTION/CANCELLED`
- `SUBSCRIPTION/CANCELLED_MAX_RETRIES`
- `SUBSCRIPTION/PAYMENT_FAILED`
- `SUBSCRIPTION/EXPIRED`
- `SUBSCRIPTION/PLAN_CREATED`
- `SUBSCRIPTION/PLAN_DEACTIVATED`

If the merchant uses prorated refunds on cancellation, also listen for:

- `REFUND/AUTO_CREATED`
- `REFUND/REQUESTED` / `REFUND/PROCESSED` as the refund lifecycle completes

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
    billingInterval: "Monthly",
  });

  const plan = await client.getSubscriptionPlan("studio-monthly");
  console.log("Plan:", plan);
}

async function subscribeCustomer() {
  await client.subscribeToPlan({
    payer: "GCUSTOMER...",
    planId: "studio-monthly",
    paymentId: "sub_001",
  });

  await client.pauseSubscriptionWithResumeDate({
    payer: "GCUSTOMER...",
    subscriptionId: "sub_001",
    resumeTimestamp: Math.floor(Date.now() / 1000) + 86400,
  });

  await client.resumeSubscription({
    payer: "GCUSTOMER...",
    subscriptionId: "sub_001",
  });
}
```

This example demonstrates the full merchant journey: create plan → subscribe → pause/resume → track charge cycle via webhooks and the daemon.

---

## Operational guidance for production

To operate subscriptions reliably:

- Keep the daemon running continuously using the repo script at [scripts/subscription-daemon.js](../scripts/subscription-daemon.js).
- Listen for subscription lifecycle webhooks and reconcile the state in your own database.
- Track `retry_count`, `next_retry_at`, `pause`, and `resume_at` values for customer support and reconciliation.
- Use idempotent handlers for webhook processing so duplicate events do not double-apply subscription state.

If you are building a merchant dashboard, the safest pattern is to source truth from the on-chain subscription record and treat webhooks as a signal stream rather than the canonical state store.

---

## Summary

FluxaPay subscriptions give merchants a Stripe-like recurring billing system with on-chain transparency and explicit lifecycle events. The core pattern is simple:

1. Create a plan
2. Subscribe a payer
3. Charge on the interval
4. Retry failed payment attempts
5. Pause, resume, or cancel as needed
6. Reconcile state from on-chain and webhook data

For an end-to-end developer workflow and event mappings, also see [docs/webhooks.md](webhooks.md), [docs/events.md](events.md), and [scripts/README.md](../scripts/README.md).
