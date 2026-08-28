//! Property-based tests (proptest) for invariants that are hard to fully
//! enumerate with discrete unit tests.
//!
//! CI runs this module with `PROPTEST_CASES=256` (see `.github/workflows/ci.yml`,
//! "Run bounded property tests") so each property below is fuzzed with 256
//! random inputs per run.
//!
//! ## Refund sum invariant (added for #463)
//! - `proptest_refund_sum_never_exceeds_payment` — fuzzes a random
//!   `payment_amount` and a random sequence of partial refund amounts against
//!   `RefundManager::create_refund`, asserting that the running total of
//!   non-rejected refunds never exceeds the payment amount, and that any
//!   rejected request would indeed have caused an overage.
//! - `proptest_concurrent_refund_creation` — same invariant, but with each
//!   refund request in the sequence coming from a distinct requester address
//!   against the same `payment_id`, modeling multiple parties racing to
//!   refund one payment before any request is approved or rejected.

extern crate alloc;
use crate::format_id;
use crate::utils::validate_id;
use alloc::format;
use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, BytesN as _, Ledger as _},
    token, Address, BytesN, Env, String, Symbol,
};

use crate::{
    access_control::{role_merchant, role_oracle, role_settlement_operator},
    BillingInterval, Error, PaymentProcessor, PaymentProcessorClient, PaymentStatus, RefundManager,
    RefundManagerClient, RefundStatus, SubscriptionStatus, PAYMENT_TOLERANCE,
    SUBSCRIPTION_RETRY_INTERVAL_SECS,
};

fn setup_payment_processor(env: &Env) -> (Address, PaymentProcessorClient<'_>) {
    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize_payment_processor(&admin);
    (admin, client)
}

fn setup_refund_manager(env: &Env) -> (Address, RefundManagerClient<'_>) {
    use soroban_sdk::token;

    let contract_id = env.register(RefundManager, ());
    let client = RefundManagerClient::new(env, &contract_id);
    let admin = Address::generate(env);

    let token_admin = Address::generate(env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    client.initialize_refund_manager(&admin, &usdc_token);

    let token_admin_client = token::StellarAssetClient::new(env, &usdc_token);
    token_admin_client.mint(&contract_id, &1_000_000_000_000_000i128);

    (admin, client)
}

fn setup_subscription_env(
    env: &Env,
    mint_payer: bool,
) -> (
    Address,
    RefundManagerClient<'_>,
    Address,
    Address,
    Address,
    Address,
) {
    let contract_id = env.register(RefundManager, ());
    let client = RefundManagerClient::new(env, &contract_id);
    let admin = Address::generate(env);

    let token_admin = Address::generate(env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    client.initialize_refund_manager(&admin, &usdc_token);

    let token_admin_client = token::StellarAssetClient::new(env, &usdc_token);
    token_admin_client.mint(&contract_id, &1_000_000_000_000i128);

    let merchant = Address::generate(env);
    client.grant_role(&admin, &role_merchant(&env), &merchant);

    let operator = Address::generate(env);
    client.grant_role(&admin, &role_oracle(&env), &operator);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    let payer = Address::generate(env);
    if mint_payer {
        token_admin_client.mint(&payer, &1_000_000_000_000i128);
    }

    (admin, client, merchant, usdc_token, payer, operator)
}

proptest! {
    #[test]
    fn test_format_id_starts_with_prefix(n in 0u64..u64::MAX) {
        let env = Env::default();
        let prefix = "refund_";
        let id = format_id(&env, prefix, n);

        let mut arr = [0u8; 64];
        let len = id.len() as usize;
        id.copy_into_slice(&mut arr[..len]);
        let id_str = core::str::from_utf8(&arr[..len]).unwrap();

        assert!(id_str.starts_with(prefix));
    }

    #[test]
    fn test_format_id_uniqueness(n1 in 0u64..u64::MAX, n2 in 0u64..u64::MAX) {
        prop_assume!(n1 != n2);
        let env = Env::default();
        let prefix = "id_";
        let id1 = format_id(&env, prefix, n1);
        let id2 = format_id(&env, prefix, n2);

        assert_ne!(id1, id2);
    }

    #[test]
    fn test_format_id_round_trip(n in 1u64..u64::MAX) {
        let env = Env::default();
        let prefix = "dispute_";
        let id = format_id(&env, prefix, n);

        let mut arr = [0u8; 64];
        let len = id.len() as usize;
        id.copy_into_slice(&mut arr[..len]);
        let id_str = core::str::from_utf8(&arr[..len]).unwrap();

        // Extract the number part
        let num_part = &id_str[prefix.len()..];
        let parsed_n: u64 = num_part.parse().unwrap();

        assert_eq!(n, parsed_n);
    }

    #[test]
    fn test_verify_payment_fails_after_expiry(
        expires_in in 1u64..300u64,
        after_expiry in 1u64..300u64,
        amount in 1i128..1_000_000i128,
        nonce in 0u64..u64::MAX,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let (admin, client) = setup_payment_processor(&env);

        let merchant = Address::generate(&env);
        let oracle = Address::generate(&env);
        client.grant_role(&admin, &role_merchant(&env), &merchant);
        client.grant_role(&admin, &role_oracle(&env), &oracle);

        let payment_id = format_id(&env, "exp_prop_", nonce);
        let expires_at = env.ledger().timestamp() + expires_in;

        let args = crate::CreatePaymentArgs {
            payment_id: payment_id.clone(),
            merchant_id: merchant.clone(),
            payer: None,
            amount,
            currency: Symbol::new(&env, "USDC"),
            deposit_address: Address::generate(&env),
            expires_at: Some(expires_at),
            duration_secs: None,
            memo: None,
            memo_type: None,
            token_address: None,
            client_token: None,
            metadata_hash: None, metadata: None,
            fee_waiver_code: None,
            retry_of_payment_id: None,
            payer_muxed_id: None,
        };

        client.create_payment(&args);

        env.ledger().set_timestamp(expires_at + after_expiry);

        let result = client.try_verify_payment(
            &oracle,
            &payment_id,
            &BytesN::<32>::random(&env),
            &Address::generate(&env),
            &amount,
            &None,
        );

        assert_eq!(result, Err(Ok(Error::PaymentExpired)));
    }

    #[test]
    fn test_verify_payment_amount_boundaries(
        amount in 5i128..1_000_000i128,
        delta in -200i128..200i128,
        nonce in 0u64..u64::MAX,
    ) {
        prop_assume!(amount + delta > 0);

        let env = Env::default();
        env.mock_all_auths();
        let (admin, client) = setup_payment_processor(&env);

        let merchant = Address::generate(&env);
        let oracle = Address::generate(&env);
        client.grant_role(&admin, &role_merchant(&env), &merchant);
        client.grant_role(&admin, &role_oracle(&env), &oracle);

        let payment_id = format_id(&env, "amt_prop_", nonce);
        let expires_at = env.ledger().timestamp() + 3600;

        let args = crate::CreatePaymentArgs {
            payment_id: payment_id.clone(),
            merchant_id: merchant.clone(),
            payer: None,
            amount,
            currency: Symbol::new(&env, "USDC"),
            deposit_address: Address::generate(&env),
            expires_at: Some(expires_at),
            duration_secs: None,
            memo: None,
            memo_type: None,
            token_address: None,
            client_token: None,
            metadata_hash: None, metadata: None,
            fee_waiver_code: None,
            retry_of_payment_id: None,
            payer_muxed_id: None,
        };

        client.create_payment(&args);

        let status = client.verify_payment(
            &oracle,
            &payment_id,
            &BytesN::<32>::random(&env),
            &Address::generate(&env),
            &(amount + delta),
            &None,
        );

        let expected = if delta > PAYMENT_TOLERANCE {
            PaymentStatus::Overpaid
        } else if delta < -PAYMENT_TOLERANCE {
            PaymentStatus::PartiallyPaid
        } else {
            PaymentStatus::Confirmed
        };

        assert_eq!(status, expected);
    }

    #[test]
    fn test_validate_id_valid_chars(
        prefix in "[a-zA-Z0-9]{1,20}",
        suffix in "[a-zA-Z0-9_-]{0,20}",
    ) {
        let env = Env::default();
        let combined = format!("{}{}", prefix, suffix);
        // Only test strings in the 3-64 char range
        prop_assume!(combined.len() >= 3 && combined.len() <= 64);
        let s = soroban_sdk::String::from_str(&env, &combined);
        assert!(validate_id(&s), "expected valid id: {}", combined);
    }

    #[test]
    fn test_validate_id_rejects_too_short(s in "[a-z]{0,2}") {
        let env = Env::default();
        let id = soroban_sdk::String::from_str(&env, &s);
        assert!(!validate_id(&id), "expected invalid (too short): {}", s);
    }

    #[test]
    fn test_validate_id_rejects_too_long(extra in "[a-z]{1,10}") {
        let env = Env::default();
        // Build a 65+ char string
        let base = "a".repeat(64);
        let long_str = format!("{}{}", base, extra);
        let id = soroban_sdk::String::from_str(&env, &long_str);
        assert!(!validate_id(&id), "expected invalid (too long)");
    }

    #[test]
    fn test_validate_id_rejects_disallowed_chars(
        valid in "[a-zA-Z0-9_-]{2,30}",
        bad_char in "[^a-zA-Z0-9_\\-]",
    ) {
        let env = Env::default();
        let with_bad = format!("{}{}", valid, bad_char);
        prop_assume!(with_bad.len() >= 3 && with_bad.len() <= 64);
        // Only test if the bad char is actually non-ASCII or a known disallowed ASCII char
        let has_disallowed = with_bad.bytes().any(|b: u8| {
            !b.is_ascii_alphanumeric() && b != b'-' && b != b'_'
        });
        if has_disallowed {
            let id = soroban_sdk::String::from_str(&env, &with_bad);
            assert!(!validate_id(&id), "expected invalid (bad char): {}", with_bad);
        }
    }

    /// Accrued amount never decreases between two consecutive timestamps.
    #[test]
    fn proptest_stream_accrual_monotonic(
        checkpoint in 0i128..1_000_000_000i128,
        last_at in 0u64..1_000_000u64,
        delta1 in 0u64..10_000u64,
        delta2 in 0u64..10_000u64,
        rate in 0i128..1_000_000i128,
        deposit in 1i128..i128::MAX / 4,
    ) {
        use crate::stream::compute_total_accrued;

        let t1 = last_at.saturating_add(delta1);
        let t2 = t1.saturating_add(delta2);
        let a1 = compute_total_accrued(checkpoint, last_at, t1, rate, deposit);
        let a2 = compute_total_accrued(checkpoint, last_at, t2, rate, deposit);
        prop_assert!(a2 >= a1, "accrual decreased: {} -> {} (t {} -> {})", a1, a2, t1, t2);
    }

    /// Large rates/durations must not overflow or panic (saturating arithmetic).
    #[test]
    fn proptest_stream_accrual_no_overflow(
        checkpoint in 0i128..=i128::MAX / 2,
        last_at in 0u64..u64::MAX / 2,
        now_offset in 0u64..u64::MAX / 2,
        rate in 0i128..=i128::MAX,
        deposit in 0i128..=i128::MAX,
    ) {
        use crate::stream::compute_total_accrued;

        let now = last_at.saturating_add(now_offset);
        let accrued = compute_total_accrued(checkpoint, last_at, now, rate, deposit);
        prop_assert!(accrued >= 0);
        prop_assert!(accrued <= deposit.max(0));
    }

    /// Withdrawal never drives remaining deposit negative.
    #[test]
    fn proptest_stream_remaining_deposit_non_negative(
        remaining in 0i128..=i128::MAX,
        withdraw in 0i128..=i128::MAX,
    ) {
        use crate::stream::compute_remaining_after_withdraw;

        let after = compute_remaining_after_withdraw(remaining, withdraw);
        prop_assert!(after >= 0);
        prop_assert!(after <= remaining.max(0));
    }

    /// The sum of all non-rejected refunds for a payment must never exceed
    /// the original payment amount, no matter what sequence of (possibly
    /// oversized) partial refund amounts is requested against it.
    #[test]
    fn proptest_refund_sum_never_exceeds_payment(
        payment_amount in 1i128..=i128::MAX / 2,
        refund_amounts in prop::collection::vec(1i128..=1_000_000_000i128, 1..8),
        nonce in 0u64..u64::MAX,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let (_admin, client) = setup_refund_manager(&env);

        let payment_id = format_id(&env, "refund_inv_", nonce);
        let merchant_id = Address::generate(&env);
        let requester = Address::generate(&env);

        client.register_payment(
            &payment_id,
            &merchant_id,
            &payment_amount,
            &Symbol::new(&env, "USDC"),
        );

        let mut accepted_total: i128 = 0;

        for &amount in refund_amounts.iter() {
            let reason = soroban_sdk::String::from_str(&env, "prop refund");
            let result = client.try_create_refund(&payment_id, &amount, &reason, &requester);

            match result {
                Ok(_) => {
                    accepted_total += amount;
                    prop_assert!(
                        accepted_total <= payment_amount,
                        "accepted refund total {} exceeded payment amount {}",
                        accepted_total,
                        payment_amount
                    );
                }
                Err(Ok(Error::RefundExceedsPayment)) => {
                    prop_assert!(
                        accepted_total + amount > payment_amount,
                        "refund of {} was rejected but total {} + {} would not have exceeded {}",
                        amount,
                        accepted_total,
                        amount,
                        payment_amount
                    );
                }
                other => prop_assert!(false, "unexpected result: {:?}", other),
            }
        }

        // Cross-check the invariant against contract-tracked refund state directly.
        let refunds = client.get_payment_refunds(&payment_id);
        let mut tracked_total: i128 = 0;
        for r in refunds.iter() {
            if r.status != RefundStatus::Rejected && r.status != RefundStatus::Cancelled {
                tracked_total += r.amount;
            }
        }
        prop_assert_eq!(tracked_total, accepted_total);
        prop_assert!(tracked_total <= payment_amount);
    }

    /// Simulates several requesters racing to refund the same payment: even
    /// when refund requests for the same `payment_id` are interleaved across
    /// different requester addresses (no single requester "owns" the order),
    /// the cumulative non-rejected refund total must still never exceed the
    /// payment amount.
    #[test]
    fn proptest_concurrent_refund_creation(
        payment_amount in 1i128..=1_000_000_000i128,
        refund_amounts in prop::collection::vec(1i128..=500_000_000i128, 2..8),
        nonce in 0u64..u64::MAX,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let (_admin, client) = setup_refund_manager(&env);

        let payment_id = format_id(&env, "refund_race_", nonce);
        let merchant_id = Address::generate(&env);

        client.register_payment(
            &payment_id,
            &merchant_id,
            &payment_amount,
            &Symbol::new(&env, "USDC"),
        );

        // Each "concurrent" request comes from a distinct requester address,
        // all targeting the same payment_id before any are approved/rejected.
        let mut accepted_total: i128 = 0;
        for &amount in refund_amounts.iter() {
            let requester = Address::generate(&env);
            let reason = soroban_sdk::String::from_str(&env, "concurrent refund");
            let result = client.try_create_refund(&payment_id, &amount, &reason, &requester);

            if result.is_ok() {
                accepted_total += amount;
            }
            prop_assert!(accepted_total <= payment_amount);
        }

        let refunds = client.get_payment_refunds(&payment_id);
        let tracked_total: i128 = refunds
            .iter()
            .filter(|r| r.status != RefundStatus::Rejected && r.status != RefundStatus::Cancelled)
            .map(|r| r.amount)
            .sum();
        prop_assert!(tracked_total <= payment_amount);
    }

    /// Issue #681: Subscription cannot be charged before the billing interval has elapsed.
    #[test]
    fn prop_subscription_cannot_charge_before_interval(
        interval_secs in 60u64..=86_400u64,
        amount in 1i128..=1_000_000i128,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let (_admin, client, merchant, _usdc_token, payer, operator) = setup_subscription_env(&env, true);

        let plan_id = String::from_str(&env, "prop_plan");
        client.create_subscription_plan(
            &merchant,
            &plan_id,
            &String::from_str(&env, "Prop Plan"),
            &String::from_str(&env, "prop"),
            &amount,
            &Symbol::new(&env, "USDC"),
            &BillingInterval::Weekly,
        );

        let sub_id = client.subscribe(
            &payer,
            &plan_id,
            &None,
            &None,
            &None,
        );

        // Immediately try to charge - should return Active without charging
        let status = client.process_subscription(&operator, &sub_id);
        assert_eq!(status, SubscriptionStatus::Active);

        let sub = client.get_subscription(&sub_id);
        assert!(sub.last_payment_at.is_none(), "Should not have charged yet");

        // Fast-forward past the interval
        let now = env.ledger().timestamp();
        env.ledger().set_timestamp(now + interval_secs + 1);

        // Now charge should succeed
        let status2 = client.process_subscription(&operator, &sub_id);
        assert_eq!(status2, SubscriptionStatus::Active);

        let sub2 = client.get_subscription(&sub_id);
        assert!(sub2.last_payment_at.is_some());
        let last_charged = sub2.last_payment_at.unwrap();
        assert!(last_charged >= now + interval_secs,
            "last_payment_at {} should be >= now + interval {}",
            last_charged, now + interval_secs);
    }

    /// Issue #681: Subscription charge timestamps are monotonic.
    #[test]
    fn prop_subscription_charge_timestamp_monotonic(
        interval_secs in 60u64..=86_400u64,
        num_charges in 1u32..=5u32,
        amount in 1i128..=1_000_000i128,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let (_admin, client, merchant, _usdc_token, payer, operator) = setup_subscription_env(&env, true);

        let plan_id = String::from_str(&env, "prop_plan_mono");
        client.create_subscription_plan(
            &merchant,
            &plan_id,
            &String::from_str(&env, "Prop Plan Mono"),
            &String::from_str(&env, "prop"),
            &amount,
            &Symbol::new(&env, "USDC"),
            &BillingInterval::Weekly,
        );

        let sub_id = client.subscribe(
            &payer,
            &plan_id,
            &None,
            &None,
            &None,
        );

        let mut last_charged: u64 = 0;
        for i in 0..num_charges {
            let now = env.ledger().timestamp();
            env.ledger().set_timestamp(now + interval_secs + 1);

            let status = client.process_subscription(&operator, &sub_id);
            assert_eq!(status, SubscriptionStatus::Active);

            let sub = client.get_subscription(&sub_id);
            let current_charged = sub.last_payment_at.unwrap();
            assert!(current_charged > last_charged,
                "charge {}: last_payment_at {} should be > previous {}",
                i, current_charged, last_charged);
            last_charged = current_charged;
        }
    }

    /// Issue #681: Retry interval is independent of billing interval.
    #[test]
    fn prop_subscription_retry_interval_independent_of_billing_interval(
        interval_secs in 60u64..=86_400u64,
        amount in 1i128..=1_000_000i128,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let (_admin, client, merchant, _usdc_token, payer, operator) = setup_subscription_env(&env, false);

        let plan_id = String::from_str(&env, "prop_plan_retry");
        client.create_subscription_plan(
            &merchant,
            &plan_id,
            &String::from_str(&env, "Prop Plan Retry"),
            &String::from_str(&env, "prop"),
            &amount,
            &Symbol::new(&env, "USDC"),
            &BillingInterval::Weekly,
        );

        let sub_id = client.subscribe(
            &payer,
            &plan_id,
            &None,
            &None,
            &None,
        );

        // Advance time to make subscription due
        let now = env.ledger().timestamp();
        env.ledger().set_timestamp(now + interval_secs + 1);

        // Charge will fail because payer has no tokens
        let status = client.try_process_subscription(&operator, &sub_id);
        assert_eq!(status, Err(Ok(Error::SubscriptionInGracePeriod)));

        let sub = client.get_subscription(&sub_id);
        let expected_retry = now + interval_secs + 1 + SUBSCRIPTION_RETRY_INTERVAL_SECS;
        assert_eq!(sub.next_retry_at, Some(expected_retry),
            "next_retry_at {:?} should equal now + SUBSCRIPTION_RETRY_INTERVAL_SECS ({})",
            sub.next_retry_at, expected_retry);
    }
}
