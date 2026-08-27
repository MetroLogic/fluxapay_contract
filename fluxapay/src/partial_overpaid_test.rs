use crate::{
    DataKey, Error, PaymentProcessor, PaymentProcessorClient, PaymentStatus, RefundManager,
    RefundManagerClient, RefundStatus,
};
use soroban_sdk::{
    testutils::{Address as _, BytesN as _, Events as _, Ledger as _},
    token, vec, Address, BytesN, Env, String, Symbol,
};

fn setup(
    env: &Env,
) -> (Address, PaymentProcessorClient, RefundManagerClient) {
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
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
    retry_of_payment_id: None,
    payer_muxed_id: None,
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
    assert_eq!(auto_refund_count, 1, "REFUND/AUTO_CREATED must be emitted once");
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
