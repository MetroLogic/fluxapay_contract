#!/usr/bin/env bash
# scripts/update-wasm-baseline.sh
#
# Rebuilds and optimizes the FluxaPay contract WASM binary and writes its
# current size (in bytes) to .wasm-size-baseline at the repo root.
#
# Run this after any change that intentionally affects the compiled contract
# size (new dependency, new public function, large refactor) so the CI
# "WASM Size Gate" job in .github/workflows/ci.yml compares against the
# correct baseline instead of failing on an expected increase.
#
# Usage:
#   ./scripts/update-wasm-baseline.sh
#
# The script always rebuilds in release mode for wasm32-unknown-unknown and
# applies `stellar contract optimize` (wasm-opt -O3), matching what CI does,
# so the recorded baseline matches what CI will measure.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BASELINE_FILE="$REPO_ROOT/.wasm-size-baseline"
WASM_DIR="$REPO_ROOT/target/wasm32-unknown-unknown/release"
WASM_PATH="$WASM_DIR/fluxapay.wasm"
HARD_CAP_BYTES=$((245 * 1024))

echo "Building fluxapay contract (release, wasm32-unknown-unknown)..."
(cd "$REPO_ROOT/fluxapay" && stellar contract build)

echo "Optimizing WASM binary..."
(cd "$REPO_ROOT/fluxapay" && stellar contract optimize --wasm "$WASM_PATH")

if [ ! -f "$WASM_PATH" ]; then
  echo "❌ Could not find built WASM at $WASM_PATH" >&2
  exit 1
fi

NEW_SIZE=$(stat -c%s "$WASM_PATH")

if [ "$NEW_SIZE" -gt "$HARD_CAP_BYTES" ]; then
  echo "❌ Refusing to record baseline: $NEW_SIZE bytes exceeds the hard cap of $HARD_CAP_BYTES bytes (245 KB)." >&2
  echo "   Soroban's upload limit is 256 KB; this leaves no safety margin. Reduce the binary size first." >&2
  exit 1
fi

OLD_SIZE="(none)"
if [ -f "$BASELINE_FILE" ]; then
  OLD_SIZE=$(cat "$BASELINE_FILE")
fi

echo "$NEW_SIZE" > "$BASELINE_FILE"

echo ""
echo "Updated $BASELINE_FILE"
echo "  previous baseline: $OLD_SIZE bytes"
echo "  new baseline:      $NEW_SIZE bytes"
echo ""
echo "Commit this file along with your change so CI's WASM size gate uses the new baseline."
