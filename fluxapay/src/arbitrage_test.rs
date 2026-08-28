use crate::{
    merchant_registry::{MaybeFeeConfig, MerchantRegistry, MerchantRegistryClient},
    DexRouter, DexRouterClient, PaymentProcessor, PaymentProcessorClient, SwapAndPayArgs,
};
use soroban_sdk::{testutils::Address as _, token, vec, Address, Env, String, Symbol};

fn setup_swap_env(
    env: &Env,
) -> (
    Address,
    PaymentProcessorClient<'_>,
    MerchantRegistryClient<'_>,
    Address,
    Address,
    DexRouterClient<'_>,
) {
    let payment_processor = env.register(PaymentProcessor, ());
    let merchant_registry = env.register(MerchantRegistry, ());
    let dex_router = env.register(DexRouter, ());

    let payment_client = PaymentProcessorClient::new(env, &payment_processor);
    let merchant_client = MerchantRegistryClient::new(env, &merchant_registry);
    let dex_client = DexRouterClient::new(env, &dex_router);

    let admin = Address::generate(env);
    payment_client.initialize_payment_processor(&admin);
    merchant_client.initialize(&admin);
    payment_client.set_merchant_registry_address(&admin, &merchant_registry);

    let token_admin = Address::generate(env);
    let token_a = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_b = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    payment_client.allow_token(&admin, &token_b);

    (
        admin,
        payment_client,
        merchant_client,
        token_a,
        token_b,
        dex_client,
    )
}

#[test]
fn test_get_amounts_out_returns_path_length_vector() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, _, token_a, token_b, dex_client) = setup_swap_env(&env);

    let path = vec![&env, token_a.clone(), token_b.clone()];
    let amounts = dex_client.get_amounts_out(&10_000i128, &path);

    assert_eq!(amounts.len(), 2);
    assert_eq!(amounts.get(0).unwrap(), 10_000);
    assert_eq!(amounts.get(1).unwrap(), 9_900);
}

#[test]
fn test_validate_path_returns_rejects_circular_path() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_a, token_b, _) = setup_swap_env(&env);

    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let dex_router = env.register(DexRouter, ());

    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Shop"),
        &String::from_str(&env, "USD"),
        &None,
        &None,
        &MaybeFeeConfig::None,
    );
    merchant_client.verify_merchant(&admin, &merchant);
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);

    let circular_path = vec![&env, token_a.clone(), token_b.clone(), token_a.clone()];
    let args = SwapAndPayArgs {
        payer: payer.clone(),
        payment_id: String::from_str(&env, "PAY_ARB_01"),
        merchant_id: merchant,
        amount: 9_000,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        token_in: token_a,
        amount_in: 10_000,
        amount_out_min: 9_000,
        path: circular_path,
        expires_at: Some(env.ledger().timestamp() + 3600),
        dex_router,
        fx_oracle: None,
        oracle_pair: None,
        max_deviation_bps: 0,
    };

    let result = payment_client.try_swap_and_pay(&args);
    assert!(result.is_err());
}

#[test]
fn test_validate_path_returns_rejects_insufficient_quote() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_a, token_b, _) = setup_swap_env(&env);

    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let dex_router = env.register(DexRouter, ());

    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Shop"),
        &String::from_str(&env, "USD"),
        &None,
        &None,
        &MaybeFeeConfig::None,
    );
    merchant_client.verify_merchant(&admin, &merchant);
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);

    let path = vec![&env, token_a.clone(), token_b.clone()];
    let args = SwapAndPayArgs {
        payer,
        payment_id: String::from_str(&env, "PAY_ARB_02"),
        merchant_id: merchant,
        amount: 9_900,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        token_in: token_a,
        amount_in: 10_000,
        amount_out_min: 10_000,
        path,
        expires_at: Some(env.ledger().timestamp() + 3600),
        dex_router,
        fx_oracle: None,
        oracle_pair: None,
        max_deviation_bps: 0,
    };

    let result = payment_client.try_swap_and_pay(&args);
    assert!(result.is_err());
}

#[test]
fn test_swap_and_pay_accepts_valid_path_returns() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_a, token_b, _) = setup_swap_env(&env);

    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let deposit = Address::generate(&env);
    let dex_router = env.register(DexRouter, ());

    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Shop"),
        &String::from_str(&env, "USD"),
        &None,
        &None,
        &MaybeFeeConfig::None,
    );
    merchant_client.verify_merchant(&admin, &merchant);
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);

    token::StellarAssetClient::new(&env, &token_a).mint(&payer, &10_000);

    let path = vec![&env, token_a.clone(), token_b.clone()];
    let args = SwapAndPayArgs {
        payer: payer.clone(),
        payment_id: String::from_str(&env, "PAY_ARB_03"),
        merchant_id: merchant.clone(),
        amount: 9_900,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: deposit.clone(),
        token_in: token_a,
        amount_in: 10_000,
        amount_out_min: 9_900,
        path,
        expires_at: Some(env.ledger().timestamp() + 3600),
        dex_router,
        fx_oracle: None,
        oracle_pair: None,
        max_deviation_bps: 0,
    };

    let payment = payment_client.swap_and_pay(&args);
    assert_eq!(payment.payment_id, String::from_str(&env, "PAY_ARB_03"));
    assert_eq!(payment.amount, 9_900);
}
