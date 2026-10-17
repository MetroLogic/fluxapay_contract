#![cfg(test)]

//! Issue #1025: Payment tip/gratuity support.
//!
//! `verify_payment` validates a tip against `MAX_TIP_BPS` (20%) of the base
//! `amount` only when the payment was created with `tip_enabled: true`, and
//! stores the validated tip separately on `PaymentCharge.tip_amount`.

use crate::{
    access_control::{role_merchant, role_oracle},
    CreatePaymentArgs, PaymentProcessor, PaymentProcessorClient,
};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Symbol};

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
    tip_enabled: Option<bool>,
) -> CreatePaymentArgs {
    CreatePaymentArgs {
        payment_id: soroban_sdk::String::from_str(env, payment_id),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 1_000_000_000i128, // 100 USDC at 7 decimals
        currency: Symbol::new(env, "USDC"),
        deposit_address: Address::generate(env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
        tip_enabled,
    }
}

fn grant_oracle(env: &Env, admin: &Address, client: &PaymentProcessorClient<'_>) -> Address {
    let oracle = Address::generate(env);
    client.grant_role(admin, &role_oracle(env), &oracle);
    oracle
}

#[test]
fn test_tip_within_max_is_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, merchant, client) = setup(&env);
    let oracle = grant_oracle(&env, &admin, &client);

    let payment = client.create_payment(&payment_args(&env, "pay_tip_01", &merchant, Some(true)));
    assert!(payment.tip_enabled);
    assert!(payment.tip_amount.is_none());

    // 15% tip — within the 20% (MAX_TIP_BPS) cap.
    let tip = payment.amount * 15 / 100;
    let payer = Address::generate(&env);
    client.verify_payment(
        &oracle,
        &payment.payment_id,
        &BytesN::from_array(&env, &[0u8; 32]),
        &payer,
        &(payment.amount + tip),
        &None,
        &Some(tip),
    );

    let confirmed = client.get_payment(&payment.payment_id);
    assert_eq!(confirmed.tip_amount, Some(tip));
    assert_eq!(confirmed.status, crate::PaymentStatus::Confirmed);
}

#[test]
fn test_zero_tip_with_tip_enabled_is_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, merchant, client) = setup(&env);
    let oracle = grant_oracle(&env, &admin, &client);

    let payment = client.create_payment(&payment_args(&env, "pay_tip_02", &merchant, Some(true)));

    let payer = Address::generate(&env);
    client.verify_payment(
        &oracle,
        &payment.payment_id,
        &BytesN::from_array(&env, &[0u8; 32]),
        &payer,
        &payment.amount,
        &None,
        &Some(0i128),
    );

    let confirmed = client.get_payment(&payment.payment_id);
    assert_eq!(confirmed.tip_amount, Some(0));
    assert_eq!(confirmed.status, crate::PaymentStatus::Confirmed);
}

#[test]
#[should_panic(expected = "Error(Contract, #57)")]
fn test_tip_over_max_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, merchant, client) = setup(&env);
    let oracle = grant_oracle(&env, &admin, &client);

    let payment = client.create_payment(&payment_args(&env, "pay_tip_03", &merchant, Some(true)));

    // 25% tip — exceeds the 20% (MAX_TIP_BPS) cap.
    let tip = payment.amount * 25 / 100;
    let payer = Address::generate(&env);
    client.verify_payment(
        &oracle,
        &payment.payment_id,
        &BytesN::from_array(&env, &[0u8; 32]),
        &payer,
        &(payment.amount + tip),
        &None,
        &Some(tip),
    );
}

#[test]
fn test_tip_ignored_when_not_enabled() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, merchant, client) = setup(&env);
    let oracle = grant_oracle(&env, &admin, &client);

    // tip_enabled defaults to false when omitted.
    let payment = client.create_payment(&payment_args(&env, "pay_tip_04", &merchant, None));
    assert!(!payment.tip_enabled);

    let payer = Address::generate(&env);
    // A tip is passed anyway; since tip_enabled is false it should be ignored
    // (not stored, not validated against MAX_TIP_BPS), so an exact-amount
    // payment still confirms normally with no stored tip.
    client.verify_payment(
        &oracle,
        &payment.payment_id,
        &BytesN::from_array(&env, &[0u8; 32]),
        &payer,
        &payment.amount,
        &None,
        &Some(payment.amount), // would be a 100% "tip" if honored
    );

    let confirmed = client.get_payment(&payment.payment_id);
    assert!(confirmed.tip_amount.is_none());
    assert_eq!(confirmed.status, crate::PaymentStatus::Confirmed);
}
