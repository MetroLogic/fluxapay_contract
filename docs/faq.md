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

### How do I convert USDC stroops to display amounts?

USDC on Stellar uses 7 decimal places, so every on-chain amount is an integer number of stroops: divide by 10^7 to display it, multiply by 10^7 to send it. Keep amounts as `bigint` until the final formatting step to avoid floating-point rounding.

```typescript
const STROOPS_PER_USDC = 10_000_000n;

const toDisplay = (stroops: bigint) =>
  `${stroops / STROOPS_PER_USDC}.${(stroops % STROOPS_PER_USDC).toString().padStart(7, "0")}`;
const toStroops = (usdc: string) => {
  const [whole, frac = ""] = usdc.split(".");
  return BigInt(whole) * STROOPS_PER_USDC + BigInt(frac.padEnd(7, "0").slice(0, 7));
};

toDisplay(10_000_000n); // "1.0000000"
toStroops("5.25");      // 52_500_000n
```

See the [Field Conversion Guide](local-invoke.md#field-conversion-guide) for the other contract field types.

### What happens to my payment if it expires?

Expired payments cannot be processed; the customer must start a new payment, and the merchant can create a fresh payment link with a new `payment_id`. A `payment.expired` webhook is emitted ([payload](webhooks.md#payment-expired-payload-paymentexpired)). If the payment allowed partial funding, any partially contributed funds are refunded to the payer automatically.

### Can I accept partial payments?

Yes. Create the payment with `allowPartial: true` (`allow_partial` on-chain). The customer can then fund it across several transactions; each one emits `PAYMENT/PARTIAL_RECEIVED`, and the payment becomes `Confirmed` once the total received reaches `amount`. A merchant can also accept a `PartiallyPaid` payment at the amount received with `acceptPartialPayment`. See [Partial Payments in the quickstart](quickstart.md).

### What is the maximum payment amount?

There is no fixed protocol-wide maximum. Limits come from the amount limits configured for the merchant, falling back to the global amount limits; an amount above the configured `max` is rejected with `AmountAboveMax` (error #22). Monthly volume is also capped by your KYC tier (`TierVolumeLimitExceeded`, error #38). See [error codes](error-codes.md).

### Why is my FX rate stale?

Fiat equivalents come from the FXOracle contract, which `scripts/fx-oracle-updater.js` refreshes every `UPDATE_INTERVAL_MS` (default 60 s). If the updater is stopped or its price feed is down, the oracle keeps its last good rates, and `get_rate` returns `RateStale` once a rate is older than `MAX_STALENESS_SECS` (default 86 400 s); the updater alerts at 80% of that age. Check that the updater is running and that `ORACLE_SECRET` and `FX_ORACLE_CONTRACT_ID` are set. See [scripts/README.md](../scripts/README.md).

---

## Refunds

### How long does a refund take?

On-chain, a refund completes in the same transaction: once an operator calls `process_refund`, the USDC transfer executes immediately, the status moves to `Completed`, and a `REFUND/COMPLETED` webhook is emitted. The wait before that is the refund cooldown (below) plus your own approval process. If the customer then cashes out to fiat through an anchor, that off-chain leg follows the anchor's own timeline — see [anchor integration](sep6-sep24-anchor-integration.md).

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

## Webhooks

### Why did my webhook not fire?

Work through this checklist:

1. The endpoint URL is registered for this merchant and reachable over public HTTPS.
2. It returns HTTP `2xx` quickly; anything else counts as a failure.
3. Signature verification is not rejecting valid deliveries — check the algorithm and secret or key ([verification](webhooks.md#webhook-signature-verification-hmac-sha256--ed25519)).
4. The retry budget is not exhausted: by default FluxaPay makes 4 attempts (1 + 3 retries, ~1 s / 2 s / 4 s backoff), then marks the event failed. Replay it from the dashboard or indexer ([retry policy](webhooks.md#retry-policy)).
5. You are not discarding a retry as a duplicate by mistake: `event_id` stays the same across retries ([idempotency](webhooks.md#idempotency)).

For local development, see [testing webhooks locally](webhooks.md#testing-webhooks-locally).

### How do I verify webhook signatures?

FluxaPay signs webhook payloads with HMAC-SHA256 or Ed25519. Verify the signature using the shared secret configured in your merchant settings, or the platform public key for Ed25519. See [docs/webhooks.md](webhooks.md) for the full verification guide.

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

## KYC & Compliance

### What are the tier limits?

KYC tiers define monthly processing volume caps for merchants. Exceeding your tier's limit returns `TierVolumeLimitExceeded` (error #38). Tier limits are configured in the MerchantRegistry contract.

### When does my KYC tier upgrade automatically?

No admin action is needed: the contract upgrades a merchant when cumulative historical volume crosses a threshold — $500 for Unverified → Basic, $10,000 for Basic → Full, and $100,000 for Full → Business. The monthly cap of the new tier still applies, and it resets at the start of each calendar month. See [ADR-0003](ADR-0003-kyc-tier-system.md#auto-upgrade-thresholds).

---

## Security

### What is SEP-10?

[SEP-10](https://stellar.org/sep-10) is the Stellar Web Authentication standard. FluxaPay uses SEP-10 for merchant authentication, ensuring that only verified merchants can access their payment data.

---

## Development

### How do I test on testnet without real USDC?

1. Fund your testnet account with XLM from [Friendbot](https://friendbot.stellar.org/): `curl "https://friendbot.stellar.org?addr=<G-address>"`.
2. Get testnet USDC from Circle's testnet faucet, or deploy your own test asset with `stellar contract asset deploy` and point `USDC_TOKEN_ADDRESS` at it.
3. Run the flow end to end with the [quickstart](quickstart.md), or use the fully local [sandbox](sandbox-deployment.md), which ships its own Friendbot.

### How do I migrate from v1 to v2 of the SDK?

v2.0 is still planned; its changes (`FeeConfig` restructuring, `FluxapayError` as the primary error type) are listed in the [SDK migration guide](sdk-migration-guide.md#v1x--v20-planned), along with the shipped v0.x → v1.0 changes and how to test a migration.

---

## Error Codes

### Where can I find the full error code list?

See [docs/error-codes.md](error-codes.md) for the complete reference of every contract error code with causes and remediation steps.

### What does `check-error-map-sync.ts` do?

It verifies that the SDK's `FLUXAPAY_CONTRACT_ERROR_MAP` matches the Rust `Error` enum. This runs in CI and prevents drift between the contract and SDK. See [scripts/README.md](../scripts/README.md) for details.
