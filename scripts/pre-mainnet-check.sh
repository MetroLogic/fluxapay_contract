#!/usr/bin/env bash
# Issue #837: Automate as many items from docs/mainnet-deployment-checklist.md
# as possible. Exits 0 only when every automated check passes.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PASS=0
FAIL=0
MAX_WASM_SIZE_KB="${MAX_WASM_SIZE_KB:-100}"
RELEASE_BRANCH="${RELEASE_BRANCH:-main}"

ok() {
  echo "✓ $1"
  PASS=$((PASS + 1))
}

bad() {
  echo "✗ $1"
  FAIL=$((FAIL + 1))
}

echo "=== FluxaPay pre-mainnet checks ==="
echo "Root: $ROOT"
echo

# ── 1. CI workflows pass on the release branch ───────────────────────────────
check_ci() {
  local name="All CI workflows pass on ${RELEASE_BRANCH}"
  if ! command -v gh >/dev/null 2>&1; then
    bad "$name (gh CLI not installed)"
    return
  fi
  if ! gh auth status >/dev/null 2>&1; then
    bad "$name (gh not authenticated)"
    return
  fi
  local conclusion
  conclusion="$(gh run list --branch "$RELEASE_BRANCH" --workflow ci.yml --limit 1 --json conclusion -q '.[0].conclusion' 2>/dev/null || true)"
  if [[ "$conclusion" == "success" ]]; then
    ok "$name"
  else
    bad "$name (latest ci.yml conclusion: ${conclusion:-unknown})"
  fi
}

# ── 2. WASM binary exists and size is within limits ──────────────────────────
check_wasm() {
  local name="WASM binary exists and size ≤ ${MAX_WASM_SIZE_KB} KB"
  local dir="target/wasm32-unknown-unknown/release"
  local max_bytes=$((MAX_WASM_SIZE_KB * 1024))
  if [[ ! -d "$dir" ]]; then
    bad "$name (no release WASM dir; run cargo build --release --target wasm32-unknown-unknown)"
    return
  fi
  local found=0
  local oversized=0
  shopt -s nullglob
  for wasm in "$dir"/*.wasm; do
    found=1
    local size
    size="$(wc -c < "$wasm" | tr -d ' ')"
    if (( size > max_bytes )); then
      oversized=1
      echo "    - $(basename "$wasm"): $size bytes (OVER)"
    else
      echo "    - $(basename "$wasm"): $size bytes"
    fi
  done
  shopt -u nullglob
  if (( found == 0 )); then
    bad "$name (no .wasm files found)"
  elif (( oversized == 1 )); then
    bad "$name"
  else
    ok "$name"
  fi
}

# ── 3. cargo deny check ──────────────────────────────────────────────────────
check_deny() {
  local name="cargo deny check passes"
  if ! command -v cargo-deny >/dev/null 2>&1 && ! cargo deny --version >/dev/null 2>&1; then
    bad "$name (cargo-deny not installed)"
    return
  fi
  if cargo deny check >/dev/null 2>&1; then
    ok "$name"
  else
    bad "$name"
  fi
}

# ── 4. cargo audit ───────────────────────────────────────────────────────────
check_audit() {
  local name="cargo audit passes with no warnings"
  if ! command -v cargo-audit >/dev/null 2>&1 && ! cargo audit --version >/dev/null 2>&1; then
    bad "$name (cargo-audit not installed)"
    return
  fi
  if cargo audit --deny warnings >/dev/null 2>&1; then
    ok "$name"
  else
    bad "$name"
  fi
}

# ── 5. .env.example has no real secrets ──────────────────────────────────────
check_env_example() {
  local name=".env.example has no SECRET_KEY or private key values committed"
  local file=".env.example"
  if [[ ! -f "$file" ]]; then
    bad "$name (file missing)"
    return
  fi
  # Reject obvious real Stellar secret keys (S...) or hex private keys, while
  # allowing placeholders like <YOUR_ADMIN_SECRET_KEY_HERE>.
  if grep -Eiq '^(STELLAR_SECRET_KEY|SECRET_KEY|PRIVATE_KEY)=S[A-Z0-9]{55}$' "$file"; then
    bad "$name (real Stellar secret key detected)"
    return
  fi
  if grep -Eiq '^(STELLAR_SECRET_KEY|SECRET_KEY|PRIVATE_KEY)=[0-9a-f]{64}$' "$file"; then
    bad "$name (hex private key detected)"
    return
  fi
  if grep -Eiq 'BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY' "$file"; then
    bad "$name (PEM private key block detected)"
    return
  fi
  ok "$name"
}

# ── 6. Mainnet contract IDs check script ─────────────────────────────────────
check_contract_ids() {
  local name="Contract IDs in scripts/check-mainnet-contract-ids.js resolve"
  if [[ ! -f scripts/check-mainnet-contract-ids.js ]]; then
    bad "$name (script missing)"
    return
  fi
  if node scripts/check-mainnet-contract-ids.js >/dev/null 2>&1; then
    # Script is informational today; treat placeholder mainnet IDs as a soft fail
    # when UNSET_CONTRACT_ID is still present.
    if grep -q 'UNSET_CONTRACT_ID' sdk/src/network-profiles.ts 2>/dev/null \
      && grep -A40 'mainnet:' sdk/src/network-profiles.ts | grep -q 'UNSET_CONTRACT_ID'; then
      bad "$name (mainnet still has UNSET_CONTRACT_ID placeholders)"
    else
      ok "$name"
    fi
  else
    bad "$name"
  fi
}

# ── 7. Admin multisig ≥ 3 signers (ADR-0004) ─────────────────────────────────
check_multisig() {
  local name="Admin multisig has ≥ 3 signers (ADR-0004)"
  if [[ -z "${MAINNET_RPC_URL:-}" || -z "${PAYMENT_PROCESSOR_CONTRACT_ID:-}" ]]; then
    bad "$name (set MAINNET_RPC_URL and PAYMENT_PROCESSOR_CONTRACT_ID to verify on-chain)"
    return
  fi
  if ! command -v stellar >/dev/null 2>&1; then
    bad "$name (stellar CLI not installed)"
    return
  fi
  local out
  if ! out="$(stellar contract invoke \
      --network mainnet \
      --rpc-url "$MAINNET_RPC_URL" \
      --id "$PAYMENT_PROCESSOR_CONTRACT_ID" \
      -- get_multisig_config 2>/dev/null)"; then
    bad "$name (invoke failed)"
    return
  fi
  # Expect a vec of signers; count Address-like entries (G... or C...).
  local count
  count="$(echo "$out" | grep -oE '"G[A-Z0-9]{55}"|"C[A-Z0-9]{55}"' | wc -l | tr -d ' ')"
  if (( count >= 3 )); then
    ok "$name ($count signers)"
  else
    bad "$name (found $count signers)"
  fi
}

# ── 8. FX oracle has at least one rate set ───────────────────────────────────
check_fx_oracle() {
  local name="FX oracle has at least one rate set"
  if [[ -z "${MAINNET_RPC_URL:-}" || -z "${FX_ORACLE_CONTRACT_ID:-}" ]]; then
    bad "$name (set MAINNET_RPC_URL and FX_ORACLE_CONTRACT_ID to verify on-chain)"
    return
  fi
  if ! command -v stellar >/dev/null 2>&1; then
    bad "$name (stellar CLI not installed)"
    return
  fi
  local pair="${FX_CHECK_PAIR:-USDC_NGN}"
  if stellar contract invoke \
      --network mainnet \
      --rpc-url "$MAINNET_RPC_URL" \
      --id "$FX_ORACLE_CONTRACT_ID" \
      -- get_rate \
      --pair "$pair" >/dev/null 2>&1; then
    ok "$name (pair $pair)"
  else
    bad "$name (no rate for $pair)"
  fi
}

check_ci
check_wasm
check_deny
check_audit
check_env_example
check_contract_ids
check_multisig
check_fx_oracle

echo
echo "Automated: $PASS passed, $FAIL failed"
echo
echo "=== Manual checks (from docs/mainnet-deployment-checklist.md) ==="
echo "  • External audit completed and SECURITY.md updated — Security"
echo "  • Testnet smoke test passing end-to-end — Dev / QA"
echo "  • CHANGELOG.md updated with release notes — Dev"
echo "  • USDC token address + KYC tier limits configured for production — Admin"
echo "  • Admin key stored in hardware wallet / HSM — Security"
echo "  • FX oracle updater + subscription daemon running — DevOps"
echo "  • Monitoring/alerting configured — DevOps"
echo "  • Deployment approved by 2 team members (GitHub Environment Protection) — Team"
echo "  • Post-deploy: first payment webhooks, FX updates, rollback plan — DevOps"
echo "  See docs/mainnet-deployment-checklist.md for the full list."
echo

if (( FAIL > 0 )); then
  echo "RESULT: FAILED ($FAIL automated check(s) failed)"
  exit 1
fi

echo "RESULT: PASSED"
exit 0
