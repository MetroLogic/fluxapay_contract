#!/usr/bin/env bash

# check-env.sh: Compare .env against .env.example and warn about missing keys.
# Non-fatal: always exits 0 so it can be used in pre-commit hooks and CI.

set -uo pipefail

ENV_EXAMPLE="${ENV_EXAMPLE:-.env.example}"
ENV_FILE="${ENV_FILE:-.env}"

if [ ! -f "$ENV_EXAMPLE" ]; then
  echo "warn: $ENV_EXAMPLE not found; skipping env check." >&2
  exit 0
fi

if [ ! -f "$ENV_FILE" ]; then
  echo "warn: $ENV_FILE not found. Copy $ENV_EXAMPLE to $ENV_FILE and fill in your values." >&2
  exit 0
fi

# Extract key names (left of the first '=') from the example file,
# ignoring blank lines and comments.
extract_keys() {
  grep -Ev '^[[:space:]]*(#|$)' "$1" | sed -e 's/^[[:space:]]*//' -e 's/=.*//' | sed -e 's/[[:space:]]*$//'
}

missing=0
while IFR= read -r key; do
  [ -z "$key" ] && continue
  if ! grep -Eq "^[[:space:]]*${key}[ ]*=" "$ENV_FILE"; then
    echo "warn: missing environment variable: $key" >&2
    missing=$((missing + 1))
  fi
done < < (extract_keys "$ENV_EXAMPLE")

if [ "$missing" -gt 0 ]; then
  echo "warn: $missing key(s) missing from $ENV_FILE. See $ENV_EXAMPLE for the full list." >&2
else
  echo "ok: $ENV_FILE contains all keys from $ENV_EXAMPLE." >&2
fi

exit 0
