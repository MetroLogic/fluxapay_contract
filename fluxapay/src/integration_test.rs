use crate::{
    merchant_registry::{KycTier, MerchantRegistry, MerchantRegistryClient},
    ArbitratorVoteChoice, DataKey, DisputeStatus, Error, PaymentProcessor, PaymentProcessorClient,
    PaymentStatus, RefundManager, RefundManagerClient, RefundStatus, SettlementSplit,
};
use soroban_sdk::{
    testutils::{Address as _, BytesN as _, Ledger as _},
    token, vec, Address, BytesN, Env, String, Symbol,
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
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    refund_client.initialize_refund_manager(&admin, &usdc_token);
    let token_admin_client = token::StellarAssetClient::new(env, &usdc_token);
    token_admin_client.mint(&refund_manager, &1_000_000_000_000i128);

    payment_client.initialize_payment_processor(&admin);
    merchant_client.initialize(&admin);

    (admin, payment_client, refund_client, merchant_client)
}

fn integration_payment_args(
    env: &Env,
    payment_id: &str,
    merchant_id: &Address,
) -> crate::CreatePaymentArgs {
    crate::CreatePaymentArgs {
        payment_id: String::from_str(env, payment_id),
        merchant_id: merchant_id.clone(),
        payer: None,
        amount: 1000,
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
    }
}

#[test]
fn test_admin_sets_merchant_registry_address() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, _, merchant_client) = setup_integration(&env);

    payment_client.set_merchant_registry_address(&admin, &merchant_client.address);

    let stored = env.as_contract(&payment_client.address, || {
        env.storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
    });
    assert_eq!(stored, Some(merchant_client.address));
}

#[test]
fn test_non_admin_cannot_set_merchant_registry_address() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, payment_client, _, merchant_client) = setup_integration(&env);
    let non_admin = Address::generate(&env);

    let result =
        payment_client.try_set_merchant_registry_address(&non_admin, &merchant_client.address);

    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_create_payment_with_verified_registry_merchant_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, _, merchant_client) = setup_integration(&env);
    let merchant = Address::generate(&env);

    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Verified Merchant"),
        &String::from_str(&env, "USD"),
        &None,
        &None,
        &MaybeFeeConfig::None);
    merchant_client.verify_merchant(&admin, &merchant);
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.set_merchant_registry_address(&admin, &merchant_client.address);

    let payment =
        payment_client.create_payment(&integration_payment_args(&env, "REG_VERIFIED", &merchant));
    assert_eq!(payment.status, PaymentStatus::Pending);
}

#[test]
fn test_create_payment_with_unverified_registry_merchant_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, _, merchant_client) = setup_integration(&env);
    let merchant = Address::generate(&env);

    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Unverified Merchant"),
        &String::from_str(&env, "USD"),
        &None,
        &None,
        &MaybeFeeConfig::None);
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.set_merchant_registry_address(&admin, &merchant_client.address);

    let result = payment_client
        .try_create_payment(&integration_payment_args(&env, "REG_UNVERIFIED", &merchant));
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_create_payment_with_suspended_registry_merchant_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, _, merchant_client) = setup_integration(&env);
    let merchant = Address::generate(&env);

    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Suspended Merchant"),
        &String::from_str(&env, "USD"),
        &None,
        &None,
        &MaybeFeeConfig::None);
    merchant_client.verify_merchant(&admin, &merchant);
    merchant_client.suspend_merchant(
        &admin,
        &merchant,
        &String::from_str(&env, "Compliance hold"),
        &3600,
    );
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.set_merchant_registry_address(&admin, &merchant_client.address);

    let result = payment_client
        .try_create_payment(&integration_payment_args(&env, "REG_SUSPENDED", &merchant));
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_create_payment_with_unregistered_registry_merchant_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, _, merchant_client) = setup_integration(&env);
    let merchant = Address::generate(&env);

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.set_merchant_registry_address(&admin, &merchant_client.address);

    let result =
        payment_client.try_create_payment(&integration_payment_args(&env, "REG_MISSING", &merchant));
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
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
        &MaybeFeeConfig::None);
    merchant_client.verify_merchant(&admin, &merchant);
    let merchant_info = merchant_client.get_merchant(&merchant);
    assert_eq!(merchant_info.kyc_tier, KycTier::Basic);

    // 2. Create and Verify Payment
    let payment_id = String::from_str(&env, "PAY_01");
    let amount = 1000i128;

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None, metadata: None,
    };
    payment_client.create_payment(&args);

    let tx_hash = BytesN::<32>::random(&env);
    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
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
        &vec![&env],
    );

    let operator = Address::generate(&env);
    refund_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    let refund_id = refund_client.resolve_dispute_with_refund(
        &operator,
        &dispute_id,
        &String::from_str(&env, "Refund approved"),
        &String::from_str(&env, "base64sig=="),
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

    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    let payment_id = String::from_str(&env, "PAY_SETTLE");
    let amount = 2000i128;
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None, metadata: None,
    };
    payment_client.create_payment(&args);

    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::<32>::random(&env),
        &customer,
        &amount,
    );

    // Settle payment to treasury as a single split
    let splits = vec![
        &env,
        SettlementSplit {
            recipient: treasury.clone(),
            amount,
        },
    ];
    payment_client.settle_payment(&operator, &payment_id, &splits);

    let payment_info = payment_client.get_payment(&payment_id);
    assert_eq!(payment_info.status, PaymentStatus::Settled);
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

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
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
    };
    payment_client.create_payment(&args);

    // Jump forward in time
    env.ledger().set_timestamp(expires_at + 1);

    // Expire payment via cleanup path
    payment_client.expire_payment(&payment_id);

    let payment_info = payment_client.get_payment(&payment_id);
    assert_eq!(payment_info.status, PaymentStatus::Expired);

    // Register payment with refund manager (with Confirmed status for testing)
    refund_client.register_payment(&payment_id, &merchant, &amount, &Symbol::new(&env, "USDC"));

    // Try to dispute an expired/cancelled payment - should still be possible to create, but maybe rejected?
    let customer = Address::generate(&env);
    let dispute_id = refund_client.create_dispute(
        &payment_id,
        &amount,
        &String::from_str(&env, "Late but flawed"),
        &String::from_str(&env, "N/A"),
        &customer,
        &vec![&env],
    );

    let operator = Address::generate(&env);
    refund_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &operator);

    // Reject dispute
    refund_client.reject_dispute(
        &operator,
        &dispute_id,
        &String::from_str(&env, "Payment already expired and cancelled"),
        &String::from_str(&env, "base64sig=="),
    );

    let dispute_info = refund_client.get_dispute(&dispute_id);
    assert_eq!(dispute_info.status, DisputeStatus::Rejected);
}

#[test]
fn test_upgrade_contract_payment_processor() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, _merchant_client) = setup_integration(&env);

    let new_wasm_hash = BytesN::<32>::random(&env);

    // Admin can upgrade
    payment_client.upgrade_contract(&admin, &new_wasm_hash);
}

#[test]
fn test_upgrade_contract_payment_processor_non_admin_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, _merchant_client) = setup_integration(&env);
    let non_admin = Address::generate(&env);

    let new_wasm_hash = BytesN::<32>::random(&env);

    // Non-admin cannot upgrade
    assert!(payment_client.try_upgrade_contract(&non_admin, &new_wasm_hash).is_err());
}

#[test]
fn test_upgrade_contract_refund_manager() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, _payment_client, refund_client, _merchant_client) = setup_integration(&env);

    let new_wasm_hash = BytesN::<32>::random(&env);

    // Admin can upgrade
    refund_client.upgrade_contract(&admin, &new_wasm_hash);
}

#[test]
fn test_upgrade_contract_refund_manager_non_admin_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, _payment_client, refund_client, _merchant_client) = setup_integration(&env);
    let non_admin = Address::generate(&env);

    let new_wasm_hash = BytesN::<32>::random(&env);

    // Non-admin cannot upgrade
    assert!(refund_client.try_upgrade_contract(&non_admin, &new_wasm_hash).is_err());
}

#[test]
fn test_upgrade_contract_storage_compatibility() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, _merchant_client) = setup_integration(&env);
    let merchant = Address::generate(&env);

    // Create a payment before upgrade to test storage compatibility
    let payment_id = String::from_str(&env, "PAY_UPGRADE");
    let amount = 1000i128;

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None, metadata: None,
    };
    payment_client.create_payment(&args);

    // Verify payment exists before upgrade
    let payment_before = payment_client.get_payment(&payment_id);
    assert_eq!(payment_before.amount, amount);

    // Perform upgrade
    let new_wasm_hash = BytesN::<32>::random(&env);
    payment_client.upgrade_contract(&admin, &new_wasm_hash);

    // Verify payment still exists after upgrade (storage compatibility)
    let payment_after = payment_client.get_payment(&payment_id);
    assert_eq!(payment_after.amount, amount);
}

#[test]
fn test_prune_expired_payments_expired_pending() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, _merchant_client) = setup_integration(&env);
    let merchant = Address::generate(&env);
    let operator = Address::generate(&env);

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    // Create an expired pending payment
    let payment_id = String::from_str(&env, "PAY_EXPIRE_PRUNE");
    let amount = 1000i128;
    let expires_at = env.ledger().timestamp() + 100;

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
    };
    payment_client.create_payment(&args);

    // Jump forward in time to expire the payment
    env.ledger().set_timestamp(expires_at + 1);

    // Prune the expired payment
    let payment_ids = vec![&env, payment_id.clone()];
    let result = payment_client.prune_expired_payments(&operator, &payment_ids);
    assert_eq!(result, 1);

    // Verify payment is deleted
    assert!(payment_client.try_get_payment(&payment_id).is_err());
}

#[test]
fn test_prune_expired_payments_non_expired_skipped() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, _merchant_client) = setup_integration(&env);
    let merchant = Address::generate(&env);
    let operator = Address::generate(&env);

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    // Create a non-expired pending payment
    let payment_id = String::from_str(&env, "PAY_NOT_EXPIRE");
    let amount = 1000i128;
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
    };
    payment_client.create_payment(&args);

    // Prune (payment should not be pruned as it's not expired)
    let payment_ids = vec![&env, payment_id.clone()];
    let result = payment_client.prune_expired_payments(&operator, &payment_ids);
    assert_eq!(result, 0);

    // Verify payment still exists
    payment_client.get_payment(&payment_id);
}

#[test]
fn test_prune_expired_payments_non_pending_skipped() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, _merchant_client) = setup_integration(&env);
    let merchant = Address::generate(&env);
    let oracle = Address::generate(&env);
    let operator = Address::generate(&env);

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    // Create a confirmed (non-pending) payment
    let payment_id = String::from_str(&env, "PAY_CONFIRMED");
    let amount = 1000i128;
    let expires_at = env.ledger().timestamp() + 100;

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
    };
    payment_client.create_payment(&args);

    // Verify the payment to change it from Pending to Confirmed
    let customer = Address::generate(&env);
    payment_client.verify_payment(&oracle, &payment_id, &BytesN::<32>::random(&env), &customer, &amount);

    // Jump forward in time
    env.ledger().set_timestamp(expires_at + 1);

    // Prune (confirmed payment should not be pruned)
    let payment_ids = vec![&env, payment_id.clone()];
    let result = payment_client.prune_expired_payments(&operator, &payment_ids);
    assert_eq!(result, 0);

    // Verify payment still exists
    let payment_info = payment_client.get_payment(&payment_id);
    assert_eq!(payment_info.status, PaymentStatus::Confirmed);
}

#[test]
fn test_prune_expired_payments_unauthorized_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, _merchant_client) = setup_integration(&env);
    let non_operator = Address::generate(&env);

    // Try to prune as non-operator
    let payment_ids = vec![&env];
    assert!(payment_client.try_prune_expired_payments(&non_operator, &payment_ids).is_err());
}

#[test]
fn test_prune_expired_payments_empty_list() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, _merchant_client) = setup_integration(&env);
    let operator = Address::generate(&env);

    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    // Prune with empty list
    let payment_ids = vec![&env];
    let result = payment_client.prune_expired_payments(&operator, &payment_ids);
    assert_eq!(result, 0);
}

#[test]
fn test_settle_payment_with_zero_merchant_fee() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, merchant_client) = setup_integration(&env);
    let merchant = Address::generate(&env);
    let operator = Address::generate(&env);

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    // Register merchant with zero fee
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USD"),
        &None::<Address>,
        &None::<String>,
        &MaybeFeeConfig::None);
    merchant_client.verify_merchant(&admin, &merchant);
    merchant_client.set_fee_config(&admin, &merchant, &0i128, &0i128, &None::<Address>);

    // Wire payment processor to merchant registry
    payment_client.set_merchant_registry_address(&admin, &merchant_client.address);

    // Create and settle payment
    let payment_id = String::from_str(&env, "PAY_ZERO_FEE");
    let amount = 1000i128;

    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None, metadata: None,
    };
    payment_client.create_payment(&args);

    let customer = Address::generate(&env);
    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(&oracle, &payment_id, &BytesN::<32>::random(&env), &customer, &amount);

    // Settlement should work with registry and fee config
    let splits = vec![&env, crate::SettlementSplit {
        recipient: merchant.clone(),
        amount,
    }];
    payment_client.settle_payment(&operator, &payment_id, &splits);

    let payment = payment_client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Settled);
}

#[test]
fn test_settle_payment_with_bps_only_fee() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, merchant_client) = setup_integration(&env);
    let merchant = Address::generate(&env);
    let operator = Address::generate(&env);

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    // Register merchant with 5% BPS fee
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USD"),
        &None::<Address>,
        &None::<String>,
        &MaybeFeeConfig::None);
    merchant_client.verify_merchant(&admin, &merchant);
    merchant_client.set_fee_config(&admin, &merchant, &500i128, &0i128, &None::<Address>);

    // Wire payment processor to merchant registry
    payment_client.set_merchant_registry_address(&admin, &merchant_client.address);

    // Create and settle payment
    let payment_id = String::from_str(&env, "PAY_BPS_FEE");
    let amount = 1000i128;

    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None, metadata: None,
    };
    payment_client.create_payment(&args);

    let customer = Address::generate(&env);
    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(&oracle, &payment_id, &BytesN::<32>::random(&env), &customer, &amount);

    // Settlement should work with registry and fee config
    let splits = vec![&env, crate::SettlementSplit {
        recipient: merchant.clone(),
        amount,
    }];
    payment_client.settle_payment(&operator, &payment_id, &splits);

    let payment = payment_client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Settled);
}

#[test]
fn test_settle_payment_with_fixed_fee() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, merchant_client) = setup_integration(&env);
    let merchant = Address::generate(&env);
    let operator = Address::generate(&env);

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    // Register merchant with fixed fee of 100
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USD"),
        &None::<Address>,
        &None::<String>,
        &MaybeFeeConfig::None);
    merchant_client.verify_merchant(&admin, &merchant);
    merchant_client.set_fee_config(&admin, &merchant, &0i128, &100i128, &None::<Address>);

    // Wire payment processor to merchant registry
    payment_client.set_merchant_registry_address(&admin, &merchant_client.address);

    // Create and settle payment
    let payment_id = String::from_str(&env, "PAY_FIXED_FEE");
    let amount = 1000i128;

    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None, metadata: None,
    };
    payment_client.create_payment(&args);

    let customer = Address::generate(&env);
    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(&oracle, &payment_id, &BytesN::<32>::random(&env), &customer, &amount);

    // Settlement should work with registry and fee config
    let splits = vec![&env, crate::SettlementSplit {
        recipient: merchant.clone(),
        amount,
    }];
    payment_client.settle_payment(&operator, &payment_id, &splits);

    let payment = payment_client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Settled);
}

#[test]
fn test_settle_payment_with_combined_fee() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, merchant_client) = setup_integration(&env);
    let merchant = Address::generate(&env);
    let operator = Address::generate(&env);

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    // Register merchant with 2% BPS + 50 fixed fee
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USD"),
        &None::<Address>,
        &None::<String>,
        &MaybeFeeConfig::None);
    merchant_client.verify_merchant(&admin, &merchant);
    merchant_client.set_fee_config(&admin, &merchant, &200i128, &50i128, &None::<Address>);

    // Wire payment processor to merchant registry
    payment_client.set_merchant_registry_address(&admin, &merchant_client.address);

    // Create and settle payment
    let payment_id = String::from_str(&env, "PAY_COMBINED_FEE");
    let amount = 1000i128;

    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None, metadata: None,
    };
    payment_client.create_payment(&args);

    let customer = Address::generate(&env);
    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(&oracle, &payment_id, &BytesN::<32>::random(&env), &customer, &amount);

    // Settlement should work with registry and fee config
    let splits = vec![&env, crate::SettlementSplit {
        recipient: merchant.clone(),
        amount,
    }];
    payment_client.settle_payment(&operator, &payment_id, &splits);

    let payment = payment_client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Settled);
}

#[test]
fn test_settle_payment_no_registry_configured() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, _merchant_client) = setup_integration(&env);
    let merchant = Address::generate(&env);
    let operator = Address::generate(&env);

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    // No registry configured - existing behavior should work

    let payment_id = String::from_str(&env, "PAY_NO_REGISTRY");
    let amount = 1000i128;

    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None, metadata: None,
    };
    payment_client.create_payment(&args);

    let customer = Address::generate(&env);
    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(&oracle, &payment_id, &BytesN::<32>::random(&env), &customer, &amount);

    // Settlement with splits should work without registry
    let splits = vec![&env, crate::SettlementSplit {
        recipient: Address::generate(&env),
        amount,
    }];
    payment_client.settle_payment(&operator, &payment_id, &splits);

    let payment = payment_client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Settled);
}

#[test]
fn test_cross_contract_happy_path() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, merchant_client) = setup_integration(&env);

    // Wire payment processor to merchant registry
    payment_client.set_merchant_registry_address(&admin, &merchant_client.address);

    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);
    let operator = Address::generate(&env);

    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    // Happy path: register merchant → verify merchant → create_payment → confirm payment → settle
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Flux Merchant"),
        &String::from_str(&env, "USD"),
        &None::<Address>,
        &None::<String>,
        &MaybeFeeConfig::None);

    let merchant_info = merchant_client.get_merchant(&merchant);
    assert_eq!(merchant_info.kyc_tier, KycTier::Unverified);

    merchant_client.verify_merchant(&admin, &merchant);
    let verified_merchant = merchant_client.get_merchant(&merchant);
    assert_eq!(verified_merchant.kyc_tier, KycTier::Basic);

    // Now create payment with verified merchant
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    let payment_id = String::from_str(&env, "PAY_CROSS_01");
    let amount = 1000i128;

    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None, metadata: None,
    };
    payment_client.create_payment(&args);

    let payment_info = payment_client.get_payment(&payment_id);
    assert_eq!(payment_info.status, PaymentStatus::Pending);

    // Confirm payment
    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    let tx_hash = BytesN::<32>::random(&env);
    payment_client.verify_payment(&oracle, &payment_id, &tx_hash, &customer, &amount);

    let confirmed = payment_client.get_payment(&payment_id);
    assert_eq!(confirmed.status, PaymentStatus::Confirmed);

    // Settle payment
    let splits = vec![
        &env,
        SettlementSplit {
            recipient: merchant.clone(),
            amount,
        },
    ];
    payment_client.settle_payment(&operator, &payment_id, &splits);

    let settled = payment_client.get_payment(&payment_id);
    assert_eq!(settled.status, PaymentStatus::Settled);
}

#[test]
fn test_cross_contract_unverified_merchant_rejection() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, merchant_client) = setup_integration(&env);

    // Wire payment processor to merchant registry
    payment_client.set_merchant_registry_address(&admin, &merchant_client.address);

    let merchant = Address::generate(&env);

    // Register but don't verify merchant
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Flux Merchant"),
        &String::from_str(&env, "USD"),
        &None::<Address>,
        &None::<String>,
        &MaybeFeeConfig::None);

    // Try to create payment with unverified merchant
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    let payment_id = String::from_str(&env, "PAY_UNVERIFIED");
    let amount = 1000i128;

    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None, metadata: None,
    };

    // Creating payment with unverified merchant should fail
    assert!(payment_client.try_create_payment(&args).is_err());
}

#[test]
fn test_cross_contract_suspended_merchant_rejection() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, merchant_client) = setup_integration(&env);

    // Wire payment processor to merchant registry
    payment_client.set_merchant_registry_address(&admin, &merchant_client.address);

    let merchant = Address::generate(&env);

    // Register and verify merchant
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Flux Merchant"),
        &String::from_str(&env, "USD"),
        &None::<Address>,
        &None::<String>,
        &MaybeFeeConfig::None);
    merchant_client.verify_merchant(&admin, &merchant);

    // Suspend the merchant
    merchant_client.suspend_merchant(
        &admin,
        &merchant,
        &String::from_str(&env, "Suspicious activity"),
        &0u64,
    );

    // Try to create payment with suspended merchant
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    let payment_id = String::from_str(&env, "PAY_SUSPENDED");
    let amount = 1000i128;

    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None, metadata: None,
    };

    // Creating payment with suspended merchant should fail
    assert!(payment_client.try_create_payment(&args).is_err());
}

#[test]
fn test_cross_contract_registry_not_set_regression() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, _refund_client, merchant_client) = setup_integration(&env);

    // Don't wire payment processor to merchant registry - regression test

    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);

    // Register merchant in merchant registry only
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Flux Merchant"),
        &String::from_str(&env, "USD"),
        &None::<Address>,
        &None::<String>,
        &MaybeFeeConfig::None);

    // Try to create payment without registry wired
    // Should check merchant role, not merchant verification status
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    let payment_id = String::from_str(&env, "PAY_NO_REG");
    let amount = 1000i128;

    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None, metadata: None,
    };

    // Should succeed because merchant has MERCHANT role (registry check skipped)
    payment_client.create_payment(&args);

    // Verify payment
    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    let tx_hash = BytesN::<32>::random(&env);
    payment_client.verify_payment(&oracle, &payment_id, &tx_hash, &customer, &amount);

    let payment_info = payment_client.get_payment(&payment_id);
    assert_eq!(payment_info.status, PaymentStatus::Confirmed);
}

/* ------------------------------------------------------------------ */
/*  Dispute arbitration lifecycle integration tests                    */
/* ------------------------------------------------------------------ */

fn mint_dispute_bonds(
    env: &Env,
    refund_client: &RefundManagerClient,
    customer: &Address,
    merchant: &Address,
) {
    let token_address = env
        .as_contract(&refund_client.address, || {
            env.storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::UsdcToken)
                .unwrap()
        });
    let usdc_client = token::StellarAssetClient::new(env, &token_address);
    usdc_client.mint(customer, &100_000);
    usdc_client.mint(merchant, &100_000);
}

fn setup_dispute(
    env: &Env,
    payment_client: &PaymentProcessorClient,
    refund_client: &RefundManagerClient,
    merchant_client: &MerchantRegistryClient,
    admin: &Address,
    payment_id: &str,
) -> (Address, Address, Address, String) {
    let merchant = Address::generate(env);
    let customer = Address::generate(env);
    let operator = Address::generate(env);
    let amount = 1000i128;

    merchant_client.register_merchant(
        &merchant,
        &String::from_str(env, "Dispute Merchant"),
        &String::from_str(env, "USD"),
        &None::<Address>,
        &None::<String>,
        &MaybeFeeConfig::None);
    merchant_client.verify_merchant(admin, &merchant);

    let pid = String::from_str(env, payment_id);
    payment_client.grant_role(admin, &Symbol::new(env, "MERCHANT"), &merchant);
    let args = crate::CreatePaymentArgs {
        payment_id: pid.clone(),
        merchant_id: merchant.clone(),
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
    };
    payment_client.create_payment(&args);

    let tx_hash = BytesN::<32>::random(env);
    let oracle = Address::generate(env);
    payment_client.grant_role(admin, &Symbol::new(env, "ORACLE"), &oracle);
    payment_client.verify_payment(&oracle, &pid, &tx_hash, &customer, &amount);

    refund_client.register_payment(&pid, &merchant, &amount, &Symbol::new(env, "USDC"));
    mint_dispute_bonds(env, refund_client, &customer, &merchant);

    refund_client.grant_role(
        admin,
        &Symbol::new(env, "SETTLEMENT_OPERATOR"),
        &operator,
    );

    let evidence = String::from_str(env, "Qm00000000000000000000000000000000000000");
    let dispute_id = refund_client.create_dispute(
        &pid,
        &amount,
        &String::from_str(env, "Product not as described"),
        &evidence,
        &customer,
        &vec![env],
    );

    (merchant, customer, operator, dispute_id)
}

#[test]
fn test_full_dispute_arbitration_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, refund_client, merchant_client) = setup_integration(&env);
    let (_merchant, _customer, operator, dispute_id) = setup_dispute(
        &env,
        &payment_client,
        &refund_client,
        &merchant_client,
        &admin,
        "PAY_ARB_APPROVE",
    );

    let arb_count = 3;
    let mut arbitrators = Vec::new();
    for i in 0..arb_count {
        let arb = Address::generate(&env);
        refund_client.grant_role(&admin, &Symbol::new(&env, "ARBITRATOR"), &arb);
        refund_client.submit_arbitrator_vote(
            &dispute_id,
            &arb,
            &ArbitratorVoteChoice::Approve,
        );
        arbitrators.push(arb);
    }

    let threshold_met = refund_client.check_arbitration_threshold(&dispute_id);
    assert!(threshold_met);

    let refund_id = refund_client.resolve_dispute_with_refund(
        &operator,
        &dispute_id,
        &String::from_str(&env, "Arbitrators approved refund"),
        &String::from_str(&env, "base64sig=="),
    );

    let dispute = refund_client.get_dispute(&dispute_id);
    assert_eq!(dispute.status, DisputeStatus::Resolved);
    assert!(dispute.refund_id.is_some());

    let refund = refund_client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Completed);

    let voters_key = DataKey::DisputeArbitratorVotes(dispute_id.clone());
    let voters: soroban_sdk::Vec<Address> = env
        .as_contract(&refund_client.address, || {
            env.storage()
                .persistent()
                .get(&voters_key)
                .unwrap_or(vec![&env])
        });
    assert_eq!(voters.len(), arb_count);
}

#[test]
fn test_dispute_rejection_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, refund_client, merchant_client) = setup_integration(&env);
    let (_merchant, _customer, operator, dispute_id) = setup_dispute(
        &env,
        &payment_client,
        &refund_client,
        &merchant_client,
        &admin,
        "PAY_ARB_REJECT",
    );

    for _ in 0..3 {
        let arb = Address::generate(&env);
        refund_client.grant_role(&admin, &Symbol::new(&env, "ARBITRATOR"), &arb);
        refund_client.submit_arbitrator_vote(
            &dispute_id,
            &arb,
            &ArbitratorVoteChoice::Reject,
        );
    }

    let threshold_result = refund_client.try_check_arbitration_threshold(&dispute_id);
    assert_eq!(
        threshold_result,
        Err(Ok(Error::ArbitrationVotingThresholdNotMet))
    );

    refund_client.reject_dispute(
        &operator,
        &dispute_id,
        &String::from_str(&env, "Arbitrators rejected dispute"),
        &String::from_str(&env, "base64sig=="),
    );

    let dispute = refund_client.get_dispute(&dispute_id);
    assert_eq!(dispute.status, DisputeStatus::Rejected);
    assert!(dispute.refund_id.is_none());
    assert!(dispute.resolved_at.is_some());
}

#[test]
fn test_dispute_escalation_past_deadline() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, refund_client, merchant_client) = setup_integration(&env);
    let (_merchant, _customer, operator, dispute_id) = setup_dispute(
        &env,
        &payment_client,
        &refund_client,
        &merchant_client,
        &admin,
        "PAY_ESCALATE",
    );

    let now = env.ledger().timestamp();
    let future_deadline = now + 3600;
    refund_client.set_dispute_deadline(&operator, &dispute_id, &future_deadline);

    let dispute_before = refund_client.get_dispute(&dispute_id);
    assert!(!dispute_before.escalated);

    env.ledger().set_timestamp(future_deadline + 1);
    refund_client.check_dispute_deadline(&dispute_id);

    let dispute_after = refund_client.get_dispute(&dispute_id);
    assert!(dispute_after.escalated);

    let event_count_before = env.events().all().len();
    refund_client.check_dispute_deadline(&dispute_id);
    assert_eq!(env.events().all().len(), event_count_before);
}
