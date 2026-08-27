# @fluxapay/sdk

Official TypeScript SDK for interacting with FluxaPay's Soroban smart contracts on the Stellar network.

## Installation

```bash
npm install @fluxapay/sdk
```

## Release Notes

See [CHANGELOG.md](./CHANGELOG.md) for version history.

Upgrading between major versions? See the
[SDK Migration Guide](../docs/sdk-migration-guide.md) for breaking changes
and before/after code snippets.

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
  });

  console.log("Payment created:", payment);

  // Get payment status
  const status = await client.getPayment("pay_123");
  console.log("Payment status:", status);
}
```

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
const paymentDisputes = await client.getPaymentDisputes("pay_123");
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
```

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

## License

MIT

## Publishing

Releases are published to npm when a version tag is pushed:

```bash
git tag sdk/v0.1.0
git push origin sdk/v0.1.0
```

The [SDK Release](https://github.com/MetroLogic/fluxapay_contract/actions/workflows/sdk-release.yml) workflow builds, tests, and publishes `@fluxapay/sdk`. Requires `NPM_TOKEN` in GitHub repository secrets (npm automation token with publish access to the `@fluxapay` scope).
