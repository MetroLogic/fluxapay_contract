# Soroban Sandbox Local Deployment Quickstart

This guide walks through deploying and testing the full FluxaPay contract stack locally using the Stellar Soroban Sandbox network.

## 📋 Prerequisites

- Docker Desktop / Docker Engine
- `stellar-cli` (v21.0.0+ installed via Cargo: `cargo install --locked stellar-cli`)
- Node.js 18+

## 🚀 Step-by-Step Walkthrough

### 1. Launch Local Soroban Sandbox

Start local Stellar standalone network via Docker:
```bash
docker run --rm -it \
  -p 8000:8000 \
  --name soroban-preview \
  stellar/quickstart:testing \
  --standalone \
  --enable-soroban-rpc
```

### 2. Initialize Network & Secret Keys

Run sandbox initialization script:
```bash
bash scripts/sandbox-init.sh
```

### 3. Deploy Contract Stack

Deploy all 5 core Soroban smart contracts:
```bash
# 1. PaymentProcessor
stellar contract deploy --wasm target/wasm32-unknown-unknown/release/payment_processor.wasm --source alice --network standalone

# 2. RefundManager
stellar contract deploy --wasm target/wasm32-unknown-unknown/release/refund_manager.wasm --source alice --network standalone

# 3. FXOracle
stellar contract deploy --wasm target/wasm32-unknown-unknown/release/fx_oracle.wasm --source alice --network standalone

# 4. MerchantRegistry
stellar contract deploy --wasm target/wasm32-unknown-unknown/release/merchant_registry.wasm --source alice --network standalone

# 5. PaymentLinkManager
stellar contract deploy --wasm target/wasm32-unknown-unknown/release/payment_link_manager.wasm --source alice --network standalone
```

### 4. Account Funding & Initializing Entrypoints

```bash
# Fund test accounts
node scripts/fund-accounts.js

# Invoke initialize on PaymentProcessor
stellar contract invoke \
  --id <PAYMENT_PROCESSOR_ID> \
  --source alice \
  --network standalone \
  -- \
  initialize --admin alice --fee_bps 100
```

## 🛠️ Troubleshooting

- **`InsufficientFunds`**: Fund the invoking account using Friendbot: `curl "http://localhost:8000/friendbot?addr=<ACCOUNT_ADDRESS>"`.
- **`ContractNotFound`**: Ensure the contract ID matches the deployment output saved in `.env`.
