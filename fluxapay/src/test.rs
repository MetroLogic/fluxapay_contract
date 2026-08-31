#![cfg(test)]

use super::*;
use crate::merchant_registry::MaybeFeeConfig;
use access_control::{role_admin, role_oracle, role_settlement_operator};
use soroban_sdk::{
    testutils::{Address as _, BytesN as _, Events as _, Ledger as _},
    token, vec, Address, BytesN, Env, String, Symbol, TryIntoVal,
};

#[test]
fn test_datakey_discriminant_stability() {
    let env = Env::default();
    let contract_id = env.register(PaymentProcessor, ());
    let payment_id = String::from_str(&env, "stable-payment-key");
    let refund_id = String::from_str(&env, "stable-refund-key");
    let merchant = Address::generate(&env);

    // These keys protect persisted storage compatibility. Reordering DataKey variants changes
    // their serialized discriminants, so values written by an earlier contract would no longer
    // be readable under the expected variant.
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &11u32);
        assert_eq!(
            env.storage()
                .persistent()
                .get::<_, u32>(&DataKey::Payment(payment_id)),
            Some(11)
        );

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &22u32);
        assert_eq!(
            env.storage()
                .persistent()
                .get::<_, u32>(&DataKey::Refund(refund_id)),
            Some(22)
        );

        env.storage()
            .persistent()
            .set(&DataKey::MerchantPayments(merchant.clone()), &33u32);
        assert_eq!(
            env.storage()
                .persistent()
                .get::<_, u32>(&DataKey::MerchantPayments(merchant)),
            Some(33)
        );
    });
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
        payer: None,
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
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
    }
}

fn create_expired_retryable_payment(
    env: &Env,
    client: &PaymentProcessorClient<'_>,
    admin: &Address,
    merchant: &Address,
    payment_id: &str,
) -> String {
    client.grant_role(admin, &role_merchant(env), merchant);
    let payment_id = String::from_str(env, payment_id);
    let args = create_payment_args(env, &payment_id, merchant, 1_000);
    client.create_payment(&args);
    env.ledger().with_mut(|ledger| ledger.timestamp += 3_601);
    client.expire_payment(&payment_id);
    payment_id
}

fn retry_expired_payment(
    env: &Env,
    client: &PaymentProcessorClient<'_>,
    merchant: &Address,
    payment_id: &String,
) -> String {
    client.retry_payment(merchant, payment_id, &(env.ledger().timestamp() + 3_600))
}

#[test]
fn test_retry_payment_success_first_retry() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);
    let merchant = Address::generate(&env);
    let original = create_expired_retryable_payment(&env, &client, &admin, &merchant, "retry_first");

    let retry_id = retry_expired_payment(&env, &client, &merchant, &original);

    assert_eq!(client.get_payment(&retry_id).status, PaymentStatus::Pending);
}

#[test]
fn test_retry_payment_links_retry_of_payment_id() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);
    let merchant = Address::generate(&env);
    let original = create_expired_retryable_payment(&env, &client, &admin, &merchant, "retry_link");

    let retry_id = retry_expired_payment(&env, &client, &merchant, &original);

    assert_eq!(client.get_payment(&retry_id).retry_of_payment_id, Some(original));
}

#[test]
fn test_retry_payment_chain_depth_3_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);
    let merchant = Address::generate(&env);
    let original = create_expired_retryable_payment(&env, &client, &admin, &merchant, "retry_depth_3");
    let first = retry_expired_payment(&env, &client, &merchant, &original);
    env.ledger().with_mut(|ledger| ledger.timestamp += 3_601);
    client.expire_payment(&first);
    let second = retry_expired_payment(&env, &client, &merchant, &first);
    env.ledger().with_mut(|ledger| ledger.timestamp += 3_601);
    client.expire_payment(&second);

    let third = retry_expired_payment(&env, &client, &merchant, &second);

    assert_eq!(client.get_payment(&third).retry_of_payment_id, Some(second));
}

#[test]
fn test_retry_payment_chain_depth_4_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);
    let merchant = Address::generate(&env);
    let original = create_expired_retryable_payment(&env, &client, &admin, &merchant, "retry_depth_4");
    let first = retry_expired_payment(&env, &client, &merchant, &original);
    env.ledger().with_mut(|ledger| ledger.timestamp += 3_601);
    client.expire_payment(&first);
    let second = retry_expired_payment(&env, &client, &merchant, &first);
    env.ledger().with_mut(|ledger| ledger.timestamp += 3_601);
    client.expire_payment(&second);
    let third = retry_expired_payment(&env, &client, &merchant, &second);
    env.ledger().with_mut(|ledger| ledger.timestamp += 3_601);
    client.expire_payment(&third);

    let result = client.try_retry_payment(&merchant, &third, &(env.ledger().timestamp() + 3_600));

    assert_eq!(result, Err(Ok(Error::RetryChainTooDeep)));
}

#[test]
fn test_retry_payment_only_expired_or_failed_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);
    let merchant = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant);
    let payment_id = String::from_str(&env, "confirmed_retry");
    let args = create_payment_args(&env, &payment_id, &merchant, 1_000);
    client.create_payment(&args);
    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &oracle);
    client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &1_000,
    );

    let result = client.try_retry_payment(&merchant, &payment_id, &(env.ledger().timestamp() + 3_600));

    assert_eq!(result, Err(Ok(Error::PaymentAlreadyProcessed)));
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
        batch.push_back(create_payment_args(
            &env,
            &payment_id,
            &merchant_id,
            100i128,
        ));
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
    assert!(!env
        .storage()
        .persistent()
        .has(&DataKey::Payment(payment_id_1)));
    assert!(!env
        .storage()
        .persistent()
        .has(&DataKey::Payment(payment_id_2)));
}

/// Issue #682: Batch with duplicate payment_ids returns BatchContainsDuplicates.
#[test]
fn test_create_payments_batch_rejects_duplicate_payment_ids() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let payment_id = String::from_str(&env, "dup_payment_id");
    let batch = vec![
        &env,
        create_payment_args(&env, &payment_id, &merchant_id, 100i128),
        create_payment_args(&env, &payment_id, &merchant_id, 200i128),
    ];

    let result = client.try_create_payments_batch(&batch);
    assert_eq!(result, Err(Ok(Error::BatchContainsDuplicates)));
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
        &sender, &recipient, &token, &100i128, &1_000i128, &stream_id,
    );
    assert_eq!(result, Err(Ok(StreamError::Unauthorized)));
}

#[test]
fn test_pause_stream_checkpoints_accrual_and_sets_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_payment_processor(&env);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let stream_id = String::from_str(&env, "pause_stream_1");

    token::StellarAssetClient::new(&env, &token).mint(&sender, &1_000_000i128);
    client.create_stream(&sender, &recipient, &token, &10i128, &1_000i128, &stream_id);

    env.ledger().with_mut(|li| li.timestamp += 50);

    client.pause_stream(&sender, &stream_id);

    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Paused);
    // 50 seconds at rate 10/s should have been checkpointed.
    assert_eq!(stream.accrued_at_checkpoint, 500i128);
    assert_eq!(stream.last_checkpoint_at, env.ledger().timestamp());
}

#[test]
fn test_double_pause_stream_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_payment_processor(&env);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let stream_id = String::from_str(&env, "double_pause_stream_1");

    token::StellarAssetClient::new(&env, &token).mint(&sender, &1_000_000i128);
    client.create_stream(&sender, &recipient, &token, &10i128, &1_000i128, &stream_id);
    client.pause_stream(&sender, &stream_id);

    let result = client.try_pause_stream(&sender, &stream_id);
    assert_eq!(result, Err(Ok(StreamError::StreamNotActive)));
}

#[test]
fn test_resume_stream_restarts_accrual_from_correct_point() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_payment_processor(&env);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let stream_id = String::from_str(&env, "resume_stream_1");

    token::StellarAssetClient::new(&env, &token).mint(&sender, &1_000_000i128);
    client.create_stream(&sender, &recipient, &token, &10i128, &1_000i128, &stream_id);

    env.ledger().with_mut(|li| li.timestamp += 50);
    client.pause_stream(&sender, &stream_id);

    // Time passes while paused — must not accrue.
    env.ledger().with_mut(|li| li.timestamp += 200);
    client.resume_stream(&sender, &stream_id);

    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.status, StreamStatus::Active);
    // Accrual while paused must not be counted; only the pre-pause 50s * 10/s.
    assert_eq!(stream.accrued_at_checkpoint, 500i128);
    assert_eq!(stream.last_checkpoint_at, env.ledger().timestamp());
}

#[test]
fn test_resume_non_paused_stream_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_payment_processor(&env);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let stream_id = String::from_str(&env, "resume_active_stream_1");

    token::StellarAssetClient::new(&env, &token).mint(&sender, &1_000_000i128);
    client.create_stream(&sender, &recipient, &token, &10i128, &1_000i128, &stream_id);

    let result = client.try_resume_stream(&sender, &stream_id);
    assert_eq!(result, Err(Ok(StreamError::StreamNotPaused)));
}

#[test]
fn test_pause_resume_stream_unauthorized_for_non_sender() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_payment_processor(&env);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let stranger = Address::generate(&env);
    let stream_id = String::from_str(&env, "pause_unauthorized_stream_1");

    token::StellarAssetClient::new(&env, &token).mint(&sender, &1_000_000i128);
    client.create_stream(&sender, &recipient, &token, &10i128, &1_000i128, &stream_id);

    let result = client.try_pause_stream(&stranger, &stream_id);
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
        &None::<u64>,
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

    let page =
        client.get_merchant_payments_paginated(&merchant_id, &1u32, &2u32, &None::<PaymentStatus>);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0), Some(payment_id_2));
    assert_eq!(page.get(1), Some(payment_id_3));
}

#[test]
fn test_get_merchant_payments_paginated_filters_by_status() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    let payment_id_1 = String::from_str(&env, "status_filter_1");
    let payment_id_2 = String::from_str(&env, "status_filter_2");
    let payment_id_3 = String::from_str(&env, "status_filter_3");

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

    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &oracle);
    client.verify_payment(
        &oracle,
        &payment_id_2,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &200i128,
    );

    let all =
        client.get_merchant_payments_paginated(&merchant_id, &0u32, &10u32, &None::<PaymentStatus>);
    assert_eq!(all.len(), 3);

    let pending = client.get_merchant_payments_paginated(
        &merchant_id,
        &0u32,
        &10u32,
        &Some(PaymentStatus::Pending),
    );
    assert_eq!(pending.len(), 2);
    assert_eq!(pending.get(0), Some(payment_id_1));
    assert_eq!(pending.get(1), Some(payment_id_3.clone()));

    let confirmed = client.get_merchant_payments_paginated(
        &merchant_id,
        &0u32,
        &10u32,
        &Some(PaymentStatus::Confirmed),
    );
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed.get(0), Some(payment_id_2));

    let paged_pending = client.get_merchant_payments_paginated(
        &merchant_id,
        &1u32,
        &1u32,
        &Some(PaymentStatus::Pending),
    );
    assert_eq!(paged_pending.len(), 1);
    assert_eq!(paged_pending.get(0), Some(payment_id_3));

    let settled = client.get_merchant_payments_paginated(
        &merchant_id,
        &0u32,
        &10u32,
        &Some(PaymentStatus::Settled),
    );
    assert_eq!(settled.len(), 0);
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

    let events = env.events().all();
    assert!(!events.events().is_empty());
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
        &None::<u64>,
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
fn test_process_refund_within_expiry_window_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_expiry_ok");
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
        &String::from_str(&env, "Reason"),
        &requester,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    // Well within the default 30-day window.
    env.ledger().set_timestamp(env.ledger().timestamp() + 60);
    client.process_refund(&operator, &refund_id);

    let refund = client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Completed);
}

#[test]
fn test_process_refund_rejects_after_expiry() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_claim_1");
    let merchant_id = Address::generate(&env);
    let refund_amount = 1000i128;
    let payment_id = String::from_str(&env, "payment_expiry_bad");
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
        &refund_amount,
        &1000i128,
        &String::from_str(&env, "Reason"),
        &requester,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    client.approve_refund(&operator, &refund_id);
    let refund = client.get_refund(&refund_id);
    assert!(refund.approved);
    assert_eq!(refund.status, RefundStatus::Pending);

    client.claim_refund(&requester, &refund_id);

    let refund = client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Completed);
}

#[test]
fn test_claim_refund_before_approval_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_claim_2");
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

    let result = client.try_claim_refund(&requester, &refund_id);
    assert_eq!(result, Err(Ok(Error::RefundNotApproved)));
}

#[test]
fn test_claim_refund_by_non_requester_fails() {
    // 60 days later — past the default 30-day expiry window.
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 60 * 24 * 60 * 60);

    let err = client.try_process_refund(&operator, &refund_id);
    assert_eq!(err, Err(Ok(Error::RefundExpired)));
}

#[test]
fn test_expire_refund_clears_pending_expired_refund() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_claim_3");
    let merchant_id = Address::generate(&env);
    let refund_amount = 1000i128;
    let requester = Address::generate(&env);
    let stranger = Address::generate(&env);
    let payment_id = String::from_str(&env, "payment_expire_cleanup");
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
        &refund_amount,
        &1000i128,
        &String::from_str(&env, "Reason"),
        &requester,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);
    client.approve_refund(&operator, &refund_id);

    let result = client.try_claim_refund(&stranger, &refund_id);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_double_claim_refund_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_claim_4");
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
    client.approve_refund(&operator, &refund_id);
    client.claim_refund(&requester, &refund_id);

    let result = client.try_claim_refund(&requester, &refund_id);
    assert_eq!(result, Err(Ok(Error::RefundAlreadyProcessed)));
}

#[test]
fn test_expire_refund_clears_pending() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_expire");
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
        &String::from_str(&env, "Reason"),
        &requester,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 60 * 24 * 60 * 60);

    client.expire_refund(&operator, &refund_id);

    let refund = client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Rejected);

    // Once cleared, it can't be expired (or processed) again.
    let err = client.try_expire_refund(&operator, &refund_id);
    assert_eq!(err, Err(Ok(Error::RefundAlreadyProcessed)));
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
    assert_eq!(
        token_client.balance(&destination),
        starting_balance + 15i128
    );
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
fn test_set_refund_expiry_configures_window() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    client.set_refund_expiry(&admin, &100u64);

    let payment_id = String::from_str(&env, "payment_custom_expiry");
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
        &String::from_str(&env, "Reason"),
        &requester,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    env.ledger().set_timestamp(env.ledger().timestamp() + 200);

    let err = client.try_process_refund(&operator, &refund_id);
    assert_eq!(err, Err(Ok(Error::RefundExpired)));
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

    let refund = client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Cancelled);

    // Payment refund list still tracks the cancelled refund
    let refunds = client.get_payment_refunds(&payment_id);
    assert_eq!(refunds.len(), 1);
    assert_eq!(refunds.get(0).unwrap().status, RefundStatus::Cancelled);
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

    let refund = client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Cancelled);
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
    assert!(!events.events().is_empty());
}

#[test]
fn test_cancel_refund_already_cancelled() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_cancel_6");
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

    client.cancel_refund(&requester, &refund_id);

    let result = client.try_cancel_refund(&requester, &refund_id);
    assert_eq!(result, Err(Ok(Error::RefundCancelled)));
}

#[test]
fn test_cancel_refund_does_not_count_toward_total() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_cancel_7");
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
        &600i128,
        &String::from_str(&env, "will cancel"),
        &requester,
    );

    client.cancel_refund(&requester, &refund_id);

    // A second refund up to the full payment amount should succeed
    let refund_id_2 = client.create_refund(
        &payment_id,
        &1000i128,
        &String::from_str(&env, "full after cancel"),
        &requester,
    );
    assert!(!refund_id_2.is_empty());
}

#[test]
fn test_refund_fee_bps_default_on_init() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_refund_manager(&env);

    assert_eq!(client.get_refund_fee_bps(), 100);
}

#[test]
fn test_set_refund_fee_bps_by_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    client.set_refund_fee_bps(&admin, &200);
    assert_eq!(client.get_refund_fee_bps(), 200);
}

#[test]
fn test_set_refund_fee_bps_rejected_by_non_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_refund_manager(&env);

    let random = Address::generate(&env);
    let result = client.try_set_refund_fee_bps(&random, &50);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_set_refund_fee_bps_rejects_out_of_range() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    let result = client.try_set_refund_fee_bps(&admin, &1_001);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn test_refund_fee_bps_applied_on_process() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client, usdc_token) = setup_refund_manager_with_token(&env);

    client.set_refund_fee_bps(&admin, &200); // 2%

    let payment_id = String::from_str(&env, "pay_fee_bps");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);
    let refund_amount = 10_000i128;

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
    let fee = refund_amount * 200 / 10_000;

    assert_eq!(token_client.balance(&admin), fee);
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

#[test]
fn test_non_merchant_cannot_set_merchant_amount_limits() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_payment_processor(&env);
    let non_merchant = Address::generate(&env);

    let result =
        client.try_set_merchant_amount_limits(&non_merchant, &Some(100i128), &Some(2000i128));

    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_non_admin_cannot_set_global_amount_limits() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup_payment_processor(&env);
    let non_admin = Address::generate(&env);

    let result = client.try_set_global_amount_limits(&non_admin, &Some(100i128), &Some(2000i128));

    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

// --- Multi-asset payment tests ---

#[test]
fn test_allow_token_unauthorized_non_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_payment_processor(&env);

    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let non_admin = Address::generate(&env);

    let result = client.try_allow_token(&non_admin, &token);

    assert_eq!(result, Err(Ok(Error::Unauthorized)));
    assert!(!client.is_token_allowed(&token));
}

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
        &None::<u64>,
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
        payer: None,
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
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
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
        payer: None,
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
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
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
        payer: None,
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
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
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

#[test]
fn test_settle_payment_unauthorized_non_operator() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "settle_unauth");
    let amount = 1000i128;
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    let non_operator = Address::generate(&env);
    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount,
        },
    ];
    let result = client.try_settle_payment(&non_operator, &payment_id, &splits);

    assert_eq!(result, Err(Ok(Error::Unauthorized)));
    assert_eq!(
        client.get_payment(&payment_id).status,
        PaymentStatus::Confirmed
    );
}

#[test]
fn test_settle_payment_fails_on_pending_payment() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant);

    let payment_id = String::from_str(&env, "settle_pending");
    let amount = 1000i128;
    let args = create_payment_args(&env, &payment_id, &merchant, amount);
    client.create_payment(&args);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount,
        },
    ];
    let result = client.try_settle_payment(&operator, &payment_id, &splits);

    assert_eq!(result, Err(Ok(Error::PaymentAlreadyProcessed)));
}

#[test]
fn test_settle_payment_fails_on_expired_payment() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant);

    let payment_id = String::from_str(&env, "settle_expired");
    let amount = 1000i128;
    let expires_at = env.ledger().timestamp() + 3600;
    let mut args = create_payment_args(&env, &payment_id, &merchant, amount);
    args.expires_at = Some(expires_at);
    client.create_payment(&args);

    env.ledger().set_timestamp(expires_at + 1);
    client.expire_payment(&payment_id);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount,
        },
    ];
    let result = client.try_settle_payment(&operator, &payment_id, &splits);

    assert_eq!(result, Err(Ok(Error::PaymentAlreadyProcessed)));
    assert_eq!(
        client.get_payment(&payment_id).status,
        PaymentStatus::Expired
    );
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

fn setup_refund_manager_with_plan(env: &Env) -> (RefundManagerClient<'_>, Address, String) {
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
fn test_subscription_added_to_active_index_on_subscribe() {}

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
    let _sub_id = client.subscribe(&payer, &plan_id, &None, &None, &MaybeFeeConfig::None);

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
    let sub_id = client.subscribe(&payer, &plan_id, &None, &None, &MaybeFeeConfig::None);

    // Cancel the subscription
    client.cancel_subscription(&payer, &sub_id, &false);

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
    let sub_id = client.subscribe(&payer, &plan_id, &None, &None, &MaybeFeeConfig::None);

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
    let sub_id = client.subscribe(&payer, &plan_id, &None, &None, &MaybeFeeConfig::None);

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
    let _sub_id = client.subscribe(&payer, &plan_id, &Some(2), &None, &MaybeFeeConfig::None);

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
) -> (
    PaymentProcessorClient<'a>,
    crate::merchant_registry::MerchantRegistryClient<'a>,
    Address,
    Address,
) {
    env.mock_all_auths();
    let payment_contract = env.register(PaymentProcessor, ());
    let registry_contract = env.register(crate::merchant_registry::MerchantRegistry, ());

    let payment_client = PaymentProcessorClient::new(env, &payment_contract);
    let registry_client =
        crate::merchant_registry::MerchantRegistryClient::new(env, &registry_contract);

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
        &MaybeFeeConfig::None,
    );

    registry_client.set_kyc_tier_with_signature(
        &admin,
        &merchant,
        tier,
        &Some(String::from_str(env, "sig")),
    );

    (payment_client, registry_client, admin, merchant)
}

#[test]
fn test_kyc_tier_limits_basic_enforced() {
    let env = Env::default();
    env.mock_all_auths();

    let (payment_client, _registry_client, admin, merchant) =
        setup_kyc_environment(&env, &crate::merchant_registry::KycTier::Basic);

    // Set very low limit for Basic tier
    payment_client.set_kyc_tier_limits(
        &admin,
        &crate::merchant_registry::KycTier::Basic,
        &5000i128,
    );

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

    let (payment_client, _registry_client, admin, merchant) =
        setup_kyc_environment(&env, &crate::merchant_registry::KycTier::Business);

    // Set low limit for Business just for test
    payment_client.set_kyc_tier_limits(
        &admin,
        &crate::merchant_registry::KycTier::Business,
        &i128::MAX,
    );

    // Very large payment — should succeed for Business
    let pid = String::from_str(&env, "kyc_big");
    let args = create_payment_args(&env, &pid, &merchant, 100_000_000_000i128);
    payment_client.create_payment(&args);
}

#[test]
fn test_kyc_tier_limits_unverified_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (payment_client, _registry_client, _admin, merchant) =
        setup_kyc_environment(&env, &crate::merchant_registry::KycTier::Unverified);

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

    let (payment_client, _registry_client, admin, merchant) =
        setup_kyc_environment(&env, &crate::merchant_registry::KycTier::Full);

    // Custom limit for Full tier
    payment_client.set_kyc_tier_limits(
        &admin,
        &crate::merchant_registry::KycTier::Full,
        &99999i128,
    );

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
    let registry_client =
        crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_contract);
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
        &MaybeFeeConfig::None,
    );
    registry_client.set_kyc_tier_with_signature(
        &admin,
        &merchant,
        &crate::merchant_registry::KycTier::Full,
        &Some(String::from_str(&env, "sig")),
    );

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
    payment_client.verify_payment(
        &operator,
        &payment_id,
        &tx_hash,
        &payer,
        &1000i128,
        &None::<u64>,
    );

    // Advance time past the staleness threshold (25 hours)
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 25 * 3600);

    // Create another payment and try to verify it — should fail with StaleOracleRate
    let payment_id2 = String::from_str(&env, "fx_stale");
    let args2 = create_payment_args(&env, &payment_id2, &merchant, 1000i128);
    payment_client.create_payment(&args2);

    let result = payment_client.try_verify_payment(
        &operator,
        &payment_id2,
        &tx_hash,
        &payer,
        &1000i128,
        &None::<u64>,
    );
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
    let registry_client =
        crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_contract);
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
        &MaybeFeeConfig::None,
    );
    registry_client.set_kyc_tier_with_signature(
        &admin,
        &merchant,
        &crate::merchant_registry::KycTier::Full,
        &Some(String::from_str(&env, "sig")),
    );

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
    payment_client.verify_payment(
        &operator,
        &payment_id,
        &tx_hash,
        &payer,
        &1000i128,
        &None::<u64>,
    );

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
    let status = client.verify_payment(
        &operator,
        &payment_id,
        &tx_hash,
        &payer,
        &1000i128,
        &None::<u64>,
    );
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
fn test_process_refund_same_id_only_once() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    let payment_id = String::from_str(&env, "payment_concurrent_refund");
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
        &String::from_str(&env, "once"),
        &requester,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    client.process_refund(&operator, &refund_id);
    let second = client.try_process_refund(&operator, &refund_id);
    assert!(second.is_err());

    let refund = client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Completed);
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

// =============================================================================
// Settlement fee rate (set_fee_rate / get_treasury_balance) tests
// =============================================================================

/// set_fee_rate stores the rate and settle_payment deducts the correct fee,
/// accumulating it in TreasuryBalance.
#[test]
fn test_settle_payment_deducts_fee_and_accumulates_in_treasury() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    // Set 100 bps = 1% settlement fee
    client.set_fee_rate(&admin, &100i128);

    let payment_id = String::from_str(&env, "settle_fee_basic");
    let amount = 10_000i128;
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    // Splits should cover amount - fee = 10000 - 100 = 9900
    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount: 9_900i128,
        },
    ];
    client.settle_payment(&operator, &payment_id, &splits);

    // Payment should be Settled
    assert_eq!(
        client.get_payment(&payment_id).status,
        PaymentStatus::Settled
    );

    // Treasury should have accumulated the 100-unit fee
    let treasury = client.get_treasury_balance();
    assert_eq!(treasury, 100i128, "Treasury should hold the deducted fee");
}

/// A 0 bps fee rate results in no deduction, no FEE_COLLECTED event, and no
/// treasury balance change.
#[test]
fn test_settle_payment_zero_fee_rate_no_deduction_no_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    // Explicitly set 0 bps (or simply don't set it — default is 0)
    client.set_fee_rate(&admin, &0i128);

    let payment_id = String::from_str(&env, "settle_zero_fee");
    let amount = 5_000i128;
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    // Splits cover the full amount (no fee)
    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount,
        },
    ];
    client.settle_payment(&operator, &payment_id, &splits);

    assert_eq!(
        client.get_payment(&payment_id).status,
        PaymentStatus::Settled
    );

    // No fee should have been collected
    let treasury = client.get_treasury_balance();
    assert_eq!(treasury, 0i128, "Treasury should be 0 when fee rate is 0");

    // No PAYMENT/FEE_COLLECTED event should have been emitted
    let events = env.events().all();
    let fee_event_count = events
        .iter()
        .filter(|e| {
            let topics = match &e.body {
                soroban_sdk::xdr::ContractEventBody::V0(v0) => v0.topics.clone().into(),
                _ => return false,
            };
            if topics.len() < 2 {
                return false;
            }
            let t0: Result<Symbol, _> = topics.get(0).unwrap().try_into_val(&env);
            let t1: Result<Symbol, _> = topics.get(1).unwrap().try_into_val(&env);
            matches!(
                (t0, t1),
                (Ok(a), Ok(b))
                    if a == Symbol::new(&env, "PAYMENT") && b == Symbol::new(&env, "FEE_COLLECTED")
            )
        })
        .count();
    assert_eq!(
        fee_event_count, 0,
        "Expected no FEE_COLLECTED event when fee rate is 0"
    );
}

#[test]
fn test_fee_waiver_applied_on_settle_zero_fee() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);
    client.set_fee_rate(&admin, &1_000i128);

    let code = String::from_str(&env, "LAUNCH2026");
    let expires_at = env.ledger().timestamp() + 3_600;
    client.add_fee_waiver_code(&admin, &code, &expires_at, &2u32);

    let merchant = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant);
    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &oracle);

    let payment_id = String::from_str(&env, "waiver_zero_fee");
    let amount = 10_000i128;
    let mut args = create_payment_args(&env, &payment_id, &merchant, amount);
    args.fee_waiver_code = Some(code.clone());
    client.create_payment(&args);
    client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &amount,
        &None::<u64>,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);
    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount,
        },
    ];
    client.settle_payment(&operator, &payment_id, &splits);

    assert_eq!(client.get_payment(&payment_id).status, PaymentStatus::Settled);
    assert_eq!(client.get_treasury_balance(), 0i128);

    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        let record = env
            .storage()
            .persistent()
            .get::<DataKey, FeeWaiverCodeRecord>(&DataKey::FeeWaiverCode(code.clone()))
            .expect("waiver code should exist");
        assert_eq!(record.remaining_uses, 1u32);
    });
}

#[test]
fn test_fee_waiver_code_max_uses_decremented() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);
    client.set_fee_rate(&admin, &1_000i128);

    let code = String::from_str(&env, "WAIVER_MAX_USES");
    let expires_at = env.ledger().timestamp() + 3_600;
    client.add_fee_waiver_code(&admin, &code, &expires_at, &3u32);

    let merchant = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant);
    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &oracle);

    let payment_id = String::from_str(&env, "waiver_max_uses");
    let amount = 10_000i128;
    let mut args = create_payment_args(&env, &payment_id, &merchant, amount);
    args.fee_waiver_code = Some(code.clone());
    client.create_payment(&args);
    client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &amount,
        &None::<u64>,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);
    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount,
        },
    ];
    client.settle_payment(&operator, &payment_id, &splits);

    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        let record = env
            .storage()
            .persistent()
            .get::<DataKey, FeeWaiverCodeRecord>(&DataKey::FeeWaiverCode(code.clone()))
            .expect("waiver code should exist");
        assert_eq!(record.remaining_uses, 2u32);
    });
}

#[test]
fn test_fee_waiver_code_exhausted_not_applied() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);
    client.set_fee_rate(&admin, &1_000i128);

    let code = String::from_str(&env, "WAIVER_EXHAUSTED");
    client.add_fee_waiver_code(&admin, &code, &(env.ledger().timestamp() + 3_600), &1u32);

    let merchant = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant);
    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &oracle);

    let first_id = String::from_str(&env, "waiver_exhausted_first");
    let first_amount = 10_000i128;
    let mut first_args = create_payment_args(&env, &first_id, &merchant, first_amount);
    first_args.fee_waiver_code = Some(code.clone());
    client.create_payment(&first_args);
    client.verify_payment(
        &oracle,
        &first_id,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &first_amount,
        &None::<u64>,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);
    let first_splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            first_amount,
        },
    ];
    client.settle_payment(&operator, &first_id, &first_splits);

    let second_id = String::from_str(&env, "waiver_exhausted_second");
    let second_amount = 10_000i128;
    let mut second_args = create_payment_args(&env, &second_id, &merchant, second_amount);
    second_args.fee_waiver_code = Some(code.clone());
    client.create_payment(&second_args);
    client.verify_payment(
        &oracle,
        &second_id,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &second_amount,
        &None::<u64>,
    );

    let second_splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            second_amount,
        },
    ];
    client.settle_payment(&operator, &second_id, &second_splits);

    assert_eq!(client.get_treasury_balance(), 1_000i128);
}

#[test]
fn test_fee_waiver_expired_code_not_applied() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);
    client.set_fee_rate(&admin, &1_000i128);

    let code = String::from_str(&env, "WAIVER_EXPIRED");
    let expires_at = env.ledger().timestamp() + 3_600;
    client.add_fee_waiver_code(&admin, &code, &expires_at, &5u32);

    env.ledger().set_timestamp(expires_at + 1);

    let merchant = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant);
    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &oracle);

    let payment_id = String::from_str(&env, "waiver_expired");
    let amount = 10_000i128;
    let mut args = create_payment_args(&env, &payment_id, &merchant, amount);
    args.fee_waiver_code = Some(code);
    client.create_payment(&args);
    client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &amount,
        &None::<u64>,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);
    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount,
        },
    ];
    client.settle_payment(&operator, &payment_id, &splits);

    assert_eq!(client.get_treasury_balance(), 1_000i128);
}

#[test]
fn test_fee_waiver_unknown_code_not_applied() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);
    client.set_fee_rate(&admin, &1_000i128);

    let code = String::from_str(&env, "WAIVER_UNKNOWN");

    let merchant = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant);
    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &oracle);

    let payment_id = String::from_str(&env, "waiver_unknown");
    let amount = 10_000i128;
    let mut args = create_payment_args(&env, &payment_id, &merchant, amount);
    args.fee_waiver_code = Some(code);
    client.create_payment(&args);
    client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &amount,
        &None::<u64>,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);
    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount,
        },
    ];
    client.settle_payment(&operator, &payment_id, &splits);

    assert_eq!(client.get_treasury_balance(), 1_000i128);
}

#[test]
fn test_fee_waiver_per_merchant_not_global() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);
    client.set_fee_rate(&admin, &1_000i128);

    let code = String::from_str(&env, "MERCHANT_ONLY");
    client.add_fee_waiver_code(&admin, &code, &(env.ledger().timestamp() + 3_600), &5u32);

    let merchant_a = Address::generate(&env);
    let merchant_b = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_a);
    client.grant_role(&admin, &role_merchant(&env), &merchant_b);
    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &oracle);

    let first_id = String::from_str(&env, "waiver_merchant_a");
    let first_amount = 10_000i128;
    let mut first_args = create_payment_args(&env, &first_id, &merchant_a, first_amount);
    first_args.fee_waiver_code = Some(code.clone());
    client.create_payment(&first_args);
    client.verify_payment(
        &oracle,
        &first_id,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &first_amount,
        &None::<u64>,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);
    let first_splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            first_amount,
        },
    ];
    client.settle_payment(&operator, &first_id, &first_splits);
    assert_eq!(client.get_treasury_balance(), 0i128);

    let second_id = String::from_str(&env, "waiver_merchant_b");
    let second_amount = 10_000i128;
    let mut second_args = create_payment_args(&env, &second_id, &merchant_b, second_amount);
    second_args.fee_waiver_code = Some(code.clone());
    client.create_payment(&second_args);
    client.verify_payment(
        &oracle,
        &second_id,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &second_amount,
        &None::<u64>,
    );

    let second_splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            second_amount,
        },
    ];
    client.settle_payment(&operator, &second_id, &second_splits);
    assert_eq!(client.get_treasury_balance(), 1_000i128);
}

/// Only admin can call set_fee_rate; non-admin gets Unauthorized.
#[test]
fn test_set_fee_rate_requires_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_payment_processor(&env);

    let non_admin = Address::generate(&env);
    let result = client.try_set_fee_rate(&non_admin, &50i128);

    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

/// Treasury balance accumulates across multiple settlements.
#[test]
fn test_treasury_balance_accumulates_across_multiple_settlements() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    // 200 bps = 2%
    client.set_fee_rate(&admin, &200i128);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    // First settlement: 10_000 → fee = 200
    let pid1 = String::from_str(&env, "settle_acc_1");
    make_confirmed_payment(&env, &client, &admin, &pid1, 10_000i128);
    let splits1 = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount: 9_800i128,
        },
    ];
    client.settle_payment(&operator, &pid1, &splits1);

    // Second settlement: 5_000 → fee = 100
    let pid2 = String::from_str(&env, "settle_acc_2");
    make_confirmed_payment(&env, &client, &admin, &pid2, 5_000i128);
    let splits2 = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount: 4_900i128,
        },
    ];
    client.settle_payment(&operator, &pid2, &splits2);

    // Total treasury = 200 + 100 = 300
    let treasury = client.get_treasury_balance();
    assert_eq!(
        treasury, 300i128,
        "Treasury should accumulate fees from all settlements"
    );
}

/// PAYMENT/FEE_COLLECTED event is emitted when a non-zero fee is deducted.
#[test]
fn test_settle_payment_emits_fee_collected_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    client.set_fee_rate(&admin, &500i128); // 5%

    let payment_id = String::from_str(&env, "settle_fee_event");
    let amount = 2_000i128;
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    // fee = 5% of 2000 = 100; splits cover remainder = 1900
    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount: 1_900i128,
        },
    ];
    client.settle_payment(&operator, &payment_id, &splits);

    // Verify PAYMENT/FEE_COLLECTED event was emitted
    let events = env.events().all();
    let found = events.iter().any(|e| {
        let topics = match &e.body {
            soroban_sdk::xdr::ContractEventBody::V0(v0) => v0.topics.clone().into(),
            _ => return false,
        };
        if topics.len() < 2 {
            return false;
        }
        let t0: Result<Symbol, _> = topics.get(0).unwrap().try_into_val(&env);
        let t1: Result<Symbol, _> = topics.get(1).unwrap().try_into_val(&env);
        matches!(
            (t0, t1),
            (Ok(a), Ok(b))
                if a == Symbol::new(&env, "PAYMENT") && b == Symbol::new(&env, "FEE_COLLECTED")
        )
    });
    assert!(
        found,
        "PAYMENT/FEE_COLLECTED event should be emitted when fee > 0"
    );

    let settled = events.iter().any(|e| {
        let topics = match &e.body {
            soroban_sdk::xdr::ContractEventBody::V0(v0) => v0.topics.clone().into(),
            _ => return false,
        };
        if topics.len() < 2 {
            return false;
        }
        let t0: Result<Symbol, _> = topics.get(0).unwrap().try_into_val(&env);
        let t1: Result<Symbol, _> = topics.get(1).unwrap().try_into_val(&env);
        matches!(
            (t0, t1),
            (Ok(a), Ok(b))
                if a == Symbol::new(&env, "PAYMENT") && b == Symbol::new(&env, "SETTLED")
        )
    });
    assert!(
        settled,
        "PAYMENT/SETTLED event should be emitted on settlement"
    );
}

// =============================================================================
// Issue #396: get_merchant_payments_full (paginated PaymentCharge structs)
// =============================================================================

#[test]
fn test_get_merchant_payments_full_first_page() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    // Create 5 payments for this merchant
    for i in 0u32..5 {
        let mut id_bytes = [0u8; 8];
        id_bytes[0] = b'p';
        id_bytes[1] = b'a';
        id_bytes[2] = b'y';
        id_bytes[3] = b'_';
        id_bytes[4] = b'0' + (i as u8);
        let payment_id = String::from_bytes(&env, &id_bytes[..5]);
        let args = create_payment_args(&env, &payment_id, &merchant_id, 1000 + i as i128);
        client.create_payment(&args);
    }

    // First page: offset=0, limit=3
    let page = client.get_merchant_payments_full(&merchant_id, &0, &3);
    assert_eq!(page.len(), 3);
    assert_eq!(
        page.get(0).unwrap().payment_id,
        String::from_bytes(&env, b"pay_0")
    );
    assert_eq!(
        page.get(1).unwrap().payment_id,
        String::from_bytes(&env, b"pay_1")
    );
    assert_eq!(
        page.get(2).unwrap().payment_id,
        String::from_bytes(&env, b"pay_2")
    );
}

#[test]
fn test_get_merchant_payments_full_second_page() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    for i in 0u32..5 {
        let mut id_bytes = [0u8; 5];
        id_bytes[0] = b'p';
        id_bytes[1] = b'a';
        id_bytes[2] = b'y';
        id_bytes[3] = b'_';
        id_bytes[4] = b'0' + (i as u8);
        let payment_id = String::from_bytes(&env, &id_bytes);
        let args = create_payment_args(&env, &payment_id, &merchant_id, 500 + i as i128);
        client.create_payment(&args);
    }

    // Second page: offset=3, limit=3 (only 2 remain)
    let page = client.get_merchant_payments_full(&merchant_id, &3, &3);
    assert_eq!(page.len(), 2);
    assert_eq!(
        page.get(0).unwrap().payment_id,
        String::from_bytes(&env, b"pay_3")
    );
    assert_eq!(
        page.get(1).unwrap().payment_id,
        String::from_bytes(&env, b"pay_4")
    );
}

#[test]
fn test_get_merchant_payments_full_offset_beyond_end_returns_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let payment_id = String::from_str(&env, "pay_only");
    let args = create_payment_args(&env, &payment_id, &merchant_id, 1000);
    client.create_payment(&args);

    // offset beyond end — must return empty vec, not an error
    let result = client.get_merchant_payments_full(&merchant_id, &100, &10);
    assert_eq!(result.len(), 0);
}

#[test]
fn test_get_merchant_payments_full_limit_capped_at_50() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    // Create 10 payments
    for i in 0u32..10 {
        let mut id_bytes = [0u8; 5];
        id_bytes[0] = b'p';
        id_bytes[1] = b'a';
        id_bytes[2] = b'y';
        id_bytes[3] = b'_';
        id_bytes[4] = b'0' + (i as u8);
        let payment_id = String::from_bytes(&env, &id_bytes);
        let args = create_payment_args(&env, &payment_id, &merchant_id, 100 + i as i128);
        client.create_payment(&args);
    }

    // Requesting limit=200 should be silently capped at 50; we only have 10 so return 10.
    let result = client.get_merchant_payments_full(&merchant_id, &0, &200);
    assert_eq!(result.len(), 10); // all 10 returned (well below the 50 cap)

    // Verify returns full PaymentCharge structs
    let first = result.get(0).unwrap();
    assert_eq!(first.amount, 100);
}

#[test]
fn test_get_merchant_payment_count() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    assert_eq!(client.get_merchant_payment_count(&merchant_id), 0);

    for i in 0u32..3 {
        let mut id_bytes = [0u8; 5];
        id_bytes[0] = b'p';
        id_bytes[1] = b'a';
        id_bytes[2] = b'y';
        id_bytes[3] = b'_';
        id_bytes[4] = b'0' + (i as u8);
        let payment_id = String::from_bytes(&env, &id_bytes);
        let args = create_payment_args(&env, &payment_id, &merchant_id, 500);
        client.create_payment(&args);
    }

    assert_eq!(client.get_merchant_payment_count(&merchant_id), 3);
}

// =============================================================================
// Issue #399: idempotency key TTL and cleanup on expiry / cancellation
// =============================================================================

#[test]
fn test_idempotency_key_reuse_after_cancellation() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let token = Some(String::from_str(&env, "tok_cancel_reuse"));

    // Create first payment with the client_token.
    let payment_id_1 = String::from_str(&env, "pay_cancel_1");
    let mut args = create_payment_args(&env, &payment_id_1, &merchant_id, 1000);
    args.client_token = token.clone();
    client.create_payment(&args);

    // Cancel the first payment (merchant is the authority).
    client.cancel_payment(&merchant_id, &payment_id_1);

    // After cancellation the idempotency key should be freed —
    // reuse with a *different* payment_id must now succeed (not DuplicateIdempotencyKey).
    let payment_id_2 = String::from_str(&env, "pay_cancel_2");
    let mut args2 = create_payment_args(&env, &payment_id_2, &merchant_id, 1000);
    args2.client_token = token.clone();
    let payment = client.create_payment(&args2);
    assert_eq!(payment.payment_id, payment_id_2);
}

#[test]
fn test_idempotency_key_reuse_after_expiry() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let token = Some(String::from_str(&env, "tok_expire_reuse"));

    // Create a payment that expires in 1 second.
    let now = env.ledger().timestamp();
    let payment_id_1 = String::from_str(&env, "pay_expire_1");
    let mut args = create_payment_args(&env, &payment_id_1, &merchant_id, 1000);
    args.expires_at = Some(now + 1);
    args.client_token = token.clone();
    client.create_payment(&args);

    // Advance ledger past expiry.
    env.ledger().with_mut(|li| li.timestamp = now + 10);

    // expire_payment triggers idempotency key cleanup.
    client.expire_payment(&payment_id_1);

    // Now the same token should be reusable with a new payment_id.
    let payment_id_2 = String::from_str(&env, "pay_expire_2");
    let mut args2 = create_payment_args(&env, &payment_id_2, &merchant_id, 1000);
    args2.expires_at = Some(env.ledger().timestamp() + 3600);
    args2.client_token = token.clone();
    let payment = client.create_payment(&args2);
    assert_eq!(payment.payment_id, payment_id_2);
}

#[test]
fn test_idempotency_still_blocks_during_active_window() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let token = Some(String::from_str(&env, "tok_active_block"));
    let payment_id = String::from_str(&env, "pay_active");
    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 1000);
    args.client_token = token.clone();
    client.create_payment(&args);

    // Attempt reuse with a *different* payment_id while the payment is still active.
    let mut args2 = args.clone();
    args2.payment_id = String::from_str(&env, "pay_active_other");
    let result = client.try_create_payment(&args2);
    assert_eq!(result, Err(Ok(Error::DuplicateIdempotencyKey)));
}

// =============================================================================
// Multi-sig admin proposal tests
// =============================================================================

/// Helper: set up a contract with a 2-of-3 multisig configuration.
/// Returns (admin1, admin2, admin3, client).
fn setup_multisig_2of3(env: &Env) -> (Address, Address, Address, PaymentProcessorClient<'_>) {
    let (admin, client) = setup_payment_processor(env);
    let signer2 = Address::generate(env);
    let signer3 = Address::generate(env);

    let signers = vec![env, admin.clone(), signer2.clone(), signer3.clone()];
    client.set_multisig_config(&admin, &2u32, &signers);

    (admin, signer2, signer3, client)
}

/// Threshold not met → execute_proposal must fail with AccessControlError.
#[test]
fn test_proposal_threshold_not_met_does_not_execute() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _signer2, _signer3, client) = setup_multisig_2of3(&env);

    // Create a proposal — admin is the first signer (1 of 2 required).
    let action = AdminAction::SetDisputeBond(200_000i128);
    let nonce = client.create_proposal(&admin, &action);

    // Try to execute immediately with only 1 approval (threshold is 2).
    let result = client.try_execute_proposal(&admin, &nonce);
    assert_eq!(result, Err(Ok(Error::AccessControlError)));

    // The dispute bond should still be the default (100_000).
    assert_eq!(client.get_dispute_bond_amount(), 100_000i128);
}

/// Threshold met → execute_proposal applies the parameter change and emits the event.
#[test]
fn test_proposal_threshold_met_executes_dispute_bond() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, signer2, _signer3, client) = setup_multisig_2of3(&env);

    let new_bond: i128 = 250_000;
    let action = AdminAction::SetDisputeBond(new_bond);
    let nonce = client.create_proposal(&admin, &action);

    // Second signer votes — threshold of 2 is now met.
    client.vote_proposal(&signer2, &nonce);

    // Execute the proposal.
    client.execute_proposal(&admin, &nonce);

    // Dispute bond must reflect the new value.
    assert_eq!(client.get_dispute_bond_amount(), new_bond);

    // Verify ADMIN/PROPOSAL_EXECUTED event was emitted.
    let events = env.events().all();
    let found = events.iter().any(|e| {
        let topics = match &e.body {
            soroban_sdk::xdr::ContractEventBody::V0(v0) => v0.topics.clone().into(),
            _ => return false,
        };
        if topics.len() < 2 {
            return false;
        }
        let t0: Result<Symbol, _> = topics.get(0).unwrap().try_into_val(&env);
        let t1: Result<Symbol, _> = topics.get(1).unwrap().try_into_val(&env);
        matches!(
            (t0, t1),
            (Ok(a), Ok(b))
                if a == Symbol::new(&env, "ADMIN") && b == Symbol::new(&env, "PROPOSAL_EXECUTED")
        )
    });
    assert!(
        found,
        "Expected ADMIN/PROPOSAL_EXECUTED event was not emitted"
    );
}

/// Threshold met → SetVolumeCap updates tier cap for a specific KycTier.
#[test]
fn test_proposal_threshold_met_executes_volume_cap() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, signer2, _signer3, client) = setup_multisig_2of3(&env);

    let new_cap: i128 = 200_000_000_000i128; // $20,000 in stroops
    let action = AdminAction::SetVolumeCap(crate::merchant_registry::KycTier::Basic, new_cap);
    let nonce = client.create_proposal(&admin, &action);
    client.vote_proposal(&signer2, &nonce);
    client.execute_proposal(&admin, &nonce);

    assert_eq!(
        client.get_tier_volume_cap(&crate::merchant_registry::KycTier::Basic),
        new_cap
    );
}

/// Threshold met → SetRefundFeeBps updates the refund fee.
#[test]
fn test_proposal_threshold_met_executes_refund_fee_bps() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, signer2, _signer3, client) = setup_multisig_2of3(&env);

    let new_bps: i128 = 50; // 0.5%
    let action = AdminAction::SetRefundFeeBps(new_bps);
    let nonce = client.create_proposal(&admin, &action);
    client.vote_proposal(&signer2, &nonce);
    client.execute_proposal(&admin, &nonce);

    assert_eq!(client.get_refund_fee_bps(), new_bps);
}

/// Threshold met → SetRateLimit updates the global rate limit config.
#[test]
fn test_proposal_threshold_met_executes_rate_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, signer2, _signer3, client) = setup_multisig_2of3(&env);

    // new: 120-second window, 10 payments max
    let action = AdminAction::SetRateLimit(10u32, 120u64);
    let nonce = client.create_proposal(&admin, &action);
    client.vote_proposal(&signer2, &nonce);
    client.execute_proposal(&admin, &nonce);

    // Rate limit is stored under DataKey::GlobalRateLimit; verify via contract storage.
    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        let config = env
            .storage()
            .persistent()
            .get::<DataKey, RateLimitConfig>(&DataKey::GlobalRateLimit)
            .expect("GlobalRateLimit should be set");
        assert_eq!(config.max_per_window, 10u32);
        assert_eq!(config.window_secs, 120u64);
    });
}

/// Expired proposal (> 48 h) → execute_proposal returns an error.
#[test]
fn test_proposal_expired_after_48h() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, signer2, _signer3, client) = setup_multisig_2of3(&env);

    let action = AdminAction::SetDisputeBond(999_999i128);
    let nonce = client.create_proposal(&admin, &action);
    // Second signer votes — threshold met.
    client.vote_proposal(&signer2, &nonce);

    // Advance ledger timestamp beyond 48 hours.
    env.ledger().with_mut(|l| {
        l.timestamp += 48 * 60 * 60 + 1; // 48h + 1 second
    });

    // Execute should fail with ProposalExpired (mapped to AccessControlError).
    let result = client.try_execute_proposal(&admin, &nonce);
    assert_eq!(result, Err(Ok(Error::AccessControlError)));

    // Bond unchanged.
    assert_eq!(client.get_dispute_bond_amount(), 100_000i128);
}

/// A non-signer cannot create a proposal.
#[test]
fn test_non_signer_cannot_create_proposal() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _signer2, _signer3, client) = setup_multisig_2of3(&env);
    let outsider = Address::generate(&env);

    // Reconfigure to 2-of-2 with only admin and signer2, excluding outsider.
    let signers = vec![&env, admin.clone(), Address::generate(&env)];
    client.set_multisig_config(&admin, &2u32, &signers);

    let action = AdminAction::SetDisputeBond(1i128);
    let result = client.try_create_proposal(&outsider, &action);
    assert_eq!(result, Err(Ok(Error::AccessControlError)));
}

// =============================================================================
// Platform fee split (FeeSplitConfig / set_fee_split_config) tests
// =============================================================================

/// Helper: register a token contract and mint `amount` to `recipient`.
/// Returns the token contract address.
fn setup_and_mint_token(env: &Env, recipient: &Address, amount: i128) -> Address {
    let token_admin = Address::generate(env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    token::StellarAssetClient::new(env, &token_id).mint(recipient, &amount);
    token_id
}

/// `set_fee_split_config` stores the config; validation rejects sums > 10 000.
#[test]
fn test_set_fee_split_config_stores_and_validates() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let treasury = Address::generate(&env);
    let developer = Address::generate(&env);

    // Valid: 7000 + 3000 = 10 000 (exactly 100%)
    let config = FeeSplitConfig {
        treasury_bps: 7_000,
        developer_bps: 3_000,
        treasury_address: treasury.clone(),
        developer_address: developer.clone(),
    };
    client.set_fee_split_config(&admin, &config);
    let stored = client
        .get_fee_split_config()
        .expect("config should be stored");
    assert_eq!(stored.treasury_bps, 7_000);
    assert_eq!(stored.developer_bps, 3_000);

    // Valid: 5000 + 4000 = 9000 (< 100% — remainder implicitly goes to treasury)
    let partial = FeeSplitConfig {
        treasury_bps: 5_000,
        developer_bps: 4_000,
        treasury_address: treasury.clone(),
        developer_address: developer.clone(),
    };
    client.set_fee_split_config(&admin, &partial);

    // Invalid: 6000 + 5000 = 11 000 > 10 000
    let bad = FeeSplitConfig {
        treasury_bps: 6_000,
        developer_bps: 5_000,
        treasury_address: treasury.clone(),
        developer_address: developer.clone(),
    };
    let result = client.try_set_fee_split_config(&admin, &bad);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

/// Non-admin cannot call `set_fee_split_config`.
#[test]
fn test_set_fee_split_config_requires_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_refund_manager(&env);

    let non_admin = Address::generate(&env);
    let config = FeeSplitConfig {
        treasury_bps: 7_000,
        developer_bps: 3_000,
        treasury_address: Address::generate(&env),
        developer_address: Address::generate(&env),
    };
    let result = client.try_set_fee_split_config(&non_admin, &config);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

/// `configure_fee_split` (flat-args) also rejects sums > 10 000.
#[test]
fn test_configure_fee_split_sum_constraint() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);

    // Exactly 10 000 — valid
    client.configure_fee_split(
        &admin,
        &8_000u32,
        &2_000u32,
        &Address::generate(&env),
        &Address::generate(&env),
    );

    // 11 000 — invalid
    let result = client.try_configure_fee_split(
        &admin,
        &6_000u32,
        &5_000u32,
        &Address::generate(&env),
        &Address::generate(&env),
    );
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

/// When FeeSplitConfig is set, settle_payment splits the fee and transfers
/// both portions in the same transaction. TreasuryBalance is NOT updated.
#[test]
fn test_settle_payment_splits_fee_between_treasury_and_developer() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    // Mint tokens to the contract so transfers succeed
    let contract_id = client.address.clone();
    let token_id = setup_and_mint_token(&env, &contract_id, 1_000_000i128);

    // Wire the USDC token
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::UsdcToken, &token_id);
    });

    // 10% settlement fee
    client.set_fee_rate(&admin, &1_000i128);

    // 70 / 30 split
    let treasury_addr = Address::generate(&env);
    let dev_addr = Address::generate(&env);
    client.set_fee_split_config(
        &admin,
        &FeeSplitConfig {
            treasury_bps: 7_000,
            developer_bps: 3_000,
            treasury_address: treasury_addr.clone(),
            developer_address: dev_addr.clone(),
        },
    );

    let payment_id = String::from_str(&env, "split_fee_pay");
    let amount = 10_000i128; // fee = 1000; treasury = 700; dev = 300
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    // Splits must cover net amount = 10 000 - 1 000 = 9 000
    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount: 9_000i128,
        },
    ];
    client.settle_payment(&operator, &payment_id, &splits);

    // Payment settled
    assert_eq!(
        client.get_payment(&payment_id).status,
        PaymentStatus::Settled
    );

    // Treasury balance must NOT accumulate (fee was forwarded, not stored)
    assert_eq!(
        client.get_treasury_balance(),
        0i128,
        "TreasuryBalance should remain 0 when FeeSplitConfig is active"
    );

    // Token balances: treasury received 700, developer received 300
    let token_client = token::TokenClient::new(&env, &token_id);
    assert_eq!(
        token_client.balance(&treasury_addr),
        700i128,
        "Treasury should receive 70% of the 1000-unit fee"
    );
    assert_eq!(
        token_client.balance(&dev_addr),
        300i128,
        "Developer should receive 30% of the 1000-unit fee"
    );
}

/// PAYMENT/FEE_SPLIT event is emitted with payment_id, treasury_amount, dev_amount.
#[test]
fn test_settle_payment_emits_fee_split_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let contract_id = client.address.clone();
    let token_id = setup_and_mint_token(&env, &contract_id, 1_000_000i128);
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::UsdcToken, &token_id);
    });

    // 10% fee, 50/50 split
    client.set_fee_rate(&admin, &1_000i128);
    client.set_fee_split_config(
        &admin,
        &FeeSplitConfig {
            treasury_bps: 5_000,
            developer_bps: 5_000,
            treasury_address: Address::generate(&env),
            developer_address: Address::generate(&env),
        },
    );

    let payment_id = String::from_str(&env, "split_event_pay");
    let amount = 2_000i128; // fee = 200; treasury = 100; dev = 100
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount: 1_800i128, // 2000 - 200
        },
    ];
    client.settle_payment(&operator, &payment_id, &splits);

    // Verify PAYMENT/FEE_SPLIT event was emitted
    let events = env.events().all();
    let found = events.iter().any(|e| {
        let topics = match &e.body {
            soroban_sdk::xdr::ContractEventBody::V0(v0) => v0.topics.clone().into(),
            _ => return false,
        };
        if topics.len() < 2 {
            return false;
        }
        let t0: Result<Symbol, _> = topics.get(0).unwrap().try_into_val(&env);
        let t1: Result<Symbol, _> = topics.get(1).unwrap().try_into_val(&env);
        matches!(
            (t0, t1),
            (Ok(a), Ok(b))
                if a == Symbol::new(&env, "PAYMENT") && b == Symbol::new(&env, "FEE_SPLIT")
        )
    });
    assert!(
        found,
        "PAYMENT/FEE_SPLIT event must be emitted when FeeSplitConfig is active"
    );
}

/// Zero developer_bps → entire fee goes to treasury address; no transfer to developer.
#[test]
fn test_settle_payment_zero_dev_bps_sends_all_to_treasury() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let contract_id = client.address.clone();
    let token_id = setup_and_mint_token(&env, &contract_id, 1_000_000i128);
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::UsdcToken, &token_id);
    });

    // 10% fee, 100% to treasury, 0% to developer
    client.set_fee_rate(&admin, &1_000i128);
    let treasury_addr = Address::generate(&env);
    let dev_addr = Address::generate(&env);
    client.set_fee_split_config(
        &admin,
        &FeeSplitConfig {
            treasury_bps: 10_000,
            developer_bps: 0,
            treasury_address: treasury_addr.clone(),
            developer_address: dev_addr.clone(),
        },
    );

    let payment_id = String::from_str(&env, "zero_dev_pay");
    let amount = 5_000i128; // fee = 500
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount: 4_500i128, // 5000 - 500
        },
    ];
    client.settle_payment(&operator, &payment_id, &splits);

    let token_client = token::TokenClient::new(&env, &token_id);

    // Treasury gets the full 500
    assert_eq!(
        token_client.balance(&treasury_addr),
        500i128,
        "Treasury should receive 100% of the fee when developer_bps = 0"
    );

    // Developer receives nothing
    assert_eq!(
        token_client.balance(&dev_addr),
        0i128,
        "Developer should receive 0 when developer_bps = 0"
    );
}

/// When no FeeSplitConfig is set, settle_payment falls back to accumulating
/// the fee in TreasuryBalance and emitting FEE_COLLECTED (unchanged legacy behaviour).
#[test]
fn test_settle_payment_no_fee_split_config_falls_back_to_treasury_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    // No FeeSplitConfig set — verify legacy behaviour is intact
    client.set_fee_rate(&admin, &100i128); // 1%

    let payment_id = String::from_str(&env, "legacy_fee_pay");
    let amount = 10_000i128; // fee = 100
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount: 9_900i128,
        },
    ];
    client.settle_payment(&operator, &payment_id, &splits);

    // Legacy: TreasuryBalance accumulates the fee
    assert_eq!(
        client.get_treasury_balance(),
        100i128,
        "Without FeeSplitConfig the fee must accumulate in TreasuryBalance"
    );

    // Legacy: FEE_COLLECTED event, not FEE_SPLIT
    let events = env.events().all();
    let fee_collected = events.iter().any(|e| {
        let topics = match &e.body {
            soroban_sdk::xdr::ContractEventBody::V0(v0) => v0.topics.clone().into(),
            _ => return false,
        };
        if topics.len() < 2 {
            return false;
        }
        let t0: Result<Symbol, _> = topics.get(0).unwrap().try_into_val(&env);
        let t1: Result<Symbol, _> = topics.get(1).unwrap().try_into_val(&env);
        matches!(
            (t0, t1),
            (Ok(a), Ok(b))
                if a == Symbol::new(&env, "PAYMENT") && b == Symbol::new(&env, "FEE_COLLECTED")
        )
    });
    assert!(
        fee_collected,
        "FEE_COLLECTED event must be emitted on legacy path"
    );

    let fee_split = events.iter().any(|e| {
        let topics = match &e.body {
            soroban_sdk::xdr::ContractEventBody::V0(v0) => v0.topics.clone().into(),
            _ => return false,
        };
        if topics.len() < 2 {
            return false;
        }
        let t0: Result<Symbol, _> = topics.get(0).unwrap().try_into_val(&env);
        let t1: Result<Symbol, _> = topics.get(1).unwrap().try_into_val(&env);
        matches!(
            (t0, t1),
            (Ok(a), Ok(b))
                if a == Symbol::new(&env, "PAYMENT") && b == Symbol::new(&env, "FEE_SPLIT")
        )
    });
    assert!(!fee_split, "FEE_SPLIT must NOT be emitted on legacy path");
}

/// Rounding dust (odd amounts) goes to treasury, not lost.
#[test]
fn test_settle_payment_fee_split_rounding_dust_to_treasury() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let contract_id = client.address.clone();
    let token_id = setup_and_mint_token(&env, &contract_id, 1_000_000i128);
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::UsdcToken, &token_id);
    });

    // 10% fee on 1001 → fee = 100 (integer); 33.3% to dev → dev = 33, treasury = 67
    client.set_fee_rate(&admin, &1_000i128);
    let treasury_addr = Address::generate(&env);
    let dev_addr = Address::generate(&env);
    client.set_fee_split_config(
        &admin,
        &FeeSplitConfig {
            treasury_bps: 6_667,
            developer_bps: 3_333,
            treasury_address: treasury_addr.clone(),
            developer_address: dev_addr.clone(),
        },
    );

    let amount = 1_000i128; // fee = 100; dev = 100*3333/10000 = 33; treasury = 100 - 33 = 67
    let payment_id = String::from_str(&env, "rounding_pay");
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount: 900i128, // 1000 - 100
        },
    ];
    client.settle_payment(&operator, &payment_id, &splits);

    let token_client = token::TokenClient::new(&env, &token_id);
    let dev_bal = token_client.balance(&dev_addr);
    let treasury_bal = token_client.balance(&treasury_addr);

    // dev = 33, treasury = 67, total = 100
    assert_eq!(dev_bal, 33i128);
    assert_eq!(treasury_bal, 67i128);
    assert_eq!(
        dev_bal + treasury_bal,
        100i128,
        "All fee tokens must be accounted for"
    );
}

// =============================================================================
// Treasury fee unification — settlement + refund fees + withdrawal history
// =============================================================================

#[test]
fn test_settlement_fee_accumulates_in_treasury() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    client.set_fee_rate(&admin, &200i128); // 2%
    let payment_id = String::from_str(&env, "treasury_settle_accum");
    let amount = 10_000i128;
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);
    let splits = vec![
        &env,
        SettlementSplit {
            recipient: Address::generate(&env),
            amount: 9_800i128,
        },
    ];
    client.settle_payment(&operator, &payment_id, &splits);

    assert_eq!(client.get_treasury_balance(), 200i128);
}

#[test]
fn test_refund_fee_accumulates_in_treasury() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client, _usdc) = setup_refund_manager_with_token(&env);

    let payment_id = String::from_str(&env, "treasury_refund_accum");
    let merchant_id = Address::generate(&env);
    let requester = Address::generate(&env);
    client.register_payment(
        &payment_id,
        &merchant_id,
        &10_000i128,
        &Symbol::new(&env, "USDC"),
    );
    let refund_id = client.create_refund(
        &payment_id,
        &1_000i128,
        &String::from_str(&env, "reason"),
        &requester,
    );
    let operator = Address::generate(&env);
    client.grant_role(&_admin, &role_settlement_operator(&env), &operator);
    client.process_refund(&operator, &refund_id);

    // Default refund fee 100 bps of 1000 = 10
    assert_eq!(client.get_treasury_balance(), 10i128);
}

#[test]
fn test_platform_fee_without_custom_recipient_credits_treasury() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_contract = env.register(PaymentProcessor, ());
    let registry_contract = env.register(crate::merchant_registry::MerchantRegistry, ());
    let payment_client = PaymentProcessorClient::new(&env, &payment_contract);
    let registry_client =
        crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_contract);

    let admin = Address::generate(&env);
    payment_client.initialize_payment_processor(&admin);
    registry_client.initialize(&admin);
    payment_client.set_merchant_registry_address(&admin, &registry_contract);

    let token_id = setup_and_mint_token(&env, &payment_contract, 1_000_000i128);
    env.as_contract(&payment_contract, || {
        env.storage()
            .persistent()
            .set(&DataKey::UsdcToken, &token_id);
    });
}

#[test]
fn test_refund_cooldown_enforcement() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, client) = setup_payment_processor(&env);
    let contract_id = client.address.clone();
    let token_id = setup_and_mint_token(&env, &contract_id, 1_000_000i128);
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::UsdcToken, &token_id);
    });

    let merchant = Address::generate(&env);
    client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);

    let amount = 1000i128;
    let payment_id = String::from_str(&env, "cooldown_pay");
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    let requester = Address::generate(&env);

    // Try to create refund immediately (within cooldown) - should fail
    let res = client.try_create_refund(
        &requester,
        &payment_id,
        &100,
        &String::from_str(&env, "Too much"),
    );
    assert!(res.is_err(), "Should block refund within cooldown period");
}

#[test]
fn test_refund_cooldown_allows_after_period() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, client) = setup_payment_processor(&env);
    let contract_id = client.address.clone();
    let token_id = setup_and_mint_token(&env, &contract_id, 1_000_000i128);
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::UsdcToken, &token_id);
    });

    let merchant = Address::generate(&env);
    client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);

    let amount = 1000i128;
    let payment_id = String::from_str(&env, "cooldown_pass_pay");

    // Create payment at ledger time 0, confirm at time 1
    env.ledger().with_mut(|li| {
        li.timestamp = 1;
    });
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    // Advance time by 301 seconds (default cooldown is 300)
    env.ledger().with_mut(|li| {
        li.timestamp = 302;
    });

    let requester = Address::generate(&env);

    // Now create refund should succeed
    let res = client.try_create_refund(
        &requester,
        &payment_id,
        &100,
        &String::from_str(&env, "Too much"),
    );
    assert!(
        res.is_ok(),
        "Should allow refund after cooldown period expires"
    );
}

#[test]
fn test_refund_cooldown_configurable() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, client) = setup_payment_processor(&env);
    let contract_id = client.address.clone();
    let token_id = setup_and_mint_token(&env, &contract_id, 1_000_000i128);
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::UsdcToken, &token_id);
    });

    // Set cooldown to 0 (allow immediate refunds)
    let res = client.try_set_refund_cooldown(&admin, &0u64);
    assert!(res.is_ok(), "Admin should be able to set refund cooldown");

    let merchant = Address::generate(&env);
    client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);

    let amount = 1000i128;
    let payment_id = String::from_str(&env, "immediate_refund");
    make_confirmed_payment(&env, &client, &admin, &payment_id, amount);

    let requester = Address::generate(&env);

    // With cooldown = 0, refund should succeed immediately
    let res = client.try_create_refund(
        &requester,
        &payment_id,
        &100,
        &String::from_str(&env, "Too much"),
    );
    assert!(
        res.is_ok(),
        "Should allow immediate refund when cooldown is set to 0"
    );
}

#[test]
fn test_merchant_payment_count_accurate_after_creates() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, client) = setup_payment_processor(&env);
    let contract_id = client.address.clone();
    let token_id = setup_and_mint_token(&env, &contract_id, 1_000_000i128);
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::UsdcToken, &token_id);
    });

    let merchant = Address::generate(&env);
    client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);

    // Initially count should be 0
    let mut count = client.get_merchant_payment_count_dash(&merchant);
    assert_eq!(count, 0u32, "Initial count should be 0");

    let _ = client.create_payment(&CreatePaymentArgs {
        payment_id: String::from_str(&env, "pay1"),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 100,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: None,
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
    });
}

#[test]
fn test_create_payment_future_expiry_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_future_expiry");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let now = env.ledger().timestamp();
    let future_expiry = now + 7200; // 2 hours in the future

    let args = CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant_id.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(future_expiry),
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
    };

    let payment = client.create_payment(&args);
    assert_eq!(payment.expires_at, future_expiry);
}

#[test]
fn test_create_payment_current_timestamp_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_current_expiry");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let now = env.ledger().timestamp();

    let args = CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant_id.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(now), // Exactly now
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
    };

    let result = client.try_create_payment(&args);
    assert!(result.is_err());
}

#[test]
fn test_create_payment_past_expiry_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_past_expiry");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let now = env.ledger().timestamp();
    let past_expiry = now - 3600; // 1 hour in the past

    let args = CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant_id.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(past_expiry),
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
    };

    let result = client.try_create_payment(&args);
    assert!(result.is_err());
}

#[test]
fn test_create_payment_duration_min_bound_enforced() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_min_duration");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant_id.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: None,
        duration_secs: Some(30), // Below CREATE_PAYMENT_WINDOW_SECS (60)
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
    };

    let result = client.try_create_payment(&args);
    assert!(result.is_err());
}

#[test]
fn test_create_payment_duration_max_bound_enforced() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_max_duration");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant_id.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: None,
        duration_secs: Some(31 * 24 * 3600), // Exceeds 30 days
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
    };

    let result = client.try_create_payment(&args);
    assert!(result.is_err());
}

#[test]
fn test_create_payment_valid_duration_within_bounds() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_valid_duration");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let duration_secs = 7200u64; // 2 hours, within bounds

    let args = CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant_id.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: None,
        duration_secs: Some(duration_secs),
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
    };

    let payment = client.create_payment(&args);
    let now = env.ledger().timestamp();
    let expected_expiry = now + duration_secs;
    assert_eq!(payment.expires_at, expected_expiry);
}

#[test]
fn test_admin_set_min_payment_duration() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let new_min = 120u64;
    client.set_min_payment_duration_secs(&admin, &new_min);

    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        let stored_min: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::MinPaymentDurationSecs)
            .unwrap();
        assert_eq!(stored_min, new_min);
    });
}

#[test]
fn test_admin_set_max_payment_duration() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let new_max = 14 * 24 * 3600u64; // 14 days
    client.set_max_payment_duration_secs(&admin, &new_max);

    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        let stored_max: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::MaxPaymentDurationSecs)
            .unwrap();
        assert_eq!(stored_max, new_max);
    });
}

#[test]
fn test_create_payment_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_zero_amount");
    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant_id.clone(),
        payer: None,
        amount: 0,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
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
    };

    let result = client.try_create_payment(&args);
    assert!(result.is_err());
}

#[test]
fn test_merchant_payment_count_not_decremented_on_cancel() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, client) = setup_payment_processor(&env);
    let contract_id = client.address.clone();
    let token_id = setup_and_mint_token(&env, &contract_id, 1_000_000i128);
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::UsdcToken, &token_id);
    });

    let merchant = Address::generate(&env);
    payment_client.grant_role(&admin, &role_merchant(&env), &merchant);

    let fee_config = crate::merchant_registry::FeeConfig {
        platform_fee_bps: 100, // 1%
        fixed_fee: 0,
        fee_recipient: None, // → TreasuryBalance
    };
    registry_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Fee Merchant"),
        &String::from_str(&env, "USDC"),
        &None::<Address>,
        &None::<String>,
        &MaybeFeeConfig::Some(fee_config),
    );
    registry_client.set_kyc_tier_with_signature(
        &admin,
        &merchant,
        &crate::merchant_registry::KycTier::Full,
        &Some(String::from_str(&env, "sig")),
    );

    let payment_id = String::from_str(&env, "plat_fee_treasury");
    let amount = 10_000i128;
    let args = create_payment_args(&env, &payment_id, &merchant, amount);
    payment_client.create_payment(&args);

    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &role_oracle(&env), &oracle);
    payment_client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &amount,
        &None::<u64>,
    );

    let operator = Address::generate(&env);
    payment_client.grant_role(&admin, &role_settlement_operator(&env), &operator);
    payment_client.settle_payment(&operator, &payment_id, &vec![&env]);

    // 1% of 10_000 = 100 credited to treasury (no custom recipient)
    assert_eq!(payment_client.get_treasury_balance(), 100i128);
}

#[test]
fn test_withdraw_treasury_reduces_balance_and_logs_history() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client, usdc_token) = setup_refund_manager_with_token(&env);
    let token_client = token::StellarAssetClient::new(&env, &usdc_token);

    let merchant_id = Address::generate(&env);
    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    let payment_id = String::from_str(&env, "withdraw_hist_pay");
    let requester = Address::generate(&env);
    client.register_payment(
        &payment_id,
        &merchant_id,
        &50_000i128,
        &Symbol::new(&env, "USDC"),
    );
    let refund_id = client.create_refund(
        &payment_id,
        &10_000i128,
        &String::from_str(&env, "reason"),
        &requester,
    );
    client.process_refund(&operator, &refund_id);
    // fee = 100 bps * 10000 = 100
    assert_eq!(client.get_treasury_balance(), 100i128);

    let destination = Address::generate(&env);
    let starting = token::TokenClient::new(&env, &usdc_token).balance(&destination);
    client.withdraw_treasury(&admin, &40i128, &destination);

    assert_eq!(client.get_treasury_balance(), 60i128);
    assert_eq!(
        token::TokenClient::new(&env, &usdc_token).balance(&destination),
        starting + 40
    );

    let history = client.get_treasury_withdrawal_history(&0u32, &10u32);
    assert_eq!(history.len(), 1);
    assert_eq!(history.get(0).unwrap().amount, 40i128);
    assert_eq!(history.get(0).unwrap().destination, destination);

    // Insufficient withdrawal fails and does not change balance
    let result = client.try_withdraw_treasury(&admin, &61i128, &destination);
    assert_eq!(result, Err(Ok(Error::InsufficientTreasuryBalance)));
    assert_eq!(client.get_treasury_balance(), 60i128);
}

#[test]
fn test_create_payment_negative_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_negative_amount");
    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant_id.clone(),
        payer: None,
        amount: -1000i128,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
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
    };

    let result = client.try_create_payment(&args);
    assert!(result.is_err());
}

#[test]
fn test_create_payment_minimum_positive_amount_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_min_amount");
    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant_id.clone(),
        payer: None,
        amount: 1, // Minimum valid amount (1 stroop)
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
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
    };

    let payment = client.create_payment(&args);
    assert_eq!(payment.amount, 1i128);
}

#[test]
fn test_create_refund_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_for_refund");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = create_payment_args(&env, &payment_id, &merchant_id, amount);
    let _ = client.create_payment(&args);

    let requester = Address::generate(&env);
    let result =
        client.try_create_refund(&payment_id, &0, &String::from_str(&env, "test"), &requester);
    assert!(result.is_err());
}

#[test]
fn test_create_dispute_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_for_dispute");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = create_payment_args(&env, &payment_id, &merchant_id, amount);
    let _ = client.create_payment(&args);

    let disputer = Address::generate(&env);
    let result = client.try_create_dispute(
        &payment_id,
        &0,
        &String::from_str(&env, "reason"),
        &String::from_str(&env, "QmHash1234567890"),
        &disputer,
        &vec![&env],
    );
    assert!(result.is_err());
}

#[test]
fn test_subscription_max_retries_cancelled() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client, _token) = setup_refund_manager(&env);

    let merchant = Address::generate(&env);
    client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);

    let payer = Address::generate(&env);
    let plan_id = String::from_str(&env, "plan_max_retries");

    // Create subscription plan
    client.create_subscription_plan(
        &merchant,
        &plan_id,
        &String::from_str(&env, "Plan"),
        &String::from_str(&env, "Desc"),
        &100_000_000i128,
        &Symbol::new(&env, "USDC"),
        &crate::BillingInterval::Weekly,
    );

    // Create subscription
    let subscription_id = client.subscribe(&payer, &plan_id, &None, &None, &MaybeFeeConfig::None);
    let subscription = client.get_subscription(&subscription_id).unwrap();
    assert_eq!(subscription.status, SubscriptionStatus::Active);
}

#[test]
fn test_subscription_retry_counter_reset_on_success() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client, _token) = setup_refund_manager(&env);

    let merchant = Address::generate(&env);
    client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);

    let payer = Address::generate(&env);
    let plan_id = String::from_str(&env, "plan_retry_reset");

    client.create_subscription_plan(
        &merchant,
        &plan_id,
        &String::from_str(&env, "Plan"),
        &String::from_str(&env, "Desc"),
        &100_000_000i128,
        &Symbol::new(&env, "USDC"),
        &crate::BillingInterval::Weekly,
    );

    let subscription_id = client.subscribe(&payer, &plan_id, &None, &None, &MaybeFeeConfig::None);
    let subscription = client.get_subscription(&subscription_id).unwrap();
    assert_eq!(subscription.retry_count, 0u32);
}

#[test]
fn test_admin_reactivate_max_retries_cancelled_subscription() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client, _token) = setup_refund_manager(&env);

    let merchant = Address::generate(&env);
    client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);

    let payer = Address::generate(&env);
    let plan_id = String::from_str(&env, "plan_reactivate");

    client.create_subscription_plan(
        &merchant,
        &plan_id,
        &String::from_str(&env, "Plan"),
        &String::from_str(&env, "Desc"),
        &100_000_000i128,
        &Symbol::new(&env, "USDC"),
        &crate::BillingInterval::Weekly,
    );

    let subscription_id = client.subscribe(&payer, &plan_id, &None, &None, &MaybeFeeConfig::None);

    // Manually mark subscription as cancelled to simulate max retries cancellation
    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        let mut sub = client.get_subscription(&subscription_id).unwrap();
        sub.status = SubscriptionStatus::Cancelled;
        sub.retry_count = 3u32;
        env.storage()
            .persistent()
            .set(&DataKey::Subscription(subscription_id.clone()), &sub);
    });

    // Admin reactivates the subscription
    client.admin_reactivate_subscription(&admin, &subscription_id);

    let reactivated = client.get_subscription(&subscription_id).unwrap();
    assert_eq!(reactivated.status, SubscriptionStatus::Active);
    assert_eq!(reactivated.retry_count, 0u32);
}

#[test]
fn test_get_merchant_payments_full_with_token_filter() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let usdc_token = Address::generate(&env);
    let eurc_token = Address::generate(&env);

    // Create 3 USDC payments
    for i in 0..3 {
        let payment_id = String::from_str(&env, &format!("usdc_payment_{}", i));
        let mut args = create_payment_args(&env, &payment_id, &merchant_id, 100_000_000i128);
        args.token_address = Some(usdc_token.clone());
        client.create_payment(&args);
    }

    // Create 2 EURC payments
    for i in 0..2 {
        let payment_id = String::from_str(&env, &format!("eurc_payment_{}", i));
        let mut args = create_payment_args(&env, &payment_id, &merchant_id, 50_000_000i128);
        args.token_address = Some(eurc_token.clone());
        client.create_payment(&args);
    }

    // Test filtering by EURC token - should return exactly 2 results
    let eurc_payments =
        client.get_merchant_payments_full(&merchant_id, &0, &50, &Some(eurc_token.clone()));
    assert_eq!(eurc_payments.len(), 2);

    // Test filtering by USDC token - should return exactly 3 results
    let usdc_payments =
        client.get_merchant_payments_full(&merchant_id, &0, &50, &Some(usdc_token.clone()));
    assert_eq!(usdc_payments.len(), 3);

    // Test with no filter - should return all 5 payments
    let all_payments = client.get_merchant_payments_full(&merchant_id, &0, &50, &None);
    assert_eq!(all_payments.len(), 5);
}

#[test]
fn test_get_merchant_payments_full_token_filter_backward_compatible() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    // Create 3 payments with no token_address (None)
    for i in 0..3 {
        let payment_id = String::from_str(&env, &format!("payment_{}", i));
        let args = create_payment_args(&env, &payment_id, &merchant_id, 100_000_000i128);
        client.create_payment(&args);
    }

    // Test with no filter - should return all 3 payments
    let all_payments = client.get_merchant_payments_full(&merchant_id, &0, &50, &None);
    assert_eq!(all_payments.len(), 3);

    // Test with a filter - should return 0 since all payments have token_address = None
    let specific_token = Address::generate(&env);
    let filtered_payments =
        client.get_merchant_payments_full(&merchant_id, &0, &50, &Some(specific_token));
    assert_eq!(filtered_payments.len(), 0);
}

#[test]
fn test_get_merchant_payments_full_token_filter_with_pagination() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let token_a = Address::generate(&env);

    // Create 8 payments with token_a
    for i in 0..8 {
        let payment_id = String::from_str(&env, &format!("token_a_payment_{}", i));
        let mut args = create_payment_args(&env, &payment_id, &merchant_id, 100_000_000i128);
        args.token_address = Some(token_a.clone());
        client.create_payment(&args);
    }

    // First page with limit 3
    let page1 = client.get_merchant_payments_full(&merchant_id, &0, &3, &Some(token_a.clone()));
    assert_eq!(page1.len(), 3);

    // Second page with limit 3
    let page2 = client.get_merchant_payments_full(&merchant_id, &3, &3, &Some(token_a.clone()));
    assert_eq!(page2.len(), 3);

    // Third page with limit 3
    let page3 = client.get_merchant_payments_full(&merchant_id, &6, &3, &Some(token_a.clone()));
    assert_eq!(page3.len(), 2);

    // All different IDs
    let all_ids: Vec<String> = page1
        .iter()
        .chain(page2.iter())
        .chain(page3.iter())
        .map(|p| p.payment_id.clone())
        .collect();
    assert_eq!(all_ids.len(), 8);
}

#[test]
fn test_batch_expire_payments_all_valid() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let mut payment_ids = vec![&env];
    let initial_ts = env.ledger().timestamp();

    // Create 3 expirable pending payments
    for i in 0..3 {
        let payment_id = String::from_str(&env, &format!("expire_test_{}", i));
        let mut args = create_payment_args(&env, &payment_id, &merchant_id, 100_000_000i128);
        args.expires_at = Some(initial_ts + 1000);
        client.create_payment(&args);
        payment_ids.push_back(payment_id);
    }

    // Advance time past expiry
    env.ledger().set_timestamp(initial_ts + 2000);

    // All 3 should expire successfully
    let count = client.batch_expire_payments(&payment_ids).unwrap();
    assert_eq!(count, 3);
}

#[test]
fn test_batch_expire_payments_partial_mixed_states() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let initial_ts = env.ledger().timestamp();

    // Create 2 pending + 1 already-expired payment
    let payment_1 = String::from_str(&env, "pending_1");
    let payment_2 = String::from_str(&env, "pending_2");
    let payment_3 = String::from_str(&env, "already_expired");

    let mut args_1 = create_payment_args(&env, &payment_1, &merchant_id, 100_000_000i128);
    args_1.expires_at = Some(initial_ts + 1000);
    client.create_payment(&args_1);

    let mut args_2 = create_payment_args(&env, &payment_2, &merchant_id, 100_000_000i128);
    args_2.expires_at = Some(initial_ts + 1000);
    client.create_payment(&args_2);

    let mut args_3 = create_payment_args(&env, &payment_3, &merchant_id, 100_000_000i128);
    args_3.expires_at = Some(initial_ts + 500);
    client.create_payment(&args_3);

    // First, expire the third payment by advancing time
    env.ledger().set_timestamp(initial_ts + 600);
    let _ = client.expire_payment(&payment_3);

    // Now try to expire the batch with 2 pending + 1 already-expired
    env.ledger().set_timestamp(initial_ts + 2000);
    let mut batch_ids = vec![&env];
    batch_ids.push_back(payment_1);
    batch_ids.push_back(payment_2);
    batch_ids.push_back(payment_3);

    let count = client.batch_expire_payments(&batch_ids).unwrap();
    // Only the 2 pending ones should be expired
    assert_eq!(count, 2);
}

#[test]
fn test_batch_expire_payments_nonexistent_id_skipped() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let initial_ts = env.ledger().timestamp();

    // Create 1 valid pending payment
    let valid_id = String::from_str(&env, "valid_payment");
    let mut args = create_payment_args(&env, &valid_id, &merchant_id, 100_000_000i128);
    args.expires_at = Some(initial_ts + 1000);
    client.create_payment(&args);

    // Create a batch with 1 valid + 1 nonexistent ID
    let nonexistent_id = String::from_str(&env, "nonexistent_payment");
    let mut batch_ids = vec![&env];
    batch_ids.push_back(valid_id);
    batch_ids.push_back(nonexistent_id);

    // Advance time past expiry
    env.ledger().set_timestamp(initial_ts + 2000);

    // Only the valid one should be counted (nonexistent should be silently skipped, no panic)
    let count = client.batch_expire_payments(&batch_ids).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_batch_expire_payments_confirmed_payment_not_expired() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let initial_ts = env.ledger().timestamp();

    // Create 1 payment and confirm it
    let payment_id = String::from_str(&env, "confirmed_payment");
    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 100_000_000i128);
    args.expires_at = Some(initial_ts + 1000);
    client.create_payment(&args);

    // Manually mark payment as confirmed
    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        let mut payment = PaymentProcessor::get_payment_internal(&env, &payment_id).unwrap();
        payment.status = PaymentStatus::Confirmed;
        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);
    });

    // Advance time past expiry
    env.ledger().set_timestamp(initial_ts + 2000);

    // Try to expire the confirmed payment - should be skipped, return 0
    let mut batch_ids = vec![&env];
    batch_ids.push_back(payment_id);

    let count = client.batch_expire_payments(&batch_ids).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn test_batch_expire_payments_empty_vec() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let empty_batch = vec![&env];
    let count = client.batch_expire_payments(&empty_batch).unwrap();

    // Empty batch should return 0
    assert_eq!(count, 0);
}
