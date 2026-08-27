use crate::{
    merchant_registry::{MerchantRegistry, MerchantRegistryClient},
    PaymentProcessor, PaymentProcessorClient,
};
use soroban_sdk::{
    testutils::{Address as _, BytesN as _},
    Address, BytesN, Env, String, Symbol,
};

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
fn test_create_payment_with_g_address_payer_muxed_id_is_none() {
    let env = Env::default();
    let (_admin, _processor_addr, payment_client, merchant_client) =
        setup_payment_processor_with_registry(&env);
    let (_oracle, merchant) = setup_oracle_and_merchant(&env, &_admin, &payment_client, &merchant_client);

    let payment_id = String::from_str(&env, "pay_test_001");
    let deposit_addr = Address::generate(&env);

    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 100_000_000i128,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: deposit_addr,
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

    let payment = payment_client.try_create_payment(&args);
    assert!(payment.is_ok());
    let charge = payment.unwrap();
    assert_eq!(charge.payer_muxed_id, None);
}

#[test]
fn test_verify_payment_with_muxed_sender_populates_muxed_id() {
    let env = Env::default();
    let (_admin, _processor_addr, payment_client, merchant_client) =
        setup_payment_processor_with_registry(&env);
    let (oracle, merchant) = setup_oracle_and_merchant(&env, &_admin, &payment_client, &merchant_client);

    let payment_id = String::from_str(&env, "pay_test_mux_001");
    let deposit_addr = Address::generate(&env);
    let payer = Address::generate(&env);
    let muxed_id: u64 = 12345;

    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 100_000_000i128,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: deposit_addr,
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

    let tx_hash = BytesN::<32>::random(&env);
    let result = payment_client.try_verify_payment(
        &oracle,
        &payment_id,
        &tx_hash,
        &payer,
        &100_000_000i128,
        &Some(muxed_id),
    );

    assert!(result.is_ok());
    let payment = payment_client.get_payment(&payment_id);
    assert_eq!(payment.payer_muxed_id, Some(muxed_id));
}

#[test]
fn test_verify_payment_without_muxed_id_remains_none() {
    let env = Env::default();
    let (_admin, _processor_addr, payment_client, merchant_client) =
        setup_payment_processor_with_registry(&env);
    let (oracle, merchant) = setup_oracle_and_merchant(&env, &_admin, &payment_client, &merchant_client);

    let payment_id = String::from_str(&env, "pay_test_nomux_001");
    let deposit_addr = Address::generate(&env);
    let payer = Address::generate(&env);

    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 100_000_000i128,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: deposit_addr,
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

    let tx_hash = BytesN::<32>::random(&env);
    let result = payment_client.try_verify_payment(
        &oracle,
        &payment_id,
        &tx_hash,
        &payer,
        &100_000_000i128,
        &None,
    );

    assert!(result.is_ok());
    let payment = payment_client.get_payment(&payment_id);
    assert_eq!(payment.payer_muxed_id, None);
}

#[test]
fn test_muxed_payer_auth_not_checked_in_create_or_verify() {
    let env = Env::default();
    let (_admin, _processor_addr, payment_client, merchant_client) =
        setup_payment_processor_with_registry(&env);
    let (oracle, merchant) = setup_oracle_and_merchant(&env, &_admin, &payment_client, &merchant_client);

    let payment_id = String::from_str(&env, "pay_test_auth_001");
    let deposit_addr = Address::generate(&env);
    let payer = Address::generate(&env);
    let muxed_id: u64 = 99999;

    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 50_000_000i128,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: deposit_addr,
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

    let tx_hash = BytesN::<32>::random(&env);
    let result = payment_client.try_verify_payment(
        &oracle,
        &payment_id,
        &tx_hash,
        &payer,
        &50_000_000i128,
        &Some(muxed_id),
    );

    assert!(result.is_ok());
}
