# i128 amount arithmetic review (issue #1168)

## Approach

Fee calculations follow the form `amount * bps / 10_000`. The intermediate
product can overflow i128 even when the final fee would fit. All reviewed
state-mutating call sites now use the shared helper
`checked_amount_bps(amount, bps)` in `fluxapay/src/lib.rs`, which uses
`i128::checked_mul` followed by `checked_div` and returns `None` on overflow.

## Reviewed call sites (fluxapay/src/lib.rs)

| Call site | Before | After |
| --- | --- | --- |
| `process_refund_internal` refund fee | `refund.amount * fee_bps / 10_000` | `checked_amount_bps(...).ok_or(InvalidAmount)?` |
| `process_refund_internal` net amount | `refund.amount - fee` | `checked_sub(...).ok_or(InvalidAmount)?` |
| `refund_instantly` fee and net | unchecked multiply/subtract | checked |
| `finalize_dispute_vote` stake slash | `stake * slash_bps / 10_000` | checked |
| `submit_usage_metrics` metered amount | `saturating_mul` (silent truncation) | `checked_mul(...).ok_or(InvalidAmount)?` |
| `generate_reconciliation_report` refund fee | unchecked multiply | saturated (read-only report must not abort) |
| `generate_reconciliation_report` gross total | `total_gross += ...` (release-mode wrap risk) | `saturating_add` |
| `settle_payment` settlement fee | unchecked multiply | checked |
| `settle_payment` developer split | unchecked multiply | checked |
| `settle_payment` merchant platform fee | unchecked multiply | checked |

Other accumulators and timestamp arithmetic in the same file already used
`saturating_add` / `saturating_sub` and were left unchanged.

## Tests

`fluxapay/src/amount_math_test.rs` covers zero amount, zero bps, typical fees,
`i128::MAX` with 0/1 bps, overflow rejection for `i128::MAX` and `i128::MIN`,
and a boundary sweep asserting `0 <= fee <= amount` for amounts up to 100,000.

Note: the repository currently has pre-existing compile errors in unrelated
test modules; these tests were added but the full test binary could not be
built in this environment.
