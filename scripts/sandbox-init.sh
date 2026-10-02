#!/usr/bin/env bash
# Quick sandbox bootstrap for local docker-compose stack (Issue #231)
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="${ROOT_DIR}/.env.docker"

if [[ ! -f "${ENV_FILE}" ]]; then
  echo "Missing ${ENV_FILE}. Copy .env.docker.example first." >&2
  exit 1
fi

# shellcheck disable=SC1090
source "${ENV_FILE}"

: "${STELLAR_RPC_URL:?STELLAR_RPC_URL is required}"
: "${STELLAR_NETWORK:?STELLAR_NETWORK is required}"

echo "Building FluxaPay WASM..."
(cd "${ROOT_DIR}/fluxapay" && stellar contract build)

echo "Generating local identities (if missing)..."
for name in sandbox-admin sandbox-merchant sandbox-customer; do
  stellar keys generate --name "${name}" --network "${STELLAR_NETWORK}" --rpc-url "${STELLAR_RPC_URL}" >/dev/null 2>&1 || true
done

ADMIN="$(stellar keys address sandbox-admin)"
echo "Admin address: ${ADMIN}"

echo "Funding admin via friendbot (local/testnet only)..."
curl -sf "http://localhost:8000/friendbot?addr=${ADMIN}" >/dev/null || \
  curl -sf "https://friendbot.stellar.org/?addr=${ADMIN}" >/dev/null || true

echo "Sandbox ready. Next steps:"
echo "  1. Deploy WASM with stellar contract deploy"
echo "  2. Export contract IDs into .env.docker"
echo "  3. Follow docs/sandbox-deployment.md invoke recipes"
