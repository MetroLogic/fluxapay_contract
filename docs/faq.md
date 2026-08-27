# Frequently Asked Questions

Common questions from merchants and developers using FluxaPay.

## Payments

### Why is my payment showing `Pending`?

The payment is waiting for the customer to complete the USDC transfer on Stellar. Once the on-chain transaction is confirmed and detected by FluxaPay, the status will update to `Confirmed`. If the payment window expires, the status changes to `Expired`.

### What is the tolerance window?

FluxaPay accepts payments within a configurable tolerance window around the expected amount. Overpayments within the tolerance are treated as `Confirmed`; underpayments below the minimum threshold are rejected with `AmountBelowMin` (error #21). See [docs/architecture.md](architecture.md) for details.

### Can I accept tokens other than USDC?

Currently FluxaPay supports USDC on Stellar. Other tokens will return `UnsupportedToken` (error #20). Multi-token support is on the roadmap.

### How do I create a payment?

Use the SDK or contract invoke:

```bash
stellar contract invoke --id <PAYMENT_PROCESSOR_ID> --network testnet \
  -- create_payment \
  --args '{"payment_id": "ord_123", "merchant_id": "...", "amount": 1000000000, "currency": "USDC", ...}'
```

See [docs/quickstart.md](quickstart.md) for the full merchant quickstart.

### What happens if a payment expires?

Expired payments cannot be processed. The customer must initiate a new payment. The merchant can create a fresh payment link with a new `payment_id`.

---

## Refunds

### How long does a refund take?

After an operator calls `process_refund`, the on-chain USDC transfer executes immediately. The refund status transitions to `Completed` and a `REFUND/COMPLETED` webhook is emitted.

### Can I issue a partial refund?

Yes. Multiple partial refunds are supported as long as the total of all non-rejected refunds does not exceed the original payment amount (`RefundExceedsPayment` error #16).

### What is the 5-minute cooldown?

After a payment is confirmed, there is a configurable cooldown period before refunds can be requested. If you try to refund too early, you will get `RefundCooldownNotElapsed` (error #42). Wait until `confirmed_at + refund_cooldown_secs` has passed.

### How do I check refund status?

```bash
stellar contract invoke --id <REFUND_MANAGER_ID> --network testnet \
  -- get_refund --refund_id "refund_1"
```

See [README.md](../README.md#querying-refunds) for more examples.

---

## Disputes

### What is a dispute bond?

A dispute bond is a stake required when opening a dispute. It prevents spam disputes and ensures the disputing party has skin in the game. The bond is returned after the dispute is resolved.

### How do I submit evidence?

Submit dispute evidence as a valid IPFS CID (CIDv0 or CIDv1). Invalid evidence returns `InvalidEvidenceFormat` (error #45). Upload your evidence to IPFS first, then reference the CID in the dispute submission.

### What happens after 7 days?

Large disputes have a computed deadline (7 days). Small disputes have a 3-day deadline. If the dispute is not resolved by the deadline, it is escalated. See `computed_deadline_secs` in the dispute data.

---

## Subscriptions

### How do retries work?

When a subscription payment fails, it enters a grace period (`SubscriptionInGracePeriod`, error #30). The daemon retries at each poll interval. After exhausting all retries, the subscription is cancelled (`SubscriptionRetryExhausted`, error #31).

### What is prorated cancellation?

When a subscription is cancelled mid-period, the customer is refunded a prorated amount for the unused portion of the billing period.

---

## SDK

### Which network do I use for development?

Use `testnet` for development and staging. The SDK defaults to testnet:

```typescript
import { FluxapayClient } from "@fluxapay/sdk";
const client = new FluxapayClient("testnet");
```

See `sdk/src/network-profiles.ts` for available network configurations.

### How do I handle `FluxapayError`?

```typescript
import { FluxapayError } from "@fluxapay/sdk";

try {
  await client.createPayment(args);
} catch (err) {
  if (err instanceof FluxapayError) {
    console.log(err.code);           // numeric error code
    console.log(err.contractErrorName); // human-readable name
  }
}
```

The full error code reference is at [docs/error-codes.md](error-codes.md).

---

## KYC

### What are the tier limits?

KYC tiers define monthly processing volume caps for merchants. Exceeding your tier's limit returns `TierVolumeLimitExceeded` (error #38). Tier limits are configured in the MerchantRegistry contract.

### How does auto-upgrade work?

Merchants can request a tier upgrade through the admin flow. Upgrades are processed after verification. The monthly volume cap resets at the start of each calendar month.

---

## Security

### How do I verify webhook signatures?

FluxaPay signs webhook payloads with HMAC-SHA256. Verify the signature using the shared secret configured in your merchant settings. See [docs/webhooks.md](webhooks.md) for the full verification guide.

### What is SEP-10?

[SEP-10](https://stellar.org/sep-10) is the Stellar Web Authentication standard. FluxaPay uses SEP-10 for merchant authentication, ensuring that only verified merchants can access their payment data.

---

## Error Codes

### Where can I find the full error code list?

See [docs/error-codes.md](error-codes.md) for the complete reference of every contract error code with causes and remediation steps.

### What does `check-error-map-sync.ts` do?

It verifies that the SDK's `FLUXAPAY_CONTRACT_ERROR_MAP` matches the Rust `Error` enum. This runs in CI and prevents drift between the contract and SDK. See [scripts/README.md](../scripts/README.md) for details.
