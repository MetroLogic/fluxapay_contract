#!/usr/bin/env bash
#
# Deploy all FluxaPay contracts to a Stellar network in dependency order.
#
# Usage:
#   ./deploy.sh [network]
#
# The target network can be supplied as an optional positional argument or via
# the STELLAR_NETWORK environment variable. The positional argument takes
# precedence over the environment variable. When neither is provided the
# network defaults to `testnet`.
#
# Accepted networks: testnet | futurenet | standalone
#
# Examples:
#   ./deploy.sh testnet
#   ./deploy.sh futurenet
#   STELLAR_NETWORK=futurenet ./deploy.sh
#   STELLAR_NETWORK=standalone ./deploy.sh standalone

set -euo pipefail

usage() {
  cat <<'EOF'
Usage: deploy.sh [network]

Deploy FluxaPay contracts to a Stellar network.

Arguments:
  network            Target network: testnet | futurenet | standalone
                     (default: testnet, or $STELLAR_NETWORK if set)

Environment:
  STELLAR_NETWORK    Override the target network (positional arg wins)
  STELLAR_SECRET_KEY Deployer secret key (required)
  STELLAR_RPC_URL    Override the Soroban RPC endpoint
  SEED_DATA          Set to "true" to seed test data
  SKIP_BUILD         Set to "true" to skip cargo build
EOF
}

# Resolve the target network: positional arg > STELLAR_NETWORK env var > testnet.
NETWORK="${1:-${STELLAR_NETWORK:-testnet}}"

case "$NETWORK" in
  testnet | futurenet | standalone) ;;
  *)
    echo "Error: unknown network '$NETWORK'." >&2
    echo "Accepted values: testnet | futurenet | standalone" >&2
    usage >&2
    exit 1
    ;;
esac

export STELLAR_NETWORK="$NETWORK"

# Delegate to the original deployment implementation, which reads
# STELLAR_NETWORK from the environment.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec bash "$SCRIPT_DIR/deploy-testnet.sh" "$@"
