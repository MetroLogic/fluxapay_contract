Fluxapay is a payment gateway on the Stellar blockchain that enables merchants to accept crypto payments and get settled in their local fiat currency.

FluxaPay bridges the gap between crypto payments and real-world commerce—making stablecoin payments as easy to integrate as Stripe.

## Getting Started

New to FluxaPay? Follow the **[Merchant Quickstart Guide](docs/quickstart.md)** to go from SDK install to your first confirmed USDC payment in minutes.

## Interactive API Reference

Browse the full, interactive API reference (with runnable request/response examples for checkout and payment links) at **`/api-docs`** once the backend is running locally:

```
http://localhost:3000/api-docs
```

The reference is served offline from the bundled OpenAPI spec (`docs/openapi.yaml`) — no CDN or network access required.

## TypeScript SDK

[![npm version](https://img.shields.io/npm/v/@fluxapay/sdk.svg)](https://www.npmjs.com/package/@fluxapay/sdk)

Install the official SDK from npm:

```bash
npm install @fluxapay/sdk
```

See [sdk/README.md](sdk/README.md) for usage examples, [docs/integration-test-guide.md](docs/integration-test-guide.md) for testnet integration testing, and [sdk/CHANGELOG.md](sdk/CHANGELOG.md) for release notes.

## CI/CD

[![CI](https://github.com/MetroLogic/fluxapay_contract/actions/workflows/ci.yml/badge.svg)](https://github.com/MetroLogic/fluxapay_contract/actions/workflows/ci.yml)
[![CD](https://github.com/MetroLogic/fluxapay_contract/actions/workflows/cd.yml/badge.svg)](https://github.com/MetroLogic/fluxapay_contract/actions/workflows/cd.yml)
Automated testing and deployment pipeline using GitHub Actions:

- **CI:** Runs tests, linting, and builds on every push/PR to main
- **CD:** Auto-deploys to development and staging on merge to main; production requires manual approval
- All tests must pass before deployment

### Security and Dependency Checks

Automated in CI workflow (`ci.yml`) and runnable locally:

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

- Create payments/charges (with optional `allow_partial: true` for installment funding)
- Fetch payment status
- Issue refunds (where supported)
- Manage customers & metadata
  - **Metadata limits:** ≤20 keys; each key ≤64 chars; each value ≤256 chars
  - Violations return `MetadataTooLarge` / `MetadataValueTooLong`
- **Partial payment accumulation:** Allow customers to fund payments in multiple transactions. Emits `PAYMENT/PARTIAL_RECEIVED` events and automatically refunds partial funds on expiry.
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

### 3) Recurring billing with subscriptions

•⁠ ⁠Merchant creates a subscription plan with a fixed amount and billing interval
•⁠ ⁠Customer subscribes and is charged automatically on each cycle
•⁠ ⁠Daemon retries failed charges through the grace-period workflow
•⁠ ⁠Merchant monitors subscription lifecycle events via webhooks or dashboard alerts

See the full guide: [docs/subscription-guide.md](docs/subscription-guide.md)

### 4) Dispute handling and resolution

•⁠ ⁠Customer raises a dispute against a confirmed payment; the merchant co-authorizes creation and both parties post a bond
•⁠ ⁠Operator reviews the case and resolves or rejects it
•⁠ ⁠Time-based escalation and arbitrator voting protect the process from stalls
•⁠ ⁠Bond return / forfeiture and merchant score impacts are enforced on-chain

Merchant lifecycle and response steps: [docs/disputes.md](docs/disputes.md). For operator details, see the [full dispute resolution guide](docs/dispute-resolution-guide.md).

### 5) Streaming payroll and milestone payouts

•⁠ ⁠Sender funds a deposit and the contract releases tokens over time at a fixed rate
•⁠ ⁠Receiver can withdraw accrued funds, pause or resume flow, or top up the deposit
•⁠ ⁠Milestone approvals and destination-based withdrawals support governance-heavy payouts
•⁠ ⁠Rate changes refund surplus deposit when the stream is slowed down

See the full guide: [docs/payment-streams-guide.md](docs/payment-streams-guide.md)

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
   # Edit .env with your credentials
   ```

2. **Install dependencies**:
   ```bash
   npm install
   ```

3. **Run the backend**:
   ```bash
   npm run dev
   ```

4. **Explore the API**: Open `http://localhost:3000/api-docs` for the interactive API reference.
