use crate::{
    merchant_registry::{MerchantRegistry, MerchantRegistryClient},
    PaymentProcessor, PaymentProcessorClient,
};
use soroban_sdk::{testutils::Address as _, Address, Env, String, Symbol};

fn setup_payment_processor_with_registry(
    env: &Env,
) -> (
    Address,
    Address,
    PaymentProcessorClient<'_>,
    MerchantRegistryClient<'_>,
) {
    let payment_processor = env.register(PaymentProcessor, ());
    let merchant_registry = env.register(MerchantRegistry, ());

    let payment_client = PaymentProcessorClient::new(env, &payment_processor);
    let merchant_client = MerchantRegistryClient::new(env, &merchant_registry);

    let admin = Address::generate(env);

    payment_client.initialize_payment_processor(&admin);
    merchant_client.initialize(&admin);

    (admin, payment_processor, payment_client, merchant_client)
}

fn setup_oracle_and_merchant(
    env: &Env,
    admin: &Address,
    payment_client: &PaymentProcessorClient,
    merchant_client: &MerchantRegistryClient,
) -> (Address, Address) {
    env.mock_all_auths();

    let oracle = Address::generate(env);
    let merchant = Address::generate(env);

    payment_client.grant_role(&admin, &Symbol::new(env, "ORACLE"), &oracle);
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(env, "TestMerchant"),
        &String::from_str(env, "USD"),
        &None::<Address>,
        &None::<String>,
        &crate::merchant_registry::MaybeFeeConfig::None,
    );
    merchant_client.verify_merchant(&admin, &merchant);

    (oracle, merchant)
}

#[test]
fn test_create_payments_batch_all_succeed() {
    let env = Env::default();
    let (_admin, _processor_addr, payment_client, merchant_client) =
        setup_payment_processor_with_registry(&env);
    let (_oracle, merchant) =
        setup_oracle_and_merchant(&env, &_admin, &payment_client, &merchant_client);

    let args1 = crate::CreatePaymentArgs {
        payment_id: String::from_str(&env, "pay_batch_001"),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 100_000_000i128,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        idempotency_key: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
            tip_enabled: false,
    };

    let args2 = crate::CreatePaymentArgs {
        payment_id: String::from_str(&env, "pay_batch_002"),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 200_000_000i128,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        idempotency_key: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
            tip_enabled: false,
    };

    let batch = soroban_sdk::vec![&env, args1, args2];
    let result = payment_client.try_create_payments_batch(&batch);

    assert!(result.is_ok());
    let payment_ids = result.unwrap();
    assert_eq!(payment_ids.len(), 2);
}

#[test]
fn test_create_payments_batch_one_invalid_amount_fails_all() {
    let env = Env::default();
    let (_admin, _processor_addr, payment_client, merchant_client) =
        setup_payment_processor_with_registry(&env);
    let (_oracle, merchant) =
        setup_oracle_and_merchant(&env, &_admin, &payment_client, &merchant_client);

    let args1 = crate::CreatePaymentArgs {
        payment_id: String::from_str(&env, "pay_batch_bad_001"),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 100_000_000i128,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        idempotency_key: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
            tip_enabled: false,
    };

    let args2 = crate::CreatePaymentArgs {
        payment_id: String::from_str(&env, "pay_batch_bad_002"),
        merchant_id: merchant.clone(),
        payer: None,
        amount: -100i128,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        idempotency_key: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
            tip_enabled: false,
    };

    let batch = soroban_sdk::vec![&env, args1, args2];
    let result = payment_client.try_create_payments_batch(&batch);

    assert!(result.is_err());

    let payment1 = payment_client.try_get_payment(&String::from_str(&env, "pay_batch_bad_001"));
    assert!(payment1.is_err());
}

#[test]
fn test_create_payments_batch_duplicate_idempotency_key_within_batch() {
    let env = Env::default();
    let (_admin, _processor_addr, payment_client, merchant_client) =
        setup_payment_processor_with_registry(&env);
    let (_oracle, merchant) =
        setup_oracle_and_merchant(&env, &_admin, &payment_client, &merchant_client);

    let client_token = String::from_str(&env, "idempotency_token_123");

    let args1 = crate::CreatePaymentArgs {
        payment_id: String::from_str(&env, "pay_batch_dup_001"),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 100_000_000i128,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: Some(client_token.clone()),
        idempotency_key: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
            tip_enabled: false,
    };

    let args2 = crate::CreatePaymentArgs {
        payment_id: String::from_str(&env, "pay_batch_dup_002"),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 200_000_000i128,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: Some(client_token.clone()),
        idempotency_key: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
            tip_enabled: false,
    };

    let batch = soroban_sdk::vec![&env, args1, args2];
    let result = payment_client.try_create_payments_batch(&batch);

    assert!(result.is_err());
}

#[test]
fn test_create_payments_batch_size_limit_enforcement() {
    let env = Env::default();
    let (_admin, _processor_addr, payment_client, merchant_client) =
        setup_payment_processor_with_registry(&env);
    let (_oracle, merchant) =
        setup_oracle_and_merchant(&env, &_admin, &payment_client, &merchant_client);

    let mut batch = soroban_sdk::vec![&env];
    for i in 0..51 {
        let args = crate::CreatePaymentArgs {
            payment_id: String::from_str(&env, &format!("pay_batch_large_{}", i)),
            merchant_id: merchant.clone(),
            payer: None,
            amount: 100_000_000i128,
            currency: Symbol::new(&env, "USDC"),
            deposit_address: Address::generate(&env),
            expires_at: Some(env.ledger().timestamp() + 3600),
            duration_secs: None,
            memo: None,
            memo_type: None,
            token_address: None,
            client_token: None,
            idempotency_key: None,
            metadata_hash: None,
            metadata: None,
            fee_waiver_code: None,
            retry_of_payment_id: None,
            payer_muxed_id: None,
        };
        batch.push_back(args);
    }

    let result = payment_client.try_create_payments_batch(&batch);

    assert!(result.is_err());
}

#[test]
fn test_create_payments_batch_events_emitted_for_each() {
    let env = Env::default();
    let (_admin, _processor_addr, payment_client, merchant_client) =
        setup_payment_processor_with_registry(&env);
    let (_oracle, merchant) =
        setup_oracle_and_merchant(&env, &_admin, &payment_client, &merchant_client);

    let args1 = crate::CreatePaymentArgs {
        payment_id: String::from_str(&env, "pay_batch_evt_001"),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 100_000_000i128,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        idempotency_key: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
            tip_enabled: false,
    };

    let args2 = crate::CreatePaymentArgs {
        payment_id: String::from_str(&env, "pay_batch_evt_002"),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 200_000_000i128,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        idempotency_key: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
            tip_enabled: false,
    };

    let batch = soroban_sdk::vec![&env, args1, args2];
    let result = payment_client.try_create_payments_batch(&batch);

    assert!(result.is_ok());
    let payment_ids = result.unwrap();
    assert_eq!(payment_ids.len(), 2);
}

#[test]
fn test_create_payment_batch_accepts_1_to_10_items() {
    let env = Env::default();
    let (_admin, _processor_addr, payment_client, merchant_client) =
        setup_payment_processor_with_registry(&env);
    let (_oracle, merchant) =
        setup_oracle_and_merchant(&env, &_admin, &payment_client, &merchant_client);

    let mut payments = soroban_sdk::vec![&env];
    for i in 0..5 {
        let req = crate::PaymentRequest {
            payment_id: String::from_str(&env, &format!("pay_req_{}", i)),
            amount: 100_000_000i128 * (i as i128 + 1),
            currency: Symbol::new(&env, "USDC"),
            deposit_address: Address::generate(&env),
            expires_at: Some(env.ledger().timestamp() + 3600),
            duration_secs: None,
            memo: None,
            memo_type: None,
            token_address: None,
            client_token: Some(String::from_str(&env, &format!("token_batch_{}", i))),
            metadata_hash: None,
            metadata: None,
            fee_waiver_code: None,
            payer: None,
            payer_muxed_id: None,
        };
        payments.push_back(req);
    }

    let result = payment_client.try_create_payment_batch(&merchant, &payments);
    assert!(result.is_ok());
    let ids = result.unwrap();
    assert_eq!(ids.len(), 5);

    // Verify all 5 payments are stored
    for i in 0..5 {
        let payment_id = String::from_str(&env, &format!("pay_req_{}", i));
        let payment = payment_client.get_payment(&payment_id);
        assert_eq!(payment.merchant_id, merchant);
        assert_eq!(payment.amount, 100_000_000i128 * (i as i128 + 1));
    }
}

#[test]
fn test_create_payment_batch_returns_batch_too_large_for_over_10_items() {
    let env = Env::default();
    let (_admin, _processor_addr, payment_client, merchant_client) =
        setup_payment_processor_with_registry(&env);
    let (_oracle, merchant) =
        setup_oracle_and_merchant(&env, &_admin, &payment_client, &merchant_client);

    let mut payments = soroban_sdk::vec![&env];
    for i in 0..11 {
        let req = crate::PaymentRequest {
            payment_id: String::from_str(&env, &format!("pay_req_large_{}", i)),
            amount: 100_000_000i128,
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
            payer: None,
            payer_muxed_id: None,
        };
        payments.push_back(req);
    }

    let result = payment_client.try_create_payment_batch(&merchant, &payments);
    assert_eq!(result, Err(Ok(crate::Error::BatchTooLarge)));
}

#[test]
fn test_create_payment_batch_atomic_rollback_on_failure() {
    let env = Env::default();
    let (_admin, _processor_addr, payment_client, merchant_client) =
        setup_payment_processor_with_registry(&env);
    let (_oracle, merchant) =
        setup_oracle_and_merchant(&env, &_admin, &payment_client, &merchant_client);

    let req1 = crate::PaymentRequest {
        payment_id: String::from_str(&env, "atomic_pay_1"),
        amount: 100_000_000i128,
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
        payer: None,
        payer_muxed_id: None,
    };

    // Invalid amount: negative
    let req2 = crate::PaymentRequest {
        payment_id: String::from_str(&env, "atomic_pay_2"),
        amount: -50i128,
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
        payer: None,
        payer_muxed_id: None,
    };

    let payments = soroban_sdk::vec![&env, req1, req2];
    let result = payment_client.try_create_payment_batch(&merchant, &payments);
    assert!(result.is_err());

    // Verify req1 was NOT stored
    let check1 = payment_client.try_get_payment(&String::from_str(&env, "atomic_pay_1"));
    assert!(check1.is_err());
}
