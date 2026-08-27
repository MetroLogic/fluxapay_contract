use crate::{
    fx_oracle::{FXOracle, FXOracleClient},
    merchant_registry::{MerchantRegistry, MerchantRegistryClient},
    DexRouter, DexRouterClient, PaymentProcessor, PaymentProcessorClient, SwapAndPayArgs,
};
use soroban_sdk::{testutils::Address as _, token, vec, Address, Env, String, Symbol};

use crate::merchant_registry::MaybeFeeConfig;
fn setup_oracle_swap_env(
    env: &Env,
) -> (
    Address,
    PaymentProcessorClient<'_>,
    MerchantRegistryClient<'_>,
    FXOracleClient<'_>,
    Address,
    Address,
    Address,
    DexRouterClient<'_>,
) {
    let payment_processor = env.register(PaymentProcessor, ());
    let merchant_registry = env.register(MerchantRegistry, ());
    let fx_oracle = env.register(FXOracle, ());
    let dex_router = env.register(DexRouter, ());

    let payment_client = PaymentProcessorClient::new(env, &payment_processor);
    let merchant_client = MerchantRegistryClient::new(env, &merchant_registry);
    let oracle_client = FXOracleClient::new(env, &fx_oracle);
    let dex_client = DexRouterClient::new(env, &dex_router);

    let admin = Address::generate(env);
    payment_client.initialize_payment_processor(&admin);
    merchant_client.initialize(&admin);
    payment_client.set_merchant_registry_address(&admin, &merchant_registry);
    oracle_client.oracle_initialize(&admin, &86400);

    let oracle_operator = Address::generate(env);
    oracle_client.oracle_grant_role(&admin, &Symbol::new(env, "ORACLE"), &oracle_operator);

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
        oracle_client,
        fx_oracle,
        token_a,
        token_b,
        dex_client,
    )
}

#[test]
fn test_oracle_sanitization_rejects_deviating_dex_quote() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, oracle_client, fx_oracle, token_a, token_b, _) =
        setup_oracle_swap_env(&env);

    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let dex_router = env.register(DexRouter, ());
    let oracle_operator = Address::generate(&env);
    oracle_client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle_operator);

    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Shop"),
        &String::from_str(&env, "USD"),
        &None,
        &None,
        &MaybeFeeConfig::None);
    merchant_client.verify_merchant(&admin, &merchant);
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);

    let pair = Symbol::new(&env, "USDC_USD");
    // Oracle expects 1:1 (9900 out for 10000 in), but DEX quotes 9900 with 1% slippage.
    // Set oracle rate that expects ~10000 out to force deviation vs DEX quote of 9900.
    oracle_client.set_rate(&oracle_operator, &pair, &10_000i128, &4);

    let path = vec![&env, token_a.clone(), token_b.clone()];
    let args = SwapAndPayArgs {
        payer,
        payment_id: String::from_str(&env, "PAY_ORACLE_01"),
        merchant_id: merchant,
        amount: 9_900,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        token_in: token_a,
        amount_in: 10_000,
        amount_out_min: 9_900,
        path,
        expires_at: Some(env.ledger().timestamp() + 3600),
        dex_router,
        fx_oracle: Some(fx_oracle),
        oracle_pair: Some(pair),
        max_deviation_bps: 50, // 0.5%
    };

    let result = payment_client.try_swap_and_pay(&args);
    assert!(result.is_err());
}

#[test]
fn test_oracle_sanitization_accepts_aligned_dex_quote() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, oracle_client, fx_oracle, token_a, token_b, _) =
        setup_oracle_swap_env(&env);

    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let dex_router = env.register(DexRouter, ());
    let oracle_operator = Address::generate(&env);
    oracle_client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle_operator);

    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Shop"),
        &String::from_str(&env, "USD"),
        &None,
        &None,
        &MaybeFeeConfig::None);
    merchant_client.verify_merchant(&admin, &merchant);
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);

    let pair = Symbol::new(&env, "USDC_USD");
    // Match DEX stub quote: 10000 in -> 9900 out (rate 9900 with 4 decimals)
    oracle_client.set_rate(&oracle_operator, &pair, &9_900i128, &4);

    token::StellarAssetClient::new(&env, &token_a).mint(&payer, &10_000);

    let path = vec![&env, token_a.clone(), token_b.clone()];
    let args = SwapAndPayArgs {
        payer: payer.clone(),
        payment_id: String::from_str(&env, "PAY_ORACLE_02"),
        merchant_id: merchant.clone(),
        amount: 9_900,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        token_in: token_a,
        amount_in: 10_000,
        amount_out_min: 9_900,
        path,
        expires_at: Some(env.ledger().timestamp() + 3600),
        dex_router,
        fx_oracle: Some(fx_oracle),
        oracle_pair: Some(pair),
        max_deviation_bps: 100,
    };

    let payment = payment_client.swap_and_pay(&args);
    assert_eq!(payment.payment_id, String::from_str(&env, "PAY_ORACLE_02"));
}
