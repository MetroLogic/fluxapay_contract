//! Issue #1168: boundary/property coverage for i128 fee arithmetic.

use crate::checked_amount_bps;

#[test]
fn zero_amount_and_zero_bps() {
    assert_eq!(checked_amount_bps(0, 10_000), Some(0));
    assert_eq!(checked_amount_bps(1_000, 0), Some(0));
}

#[test]
fn typical_fee_calculation() {
    assert_eq!(checked_amount_bps(10_000, 250), Some(250));
    assert_eq!(checked_amount_bps(1_000_000_000, 1), Some(100_000));
}

#[test]
fn max_value_with_zero_bps_does_not_overflow() {
    assert_eq!(checked_amount_bps(i128::MAX, 0), Some(0));
}

#[test]
fn overflow_is_rejected_instead_of_wrapping() {
    assert_eq!(checked_amount_bps(i128::MAX, 10_000), None);
    assert_eq!(checked_amount_bps(i128::MIN, 2), None);
}

#[test]
fn max_amount_with_small_bps_is_checked() {
    // i128::MAX / 10_000 rounds down; the multiplication itself is still
    // checked and this boundary must not panic.
    let fee = checked_amount_bps(i128::MAX, 1).expect("must not overflow at 1 bps");
    assert!(fee > 0);
    assert!(fee <= i128::MAX);
}

#[test]
fn property_fee_never_exceeds_amount_for_small_bps() {
    let mut amount: i128 = 0;
    while amount < 100_000 {
        for bps in [0i128, 1, 100, 10_000] {
            let fee = checked_amount_bps(amount, bps).expect("in-range amount");
            assert!(fee >= 0);
            assert!(fee <= amount);
        }
        amount += 1_337;
    }
}
