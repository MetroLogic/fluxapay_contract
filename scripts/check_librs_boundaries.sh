#!/usr/bin/env bash
# CI guard for ADR-0006 (Issue #1222): keep fluxapay/src/lib.rs a thin wiring file.
#
# Fails if lib.rs grows beyond the recorded baseline (.librs-baseline) in either
# total line count (with a small tolerance) or pub-fn count (strict). The intent
# is to prevent business logic from re-accumulating in lib.rs; new logic belongs
# in submodules. Lower the baseline as logic is moved out — never raise it.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
librs="$repo_root/fluxapay/src/lib.rs"
baseline_file="$repo_root/.librs-baseline"

if [ ! -f "$librs" ]; then
  echo "::error::lib.rs not found at $librs"
  exit 1
fi
if [ ! -f "$baseline_file" ]; then
  echo "::error::.librs-baseline not found; run 'scripts/check_librs_boundaries.sh --update-baseline' once."
  exit 1
fi

# shellcheck disable=SC1090
source "$baseline_file"

line_tolerance="${LIBRS_LINE_TOLERANCE:-50}"

cur_lines=$(wc -l < "$librs" | tr -d ' ')
cur_pub_fns=$(grep -c 'pub fn' "$librs" || true)

status=0

if [ "$cur_lines" -gt "$((BASE_LINES + line_tolerance))" ]; then
  echo "::error file=fluxapay/src/lib.rs::lib.rs grew to ${cur_lines} lines (baseline ${BASE_LINES}, tolerance ${line_tolerance}). Move new logic into a submodule and lower .librs-baseline only if appropriate."
  status=1
fi

if [ "$cur_pub_fns" -gt "$BASE_PUB_FNS" ]; then
  echo "::error file=fluxapay/src/lib.rs::lib.rs pub-fn count grew to ${cur_pub_fns} (baseline ${BASE_PUB_FNS}). New entry points must delegate to submodule logic, not accumulate in lib.rs."
  status=1
fi

if [ "$status" -eq 0 ]; then
  echo "lib.rs boundaries OK: ${cur_lines} lines (<= ${BASE_LINES} + ${line_tolerance}), ${cur_pub_fns} pub fns (<= ${BASE_PUB_FNS})."
fi

exit "$status"
