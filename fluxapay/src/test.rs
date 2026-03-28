#![cfg(test)]

use super::*;
use access_control::{role_admin, role_merchant, role_oracle, role_settlement_operator};
use soroban_sdk::{
    testutils::{Address as _, BytesN as _, Ledger as _},
    token, Address, BytesN, Env, String, Symbol,
};

fn setup_payment_processor(env: &Env) -> (Address, PaymentProcessorClient<'_>) {
    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize_payment_processor(&admin);
    (admin, client)
}

fn setup_refund_manager(env: &Env) -> (Address, RefundManagerClient<'_>) {
    let contract_id = env.register(RefundManager, ());
    let client = RefundManagerClient::new(env, &contract_id);
    let admin = Address::generate(env);

    let token_admin = Address::generate(env);
    let usdc_token = env.register_stellar_asset_contract_v2(token_admin).address();
    client.initialize_refund_manager(&admin, &usdc_token);

    let token_admin_client = token::StellarAssetClient::new(env, &usdc_token);
    token_admin_client.mint(&contract_id, &1_000_000_000_000i128);

    (admin, client)
}

#[test]
fn test_create_payment() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_123");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128; // 1000 USDC (6 decimals)
    let currency = Symbol::new(&env, "USDC");
    let deposit_address = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600;
    client.payment_grant_role(&admin, &role_merchant(&env), &merchant_id);

    let payment = client.create_payment(
        &payment_id,
        &merchant_id,
        &amount,
        &currency,
        &deposit_address,
        &expires_at,
    );

    assert_eq!(payment.payment_id, payment_id);
    assert_eq!(payment.merchant_id, merchant_id);
    assert_eq!(payment.amount, amount);
    assert_eq!(payment.currency, currency);
    assert_eq!(payment.deposit_address, deposit_address);
    assert_eq!(payment.status, PaymentStatus::Pending);
}

#[test]
fn test_verify_payment_success() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_123");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    let expires_at = env.ledger().timestamp() + 3600;
    client.payment_grant_role(&admin, &role_merchant(&env), &merchant_id);

    client.create_payment(
        &payment_id,
        &merchant_id,
        &amount,
        &Symbol::new(&env, "USDC"),
        &Address::generate(&env),
        &expires_at,
    );

    let payer_address = Address::generate(&env);
    let transaction_hash = BytesN::<32>::random(&env);
    let oracle = Address::generate(&env);
    client.payment_grant_role(&admin, &role_oracle(&env), &oracle);

    let status = client.verify_payment(
        &oracle,
        &payment_id,
        &transaction_hash,
        &payer_address,
        &amount,
    );

    assert_eq!(status, PaymentStatus::Confirmed);
    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Confirmed);
}

#[test]
fn test_get_merchant_payments_index_and_pagination() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    let currency = Symbol::new(&env, "USDC");
    let deposit_address = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600;

    let payment_id_1 = String::from_str(&env, "merchant_pay_1");
    let payment_id_2 = String::from_str(&env, "merchant_pay_2");
    let payment_id_3 = String::from_str(&env, "merchant_pay_3");

    client.payment_grant_role(&admin, &role_merchant(&env), &merchant_id);
    client.create_payment(
        &payment_id_1,
        &merchant_id,
        &100i128,
        &currency,
        &deposit_address,
        &expires_at,
    );
    client.create_payment(
        &payment_id_2,
        &merchant_id,
        &200i128,
        &currency,
        &deposit_address,
        &expires_at,
    );
    client.create_payment(
        &payment_id_3,
        &merchant_id,
        &300i128,
        &currency,
        &deposit_address,
        &expires_at,
    );

    let all = client.get_merchant_payments(&merchant_id);
    assert_eq!(all.len(), 3);
    assert_eq!(all.get(0), Some(payment_id_1.clone()));
    assert_eq!(all.get(1), Some(payment_id_2.clone()));
    assert_eq!(all.get(2), Some(payment_id_3.clone()));

    let page = client.get_merchant_payments_paginated(&merchant_id, &1u32, &2u32);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0), Some(payment_id_2));
    assert_eq!(page.get(1), Some(payment_id_3));
}

#[test]
fn test_cancel_payment_before_expiry_by_merchant() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "cancel_before_expiry");
    let merchant_id = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600;
    client.payment_grant_role(&admin, &role_merchant(&env), &merchant_id);

    client.create_payment(
        &payment_id,
        &merchant_id,
        &500i128,
        &Symbol::new(&env, "USDC"),
        &Address::generate(&env),
        &expires_at,
    );

    client.cancel_payment(&merchant_id, &payment_id);

    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Failed);
}

#[test]
fn test_expire_payment_after_deadline() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "expire_after_deadline");
    let merchant_id = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 10;
    client.payment_grant_role(&admin, &role_merchant(&env), &merchant_id);

    client.create_payment(
        &payment_id,
        &merchant_id,
        &500i128,
        &Symbol::new(&env, "USDC"),
        &Address::generate(&env),
        &expires_at,
    );

    env.ledger().set_timestamp(expires_at + 1);
    client.expire_payment(&payment_id);

    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Expired);
}

#[test]
fn test_create_and_get_refund() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_123");
    let merchant_id = Address::generate(&env);
    let refund_amount = 1000i128;
    let reason = String::from_str(&env, "Reason");
    let requester = Address::generate(&env);

    // Register payment so refund amount can be validated
    client.register_payment(&payment_id, &merchant_id, &5000i128, &Symbol::new(&env, "USDC"));

    let refund_id = client.create_refund(&payment_id, &refund_amount, &reason, &requester);
    let refund = client.get_refund(&refund_id);

    assert_eq!(refund.payment_id, payment_id);
    assert_eq!(refund.amount, refund_amount);
    assert_eq!(refund.status, RefundStatus::Pending);
}

#[test]
fn test_process_refund() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_123");
    let merchant_id = Address::generate(&env);
    let refund_amount = 1000i128;
    let requester = Address::generate(&env);

    client.register_payment(&payment_id, &merchant_id, &5000i128, &Symbol::new(&env, "USDC"));

    let refund_id = client.create_refund(
        &payment_id,
        &refund_amount,
        &String::from_str(&env, "Reason"),
        &requester,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    client.process_refund(&operator, &refund_id);

    let refund = client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Completed);
}

#[test]
fn test_initialize_contract() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let usdc_token = env.register_stellar_asset_contract_v2(token_admin).address();

    let contract_id = env.register(RefundManager, ());
    let client = RefundManagerClient::new(&env, &contract_id);
    client.initialize_refund_manager(&admin, &usdc_token);

    assert_eq!(client.get_admin(), Some(admin.clone()));
    assert!(client.has_role(&role_admin(&env), &admin));
}

#[test]
fn test_grant_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);
    let account = Address::generate(&env);
    let role = role_oracle(&env);

    client.grant_role(&admin, &role, &account);
    assert!(client.has_role(&role, &account));
}

#[test]
fn test_transfer_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (current_admin, client) = setup_refund_manager(&env);
    let new_admin = Address::generate(&env);

    client.transfer_admin(&current_admin, &new_admin);
    assert!(client.has_role(&role_admin(&env), &new_admin));
    assert_eq!(client.get_admin(), Some(new_admin));
}

#[test]
fn test_multiple_refunds_unique_ids() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_123");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);

    client.register_payment(&payment_id, &merchant_id, &5000i128, &Symbol::new(&env, "USDC"));

    // Create first refund
    let refund_id_1 = client.create_refund(
        &payment_id,
        &1000i128,
        &String::from_str(&env, "First refund"),
        &requester,
    );

    // Create second refund
    let refund_id_2 = client.create_refund(
        &payment_id,
        &500i128,
        &String::from_str(&env, "Second refund"),
        &requester,
    );

    // Create third refund
    let refund_id_3 = client.create_refund(
        &payment_id,
        &250i128,
        &String::from_str(&env, "Third refund"),
        &requester,
    );

    // Verify all refund IDs are unique
    assert_ne!(refund_id_1, refund_id_2);
    assert_ne!(refund_id_2, refund_id_3);
    assert_ne!(refund_id_1, refund_id_3);

    // Verify all refunds can be retrieved independently
    let refund_1 = client.get_refund(&refund_id_1);
    let refund_2 = client.get_refund(&refund_id_2);
    let refund_3 = client.get_refund(&refund_id_3);

    assert_eq!(refund_1.amount, 1000i128);
    assert_eq!(refund_2.amount, 500i128);
    assert_eq!(refund_3.amount, 250i128);

    // Verify refund IDs follow expected pattern
    assert_eq!(refund_id_1, String::from_str(&env, "refund_1"));
    assert_eq!(refund_id_2, String::from_str(&env, "refund_2"));
    assert_eq!(refund_id_3, String::from_str(&env, "refund_3"));
}

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn test_create_refund_requires_auth() {
    let env = Env::default();
    let (_, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_123");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);

    client.register_payment(&payment_id, &merchant_id, &5000i128, &Symbol::new(&env, "USDC"));

    // This should panic because we're not mocking auth
    client.create_refund(
        &payment_id,
        &1000i128,
        &String::from_str(&env, "Unauthorized refund"),
        &requester,
    );
}

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn test_create_payment_requires_auth() {
    let env = Env::default();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_123");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    let currency = Symbol::new(&env, "USDC");
    let deposit_address = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600;

    // This should panic because we're not mocking auth
    client.create_payment(
        &payment_id,
        &merchant_id,
        &amount,
        &currency,
        &deposit_address,
        &expires_at,
    );
}

/// Issue #37: verify role membership list integrity.
#[test]
fn test_get_role_members() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    let oracle1 = Address::generate(&env);
    let oracle2 = Address::generate(&env);
    let oracle_role = role_oracle(&env);

    // Initially no oracle members
    let members = client.get_role_members(&oracle_role);
    assert_eq!(members.len(), 0);

    // Grant oracle to oracle1
    client.grant_role(&admin, &oracle_role, &oracle1);
    let members = client.get_role_members(&oracle_role);
    assert_eq!(members.len(), 1);
    assert_eq!(members.get(0), Some(oracle1.clone()));

    // Grant oracle to oracle2
    client.grant_role(&admin, &oracle_role, &oracle2);
    let members = client.get_role_members(&oracle_role);
    assert_eq!(members.len(), 2);

    // Revoke oracle1 — list should shrink
    client.revoke_role(&admin, &oracle_role, &oracle1);
    let members = client.get_role_members(&oracle_role);
    assert_eq!(members.len(), 1);
    assert_eq!(members.get(0), Some(oracle2.clone()));
}

/// Issue #37: admin is automatically in the ADMIN role members list after initialize.
#[test]
fn test_admin_in_role_members_after_init() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    let admin_role = role_admin(&env);
    let members = client.get_role_members(&admin_role);
    assert_eq!(members.len(), 1);
    assert_eq!(members.get(0), Some(admin));
}
