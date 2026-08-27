# Merchant Quickstart: API Key to First Payment

This guide takes you from zero to receiving your first confirmed USDC payment on Stellar testnet, end to end: install the SDK, register a merchant, create a payment charge, share the payment link, listen for the `payment.confirmed` webhook, and verify settlement.

## 0. Prerequisites

- Node.js 18+
- A Stellar testnet keypair for your merchant account (create one at [Stellar Laboratory — Create Account](https://laboratory.stellar.org/#account-creator?network=test))
- Testnet XLM funded via [Friendbot](https://laboratory.stellar.org/#account-creator?network=test) (click "Get test network lumens")
- The FluxaPay `PaymentProcessor` and `MerchantRegistry` contract IDs for testnet (see [DEPLOYMENT.md](../DEPLOYMENT.md) or your `.env.testnet` if you deployed your own instance)

## 1. Authenticate with SEP-10 (Issue #675)

Before calling the FluxaPay API, exchange your Stellar keypair for a JWT
using the backend's SEP-10 endpoints (see [backend/README.md](../backend/README.md)
for the full reference):

```bash
# 1. Request a challenge transaction for your merchant account
curl "https://api.fluxapay.example/auth/challenge?account=GABC...YOUR_ACCOUNT"

# 2. Sign the returned transaction XDR with your Stellar keypair (client-side),
#    then exchange it for a JWT
curl -X POST https://api.fluxapay.example/auth/token \
  -H "Content-Type: application/json" \
  -d '{"transaction": "<signed XDR>", "account": "GABC...YOUR_ACCOUNT"}'
```

The response is a JWT containing `merchant_id`, `iat`, and `exp` claims,
valid for 24 hours. Include it as `Authorization: Bearer <token>` on
subsequent API calls. The SDK's `SEP10Authenticator` (`sdk/src/sep10.ts`)
implements the client-side signing/verification flow if you'd rather not
call the REST endpoints directly.

## 2. Install the SDK

```bash
npm install @fluxapay/sdk
```

## 3. Initialize the client

```typescript
import { FluxapayClient } from "@fluxapay/sdk";

const client = new FluxapayClient({
  network: "testnet",
  contractId: process.env.PAYMENT_PROCESSOR_ID!,
  merchantRegistryContractId: process.env.MERCHANT_REGISTRY_ID!,
});
```

## 4. Register your merchant

```typescript
await client.registerMerchant({
  merchantId: "merchant_acme_001",
  businessName: "Acme Co",
  settlementCurrency: "NGN",
  payoutAddress: "GABC...YOUR_PAYOUT_ADDRESS",
});
```

You can confirm registration on-chain in [Stellar Laboratory — Submit Transaction](https://laboratory.stellar.org/#txsigner?network=test) by inspecting the resulting transaction hash, or by calling:

```typescript
const merchant = await client.getMerchant("merchant_acme_001");
console.log(merchant);
```

## 5. Create a payment charge

```typescript
const payment = await client.createPayment({
  paymentId: "payment_abc123",
  merchantId: "merchant_acme_001",
  amount: 10_000_000n, // 1.00 USDC (7 decimals)
  currency: "USDC",
  depositAddress: "GABC...MERCHANT_DEPOSIT_ADDRESS",
  durationSecs: 3600n, // charge expires in 1 hour
});
```

## 6. Share the payment link with your customer

If you have a `PaymentLinkManager` contract ID configured, generate a shareable checkout link and QR payload:

```typescript
const { linkId, shareableUrl, qrCodeData } = await client.createPaymentLink({
  merchant: "GABC...MERCHANT_ADDRESS",
  amount: 10_000_000n,
  usdcToken: process.env.USDC_TOKEN_ADDRESS!,
});

console.log(`Send this link to your customer: ${shareableUrl}`);
```

Your customer opens the link, connects a Stellar wallet, and pays in USDC. To simulate a payment manually on testnet, use [Stellar Laboratory — Build Transaction](https://laboratory.stellar.org/#txbuilder?network=test) to submit a token transfer to your `depositAddress`.

## 7. Listen for the `payment.confirmed` webhook

Configure a webhook endpoint and verify FluxaPay's HMAC signature (see [docs/webhooks.md](webhooks.md) for the full reference):

```typescript
import express from "express";
import crypto from "crypto";

const app = express();
app.use(express.raw({ type: "application/json" }));

app.post("/webhooks/fluxapay", (req, res) => {
  const signature = req.header("X-Fluxapay-Signature") ?? "";
  const expected = crypto
    .createHmac("sha256", process.env.FLUXAPAY_WEBHOOK_SECRET!)
    .update(req.body)
    .digest("hex");

  if (signature !== expected) {
    return res.status(401).send("invalid signature");
  }

  const event = JSON.parse(req.body.toString());
  if (event.type === "payment.confirmed") {
    console.log(`Payment ${event.data.payment_id} confirmed!`);
    // fulfill the order here
  }

  res.status(200).send("ok");
});
```

Alternatively, poll the payment status directly:

```typescript
const status = await client.getPayment("payment_abc123");
console.log(status.status); // "Confirmed" once the oracle verifies the transfer
```

## 8. Verify settlement

Once confirmed, FluxaPay converts and settles the payment to your configured `settlementCurrency`. Verify the payout amount using the FX Oracle:

```typescript
const settlementAmount = await client.fxOracle().getSettlementAmount(
  10_000_000n, // USDC amount received
  "NGN",
);
console.log(`Merchant will be settled ${settlementAmount} NGN`);
```

Settlement events (`payment.settled`) are emitted the same way as `payment.confirmed` — see [docs/webhooks.md](webhooks.md) for the full event list.

## Common errors and fixes

| Error | Cause | Fix |
|---|---|---|
| `PaymentAlreadyExists` (#2) | `paymentId` was already used | Generate a new unique `paymentId` per charge |
| `RateLimitExceeded` (#18) | Too many `create_payment` calls in a short window | Wait for the rate-limit window to reset (60s) or batch requests |
| `UnsupportedToken` (#20) | `tokenAddress` isn't on the merchant's allowlist | Use the default USDC token or register the token first |
| `InvalidExpiry` (#23) | `durationSecs`/`expiresAt` is in the past or unreasonably far out | Use a duration between a few minutes and a few days |
| `MetadataTooLarge` (#49) / `MetadataValueTooLong` (#47) | `metadata` exceeds limits (20 keys, 64-char keys, 256-char values) | Trim metadata to stay within limits |
| Webhook signature mismatch | Wrong `FLUXAPAY_WEBHOOK_SECRET`, or body was parsed/mutated before HMAC check | Use the raw request body (not JSON-parsed) when computing the HMAC |
| `ContractPaused` (#17) | Contract is paused for maintenance | Retry after the maintenance window; check contract status |

## Next steps

- [docs/webhooks.md](webhooks.md) — full webhook payload reference, retries, idempotency
- [docs/architecture.md](architecture.md) — contract structure and payment lifecycle
- [sdk/README.md](../sdk/README.md) — full SDK API reference
