use crate::{
    merchant_registry::{MaybeFeeConfig, MerchantRegistry, MerchantRegistryClient},
    DataKey, Error, PaymentProcessor, PaymentProcessorClient, PaymentStatus, RefundManager,
    RefundManagerClient, RefundStatus,
};
use soroban_sdk::{
    testutils::{Address as _, BytesN as _, Events as _, Ledger as _},
    token, vec, Address, BytesN, Env, String, Symbol,
};

fn setup(env: &Env) -> (Address, PaymentProcessorClient, RefundManagerClient) {
    let payment_processor = env.register(PaymentProcessor, ());
    let refund_manager = env.register(RefundManager, ());

    let refund_client = RefundManagerClient::new(env, &refund_manager);
    let payment_client = PaymentProcessorClient::new(env, &payment_processor);
    let admin = Address::generate(env);
    let token_admin = Address::generate(env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    refund_client.initialize_refund_manager(&admin, &usdc_token);
    let token_admin_client = token::StellarAssetClient::new(env, &usdc_token);
    token_admin_client.mint(&refund_manager, &1_000_000_000_000i128);

    payment_client.initialize_payment_processor(&admin);

    (admin, payment_client, refund_client)
}

fn create_and_verify(
    env: &Env,
    payment_client: &PaymentProcessorClient,
    merchant: &Address,
    payment_id: &str,
    amount: i128,
    amount_received: i128,
) -> PaymentStatus {
    payment_client.grant_role(env, &Symbol::new(env, "MERCHANT"), merchant);
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
    payment_client.grant_role(env, &Symbol::new(env, "ORACLE"), &oracle);
    payment_client.verify_payment(
        oracle,
        &String::from_str(env, payment_id),
        &tx_hash,
        &Address::generate(env),
        &amount_received,
    )
}

fn count_event(env: &Env, namespace: &str, name: &str) -> usize {
    env.events()
        .all()
        .iter()
        .filter(|e| {
            let topics = e.0.clone();
            topics.len() >= 2
                && topics
                    .get(0)
                    .and_then(|t| t.try_into_val::<Symbol>(env).ok())
                    == Some(Symbol::new(env, namespace))
                && topics
                    .get(1)
                    .and_then(|t| t.try_into_val::<Symbol>(env).ok())
                    == Some(Symbol::new(env, name))
        })
        .count()
}

/* ------------------------------------------------------------------ */
/*  Overpaid auto-refund                                               */
/* ------------------------------------------------------------------ */

#[test]
fn test_overpaid_auto_creates_refund() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, payment_client, refund_client) = setup(&env);
    let merchant = Address::generate(&env);

    create_and_verify(&env, &payment_client, &merchant, "PAY_OVER_001", 1000, 1500);

    let payment = payment_client.get_payment(&String::from_str(&env, "PAY_OVER_001"));
    assert_eq!(payment.status, PaymentStatus::Overpaid);
    assert_eq!(payment.amount_received, Some(1500));

    let auto_refund_count = count_event(&env, "REFUND", "AUTO_CREATED");
    assert_eq!(
        auto_refund_count, 1,
        "REFUND/AUTO_CREATED must be emitted once"
    );
}

#[test]
fn test_overpaid_payment_keeps_overpaid_status() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, payment_client, _refund_client) = setup(&env);
    let merchant = Address::generate(&env);

    let status = create_and_verify(&env, &payment_client, &merchant, "PAY_OVER_002", 1000, 1200);
    assert_eq!(status, PaymentStatus::Overpaid);
}

#[test]
fn test_auto_refund_disabled_skips_refund() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, _refund_client) = setup(&env);
    let merchant = Address::generate(&env);

    payment_client.set_auto_refund_overpayment(&admin, &false);

    create_and_verify(&env, &payment_client, &merchant, "PAY_OVER_003", 1000, 1100);

    let auto_refund_count = count_event(&env, "REFUND", "AUTO_CREATED");
    assert_eq!(auto_refund_count, 0);
}

#[test]
fn test_auto_refund_default_true() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, payment_client, _refund_client) = setup(&env);

    let enabled = payment_client.get_auto_refund_overpayment();
    assert!(enabled);
}

/* ------------------------------------------------------------------ */
/*  PartiallyPaid events                                               */
/* ------------------------------------------------------------------ */

#[test]
fn test_partially_paid_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, payment_client, _refund_client) = setup(&env);
    let merchant = Address::generate(&env);

    create_and_verify(&env, &payment_client, &merchant, "PAY_PART_001", 1000, 500);

    let partial_count = count_event(&env, "PAYMENT", "PARTIALLY_PAID");
    assert_eq!(partial_count, 1, "PAYMENT/PARTIALLY_PAID must be emitted");
}

/* ------------------------------------------------------------------ */
/*  Overpaid events                                                    */
/* ------------------------------------------------------------------ */

#[test]
fn test_overpaid_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, payment_client, _refund_client) = setup(&env);
    let merchant = Address::generate(&env);

    create_and_verify(&env, &payment_client, &merchant, "PAY_OVER_EVT", 1000, 1500);

    let overpaid_count = count_event(&env, "PAYMENT", "OVERPAID");
    assert_eq!(overpaid_count, 1, "PAYMENT/OVERPAID must be emitted");
}

#[test]
fn test_partial_payment_accumulation_multi_tx() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, payment_client, _) = setup(&env);
    let merchant = Address::generate(&env);
    let oracle = Address::generate(&env);
    let payer = Address::generate(&env);

    payment_client.grant_role(&Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.grant_role(&Symbol::new(&env, "ORACLE"), &oracle);

    let payment_id = String::from_str(&env, "PAY_PARTIAL_ACCUM");
    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: Some(payer.clone()),
        amount: 1000,
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
        allow_partial: Some(true),
    };
    payment_client.create_payment(&args);

    // First partial contribution: 400 of 1000
    let status1 = payment_client.confirm_payment(
        &oracle,
        &payment_id,
        &BytesN::random(&env),
        &payer,
        &400,
        &None,
    );
    assert_eq!(status1, PaymentStatus::PartiallyPaid);
    let p1 = payment_client.get_payment(&payment_id);
    assert_eq!(p1.amount_received, Some(400));
    assert_eq!(p1.status, PaymentStatus::PartiallyPaid);

    // Second contribution: 600 of 1000 (total 1000) -> Confirmed
    let status2 = payment_client.confirm_payment(
        &oracle,
        &payment_id,
        &BytesN::random(&env),
        &payer,
        &600,
        &None,
    );
    assert_eq!(status2, PaymentStatus::Confirmed);
    let p2 = payment_client.get_payment(&payment_id);
    assert_eq!(p2.amount_received, Some(1000));
    assert_eq!(p2.status, PaymentStatus::Confirmed);
}

#[test]
fn test_partial_payment_expiry_refund() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, payment_client, _) = setup(&env);
    let merchant = Address::generate(&env);
    let oracle = Address::generate(&env);
    let payer = Address::generate(&env);

    payment_client.grant_role(&Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.grant_role(&Symbol::new(&env, "ORACLE"), &oracle);

    let payment_id = String::from_str(&env, "PAY_PARTIAL_EXPIRE");
    let expiry_ts = env.ledger().timestamp() + 3600;
    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: Some(payer.clone()),
        amount: 1000,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(expiry_ts),
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
        allow_partial: Some(true),
    };
    payment_client.create_payment(&args);

    // Partially fund 300
    payment_client.confirm_payment(
        &oracle,
        &payment_id,
        &BytesN::random(&env),
        &payer,
        &300,
        &None,
    );

    // Advance time past expiry
    env.ledger().with_mut(|li| li.timestamp = expiry_ts + 1);

    // Expire payment
    assert!(payment_client.try_expire_payment(&payment_id).is_ok());
    let payment = payment_client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Expired);
}

/// Issue #1018: With the merchant registry wired up (so `expire_payment` can
/// reach `RefundManager`), an expired partially-funded payment must
/// automatically create a real, claimable `Refund` record for the
/// accumulated partial amount, and emit `PAYMENT/PARTIAL_REFUNDED`.
#[test]
fn test_partial_payment_expiry_auto_creates_refund() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, refund_client) = setup(&env);

    let merchant_registry = env.register(MerchantRegistry, ());
    let merchant_client = MerchantRegistryClient::new(&env, &merchant_registry);
    merchant_client.initialize(&admin);
    payment_client.set_merchant_registry_address(&admin, &merchant_registry);
    merchant_client.set_refund_manager_address(&admin, &refund_client.address);

    let merchant = Address::generate(&env);
    let oracle = Address::generate(&env);
    let payer = Address::generate(&env);

    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USD"),
        &Some(merchant.clone()),
        &None::<String>,
        &MaybeFeeConfig::None,
    );
    merchant_client.verify_merchant(&admin, &merchant);

    payment_client.grant_role(&Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.grant_role(&Symbol::new(&env, "ORACLE"), &oracle);

    let payment_id = String::from_str(&env, "PAY_PARTIAL_AUTO_REFUND");
    let expiry_ts = env.ledger().timestamp() + 3600;
    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: Some(payer.clone()),
        amount: 1000,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(expiry_ts),
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
        allow_partial: Some(true),
    };
    payment_client.create_payment(&args);

    // Partially fund 300 of 1000.
    payment_client.confirm_payment(
        &oracle,
        &payment_id,
        &BytesN::random(&env),
        &payer,
        &300,
        &None,
    );

    env.ledger().with_mut(|li| li.timestamp = expiry_ts + 1);
    assert!(payment_client.try_expire_payment(&payment_id).is_ok());

    let payment = payment_client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Expired);

    let refunds = refund_client.get_payment_refunds(&payment_id);
    assert_eq!(refunds.len(), 1, "expiry must auto-create exactly one refund");
    let refund = refunds.get(0).unwrap();
    assert_eq!(refund.amount, 300);
    assert_eq!(refund.status, RefundStatus::Pending);
    assert_eq!(refund.requester, payer);

    let partial_refunded_count = count_event(&env, "PAYMENT", "PARTIAL_REFUNDED");
    assert_eq!(
        partial_refunded_count, 1,
        "PAYMENT/PARTIAL_REFUNDED must be emitted once"
    );
}

#[test]
fn test_bump_payment_ttl() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, payment_client, _) = setup(&env);
    let merchant = Address::generate(&env);

    payment_client.grant_role(&Symbol::new(&env, "MERCHANT"), &merchant);
    let payment_id = String::from_str(&env, "PAY_TTL_TEST");
    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 100,
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
        allow_partial: None,
    };
    payment_client.create_payment(&args);

    // Permissionless bump_payment_ttl call succeeds
    assert!(payment_client.try_bump_payment_ttl(&payment_id).is_ok());
}
