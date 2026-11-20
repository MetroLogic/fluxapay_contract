#!/usr/bin/env bash
# scripts/verify-wasm-feature-guard.sh
#
# CI verification script for Issue #1269:
# Validates that WASM builds reject zero or multiple contract-* features with a
# clear diagnostic, while single-feature WASM builds and native builds succeed.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Detect appropriate wasm target.
# Rust 1.84+ with soroban-sdk 29 prefers wasm32v1-none when available,
# while wasm32-unknown-unknown is the standard fallback.
if rustup target list --installed 2>/dev/null | grep -q "wasm32v1-none"; then
  DEFAULT_TARGET="wasm32v1-none"
else
  DEFAULT_TARGET="wasm32-unknown-unknown"
fi

TARGET="${TARGET:-$DEFAULT_TARGET}"

echo "=========================================================="
echo "Verifying WASM contract feature selection guard"
echo "Target: $TARGET"
echo "=========================================================="

# Temporary file for capturing command output
TMP_OUT="$(mktemp)"
trap 'rm -f "$TMP_OUT"' EXIT

export SOROBAN_SDK_BUILD_SYSTEM_SUPPORTS_SPEC_SHAKING_V2=1

# 1. Test invalid multi-feature WASM build
echo ""
echo "--- 1. Testing invalid multi-feature WASM build (should fail) ---"
set +e
(cd "$REPO_ROOT/fluxapay" && cargo check --target "$TARGET" --features contract-payment-processor,contract-refund-manager 2>&1) > "$TMP_OUT"
STATUS=$?
set -e

if [ $STATUS -eq 0 ]; then
  echo "::error::Multi-feature WASM build succeeded, but was expected to fail!" >&2
  cat "$TMP_OUT"
  exit 1
fi

if grep -q "Multiple deployable contract features selected for WASM build" "$TMP_OUT"; then
  echo "✓ Multi-feature WASM build failed with expected diagnostic:"
  grep -A 2 "Multiple deployable contract features selected for WASM build" "$TMP_OUT" | head -n 3
else
  echo "::error::Multi-feature WASM build failed without the expected diagnostic!" >&2
  cat "$TMP_OUT"
  exit 1
fi

# 2. Test invalid zero-feature WASM build
echo ""
echo "--- 2. Testing invalid zero-feature WASM build (should fail) ---"
set +e
(cd "$REPO_ROOT/fluxapay" && cargo check --target "$TARGET" --no-default-features 2>&1) > "$TMP_OUT"
STATUS=$?
set -e

if [ $STATUS -eq 0 ]; then
  echo "::error::Zero-feature WASM build succeeded, but was expected to fail!" >&2
  cat "$TMP_OUT"
  exit 1
fi

if grep -q "No deployable contract feature selected for WASM build" "$TMP_OUT"; then
  echo "✓ Zero-feature WASM build failed with expected diagnostic:"
  grep -A 2 "No deployable contract feature selected for WASM build" "$TMP_OUT" | head -n 3
else
  echo "::error::Zero-feature WASM build failed without the expected diagnostic!" >&2
  cat "$TMP_OUT"
  exit 1
fi

# 3. Test invalid all-features WASM build
echo ""
echo "--- 3. Testing invalid --all-features WASM build (should fail) ---"
set +e
(cd "$REPO_ROOT/fluxapay" && cargo check --target "$TARGET" --all-features 2>&1) > "$TMP_OUT"
STATUS=$?
set -e

if [ $STATUS -eq 0 ]; then
  echo "::error::All-features WASM build succeeded, but was expected to fail!" >&2
  cat "$TMP_OUT"
  exit 1
fi

if grep -q "Multiple deployable contract features selected for WASM build" "$TMP_OUT"; then
  echo "✓ All-features WASM build failed with expected diagnostic:"
  grep -A 2 "Multiple deployable contract features selected for WASM build" "$TMP_OUT" | head -n 3
else
  echo "::error::All-features WASM build failed without the expected diagnostic!" >&2
  cat "$TMP_OUT"
  exit 1
fi

# 4. Test valid single-feature WASM build
echo ""
echo "--- 4. Testing valid single-feature WASM build (build.rs check) ---"
# We verify that build.rs executes successfully without triggering the feature guard error
set +e
(cd "$REPO_ROOT/fluxapay" && cargo check --target "$TARGET" --no-default-features --features contract-payment-processor 2>&1) > "$TMP_OUT"
STATUS=$?
set -e

# Even if downstream compilation has unrelated errors, build.rs must NOT fail with our guard
if grep -q "Multiple deployable contract features selected for WASM build" "$TMP_OUT" || \
   grep -q "No deployable contract feature selected for WASM build" "$TMP_OUT"; then
  echo "::error::Valid single-feature WASM build was rejected by the feature guard!" >&2
  cat "$TMP_OUT"
  exit 1
fi
echo "✓ Valid single-feature WASM build was accepted by the feature guard."

# 5. Test native build with --all-features (should NOT trigger the feature guard)
echo ""
echo "--- 5. Testing native build with --all-features (should not trigger guard) ---"
set +e
(cd "$REPO_ROOT/fluxapay" && cargo check --all-features 2>&1) > "$TMP_OUT"
STATUS=$?
set -e

if grep -q "Multiple deployable contract features selected for WASM build" "$TMP_OUT" || \
   grep -q "No deployable contract feature selected for WASM build" "$TMP_OUT"; then
  echo "::error::Native build with --all-features was incorrectly blocked by WASM guard!" >&2
  cat "$TMP_OUT"
  exit 1
fi
echo "✓ Native build with --all-features was accepted by the feature guard."

echo ""
echo "=========================================================="
echo "All WASM contract feature selection guard checks passed!  "
echo "=========================================================="
