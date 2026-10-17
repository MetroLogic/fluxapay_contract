//! Issue #1024: Fuzz `fluxapay::refund_manager::RefundManager::min_bond_for`,
//! the proportional minimum dispute bond computation
//! (`max(ABSOLUTE_MIN_BOND, disputed_amount * MIN_BOND_BPS / BPS_DENOMINATOR)`).
//!
//! These constants (issue #820: `ABSOLUTE_MIN_BOND`, `MIN_BOND_BPS`,
//! `BPS_DENOMINATOR`) were declared but had no consumer anywhere in the
//! contract until `min_bond_for` was added alongside this fuzz target.
//! Exercises the full `i128` range for `disputed_amount` — including
//! negative values and `i128::MIN`/`i128::MAX` — to confirm the computation
//! never panics and never returns less than `ABSOLUTE_MIN_BOND`.
#![no_main]

use fluxapay::refund_manager::RefundManager;
use fluxapay::{ABSOLUTE_MIN_BOND, BPS_DENOMINATOR, MIN_BOND_BPS};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::Env;

fuzz_target!(|disputed_amount: i128| {
    let env = Env::default();
    let bond = RefundManager::min_bond_for(env, disputed_amount);

    assert!(
        bond >= ABSOLUTE_MIN_BOND,
        "min_bond_for({}) = {}, below the absolute floor {}",
        disputed_amount, bond, ABSOLUTE_MIN_BOND
    );

    let expected_proportional = disputed_amount.max(0).saturating_mul(MIN_BOND_BPS) / BPS_DENOMINATOR;
    let expected = expected_proportional.max(ABSOLUTE_MIN_BOND);
    assert_eq!(
        bond, expected,
        "min_bond_for({}) = {}, expected {}",
        disputed_amount, bond, expected
    );
});
