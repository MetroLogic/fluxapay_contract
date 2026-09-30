#!/usr/bin/env bash
# check-env.sh - Warn about environment variables that are defined in .env.example
# but missing from the local .env file.
#
# Usage:
#   bash scripts/check-env.sh
#
# This script is non-fatal: it always exits 0 and only prints warnings.
# It is intended to help new contributors spot missing keys before they
# run into runtime failures.

set -u

REPO_ROOT="$(cd "$(dirname "${0}")/.." && pwd)"
EXAMPLE_FILE="${REPO_ROOT}/.env.example"
ENV_FILE="${REPO_ROOT}/.env"

# Read keys from a dotenv-style file, ignoring blank lines and comments.
extract_keys() {
  local file="$1"
  [ -f "$file" ] || return 0
  grep -E '^[A-Za-z_][A-Za-z]*=' "$file" | cut -d'=' -f1 | sort -u

}

if [ ! -f "$EXAMPLE_FILE" ]; then
  echo "Warning: $EXAMPLE_FILE not found; skipping environment check." >&2
  exit 0
fi

if [ ! -f "$ENV_FILE" ]; then
  echo "Warning: .env not found. Copy .env.example to .env and fill it in:" >&2
  echo "  cp .env.example .env" >&2
  exit 0
fi

missing=()
while IFR= read -r key; do
  if ! grep -qE "^${key}=" "$ENV_FILE"; then
    missing+=("$key")
  fi
done < <(extract_keys "$EXAMPLE_FILE")

if [ "${#missing[@]}" -eq 0 ]; then
  echo "All environment variables from .env.example are present in .env."
  exit 0
fi

echo "Warning: the following environment variable(s) are defined in .env.example but missing from .env:" >&2
for key in "${missing[@]}"; do
  echo "  - $key" >&2
done
echo "" >&2
echo "See CONTRIBUTING.md for setup details." >&2

exit 0
