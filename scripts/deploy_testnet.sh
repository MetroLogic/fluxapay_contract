#!/usr/bin/env bash
set -euo pipefail

echo "=== FluxaPay Testnet Deployment with Faucet Auto-Funding ==="

NETWORK="testnet"
SECRET_KEY="${STELLAR_SECRET_KEY:-}"
PUBLIC_KEY="${STELLAR_PUBLIC_KEY:-}"

if [ -z "$PUBLIC_KEY" ]; then
    echo "Generating temporary deployment keypair..."
    PUBLIC_KEY=$(stellar keys generate temp_deployer --network $NETWORK | grep "Public Key" | awk '{print $3}')
fi

echo "Auto-funding deployer account ($PUBLIC_KEY) via Stellar Testnet Friendbot Faucet..."
curl -X POST -s "https://friendbot.stellar.org?addr=${PUBLIC_KEY}" > /dev/null
echo "Account funded successfully!"

echo "Deploying contracts to $NETWORK..."
CONTRACT_ID=$(stellar contract deploy --wasm target/wasm32-unknown-unknown/release/fluxapay_contract.wasm --source temp_deployer --network $NETWORK)
echo "Deployed FluxaPay Contract ID: $CONTRACT_ID"
