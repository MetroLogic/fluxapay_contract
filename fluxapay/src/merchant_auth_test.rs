#![cfg(test)]

use crate::{
    CreatePaymentArgs, Error, PaymentCharge, PaymentProcessor, PaymentProcessorClient,
    PaymentStatus, RefundManager, RefundManagerClient,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, String, Symbol,
};

fn create_payment_args(env: &Env, merchant_id: &Address, payment_id: &str) -> CreatePaymentArgs {
    CreatePaymentArgs {
        payment_id: String::from_str(env, payment_id),
        merchant_id: merchant_id.clone(),
        payer: None,
        amount: 10_000,
        currency: Symbol::new(env, "USDC"),
        deposit_address: Address::generate(env),
        expires_at: Some(env.ledger().timestamp() + 3_600),
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
    }
}

#[test]
fn merchant_role_grants_and_revokes_payment_creation() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize_payment_processor(&admin);

    let merchant = Address::generate(&env);
    client.grant_merchant_role(&admin, &merchant);

    let mut args = create_payment_args(&env, &merchant, "auth_payment");
    args.client_token = Some(String::from_str(&env, "merchant_create_key"));
    let payment = client.create_payment(&args);
    assert_eq!(payment.merchant_id, merchant);

    let other_merchant = Address::generate(&env);
    let other_result = client.try_create_payment(&create_payment_args(
        &env,
        &other_merchant,
        "other_merchant_payment",
    ));
    assert_eq!(other_result, Err(Ok(Error::Unauthorized)));

    client.revoke_merchant_role(&admin, &merchant);
    let replay_result = client.try_create_payment(&args);
    assert_eq!(replay_result, Err(Ok(Error::Unauthorized)));
    let revoked_result =
        client.try_create_payment(&create_payment_args(&env, &merchant, "revoked_payment"));
    assert_eq!(revoked_result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn merchant_role_is_required_for_merchant_requested_refunds() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|ledger| ledger.timestamp = 10_000);

    let contract_id = env.register(RefundManager, ());
    let client = RefundManagerClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    client.initialize_refund_manager(&admin, &token_address);

    let merchant = Address::generate(&env);
    let payment_id = String::from_str(&env, "merchant_refund_payment");
    let payment = PaymentCharge {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        amount: 1_000,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        status: PaymentStatus::Confirmed,
        payer_address: None,
        transaction_hash: None,
        created_at: 1,
        confirmed_at: Some(1),
        expires_at: 0,
        amount_received: None,
        memo: None,
        memo_type: None,
        token_address: None,
        metadata_hash: None,
        fx_rate: None,
        fx_rate_at: None,
        original_token: None,
        swap_path: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
        payment_link_id: None,
    };
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&crate::DataKey::Payment(payment_id.clone()), &payment);
    });

    client.grant_merchant_role(&admin, &merchant);
    let reason = String::from_str(&env, "merchant refund");
    let idempotency_key = Some(String::from_str(&env, "merchant_refund_key"));
    client.create_refund_idempotent(&payment_id, &400, &reason, &merchant, &idempotency_key);

    client.revoke_merchant_role(&admin, &merchant);
    let result = client.try_create_refund_idempotent(
        &payment_id,
        &400,
        &reason,
        &merchant,
        &idempotency_key,
    );
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}
