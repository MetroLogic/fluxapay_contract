# SDK Integration Test Guide

This guide explains how to set up, configure, and execute the full integration test suite for `@fluxapay/sdk` (`sdk/test/integration/`) against real Soroban smart contracts deployed to the Stellar Testnet.

---

## 1. Prerequisites

Before running the integration tests, ensure you have the following installed and set up:

1. **Node.js**: Version 20 or later (`node -v`).
2. **Stellar CLI**: Installed and configured (`stellar --version`).
   ```bash
   cargo install --locked stellar-cli --features opt
   ```
3. **Funded Stellar Testnet Account**:
   Generate an identity and fund it using Friendbot:
   ```bash
   stellar keys generate --network testnet fluxapay-admin
   stellar keys fund fluxapay-admin --network testnet
   ```
4. **Deployed Contracts**:
   The test suite executes against live on-chain instances of `PaymentProcessor` and `MerchantRegistry`.
   You can deploy them using:
   ```bash
   bash scripts/deploy-testnet.sh
   ```

---

## 2. Environment Variables

Integration tests read configuration from environment variables. If `TESTNET_RPC_URL` is omitted, tests automatically skip gracefully.

Configure these variables in your shell or `.env`:

| Variable | Description | Example / Default |
|---|---|---|
| `TESTNET_RPC_URL` | Stellar Soroban RPC testnet endpoint | `https://soroban-testnet.stellar.org` |
| `TESTNET_PAYMENT_PROCESSOR_CONTRACT_ID` | Deployed `PaymentProcessor` contract address (C...) | `CA...` |
| `TESTNET_MERCHANT_REGISTRY_CONTRACT_ID` | Deployed `MerchantRegistry` contract address (C...) | `CB...` |
| `TESTNET_ADMIN_SECRET_KEY` | Stellar secret key (S...) for contract admin operations | `SA...` |
| `TESTNET_MERCHANT_SECRET_KEY` | Stellar secret key (S...) for merchant identity | `SB...` |
| `TESTNET_PAYER_SECRET_KEY` | Stellar secret key (S...) for customer/payer identity | `SC...` |

> [!WARNING]
> Never commit real secret keys or `.env` files containing secrets to version control.

---

## 3. Step-by-Step Setup

### Step 1: Generate & Fund Keypairs

Generate three distinct keypairs for admin, merchant, and payer:
```bash
stellar keys generate --network testnet test-admin
stellar keys fund test-admin --network testnet

stellar keys generate --network testnet test-merchant
stellar keys fund test-merchant --network testnet

stellar keys generate --network testnet test-payer
stellar keys fund test-payer --network testnet
```

Retrieve the secret keys:
```bash
export TESTNET_ADMIN_SECRET_KEY=$(stellar keys show test-admin)
export TESTNET_MERCHANT_SECRET_KEY=$(stellar keys show test-merchant)
export TESTNET_PAYER_SECRET_KEY=$(stellar keys show test-payer)
```

### Step 2: Deploy Contracts

Run the deployment script or deploy manually:
```bash
bash scripts/deploy-testnet.sh
```
Export the resulting contract addresses:
```bash
export TESTNET_RPC_URL=https://soroban-testnet.stellar.org
export TESTNET_PAYMENT_PROCESSOR_CONTRACT_ID=<PAYMENT_PROCESSOR_CONTRACT_ID>
export TESTNET_MERCHANT_REGISTRY_CONTRACT_ID=<MERCHANT_REGISTRY_CONTRACT_ID>
```

### Step 3: Build SDK and Run Suite

From the `sdk/` directory:
```bash
cd sdk
npm install
npm run build
npm run test:integration
```

---

## 4. Running Individual Test Files

You can run individual test files using `tsx`:

### Payment Creation & Lifecycle
```bash
npx tsx --test test/integration/create-payment.test.ts
```

### Payment Lifecycle Transitions (Verification, Settle, Cancel)
```bash
npx tsx --test test/integration/payment-lifecycle.test.ts
```

### Merchant Registration
```bash
npx tsx --test test/integration/merchant-registration.test.ts
```

---

## 5. Common Failures & Troubleshooting

### `AccountNotFound`
- **Cause**: The keypair has not been funded on testnet yet.
- **Solution**: Fund the account via Friendbot:
  ```bash
  curl "https://friendbot.stellar.org?addr=<PUBLIC_KEY>"
  ```

### Stale Contract ID / `HostError: Error(Value, InvalidInput)`
- **Cause**: Stellar testnet resets periodically (quarterly). Contract IDs or state deployed before the reset no longer exist.
- **Solution**: Re-deploy fresh contract instances using `bash scripts/deploy-testnet.sh` and update the environment variables.

### `Missing env vars`
- **Error**: `TESTNET_RPC_URL is set but one or more of TESTNET_ADMIN_SECRET_KEY / TESTNET_MERCHANT_SECRET_KEY / TESTNET_PAYER_SECRET_KEY is missing.`
- **Solution**: Ensure all 6 required variables are exported in your current shell session.

### `txBadSeq` or Nonce Collisions
- **Cause**: Multiple tests submitting transactions rapidly with the same account sequence number.
- **Solution**: Re-run with single concurrency or wait a few seconds between individual test runs.

---

## 6. CI vs Local Differences

| Aspect | Local Development | GitHub Actions CI (`sdk-integration-tests.yml`) |
|---|---|---|
| **Trigger** | Manual invocation on demand | Runs only on `push` to `main` (never on PRs to protect testnet funds) |
| **Secrets** | Local environment variables | GitHub Repository Secrets |
| **Accounts** | Developer-managed persistent accounts | Pre-configured dedicated testnet secrets |
| **Timeout** | Unrestricted | 15-minute workflow timeout |
