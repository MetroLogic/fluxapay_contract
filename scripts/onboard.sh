#!/usr/bin/env bash
# scripts/onboard.sh — interactive testnet onboarding wizard (Issue #818)
#
# docs/quickstart.md leaves a new contributor running five-plus Soroban CLI
# invocations by hand, each with its own argument shape. This walks through the
# same flow with prompts: generate or accept a keypair, fund it, register a
# merchant, create a payment, and poll until it confirms.
#
# Idempotent. Re-running reuses the keypair and contract IDs already in
# .env.testnet and skips work that is already done, so an interrupted run can
# simply be run again — which is the property that makes a wizard safe to trust.
#
# Usage:
#   ./scripts/onboard.sh                  # interactive
#   MERCHANT_NAME="Acme Store" \
#     STELLAR_SECRET_KEY=S... \
#     NON_INTERACTIVE=1 ./scripts/onboard.sh
#
# Environment (all optional — prompted when absent):
#   STELLAR_SECRET_KEY   deployer/merchant secret. Generated if unset.
#   MERCHANT_NAME        business name. Default "Acme Store".
#   PAYMENT_AMOUNT       amount in stroops (7 dp). Default 100000000 (10 USDC).
#   STELLAR_NETWORK      default "testnet".
#   STELLAR_RPC_URL      override the RPC endpoint.
#   NON_INTERACTIVE      set to 1 to accept every default without prompting.
#   ENV_FILE             state file. Default .env.testnet.
#
# Requires: bash 5+, stellar CLI, curl, and one of jq / python3 for JSON.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

ENV_FILE="${ENV_FILE:-$REPO_ROOT/.env.testnet}"
STELLAR_NETWORK="${STELLAR_NETWORK:-testnet}"
NON_INTERACTIVE="${NON_INTERACTIVE:-0}"

# ── Output helpers ────────────────────────────────────────────────────────────
# Colour only when stdout is a terminal, so piping to a log file stays readable.
if [[ -t 1 ]]; then
  BOLD=$'\033[1m'; GREEN=$'\033[32m'; YELLOW=$'\033[33m'; RED=$'\033[31m'; DIM=$'\033[2m'; OFF=$'\033[0m'
else
  BOLD=""; GREEN=""; YELLOW=""; RED=""; DIM=""; OFF=""
fi

say()  { printf '%s\n' "$*"; }
step() { printf '%s>%s %s\n' "$BOLD" "$OFF" "$*"; }
ok()   { printf '%s  ✓%s %s\n' "$GREEN" "$OFF" "$*"; }
warn() { printf '%s  !%s %s\n' "$YELLOW" "$OFF" "$*" >&2; }
die()  { printf '%s  ✗%s %s\n' "$RED" "$OFF" "$*" >&2; exit 1; }
note() { printf '%s    %s%s\n' "$DIM" "$*" "$OFF"; }

# ── Preflight ─────────────────────────────────────────────────────────────────

# bash 4 lacks the associative-array and `${var,,}` behaviour used below, and
# macOS still ships bash 3.2 as /bin/bash — so this is checked rather than
# discovered halfway through a deployment.
if (( BASH_VERSINFO[0] < 5 )); then
  die "bash 5+ required (found ${BASH_VERSION}). On macOS: brew install bash, then run with \`bash scripts/onboard.sh\`."
fi

command -v stellar >/dev/null 2>&1 || die "'stellar' CLI not found. Install: cargo install --locked stellar-cli"
command -v curl    >/dev/null 2>&1 || die "'curl' not found."

# One JSON reader, either tool. Avoids making jq a hard dependency for a script
# whose only JSON need is reading two fields out of a Friendbot response.
if command -v jq >/dev/null 2>&1; then
  json_field() { jq -r "$1 // empty" 2>/dev/null; }
elif command -v python3 >/dev/null 2>&1; then
  json_field() {
    local path="${1#.}"
    python3 -c "
import json,sys
try: d=json.load(sys.stdin)
except Exception: sys.exit(0)
for k in '$path'.split('.'):
    if not k: continue
    d = d.get(k) if isinstance(d, dict) else None
    if d is None: sys.exit(0)
print(d)
"
  }
else
  die "Need either 'jq' or 'python3' to read JSON responses."
fi

# ── State file ────────────────────────────────────────────────────────────────

# Sourced rather than parsed so contract IDs written by deploy-testnet.sh are
# picked up as-is. Existing environment variables win, so an explicit
# STELLAR_SECRET_KEY on the command line overrides a saved one.
if [[ -f "$ENV_FILE" ]]; then
  note "Reading existing state from ${ENV_FILE#$REPO_ROOT/}"
  set -a
  # shellcheck disable=SC1090
  source <(grep -E '^[A-Z_][A-Z0-9_]*=' "$ENV_FILE" || true)
  set +a
fi

save_var() {
  local key="$1" value="$2"
  touch "$ENV_FILE"
  if grep -qE "^${key}=" "$ENV_FILE" 2>/dev/null; then
    # Rewritten via a temp file: in-place sed differs between GNU and BSD, and
    # the value can contain characters sed would treat as delimiters.
    local tmp
    tmp="$(mktemp)"
    grep -vE "^${key}=" "$ENV_FILE" > "$tmp" || true
    printf '%s=%s\n' "$key" "$value" >> "$tmp"
    mv -f "$tmp" "$ENV_FILE"
  else
    printf '%s=%s\n' "$key" "$value" >> "$ENV_FILE"
  fi
}

ask() {
  local prompt="$1" default="${2:-}" answer
  if [[ "$NON_INTERACTIVE" == "1" ]]; then
    printf '%s\n' "$default"
    return
  fi
  # Read from the terminal when there is one, so prompts still work if stdout
  # is piped to a log. Falls back to stdin — and then to the default — when
  # there is no tty at all, which is how this behaves under CI without needing
  # NON_INTERACTIVE to be set.
  if [[ -r /dev/tty ]]; then
    if [[ -n "$default" ]]; then
      read -r -p "> ${prompt} [${default}]: " answer </dev/tty || answer=""
    else
      read -r -p "> ${prompt}: " answer </dev/tty || answer=""
    fi
  elif [[ -t 0 ]]; then
    read -r -p "> ${prompt}${default:+ [$default]}: " answer || answer=""
  else
    answer=""
  fi
  printf '%s\n' "${answer:-$default}"
}

# ── Welcome ───────────────────────────────────────────────────────────────────

say ""
say "${BOLD}Welcome to FluxaPay ${STELLAR_NETWORK} onboarding!${OFF}"
say ""

MERCHANT_NAME="${MERCHANT_NAME:-$(ask 'Enter merchant name' 'Acme Store')}"

# ── 1. Keypair ────────────────────────────────────────────────────────────────

step "Resolving a keypair"

if [[ -z "${STELLAR_SECRET_KEY:-}" ]]; then
  say "  ${DIM}Press Enter to generate a new testnet keypair.${OFF}"
  STELLAR_SECRET_KEY="$(ask 'Enter your Stellar testnet secret key' '')"
fi

if [[ -z "$STELLAR_SECRET_KEY" ]]; then
  IDENTITY="fluxapay-onboard"
  stellar keys generate --global "$IDENTITY" --network "$STELLAR_NETWORK" --overwrite >/dev/null 2>&1 \
    || stellar keys generate "$IDENTITY" --network "$STELLAR_NETWORK" --overwrite >/dev/null 2>&1 \
    || die "Could not generate a keypair with the stellar CLI."
  STELLAR_SECRET_KEY="$(stellar keys show "$IDENTITY" 2>/dev/null || true)"
  [[ -n "$STELLAR_SECRET_KEY" ]] || die "Generated a key but could not read it back."
  ok "Generated a new keypair"
else
  [[ "$STELLAR_SECRET_KEY" == S* ]] || die "A Stellar secret key starts with 'S'."
  ok "Using the provided key"
fi

ACCOUNT_ADDRESS="$(stellar keys address --secret-key "$STELLAR_SECRET_KEY" 2>/dev/null \
  || stellar keys address "$STELLAR_SECRET_KEY" 2>/dev/null || true)"
[[ -n "$ACCOUNT_ADDRESS" ]] || die "Could not derive the public address from the secret key."
note "Address: $ACCOUNT_ADDRESS"

# Saved so a re-run is idempotent. Written to .env.testnet, which is gitignored —
# checked here rather than assumed, because leaking a key through a commit is
# not a mistake worth leaving to chance even on testnet.
if ! git check-ignore -q "$ENV_FILE" 2>/dev/null; then
  warn "${ENV_FILE#$REPO_ROOT/} is not gitignored. Add it before committing — it will hold a secret key."
fi
save_var STELLAR_SECRET_KEY "$STELLAR_SECRET_KEY"
save_var MERCHANT_ADDRESS "$ACCOUNT_ADDRESS"

# ── 2. Funding ────────────────────────────────────────────────────────────────

step "Funding the account via Friendbot"

HORIZON="${STELLAR_HORIZON_URL:-https://horizon-testnet.stellar.org}"

if curl -sf --max-time 15 "${HORIZON}/accounts/${ACCOUNT_ADDRESS}" >/dev/null 2>&1; then
  ok "Account already funded — skipping Friendbot"
elif [[ "$STELLAR_NETWORK" != "testnet" ]]; then
  warn "Friendbot only funds testnet. Fund $ACCOUNT_ADDRESS manually and re-run."
else
  if curl -sf --max-time 30 "https://friendbot.stellar.org/?addr=${ACCOUNT_ADDRESS}" >/dev/null 2>&1; then
    ok "Funded via Friendbot"
  else
    # Friendbot rate-limits and occasionally 500s. That is not a reason to lose
    # the run — the account may also already exist from a previous attempt.
    warn "Friendbot did not confirm. Continuing; the account may already exist."
  fi
fi

# ── 3. Contract IDs ───────────────────────────────────────────────────────────

step "Locating deployed contracts"

if [[ -z "${MERCHANT_REGISTRY_CONTRACT_ID:-}" || -z "${PAYMENT_PROCESSOR_CONTRACT_ID:-}" ]]; then
  die "$(cat <<MSG
Contract IDs not found in ${ENV_FILE#$REPO_ROOT/}.
    Deploy first:
      STELLAR_SECRET_KEY=$STELLAR_SECRET_KEY STELLAR_NETWORK=$STELLAR_NETWORK \\
        ./scripts/deploy-testnet.sh
    then re-run this wizard.
MSG
)"
fi

note "MerchantRegistry:  $MERCHANT_REGISTRY_CONTRACT_ID"
note "PaymentProcessor:  $PAYMENT_PROCESSOR_CONTRACT_ID"

STELLAR_COMMON_ARGS=(
  --source-account "$STELLAR_SECRET_KEY"
  --network "$STELLAR_NETWORK"
  --send=yes
)
[[ -n "${STELLAR_RPC_URL:-}" ]] && STELLAR_COMMON_ARGS+=(--rpc-url "$STELLAR_RPC_URL")

invoke() {
  local contract_id="$1"; shift
  stellar contract invoke --id "$contract_id" "${STELLAR_COMMON_ARGS[@]}" -- "$@"
}

# ── 4. Register the merchant ──────────────────────────────────────────────────

step "Registering merchant \"$MERCHANT_NAME\""

# `register_merchant` errors if the merchant already exists, which on a re-run is
# success, not failure — so the existence check comes first and the error is
# tolerated rather than fatal.
if invoke "$MERCHANT_REGISTRY_CONTRACT_ID" get_merchant \
     --merchant_id "$ACCOUNT_ADDRESS" >/dev/null 2>&1; then
  ok "Merchant already registered — skipping"
else
  if invoke "$MERCHANT_REGISTRY_CONTRACT_ID" register_merchant \
       --merchant_id "$ACCOUNT_ADDRESS" \
       --business_name "$MERCHANT_NAME" \
       --settlement_currency USD \
       --payout_address null \
       --bank_account null \
       --fee_config null >/dev/null 2>&1; then
    ok "Merchant registered"
  else
    warn "register_merchant failed. It may already exist, or the admin may need to verify it."
  fi
fi

MERCHANT_SLUG="$(printf '%s' "$MERCHANT_NAME" | tr '[:upper:] ' '[:lower:]_' | tr -cd 'a-z0-9_')"
note "merchant_id: $ACCOUNT_ADDRESS  (${MERCHANT_SLUG})"

# ── 5. Create a payment ───────────────────────────────────────────────────────

PAYMENT_AMOUNT="${PAYMENT_AMOUNT:-100000000}"   # 10 USDC at 7 decimals
HUMAN_AMOUNT="$(awk -v a="$PAYMENT_AMOUNT" 'BEGIN{printf "%.7g", a/10000000}')"

step "Creating a test payment of ${HUMAN_AMOUNT} USDC"

PAYMENT_ID="${SAMPLE_PAYMENT_ID:-onboard_$(date +%s)}"
EXPIRES_AT="$(( $(date +%s) + 3600 ))"

if ! invoke "$PAYMENT_PROCESSOR_CONTRACT_ID" create_payment --args "{
  \"payment_id\": \"$PAYMENT_ID\",
  \"merchant_id\": \"$ACCOUNT_ADDRESS\",
  \"amount\": $PAYMENT_AMOUNT,
  \"currency\": \"USDC\",
  \"deposit_address\": \"$ACCOUNT_ADDRESS\",
  \"expires_at\": $EXPIRES_AT,
  \"duration_secs\": null,
  \"memo\": null,
  \"memo_type\": null,
  \"token_address\": null,
  \"client_token\": \"$PAYMENT_ID\",
  \"metadata_hash\": null,
  \"metadata\": null
}" >/dev/null 2>&1; then
  warn "create_payment failed. The merchant may need the MERCHANT role:"
  note "stellar contract invoke --id $PAYMENT_PROCESSOR_CONTRACT_ID ${STELLAR_COMMON_ARGS[*]} \\"
  note "  -- grant_role --admin <ADMIN> --role MERCHANT --account $ACCOUNT_ADDRESS"
  die "Stopping before the polling step."
fi

ok "Payment created: payment_id = $PAYMENT_ID"
save_var SAMPLE_PAYMENT_ID "$PAYMENT_ID"

# `client_token` is set to the payment id above, so re-running this wizard hits
# the contract's idempotency path and returns the existing payment rather than
# creating a duplicate.

say ""
say "  ${BOLD}Send ${HUMAN_AMOUNT} USDC to:${OFF} $ACCOUNT_ADDRESS"
say "  ${DIM}Payment expires at $(date -r "$EXPIRES_AT" 2>/dev/null || date -d "@$EXPIRES_AT" 2>/dev/null || echo "$EXPIRES_AT")${OFF}"
say ""

# ── 6. Poll for confirmation ──────────────────────────────────────────────────

POLL_ATTEMPTS="${POLL_ATTEMPTS:-20}"
POLL_INTERVAL="${POLL_INTERVAL:-6}"

step "Waiting for confirmation (${POLL_ATTEMPTS} checks, ${POLL_INTERVAL}s apart)"

CONFIRMED=0
for (( attempt = 1; attempt <= POLL_ATTEMPTS; attempt++ )); do
  STATUS_RAW="$(invoke "$PAYMENT_PROCESSOR_CONTRACT_ID" get_payment \
    --payment_id "$PAYMENT_ID" 2>/dev/null || true)"

  if [[ "$STATUS_RAW" == *Confirmed* ]]; then
    CONFIRMED=1
    break
  fi
  if [[ "$STATUS_RAW" == *Expired* || "$STATUS_RAW" == *Cancelled* ]]; then
    warn "Payment reached a terminal non-confirmed state. Re-run to create a fresh one."
    break
  fi

  printf '%s    check %d/%d — still pending%s\r' "$DIM" "$attempt" "$POLL_ATTEMPTS" "$OFF"
  sleep "$POLL_INTERVAL"
done
printf '\n'

EXPLORER="https://stellar.expert/explorer/${STELLAR_NETWORK}/account/${ACCOUNT_ADDRESS}"

if (( CONFIRMED )); then
  ok "Payment confirmed!"
else
  warn "Not confirmed within $(( POLL_ATTEMPTS * POLL_INTERVAL ))s — the deposit may not have arrived yet."
  note "Re-run this script to resume polling; it will reuse payment_id $PAYMENT_ID."
fi

say ""
say "  View on Stellar Expert: $EXPLORER"
say ""
say "${BOLD}Saved to ${ENV_FILE#$REPO_ROOT/}:${OFF}"
note "MERCHANT_ADDRESS=$ACCOUNT_ADDRESS"
note "SAMPLE_PAYMENT_ID=$PAYMENT_ID"
say ""
say "Next: check the status from the SDK —"
note "await client.getPaymentStatuses(['$PAYMENT_ID'])"
say ""
