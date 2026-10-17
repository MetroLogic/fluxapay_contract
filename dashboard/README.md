# FluxaPay Merchant Dashboard

A minimal Next.js 14+ (App Router) merchant dashboard for managing FluxaPay payments. It authenticates merchants via SEP-10 wallet connect (Freighter) and reads all data from the indexer API using `@fluxapay/sdk` and `@fluxapay/sdk/react`.

## Features (MVP)

- **Auth** — SEP-10 wallet connect with Freighter, JWT issuance, and JWT-authenticated API calls.
- **Payments list** — table with status and date-range filters, pagination, and search by payment ID.
- **Payment detail** — expandable view with refunds, events timeline, and dispute status.
- **Create payment link** — simple form with QR code display.
- **Analytics** — weekly revenue bar chart from the analytics endpoint.
- **Subscription metrics** — monthly recurring revenue, active subscribers, churn, trial conversion, and dunning recovery.

## Tech Stack

- Next.js 14+ (App Router)
- `@fluxapay/sdk` and `@fluxapay/sdk/react`
- Tailwind CSS
- Freighter wallet for SEP-10 authentication

No dashboard-specific backend is required — all data comes from the indexer API.

## Getting Started

### Prerequisites

- Node.js 18+
- npm (or pnpm/yarn)
- The [Freighter](https://www.freighter.app/) browser extension

### Install

```bash
cd dashboard
npm install
```

### Configure

Create a `.env.local` file in `dashboard/`:

```bash
# Base URL of the FluxaPay indexer API
NEXT_PUBLIC_FLUXAPAY_API_URL=http://localhost:3000

# Stellar network to authenticate against (testnet | mainnet)
NEXT_PUBLIC_STELLAR_NETWORK=testnet
```

### Run locally

```bash
npm run dev
```

Open [http://localhost:3001](http://localhost:3001) and connect your Freighter wallet to sign in.

### Build

```bash
npm run build
npm run start
```

## Project Structure

```
dashboard/
├── app/
│   ├── (auth)/login/        # SEP-10 wallet connect + JWT issuance
│   ├── payments/            # Payments list and detail views
│   ├── payment-links/new/   # Create payment link + QR code
│   └── analytics/           # Weekly revenue chart
├── components/              # Shared UI (tables, filters, charts)
├── lib/                     # SDK client, auth context, API helpers
└── README.md
```

## Authentication

1. The dashboard requests a SEP-10 challenge from the indexer.
2. Freighter signs the challenge transaction.
3. The signed challenge is exchanged for a JWT.
4. The JWT is attached to all subsequent indexer API calls.

The subscription metrics endpoint uses merchant API keys rather than SEP-10
JWTs. The `/subscriptions` dashboard page reads the merchant-bound API key and
merchant ID from `localStorage` keys `fluxapay_api_key` and
`fluxapay_merchant_id`; the key must include the indexer `read:analytics` scope.

## Responsive Design

The layout is responsive and usable on tablet-sized viewports and larger.

## Linting

CI runs ESLint on the dashboard for every pull request:

```bash
cd dashboard
npm run lint
```
