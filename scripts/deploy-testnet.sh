#!/usr/bin/env bash
# scripts/deploy-testnet.sh
#
# Deploys all six FluxaPay contracts to Stellar testnet (or the network set
# in STELLAR_NETWORK) in dependency order, initialises each contract with the
# deployer as admin, and writes the resulting contract IDs to .env.testnet.
#
# Optionally seeds:
#   - A test merchant in MerchantRegistry
#   - A USDC/USD FX rate in FXOracle
#   - A sample payment in PaymentProcessor
#
# Deployment order:
#   1. FXOracle
#   2. MerchantRegistry
#   3. PaymentProcessor
#   4. RefundManager
#   5. PaymentLinkManager
#   6. GasEstimator
#
# Required environment variables:
#   STELLAR_SECRET_KEY  — deployer secret key (starts with S)
#   STELLAR_NETWORK     — target network (testnet | mainnet)
#
# Optional environment variables:
#   STELLAR_RPC_URL     — override RPC endpoint
#   SEED_DATA           — set to "true" to seed test data after deploy
#   SKIP_BUILD          — set to "true" to skip cargo build step

set -euo pipefail

# ── Validation ────────────────────────────────────────────────────────────────

: "${STELLAR_SECRET_KEY:?Error: STELLAR_SECRET_KEY must be set}"
: "${STELLAR_NETWORK:?Error: STELLAR_NETWORK must be set}"

if ! command -v stellar &>/dev/null; then
    echo "Error: 'stellar' CLI not found. Install it with:"
    echo "  cargo install --locked stellar-cli"
    exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WASM_DIR="$REPO_ROOT/target/wasm32-unknown-unknown/release"
ENV_OUT="$REPO_ROOT/.env.testnet"

SEED_DATA="${SEED_DATA:-false}"
SKIP_BUILD="${SKIP_BUILD:-false}"

# Derive the deployer public address from the secret key
DEPLOYER_ADDRESS="$(stellar keys address --secret-key "$STELLAR_SECRET_KEY" 2>/dev/null || \
    stellar keys address "$STELLAR_SECRET_KEY" 2>/dev/null || echo "")"

if [[ -z "$DEPLOYER_ADDRESS" ]]; then
    # Fallback: generate a named key and derive address
    stellar keys generate --name deploy-admin --overwrite \
        --network "$STELLAR_NETWORK" &>/dev/null || true
    DEPLOYER_ADDRESS="$(stellar keys address deploy-admin)"
fi

STELLAR_COMMON_ARGS=(--network "$STELLAR_NETWORK" --source "$STELLAR_SECRET_KEY")

echo "========================================================"
echo "  FluxaPay Testnet Deployment"
echo "========================================================"
echo "  Network   : $STELLAR_NETWORK"
echo "  Deployer  : $DEPLOYER_ADDRESS"
echo "  Output    : $ENV_OUT"
echo "========================================================"
echo ""

# ── Fund deployer via Friendbot (testnet only) ────────────────────────────────

if [[ "$STELLAR_NETWORK" == "testnet" ]]; then
    echo ">> Funding deployer via Friendbot..."
    curl -sf "https://friendbot.stellar.org/?addr=${DEPLOYER_ADDRESS}" >/dev/null \
        && echo "   Funded: $DEPLOYER_ADDRESS" \
        || echo "   (Already funded or faucet unavailable – continuing)"
    echo ""
fi

# ── Build ─────────────────────────────────────────────────────────────────────

if [[ "$SKIP_BUILD" != "true" ]]; then
    echo ">> Building FluxaPay WASM contracts..."
    cargo build \
        --manifest-path "$REPO_ROOT/Cargo.toml" \
        --target wasm32-unknown-unknown \
        --release \
        -p fluxapay
    echo "   Build complete."
    echo ""
fi

WASM="$WASM_DIR/fluxapay.wasm"
if [[ ! -f "$WASM" ]]; then
    echo "Error: WASM not found at $WASM — run without SKIP_BUILD=true."
    exit 1
fi

# ── Deploy helper ─────────────────────────────────────────────────────────────

deploy_contract() {
    local step="$1"
    local label="$2"
    printf ">> [%s] Deploying %-24s " "$step" "$label..."
    local id
    id="$(stellar contract deploy \
        --wasm "$WASM" \
        "${STELLAR_COMMON_ARGS[@]}")"
    echo "$id"
    echo "$id"
}

# ── Invoke helper ─────────────────────────────────────────────────────────────

invoke() {
    local contract_id="$1"
    shift
    stellar contract invoke \
        --id "$contract_id" \
        "${STELLAR_COMMON_ARGS[@]}" \
        -- "$@"
}

# ── Deploy each contract ──────────────────────────────────────────────────────

echo "=== Deploying Contracts ==="
echo ""

# 1. FXOracle
FX_ORACLE_CONTRACT_ID="$(deploy_contract "1/6" "FXOracle" | tail -1)"

# 2. MerchantRegistry
MERCHANT_REGISTRY_CONTRACT_ID="$(deploy_contract "2/6" "MerchantRegistry" | tail -1)"

# 3. PaymentProcessor
PAYMENT_PROCESSOR_CONTRACT_ID="$(deploy_contract "3/6" "PaymentProcessor" | tail -1)"

# 4. RefundManager
REFUND_MANAGER_CONTRACT_ID="$(deploy_contract "4/6" "RefundManager" | tail -1)"

# 5. PaymentLinkManager
PAYMENT_LINK_MANAGER_CONTRACT_ID="$(deploy_contract "5/6" "PaymentLinkManager" | tail -1)"

# 6. GasEstimator
GAS_ESTIMATOR_CONTRACT_ID="$(deploy_contract "6/6" "GasEstimator" | tail -1)"

echo ""
echo "=== Initialising Contracts ==="
echo ""

# ── Initialize FXOracle ───────────────────────────────────────────────────────

echo ">> Initialising FXOracle..."
invoke "$FX_ORACLE_CONTRACT_ID" \
    oracle_initialize \
    --admin "$DEPLOYER_ADDRESS" \
    --staleness_threshold 86400
# Grant oracle role to deployer so we can seed rates
invoke "$FX_ORACLE_CONTRACT_ID" \
    oracle_grant_role \
    --admin "$DEPLOYER_ADDRESS" \
    --role ORACLE \
    --account "$DEPLOYER_ADDRESS"
echo "   Done."

# ── Initialize MerchantRegistry ───────────────────────────────────────────────

echo ">> Initialising MerchantRegistry..."
invoke "$MERCHANT_REGISTRY_CONTRACT_ID" \
    initialize \
    --admin "$DEPLOYER_ADDRESS"
echo "   Done."

# ── Initialize PaymentProcessor ───────────────────────────────────────────────

echo ">> Initialising PaymentProcessor..."
invoke "$PAYMENT_PROCESSOR_CONTRACT_ID" \
    initialize_payment_processor \
    --admin "$DEPLOYER_ADDRESS"

# Wire MerchantRegistry and FXOracle into PaymentProcessor
invoke "$PAYMENT_PROCESSOR_CONTRACT_ID" \
    set_merchant_registry_address \
    --admin "$DEPLOYER_ADDRESS" \
    --registry_address "$MERCHANT_REGISTRY_CONTRACT_ID"

invoke "$PAYMENT_PROCESSOR_CONTRACT_ID" \
    set_fx_oracle_address \
    --admin "$DEPLOYER_ADDRESS" \
    --oracle_address "$FX_ORACLE_CONTRACT_ID"
echo "   Done."

# ── Initialize RefundManager ──────────────────────────────────────────────────

echo ">> Initialising RefundManager..."
# RefundManager requires a USDC token address — use a placeholder that can be
# updated post-deploy via a follow-up admin call when the real token is known.
USDC_PLACEHOLDER="CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM"
invoke "$REFUND_MANAGER_CONTRACT_ID" \
    initialize_refund_manager \
    --admin "$DEPLOYER_ADDRESS" \
    --usdc_token_address "$USDC_PLACEHOLDER" || \
    echo "   (RefundManager init skipped — may need real USDC token address)"
echo "   Done."

# Authorize the deployed system contracts for MerchantRegistry callbacks.
invoke "$MERCHANT_REGISTRY_CONTRACT_ID" \
    set_refund_manager_address \
    --admin "$DEPLOYER_ADDRESS" \
    --refund_manager "$REFUND_MANAGER_CONTRACT_ID"
invoke "$MERCHANT_REGISTRY_CONTRACT_ID" \
    set_payment_processor_address \
    --admin "$DEPLOYER_ADDRESS" \
    --payment_processor "$PAYMENT_PROCESSOR_CONTRACT_ID"
echo "   MerchantRegistry system callers configured."

# ── Initialize PaymentLinkManager ────────────────────────────────────────────

echo ">> Initialising PaymentLinkManager..."
invoke "$PAYMENT_LINK_MANAGER_CONTRACT_ID" \
    initialize \
    --admin "$DEPLOYER_ADDRESS"
echo "   Done."

# ── Write .env.testnet ────────────────────────────────────────────────────────

echo ""
echo "=== Writing $ENV_OUT ==="

cat > "$ENV_OUT" <<EOF
# Auto-generated by scripts/deploy-testnet.sh — do not edit manually
# Re-run the script to refresh; it is safe to run multiple times.

STELLAR_NETWORK=${STELLAR_NETWORK}
DEPLOYER_ADDRESS=${DEPLOYER_ADDRESS}

FX_ORACLE_CONTRACT_ID=${FX_ORACLE_CONTRACT_ID}
MERCHANT_REGISTRY_CONTRACT_ID=${MERCHANT_REGISTRY_CONTRACT_ID}
PAYMENT_PROCESSOR_CONTRACT_ID=${PAYMENT_PROCESSOR_CONTRACT_ID}
REFUND_MANAGER_CONTRACT_ID=${REFUND_MANAGER_CONTRACT_ID}
PAYMENT_LINK_MANAGER_CONTRACT_ID=${PAYMENT_LINK_MANAGER_CONTRACT_ID}
GAS_ESTIMATOR_CONTRACT_ID=${GAS_ESTIMATOR_CONTRACT_ID}

# Aliases kept for backward compatibility with existing tooling
PAYMENT_PROCESSOR_ID=${PAYMENT_PROCESSOR_CONTRACT_ID}
REFUND_MANAGER_ID=${REFUND_MANAGER_CONTRACT_ID}
MERCHANT_REGISTRY_ID=${MERCHANT_REGISTRY_CONTRACT_ID}
FX_ORACLE_ID=${FX_ORACLE_CONTRACT_ID}
GAS_ESTIMATOR_ID=${GAS_ESTIMATOR_CONTRACT_ID}
EOF

echo "   Written."

# ── Optional: Seed test data ──────────────────────────────────────────────────

if [[ "$SEED_DATA" == "true" ]]; then
    echo ""
    echo "=== Seeding Test Data ==="
    echo ""

    # Fund extra test wallets
    if [[ "$STELLAR_NETWORK" == "testnet" ]]; then
        stellar keys generate --name test-merchant --overwrite \
            --network "$STELLAR_NETWORK" &>/dev/null || true
        stellar keys generate --name test-oracle --overwrite \
            --network "$STELLAR_NETWORK" &>/dev/null || true

        TEST_MERCHANT_ADDRESS="$(stellar keys address test-merchant)"
        TEST_ORACLE_ADDRESS="$(stellar keys address test-oracle)"

        echo ">> Funding test wallets via Friendbot..."
        curl -sf "https://friendbot.stellar.org/?addr=${TEST_MERCHANT_ADDRESS}" >/dev/null || true
        curl -sf "https://friendbot.stellar.org/?addr=${TEST_ORACLE_ADDRESS}" >/dev/null || true
        echo "   test-merchant : $TEST_MERCHANT_ADDRESS"
        echo "   test-oracle   : $TEST_ORACLE_ADDRESS"
    else
        TEST_MERCHANT_ADDRESS="$DEPLOYER_ADDRESS"
        TEST_ORACLE_ADDRESS="$DEPLOYER_ADDRESS"
    fi

    # Seed: USDC/USD rate in FXOracle (rate=10000, decimals=4 → 1.0000 USDC/USD)
    echo ">> Setting USDC/USD rate in FXOracle..."
    invoke "$FX_ORACLE_CONTRACT_ID" \
        set_rate \
        --operator "$DEPLOYER_ADDRESS" \
        --pair USDCUSD \
        --rate 10000 \
        --decimals 4
    echo "   USDC/USD rate set."

    # Seed: Register and verify a test merchant
    echo ">> Registering test merchant in MerchantRegistry..."
    invoke "$MERCHANT_REGISTRY_CONTRACT_ID" \
        register_merchant \
        --merchant_id "$TEST_MERCHANT_ADDRESS" \
        --business_name "FluxaPay Demo Merchant" \
        --settlement_currency USD \
        --payout_address null \
        --bank_account null \
        --metadata_hash null || echo "   (Merchant may already be registered)"

    invoke "$MERCHANT_REGISTRY_CONTRACT_ID" \
        verify_merchant \
        --admin "$DEPLOYER_ADDRESS" \
        --merchant_id "$TEST_MERCHANT_ADDRESS"
    echo "   Merchant registered and verified."

    # Seed: Grant MERCHANT role in PaymentProcessor
    invoke "$PAYMENT_PROCESSOR_CONTRACT_ID" \
        grant_role \
        --admin "$DEPLOYER_ADDRESS" \
        --role MERCHANT \
        --account "$TEST_MERCHANT_ADDRESS"

    # Seed: Create a sample payment
    EXPIRES_AT="$(( $(date +%s) + 3600 ))"
    PAYMENT_ID="seed_pay_$(date +%s)"
    echo ">> Creating sample payment: $PAYMENT_ID..."
    invoke "$PAYMENT_PROCESSOR_CONTRACT_ID" \
        create_payment \
        --args "{
          \"payment_id\": \"$PAYMENT_ID\",
          \"merchant_id\": \"$TEST_MERCHANT_ADDRESS\",
          \"amount\": 1000000000,
          \"currency\": \"USDC\",
          \"deposit_address\": \"$TEST_MERCHANT_ADDRESS\",
          \"expires_at\": $EXPIRES_AT,
          \"duration_secs\": null,
          \"memo\": null,
          \"memo_type\": null,
          \"token_address\": null,
          \"client_token\": null,
          \"metadata_hash\": null,
          \"metadata\": null
        }" || echo "   (Sample payment skipped — may need struct arg format adjustment)"
    echo "   Seeding complete."

    # Append test addresses to .env.testnet
    cat >> "$ENV_OUT" <<EOF

# Seeded test addresses
TEST_MERCHANT_ADDRESS=${TEST_MERCHANT_ADDRESS}
TEST_ORACLE_ADDRESS=${TEST_ORACLE_ADDRESS}
SAMPLE_PAYMENT_ID=${PAYMENT_ID}
EOF
fi

# ── Summary ───────────────────────────────────────────────────────────────────

echo ""
echo "========================================================"
echo "  Deployment Complete"
echo "========================================================"
echo "  FXOracle               : $FX_ORACLE_CONTRACT_ID"
echo "  MerchantRegistry       : $MERCHANT_REGISTRY_CONTRACT_ID"
echo "  PaymentProcessor       : $PAYMENT_PROCESSOR_CONTRACT_ID"
echo "  RefundManager          : $REFUND_MANAGER_CONTRACT_ID"
echo "  PaymentLinkManager     : $PAYMENT_LINK_MANAGER_CONTRACT_ID"
echo "  GasEstimator           : $GAS_ESTIMATOR_CONTRACT_ID"
echo ""
echo "  Contract IDs written to: $ENV_OUT"
echo ""
echo "  Load them with:"
echo "    source $ENV_OUT"
echo "========================================================"
