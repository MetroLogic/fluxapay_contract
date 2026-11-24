use crate::merchant_registry::MaybeFeeConfig;
use crate::{
    merchant_registry::{MerchantRegistry, MerchantRegistryClient, SettlementSchedule},
    Error, PaymentProcessor, PaymentProcessorClient, PaymentStatus,
};
use soroban_sdk::{
    testutils::{Address as _, BytesN as _, Ledger as _},
    token, vec, Address, BytesN, Env, String, Symbol,
};

fn setup(
    env: &Env,
) -> (
    Address,
    PaymentProcessorClient,
    MerchantRegistryClient,
    Address,
) {
    let payment_processor = env.register(PaymentProcessor, ());
    let merchant_registry = env.register(MerchantRegistry, ());
    let refund_manager = env.register(crate::RefundManager, ());

    let payment_client = PaymentProcessorClient::new(env, &payment_processor);
    let merchant_client = MerchantRegistryClient::new(env, &merchant_registry);
    let admin = Address::generate(env);
    let token_admin = Address::generate(env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    let refund_client = crate::RefundManagerClient::new(env, &refund_manager);
    refund_client.initialize_refund_manager(&admin, &usdc_token);
    let token_admin_client = token::StellarAssetClient::new(env, &usdc_token);
    token_admin_client.mint(&refund_manager, &1_000_000_000_000i128);

    payment_client.initialize_payment_processor(&admin);
    merchant_client.initialize(&admin);

    // Wire payment processor to merchant registry.
    payment_client.set_merchant_registry_address(&admin, &merchant_client.address);
    merchant_client.set_payment_processor_address(&admin, &payment_client.address);

    let merchant = Address::generate(env);
    (admin, payment_client, merchant_client, merchant)
}

fn register_merchant(
    env: &Env,
    admin: &Address,
    merchant_client: &MerchantRegistryClient,
    merchant: &Address,
) {
    merchant_client.register_merchant(
        merchant,
        &String::from_str(env, "Test Merchant"),
        &String::from_str(env, "USD"),
        &Some(merchant.clone()),
        &None::<String>,
        &MaybeFeeConfig::None,
    );
    merchant_client.verify_merchant(admin, merchant);
}

/// Create and verify a payment (no settlement), returning its payment_id string.
/// Used by the `settle_payment` interval/minimum enforcement tests (Issue #1031),
/// which need to call `settle_payment` themselves rather than through
/// `create_and_settle`.
fn create_and_verify(
    env: &Env,
    admin: &Address,
    payment_client: &PaymentProcessorClient,
    merchant: &Address,
    payment_id: &str,
    amount: i128,
) {
    payment_client.grant_role(admin, &Symbol::new(env, "MERCHANT"), merchant);
    let args = crate::CreatePaymentArgs {
        payment_id: String::from_str(env, payment_id),
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
        idempotency_key: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
        tip_enabled: false,
    };
    payment_client.create_payment(&args);

    let tx_hash = BytesN::<32>::random(env);
    let oracle = Address::generate(env);
    payment_client.grant_role(admin, &Symbol::new(env, "ORACLE"), &oracle);
    payment_client.verify_payment(
        &oracle,
        &String::from_str(env, payment_id),
        &tx_hash,
        &Address::generate(env),
        &amount,
        &None::<u64>,
    );
}

fn create_and_settle(
    env: &Env,
    admin: &Address,
    payment_client: &PaymentProcessorClient,
    merchant: &Address,
    payment_id: &str,
    amount: i128,
) {
    payment_client.grant_role(admin, &Symbol::new(env, "MERCHANT"), merchant);
    let args = crate::CreatePaymentArgs {
        payment_id: String::from_str(env, payment_id),
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
        idempotency_key: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
            tip_enabled: false,
    };
    payment_client.create_payment(&args);

    let tx_hash = BytesN::<32>::random(env);
    let oracle = Address::generate(env);
    payment_client.grant_role(admin, &Symbol::new(env, "ORACLE"), &oracle);
    payment_client.verify_payment(
        &oracle,
        &String::from_str(env, payment_id),
        &tx_hash,
        &Address::generate(env),
        &amount,
        &None::<u64>,
    );

    let operator = Address::generate(env);
    payment_client.grant_role(admin, &Symbol::new(env, "SETTLEMENT_OPERATOR"), &operator);
    let splits = soroban_sdk::vec![
        env,
        crate::SettlementSplit {
            recipient: merchant.clone(),
            amount,
        },
    ];
    payment_client.settle_payment(&operator, &String::from_str(env, payment_id), &splits);
}

/* ------------------------------------------------------------------ */
/*  SettlementSchedule config                                          */
/* ------------------------------------------------------------------ */

#[test]
fn test_settlement_schedule_defaults_to_manual() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _payment_client, merchant_client, merchant) = setup(&env);

    register_merchant(&env, &admin, &merchant_client, &merchant);

    let info = merchant_client.get_merchant(&merchant);
    assert_eq!(info.settlement_schedule, SettlementSchedule::Manual);
    assert_eq!(info.last_settlement_at, None);
}

#[test]
fn test_settlement_schedule_can_be_changed() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _payment_client, merchant_client, merchant) = setup(&env);

    register_merchant(&env, &admin, &merchant_client, &merchant);

    merchant_client.set_settlement_schedule(&merchant, &SettlementSchedule::Daily);

    let info = merchant_client.get_merchant(&merchant);
    assert_eq!(info.settlement_schedule, SettlementSchedule::Daily);
}

/* ------------------------------------------------------------------ */
/*  Pending settlement accumulation                                    */
/* ------------------------------------------------------------------ */

#[test]
fn test_pending_settlement_accumulates_on_settle() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, merchant) = setup(&env);

    register_merchant(&env, &admin, &merchant_client, &merchant);

    create_and_settle(
        &env,
        &admin,
        &payment_client,
        &merchant,
        "PAY_SETTLE_001",
        1000,
    );

    let pending = merchant_client.get_pending_settlement(&merchant);
    assert_eq!(pending, 1000);
}

#[test]
fn test_pending_settlement_multiple_payments() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, merchant) = setup(&env);

    register_merchant(&env, &admin, &merchant_client, &merchant);

    create_and_settle(&env, &admin, &payment_client, &merchant, "PAY_001", 500);
    create_and_settle(&env, &admin, &payment_client, &merchant, "PAY_002", 300);

    let pending = merchant_client.get_pending_settlement(&merchant);
    assert_eq!(pending, 800);
}

/* ------------------------------------------------------------------ */
/*  trigger_settlement                                                 */
/* ------------------------------------------------------------------ */

#[test]
fn test_trigger_settlement_manual_settles_immediately() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, merchant) = setup(&env);

    // Register merchant with payout address
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USD"),
        &Some(merchant.clone()),
        &None::<String>,
        &MaybeFeeConfig::None,
    );
    merchant_client.verify_merchant(&admin, &merchant);

    create_and_settle(
        &env,
        &admin,
        &payment_client,
        &merchant,
        "PAY_TRIGGER",
        2000000,
    );

    let operator = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    let swept = payment_client.trigger_settlement(&operator, &merchant);
    assert!(swept >= 2000000);

    let pending = merchant_client.get_pending_settlement(&merchant);
    assert_eq!(pending, 0);
}

#[test]
fn test_trigger_settlement_daily_enforces_interval() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, merchant) = setup(&env);

    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USD"),
        &Some(merchant.clone()),
        &None::<String>,
        &MaybeFeeConfig::None,
    );
    merchant_client.verify_merchant(&admin, &merchant);

    // Set to daily schedule.
    merchant_client.set_settlement_schedule(&merchant, &SettlementSchedule::Daily);

    create_and_settle(
        &env,
        &admin,
        &payment_client,
        &merchant,
        "PAY_DAILY",
        2000000,
    );

    let operator = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    // First settlement succeeds.
    let swept = payment_client.trigger_settlement(&operator, &merchant);
    assert!(swept >= 2000000);

    // Immediate second settlement should fail (daily interval not elapsed).
    let result = payment_client.try_trigger_settlement(&operator, &merchant);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_trigger_settlement_below_min_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, merchant) = setup(&env);

    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USD"),
        &Some(merchant.clone()),
        &None::<String>,
        &MaybeFeeConfig::None,
    );
    merchant_client.verify_merchant(&admin, &merchant);

    // Settle a very small amount (below SETTLEMENT_MIN_AMOUNT = 1_000_000).
    create_and_settle(&env, &admin, &payment_client, &merchant, "PAY_SMALL", 500);

    let operator = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    let result = payment_client.try_trigger_settlement(&operator, &merchant);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

/* ------------------------------------------------------------------ */
/*  Issue #1031: settle_payment interval / minimum enforcement         */
/* ------------------------------------------------------------------ */

#[test]
fn test_settle_payment_blocked_before_daily_interval_elapsed() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, merchant) = setup(&env);

    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USD"),
        &Some(merchant.clone()),
        &None::<String>,
        &MaybeFeeConfig::None,
    );
    merchant_client.verify_merchant(&admin, &merchant);
    merchant_client.set_settlement_schedule(&merchant, &SettlementSchedule::Daily);

    // First settlement cycle: settle a payment and sweep it, which sets
    // `last_settlement_at` on the merchant record.
    create_and_settle(
        &env,
        &admin,
        &payment_client,
        &merchant,
        "PAY_D1_SETTLE",
        2_000_000,
    );
    let operator = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);
    payment_client.trigger_settlement(&operator, &merchant);

    // A second payment settled immediately after should be blocked: the
    // merchant's daily interval has not elapsed since `last_settlement_at`.
    create_and_verify(
        &env,
        &admin,
        &payment_client,
        &merchant,
        "PAY_D2_SETTLE",
        2_000_000,
    );
    let splits = soroban_sdk::vec![
        &env,
        crate::SettlementSplit {
            recipient: merchant.clone(),
            amount: 2_000_000,
        },
    ];
    let result = payment_client.try_settle_payment(
        &operator,
        &String::from_str(&env, "PAY_D2_SETTLE"),
        &splits,
    );
    assert_eq!(result, Err(Ok(Error::SettlementIntervalNotElapsed)));
}

#[test]
fn test_settle_payment_succeeds_after_daily_interval_elapsed() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, merchant) = setup(&env);

    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USD"),
        &Some(merchant.clone()),
        &None::<String>,
        &MaybeFeeConfig::None,
    );
    merchant_client.verify_merchant(&admin, &merchant);
    merchant_client.set_settlement_schedule(&merchant, &SettlementSchedule::Daily);

    create_and_settle(
        &env,
        &admin,
        &payment_client,
        &merchant,
        "PAY_D3_SETTLE",
        2_000_000,
    );
    let operator = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);
    payment_client.trigger_settlement(&operator, &merchant);

    create_and_verify(
        &env,
        &admin,
        &payment_client,
        &merchant,
        "PAY_D4_SETTLE",
        2_000_000,
    );

    // Advance past the daily interval; settlement should now succeed.
    env.ledger()
        .with_mut(|li| li.timestamp += crate::SETTLEMENT_DAILY_INTERVAL_SECS + 1);

    let splits = soroban_sdk::vec![
        &env,
        crate::SettlementSplit {
            recipient: merchant.clone(),
            amount: 2_000_000,
        },
    ];
    let result = payment_client.try_settle_payment(
        &operator,
        &String::from_str(&env, "PAY_D4_SETTLE"),
        &splits,
    );
    assert!(result.is_ok());
}

#[test]
fn test_settle_payment_below_min_rejected_for_scheduled_merchant() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, merchant) = setup(&env);

    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USD"),
        &Some(merchant.clone()),
        &None::<String>,
        &MaybeFeeConfig::None,
    );
    merchant_client.verify_merchant(&admin, &merchant);
    merchant_client.set_settlement_schedule(&merchant, &SettlementSchedule::Daily);

    // No prior settlement, so the interval check is skipped (last_settlement_at
    // is None), but the amount (500) is well below SETTLEMENT_MIN_AMOUNT.
    create_and_verify(
        &env,
        &admin,
        &payment_client,
        &merchant,
        "PAY_D5_SETTLE",
        500,
    );
    let operator = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);
    let splits = soroban_sdk::vec![
        &env,
        crate::SettlementSplit {
            recipient: merchant.clone(),
            amount: 500,
        },
    ];
    let result = payment_client.try_settle_payment(
        &operator,
        &String::from_str(&env, "PAY_D5_SETTLE"),
        &splits,
    );
    assert_eq!(result, Err(Ok(Error::SettlementBelowMinimum)));
}
