use crate::merchant_registry::MaybeFeeConfig;
use crate::{
    merchant_registry::{MerchantRegistry, MerchantRegistryClient},
    mock_dex_router::{configure_mock_dex, MockDexRouter, MockDexRouterClient},
    Error, PaymentProcessor, PaymentProcessorClient, PaymentStatus, SwapAndPayArgs,
    SwapAndPayArgsV2, SwapRoute, SwapSplit, ZERO_CONTRACT_STRKEY,
};
use soroban_sdk::{
    contract, contractimpl, testutils::Address as _, testutils::Events, token, vec, Address, Env,
    String, Symbol, Vec,
};

#[contract]
pub struct MockWrappedXlm;

#[contractimpl]
impl MockWrappedXlm {
    pub fn configure(env: Env, fail: bool) {
        env.storage()
            .persistent()
            .set(&Symbol::new(&env, "fail_wrap"), &fail);
    }

    pub fn wrap(env: Env, from: Address, amount: i128, _recipient: Address) {
        if env
            .storage()
            .persistent()
            .get::<Symbol, bool>(&Symbol::new(&env, "fail_wrap"))
            .unwrap_or(false)
        {
            panic!("mock wrap failed");
        }
        env.storage()
            .persistent()
            .set(&Symbol::new(&env, "last_wrap"), &(from, amount));
    }

    pub fn last_wrap(env: Env) -> Option<(Address, i128)> {
        env.storage()
            .persistent()
            .get(&Symbol::new(&env, "last_wrap"))
    }
}

fn configure_mock_wxlm(env: &Env, wxlm: &Address, fail: bool) {
    MockWrappedXlmClient::new(env, wxlm).configure(&fail);
}

fn last_mock_wrap(env: &Env, wxlm: &Address) -> Option<(Address, i128)> {
    MockWrappedXlmClient::new(env, wxlm).last_wrap()
}

fn setup_swap_test_env(
    env: &Env,
) -> (
    Address,
    PaymentProcessorClient<'_>,
    MerchantRegistryClient<'_>,
    Address,
    Address,
) {
    let payment_processor = env.register(PaymentProcessor, ());
    let merchant_registry = env.register(MerchantRegistry, ());

    let payment_client = PaymentProcessorClient::new(env, &payment_processor);
    let merchant_client = MerchantRegistryClient::new(env, &merchant_registry);

    let admin = Address::generate(env);
    payment_client.initialize_payment_processor(&admin);
    merchant_client.initialize(&admin);
    payment_client.set_merchant_registry_address(&admin, &merchant_registry);

    let token_admin = Address::generate(env);
    let token_in = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_out = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    payment_client.allow_token(&admin, &token_out);

    (admin, payment_client, merchant_client, token_in, token_out)
}

fn mint_swap_input(env: &Env, token_in: &Address, payer: &Address, amount: i128) {
    let token_client = token::StellarAssetClient::new(env, token_in);
    token_client.mint(payer, &amount);
}

fn register_verified_merchant(
    env: &Env,
    admin: &Address,
    payment_client: &PaymentProcessorClient<'_>,
    merchant_client: &MerchantRegistryClient<'_>,
) -> Address {
    let merchant = Address::generate(env);
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(env, "TestShop"),
        &String::from_str(env, "USD"),
        &None,
        &None,
        &MaybeFeeConfig::None,
    );
    merchant_client.verify_merchant(admin, &merchant);
    payment_client.grant_role(admin, &Symbol::new(env, "MERCHANT"), &merchant);
    merchant
}

fn build_swap_args(
    env: &Env,
    payment_id: &str,
    payer: &Address,
    merchant: &Address,
    deposit_address: &Address,
    token_in: &Address,
    token_out: &Address,
    mock_dex: &Address,
    amount: i128,
    amount_in: i128,
) -> SwapAndPayArgs {
    let path = vec![env, token_in.clone(), token_out.clone()];
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
        path,
        expires_at: Some(env.ledger().timestamp() + 3600),
        dex_router: mock_dex.clone(),
        fx_oracle: None,
        oracle_pair: None,
        max_deviation_bps: 0,
    }
}

fn build_swap_v2_args(
    env: &Env,
    payment_id: &str,
    payer: &Address,
    merchant: &Address,
    deposit_address: &Address,
    token_in: &Address,
    splits: Vec<SwapSplit>,
    min_output: i128,
) -> SwapAndPayArgsV2 {
    SwapAndPayArgsV2 {
        payer: payer.clone(),
        payment_id: String::from_str(env, payment_id),
        merchant_id: merchant.clone(),
        currency: Symbol::new(env, "USDC"),
        deposit_address: deposit_address.clone(),
        token_in: token_in.clone(),
        expires_at: Some(env.ledger().timestamp() + 3600),
        fx_oracle: None,
        oracle_pair: None,
        max_deviation_bps: 0,
        splits,
        min_output,
    }
}

#[test]
fn test_swap_and_pay_happy_path_with_mock_dex() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_in, token_out) = setup_swap_test_env(&env);

    let mock_dex = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &mock_dex, 10_000, false);

    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let payer = Address::generate(&env);
    let deposit_address = Address::generate(&env);
    let payment_id = String::from_str(&env, "SWAP_TEST_001");

    let args = build_swap_args(
        &env,
        "SWAP_TEST_001",
        &payer,
        &merchant,
        &deposit_address,
        &token_in,
        &token_out,
        &mock_dex,
        9_900,
        10_000,
    );
    mint_swap_input(&env, &token_in, &payer, 10_000);
    payment_client.add_approved_router(&admin, &mock_dex);

    let events_before = env.events().all().len();
    let payment = payment_client.swap_and_pay(&args);
    let events = env.events().all();
    assert!(
        events.len() > events_before,
        "swap_and_pay should emit events"
    );

    assert_eq!(payment.payment_id, payment_id);
    assert_eq!(payment.amount, 9_900);
    assert_eq!(payment.status, PaymentStatus::Pending);

    let stored = payment_client.get_payment(&payment_id);
    assert_eq!(stored.amount, 9_900);
    assert_eq!(stored.original_token, Some(token_in));
    assert!(stored.swap_path.is_some());

    assert!(
        events.len() > events_before,
        "swap_and_pay should emit contract events"
    );
    assert!(
        events
            .iter()
            .any(|(_, topics, _)| topics.len() == 2 || topics.len() == 3),
        "expected structured payment events after swap_and_pay"
    );
}

#[test]
fn test_swap_and_pay_failure_with_mock_dex_insufficient_output() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_in, token_out) = setup_swap_test_env(&env);

    let mock_dex = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &mock_dex, 8_000, false);

    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let payer = Address::generate(&env);
    let deposit_address = Address::generate(&env);
    let payment_id = String::from_str(&env, "SWAP_TEST_002");

    let args = build_swap_args(
        &env,
        "SWAP_TEST_002",
        &payer,
        &merchant,
        &deposit_address,
        &token_in,
        &token_out,
        &mock_dex,
        9_900,
        10_000,
    );
    mint_swap_input(&env, &token_in, &payer, 10_000);
    payment_client.add_approved_router(&admin, &mock_dex);

    let result = payment_client.try_swap_and_pay(&args);
    assert!(result.is_err(), "expected swap_and_pay to fail");
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
    assert!(
        payment_client.try_get_payment(&payment_id).is_err(),
        "payment should not be stored when swap fails"
    );
}

#[test]
fn test_swap_and_pay_failure_when_mock_dex_swap_errors() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_in, token_out) = setup_swap_test_env(&env);

    let mock_dex = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &mock_dex, 10_000, true);

    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let payer = Address::generate(&env);
    let deposit_address = Address::generate(&env);
    let payment_id = String::from_str(&env, "SWAP_TEST_003");

    let args = build_swap_args(
        &env,
        "SWAP_TEST_003",
        &payer,
        &merchant,
        &deposit_address,
        &token_in,
        &token_out,
        &mock_dex,
        9_900,
        10_000,
    );
    mint_swap_input(&env, &token_in, &payer, 10_000);
    payment_client.add_approved_router(&admin, &mock_dex);

    let result = payment_client.try_swap_and_pay(&args);
    assert!(
        result.is_err(),
        "expected swap_and_pay to propagate DEX error"
    );
    assert!(
        payment_client.try_get_payment(&payment_id).is_err(),
        "payment should not be stored when DEX swap errors"
    );
}

#[test]
fn test_swap_and_pay_rejects_invalid_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_in, token_out) = setup_swap_test_env(&env);

    let mock_dex = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &mock_dex, 10_000, false);

    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let payer = Address::generate(&env);
    let deposit_address = Address::generate(&env);

    let mut zero_amount_args = build_swap_args(
        &env,
        "SWAP_TEST_ZERO_AMOUNT",
        &payer,
        &merchant,
        &deposit_address,
        &token_in,
        &token_out,
        &mock_dex,
        0,
        10_000,
    );
    let zero_amount_result = payment_client.try_swap_and_pay(&zero_amount_args);
    assert_eq!(zero_amount_result, Err(Ok(Error::InvalidAmount)));
    assert!(payment_client
        .try_get_payment(&zero_amount_args.payment_id)
        .is_err());

    zero_amount_args.payment_id = String::from_str(&env, "SWAP_TEST_ZERO_AMOUNT_IN");
    zero_amount_args.amount = 9_900;
    zero_amount_args.amount_in = 0;
    let zero_amount_in_result = payment_client.try_swap_and_pay(&zero_amount_args);
    assert_eq!(zero_amount_in_result, Err(Ok(Error::InvalidAmount)));
    assert!(payment_client
        .try_get_payment(&zero_amount_args.payment_id)
        .is_err());
}

#[test]
fn test_dex_router_allowlist() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_in, token_out) = setup_swap_test_env(&env);

    let mock_dex = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &mock_dex, 10_000, false);

    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let payer = Address::generate(&env);
    let deposit_address = Address::generate(&env);

    assert!(payment_client.get_approved_routers().is_empty());
    assert!(!payment_client.is_router_allowed(&mock_dex));
    assert_eq!(payment_client.get_approved_routers().len(), 0);

    let unapproved_args = build_swap_args(
        &env,
        "SWAP_UNAPPROVED_ROUTER",
        &payer,
        &merchant,
        &deposit_address,
        &token_in,
        &token_out,
        &mock_dex,
        9_900,
        10_000,
    );

    let res = payment_client.try_swap_and_pay(&unapproved_args);
    assert_eq!(res, Err(Ok(Error::UnapprovedRouter)));

    payment_client.add_approved_router(&admin, &mock_dex);
    mint_swap_input(&env, &token_in, &payer, 10_000);
    let approved_res = payment_client.try_swap_and_pay(&unapproved_args);
    assert!(approved_res.is_ok());

    payment_client.remove_approved_router(&admin, &mock_dex);
    assert!(!payment_client.is_router_allowed(&mock_dex));
    let removed_res = payment_client.try_swap_and_pay(&unapproved_args);
    assert_eq!(removed_res, Err(Ok(Error::UnapprovedRouter)));
}

#[test]
fn test_xlm_auto_wrapping_and_wrapped_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, _token_in, token_out) = setup_swap_test_env(&env);

    let wxlm = env.register(MockWrappedXlm, ());
    configure_mock_wxlm(&env, &wxlm, false);
    payment_client.set_wrapped_xlm_contract(&admin, &wxlm);
    assert_eq!(
        payment_client.get_wrapped_xlm_contract(),
        Some(wxlm.clone())
    );

    let mock_dex = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &mock_dex, 10_000, false);

    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let payer = Address::generate(&env);
    let deposit_address = Address::generate(&env);
    let native_xlm = Address::from_str(&env, ZERO_CONTRACT_STRKEY);
    payment_client.set_native_xlm_contract(&admin, &native_xlm);

    let xlm_args = build_swap_args(
        &env,
        "XLM_AUTO_WRAP_TEST",
        &payer,
        &merchant,
        &deposit_address,
        &native_xlm,
        &token_out,
        &mock_dex,
        9_900,
        10_000,
    );
    payment_client.add_approved_router(&admin, &mock_dex);

    let payment = payment_client.swap_and_pay(&xlm_args);
    assert_eq!(payment.original_token, Some(native_xlm));
    assert_eq!(
        payment.swap_path.as_ref().and_then(|path| path.get(0)),
        Some(native_xlm)
    );
    assert_eq!(last_mock_wrap(&env, &wxlm), Some((payer, 10_000)));
    assert_eq!(
        MockDexRouterClient::new(&env, &mock_dex)
            .last_swap()
            .map(|(amount, path)| (amount, path.get(0))),
        Some((10_000, Some(wxlm)))
    );
}

#[test]
fn test_swap_and_pay_non_xlm_does_not_invoke_wrapper() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_in, token_out) = setup_swap_test_env(&env);
    let wxlm = env.register(MockWrappedXlm, ());
    configure_mock_wxlm(&env, &wxlm, false);
    payment_client.set_wrapped_xlm_contract(&admin, &wxlm);

    let router = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &router, 10_000, false);
    payment_client.add_approved_router(&admin, &router);
    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let payer = Address::generate(&env);
    mint_swap_input(&env, &token_in, &payer, 10_000);
    let args = build_swap_args(
        &env,
        "SWAP_NON_XLM_NO_WRAP",
        &payer,
        &merchant,
        &Address::generate(&env),
        &token_in,
        &token_out,
        &router,
        9_900,
        10_000,
    );

    assert!(payment_client.try_swap_and_pay(&args).is_ok());
    assert_eq!(last_mock_wrap(&env, &wxlm), None);
    assert_eq!(
        MockDexRouterClient::new(&env, &router)
            .last_swap()
            .map(|(amount, path)| (amount, path.get(0))),
        Some((10_000, Some(token_in)))
    );
}

#[test]
fn test_swap_and_pay_wrap_failure_reverts_payment() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, _token_in, token_out) = setup_swap_test_env(&env);
    let wxlm = env.register(MockWrappedXlm, ());
    configure_mock_wxlm(&env, &wxlm, true);
    payment_client.set_wrapped_xlm_contract(&admin, &wxlm);

    let router = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &router, 10_000, false);
    payment_client.add_approved_router(&admin, &router);
    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let payer = Address::generate(&env);
    let native_xlm = Address::from_str(&env, ZERO_CONTRACT_STRKEY);
    payment_client.set_native_xlm_contract(&admin, &native_xlm);
    let args = build_swap_args(
        &env,
        "XLM_WRAP_FAILURE",
        &payer,
        &merchant,
        &Address::generate(&env),
        &native_xlm,
        &token_out,
        &router,
        9_900,
        10_000,
    );

    assert!(payment_client.try_swap_and_pay(&args).is_err());
    assert!(payment_client.try_get_payment(&args.payment_id).is_err());
}

#[test]
fn test_swap_and_pay_multi_route_aggregation() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_in, token_out) = setup_swap_test_env(&env);

    let mock_dex1 = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &mock_dex1, 5_000, false);

    let mock_dex2 = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &mock_dex2, 5_000, false);

    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let payer = Address::generate(&env);
    let deposit_address = Address::generate(&env);

    let args = build_swap_args(
        &env,
        "MULTI_ROUTE_PAYMENT",
        &payer,
        &merchant,
        &deposit_address,
        &token_in,
        &token_out,
        &mock_dex1,
        9_900,
        10_000,
    );

    let route1 = SwapRoute {
        router: mock_dex1.clone(),
        path: vec![&env, token_in.clone(), token_out.clone()],
        amount_in: 5_000,
    };
    let route2 = SwapRoute {
        router: mock_dex2.clone(),
        path: vec![&env, token_in.clone(), token_out.clone()],
        amount_in: 5_000,
    };
    payment_client.add_approved_router(&admin, &mock_dex1);
    payment_client.add_approved_router(&admin, &mock_dex2);
    let routes = vec![&env, route1, route2];

    let payment = payment_client.swap_and_pay_multi_route(&args, &routes, &9_900);
    assert_eq!(
        payment.payment_id,
        String::from_str(&env, "MULTI_ROUTE_PAYMENT")
    );
    assert_eq!(payment.amount, 9_900);
}

#[test]
fn test_swap_and_pay_v2_single_split_matches_single_router_flow() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_in, token_out) = setup_swap_test_env(&env);
    let router = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &router, 10_000, false);
    payment_client.add_approved_router(&admin, &router);

    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let payer = Address::generate(&env);
    let deposit_address = Address::generate(&env);
    mint_swap_input(&env, &token_in, &payer, 10_000);
    let splits = vec![
        &env,
        SwapSplit {
            router,
            amount: 10_000,
            path: vec![&env, token_in.clone(), token_out],
        },
    ];
    let args = build_swap_v2_args(
        &env,
        "SWAP_V2_SINGLE",
        &payer,
        &merchant,
        &deposit_address,
        &token_in,
        splits,
        9_900,
    );

    let payment = payment_client.swap_and_pay_v2(&args);
    assert_eq!(payment.payment_id, String::from_str(&env, "SWAP_V2_SINGLE"));
    assert_eq!(payment.amount, 10_000);
}

#[test]
fn test_swap_and_pay_v2_aggregates_two_splits() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_in, token_out) = setup_swap_test_env(&env);
    let router_a = env.register(MockDexRouter, ());
    let router_b = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &router_a, 5_000, false);
    configure_mock_dex(&env, &router_b, 6_000, false);
    payment_client.add_approved_router(&admin, &router_a);
    payment_client.add_approved_router(&admin, &router_b);

    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let payer = Address::generate(&env);
    let deposit_address = Address::generate(&env);
    mint_swap_input(&env, &token_in, &payer, 10_000);
    let splits = vec![
        &env,
        SwapSplit {
            router: router_a,
            amount: 5_000,
            path: vec![&env, token_in.clone(), token_out.clone()],
        },
        SwapSplit {
            router: router_b,
            amount: 5_000,
            path: vec![&env, token_in.clone(), token_out],
        },
    ];
    let args = build_swap_v2_args(
        &env,
        "SWAP_V2_TWO",
        &payer,
        &merchant,
        &deposit_address,
        &token_in,
        splits,
        10_000,
    );

    let payment = payment_client.swap_and_pay_v2(&args);
    assert_eq!(payment.amount, 11_000);
}

#[test]
fn test_swap_and_pay_v2_aggregates_three_splits() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_in, token_out) = setup_swap_test_env(&env);
    let routers = [
        env.register(MockDexRouter, ()),
        env.register(MockDexRouter, ()),
        env.register(MockDexRouter, ()),
    ];
    for (router, output) in routers.iter().zip([3_000, 3_500, 4_000]) {
        configure_mock_dex(&env, router, output, false);
        payment_client.add_approved_router(&admin, router);
    }

    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let payer = Address::generate(&env);
    let deposit_address = Address::generate(&env);
    mint_swap_input(&env, &token_in, &payer, 15_000);
    let splits = vec![
        &env,
        SwapSplit {
            router: routers[0].clone(),
            amount: 5_000,
            path: vec![&env, token_in.clone(), token_out.clone()],
        },
        SwapSplit {
            router: routers[1].clone(),
            amount: 5_000,
            path: vec![&env, token_in.clone(), token_out.clone()],
        },
        SwapSplit {
            router: routers[2].clone(),
            amount: 5_000,
            path: vec![&env, token_in.clone(), token_out],
        },
    ];
    let args = build_swap_v2_args(
        &env,
        "SWAP_V2_THREE",
        &payer,
        &merchant,
        &deposit_address,
        &token_in,
        splits,
        10_000,
    );

    let payment = payment_client.swap_and_pay_v2(&args);
    assert_eq!(payment.amount, 10_500);
}

#[test]
fn test_swap_and_pay_v2_rejects_output_below_minimum() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_in, token_out) = setup_swap_test_env(&env);
    let router_a = env.register(MockDexRouter, ());
    let router_b = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &router_a, 5_000, false);
    configure_mock_dex(&env, &router_b, 5_000, false);
    payment_client.add_approved_router(&admin, &router_a);
    payment_client.add_approved_router(&admin, &router_b);

    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let payer = Address::generate(&env);
    let deposit_address = Address::generate(&env);
    let payment_id = String::from_str(&env, "SWAP_V2_SLIPPAGE");
    let splits = vec![
        &env,
        SwapSplit {
            router: router_a,
            amount: 5_000,
            path: vec![&env, token_in.clone(), token_out.clone()],
        },
        SwapSplit {
            router: router_b,
            amount: 5_000,
            path: vec![&env, token_in.clone(), token_out],
        },
    ];
    let args = build_swap_v2_args(
        &env,
        "SWAP_V2_SLIPPAGE",
        &payer,
        &merchant,
        &deposit_address,
        &token_in,
        splits,
        10_001,
    );

    let result = payment_client.try_swap_and_pay_v2(&args);
    assert_eq!(result, Err(Ok(Error::RouteOutputInsufficient)));
    assert!(payment_client.try_get_payment(&payment_id).is_err());
}

#[test]
fn test_swap_and_pay_v2_reverts_when_a_split_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_in, token_out) = setup_swap_test_env(&env);
    let router_a = env.register(MockDexRouter, ());
    let router_b = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &router_a, 5_000, false);
    configure_mock_dex(&env, &router_b, 5_000, true);
    payment_client.add_approved_router(&admin, &router_a);
    payment_client.add_approved_router(&admin, &router_b);

    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let payer = Address::generate(&env);
    let splits = vec![
        &env,
        SwapSplit {
            router: router_a,
            amount: 5_000,
            path: vec![&env, token_in.clone(), token_out.clone()],
        },
        SwapSplit {
            router: router_b,
            amount: 5_000,
            path: vec![&env, token_in.clone(), token_out],
        },
    ];
    let args = build_swap_v2_args(
        &env,
        "SWAP_V2_FAILED_SPLIT",
        &payer,
        &merchant,
        &Address::generate(&env),
        &token_in,
        splits,
        9_000,
    );

    assert!(payment_client.try_swap_and_pay_v2(&args).is_err());
    assert!(payment_client.try_get_payment(&args.payment_id).is_err());
}

#[test]
fn test_swap_and_pay_v2_enforces_maximum_split_count() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_in, token_out) = setup_swap_test_env(&env);
    let router = Address::generate(&env);
    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let split = SwapSplit {
        router,
        amount: 1,
        path: vec![&env, token_in.clone(), token_out],
    };
    let splits = vec![&env, split.clone(), split.clone(), split.clone(), split];
    let args = build_swap_v2_args(
        &env,
        "SWAP_V2_TOO_MANY_SPLITS",
        &Address::generate(&env),
        &merchant,
        &Address::generate(&env),
        &token_in,
        splits,
        1,
    );

    let result = payment_client.try_swap_and_pay_v2(&args);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn test_swap_and_pay_v2_rejects_unapproved_router() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_in, token_out) = setup_swap_test_env(&env);
    let router = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &router, 10_000, false);
    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let split = SwapSplit {
        router,
        amount: 10_000,
        path: vec![&env, token_in.clone(), token_out],
    };
    let args = build_swap_v2_args(
        &env,
        "SWAP_V2_UNAPPROVED",
        &Address::generate(&env),
        &merchant,
        &Address::generate(&env),
        &token_in,
        vec![&env, split],
        9_000,
    );

    let result = payment_client.try_swap_and_pay_v2(&args);
    assert_eq!(result, Err(Ok(Error::UnapprovedRouter)));
}

#[test]
fn test_dummy_events() {
    let env = Env::default();
    let ev = env.events().all();
    ev.dummy_method();
}

#[test]
fn test_execute_swap_success_with_slippage_tolerance() {
    let env = Env::default();
    env.mock_all_auths();

    let router_id = env.register(crate::dex_router::DexRouter, ());
    let client = crate::dex_router::DexRouterClient::new(&env, &router_id);

    let caller = Address::generate(&env);
    let token_in = Address::generate(&env);
    let token_out = Address::generate(&env);

    let amount_in = 1000i128;
    // With 1 hop, get_amounts_out simulates 1% slippage -> 990 output
    let min_amount_out = 950i128;
    let max_slippage_bps = 500u32; // 5%

    let result = client.execute_swap(
        &caller,
        &token_in,
        &token_out,
        &amount_in,
        &min_amount_out,
        &max_slippage_bps,
    );
    assert_eq!(result, 990);
}

#[test]
fn test_execute_swap_rejects_insufficient_output_slippage() {
    let env = Env::default();
    env.mock_all_auths();

    let router_id = env.register(crate::dex_router::DexRouter, ());
    let client = crate::dex_router::DexRouterClient::new(&env, &router_id);

    let caller = Address::generate(&env);
    let token_in = Address::generate(&env);
    let token_out = Address::generate(&env);

    let amount_in = 1000i128;
    // get_amounts_out gives 990, require 995 -> should fail
    let min_amount_out = 995i128;
    let max_slippage_bps = 500u32;

    let result = client.try_execute_swap(
        &caller,
        &token_in,
        &token_out,
        &amount_in,
        &min_amount_out,
        &max_slippage_bps,
    );
    assert!(result.is_err());
}

#[test]
fn test_execute_swap_rejects_excessive_max_slippage_bps() {
    let env = Env::default();
    env.mock_all_auths();

    let router_id = env.register(crate::dex_router::DexRouter, ());
    let client = crate::dex_router::DexRouterClient::new(&env, &router_id);

    let caller = Address::generate(&env);
    let token_in = Address::generate(&env);
    let token_out = Address::generate(&env);

    let amount_in = 1000i128;
    let min_amount_out = 500i128;
    let max_slippage_bps = 5001u32; // > 5000 bps (50%) must be rejected

    let result = client.try_execute_swap(
        &caller,
        &token_in,
        &token_out,
        &amount_in,
        &min_amount_out,
        &max_slippage_bps,
    );
    assert!(result.is_err());
}

// ── Issue #1171: address validation at public entrypoints ─────────────────────

#[test]
fn test_swap_and_pay_rejects_malformed_dex_router() {
    use soroban_sdk::testutils::MuxedAddress as _;

    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_in, token_out) = setup_swap_test_env(&env);
    let mock_dex = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &mock_dex, 10_000, false);
    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let payer = Address::generate(&env);
    let deposit_address = Address::generate(&env);
    mint_swap_input(&env, &token_in, &payer, 10_000);

    let mut args = build_swap_args(
        &env,
        "swap_bad_router",
        &payer,
        &merchant,
        &deposit_address,
        &token_in,
        &token_out,
        &mock_dex,
        1_000,
        1_000,
    );

    // Default/all-zero address is rejected before any token movement.
    args.dex_router = Address::from_str(&env, ZERO_CONTRACT_STRKEY);
    assert_eq!(
        payment_client.try_swap_and_pay(&args),
        Err(Ok(Error::InvalidAddress))
    );

    // An account address cannot stand in for a DEX router contract.
    args.dex_router = soroban_sdk::testutils::MuxedAddress::generate(&env).address();
    assert_eq!(
        payment_client.try_swap_and_pay(&args),
        Err(Ok(Error::InvalidAddress))
    );

    // Restoring a valid router succeeds, proving only the input was rejected.
    args.dex_router = mock_dex;
    assert!(payment_client.try_swap_and_pay(&args).is_ok());
}

// ── Issue #1172: independent DEX circuit breaker ──────────────────────────────

#[test]
fn test_swap_circuit_breaker_pauses_and_resumes_only_dex_path() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, token_in, token_out) = setup_swap_test_env(&env);
    let mock_dex = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &mock_dex, 10_000, false);
    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let payer = Address::generate(&env);
    let deposit_address = Address::generate(&env);
    mint_swap_input(&env, &token_in, &payer, 10_000);
    let args = build_swap_args(
        &env,
        "swap_cb_payment",
        &payer,
        &merchant,
        &deposit_address,
        &token_in,
        &token_out,
        &mock_dex,
        1_000,
        1_000,
    );

    // Initially open.
    assert!(!payment_client.get_swap_pause_info().paused);

    let reason = String::from_str(&env, "simulated DEX unsafe");
    let intruder = Address::generate(&env);
    assert_eq!(
        payment_client.try_set_swap_pause(&intruder, &true, &reason),
        Err(Ok(Error::Unauthorized))
    );

    payment_client.set_swap_pause(&admin, &true, &reason);
    let info = payment_client.get_swap_pause_info();
    assert!(info.paused);
    assert_eq!(info.reason, reason);
    assert_eq!(info.admin, Some(admin.clone()));

    // The breaker blocks the swap path but leaves the global pause untouched.
    assert_eq!(
        payment_client.try_swap_and_pay(&args),
        Err(Ok(Error::ContractPaused))
    );
    assert!(!payment_client.get_pause_info().global.paused);
    assert!(!payment_client.get_creation_pause_info().paused);

    // Resuming restores the DEX path.
    payment_client.set_swap_pause(
        &admin,
        &false,
        &String::from_str(&env, "router fixed"),
    );
    assert!(!payment_client.get_swap_pause_info().paused);
    assert!(payment_client.try_swap_and_pay(&args).is_ok());
}
