//! Issue #674: Fuzz payment/dispute ID creation and parsing.
//!
//! Two invariants are checked here, both reachable from the
//! `create_payment` / `create_dispute` entry points in `fluxapay/src/lib.rs`,
//! which gate on `utils::validate_id(&args.payment_id)` /
//! `utils::validate_id(&dispute_id)`:
//!
//! 1. Every ID the *contract itself* mints via `format_id` (used for
//!    payment, dispute, refund, and link IDs — e.g.
//!    `format_id(&env, "dispute_", n)` in `lib.rs`) must always be accepted
//!    by `validate_id`. If it weren't, the contract would reject IDs it
//!    generated itself.
//! 2. An arbitrary *client-supplied* ID (unicode text, as opposed to
//!    `fuzz_validate_payment_id.rs`'s raw/non-UTF8 bytes) must be classified
//!    consistently with `validate_id`'s documented contract and must never
//!    panic while doing so.
//!
//! NOTE: this intentionally exercises `format_id`/`validate_id` directly
//! rather than calling `PaymentProcessor::create_payment` end-to-end.
//! `CreatePaymentArgs` gained fields (`retry_of_payment_id`,
//! `payer_muxed_id`) that existing call sites in `test.rs`/`proptests.rs`
//! don't set either, so a full-contract fuzz target should follow once
//! those call sites are reconciled — tracked as a follow-up, not blocking
//! this ID-validation coverage.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{Env, String as SorobanString};

const MAX_EXTERNAL_ID_LEN: usize = 4096;

#[derive(Debug, Arbitrary)]
enum IdKind {
    Payment,
    Dispute,
    Refund,
    Link,
}

#[derive(Debug, Arbitrary)]
struct Input {
    kind: IdKind,
    counter: u64,
    /// A client-supplied candidate ID, as `create_payment`/`create_dispute`
    /// would receive it. `arbitrary` generates valid (but otherwise
    /// unconstrained) Unicode text here.
    external_id: String,
}

fuzz_target!(|input: Input| {
    let env = Env::default();

    // 1. Contract-generated IDs must always validate.
    let prefix = match input.kind {
        IdKind::Payment => "payment_",
        IdKind::Dispute => "dispute_",
        IdKind::Refund => "refund_",
        IdKind::Link => "link_",
    };
    let generated = fluxapay::format_id(&env, prefix, input.counter);
    assert!(
        fluxapay::validate_id(&generated),
        "format_id(env, {:?}, {}) produced an ID rejected by its own validate_id gate",
        prefix, input.counter
    );

    // 2. Client-supplied IDs must be classified consistently, and must
    //    never panic, regardless of Unicode content, length, or empty
    //    input.
    let bytes = input.external_id.as_bytes();
    let capped = if bytes.len() > MAX_EXTERNAL_ID_LEN {
        &bytes[..MAX_EXTERNAL_ID_LEN]
    } else {
        bytes
    };
    let candidate = SorobanString::from_bytes(&env, capped);
    let is_valid = fluxapay::validate_id(&candidate);

    let expected = capped.len() >= 3
        && capped.len() <= 64
        && capped
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_');

    assert_eq!(
        is_valid, expected,
        "validate_id disagreed with its documented contract for external_id derived from {:?}",
        input.external_id
    );
});
