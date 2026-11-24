#![cfg(test)]

//! Issue #1037: `create_payment` retry validation.
//!
//! These cover the `retry_of_payment_id` path added to `create_payment`
//! directly (distinct from the pre-existing standalone `retry_payment`
//! entry point from issue #482): a retry must reference an existing,
//! same-merchant payment that is in `Expired` status, or `create_payment`
//! returns `Error::InvalidRetryTarget`.

use crate::{
    access_control::{role_merchant, role_oracle},
    CreatePaymentArgs, PaymentProcessor, PaymentProcessorClient,
};
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, BytesN, Env, Symbol};

fn setup(env: &Env) -> (Address, Address, PaymentProcessorClient<'_>) {
    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize_payment_processor(&admin);
    let merchant = Address::generate(env);
    client.grant_role(&admin, &role_merchant(env), &merchant);
    (admin, merchant, client)
}

fn payment_args(
    env: &Env,
    payment_id: &str,
    merchant: &Address,
    expires_at: u64,
    retry_of_payment_id: Option<soroban_sdk::String>,
) -> CreatePaymentArgs {
    CreatePaymentArgs {
        payment_id: soroban_sdk::String::from_str(env, payment_id),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 1_000_000_000i128,
        currency: Symbol::new(env, "USDC"),
        deposit_address: Address::generate(env),
        expires_at: Some(expires_at),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id,
        payer_muxed_id: None,
    }
}

/// Advances the ledger past `expires_at` and permissionlessly expires the payment.
fn expire(env: &Env, client: &PaymentProcessorClient<'_>, payment_id: &soroban_sdk::String, expires_at: u64) {
    env.ledger().set_timestamp(expires_at + 1);
    client.expire_payment(payment_id);
}

#[test]
fn test_retry_of_expired_payment_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, merchant, client) = setup(&env);

    let now = env.ledger().timestamp();
    let original_expires_at = now + 3600;
    let original = client.create_payment(&payment_args(
        &env,
        "pay_original_01",
        &merchant,
        original_expires_at,
        None,
    ));

    expire(&env, &client, &original.payment_id, original_expires_at);

    let retry = client.create_payment(&payment_args(
        &env,
        "pay_retry_01",
        &merchant,
        env.ledger().timestamp() + 3600,
        Some(original.payment_id.clone()),
    ));

    assert_eq!(retry.retry_of_payment_id, Some(original.payment_id.clone()));

    // A second 2-tuple-topic event (PAYMENT/RETRY_CREATED, alongside
    // PAYMENT/CREATED) should have been emitted for this retry.
    let events = env.events().all();
    let two_tuple_topic_events = events.iter().filter(|(_, topics, _)| topics.len() == 2).count();
    assert!(
        two_tuple_topic_events >= 2,
        "Expected both PAYMENT/CREATED and PAYMENT/RETRY_CREATED events, got {} 2-tuple-topic events",
        two_tuple_topic_events
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #56)")]
fn test_retry_of_non_expired_payment_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, merchant, client) = setup(&env);

    let now = env.ledger().timestamp();
    let original_expires_at = now + 3600;
    let original = client.create_payment(&payment_args(
        &env,
        "pay_original_02",
        &merchant,
        original_expires_at,
        None,
    ));

    // Still Pending (not expired) — retrying against it must fail.
    client.create_payment(&payment_args(
        &env,
        "pay_retry_02",
        &merchant,
        now + 7200,
        Some(original.payment_id.clone()),
    ));
}

#[test]
#[should_panic(expected = "Error(Contract, #56)")]
fn test_retry_of_confirmed_payment_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, merchant, client) = setup(&env);

    let now = env.ledger().timestamp();
    let original_expires_at = now + 3600;
    let original = client.create_payment(&payment_args(
        &env,
        "pay_original_03",
        &merchant,
        original_expires_at,
        None,
    ));

    // Confirm the original via the oracle role so its status is `Confirmed`.
    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &oracle);
    let payer = Address::generate(&env);
    client.verify_payment(
        &oracle,
        &original.payment_id,
        &BytesN::from_array(&env, &[0u8; 32]),
        &payer,
        &original.amount,
        &None,
        &None,
    );

    // Confirmed, not Expired — retrying against it must fail.
    client.create_payment(&payment_args(
        &env,
        "pay_retry_03",
        &merchant,
        now + 7200,
        Some(original.payment_id.clone()),
    ));
}

#[test]
#[should_panic(expected = "Error(Contract, #56)")]
fn test_retry_of_nonexistent_payment_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, merchant, client) = setup(&env);

    let now = env.ledger().timestamp();
    client.create_payment(&payment_args(
        &env,
        "pay_retry_04",
        &merchant,
        now + 3600,
        Some(soroban_sdk::String::from_str(&env, "pay_does_not_exist")),
    ));
}

#[test]
#[should_panic(expected = "Error(Contract, #56)")]
fn test_retry_by_different_merchant_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, merchant, client) = setup(&env);

    let now = env.ledger().timestamp();
    let original_expires_at = now + 3600;
    let original = client.create_payment(&payment_args(
        &env,
        "pay_original_05",
        &merchant,
        original_expires_at,
        None,
    ));
    expire(&env, &client, &original.payment_id, original_expires_at);

    // A different merchant tries to retry someone else's expired payment.
    let other_merchant = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &other_merchant);

    client.create_payment(&payment_args(
        &env,
        "pay_retry_05",
        &other_merchant,
        env.ledger().timestamp() + 3600,
        Some(original.payment_id.clone()),
    ));
}
