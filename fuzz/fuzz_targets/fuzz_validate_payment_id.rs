//! Issue #674: Fuzz `fluxapay::utils::validate_id`, the shared format
//! validator for payment IDs, dispute IDs, refund IDs, and link IDs
//! (`fluxapay/src/utils.rs`).
//!
//! `validate_id` is documented to accept 3-64 ASCII alphanumeric/`-`/`_`
//! characters and reject everything else. This target hands it completely
//! arbitrary bytes — not necessarily valid UTF-8 — to check the function
//! never panics on unicode, null bytes, control characters, or oversized
//! input, and that its length/charset invariants hold on every input it
//! accepts.
#![no_main]

use libfuzzer_sys::fuzz_target;
use soroban_sdk::{Env, String as SorobanString};

// `soroban_sdk::String` is internally capped; anything past this is not a
// realistic on-chain ID and would fail length validation anyway, so cap the
// fuzzer's input to keep each run fast without losing coverage of the
// "very long string" edge case (64 is the validator's own max, so we go a
// couple orders of magnitude past it).
const MAX_INPUT_LEN: usize = 4096;

fuzz_target!(|data: &[u8]| {
    let capped = if data.len() > MAX_INPUT_LEN {
        &data[..MAX_INPUT_LEN]
    } else {
        data
    };

    let env = Env::default();
    // `String::from_bytes` does not require valid UTF-8 in the Soroban
    // sandbox host, mirroring what a malicious client could submit as
    // `CreatePaymentArgs.payment_id` / `create_dispute`'s `payment_id`.
    let candidate = SorobanString::from_bytes(&env, capped);

    let is_valid = fluxapay::validate_id(&candidate);

    // Cross-check the validator's own documented invariants (length 3-64,
    // ASCII alphanumeric / '-' / '_' only) against whatever it just
    // returned, using an independent re-implementation over the raw bytes.
    // A divergence here means `validate_id` accepted something outside its
    // documented contract (or rejected something inside it) — flag it as a
    // fuzz failure rather than only checking for panics.
    let expected = capped.len() >= 3
        && capped.len() <= 64
        && capped
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_');

    assert_eq!(
        is_valid, expected,
        "validate_id({:?}) returned {}, expected {} per its documented contract",
        capped, is_valid, expected
    );
});
