//! Issue #1017: merchant rolling reserve deduction on payment confirmation.
//!
//! These tests exercise `MerchantRegistry::set_reserve_policy` /
//! `get_reserve_policy` / `get_locked_reserve`, and the end-to-end flow
//! through `PaymentProcessor::settle_payment` (which withholds the
//! configured `reserve_bps` share into a reserve bucket) and
//! `PaymentProcessor::release_merchant_reserve` (which sweeps matured
//! buckets back into pending settlement once `holding_period_secs` elapses).

use crate::{
    merchant_registry::{
        MaybeFeeConfig, MerchantRegistry, MerchantRegistryClient, RollingReservePolicy,
    },
    CreatePaymentArgs, Error, PaymentProcessor, PaymentProcessorClient, SettlementSplit,
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

fn create_and_settle(
    env: &Env,
    admin: &Address,
    payment_client: &PaymentProcessorClient,
    merchant: &Address,
    payment_id: &str,
    amount: i128,
) {
    payment_client.grant_role(admin, &Symbol::new(env, "MERCHANT"), merchant);
    let args = CreatePaymentArgs {
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

    let operator = Address::generate(env);
    payment_client.grant_role(admin, &Symbol::new(env, "SETTLEMENT_OPERATOR"), &operator);
    let splits = vec![
        env,
        SettlementSplit {
            recipient: merchant.clone(),
            amount,
        },
    ];
    payment_client.settle_payment(&operator, &String::from_str(env, payment_id), &splits);
}

/* ------------------------------------------------------------------ */
/*  Reserve policy configuration                                       */
/* ------------------------------------------------------------------ */

#[test]
fn test_reserve_policy_defaults_to_none() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _payment_client, merchant_client, merchant) = setup(&env);
    register_merchant(&env, &admin, &merchant_client, &merchant);

    assert_eq!(merchant_client.get_reserve_policy(&merchant), None);
    assert_eq!(merchant_client.get_locked_reserve(&merchant), 0);
}

#[test]
fn test_admin_can_set_and_clear_reserve_policy() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _payment_client, merchant_client, merchant) = setup(&env);
    register_merchant(&env, &admin, &merchant_client, &merchant);

    let policy = RollingReservePolicy {
        reserve_bps: 1_000, // 10%
        holding_period_secs: 90 * 24 * 60 * 60,
    };
    merchant_client.set_reserve_policy(&admin, &merchant, &Some(policy.clone()));
    assert_eq!(merchant_client.get_reserve_policy(&merchant), Some(policy));

    merchant_client.set_reserve_policy(&admin, &merchant, &None);
    assert_eq!(merchant_client.get_reserve_policy(&merchant), None);
}

#[test]
fn test_reserve_policy_rejects_out_of_range_bps() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _payment_client, merchant_client, merchant) = setup(&env);
    register_merchant(&env, &admin, &merchant_client, &merchant);

    let policy = RollingReservePolicy {
        reserve_bps: 10_001,
        holding_period_secs: 100,
    };
    let result = merchant_client.try_set_reserve_policy(&admin, &merchant, &Some(policy));
    assert!(result.is_err());
}

#[test]
fn test_non_admin_cannot_set_reserve_policy() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _payment_client, merchant_client, merchant) = setup(&env);
    register_merchant(&env, &admin, &merchant_client, &merchant);

    let not_admin = Address::generate(&env);
    let policy = RollingReservePolicy {
        reserve_bps: 500,
        holding_period_secs: 100,
    };
    let result = merchant_client.try_set_reserve_policy(&not_admin, &merchant, &Some(policy));
    assert!(result.is_err());
}

/* ------------------------------------------------------------------ */
/*  Reserve deduction on settlement                                    */
/* ------------------------------------------------------------------ */

#[test]
fn test_settle_payment_withholds_reserve_share() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, merchant) = setup(&env);
    register_merchant(&env, &admin, &merchant_client, &merchant);

    let policy = RollingReservePolicy {
        reserve_bps: 1_000, // 10%
        holding_period_secs: 90 * 24 * 60 * 60,
    };
    merchant_client.set_reserve_policy(&admin, &merchant, &Some(policy));

    create_and_settle(
        &env,
        &admin,
        &payment_client,
        &merchant,
        "PAY_RESERVE_001",
        1_000_000,
    );

    // 10% of 1_000_000 = 100_000 withheld in reserve; 900_000 immediately pending.
    assert_eq!(merchant_client.get_locked_reserve(&merchant), 100_000);
    assert_eq!(merchant_client.get_pending_settlement(&merchant), 900_000);
}

#[test]
fn test_settle_payment_without_policy_keeps_full_amount_pending() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, merchant) = setup(&env);
    register_merchant(&env, &admin, &merchant_client, &merchant);

    create_and_settle(
        &env,
        &admin,
        &payment_client,
        &merchant,
        "PAY_RESERVE_002",
        1_000_000,
    );

    assert_eq!(merchant_client.get_locked_reserve(&merchant), 0);
    assert_eq!(merchant_client.get_pending_settlement(&merchant), 1_000_000);
}

/* ------------------------------------------------------------------ */
/*  Reserve release after the holding period                          */
/* ------------------------------------------------------------------ */

#[test]
fn test_release_merchant_reserve_before_hold_period_is_noop() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, merchant) = setup(&env);
    register_merchant(&env, &admin, &merchant_client, &merchant);

    let policy = RollingReservePolicy {
        reserve_bps: 1_000,
        holding_period_secs: 90 * 24 * 60 * 60,
    };
    merchant_client.set_reserve_policy(&admin, &merchant, &Some(policy));

    create_and_settle(
        &env,
        &admin,
        &payment_client,
        &merchant,
        "PAY_RESERVE_003",
        1_000_000,
    );

    let operator = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    let released = payment_client.release_merchant_reserve(&operator, &merchant);
    assert_eq!(released, 0);
    assert_eq!(merchant_client.get_locked_reserve(&merchant), 100_000);
}

#[test]
fn test_release_merchant_reserve_after_hold_period_moves_to_pending() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, merchant) = setup(&env);
    register_merchant(&env, &admin, &merchant_client, &merchant);

    let holding_period_secs = 90 * 24 * 60 * 60;
    let policy = RollingReservePolicy {
        reserve_bps: 1_000,
        holding_period_secs,
    };
    merchant_client.set_reserve_policy(&admin, &merchant, &Some(policy));

    create_and_settle(
        &env,
        &admin,
        &payment_client,
        &merchant,
        "PAY_RESERVE_004",
        1_000_000,
    );

    let pending_before = merchant_client.get_pending_settlement(&merchant);
    assert_eq!(pending_before, 900_000);

    env.ledger()
        .with_mut(|li| li.timestamp += holding_period_secs + 1);

    let operator = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);
    let released = payment_client.release_merchant_reserve(&operator, &merchant);

    assert_eq!(released, 100_000);
    assert_eq!(merchant_client.get_locked_reserve(&merchant), 0);
    assert_eq!(merchant_client.get_pending_settlement(&merchant), 1_000_000);
}

#[test]
fn test_release_merchant_reserve_requires_settlement_operator_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, merchant) = setup(&env);
    register_merchant(&env, &admin, &merchant_client, &merchant);

    let caller = Address::generate(&env);
    let result = payment_client.try_release_merchant_reserve(&caller, &merchant);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}
