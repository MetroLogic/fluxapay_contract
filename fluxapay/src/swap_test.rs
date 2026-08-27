use crate::{
    merchant_registry::{MerchantRegistry, MerchantRegistryClient},
    mock_dex_router::{configure_mock_dex, MockDexRouter},
    Error, PaymentProcessor, PaymentProcessorClient, PaymentStatus, SwapAndPayArgs, SwapRoute,
    ZERO_CONTRACT_STRKEY,
};
use soroban_sdk::{
    testutils::Address as _, testutils::Events, token, vec, Address, Env, String, Symbol,
    Vec,
};

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
        &MaybeFeeConfig::None);
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

    let approved_router = Address::generate(&env);
    payment_client.add_router(&admin, &approved_router);

    assert!(payment_client.is_router_allowed(&approved_router));
    assert!(!payment_client.is_router_allowed(&mock_dex));
    assert_eq!(payment_client.get_allowed_routers().len(), 1);

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
    assert_eq!(res, Err(Ok(Error::RouterNotAllowed)));

    payment_client.add_router(&admin, &mock_dex);
    let approved_res = payment_client.try_swap_and_pay(&unapproved_args);
    assert!(approved_res.is_ok());

    payment_client.remove_router(&admin, &mock_dex);
    assert!(!payment_client.is_router_allowed(&mock_dex));
}

#[test]
fn test_xlm_auto_wrapping_and_wrapped_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, merchant_client, _token_in, token_out) = setup_swap_test_env(&env);

    let wxlm = Address::generate(&env);
    payment_client.set_wrapped_xlm_contract(&admin, &wxlm);
    assert_eq!(payment_client.get_wrapped_xlm_contract(), Some(wxlm.clone()));

    let mock_dex = env.register(MockDexRouter, ());
    configure_mock_dex(&env, &mock_dex, 10_000, false);

    let merchant = register_verified_merchant(&env, &admin, &payment_client, &merchant_client);
    let payer = Address::generate(&env);
    let deposit_address = Address::generate(&env);
    let native_xlm = Address::from_str(&env, ZERO_CONTRACT_STRKEY);

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

    let payment = payment_client.swap_and_pay(&xlm_args);
    assert_eq!(payment.original_token, Some(native_xlm));
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
    let routes = vec![&env, route1, route2];

    let payment = payment_client.swap_and_pay_multi_route(&args, &routes, &9_900);
    assert_eq!(payment.payment_id, String::from_str(&env, "MULTI_ROUTE_PAYMENT"));
    assert_eq!(payment.amount, 9_900);
}

#[test]
fn test_dummy_events() {
    let env = Env::default();
    let ev = env.events().all();
    ev.dummy_method();
}