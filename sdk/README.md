# @fluxapay/sdk

Official TypeScript SDK for interacting with FluxaPay's Soroban smart contracts on the Stellar network.

## Installation

```bash
npm install @fluxapay/sdk
```

### Browser vs Node.js

| Environment | Import | Notes |
|-------------|--------|-------|
| Browser / bundlers | `import { FluxapayClient } from "@fluxapay/sdk"` | Uses the default build; relies on the runtime `fetch`. |
| Node.js 18+ (scripts, daemons, backends) | `import { FluxapayClient } from "@fluxapay/sdk/node"` | Applies Node-friendly Stellar SDK defaults (`setAllowHttp`, native `fetch`). No caller-side polyfill needed. |

```typescript
// Node.js
import { FluxapayClient } from "@fluxapay/sdk/node";

const client = new FluxapayClient({
  network: "testnet",
  rpcUrl: "https://soroban-testnet.stellar.org",
  contractId: "C...",
});
```

The browser import path is unchanged and unaffected by the `/node` entry.
## Payment Receipts (Issue #816)

After a payment is confirmed, generate a signed, shareable receipt:

```ts
const client = new FluxapayClient({
  network: "testnet",
  contractId: "C...",
  platformSigningKey: process.env.FLUXAPAY_PLATFORM_SECRET!, // S...
  platformPublicKey: process.env.FLUXAPAY_PLATFORM_PUBLIC,   // G... (optional)
  receiptBaseUrl: "https://receipts.fluxapay.io",            // optional
});

const receipt = await client.generateReceipt(paymentId);
// receipt.receipt_url → https://receipts.fluxapay.io/r/{payment_id}
// receipt.proof → base64 Ed25519 signature over canonical fields

import { verifyReceipt } from "@fluxapay/sdk";
verifyReceipt(receipt, platformPublicKey); // pure, no network
```

**Receipt URL format:** `{receiptBaseUrl}/r/{payment_id}`  
**Signed fields:** `payment_id`, `amount`, `merchant_name`, `confirmed_at`, `tx_hash`

## Release Notes

See [CHANGELOG.md](./CHANGELOG.md) for version history.

Upgrading between major versions? See the
[SDK Migration Guide](../docs/sdk-migration-guide.md) for breaking changes
and before/after code snippets.

Running testnet integration tests? See the
[Integration Test Guide](../docs/integration-test-guide.md) for setup
and prerequisites.

## Quick Start

```typescript
import { FluxapayClient } from "@fluxapay/sdk";

const client = new FluxapayClient({
  network: "testnet",
  rpcUrl: "https://soroban-testnet.stellar.org",
  contractId: "C...", // PaymentProcessor contract ID
  merchantRegistryContractId: "C...", // MerchantRegistry contract ID (optional)
});

async function main() {
  // Create a payment with full CreatePaymentArgs support
  const payment = await client.createPayment({
    paymentId: "pay_123",
    merchantId: "G...",
    amount: 1000000n, // 1 USDC
    currency: "USDC",
    depositAddress: "G...",
    expiresAt: BigInt(Math.floor(Date.now() / 1000) + 3600),
    durationSecs: 3600n,           // optional: alternative to expiresAt
    memo: "Order #42",             // optional
    memoType: "Text",              // optional: Text | Id | Hash | Return
    tokenAddress: "C...",          // optional: custom token
    clientToken: "idempotency-key", // optional: idempotency key
    idempotencyKey: "retry-key-123", // optional: 24-hour retry key
  });

  console.log("Payment created:", payment);

  // Get payment status
  const status = await client.getPayment("pay_123");
  console.log("Payment status:", status);
}
```

## Error Handling

The SDK translates all Soroban contract error codes into typed JavaScript `FluxapayError` subclasses (such as `KycLimitExceededError`, `PaymentExpiredError`, `PaymentNotFoundError`, etc.), allowing developers to catch specific error conditions using standard `instanceof` checks.

For the full list of error classes, corresponding contract error codes, and handling guidance, see the **[Error Reference Guide](./docs/error-reference.md)**.

```typescript
import {
  FluxapayError,
  KycLimitExceededError,
  PaymentExpiredError,
} from "@fluxapay/sdk";

try {
  await client.createPayment({ ... });
} catch (error) {
  if (error instanceof KycLimitExceededError) {
    console.error("KYC limit exceeded. Upgrade merchant verification.");
  } else if (error instanceof PaymentExpiredError) {
    console.error("Payment has expired.");
  } else if (error instanceof FluxapayError) {
    console.error(`Contract error #${error.code} (${error.contractErrorName}):`, error.localizedMessage);
  } else {
    throw error;
  }
}
```

## On-Chain Event Types & Parsing (Issue #765)

The SDK exports typed interfaces for all on-chain events emitted across FluxaPay contracts, plus a `parseFluxapayEvent` helper that discriminates raw Soroban RPC or Horizon event streams into strongly typed events.

```typescript
import {
  parseFluxapayEvent,
  type FluxapayEvent,
  type PaymentCreatedEvent,
  type RefundCompletedEvent,
  type StreamWithdrawnEvent,
} from "@fluxapay/sdk";

// Listen to Horizon or Soroban RPC events
for (const rawEvent of eventsFromRpc) {
  const event: FluxapayEvent = parseFluxapayEvent(rawEvent);

  switch (event.type) {
    case "PAYMENT/CREATED":
      // TypeScript automatically narrows payload to PaymentCreatedPayload
      console.log(`Payment created: ${event.payload.payment_id} for ${event.payload.amount} stroops`);
      break;

    case "REFUND/COMPLETED":
      console.log(`Refund ${event.payload.refund_id} completed: ${event.payload.refund_amount} stroops`);
      break;

    case "STREAM/WITHDRAWN":
      console.log(`Stream ${event.payload.stream_id} withdrawn: ${event.payload.amount} (memo: ${event.payload.memo})`);
      break;

    case "ACCESS_CONTROL/ADMIN_TRANSFER_PROPOSED":
      console.log(`Admin transfer proposed for ${event.payload.new_admin} at ledger ${event.payload.earliest_acceptance_ledger}`);
      break;

    default:
      console.log(`Event: ${event.type}`, event.payload);
  }
}
```

You can also import from the dedicated `events` namespace:

```typescript
import { events } from "@fluxapay/sdk";

const parsed = events.parseFluxapayEvent(rawEvent);
```

## Bulk payment status

Reconciling a batch of orders with `getPayment` in a loop costs N sequential RPC
round trips — latency grows with the order book. `getPaymentStatuses` fans the
reads out concurrently instead:

```typescript
const statuses = await client.getPaymentStatuses([
  "pay_001",
  "pay_002",
  "pay_003",
]);

// Map<string, PaymentStatusValue | null>
for (const [id, status] of statuses) {
  if (status === null) {
    console.warn(`${id}: no such payment`);
  } else {
    console.log(`${id}: ${JSON.stringify(status)}`);
  }
}
```

**A missing payment is `null`, not an error.** A merchant checking 50 orders
should not lose the other 49 because one ID was mistyped. Anything *other* than
not-found — an RPC outage, an auth failure — is rethrown, because silently
reporting "these 50 orders do not exist" would be far worse than an error.

**Capped at 50 IDs**, enforced client-side before any request:

```typescript
import { BatchTooLargeError, MAX_BATCH_STATUS_IDS } from "@fluxapay/sdk";

try {
  await client.getPaymentStatuses(tooMany);
} catch (err) {
  if (err instanceof BatchTooLargeError) {
    console.error(`Split into chunks of ${MAX_BATCH_STATUS_IDS}`);
  }
}
```

Duplicate IDs are collapsed into a single read; the returned Map is keyed by ID
either way.

> The reads are issued concurrently against the same RPC rather than as one
> contract invocation. A true single-call batch needs an on-chain view taking a
> vector of IDs, and `get_payment` takes one — so batching on-chain would mean a
> contract change and a redeploy. This gets the latency win without that. If
> such a view lands later, the method signature does not change; only its body
> does.

## Contract IDs

Every network environment (`mainnet`, `testnet`, `standalone`) has a canonical
set of deployed contract addresses exported as `FLUXAPAY_CONTRACT_IDS`:

```typescript
import { FLUXAPAY_CONTRACT_IDS } from "@fluxapay/sdk";

FLUXAPAY_CONTRACT_IDS.testnet.paymentProcessor;
FLUXAPAY_CONTRACT_IDS.testnet.refundManager;
FLUXAPAY_CONTRACT_IDS.testnet.merchantRegistry;
FLUXAPAY_CONTRACT_IDS.testnet.fxOracle;
FLUXAPAY_CONTRACT_IDS.testnet.paymentLinkManager;
```

`FluxapayClient` reads from this map automatically whenever a `*ContractId`
field is omitted from its config, so you only need to pass explicit contract
IDs when overriding the default deployment (e.g. testing against a locally
deployed contract):

```typescript
// Uses FLUXAPAY_CONTRACT_IDS.testnet.paymentProcessor automatically —
// no contractId needed.
const client = new FluxapayClient({ network: "testnet" });
```

Until the mainnet contracts are deployed, every `FLUXAPAY_CONTRACT_IDS.mainnet.*`
entry is set to the `UNSET_CONTRACT_ID` placeholder; constructing a client (or
calling `fxOracle()` / merchant-registry / payment-link methods) against
`mainnet` without an explicit override throws a clear configuration error
rather than making an RPC call to a nonsense address. CI runs
`scripts/check-mainnet-contract-ids.js` on every build, which prints a warning
(without failing the build) listing any mainnet fields still left as
placeholders — a reminder to update `sdk/src/network-profiles.ts` once the
mainnet deployment lands.

## Features

- **High-level Wrapper**: `FluxapayClient`, `RefundManagerClient`, `MerchantRegistryClient`, and `FxOracleClient` simplify complex contract interactions.
- **Typed Interfaces**: Full TypeScript support for all contract models (`Merchant`, `PaymentCharge`, `Refund`, `FeeConfig`, etc.).
- **Automatic Simulation**: Built-in support for Soroban transaction simulation.
- **Network Presets**: Easy switching between `testnet` and `mainnet`.
- **SEP-10 Authentication**: Merchant API access via Stellar Web Authentication standard.

## SEP-10 Merchant Authentication

Authenticate merchants using their Stellar keypair via Stellar SEP-10 Web Authentication:

```typescript
import { FluxapayClient } from "@fluxapay/sdk";
import { Keypair } from "@stellar/stellar-sdk";

const client = new FluxapayClient({
  network: "testnet",
  rpcUrl: "https://soroban-testnet.stellar.org",
  contractId: "C...",
});

// Initialize SEP-10 authenticator (server keypair should be stored securely)
client.initSEP10(
  "GBRPYHIL2CI3WHZDTOOQFC6EB4RRJC3XVCDTUJ76ZAE2QL4LFD5TWUC",
  "fluxapay.stellar.org"
);

// 1. Get challenge for a merchant keypair
const merchantKeypair = Keypair.random();
const challenge = client.generateSEP10Challenge(merchantKeypair.publicKey());

// 2. Merchant signs the challenge
const signedChallenge = merchantKeypair.sign(
  Buffer.from(challenge.challenge, "base64")
).toString("base64");

// 3. Client verifies signature and returns JWT
const { jwt } = client.authorizeSEP10(
  challenge.challenge,
  signedChallenge,
  merchantKeypair.publicKey()
);

console.log("JWT for API access:", jwt);

// 4. Include JWT in Authorization header for API calls
const headers = {
  "Authorization": `Bearer ${jwt}`
};
```

## Merchant Management (FluxapayClient)

Register and manage merchants directly through `FluxapayClient`. Pass `merchantRegistryContractId` in config to target the dedicated MerchantRegistry contract.

### Register without custom fee

```typescript
await client.registerMerchant({
  merchantId: "G...",
  businessName: "Acme Corp",
  settlementCurrency: "USDC",
  payoutAddress: "G...",
});
```

### Register with custom FeeConfig

```typescript
import { FluxapayClient, FeeConfig } from "@fluxapay/sdk";

const feeConfig: FeeConfig = {
  platform_fee_bps: 200n,   // 2%
  fixed_fee: 100000n,       // 0.01 USDC fixed fee
  fee_recipient: "G...",    // optional custom recipient
};

await client.registerMerchant({
  merchantId: "G...",
  businessName: "Acme Corp",
  settlementCurrency: "USDC",
  payoutAddress: "G...",
  feeConfig,
});
```

### Update, verify, and query merchants

```typescript
// Update merchant settings (including fee config)
await client.updateMerchant({
  merchantId: "G...",
  businessName: "Updated Corp Name",
  settlementCurrency: "EUR",
  feeConfig: {
    platform_fee_bps: 150n,
    fixed_fee: 0n,
    fee_recipient: undefined,
  },
});

// Verify merchant (admin only)
await client.verifyMerchant("G...", "G..."); // admin, merchantId

// Get merchant details
const merchant = await client.getMerchant("G...");
console.log("Merchant:", merchant);
```

## Refunds and Disputes (FluxapayClient)

```typescript
// Create a refund request
const refundTx = await client.createRefund({
  paymentId: "pay_123",
  amount: 500000n,
  reason: "Damaged goods",
  requester: "G...",
});

// Process a pending refund (operator)
await client.processRefund("G...", "refund_001");

// Query refunds
const refund = await client.getRefund("refund_001");
const paymentRefunds = await client.getPaymentRefunds("pay_123");

// Create a dispute
const disputeTx = await client.createDispute({
  paymentId: "pay_123",
  amount: 500000n,
  reason: "Unauthorized charge",
  evidence: "ipfs://...",
  disputer: "G...",
});

// Dispute lifecycle (operator)
await client.reviewDispute("G...", "dispute_001");
await client.resolveDisputeWithRefund("G...", "dispute_001", "Refund approved");
// or: await client.rejectDispute("G...", "dispute_001", "Insufficient evidence");

// Query disputes
const dispute = await client.getDispute("dispute_001");
const allDisputes = await client.getPaymentDisputes("pay_123");
// Filter disputes by status (Issue #575)
const openDisputes = await client.getPaymentDisputes("pay_123", "Open");
const resolvedDisputes = await client.getPaymentDisputesByStatus("pay_123", "Resolved");
```

## Partial / Overpaid Payments (FluxapayClient)

When `verifyPayment` sees an amount that doesn't match the expected total, the
payment moves to `PaymentStatus.PartiallyPaid` (underpaid) or
`PaymentStatus.Overpaid` (overpaid) instead of `Confirmed`.

```typescript
// Merchant accepts the partial amount actually received (no refund issued
// for the shortfall) — moves the payment to Confirmed.
await client.acceptPartialPayment("G...MERCHANT", "pay_123");

// Customer tops up a PartiallyPaid payment instead — moves it back to
// Pending so a following verifyPayment call can confirm it with the
// combined amount.
await client.completePartialPayment("G...CUSTOMER", "pay_123", 250000n);
```

Both calls throw `FluxapayError` with `contractErrorName: "PaymentAlreadyProcessed"`
if the payment isn't currently `PartiallyPaid`.
## Compliance / Admin Tooling (FluxapayClient)

Blacklist management for blocking fraudulent payers, merchants, or requesters.
`addToBlacklist` / `removeFromBlacklist` require the PaymentProcessor `ADMIN`
role; `isBlacklisted` is a read-only call with no authorization required.
Blacklisted addresses are rejected on subsequent payment, refund, and
dispute operations.

```typescript
// Block an address (admin only)
await client.addToBlacklist("G...ADMIN", "G...FRAUDULENT_ADDRESS");

// Check blacklist status (no auth required)
const blocked = await client.isBlacklisted("G...FRAUDULENT_ADDRESS"); // true

// Unblock an address (admin only)
await client.removeFromBlacklist("G...ADMIN", "G...FRAUDULENT_ADDRESS");
```

## Treasury / Platform Fee Reporting (FluxapayClient)

`getPlatformFeeReport` aggregates platform fee collection over a queried
time period `[fromTs, toTs]` (ledger timestamps, in seconds) for treasury
reporting. Read-only — no authorization required.

```typescript
const report = await client.getPlatformFeeReport(1700000000n, 1700086400n);
// { totalFeesCollected, treasuryShare, developerShare, paymentCount }
```

## Merchant Analytics (FluxapayClient)

All three calls are read-only — no authorization required.

`getMerchantPaymentCount` returns the O(1) count of payments created for a
merchant (backed by the on-chain `MerchantPaymentCount` index), for dashboard
pagination:

```typescript
const count = await client.getMerchantPaymentCount(merchantId); // number
```

`getMerchantAnalytics` aggregates a merchant's activity over a time window
`[fromTimestamp, toTimestamp]` (inclusive, ledger seconds). Omit `toTimestamp`
(or pass `0`) for all-time figures — the client then sends the contract's
`u64::MAX` sentinel so it scans the merchant index directly:

```typescript
const analytics = await client.getMerchantAnalytics(merchantId, 0);
// or a bounded window:
// await client.getMerchantAnalytics(merchantId, 1700000000, 1702592000);
// MerchantAnalytics:
// {
//   totalPayments, confirmedPayments, failedPayments,
//   totalVolume, averageAmount,
//   disputeCount, refundCount, netSettledVolume
// }
```

`getTopMerchants` ranks merchants by cumulative gross payment volume across the
whole platform (operator-level reporting). It reads only the volume/count
indexes — never individual payments — and the contract caps `limit` at 100
(`limit = 0` returns up to the cap). Results are ordered by `totalVolume`
descending:

```typescript
const leaderboard = await client.getTopMerchants(10);
// MerchantRanking[]: [{ merchantId, totalVolume, paymentCount }, ...]
```

## Collaborative Dispute Settlement (issue #665)

When the buyer and merchant agree on a settlement amount off-chain, they can
close the dispute instantly by each signing the settlement with Ed25519
instead of waiting on operator/arbitrator review:

```typescript
// Both parties sign SHA-256(dispute_id || settlement_amount_le16) off-chain
// and hand their signatures to whichever party submits the transaction.
const refundId = await client.settleDisputeCollaboratively({
  disputeId: "dispute_001",
  settlementAmount: 250_000n,
  buyerPubkey: buyerPubkeyBytes, // 32-byte Ed25519 public key
  signatureBuyer: buyerSigBytes, // 64-byte Ed25519 signature
  merchantPubkey: merchantPubkeyBytes,
  signatureMerchant: merchantSigBytes,
});

// Look up the recorded settlement (null if none exists / dispute not found).
const settlement = await client.getCollaborativeSettlement("dispute_001");
```

An invalid or mismatched signature surfaces as a mapped `InvalidSettlementSignature`
`FluxapayError` (see `docs/error-codes.md`).

## Subscription Management

Create a merchant plan, subscribe a payer, and let an authorized billing
operator process charges when they become due:

```typescript
const planId = await client.createSubscriptionPlan({
  merchant: "GMERCHANT...",
  planId: "pro_monthly",
  name: "Pro",
  description: "Monthly Pro subscription",
  amount: 2_000_000n,
  currency: "USDC",
  billingInterval: "Monthly",
});

const subscriptionId = await client.subscribe({
  payer: "GPAYER...",
  planId,
  maxPayments: 12,
});

await client.chargeSubscription("GBILLING_OPERATOR...", subscriptionId);
const subscription = await client.getSubscription(subscriptionId);
// Pause, resume, or cancel from the payer (or merchant for cancellation).
await client.pauseSubscription("GPAYER...", subscriptionId);
```

`getPayerSubscriptions(payer)` returns all subscriptions associated with a
payer. Subscription charge failures surface the mapped
`SubscriptionInGracePeriod` and `SubscriptionRetryExhausted` errors.

## Usage-Based Billing (Metered Subscriptions) (issue #664)

For pay-per-use subscriptions, an operator (oracle or settlement-operator
role) reports usage units for a billing cycle; the subscription's charge
amount is overridden to `units * unitPrice` and charged immediately:

```typescript
await client.submitUsageMetrics({
  subscriptionId: "sub_123",
  units: 1_500n,
  unitPrice: 100n, // smallest unit of the subscription's token
  token: "C...",
  caller: "G_operator...",
});

// Query usage history recorded for a subscription in a time range.
const history = await client.getUsageMetrics(
  "sub_123",
  Math.floor(Date.now() / 1000) - 30 * 24 * 60 * 60, // 30 days ago
  Math.floor(Date.now() / 1000),
);
```

Submitting metrics for a Cancelled/Expired subscription is rejected with a
mapped `InvalidStatusTransition` `FluxapayError`.

## Merchant Pre-Authorization (Pull Billing)

`MerchantPreAuth` lets a customer grant a merchant permission to pull up to a
fixed amount per billing period — useful for SaaS-style recurring charges
without requiring a fresh signature on every charge.

```typescript
// Customer grants the merchant a $50/30-day pull allowance.
const auth = await client.preAuthorizeMerchant({
  customer: "GCUSTOMER...",
  merchant: "GMERCHANT...",
  token: "CUSDC...",
  limitPerPeriod: 50_000_000n, // 50 USDC (7 decimals)
  periodSecs: 2_592_000n, // 30 days
});

// Merchant pulls a charge against the authorization. Returns the
// cumulative amount pulled so far in the current period.
const pulledThisPeriod = await client.pullFromAuthorization(
  "GMERCHANT...",
  "GCUSTOMER...",
  10_000_000n, // 10 USDC
);

// Look up the current authorization (null if none exists).
const current = await client.getAuthorization("GCUSTOMER...", "GMERCHANT...");

// Customer revokes the authorization at any time.
await client.revokeAuthorization("GCUSTOMER...", "GMERCHANT...");
```

Billing periods reset automatically: once `now >= period_start + period_secs`,
the next `pullFromAuthorization` call resets `pulled_this_period` to 0 and
emits a `MERCHANT_AUTH/PERIOD_RESET` event before applying the pull, so a new
period always starts with the full `limitPerPeriod` available regardless of
how many periods were skipped with no activity.

## RefundManagerClient

The `RefundManagerClient` provides methods for managing refunds on a dedicated RefundManager contract:

```typescript
import { RefundManagerClient } from "@fluxapay/sdk";

const refundClient = new RefundManagerClient({
  network: "testnet",
  rpcUrl: "https://soroban-testnet.stellar.org",
  contractId: "C...", // RefundManager contract ID
});

async function handleRefund() {
  const refundId = await refundClient.createRefund(
    "payment_123",
    500000n,
    "Damaged goods",
    "G...",
  );

  const refund = await refundClient.getRefund(refundId);
  await refundClient.processRefund("G...", refundId);
  const allRefunds = await refundClient.getPaymentRefunds("payment_123");
}
```

## MerchantRegistryClient

The standalone `MerchantRegistryClient` is also available for direct registry access:

```typescript
import { MerchantRegistryClient, FeeConfig } from "@fluxapay/sdk";

const merchantClient = new MerchantRegistryClient({
  network: "testnet",
  rpcUrl: "https://soroban-testnet.stellar.org",
  contractId: "C...",
});

// Without fee config
await merchantClient.registerMerchant({
  merchantId: "merchant_001",
  businessName: "Acme Corp",
  settlementCurrency: "USDC",
});

// With fee config
const feeConfig: FeeConfig = {
  platform_fee_bps: 100n,
  fixed_fee: 50000n,
  fee_recipient: undefined,
};

await merchantClient.registerMerchant({
  merchantId: "merchant_002",
  businessName: "Beta Inc",
  settlementCurrency: "USDC",
  feeConfig,
});

await merchantClient.verifyMerchant("G...", "merchant_001");
await merchantClient.updateMerchant({
  merchantId: "merchant_001",
  businessName: "Updated Corp Name",
});
```

### Payment tolerance configuration (issue #529 / #630)

Payment tolerance is the amount an incoming payment may fall short of the
requested amount and still be accepted (smallest currency unit). There is a
global default plus an optional per-merchant override; the contract caps the
effective tolerance at 1% of each payment amount.

The setters and getters are exposed on both `FluxapayClient` and the standalone
`MerchantRegistryClient` with identical signatures:

```typescript
// Global default — signed by the MerchantRegistry admin.
// An unauthorized signer throws a mapped `Unauthorized` FluxapayError.
await client.setGlobalPaymentTolerance("G...ADMIN", 100n);
const globalTolerance = await client.getGlobalPaymentTolerance(); // bigint

// Per-merchant override — signed by the merchant. Pass null to clear it and
// fall back to the global default.
await client.setMerchantPaymentTolerance("merchant_001", 250n);
await client.setMerchantPaymentTolerance("merchant_001", null);

// Effective value: the merchant override if set, otherwise the global default.
const effective = await client.getMerchantPaymentTolerance("merchant_001"); // bigint
```

## FxOracleClient

The `FxOracleClient` provides methods for querying and publishing FX exchange rates.

### Standalone client

```typescript
import { FxOracleClient } from "@fluxapay/sdk";

const oracleClient = new FxOracleClient({
  network: "testnet",
  rpcUrl: "https://soroban-testnet.stellar.org",
  oracleContractId: "C...",
});

const rate = await oracleClient.getRate("USDCNGN");
const settlementAmount = await oracleClient.getSettlementAmount(1_000_000n, "NGN");
```

### Via FluxapayClient

```typescript
const client = new FluxapayClient({
  network: "testnet",
  contractId: "C...",
  oracleContractId: "C...",
});

const oracle = client.fxOracle();
const rate = await oracle.getRate("USDCNGN");
```

## Payment Links (FluxapayClient)

Payment links let merchants share a reusable URL that payers can settle against. Pass `paymentLinkContractId` in config to enable these methods.

### Create a payment link

```typescript
import { FluxapayClient } from "@fluxapay/sdk";

const client = new FluxapayClient({
  network: "testnet",
  contractId: "C...",
  paymentLinkContractId: "C...", // PaymentLinkManager contract ID
});

// Fixed-amount link
const linkId = await client.createLink({
  merchant: "G...",
  amount: 5_000_000n, // 0.5 USDC (7 decimals)
  usdcToken: "C...",
  // metadata: ≤20 keys; key ≤64 chars; value ≤256 chars
  metadata: { product: "Coffee", ref: "order_42" }, // optional
  baseUrl: "https://pay.example.com", // optional → shareable_url
});
console.log("Link created:", linkId);

// Prefer createPaymentLink when you need QR / shareable URL
const { linkId: payLinkId, shareableUrl, qrCodeData } = await client.createPaymentLink({
  merchant: "G...",
  amount: 5_000_000n,
  usdcToken: "C...",
  baseUrl: "https://pay.example.com",
});
console.log(shareableUrl, qrCodeData);

// Open-amount link (payer sets the amount)
const openLinkId = await client.createLink({
  merchant: "G...",
  usdcToken: "C...",
});
```

### Use a payment link

```typescript
await client.useLink(
  "G...",        // payer address
  linkId,        // link ID returned by createLink
  5_000_000n,    // amount in stroops
  "C...",        // USDC token contract address
);
```

### Retrieve and verify links

```typescript
// Fetch a single link
const link = await client.getLink(linkId);
console.log("Link active:", link.active);
console.log("Merchant:", link.merchant);
console.log("Metadata:", link.metadata);

// Batch-verify multiple links (returns only active link IDs)
const activeLinkIds = await client.verifyBatch([linkId, openLinkId, "C_other..."]);
console.log("Active links:", activeLinkIds);
```

### Deactivate a link

```typescript
// Only the merchant that created the link can deactivate it
await client.deactivateLink("G...", linkId);
```

### Per-link and global fee overrides (issue #663)

By default, payments collected via a link don't have any link-level fee
deducted. An admin can override this per-link (e.g. a promotional 0-fee
link) or set a contract-wide default that applies to any link without its
own override — available on the standalone `PaymentLinkManagerClient`:

```typescript
// Zero-fee promotional link: overrides take precedence over the global default.
await linkClient.setPaymentLinkFeeBps("G_admin...", linkId, 0n);

// Contract-wide default fee (500 bps = 5%) for links with no override.
await linkClient.setPaymentLinkFeeBps("G_admin...", null, 500n);

// Inspect what fee would currently apply to a link.
const feeBps = await linkClient.getEffectiveFeeBps(linkId);
```

### Standalone PaymentLinkManagerClient

```typescript
import { PaymentLinkManagerClient } from "@fluxapay/sdk";

const linkClient = new PaymentLinkManagerClient({
  network: "testnet",
  rpcUrl: "https://soroban-testnet.stellar.org",
  contractId: "C...", // PaymentLinkManager contract ID
});

const linkId = await linkClient.createLink({
  merchant: "G...",
  amount: 10_000_000n,
  usdcToken: "C...",
  metadata: { item: "Widget" },
});

const link = await linkClient.getLink(linkId);
await linkClient.useLink("G_payer...", linkId, 10_000_000n, "C...");
await linkClient.deactivateLink("G_merchant...", linkId);
const active = await linkClient.verifyBatch([linkId]);
```

## Payment Streams

`FluxapayClient` exposes wrappers for continuous payment streaming (`PaymentProcessor.create_stream` and related on-chain methods).

```typescript
const stream = await client.createStream({
  sender: "G_SENDER...",
  receiver: "G_RECEIVER...",
  token: "C_USDC_TOKEN...",
  ratePerSecond: 100n,
  deposit: 1_000_000n,
  streamId: "stream_001",
});

await client.topUpStream("G_SENDER...", "stream_001", 500_000n);
await client.pauseStream("G_SENDER...", "stream_001");
await client.resumeStream("G_SENDER...", "stream_001");

// Withdraw everything accrued so far to the receiver
await client.withdrawStream("G_RECEIVER...", "stream_001");

const details = await client.getStream("stream_001");
const senderStreams = await client.getSenderStreams("G_SENDER...");

await client.cancelStream("G_SENDER...", "stream_001");

// Batch stream operations (Issue #576)
// Top up multiple streams in a single transaction
await client.topUpMultipleStreams("G_SENDER...", [
  { streamId: "stream_001", amount: 250_000n },
  { streamId: "stream_002", amount: 500_000n },
]);

// Batch withdraw accrued balances from multiple streams
await client.batchWithdrawTo("G_RECEIVER...", ["stream_001", "stream_002"]);

// Cancel multiple streams in a single transaction
await client.cancelMultipleStreams("G_SENDER...", ["stream_001", "stream_002"]);
```

## Swap Payments (DEX Routing)

Merchants who accept payments via Soroban DEX swap routing can have payers execute token swaps and payments atomically via `FluxapayClient` (Issue #577).

```typescript
// Single-route swap-and-pay
const charge = await client.swapAndPay({
  payer: "G_PAYER...",
  merchantId: "G_MERCHANT...",
  paymentId: "swap_pay_001",
  dexRouter: "C_DEX_ROUTER...",
  path: ["C_TOKEN_IN...", "C_USDC_TOKEN..."],
  amountIn: 10_000_000n,
  amountOutMin: 9_900_000n,
  deadline: Math.floor(Date.now() / 1000) + 3600,
});

// Multi-route split swap-and-pay
const multiCharge = await client.swapAndPayMultiRoute({
  payer: "G_PAYER...",
  merchantId: "G_MERCHANT...",
  paymentId: "multi_swap_pay_001",
  dexRouter: "C_PRIMARY_ROUTER...",
  path: ["C_TOKEN_IN...", "C_USDC_TOKEN..."],
  amountIn: 10_000_000n,
  amountOutMin: 9_900_000n,
  routes: [
    {
      router: "C_DEX_ROUTER_1...",
      path: ["C_TOKEN_IN...", "C_USDC_TOKEN..."],
      amountIn: 5_000_000n,
    },
    {
      router: "C_DEX_ROUTER_2...",
      path: ["C_TOKEN_IN...", "C_USDC_TOKEN..."],
      amountIn: 5_000_000n,
    },
  ],
});
```

### Swap Error Handling

Swap failures map directly to typed `FluxapayError` subclasses:

| Error Class | Code | Reason |
|-------------|------|--------|
| `ArbitrageDetectedError` | `#27` | DEX router swap slippage or price sandwich detected. |
| `SwapPathInvalidError` | `#28` | Route swap path between tokens is invalid or empty. |
| `OraclePriceDeviationError` | `#29` | DEX swap quote deviates from oracle price beyond tolerance threshold. |


## Gas Estimation

`GasEstimatorClient` queries the on-chain `GasEstimator` contract for predicted Soroban resource costs (instructions, ledger reads/writes, events, and resource fee in stroops) before submitting a transaction.

```typescript
import { GasEstimatorClient } from "@fluxapay/sdk";

const gasEstimator = new GasEstimatorClient({
  network: "testnet",
  gasEstimatorContractId: "C...", // GasEstimator contract ID
});

const estimate = await gasEstimator.estimate("CreatePayment");
console.log(estimate.resourceFeeStroops);

const allEstimates = await gasEstimator.estimateAll();
```

## Offline / Hardware Wallet Signing

`FluxapayClient.offlineSigner()` returns a `FluxapayOfflineSigner` that builds unsigned transaction payloads (XDR + JSON snapshot + required signers) for offline or hardware-wallet signing workflows, without submitting them.

Supported operations: `create_payment`, `verify_payment`, `create_refund`, and (for backend billing services) `charge_subscription` and `pull_payment`.

```typescript
const signer = client.offlineSigner();

// Pre-build a subscription tick for batch submission or hardware-wallet signing.
const tickPayload = await signer.buildSubscriptionTick({
  operator: "G_OPERATOR...",
  subscriptionId: "sub_123",
  token: "C_USDC_TOKEN...",
});

// Pre-build a pre-authorized pull payment.
const pullPayload = await signer.buildPullAuthorization({
  merchant: "G_MERCHANT...",
  customer: "G_CUSTOMER...",
  amount: 5_000_000n,
});

// Each payload contains `unsignedXdr`, `hash`, `json`, and `requiredAuthSigners`.
// Sign `unsignedXdr` offline, then restore + submit:
const restored = signer.restore(tickPayload);
```

You can also use the standalone builder functions directly: `buildSubscriptionTickPayload`, `buildPullAuthorizationPayload`, `buildCreatePaymentPayload`, `buildVerifyPaymentPayload`, `buildCreateRefundPayload`.

## Scoped API Keys

Issue scoped API keys to restrict integrations or services to specific capabilities:

```typescript
// Create a scoped API key
const keyRecord = await client.createApiKey({
  merchant: "G_MERCHANT...",
  keyHash: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  scopes: ["read:payments", "write:payments"],
});

// Retrieve an API key
const record = await client.getApiKey(keyHash);

// Revoke an API key
await client.revokeApiKey("G_MERCHANT...", keyHash);
```

Available scopes:
- `read:payments`: Query payments, refunds, and disputes.
- `write:payments`: Create payments and charge authorizations.
- `read:analytics`: Query payment metrics and events.
- `manage:webhooks`: Manage webhook registrations.
- `admin`: Full unrestricted access.

## License


MIT

## Publishing

Releases are published to npm when a version tag is pushed:

```bash
git tag sdk/v0.1.0
git push origin sdk/v0.1.0
```

The [SDK Release](https://github.com/MetroLogic/fluxapay_contract/actions/workflows/sdk-release.yml) workflow builds, tests, and publishes `@fluxapay/sdk`. Requires `NPM_TOKEN` in GitHub repository secrets (npm automation token with publish access to the `@fluxapay` scope).
