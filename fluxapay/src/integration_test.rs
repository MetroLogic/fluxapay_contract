#![cfg(test)]

use crate::{
    access_control::role_merchant,
    merchant_registry::{KycTier, MerchantRegistry, MerchantRegistryClient},
    DisputeStatus, PaymentProcessor, PaymentProcessorClient, PaymentStatus, RefundManager,
    RefundManagerClient, RefundStatus,
};
use soroban_sdk::{
    testutils::{Address as _, BytesN as _, Ledger as _},
    token, Address, BytesN, Env, String, Symbol,
};

fn setup_integration(
    env: &Env,
) -> (
    Address,
    PaymentProcessorClient<'_>,
    RefundManagerClient<'_>,
    MerchantRegistryClient<'_>,
) {
    let payment_processor = env.register(PaymentProcessor, ());
    let refund_manager = env.register(RefundManager, ());
    let merchant_registry = env.register(MerchantRegistry, ());

    let refund_client = RefundManagerClient::new(env, &refund_manager);
    let payment_client = PaymentProcessorClient::new(env, &payment_processor);
    let merchant_client = MerchantRegistryClient::new(env, &merchant_registry);

    let admin = Address::generate(env);
    let token_admin = Address::generate(env);
    let usdc_token = env.register_stellar_asset_contract_v2(token_admin).address();
    refund_client.initialize_refund_manager(&admin, &usdc_token);
    let token_admin_client = token::StellarAssetClient::new(env, &usdc_token);
    token_admin_client.mint(&refund_manager, &1_000_000_000_000i128);

    payment_client.initialize_payment_processor(&admin);
    merchant_client.merchant_initialize(&admin);

    (admin, payment_client, refund_client, merchant_client)
}

#[test]
fn test_happy_path_flow() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, refund_client, merchant_client) = setup_integration(&env);
    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);

    // 1. Register and Verify Merchant
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Flux Merchant"),
        &String::from_str(&env, "USD"),
        &None::<Address>,
        &None::<String>,
    );
    merchant_client.verify_merchant(&admin, &merchant);
    let merchant_info = merchant_client.get_merchant(&merchant);
    assert_eq!(merchant_info.kyc_tier, KycTier::Basic);

    // 2. Create and Verify Payment
    let payment_id = String::from_str(&env, "PAY_01");
    let amount = 1000i128;
    let expires_at = env.ledger().timestamp() + 3600;

    payment_client.payment_grant_role(&admin, &role_merchant(&env), &merchant);

    payment_client.create_payment(
        &payment_id,
        &merchant,
        &amount,
        &Symbol::new(&env, "USDC"),
        &Address::generate(&env),
        &expires_at,
    );

    let tx_hash = BytesN::<32>::random(&env);
    let oracle = Address::generate(&env);
    payment_client.payment_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(&oracle, &payment_id, &tx_hash, &customer, &amount);

    let payment_info = payment_client.get_payment(&payment_id);
    assert_eq!(payment_info.status, PaymentStatus::Confirmed);

    // Register payment with refund manager for amount validation
    refund_client.register_payment(&payment_id, &merchant, &amount, &Symbol::new(&env, "USDC"));

    // 3. Create Dispute and Resolve with Refund
    let dispute_id = refund_client.create_dispute(
        &payment_id,
        &amount,
        &String::from_str(&env, "Product Damaged"),
        &String::from_str(&env, "Video evidence"),
        &customer,
    );

    let operator = Address::generate(&env);
    refund_client.refund_grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    let refund_id = refund_client.resolve_dispute_with_refund(
        &operator,
        &dispute_id,
        &String::from_str(&env, "Refund approved"),
    );

    let dispute_info = refund_client.get_dispute(&dispute_id);
    assert_eq!(dispute_info.status, DisputeStatus::Resolved);
    assert!(dispute_info.refund_id.is_some());

    let refund_info = refund_client.get_refund(&refund_id);
    assert_eq!(refund_info.status, RefundStatus::Completed);
}

#[test]
fn test_settlement_path() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, _merchant_client) = setup_integration(&env);
    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);
    let treasury = Address::generate(&env);
    let operator = Address::generate(&env);

    payment_client.payment_grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    let payment_id = String::from_str(&env, "PAY_SETTLE");
    let amount = 2000i128;

    payment_client.payment_grant_role(&admin, &role_merchant(&env), &merchant);

    payment_client.create_payment(
        &payment_id,
        &merchant,
        &amount,
        &Symbol::new(&env, "USDC"),
        &Address::generate(&env),
        &(env.ledger().timestamp() + 3600),
    );

    let oracle = Address::generate(&env);
    payment_client.payment_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::<32>::random(&env),
        &customer,
        &amount,
    );

    // Settle payment (Sweep to treasury)
    payment_client.settle_payment(&operator, &payment_id, &treasury);

    let payment_info = payment_client.get_payment(&payment_id);
    assert_eq!(payment_info.status, PaymentStatus::Settled);
    assert_eq!(payment_info.deposit_address, treasury);
}

#[test]
fn test_failure_and_expiration_path() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, refund_client, _merchant_client) = setup_integration(&env);
    let merchant = Address::generate(&env);

    let payment_id = String::from_str(&env, "PAY_EXPIRE");
    let amount = 500i128;
    let expires_at = env.ledger().timestamp() + 100;

    payment_client.payment_grant_role(&admin, &role_merchant(&env), &merchant);

    payment_client.create_payment(
        &payment_id,
        &merchant,
        &amount,
        &Symbol::new(&env, "USDC"),
        &Address::generate(&env),
        &expires_at,
    );

    // Jump forward in time
    env.ledger().set_timestamp(expires_at + 1);

    // Expire payment via cleanup path
    payment_client.expire_payment(&payment_id);

    let payment_info = payment_client.get_payment(&payment_id);
    assert_eq!(payment_info.status, PaymentStatus::Expired);

    // Try to dispute an expired/cancelled payment - should still be possible to create, but maybe rejected?
    let customer = Address::generate(&env);
    let dispute_id = refund_client.create_dispute(
        &payment_id,
        &amount,
        &String::from_str(&env, "Late but flawed"),
        &String::from_str(&env, "N/A"),
        &customer,
    );

    let operator = Address::generate(&env);
    refund_client.refund_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &operator);

    // Reject dispute
    refund_client.reject_dispute(
        &operator,
        &dispute_id,
        &String::from_str(&env, "Payment already expired and cancelled"),
    );

    let dispute_info = refund_client.get_dispute(&dispute_id);
    assert_eq!(dispute_info.status, DisputeStatus::Rejected);
}
