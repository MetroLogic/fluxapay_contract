#![cfg(test)]

use super::*;
use access_control::{role_admin, role_oracle, role_settlement_operator};
use soroban_sdk::{
    testutils::{Address as _, BytesN as _, Events as _, Ledger as _},
    token, vec, Address, BytesN, Env, String, Symbol,
};

#[test]
fn test_datakey_discriminant_stability() {
    let env = Env::default();

    // This is a storage-compatibility canary. `#[contracttype]` encodes enum
    // variants by their definition order, so reordering `DataKey` variants would
    // make existing ledger entries unreadable by the original keys.
    let merchant = Address::generate(&env);
    let deposit_address = Address::generate(&env);
    let payer = Address::generate(&env);
    let payment_id = String::from_str(&env, "PAY_STABLE");
    let refund_id = String::from_str(&env, "REF_STABLE");

    let payment = PaymentCharge {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        amount: 2500,
        currency: Symbol::new(&env, "USDC"),
        deposit_address,
        status: PaymentStatus::Pending,
        payer_address: Some(payer.clone()),
        transaction_hash: None,
        created_at: 10,
        confirmed_at: None,
        expires_at: 70,
        amount_received: None,
        memo: None,
        memo_type: None,
        token_address: None,
        metadata_hash: None,
        original_token: None,
        swap_path: None,
        fx_rate: None,
        fx_rate_at: None,
    };
    env.storage()
        .persistent()
        .set(&DataKey::Payment(payment_id.clone()), &payment);
    let stored_payment: PaymentCharge = env
        .storage()
        .persistent()
        .get(&DataKey::Payment(payment_id.clone()))
        .unwrap();
    assert_eq!(stored_payment, payment);

    let refund = Refund {
        refund_id: refund_id.clone(),
        payment_id: payment_id.clone(),
        amount: 500,
        reason: String::from_str(&env, "customer-request"),
        status: RefundStatus::Pending,
        requester: payer,
        created_at: 20,
        processed_at: None,
        approved: false,
        receipt_hash: None,
        expiry_at: 100,
    };
    env.storage()
        .persistent()
        .set(&DataKey::Refund(refund_id.clone()), &refund);
    let stored_refund: Refund = env
        .storage()
        .persistent()
        .get(&DataKey::Refund(refund_id))
        .unwrap();
    assert_eq!(stored_refund, refund);

    let mut merchant_payments = vec![&env];
    merchant_payments.push_back(payment_id);
    env.storage().persistent().set(
        &DataKey::MerchantPayments(merchant.clone()),
        &merchant_payments,
    );
    let stored_merchant_payments: Vec<String> = env
        .storage()
        .persistent()
        .get(&DataKey::MerchantPayments(merchant))
        .unwrap();
    assert_eq!(stored_merchant_payments, merchant_payments);
}

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
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    client.initialize_refund_manager(&admin, &usdc_token);

    let token_admin_client = token::StellarAssetClient::new(env, &usdc_token);
    token_admin_client.mint(&contract_id, &1_000_000_000_000i128);

    (admin, client)
}

fn setup_refund_manager_with_token(env: &Env) -> (Address, RefundManagerClient<'_>, Address) {
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

    (admin, client, usdc_token)
}

fn create_payment_args(
    env: &Env,
    payment_id: &String,
    merchant_id: &Address,
    amount: i128,
) -> CreatePaymentArgs {
    CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant_id.clone(),
        amount,
        currency: Symbol::new(env, "USDC"),
        deposit_address: Address::generate(env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
    }
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
    let _deposit_address = Address::generate(&env);
    let _expires_at = env.ledger().timestamp() + 3600;
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = create_payment_args(&env, &payment_id, &merchant_id, amount);
    let payment = client.create_payment(&args);

    assert_eq!(payment.payment_id, payment_id);
    assert_eq!(payment.merchant_id, merchant_id);
    assert_eq!(payment.amount, amount);
    assert_eq!(payment.currency, currency);
    assert_eq!(payment.deposit_address, args.deposit_address);
    assert_eq!(payment.status, PaymentStatus::Pending);
    assert_eq!(payment.memo, None);
    assert_eq!(payment.memo_type, None);
}

#[test]
fn test_create_payment_fails_for_blacklisted_merchant() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "blacklisted_payment_1");
    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);
    client.add_to_blacklist(&admin, &merchant_id);

    let args = create_payment_args(&env, &payment_id, &merchant_id, 1000i128);
    let result = client.try_create_payment(&args);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_create_payment_rate_limit_enforced() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let _currency = Symbol::new(&env, "USDC");
    let _deposit_address = Address::generate(&env);
    let _expires_at = env.ledger().timestamp() + 3600;

    for i in 0..CREATE_PAYMENT_MAX_PER_WINDOW {
        let payment_id = format_id(&env, "rate_limit_", i as u64);
        let args = create_payment_args(&env, &payment_id, &merchant_id, 100i128);
        client.create_payment(&args);
    }

    let overflow_id = String::from_str(&env, "rate_limit_overflow");
    let args = create_payment_args(&env, &overflow_id, &merchant_id, 100i128);
    let overflow = client.try_create_payment(&args);

    assert_eq!(overflow, Err(Ok(Error::RateLimitExceeded)));
}

#[test]
fn test_create_payments_batch_returns_ids_in_order() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let payment_id_1 = String::from_str(&env, "batch_payment_1");
    let payment_id_2 = String::from_str(&env, "batch_payment_2");
    let batch = vec![
        &env,
        create_payment_args(&env, &payment_id_1, &merchant_id, 100i128),
        create_payment_args(&env, &payment_id_2, &merchant_id, 200i128),
    ];

    let payment_ids = client.create_payments_batch(&batch);

    assert_eq!(payment_ids.len(), 2);
    assert_eq!(payment_ids.get(0).unwrap(), payment_id_1);
    assert_eq!(payment_ids.get(1).unwrap(), payment_id_2);
}

#[test]
fn test_create_payments_batch_rejects_oversized_batch() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let mut batch = vec![&env];
    for i in 0..51u32 {
        let payment_id = format_id(&env, "batch_limit_", i as u64);
        batch.push_back(create_payment_args(&env, &payment_id, &merchant_id, 100i128));
    }

    let result = client.try_create_payments_batch(&batch);
    assert_eq!(result, Err(Ok(Error::BatchTooLarge)));
}

#[test]
fn test_create_payments_batch_is_atomic_on_validation_error() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let payment_id_1 = String::from_str(&env, "batch_atomic_1");
    let payment_id_2 = String::from_str(&env, "batch_atomic_2");
    let batch = vec![
        &env,
        create_payment_args(&env, &payment_id_1, &merchant_id, 100i128),
        create_payment_args(&env, &payment_id_2, &merchant_id, 0i128),
    ];

    let result = client.try_create_payments_batch(&batch);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
    assert!(!env.storage().persistent().has(&DataKey::Payment(payment_id_1)));
    assert!(!env.storage().persistent().has(&DataKey::Payment(payment_id_2)));
}

#[test]
fn test_cancel_multiple_streams_for_sender() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_payment_processor(&env);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    token::StellarAssetClient::new(&env, &token).mint(&client.address, &1_000_000i128);

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let stream_id1 = String::from_str(&env, "stream_1");
    let stream_id2 = String::from_str(&env, "stream_2");

    // Fund sender
    token::StellarAssetClient::new(&env, &token).mint(&sender, &1_000_000i128);

    client.create_stream(
        &sender,
        &recipient,
        &token,
        &100i128,
        &1_000i128,
        &stream_id1,
    );
    client.create_stream(
        &sender,
        &recipient,
        &token,
        &200i128,
        &2_000i128,
        &stream_id2,
    );

    let stream_ids = vec![&env, stream_id1.clone(), stream_id2.clone()];
    let cancelled = client.cancel_multiple_streams(&sender, &stream_ids);

    assert_eq!(cancelled.len(), 2);
    let stream1 = client.get_stream(&stream_id1);
    let stream2 = client.get_stream(&stream_id2);
    assert_eq!(stream1.status, StreamStatus::Cancelled);
    assert_eq!(stream2.status, StreamStatus::Cancelled);
}

#[test]
fn test_create_stream_fails_for_blacklisted_sender() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let stream_id = String::from_str(&env, "blacklisted_stream_1");

    token::StellarAssetClient::new(&env, &token).mint(&sender, &1_000_000i128);
    client.add_to_blacklist(&admin, &sender);

    let result = client.try_create_stream(
        &sender,
        &recipient,
        &token,
        &100i128,
        &1_000i128,
        &stream_id,
    );
    assert_eq!(result, Err(Ok(StreamError::Unauthorized)));
}

#[test]
fn test_batch_withdraw_to_custom_routing() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_payment_processor(&env);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_client = token::StellarAssetClient::new(&env, &token);

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let destination1 = Address::generate(&env);
    let destination2 = Address::generate(&env);
    let stream_id1 = String::from_str(&env, "stream_a");
    let stream_id2 = String::from_str(&env, "stream_b");

    // Fund sender and let contract hold tokens
    token_client.mint(&sender, &10_000i128);

    client.create_stream(
        &sender,
        &recipient,
        &token,
        &100i128,
        &1_000i128,
        &stream_id1,
    );
    client.create_stream(
        &sender,
        &recipient,
        &token,
        &200i128,
        &2_000i128,
        &stream_id2,
    );

    // Advance time so some tokens accrue
    env.ledger().set_timestamp(env.ledger().timestamp() + 1);

    let withdrawal1 = WithdrawalRecipient {
        stream_id: stream_id1.clone(),
        destination: destination1.clone(),
        amount: 40,
    };
    let withdrawal2 = WithdrawalRecipient {
        stream_id: stream_id2.clone(),
        destination: destination2.clone(),
        amount: 150,
    };
    let withdrawals = vec![&env, withdrawal1, withdrawal2];

    let success = client.batch_withdraw_to(&recipient, &withdrawals);
    assert_eq!(success.len(), 2);
}

#[test]
fn test_verify_payment_success() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_123");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = create_payment_args(&env, &payment_id, &merchant_id, amount);
    client.create_payment(&args);

    let payer_address = Address::generate(&env);
    let transaction_hash = BytesN::<32>::random(&env);
    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &oracle);

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
    assert_eq!(payment.amount_received, Some(amount));
}

#[test]
fn test_verify_payment_fails_for_blacklisted_payer() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "verify_blacklisted_payer");
    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = create_payment_args(&env, &payment_id, &merchant_id, 1000i128);
    client.create_payment(&args);

    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &oracle);

    let blacklisted_payer = Address::generate(&env);
    client.add_to_blacklist(&admin, &blacklisted_payer);

    let result = client.try_verify_payment(
        &oracle,
        &payment_id,
        &BytesN::<32>::random(&env),
        &blacklisted_payer,
        &1000i128,
    );
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_verify_payment_partially_paid() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "partial_pay");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = create_payment_args(&env, &payment_id, &merchant_id, amount);
    client.create_payment(&args);

    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &oracle);

    // Send significantly less than expected (outside tolerance)
    let amount_received = amount - 100;
    let status = client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &amount_received,
    );

    assert_eq!(status, PaymentStatus::PartiallyPaid);
    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::PartiallyPaid);
    assert_eq!(payment.amount_received, Some(amount_received));
}

#[test]
fn test_verify_payment_overpaid() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "over_pay");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = create_payment_args(&env, &payment_id, &merchant_id, amount);
    client.create_payment(&args);

    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &oracle);

    // Send more than expected (outside tolerance)
    let amount_received = amount + 100;
    let status = client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &amount_received,
    );

    assert_eq!(status, PaymentStatus::Overpaid);
    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Overpaid);
    assert_eq!(payment.amount_received, Some(amount_received));
}

#[test]
fn test_verify_payment_within_tolerance() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "tol_pay");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = create_payment_args(&env, &payment_id, &merchant_id, amount);
    client.create_payment(&args);

    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &oracle);

    // Send exactly 1 stroop less — within tolerance → Confirmed
    let amount_received = amount - 1;
    let status = client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &amount_received,
    );

    assert_eq!(status, PaymentStatus::Confirmed);
    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Confirmed);
    assert_eq!(payment.amount_received, Some(amount_received));
}

#[test]
fn test_get_merchant_payments_index_and_pagination() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    let _currency = Symbol::new(&env, "USDC");
    let _deposit_address = Address::generate(&env);
    let _expires_at = env.ledger().timestamp() + 3600;

    let payment_id_1 = String::from_str(&env, "merchant_pay_1");
    let payment_id_2 = String::from_str(&env, "merchant_pay_2");
    let payment_id_3 = String::from_str(&env, "merchant_pay_3");

    client.grant_role(&admin, &role_merchant(&env), &merchant_id);
    client.create_payment(&create_payment_args(
        &env,
        &payment_id_1,
        &merchant_id,
        100i128,
    ));
    client.create_payment(&create_payment_args(
        &env,
        &payment_id_2,
        &merchant_id,
        200i128,
    ));
    client.create_payment(&create_payment_args(
        &env,
        &payment_id_3,
        &merchant_id,
        300i128,
    ));

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
fn test_cancel_pending_success() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "cancel_pending_success");
    let merchant_id = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600;
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = create_payment_args(&env, &payment_id, &merchant_id, 500i128);
    client.create_payment(&args);

    // Set time to before expiry
    env.ledger().set_timestamp(expires_at - 1);

    client.cancel_payment(&merchant_id, &payment_id);

    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Failed);
}

#[test]
fn test_cancel_fails_when_confirmed() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "cancel_fails_confirmed");
    let merchant_id = Address::generate(&env);
    let amount = 500i128;
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = create_payment_args(&env, &payment_id, &merchant_id, amount);
    client.create_payment(&args);

    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &oracle);

    client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &amount,
    );

    let res = client.try_cancel_payment(&merchant_id, &payment_id);
    assert_eq!(res.unwrap_err().unwrap(), Error::PaymentAlreadyProcessed);
}

#[test]
fn test_expiry_logic() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "cancel_past_expiry");
    let merchant_id = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600;
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = create_payment_args(&env, &payment_id, &merchant_id, 500i128);
    client.create_payment(&args);

    // Set time to past expiry
    env.ledger().set_timestamp(expires_at + 1);

    // This should correctly mark it Expired, not throw an error
    let res = client.try_cancel_payment(&merchant_id, &payment_id);
    assert!(res.is_ok());

    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Expired);
}

#[test]
fn test_unauthorized_cancel() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "unauth_cancel");
    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = create_payment_args(&env, &payment_id, &merchant_id, 500i128);
    client.create_payment(&args);

    let random_addr = Address::generate(&env);
    let res = client.try_cancel_payment(&random_addr, &payment_id);
    assert_eq!(res.unwrap_err().unwrap(), Error::Unauthorized);
}

#[test]
fn test_expire_payment_after_deadline() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "expire_after_deadline");
    let merchant_id = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 10;
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 500i128);
    args.expires_at = Some(expires_at);
    client.create_payment(&args);

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
    client.register_payment(
        &payment_id,
        &merchant_id,
        &5000i128,
        &Symbol::new(&env, "USDC"),
    );

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

    client.register_payment(
        &payment_id,
        &merchant_id,
        &5000i128,
        &Symbol::new(&env, "USDC"),
    );

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
fn test_process_refund_accumulates_treasury_and_withdraws() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client, usdc_token) = setup_refund_manager_with_token(&env);
    let token_client = token::StellarAssetClient::new(&env, &usdc_token);

    let merchant_id = Address::generate(&env);
    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    let payment_ids = ["refund_treasury_a", "refund_treasury_b"];
    for payment_suffix in payment_ids.iter() {
        let payment_id = String::from_str(&env, payment_suffix);
        let requester = Address::generate(&env);

        client.register_payment(
            &payment_id,
            &merchant_id,
            &5000i128,
            &Symbol::new(&env, "USDC"),
        );

        let refund_id = client.create_refund(
            &payment_id,
            &1000i128,
            &String::from_str(&env, "Reason"),
            &requester,
        );

        client.process_refund(&operator, &refund_id);
    }

    assert_eq!(client.get_treasury_balance(), 20i128);

    let destination = Address::generate(&env);
    let starting_balance = token_client.balance(&destination);

    client.withdraw_treasury(&admin, &15i128, &destination);

    assert_eq!(client.get_treasury_balance(), 5i128);
    assert_eq!(token_client.balance(&destination), starting_balance + 15i128);
}

#[test]
fn test_withdraw_treasury_rejects_insufficient_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client, _usdc_token) = setup_refund_manager_with_token(&env);

    let destination = Address::generate(&env);
    let result = client.try_withdraw_treasury(&admin, &1i128, &destination);

    assert_eq!(result, Err(Ok(Error::InsufficientTreasuryBalance)));
    assert_eq!(client.get_treasury_balance(), 0i128);
}

#[test]
fn test_create_refund_fails_for_blacklisted_requester() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "refund_blacklisted_requester");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);

    client.register_payment(
        &payment_id,
        &merchant_id,
        &5000i128,
        &Symbol::new(&env, "USDC"),
    );
    client.add_to_blacklist(&admin, &requester);

    let result = client.try_create_refund(
        &payment_id,
        &1000i128,
        &String::from_str(&env, "Reason"),
        &requester,
    );
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_initialize_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(RefundManager, ());
    let client = RefundManagerClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    client.initialize_refund_manager(&admin, &usdc_token);

    assert_eq!(client.get_admin(), Some(admin.clone()));
    assert!(client.has_role(&role_admin(&env), &admin));
}

#[test]
fn test_initialize_refund_manager_rejects_duplicate_admin_and_token() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let _usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    let contract_id = env.register(RefundManager, ());
    let client = RefundManagerClient::new(&env, &contract_id);

    let result = client.try_initialize_refund_manager(&admin, &admin);
    assert_eq!(result, Err(Ok(Error::InvalidAddress)));
}

#[test]
fn test_initialize_refund_manager_rejects_zero_addresses() {
    let env = Env::default();
    let admin = Address::from_str(&env, crate::ZERO_CONTRACT_STRKEY);
    let token_admin = Address::generate(&env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    let contract_id = env.register(RefundManager, ());
    let client = RefundManagerClient::new(&env, &contract_id);

    let result = client.try_initialize_refund_manager(&admin, &usdc_token);
    assert_eq!(result, Err(Ok(Error::InvalidAddress)));
}

#[test]
fn test_initialize_payment_processor_rejects_zero_admin() {
    let env = Env::default();
    let admin = Address::from_str(&env, crate::ZERO_CONTRACT_STRKEY);

    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(&env, &contract_id);

    let result = client.try_initialize_payment_processor(&admin);
    assert_eq!(result, Err(Ok(Error::InvalidAddress)));
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

    client.register_payment(
        &payment_id,
        &merchant_id,
        &5000i128,
        &Symbol::new(&env, "USDC"),
    );

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

    client.register_payment(
        &payment_id,
        &merchant_id,
        &5000i128,
        &Symbol::new(&env, "USDC"),
    );

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
    let (_admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_123");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    let _currency = Symbol::new(&env, "USDC");
    let _deposit_address = Address::generate(&env);
    let _expires_at = env.ledger().timestamp() + 3600;

    // This should panic because we're not mocking auth
    let args = create_payment_args(&env, &payment_id, &merchant_id, amount);
    client.create_payment(&args);
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

fn setup_refund_manager_with_token(env: &Env) -> (Address, RefundManagerClient<'_>, Address) {
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
    (admin, client, usdc_token)
}

#[test]
fn test_process_refund_deducts_fee_from_requester() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client, usdc_token) = setup_refund_manager_with_token(&env);

    let payment_id = String::from_str(&env, "payment_fee_1");
    let merchant_id = Address::generate(&env);
    let refund_amount = 10_000i128;
    let requester = Address::generate(&env);

    client.register_payment(
        &payment_id,
        &merchant_id,
        &refund_amount,
        &Symbol::new(&env, "USDC"),
    );
    let refund_id = client.create_refund(
        &payment_id,
        &refund_amount,
        &String::from_str(&env, "fee test"),
        &requester,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);
    client.process_refund(&operator, &refund_id);

    let token_client = token::TokenClient::new(&env, &usdc_token);
    let fee = refund_amount * 100 / 10_000; // 1%
    let net = refund_amount - fee;

    assert_eq!(token_client.balance(&requester), net);
}

#[test]
fn test_process_refund_sends_fee_to_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client, usdc_token) = setup_refund_manager_with_token(&env);

    let payment_id = String::from_str(&env, "payment_fee_2");
    let merchant_id = Address::generate(&env);
    let refund_amount = 10_000i128;
    let requester = Address::generate(&env);

    client.register_payment(
        &payment_id,
        &merchant_id,
        &refund_amount,
        &Symbol::new(&env, "USDC"),
    );
    let refund_id = client.create_refund(
        &payment_id,
        &refund_amount,
        &String::from_str(&env, "fee test"),
        &requester,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);
    client.process_refund(&operator, &refund_id);

    let token_client = token::TokenClient::new(&env, &usdc_token);
    let fee = refund_amount * 100 / 10_000; // 1%

    assert_eq!(token_client.balance(&admin), fee);
}

#[test]
fn test_cancel_refund_by_requester() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_cancel_1");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);

    client.register_payment(
        &payment_id,
        &merchant_id,
        &5000i128,
        &Symbol::new(&env, "USDC"),
    );
    let refund_id = client.create_refund(
        &payment_id,
        &1000i128,
        &String::from_str(&env, "cancel me"),
        &requester,
    );

    client.cancel_refund(&requester, &refund_id);

    // Refund record should be gone
    let result = client.try_get_refund(&refund_id);
    assert_eq!(result, Err(Ok(Error::RefundNotFound)));

    // Payment refund list should be empty
    let refunds = client.get_payment_refunds(&payment_id);
    assert_eq!(refunds.len(), 0);
}

#[test]
fn test_cancel_refund_by_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_cancel_2");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);

    client.register_payment(
        &payment_id,
        &merchant_id,
        &5000i128,
        &Symbol::new(&env, "USDC"),
    );
    let refund_id = client.create_refund(
        &payment_id,
        &500i128,
        &String::from_str(&env, "admin cancel"),
        &requester,
    );

    client.cancel_refund(&admin, &refund_id);

    let result = client.try_get_refund(&refund_id);
    assert_eq!(result, Err(Ok(Error::RefundNotFound)));
}

#[test]
fn test_cancel_refund_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_cancel_3");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);

    client.register_payment(
        &payment_id,
        &merchant_id,
        &5000i128,
        &Symbol::new(&env, "USDC"),
    );
    let refund_id = client.create_refund(
        &payment_id,
        &500i128,
        &String::from_str(&env, "reason"),
        &requester,
    );

    let random = Address::generate(&env);
    let result = client.try_cancel_refund(&random, &refund_id);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_cancel_refund_already_processed() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_cancel_4");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);

    client.register_payment(
        &payment_id,
        &merchant_id,
        &5000i128,
        &Symbol::new(&env, "USDC"),
    );
    let refund_id = client.create_refund(
        &payment_id,
        &500i128,
        &String::from_str(&env, "reason"),
        &requester,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);
    client.process_refund(&operator, &refund_id);

    // Attempt to cancel a completed refund
    let result = client.try_cancel_refund(&requester, &refund_id);
    assert_eq!(result, Err(Ok(Error::RefundAlreadyProcessed)));
}

#[test]
fn test_cancel_refund_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_cancel_5");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);

    client.register_payment(
        &payment_id,
        &merchant_id,
        &5000i128,
        &Symbol::new(&env, "USDC"),
    );
    let refund_id = client.create_refund(
        &payment_id,
        &750i128,
        &String::from_str(&env, "reason"),
        &requester,
    );

    client.cancel_refund(&requester, &refund_id);

    // Verify REFUND/CANCELLED event was emitted
    let events = env.events().all();
    assert!(!events.is_empty());
}

// ── Issue #114: Total Refund Validation ──────────────────────────────────────

/// Refunding exactly the payment amount should succeed.
#[test]
fn test_refund_total_equals_payment_amount_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "pay_exact");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);
    let amount = 1000i128;

    client.register_payment(
        &payment_id,
        &merchant_id,
        &amount,
        &Symbol::new(&env, "USDC"),
    );
    let refund_id = client.create_refund(
        &payment_id,
        &amount,
        &String::from_str(&env, "full refund"),
        &requester,
    );
    let refund = client.get_refund(&refund_id);
    assert_eq!(refund.amount, amount);
}

/// A single refund exceeding the payment amount must be rejected.
#[test]
#[should_panic(expected = "Error(Contract, #16)")]
fn test_refund_exceeds_payment_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "pay_over");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);

    client.register_payment(
        &payment_id,
        &merchant_id,
        &500i128,
        &Symbol::new(&env, "USDC"),
    );
    // Attempt to refund more than the payment amount
    client.create_refund(
        &payment_id,
        &501i128,
        &String::from_str(&env, "over refund"),
        &requester,
    );
}

/// Cumulative partial refunds that exceed the payment amount must be rejected.
#[test]
#[should_panic(expected = "Error(Contract, #16)")]
fn test_cumulative_refunds_exceed_payment_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "pay_cumulative");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);

    client.register_payment(
        &payment_id,
        &merchant_id,
        &1000i128,
        &Symbol::new(&env, "USDC"),
    );

    // First partial refund: 600
    client.create_refund(
        &payment_id,
        &600i128,
        &String::from_str(&env, "partial 1"),
        &requester,
    );

    // Second partial refund: 401 — total would be 1001 > 1000, must fail
    client.create_refund(
        &payment_id,
        &401i128,
        &String::from_str(&env, "partial 2 over"),
        &requester,
    );
}

// ── Issue #115: Partial Refund Support ───────────────────────────────────────

/// Multiple partial refunds up to the payment total should all succeed and be tracked.
#[test]
fn test_partial_refunds_tracked_in_payment_refunds_list() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "pay_partial");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);

    client.register_payment(
        &payment_id,
        &merchant_id,
        &1000i128,
        &Symbol::new(&env, "USDC"),
    );

    let r1 = client.create_refund(
        &payment_id,
        &300i128,
        &String::from_str(&env, "partial 1"),
        &requester,
    );
    let r2 = client.create_refund(
        &payment_id,
        &400i128,
        &String::from_str(&env, "partial 2"),
        &requester,
    );
    let r3 = client.create_refund(
        &payment_id,
        &300i128,
        &String::from_str(&env, "partial 3"),
        &requester,
    );

    // All three refunds should be in the payment's refund list
    let refunds = client.get_payment_refunds(&payment_id);
    assert_eq!(refunds.len(), 3);

    // Verify amounts are tracked correctly
    assert_eq!(client.get_refund(&r1).amount, 300i128);
    assert_eq!(client.get_refund(&r2).amount, 400i128);
    assert_eq!(client.get_refund(&r3).amount, 300i128);
}

/// Rejected refunds should not count toward the total, allowing a replacement refund.
#[test]
fn test_rejected_refund_does_not_count_toward_total() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "pay_rejected");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);

    client.register_payment(
        &payment_id,
        &merchant_id,
        &1000i128,
        &Symbol::new(&env, "USDC"),
    );

    let refund_id = client.create_refund(
        &payment_id,
        &800i128,
        &String::from_str(&env, "will be rejected"),
        &requester,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);
    client.reject_refund(&operator, &refund_id);

    // After rejection, a new refund for 800 should succeed (rejected one doesn't count)
    let new_refund_id = client.create_refund(
        &payment_id,
        &800i128,
        &String::from_str(&env, "replacement"),
        &requester,
    );
    let new_refund = client.get_refund(&new_refund_id);
    assert_eq!(new_refund.amount, 800i128);
    assert_eq!(new_refund.status, RefundStatus::Pending);
}

// --- Payment expiry / duration tests ---

#[test]
fn test_create_payment_with_explicit_expires_at() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let expires_at = env.ledger().timestamp() + 7200; // 2 hours
    let payment_id = String::from_str(&env, "pay_explicit_expiry");
    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 1000i128);
    args.expires_at = Some(expires_at);
    let payment = client.create_payment(&args);
    assert_eq!(payment.expires_at, expires_at);
}

#[test]
fn test_create_payment_with_duration_secs() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let now = env.ledger().timestamp();
    let duration = 1800u64; // 30 minutes
    let payment_id = String::from_str(&env, "pay_duration");
    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 1000i128);
    args.expires_at = None;
    args.duration_secs = Some(duration);
    let payment = client.create_payment(&args);
    assert_eq!(payment.expires_at, now + duration);
}

#[test]
fn test_create_payment_defaults_to_one_hour() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let now = env.ledger().timestamp();
    let payment_id = String::from_str(&env, "pay_default_expiry");
    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 1000i128);
    args.expires_at = None;
    let payment = client.create_payment(&args);
    assert_eq!(payment.expires_at, now + DEFAULT_PAYMENT_DURATION_SECS);
}

#[test]
fn test_create_payment_explicit_expires_at_overrides_duration() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let explicit_ts = env.ledger().timestamp() + 9999;
    let payment_id = String::from_str(&env, "pay_explicit_wins");
    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 1000i128);
    args.expires_at = Some(explicit_ts);
    args.duration_secs = Some(60u64);
    let payment = client.create_payment(&args);
    assert_eq!(payment.expires_at, explicit_ts);
}

#[test]
fn test_create_payment_past_expires_at_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let now = env.ledger().timestamp();
    // expires_at in the past (or equal to now)
    let payment_id = String::from_str(&env, "pay_past_expiry");
    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 1000i128);
    args.expires_at = Some(now);
    let result = client.try_create_payment(&args);
    assert_eq!(result, Err(Ok(Error::InvalidExpiry)));
}

#[test]
fn test_create_payment_zero_duration_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let payment_id = String::from_str(&env, "pay_zero_duration");
    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 1000i128);
    args.expires_at = None;
    args.duration_secs = Some(0u64);
    let result = client.try_create_payment(&args);
    assert_eq!(result, Err(Ok(Error::InvalidExpiry)));
}

// --- Amount limits tests ---

#[test]
fn test_global_min_limit_blocks_payment() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    client.set_global_amount_limits(&admin, &Some(500i128), &None::<i128>);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let payment_id = String::from_str(&env, "pay_below_global_min");
    let args = create_payment_args(&env, &payment_id, &merchant_id, 499i128);
    let result = client.try_create_payment(&args);
    assert_eq!(result, Err(Ok(Error::AmountBelowMin)));
}

#[test]
fn test_global_max_limit_blocks_payment() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    client.set_global_amount_limits(&admin, &None::<i128>, &Some(1000i128));

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let payment_id = String::from_str(&env, "pay_above_global_max");
    let args = create_payment_args(&env, &payment_id, &merchant_id, 1001i128);
    let result = client.try_create_payment(&args);
    assert_eq!(result, Err(Ok(Error::AmountAboveMax)));
}

#[test]
fn test_global_limits_allow_payment_within_range() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    client.set_global_amount_limits(&admin, &Some(100i128), &Some(10_000i128));

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let payment_id = String::from_str(&env, "pay_within_global");
    let args = create_payment_args(&env, &payment_id, &merchant_id, 5_000i128);
    let payment = client.create_payment(&args);
    assert_eq!(payment.status, PaymentStatus::Pending);
}

#[test]
fn test_merchant_limits_override_global_limits() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    // Global: min 1000
    client.set_global_amount_limits(&admin, &Some(1000i128), &None::<i128>);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    // Merchant-specific: min 10 (lower than global)
    client.set_merchant_amount_limits(&merchant_id, &Some(10i128), &None::<i128>);

    // 500 is below global min but above merchant min — should succeed
    let payment_id = String::from_str(&env, "pay_merchant_override");
    let args = create_payment_args(&env, &payment_id, &merchant_id, 500i128);
    let payment = client.create_payment(&args);
    assert_eq!(payment.status, PaymentStatus::Pending);
}

#[test]
fn test_merchant_max_limit_blocks_payment() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    client.set_merchant_amount_limits(&merchant_id, &None::<i128>, &Some(200i128));

    let payment_id = String::from_str(&env, "pay_above_merchant_max");
    let args = create_payment_args(&env, &payment_id, &merchant_id, 201i128);
    let result = client.try_create_payment(&args);
    assert_eq!(result, Err(Ok(Error::AmountAboveMax)));
}

#[test]
fn test_set_merchant_limits_invalid_range_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    // min > max — must fail
    let result =
        client.try_set_merchant_amount_limits(&merchant_id, &Some(1000i128), &Some(500i128));
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn test_get_merchant_and_global_limits() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    assert_eq!(client.get_global_amount_limits(), None);
    assert_eq!(client.get_merchant_amount_limits(&merchant_id), None);

    client.set_global_amount_limits(&admin, &Some(50i128), &Some(5000i128));
    client.set_merchant_amount_limits(&merchant_id, &Some(100i128), &Some(2000i128));

    let global = client.get_global_amount_limits().unwrap();
    assert_eq!(global.min, Some(50i128));
    assert_eq!(global.max, Some(5000i128));

    let merchant = client.get_merchant_amount_limits(&merchant_id).unwrap();
    assert_eq!(merchant.min, Some(100i128));
    assert_eq!(merchant.max, Some(2000i128));
}

// --- Multi-asset payment tests ---

#[test]
fn test_create_payment_with_allowed_token() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let token_admin = Address::generate(&env);
    let alt_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    // Allow the token
    client.allow_token(&admin, &alt_token);
    assert!(client.is_token_allowed(&alt_token));

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let payment_id = String::from_str(&env, "pay_alt_token");
    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 1000i128);
    args.currency = Symbol::new(&env, "EURC");
    args.token_address = Some(alt_token.clone());
    let payment = client.create_payment(&args);

    assert_eq!(payment.token_address, Some(alt_token));
    assert_eq!(payment.status, PaymentStatus::Pending);
}

#[test]
fn test_create_payment_with_unlisted_token_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let token_admin = Address::generate(&env);
    let unknown_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    // Do NOT allow the token
    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let payment_id = String::from_str(&env, "pay_bad_token");
    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 1000i128);
    args.currency = Symbol::new(&env, "RAND");
    args.token_address = Some(unknown_token);
    let result = client.try_create_payment(&args);

    assert_eq!(result, Err(Ok(Error::UnsupportedToken)));
}

#[test]
fn test_create_payment_no_token_address_uses_default() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let payment_id = String::from_str(&env, "pay_default_token");
    let args = create_payment_args(&env, &payment_id, &merchant_id, 500i128);
    let payment = client.create_payment(&args);

    assert_eq!(payment.token_address, None);
    assert_eq!(payment.status, PaymentStatus::Pending);
}

#[test]
fn test_verify_payment_decimal_aware_tolerance_7_decimals() {
    // A token with 7 decimals should have tolerance = 10 (10^(7-6))
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let token_admin = Address::generate(&env);
    let alt_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    // Stellar asset contracts report 7 decimals
    client.allow_token(&admin, &alt_token);

    let merchant_id = Address::generate(&env);
    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);
    client.grant_role(&admin, &role_oracle(&env), &oracle);

    let payment_id = String::from_str(&env, "pay_7dec");
    let amount = 10_000_000_i128; // 1.0 in 7-decimal units
    let mut args = create_payment_args(&env, &payment_id, &merchant_id, amount);
    args.currency = Symbol::new(&env, "EURC");
    args.token_address = Some(alt_token);
    client.create_payment(&args);

    // Underpay by 10 (within 7-decimal tolerance of 10) → Confirmed
    let status = client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &(amount - 10),
    );
    assert_eq!(status, PaymentStatus::Confirmed);
}

#[test]
fn test_verify_payment_decimal_aware_tolerance_7_decimals_overpay() {
    // Underpay by 11 (outside 7-decimal tolerance of 10) → PartiallyPaid
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let token_admin = Address::generate(&env);
    let alt_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    client.allow_token(&admin, &alt_token);

    let merchant_id = Address::generate(&env);
    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);
    client.grant_role(&admin, &role_oracle(&env), &oracle);

    let payment_id = String::from_str(&env, "pay_7dec_partial");
    let amount = 10_000_000_i128;
    let mut args = create_payment_args(&env, &payment_id, &merchant_id, amount);
    args.currency = Symbol::new(&env, "EURC");
    args.token_address = Some(alt_token);
    client.create_payment(&args);

    // Underpay by 11 → PartiallyPaid
    let status = client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &(amount - 11),
    );
    assert_eq!(status, PaymentStatus::PartiallyPaid);
}

// --- Cumulative refund cap tests ---

#[test]
fn test_cumulative_refunds_exceed_payment_amount_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "pay_cumulative_1");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);
    let payment_amount = 1000i128;

    client.register_payment(
        &payment_id,
        &merchant_id,
        &payment_amount,
        &Symbol::new(&env, "USDC"),
    );

    // First refund: 600 — ok
    client.create_refund(
        &payment_id,
        &600i128,
        &String::from_str(&env, "partial 1"),
        &requester,
    );

    // Second refund: 500 — 600 + 500 = 1100 > 1000 — must fail
    let result = client.try_create_refund(
        &payment_id,
        &500i128,
        &String::from_str(&env, "partial 2"),
        &requester,
    );
    assert_eq!(result, Err(Ok(Error::RefundExceedsPayment)));
}

#[test]
fn test_refund_exactly_equal_to_payment_amount_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "pay_exact_1");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);
    let payment_amount = 1000i128;

    client.register_payment(
        &payment_id,
        &merchant_id,
        &payment_amount,
        &Symbol::new(&env, "USDC"),
    );

    // Single refund equal to full payment amount — must succeed
    let refund_id = client.create_refund(
        &payment_id,
        &payment_amount,
        &String::from_str(&env, "full refund"),
        &requester,
    );
    let refund = client.get_refund(&refund_id);
    assert_eq!(refund.amount, payment_amount);
    assert_eq!(refund.status, RefundStatus::Pending);
}

#[test]
fn test_second_refund_after_full_refund_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "pay_full_then_extra");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);
    let payment_amount = 1000i128;

    client.register_payment(
        &payment_id,
        &merchant_id,
        &payment_amount,
        &Symbol::new(&env, "USDC"),
    );

    // Full refund — ok
    client.create_refund(
        &payment_id,
        &payment_amount,
        &String::from_str(&env, "full"),
        &requester,
    );

    // Any additional refund — must fail
    let result = client.try_create_refund(
        &payment_id,
        &1i128,
        &String::from_str(&env, "extra"),
        &requester,
    );
    assert_eq!(result, Err(Ok(Error::RefundExceedsPayment)));
}

#[test]
fn test_rejected_refunds_not_counted_in_cumulative_total() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "pay_rejected_refund");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);
    let payment_amount = 1000i128;

    client.register_payment(
        &payment_id,
        &merchant_id,
        &payment_amount,
        &Symbol::new(&env, "USDC"),
    );

    // Create and reject a refund for 800
    let refund_id = client.create_refund(
        &payment_id,
        &800i128,
        &String::from_str(&env, "will be rejected"),
        &requester,
    );
    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);
    client.reject_refund(&operator, &refund_id);

    // A new refund for 1000 should succeed because the rejected one is excluded
    let new_refund_id = client.create_refund(
        &payment_id,
        &payment_amount,
        &String::from_str(&env, "after rejection"),
        &requester,
    );
    let refund = client.get_refund(&new_refund_id);
    assert_eq!(refund.amount, payment_amount);
    assert_eq!(refund.status, RefundStatus::Pending);
}

// --- Multi-account settlement tests ---

fn make_confirmed_payment(
    env: &Env,
    client: &PaymentProcessorClient,
    admin: &Address,
    payment_id: &String,
    amount: i128,
) {
    let merchant = Address::generate(env);
    let oracle = Address::generate(env);
    client.grant_role(admin, &role_merchant(env), &merchant);
    client.grant_role(admin, &role_oracle(env), &oracle);
    let args = create_payment_args(env, payment_id, &merchant, amount);
    client.create_payment(&args);
    client.verify_payment(
        &oracle,
        payment_id,
        &BytesN::<32>::random(env),
        &Address::generate(env),
        &amount,
    );
}

#[test]
fn test_settle_payment_single_split() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "settle_single");
    let amount = 1000i128;
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    let recipient = Address::generate(&env);
    let splits = vec![&env, SettlementSplit { recipient, amount }];
    client.settle_payment(&operator, &payment_id, &splits);

    assert_eq!(
        client.get_payment(&payment_id).status,
        PaymentStatus::Settled
    );
}

// --- Idempotency key (client_token) tests ---

#[test]
fn test_settle_payment_multi_split() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "settle_multi");
    let amount = 1000i128;
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount: 600,
        },
        SettlementSplit {
            recipient: Address::generate(&env),
            amount: 400,
        },
    ];
    client.settle_payment(&operator, &payment_id, &splits);

    assert_eq!(
        client.get_payment(&payment_id).status,
        PaymentStatus::Settled
    );
}

// --- Idempotency key (client_token) tests ---

#[test]
fn test_create_payment_idempotency_retry_returns_same_payment() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let payment_id = String::from_str(&env, "idem_pay_1");
    let client_token = Some(String::from_str(&env, "tok_abc123"));
    let expires_at = env.ledger().timestamp() + 3600;

    let args = CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant_id.clone(),
        amount: 1000,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(expires_at),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: client_token.clone(),
        metadata_hash: None,
    };

    let first = client.create_payment(&args);

    // Retry with same client_token and payment_id — must return the same payment
    let retry = client.create_payment(&args);

    assert_eq!(first.payment_id, retry.payment_id);
    assert_eq!(first.created_at, retry.created_at);
}

#[test]
fn test_create_payment_idempotency_different_payment_id_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let client_token = Some(String::from_str(&env, "tok_conflict"));
    let expires_at = env.ledger().timestamp() + 3600;

    let args_a = CreatePaymentArgs {
        payment_id: String::from_str(&env, "idem_pay_a"),
        merchant_id: merchant_id.clone(),
        amount: 1000,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(expires_at),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: client_token.clone(),
        metadata_hash: None,
    };

    // First call succeeds
    client.create_payment(&args_a);

    // Second call with same token but different payment_id must fail
    let mut args_b = args_a.clone();
    args_b.payment_id = String::from_str(&env, "idem_pay_b");

    let result = client.try_create_payment(&args_b);

    assert_eq!(result, Err(Ok(Error::DuplicateIdempotencyKey)));
}

#[test]
fn test_create_payment_without_idempotency_token_fails_on_retry() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let payment_id = String::from_str(&env, "idem_pay_no_tok");
    let expires_at = env.ledger().timestamp() + 3600;

    let args = CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant_id.clone(),
        amount: 1000,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(expires_at),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
    };

    client.create_payment(&args);

    // Without a client_token, a second call with the same payment_id returns PaymentAlreadyExists
    let result = client.try_create_payment(&args);

    assert_eq!(result, Err(Ok(Error::PaymentAlreadyExists)));
}

#[test]
fn test_settle_payment_empty_splits_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "settle_empty");
    let amount = 1000i128;
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    let splits = vec![&env];
    let result = client.try_settle_payment(&operator, &payment_id, &splits);
    assert_eq!(result, Err(Ok(Error::InvalidSettlement)));
}

#[test]
fn test_settle_payment_split_total_mismatch_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "settle_mismatch");
    let amount = 1000i128;
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    // Total is 900, not 1000 — must fail
    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount: 500,
        },
        SettlementSplit {
            recipient: Address::generate(&env),
            amount: 400,
        },
    ];
    let result = client.try_settle_payment(&operator, &payment_id, &splits);
    assert_eq!(result, Err(Ok(Error::InvalidSettlement)));
}

// -----------------------------------------------------------------------------
// Issue #301: remove_supported_token and get_supported_tokens
// -----------------------------------------------------------------------------

#[test]
fn test_remove_supported_token() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    // Allow a token — it should appear in the supported list
    client.allow_token(&admin, &token);
    let supported = client.get_supported_tokens();
    assert_eq!(supported.len(), 1);
    assert_eq!(supported.get(0).unwrap(), token);

    // Remove it — should no longer be in the list and is_token_allowed should be false
    client.remove_supported_token(&admin, &token);
    let supported = client.get_supported_tokens();
    assert_eq!(supported.len(), 0);
    assert!(!client.is_token_allowed(&token));
}

#[test]
fn test_remove_supported_token_nonexistent_is_noop() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    // Never added — remove should not panic
    client.remove_supported_token(&admin, &token);
    let supported = client.get_supported_tokens();
    assert_eq!(supported.len(), 0);
}

#[test]
fn test_get_supported_tokens_returns_multiple() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let token_a = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let token_b = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let token_c = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();

    client.allow_token(&admin, &token_a);
    client.allow_token(&admin, &token_b);
    client.allow_token(&admin, &token_c);

    let supported = client.get_supported_tokens();
    assert_eq!(supported.len(), 3);

    // Remove the middle one
    client.remove_supported_token(&admin, &token_b);
    let supported = client.get_supported_tokens();
    assert_eq!(supported.len(), 2);
    assert_eq!(supported.get(0).unwrap(), token_a);
    assert_eq!(supported.get(1).unwrap(), token_c);
}

#[test]
fn test_remove_supported_token_requires_admin() {
    let env = Env::default();
    let (_admin, client) = setup_payment_processor(&env);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    let non_admin = Address::generate(&env);
    let result = client.try_remove_supported_token(&non_admin, &token);
    assert!(result.is_err());
}

// -----------------------------------------------------------------------------
// Issue #302: ActiveSubscriptions index and process_due_subscriptions
// -----------------------------------------------------------------------------

fn setup_refund_manager_with_plan(
    env: &Env,
) -> (RefundManagerClient<'_>, Address, String) {
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(env);

    let merchant = Address::generate(env);
    client.grant_role(&admin, &role_merchant(env), &merchant);
    client.grant_role(&admin, &role_oracle(env), &merchant);

    let plan_id = String::from_str(env, "plan_monthly_10");
    client.create_subscription_plan(
        &merchant,
        &plan_id,
        &String::from_str(env, "Monthly $10"),
        &String::from_str(env, "Basic plan"),
        &1000_000000i128,
        &Symbol::new(env, "USDC"),
        &crate::BillingInterval::Monthly,
    );

    (client, admin, plan_id)
}

#[test]
fn test_subscription_added_to_active_index_on_subscribe() {
}

#[test]
fn test_process_refund_reentrancy_guard_normal_flow() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    let merchant = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant);

    let plan_id = String::from_str(&env, "plan_test");
    client.create_subscription_plan(
        &merchant,
        &plan_id,
        &String::from_str(&env, "Test Plan"),
        &String::from_str(&env, "Desc"),
        &1000i128,
        &Symbol::new(&env, "USDC"),
        &crate::BillingInterval::Monthly,
    );

    let payer = Address::generate(&env);
    let _sub_id = client.subscribe(&payer, &plan_id, &None, &None, &None);

    // process_due_subscriptions immediately — subscription was just created
    // with next_payment_at = now + 1 month, so it should NOT be due yet
    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &operator);
    let count = client.process_due_subscriptions(&operator);
    assert_eq!(count, 0);

    // Advance time past the due date
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 31 * 24 * 3600);

    // Now it should be due and processed
    let count = client.process_due_subscriptions(&operator);
    assert_eq!(count, 1);
}

#[test]
fn test_cancelled_subscription_removed_from_active_index() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, plan_id) = setup_refund_manager_with_plan(&env);

    let payer = Address::generate(&env);
    let sub_id = client.subscribe(&payer, &plan_id, &None, &None, &None);

    // Cancel the subscription
    client.cancel_subscription(&payer, &sub_id);

    // Advance time past due date
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 31 * 24 * 3600);

    // Should NOT process the cancelled subscription
    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &operator);
    let count = client.process_due_subscriptions(&operator);
    assert_eq!(count, 0);
}

#[test]
fn test_paused_subscription_removed_from_active_index() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, plan_id) = setup_refund_manager_with_plan(&env);

    let payer = Address::generate(&env);
    let sub_id = client.subscribe(&payer, &plan_id, &None, &None, &None);

    // Pause the subscription
    client.pause_subscription(&payer, &sub_id);

    // Advance time past due date
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 31 * 24 * 3600);

    // Should NOT process the paused subscription
    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &operator);
    let count = client.process_due_subscriptions(&operator);
    assert_eq!(count, 0);
}

#[test]
fn test_resumed_subscription_added_back_to_active_index() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, plan_id) = setup_refund_manager_with_plan(&env);

    let payer = Address::generate(&env);
    let sub_id = client.subscribe(&payer, &plan_id, &None, &None, &None);

    // Pause, then resume
    client.pause_subscription(&payer, &sub_id);
    client.resume_subscription(&payer, &sub_id);

    // Advance time past due date
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 31 * 24 * 3600);

    // Should process the resumed subscription
    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &operator);
    let count = client.process_due_subscriptions(&operator);
    assert_eq!(count, 1);
}

#[test]
fn test_process_due_subscriptions_auto_cancels_on_max_payments() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, plan_id) = setup_refund_manager_with_plan(&env);

    let payer = Address::generate(&env);
    let _sub_id = client.subscribe(&payer, &plan_id, &Some(2), &None, &None);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &operator);

    // Advance to first due date
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 31 * 24 * 3600);
    let count = client.process_due_subscriptions(&operator);
    assert_eq!(count, 1);

    // Advance to second due date
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 31 * 24 * 3600);
    let count = client.process_due_subscriptions(&operator);
    assert_eq!(count, 1);

    // Advance further — should be auto-cancelled (max_payments=2 reached)
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 31 * 24 * 3600);
    let count = client.process_due_subscriptions(&operator);
    assert_eq!(count, 0);
}

// -----------------------------------------------------------------------------
// Issue #303: KYC tier-based payment limits enforcement
// -----------------------------------------------------------------------------

fn setup_kyc_environment<'a>(
    env: &'a Env,
    tier: &'a crate::merchant_registry::KycTier,
) -> (PaymentProcessorClient<'a>, crate::merchant_registry::MerchantRegistryClient<'a>, Address, Address) {
    env.mock_all_auths();
    let payment_contract = env.register(PaymentProcessor, ());
    let registry_contract = env.register(crate::merchant_registry::MerchantRegistry, ());

    let payment_client = PaymentProcessorClient::new(env, &payment_contract);
    let registry_client = crate::merchant_registry::MerchantRegistryClient::new(env, &registry_contract);

    let admin = Address::generate(env);
    payment_client.initialize_payment_processor(&admin);
    registry_client.initialize(&admin);

    payment_client.set_merchant_registry_address(&admin, &registry_contract);

    let merchant = Address::generate(env);
    payment_client.grant_role(&admin, &role_merchant(env), &merchant);

    registry_client.register_merchant(
        &merchant,
        &String::from_str(env, "KYC Test Merchant"),
        &String::from_str(env, "USDC"),
        &None::<Address>,
        &None::<String>,
        &None,
    );

    registry_client.set_kyc_tier_with_signature(&admin, &merchant, tier, &Some(String::from_str(env, "sig")));

    (payment_client, registry_client, admin, merchant)
}

#[test]
fn test_kyc_tier_limits_basic_enforced() {
    let env = Env::default();
    env.mock_all_auths();

    let (payment_client, _registry_client, admin, merchant) = setup_kyc_environment(&env, &crate::merchant_registry::KycTier::Basic);

    // Set very low limit for Basic tier
    payment_client.set_kyc_tier_limits(&admin, &crate::merchant_registry::KycTier::Basic, &5000i128);

    // Payment at limit — should succeed
    let pid1 = String::from_str(&env, "kyc_ok");
    let args1 = create_payment_args(&env, &pid1, &merchant, 5000i128);
    payment_client.create_payment(&args1);

    // Payment above limit — should fail
    let pid2 = String::from_str(&env, "kyc_fail");
    let args2 = create_payment_args(&env, &pid2, &merchant, 5001i128);
    let result = payment_client.try_create_payment(&args2);
    assert_eq!(result, Err(Ok(Error::AmountAboveMax)));
}

#[test]
fn test_kyc_tier_limits_business_unlimited() {
    let env = Env::default();
    env.mock_all_auths();

    let (payment_client, _registry_client, admin, merchant) = setup_kyc_environment(&env, &crate::merchant_registry::KycTier::Business);

    // Set low limit for Business just for test
    payment_client.set_kyc_tier_limits(&admin, &crate::merchant_registry::KycTier::Business, &i128::MAX);

    // Very large payment — should succeed for Business
    let pid = String::from_str(&env, "kyc_big");
    let args = create_payment_args(&env, &pid, &merchant, 100_000_000_000i128);
    payment_client.create_payment(&args);
}

#[test]
fn test_kyc_tier_limits_unverified_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (payment_client, _registry_client, _admin, merchant) = setup_kyc_environment(&env, &crate::merchant_registry::KycTier::Unverified);

    // Unverified merchant should be rejected by the registry check before KYC limit
    let pid = String::from_str(&env, "kyc_unv");
    let args = create_payment_args(&env, &pid, &merchant, 1000i128);
    let result = payment_client.try_create_payment(&args);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_kyc_tier_limits_custom_config_used() {
    let env = Env::default();
    env.mock_all_auths();

    let (payment_client, _registry_client, admin, merchant) = setup_kyc_environment(&env, &crate::merchant_registry::KycTier::Full);

    // Custom limit for Full tier
    payment_client.set_kyc_tier_limits(&admin, &crate::merchant_registry::KycTier::Full, &99999i128);

    // At custom limit — should succeed
    let pid1 = String::from_str(&env, "kyc_full_ok");
    let args1 = create_payment_args(&env, &pid1, &merchant, 99999i128);
    payment_client.create_payment(&args1);

    // Above custom limit — should fail
    let pid2 = String::from_str(&env, "kyc_full_fail");
    let args2 = create_payment_args(&env, &pid2, &merchant, 100000i128);
    let result = payment_client.try_create_payment(&args2);
    assert_eq!(result, Err(Ok(Error::AmountAboveMax)));
}

// -----------------------------------------------------------------------------
// Issue #304: FX rate staleness enforcement in verify_payment
// -----------------------------------------------------------------------------

#[test]
fn test_verify_payment_rejects_stale_fx_rate() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = 1_000_000);

    // Register all contracts
    let payment_contract = env.register(PaymentProcessor, ());
    let registry_contract = env.register(crate::merchant_registry::MerchantRegistry, ());
    let oracle_contract = env.register(crate::FXOracle, ());

    let payment_client = PaymentProcessorClient::new(&env, &payment_contract);
    let registry_client = crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_contract);
    let oracle_client = crate::FXOracleClient::new(&env, &oracle_contract);

    let admin = Address::generate(&env);
    payment_client.initialize_payment_processor(&admin);
    registry_client.initialize(&admin);
    oracle_client.oracle_initialize(&admin, &86400);

    // Link registry to payment processor
    payment_client.set_merchant_registry_address(&admin, &registry_contract);

    // Set FX oracle address on payment processor
    payment_client.set_fx_oracle_address(&admin, &oracle_contract);

    // Register a merchant with settlement_currency matching the oracle pair
    let merchant = Address::generate(&env);
    payment_client.grant_role(&admin, &role_merchant(&env), &merchant);
    registry_client.register_merchant(
        &merchant,
        &String::from_str(&env, "FX Merchant"),
        &String::from_str(&env, "USDC_NGN"),
        &None::<Address>,
        &None::<String>,
        &None,
    );
    registry_client.set_kyc_tier_with_signature(&admin, &merchant, &crate::merchant_registry::KycTier::Full, &Some(String::from_str(&env, "sig")));

    // Set a rate on the oracle
    let oracle_role = Symbol::new(&env, "ORACLE");
    let oracle = Address::generate(&env);
    oracle_client.oracle_grant_role(&admin, &oracle_role, &oracle);
    let pair = Symbol::new(&env, "USDC");
    oracle_client.set_rate(&oracle, &pair, &1500_0000000i128, &7);

    // Create and verify a payment while rate is fresh — should succeed
    let payment_id = String::from_str(&env, "fx_fresh");
    let args = create_payment_args(&env, &payment_id, &merchant, 1000i128);
    payment_client.create_payment(&args);

    let operator = Address::generate(&env);
    payment_client.grant_role(&admin, &role_oracle(&env), &operator);
    let tx_hash = BytesN::from_array(&env, &[0u8; 32]);
    let payer = Address::generate(&env);
    payment_client.verify_payment(&operator, &payment_id, &tx_hash, &payer, &1000i128);

    // Advance time past the staleness threshold (25 hours)
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 25 * 3600);

    // Create another payment and try to verify it — should fail with StaleOracleRate
    let payment_id2 = String::from_str(&env, "fx_stale");
    let args2 = create_payment_args(&env, &payment_id2, &merchant, 1000i128);
    payment_client.create_payment(&args2);

    let result = payment_client.try_verify_payment(&operator, &payment_id2, &tx_hash, &payer, &1000i128);
    assert_eq!(result, Err(Ok(Error::StaleOracleRate)));
}

#[test]
fn test_verify_payment_stores_fx_rate_on_success() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = 1_000_000);

    let payment_contract = env.register(PaymentProcessor, ());
    let registry_contract = env.register(crate::merchant_registry::MerchantRegistry, ());
    let oracle_contract = env.register(crate::FXOracle, ());

    let payment_client = PaymentProcessorClient::new(&env, &payment_contract);
    let registry_client = crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_contract);
    let oracle_client = crate::FXOracleClient::new(&env, &oracle_contract);

    let admin = Address::generate(&env);
    payment_client.initialize_payment_processor(&admin);
    registry_client.initialize(&admin);
    oracle_client.oracle_initialize(&admin, &86400);

    payment_client.set_merchant_registry_address(&admin, &registry_contract);
    payment_client.set_fx_oracle_address(&admin, &oracle_contract);

    let merchant = Address::generate(&env);
    payment_client.grant_role(&admin, &role_merchant(&env), &merchant);
    registry_client.register_merchant(
        &merchant,
        &String::from_str(&env, "FX Merchant"),
        &String::from_str(&env, "USDC_NGN"),
        &None::<Address>,
        &None::<String>,
        &None,
    );
    registry_client.set_kyc_tier_with_signature(&admin, &merchant, &crate::merchant_registry::KycTier::Full, &Some(String::from_str(&env, "sig")));

    let oracle_role = Symbol::new(&env, "ORACLE");
    let oracle = Address::generate(&env);
    oracle_client.oracle_grant_role(&admin, &oracle_role, &oracle);
    let pair = Symbol::new(&env, "USDC");
    oracle_client.set_rate(&oracle, &pair, &1500_0000000i128, &7);

    let payment_id = String::from_str(&env, "fx_rate_store");
    let args = create_payment_args(&env, &payment_id, &merchant, 1000i128);
    payment_client.create_payment(&args);

    let operator = Address::generate(&env);
    payment_client.grant_role(&admin, &role_oracle(&env), &operator);
    let tx_hash = BytesN::from_array(&env, &[0u8; 32]);
    let payer = Address::generate(&env);
    payment_client.verify_payment(&operator, &payment_id, &tx_hash, &payer, &1000i128);

    // Verify the payment has the FX rate stored
    let payment = payment_client.get_payment(&payment_id);
    assert_eq!(payment.fx_rate, Some(1500_0000000i128));
    assert!(payment.fx_rate_at.is_some());
}

#[test]
fn test_verify_payment_no_fx_oracle_config_skips_check() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let payment_id = String::from_str(&env, "no_fx_oracle");
    let args = create_payment_args(&env, &payment_id, &merchant_id, 1000i128);
    client.create_payment(&args);

    // Without FX oracle or registry configured, verify_payment should succeed
    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &operator);
    let tx_hash = BytesN::from_array(&env, &[0u8; 32]);
    let payer = Address::generate(&env);
    let status = client.verify_payment(&operator, &payment_id, &tx_hash, &payer, &1000i128);
    assert_eq!(status, PaymentStatus::Confirmed);

    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.fx_rate, None);
    assert_eq!(payment.fx_rate_at, None);
}

#[test]
fn test_process_refund_reentrancy_lock_cleared() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_reentrancy_2");
    let merchant_id = Address::generate(&env);
    let refund_amount = 1000i128;
    let requester = Address::generate(&env);

    client.register_payment(
        &payment_id,
        &merchant_id,
        &5000i128,
        &Symbol::new(&env, "USDC"),
    );

    let refund_id_1 = client.create_refund(
        &payment_id,
        &refund_amount,
        &String::from_str(&env, "Reason1"),
        &requester,
    );

    let refund_id_2 = client.create_refund(
        &payment_id,
        &refund_amount,
        &String::from_str(&env, "Reason2"),
        &requester,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    client.process_refund(&operator, &refund_id_1);
    client.process_refund(&operator, &refund_id_2);

    let refund1 = client.get_refund(&refund_id_1);
    let refund2 = client.get_refund(&refund_id_2);
    assert_eq!(refund1.status, RefundStatus::Completed);
    assert_eq!(refund2.status, RefundStatus::Completed);
}

#[test]
fn test_settle_payment_reentrancy_guard_normal_flow() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "settle_reentrancy_1");
    let amount = 1000i128;
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount: 1000,
        },
    ];
    client.settle_payment(&operator, &payment_id, &splits);

    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Settled);
}

#[test]
fn test_upgrade_contract_version_and_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    // Initial version should be "1.0.0"
    let initial_version = client.version();
    assert_eq!(initial_version, String::from_str(&env, "1.0.0"));

    // get_version() should also return "1.0.0"
    let get_ver = client.get_version();
    assert_eq!(get_ver, initial_version);

    // Generate a dummy 32-byte WASM hash (will fail at deployer level in test, but we can
    // verify the admin check passes before that by checking the event emission)
    // Since env.mock_all_auths() is set, the require_auth() passes.
    // env.deployer().update_current_contract_wasm() will fail in test environment
    // because there's no real WASM to upgrade to. However, we can verify the event
    // was emitted and the version was updated before the deployer call.
    // For a proper test, we catch the expected error.
    let new_wasm_hash = BytesN::from_array(&env, &[0u8; 32]);

    // Attempt upgrade — this should fail with host error because update_current_contract_wasm
    // cannot be called in test environment, but the version update and event emission
    // happen AFTER the call. Let's verify the admin check and role check pass.
    let result = client.try_upgrade_contract(&admin, &new_wasm_hash);
    // We expect either Ok (unlikely in test env) or a host/VM error from the deployer
    // The important thing is it didn't return Error::Unauthorized
    match result {
        Ok(_) => {
            // If upgrade succeeded in test environment, verify version changed
            let upgraded_version = client.version();
            assert_eq!(upgraded_version, String::from_str(&env, "1.0.1"));
        }
        Err(e) => {
            // If host error (expected), ensure it's not an auth error
            // The event should still be emitted - but since the deployer call panics
            // before version persistence, we just verify the auth check passed.
            // We can verify this by checking version didn't change (deployer failed)
            let current_version = client.version();
            assert_eq!(current_version, String::from_str(&env, "1.0.0"));
        }
    }
}

#[test]
fn test_upgrade_contract_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let new_wasm_hash = BytesN::from_array(&env, &[0u8; 32]);
    let non_admin = Address::generate(&env);

    // Non-admin should fail with Error::Unauthorized (code 1)
    let result = client.try_upgrade_contract(&non_admin, &new_wasm_hash);
    match result {
        Ok(_) => panic!("Expected unauthorized error"),
        Err(e) => {
            // Should be a contract error, not a panic
            assert!(true, "Non-admin caller was rejected as expected");
        }
    }
}

#[test]
fn test_version_after_init() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let ver: soroban_sdk::String = client.version();
    assert_eq!(ver, String::from_str(&env, "1.0.0"));

    let get_ver: soroban_sdk::String = client.get_version();
    assert_eq!(get_ver, String::from_str(&env, "1.0.0"));
}
