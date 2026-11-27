//! Issue #1024: Fuzz `fluxapay::refund_manager::compute_refund_fee`, the
//! shared refund-fee helper backing `RefundManager::process_refund` /
//! `RefundManager::process_refund_direct`.
//!
//! Each call site previously computed `refund_amount * fee_bps / 10_000`
//! inline with the plain (non-saturating) `*` operator. `fee_bps` is always
//! admin-bounded to 0..=1000 via `set_refund_fee_bps`, but `refund_amount` is
//! an unconstrained `i128` read back from storage, so an implausibly large
//! stored amount could overflow that multiplication and panic. This target
//! throws the full `i128` range at both arguments (not just realistic
//! payment amounts) to confirm the now-saturating implementation never
//! panics.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct Input {
    refund_amount: i128,
    fee_bps: i128,
}

fuzz_target!(|input: Input| {
    // Must never panic regardless of how extreme the inputs are.
    let fee = fluxapay::refund_manager::compute_refund_fee(input.refund_amount, input.fee_bps);

    // Cross-check against an independent saturating re-implementation of
    // the documented formula.
    let expected = input.refund_amount.saturating_mul(input.fee_bps) / 10_000i128;

    assert_eq!(
        fee, expected,
        "compute_refund_fee({}, {}) = {}, expected {}",
        input.refund_amount, input.fee_bps, fee, expected
    );
});
