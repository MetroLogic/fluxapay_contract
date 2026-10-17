//! Issue #1024: Fuzz `fluxapay::stream::compute_total_accrued`, the pure
//! lazy-accrual formula backing `PaymentStreaming::withdraw` /
//! `get_stream_status`:
//!
//! ```text
//! total = min(deposit, checkpoint + (now - last_checkpoint) * rate)
//! ```
//!
//! Exercises edge-case timestamps (including `last_checkpoint_at > now`,
//! which `saturating_sub` turns into zero elapsed time rather than
//! underflowing) and extreme `i128` rates/deposits/checkpoints to confirm
//! the implementation never panics and always returns a value in
//! `[0, max(remaining_deposit, 0)]`, honoring the cliff gate.
#![no_main]

use arbitrary::Arbitrary;
use fluxapay::stream::compute_total_accrued;
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct Input {
    accrued_at_checkpoint: i128,
    last_checkpoint_at: u64,
    now: u64,
    rate_per_second: i128,
    remaining_deposit: i128,
    cliff_at: Option<u64>,
}

fuzz_target!(|input: Input| {
    let total = compute_total_accrued(
        input.accrued_at_checkpoint,
        input.last_checkpoint_at,
        input.now,
        input.rate_per_second,
        input.remaining_deposit,
        input.cliff_at,
    );

    assert!(
        total >= 0,
        "compute_total_accrued({:?}) returned a negative value: {}",
        input, total
    );

    let cap = input.remaining_deposit.max(0);
    assert!(
        total <= cap,
        "compute_total_accrued({:?}) = {} exceeds the remaining deposit cap {}",
        input, total, cap
    );

    if let Some(cliff) = input.cliff_at {
        if input.now < cliff {
            assert_eq!(
                total, 0,
                "compute_total_accrued({:?}) should be 0 before the cliff",
                input
            );
        }
    }
});
