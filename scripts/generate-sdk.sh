#!/usr/bin/env bash
# Generate TypeScript SDK bindings from compiled Wasm files.
# Usage: ./scripts/generate-sdk.sh [--network testnet|mainnet]
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WASM_DIR="$REPO_ROOT/target/wasm32v1-none/release"
OUT_DIR="$REPO_ROOT/sdk/src/contracts"
NETWORK="${NETWORK:-testnet}"

# Map of feature flag -> contract name -> output package name
declare -A CONTRACTS=(
  [contract-payment-processor]="fluxapay"
  [contract-refund-manager]="fluxapay_refund_manager"
  [contract-gas-estimator]="fluxapay_gas_estimator"
  [contract-fx-oracle]="fluxapay_fx_oracle"
  [contract-merchant-registry]="fluxapay_merchant_registry"
  [contract-payment-link]="fluxapay_payment_link"
)

echo "==> Building contracts and generating TypeScript bindings"

for feature in "${!CONTRACTS[@]}"; do
  pkg="${CONTRACTS[$feature]}"
  wasm="$WASM_DIR/${pkg}.wasm"

  echo ""
  echo "--- Building feature: $feature ---"
  (cd "$REPO_ROOT/fluxapay" && stellar contract build --features "$feature" --no-default-features)

  if [[ ! -f "$wasm" ]]; then
    echo "ERROR: Expected Wasm not found at $wasm" >&2
    exit 1
  fi

  echo "--- Generating bindings for $pkg ---"
  stellar contract bindings typescript \
    --wasm "$wasm" \
    --output-dir "$OUT_DIR/$pkg" \
    --overwrite \
    --network "$NETWORK"

  echo "✅ $pkg bindings written to $OUT_DIR/$pkg"
done

echo ""
echo "==> All bindings generated successfully."
