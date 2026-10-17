#!/usr/bin/env bash
# Validates the FluxaPay OpenAPI specification in CI.
# Fails on any lint error reported by @redocly/cli.
set -euo pipefail

SPEC="${1:-docs/openapi.yaml}"

if [ ! -f "$SPEC" ]; then
  echo "error: OpenAPI spec not found at $SPEC" >&2
  exit 1
fi

echo "Linting $SPEC with @redocly/cli..."
npx --yes @redocly/cli@1.25.0 lint "$SPEC"

echo "OpenAPI spec is valid."
