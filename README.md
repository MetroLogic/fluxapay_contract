Fluxapay is a payment gateway on the Stellar blockchain that enables merchants to accept crypto payments and get settled in their local fiat currency.

FluxaPay bridges the gap between crypto payments and real-world commerce—making stablecoin payments as easy to integrate as Stripe.

## Getting Started

New to FluxaPay? Follow the **[Merchant Quickstart Guide](docs/quickstart.md)** to go from SDK install to your first confirmed USDC payment in minutes.

## TypeScript SDK

[![npm version](https://img.shields.io/npm/v/@fluxapay/sdk.svg)](https://www.npmjs.com/package/@fluxapay/sdk)

Install the official SDK from npm:

```bash
npm install @fluxapay/sdk
```

See [sdk/README.md](sdk/README.md) for usage examples and [sdk/CHANGELOG.md](sdk/CHANGELOG.md) for release notes.

## CI/CD

[![CI](https://github.com/MetroLogic/fluxapay_contract/actions/workflows/ci.yml/badge.svg)](https://github.com/MetroLogic/fluxapay_contract/actions/workflows/ci.yml)
[![CD](https://github.com/MetroLogic/fluxapay_contract/actions/workflows/cd.yml/badge.svg)](https://github.com/MetroLogic/fluxapay_contract/actions/workflows/cd.yml)
Automated testing and deployment pipeline using GitHub Actions:

- **CI:** Runs tests, linting, and builds on every push/PR to main
- **CD:** Auto-deploys to development and staging on merge to main; production requires manual approval
- All tests must pass before deployment

### Security and Dependency Checks (Local)

- `cargo audit --deny warnings`
- `cargo deny check bans licenses advisories`

### Bounded Property Tests (Local)

- `PROPTEST_CASES=64 cargo test -p fluxapay proptests:: --all-features -- --test-threads=1`

---

## What Problem does Fluxapay solve?

Despite growing crypto adoption, everyday commerce remains largely fiat-based.

A major pain point is that crypto-native customers are forced to offramp every time they want to pay a merchant. This introduces:

•⁠ ⁠Extra fees from offramping and FX conversions  
•⁠ ⁠Payment delays and failed transactions  
•⁠ ⁠Poor checkout experience for crypto users  
•⁠ ⁠Lost sales for merchants

At the same time, merchants want to accept crypto without holding volatile assets, managing wallets, or dealing with on-chain complexity.

Fluxapay solves this by enabling _USDC-in → fiat-out_ payments with a merchant-friendly experience.

## How FluxaPay Works

1.⁠ ⁠*Merchant Creates a Charge*  
 Merchant creates a payment request via API or Payment Link.

2.⁠ ⁠*Customer Pays in USDC (Stellar)*  
 Customer pays from any supported Stellar wallet.

3.⁠ ⁠*Instant Verification*  
 FluxaPay verifies the payment on-chain and updates the payment status in real-time.

4.⁠ ⁠*Settlement to Merchant (Local Fiat)*  
 FluxaPay converts and settles the value to the merchant’s preferred local currency via bank transfer or supported payout channels.

## Key Features

### Developer Platform (Stripe-like)

•⁠ ⁠*Merchant API for Seamless Integration*

- Create payments/charges
- Fetch payment status
- Issue refunds (where supported)
- Manage customers & metadata
  - **Metadata limits:** ≤20 keys; each key ≤64 chars; each value ≤256 chars
  - Violations return `MetadataTooLarge` / `MetadataValueTooLong`
  •⁠ ⁠*Webhooks*
- ⁠ payment.created ⁠, ⁠ payment.pending ⁠, ⁠ payment.confirmed ⁠, ⁠ payment.failed ⁠, ⁠ payment.settled ⁠
- Also: `refund.*` and `dispute.*` lifecycle events
- Full merchant guide (payloads, HMAC verification, retries, idempotency, examples): **[docs/webhooks.md](docs/webhooks.md)**

### No-Code / Low-Code

•⁠ ⁠*Payment Links*

- Shareable links for quick checkout (social commerce, WhatsApp, Instagram, etc.)
- Optional `base_url` builds `{base_url}/pay/{link_id}` as `shareable_url` (QR-ready)
  •⁠ ⁠*Invoices*
- Generate invoices with payment links and track payment status
- Perfect for freelancers, agencies, and B2B billing

### Merchant Tools

•⁠ ⁠Merchant Dashboard & Analytics
•⁠ ⁠Reconciliation Reports
•⁠ ⁠Built for Emerging Markets

## Typical Integrations

### 1) Checkout on your website/app

•⁠ ⁠Merchant calls FluxaPay API to create a payment
•⁠ ⁠Customer completes payment via hosted checkout or embedded flow
•⁠ ⁠Fluxapay sends webhook when confirmed
•⁠ ⁠Merchant fulfills the order

### 2) Payment links for invoices & social commerce

•⁠ ⁠Merchant generates a payment link (amount, currency, description)
•⁠ ⁠Customer pays using Stellar USDC
•⁠ ⁠Merchant is notified via dashboard + webhook/email (optional)

## Tech Stack (Planned)

•⁠ ⁠*Blockchain:* Stellar  
•⁠ ⁠*Stablecoin Rail:* USDC on Stellar  
•⁠ ⁠*Backend:* Node.js (TBD)  
•⁠ ⁠*Smart Contracts:* Stellar Soroban
•⁠ ⁠*Database:* PostgreSQL  
•⁠ ⁠*APIs:* REST + Webhooks  
•⁠ ⁠*Frontend:* Next.js (Merchant Dashboard)  
•⁠ ⁠*FX & Settlement:* On-chain liquidity + payout partners

## Use Cases

•⁠ ⁠E-commerce stores and marketplaces
•⁠ ⁠SaaS and subscription businesses
•⁠ ⁠Freelancers & agencies (invoices + payment links)
•⁠ ⁠Cross-border payments for global customers
•⁠ ⁠Merchants in emerging markets accepting stablecoin payments

## Vision

Make stablecoin payments simple, practical, and accessible so merchants can sell globally while customers pay directly with USDC, without offramping friction.

## Roadmap

•⁠ ⁠[ ] Core payment gateway (USDC on Stellar)
•⁠ ⁠[ ] Merchant dashboard
•⁠ ⁠[ ] API for payments + webhooks
•⁠ ⁠[ ] Payment links
•⁠ ⁠[ ] Invoicing
•⁠ ⁠[ ] SDKs
•⁠ ⁠[ ] Fiat settlement integrations
•⁠ ⁠[ ] Refunds & dispute tooling (where applicable)
•⁠ ⁠[ ] Multi-currency support & expanded stablecoins

## Contributing

Contributions are welcome!  
Open an issue or submit a PR to help build Fluxapay.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full guide: local setup, build and test commands, branch naming, commit format, and PR requirements.

**Before making breaking changes**, please review our [Breaking Change Policy](docs/BREAKING_CHANGES.md) to understand deprecation timelines, versioning strategy, and communication requirements.

### Local Development Setup

1. **Environment Variables**: Copy `.env.example` to `.env` and populate with your testnet credentials (do not commit `.env`):
   ```bash
   cp .env.example .env
   # Edit .env with your Stellar testnet keys and contract IDs
   ```

2. **Local Contract Invocation**: See [docs/local-invoke.md](docs/local-invoke.md) for step-by-step recipes to test `create_payment`, `register_merchant`, and other contract functions on testnet.

3. **Running Tests**:
   ```bash
   cd fluxapay && make test
   ```

4. **Code Quality**: Format, lint, and audit before submitting:
   ```bash
   cd fluxapay && make fmt && cargo clippy --all-targets --all-features && cargo audit
   ```

## Security

Please refer to our [Security Policy](SECURITY.md) for information on reporting vulnerabilities and our current audit status.

## Refunds

FluxaPay supports both full and partial refunds on confirmed USDC payments via the `RefundManager` contract.

### How Refunds Work

1. A merchant (or authorized requester) calls `create_refund` with the `payment_id`, the refund amount, and a reason.
2. The refund is created in `Pending` status and added to the payment's refund list.
3. A settlement operator calls `process_refund` to execute the on-chain USDC transfer back to the requester (minus a 1% processing fee).
4. The refund status transitions to `Completed`.

**Constraints:**
- The sum of all non-rejected refunds for a payment cannot exceed the original payment amount (`RefundExceedsPayment` error #16).
- Multiple partial refunds are supported — each is tracked independently in the `PaymentRefunds` list.
- Only `Confirmed` payments can be refunded.
- Rejected refunds do not count toward the total, allowing replacement refunds.

### Creating a Refund (Soroban CLI)

```bash
stellar contract invoke \
  --id <REFUND_MANAGER_CONTRACT_ID> \
  --source <REQUESTER_SECRET_KEY> \
  --network testnet \
  -- create_refund \
  --payment_id "payment_abc123" \
  --refund_amount 500000000 \
  --reason "Customer requested return" \
  --requester <REQUESTER_ADDRESS>
```

### Processing a Refund (Soroban CLI — settlement operator)

```bash
stellar contract invoke \
  --id <REFUND_MANAGER_CONTRACT_ID> \
  --source <OPERATOR_SECRET_KEY> \
  --network testnet \
  -- process_refund \
  --operator <OPERATOR_ADDRESS> \
  --refund_id "refund_1"
```

### Partial Refund Example (Rust SDK)

```rust
// Register the payment first (done automatically when a payment is confirmed)
client.register_payment(&payment_id, &merchant_id, &1_000_000_000i128, &usdc_symbol);

// Issue three partial refunds totalling the full payment amount
let r1 = client.create_refund(&payment_id, &300_000_000i128, &reason, &requester);
let r2 = client.create_refund(&payment_id, &400_000_000i128, &reason, &requester);
let r3 = client.create_refund(&payment_id, &300_000_000i128, &reason, &requester);

// Process each refund (operator role required)
client.process_refund(&operator, &r1);
client.process_refund(&operator, &r2);
client.process_refund(&operator, &r3);
```

### Querying Refunds

```bash
# Get a single refund by ID
stellar contract invoke --id <CONTRACT_ID> --network testnet \
  -- get_refund --refund_id "refund_1"

# Get all refunds for a payment
stellar contract invoke --id <CONTRACT_ID> --network testnet \
  -- get_payment_refunds --payment_id "payment_abc123"
```

### Refund Webhooks

FluxaPay emits the following on-chain events for refund lifecycle tracking:

| Event | Trigger |
|---|---|
| `REFUND/CREATED` | A new refund request is submitted |
| `REFUND/COMPLETED` | Refund is processed and USDC transferred |
| `REFUND/REJECTED` | Operator rejects the refund request |
| `REFUND/CANCELLED` | Requester or admin cancels a pending refund |

## Architecture

For a detailed understanding of the contract structure, cross-contract interactions, role model, and payment lifecycle, see [docs/architecture.md](docs/architecture.md).

## Error Codes

For a unified reference of every contract error code (`PaymentProcessor`, `RefundManager`, `AccessControl`, `Stream`, `FXOracle`, `MerchantRegistry`, `MerchantAuth`, `DexRouter`, `AccountAbstraction`) with common causes and remediation steps, see [docs/error-codes.md](docs/error-codes.md).

## FAQ

Common questions from merchants and developers: [docs/faq.md](docs/faq.md)

## Scripts

Operational scripts for deployment, SDK generation, and CI: [scripts/README.md](scripts/README.md)

## Telegram link

<https://t.me/+m23gN14007w0ZmQ0>
