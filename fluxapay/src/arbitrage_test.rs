use crate::fx_oracle::{FXOracle, FXOracleClient};
use crate::{
    merchant_registry::{MaybeFeeConfig, MerchantRegistry, MerchantRegistryClient},
    DexRouter, DexRouterClient, PaymentProcessor, PaymentProcessorClient, SwapAndPayArgs,
};
use soroban_sdk::{
    contract, contracterror, contractimpl,
    testutils::{Address as _, Ledger as _},
    token, vec, Address, Env, String, Symbol, Vec,
};

#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SandwichRouterError {
    SwapFailed = 1,
}

#[contract]
pub struct SandwichDexRouter;

#[contractimpl]
impl SandwichDexRouter {
    fn quote(env: &Env, amount_in: i128) -> i128 {
        env.storage()
            .persistent()
            .get(&Symbol::new(env, "quote"))
            .unwrap_or(amount_in)
    }

    fn output(env: &Env, amount_in: i128) -> i128 {
        env.storage()
            .persistent()
            .get(&Symbol::new(env, "output"))
            .unwrap_or(amount_in)
    }

    pub fn get_amounts_out(env: Env, amount_in: i128, path: Vec<Address>) -> Vec<i128> {
        if path.len() < 2 {
            return vec![&env];
        }
        vec![&env, amount_in, Self::quote(&env, amount_in)]
    }

    pub fn swap_exact_tokens_for_tokens(
        env: Env,
        amount_in: i128,
        _amount_out_min: i128,
        path: Vec<Address>,
        _to: Address,
        _deadline: u64,
    ) -> Result<Vec<i128>, SandwichRouterError> {
        if path.len() < 2 {
            return Err(SandwichRouterError::SwapFailed);
        }
        Ok(vec![&env, amount_in, Self::output(&env, amount_in)])
    }
}

fn configure_sandwich_dex(env: &Env, router: &Address, quote: i128, output: i128) {
    env.as_contract(router, || {
        env.storage()
            .persistent()
            .set(&Symbol::new(env, "quote"), &quote);
        env.storage()
            .persistent()
            .set(&Symbol::new(env, "output"), &output);
    });
}

fn prepare_swap_merchant(
    env: &Env,
    admin: &Address,
    payment_client: &PaymentProcessorClient<'_>,
    merchant_client: &MerchantRegistryClient<'_>,
    token_in: &Address,
) -> (Address, Address, Address) {
    let merchant = Address::generate(env);
    let payer = Address::generate(env);
    let deposit_address = Address::generate(env);

    merchant_client.register_merchant(
        &merchant,
        &String::from_str(env, "Shop"),
        &String::from_str(env, "USD"),
        &None,
        &None,
        &MaybeFeeConfig::None,
    );
    merchant_client.verify_merchant(admin, &merchant);
    payment_client.grant_role(admin, &Symbol::new(env, "MERCHANT"), &merchant);
    token::StellarAssetClient::new(env, token_in).mint(&payer, &10_000);
    (merchant, payer, deposit_address)
}

fn build_swap_args(
    env: &Env,
    payment_id: &str,
    merchant: &Address,
    payer: &Address,
    deposit_address: &Address,
    token_in: &Address,
    token_out: &Address,
    dex_router: &Address,
    amount: i128,
    amount_in: i128,
) -> SwapAndPayArgs {
    SwapAndPayArgs {
        payer: payer.clone(),
        payment_id: String::from_str(env, payment_id),
        merchant_id: merchant.clone(),
        amount,
        currency: Symbol::new(env, "USDC"),
        deposit_address: deposit_address.clone(),
        token_in: token_in.clone(),
        amount_in,
        amount_out_min: amount,
        path: vec![env, token_in.clone(), token_out.clone()],
        expires_at: Some(env.ledger().timestamp() + 3_600),
        dex_router: dex_router.clone(),
        fx_oracle: None,
        oracle_pair: None,
        max_deviation_bps: 0,
    }
}

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

#[test]
fn test_swap_and_pay_blocks_sandwich_output_below_default_slippage_floor() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_a, token_b, _) = setup_swap_env(&env);
    let router = env.register(SandwichDexRouter, ());
    configure_sandwich_dex(&env, &router, 10_000, 9_940);
    let (merchant, payer, deposit) =
        prepare_swap_merchant(&env, &admin, &payment_client, &merchant_client, &token_a);
    let args = build_swap_args(
        &env,
        "PAY_SANDWICH_SLIPPAGE",
        &merchant,
        &payer,
        &deposit,
        &token_a,
        &token_b,
        &router,
        9_000,
        10_000,
    );

    let result = payment_client.try_swap_and_pay(&args);
    assert_eq!(result, Err(Ok(crate::Error::InvalidAmount)));
}

#[test]
fn test_swap_and_pay_blocks_sandwich_price_impact_against_oracle() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_a, token_b, _) = setup_swap_env(&env);
    let fx_oracle = env.register(FXOracle, ());
    let oracle_client = FXOracleClient::new(&env, &fx_oracle);
    oracle_client.oracle_initialize(&admin, &86_400);
    let oracle_operator = Address::generate(&env);
    oracle_client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle_operator);
    let pair = Symbol::new(&env, "TOKENA_TOKENB");
    oracle_client.set_rate(&oracle_operator, &pair, &10_100, &4);

    let router = env.register(DexRouter, ());
    let (merchant, payer, deposit) =
        prepare_swap_merchant(&env, &admin, &payment_client, &merchant_client, &token_a);
    let mut args = build_swap_args(
        &env,
        "PAY_SANDWICH_IMPACT",
        &merchant,
        &payer,
        &deposit,
        &token_a,
        &token_b,
        &router,
        9_000,
        10_000,
    );
    args.fx_oracle = Some(fx_oracle);
    args.oracle_pair = Some(pair);
    args.max_deviation_bps = 1_000;

    let result = payment_client.try_swap_and_pay(&args);
    assert_eq!(result, Err(Ok(crate::Error::PriceImpactTooHigh)));
}

#[test]
fn test_swap_and_pay_enforces_per_ledger_volume_cap_and_resets_next_ledger() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_a, token_b, _) = setup_swap_env(&env);
    payment_client.set_swap_limits(&admin, &50, &100, &15_000);
    let router = env.register(DexRouter, ());
    let (merchant, payer, deposit) =
        prepare_swap_merchant(&env, &admin, &payment_client, &merchant_client, &token_a);
    let first = build_swap_args(
        &env,
        "PAY_VOLUME_FIRST",
        &merchant,
        &payer,
        &deposit,
        &token_a,
        &token_b,
        &router,
        9_000,
        10_000,
    );
    payment_client.swap_and_pay(&first);

    let second = build_swap_args(
        &env,
        "PAY_VOLUME_SECOND",
        &merchant,
        &payer,
        &deposit,
        &token_a,
        &token_b,
        &router,
        5_000,
        6_000,
    );
    assert_eq!(
        payment_client.try_swap_and_pay(&second),
        Err(Ok(crate::Error::SwapVolumeLimitExceeded))
    );

    env.ledger()
        .set_sequence(env.ledger().sequence().saturating_add(1));
    assert!(payment_client.try_swap_and_pay(&second).is_ok());
}

#[test]
fn test_set_swap_limits_requires_admin_and_valid_values() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, _, _, _, _) = setup_swap_env(&env);
    let limits = payment_client.get_swap_limits();
    assert_eq!(
        limits.default_max_slippage_bps,
        crate::DEFAULT_MAX_SLIPPAGE_BPS
    );
    assert_eq!(limits.max_price_impact_bps, crate::MAX_PRICE_IMPACT_BPS);
    assert_eq!(
        limits.max_swap_volume_per_ledger,
        crate::MAX_SWAP_VOLUME_PER_LEDGER
    );

    assert_eq!(
        payment_client.try_set_swap_limits(&admin, &10_001, &100, &1_000),
        Err(Ok(crate::Error::InvalidAmount))
    );
    assert_eq!(
        payment_client.try_set_swap_limits(&admin, &50, &100, &0),
        Err(Ok(crate::Error::InvalidAmount))
    );

    let stranger = Address::generate(&env);
    assert_eq!(
        payment_client.try_set_swap_limits(&stranger, &50, &100, &1_000),
        Err(Ok(crate::Error::Unauthorized))
    );
}
