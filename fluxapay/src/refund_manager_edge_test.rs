#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token, Address, Env, String, Symbol,
};

fn setup_refund_manager(env: &Env) -> RefundManagerClient<'_> {
    let contract_id = env.register(RefundManager, ());
    let client = RefundManagerClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let token_admin = Address::generate(env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    client.initialize_refund_manager(&admin, &token_address);
    token::StellarAssetClient::new(env, &token_address).mint(&contract_id, &1_000_000_000_000i128);

    client
}

fn register_payment(
    env: &Env,
    client: &RefundManagerClient<'_>,
    payment_id: &String,
    amount: i128,
) {
    let merchant = Address::generate(env);
    client.register_payment(payment_id, &merchant, &amount, &Symbol::new(env, "USDC"));
}

fn elapse_refund_cooldown(env: &Env) {
    env.ledger().set_timestamp(
        env.ledger()
            .timestamp()
            .saturating_add(REFUND_COOLDOWN_SECS),
    );
}

fn create_refund(
    env: &Env,
    client: &RefundManagerClient<'_>,
    payment_id: &String,
    amount: i128,
    requester: &Address,
) -> String {
    client.create_refund(
        payment_id,
        &amount,
        &String::from_str(env, "edge case refund"),
        requester,
    )
}

fn set_payment_status(
    env: &Env,
    client: &RefundManagerClient<'_>,
    payment_id: &String,
    status: PaymentStatus,
) {
    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        let key = DataKey::Payment(payment_id.clone());
        let mut payment: PaymentCharge = env.storage().persistent().get(&key).unwrap();
        payment.status = status;
        env.storage().persistent().set(&key, &payment);
    });
}

#[test]
fn test_refund_amount_exceeding_payment_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup_refund_manager(&env);
    let payment_id = String::from_str(&env, "refund_limit");
    register_payment(&env, &client, &payment_id, 1_000);
    elapse_refund_cooldown(&env);
    let requester = Address::generate(&env);

    let result = client.try_create_refund(
        &payment_id,
        &1_001i128,
        &String::from_str(&env, "over payment amount"),
        &requester,
    );

    assert_eq!(result, Err(Ok(Error::RefundExceedsPayment)));
}

#[test]
fn test_refund_on_non_confirmed_payment_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup_refund_manager(&env);
    let payment_id = String::from_str(&env, "refund_pending");
    register_payment(&env, &client, &payment_id, 1_000);
    set_payment_status(&env, &client, &payment_id, PaymentStatus::Pending);
    elapse_refund_cooldown(&env);

    let result = client.try_create_refund(
        &payment_id,
        &100i128,
        &String::from_str(&env, "pending payment"),
        &Address::generate(&env),
    );

    assert_eq!(result, Err(Ok(Error::PaymentAlreadyProcessed)));
}

#[test]
fn test_multiple_partial_refunds_at_payment_amount_succeed() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup_refund_manager(&env);
    let payment_id = String::from_str(&env, "refund_partial_total");
    register_payment(&env, &client, &payment_id, 1_000);
    elapse_refund_cooldown(&env);
    let requester = Address::generate(&env);

    let first_refund = create_refund(&env, &client, &payment_id, 400, &requester);
    let second_refund = client.create_refund(
        &payment_id,
        &600i128,
        &String::from_str(&env, "remaining amount"),
        &requester,
    );

    assert_eq!(client.get_refund(&first_refund).amount, 400);
    assert_eq!(client.get_refund(&second_refund).amount, 600);
    assert_eq!(client.get_payment_refunds(&payment_id).len(), 2);
}

#[test]
fn test_refund_requested_within_cooldown_window_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup_refund_manager(&env);
    let payment_id = String::from_str(&env, "refund_cooldown");
    register_payment(&env, &client, &payment_id, 1_000);

    let result = client.try_create_refund(
        &payment_id,
        &100i128,
        &String::from_str(&env, "too early"),
        &Address::generate(&env),
    );

    assert_eq!(result, Err(Ok(Error::RefundCooldownNotElapsed)));
}

#[test]
fn test_refund_idempotency_returns_existing_refund() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup_refund_manager(&env);
    let payment_id = String::from_str(&env, "refund_idempotent");
    register_payment(&env, &client, &payment_id, 1_000);
    elapse_refund_cooldown(&env);
    let requester = Address::generate(&env);
    let reason = String::from_str(&env, "same request");
    let idempotency_key = Some(String::from_str(&env, "refund-key"));

    let first = client.create_refund_idempotent(
        &payment_id,
        &400i128,
        &reason,
        &requester,
        &idempotency_key,
    );
    let retry = client.create_refund_idempotent(
        &payment_id,
        &400i128,
        &reason,
        &requester,
        &idempotency_key,
    );

    assert_eq!(first, retry);
    assert_eq!(client.get_payment_refunds(&payment_id).len(), 1);
}

#[test]
fn test_process_refund_by_non_operator_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup_refund_manager(&env);
    let payment_id = String::from_str(&env, "refund_process_auth");
    register_payment(&env, &client, &payment_id, 1_000);
    elapse_refund_cooldown(&env);
    let requester = Address::generate(&env);
    let refund_id = create_refund(&env, &client, &payment_id, 100, &requester);

    let result = client.try_process_refund(&Address::generate(&env), &refund_id);

    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_cancel_refund_by_non_requester_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup_refund_manager(&env);
    let payment_id = String::from_str(&env, "refund_cancel_auth");
    register_payment(&env, &client, &payment_id, 1_000);
    elapse_refund_cooldown(&env);
    let requester = Address::generate(&env);
    let refund_id = create_refund(&env, &client, &payment_id, 100, &requester);

    let result = client.try_cancel_refund(&Address::generate(&env), &refund_id);

    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_refund_on_expired_payment_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup_refund_manager(&env);
    let payment_id = String::from_str(&env, "refund_expired_payment");
    register_payment(&env, &client, &payment_id, 1_000);
    set_payment_status(&env, &client, &payment_id, PaymentStatus::Expired);
    elapse_refund_cooldown(&env);

    let result = client.try_create_refund(
        &payment_id,
        &100i128,
        &String::from_str(&env, "expired payment"),
        &Address::generate(&env),
    );

    assert_eq!(result, Err(Ok(Error::PaymentAlreadyProcessed)));
}
