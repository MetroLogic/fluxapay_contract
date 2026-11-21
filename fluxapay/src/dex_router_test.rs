use super::dex_router::*;
use soroban_sdk::{testutils::Address as _, testutils::Ledger, Address, Env, Symbol};

#[test]
fn test_swap_exact_tokens_for_tokens_success() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(DexRouter, ());
    let client = DexRouterClient::new(&env, &contract_id);

    let amount_in = 1000i128;
    let amount_out_min = 950i128;
    let path = vec![&env]; // Simplified path for testing
    let to = Address::generate(&env);
    let deadline = env.ledger().timestamp() + 3600;

    // Test successful swap - should return Ok with amounts
    let result = client.swap_exact_tokens_for_tokens(
        &amount_in,
        &amount_out_min,
        &path,
        &to,
        &deadline,
    );
    
    assert!(result.is_ok());
    let amounts = result.unwrap();
    assert_eq!(amounts.len(), 2); // Should have input and output amounts
}

#[test]
#[should_panic(expected = "Slippage protection triggered: output is below minimum required")]
fn test_swap_exact_tokens_for_tokens_slippage_protection() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(DexRouter, ());
    let client = DexRouterClient::new(&env, &contract_id);

    let amount_in = 1000i128;
    // Set amount_out_min higher than expected output (our get_amounts_out simulates 1% slippage per hop)
    // With 1 hop, we get 990 output, so setting 995 should trigger protection
    let amount_out_min = 995i128;
    let path = vec![&env]; // Simplified path for testing
    let to = Address::generate(&env);
    let deadline = env.ledger().timestamp() + 3600;

    // This should panic due to slippage protection
    client.swap_exact_tokens_for_tokens(
        &amount_in,
        &amount_out_min,
        &path,
        &to,
        &deadline,
    );
}

#[test]
fn test_get_amounts_out_basic() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(DexRouter, ());
    let client = DexRouterClient::new(&env, &contract_id);

    let amount_in = 1000i128;
    let path = vec![&env]; // Single hop

    let amounts = client.get_amounts_out(&amount_in, &path);
    assert_eq!(amounts.len(), 2); // input + output
    assert_eq!(amounts.get(0).unwrap(), 1000);
    // With 1% slippage, output should be 990
    assert_eq!(amounts.get(1).unwrap(), 990);
}

#[test]
fn test_get_amounts_out_multiple_hops() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(DexRouter, ());
    let client = DexRouterClient::new(&env, &contract_id);

    let amount_in = 1000i128;
    // Two hops
    let path = vec![&env, &env];

    let amounts = client.get_amounts_out(&amount_in, &path);
    assert_eq!(amounts.len(), 3); // input + 2 outputs
    assert_eq!(amounts.get(0).unwrap(), 1000);
    // First hop: 1000 * 0.99 = 990
    assert_eq!(amounts.get(1).unwrap(), 990);
    // Second hop: 990 * 0.99 = 980.1 -> 980 (integer division)
    assert_eq!(amounts.get(2).unwrap(), 980);
}

#[test]
fn test_check_liquidity_sufficient_and_insufficient() {
fn test_factory_address_configuration() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(DexRouter, ());
    let client = DexRouterClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    let mut path = soroban_sdk::Vec::new(&env);
    path.push_back(token_a.clone());
    path.push_back(token_b.clone());

    // By default in simulation mode, check_liquidity returns true
    assert!(client.check_liquidity(&path));

    // Disable simulation mode without configured reserve -> insufficient liquidity (#1147)
    client.set_simulation_mode(&admin, &false);
    assert!(!client.check_liquidity(&path));

    // Configure sufficient pool reserve -> returns true (#1147)
    client.set_pool_reserve(&admin, &token_a, &token_b, &50000i128);
    assert!(client.check_liquidity(&path));

    // Configure zero / empty pool reserve -> returns false (#1147)
    client.set_pool_reserve(&admin, &token_a, &token_b, &0i128);
    assert!(!client.check_liquidity(&path));
}

#[test]
fn test_get_amounts_out_with_pool_reserves_and_simulation_fallback() {

    // Default factory returns current contract address before configuration
    assert_eq!(client.factory(), contract_id);

    let admin = Address::generate(&env);
    let real_factory = Address::generate(&env);

    // Initialize with real factory (#1145)
    client.initialize(&admin, &real_factory);
    assert_eq!(client.factory(), real_factory);

    // Admin can update factory address (#1145)
    let updated_factory = Address::generate(&env);
    client.set_factory(&admin, &updated_factory);
    assert_eq!(client.factory(), updated_factory);
}

#[test]
fn test_refund_caller_transfer_and_event() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(DexRouter, ());
    let client = DexRouterClient::new(&env, &contract_id);

    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_client = soroban_sdk::token::Client::new(&env, &token_address);
    let stellar_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_address);

    let recipient = Address::generate(&env);
    let refund_amount = 500i128;

    // Fund the DexRouter so it can transfer tokens on refund
    stellar_client.mint(&contract_id, &refund_amount);
    assert_eq!(token_client.balance(&recipient), 0);

    let events_before = env.events().all().len();

    // Call refund_caller (#1143)
    let res = client.refund_caller(&token_address, &recipient, &refund_amount);
    assert!(res.is_ok());

    // Assert recipient balance increased by the refund amount
    assert_eq!(token_client.balance(&recipient), refund_amount);

    // Assert REFUND / CALLER event was emitted
    let events = env.events().all();
    assert!(events.len() > events_before);
}

#[test]
fn test_swap_tokens_for_exact_tokens_execution() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(DexRouter, ());
    let client = DexRouterClient::new(&env, &contract_id);

    let token_admin = Address::generate(&env);
    let token_in = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_out = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let token_out_client = soroban_sdk::token::Client::new(&env, &token_out);
    let stellar_out_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_out);

    let to = Address::generate(&env);
    let amount_out = 990i128;
    let amount_in_max = 2000i128;
    let deadline = env.ledger().timestamp() + 3600;

    // Fund router with token_out
    stellar_out_client.mint(&contract_id, &10000);

    let mut path = soroban_sdk::Vec::new(&env);
    path.push_back(token_in.clone());
    path.push_back(token_out.clone());

    // Execute exact output swap (#1146)
    let res = client.swap_tokens_for_exact_tokens(
        &amount_out,
        &amount_in_max,
        &path,
        &to,
        &deadline,
    );
    assert!(res.is_ok());
    let amounts = res.unwrap();
    assert_eq!(amounts.len(), 2);
    assert_eq!(amounts.get(1).unwrap(), amount_out);

    // Verify recipient received exact output tokens
    assert_eq!(token_out_client.balance(&to), amount_out);
}

#[test]
fn test_swap_tokens_for_exact_tokens_slippage_revert() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(DexRouter, ());
    let client = DexRouterClient::new(&env, &contract_id);

    let token_admin = Address::generate(&env);
    let token_in = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_out = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let to = Address::generate(&env);
    let amount_out = 1000i128;
    // get_amounts_in will require > 1000 in, so max 100 triggers slippage protection
    let amount_in_max = 100i128;
    let deadline = env.ledger().timestamp() + 3600;

    let mut path = soroban_sdk::Vec::new(&env);
    path.push_back(token_in);
    path.push_back(token_out);

    let res = client.try_swap_tokens_for_exact_tokens(
        &amount_out,
        &amount_in_max,
        &path,
        &to,
        &deadline,
    );
    assert!(res.is_err());
}

#[test]
fn test_swap_tokens_for_exact_tokens_deadline_revert() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(DexRouter, ());
    let client = DexRouterClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let token_c = Address::generate(&env);

    let mut path = soroban_sdk::Vec::new(&env);
    path.push_back(token_a.clone());
    path.push_back(token_b.clone());
    path.push_back(token_c.clone());

    // In simulation mode without explicit reserves, uses default 1% haircut (#1144)
    let sim_amounts = client.get_amounts_out(&1000, &path);
    assert_eq!(sim_amounts.get(0).unwrap(), 1000);
    assert_eq!(sim_amounts.get(1).unwrap(), 990);
    assert_eq!(sim_amounts.get(2).unwrap(), 980);

    // Configure known pool reserves for multi-hop quote (#1144)
    // Hop 1 (A -> B): reserve 500 (lower than 990 standard quote)
    client.set_pool_reserve(&admin, &token_a, &token_b, &500);
    // Hop 2 (B -> C): reserve 1000 (standard quote 500 * 0.99 = 495 is below 1000)
    client.set_pool_reserve(&admin, &token_b, &token_c, &1000);

    let pool_amounts = client.get_amounts_out(&1000, &path);
    assert_eq!(pool_amounts.get(0).unwrap(), 1000);
    assert_eq!(pool_amounts.get(1).unwrap(), 500); // capped by hop 1 reserve
    assert_eq!(pool_amounts.get(2).unwrap(), 495); // 500 * 99 / 100

    // In production mode (simulation disabled) with missing reserves, output is 0
    let token_d = Address::generate(&env);
    let mut unconfigured_path = soroban_sdk::Vec::new(&env);
    unconfigured_path.push_back(token_a);
    unconfigured_path.push_back(token_d);

    client.set_simulation_mode(&admin, &false);
    let unconfigured_amounts = client.get_amounts_out(&1000, &unconfigured_path);
    assert_eq!(unconfigured_amounts.get(1).unwrap(), 0);

    let token_admin = Address::generate(&env);
    let token_in = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_out = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let to = Address::generate(&env);
    let amount_out = 990i128;
    let amount_in_max = 2000i128;
    // Expired deadline
    let expired_deadline = env.ledger().timestamp().saturating_sub(10);

    let mut path = soroban_sdk::Vec::new(&env);
    path.push_back(token_in);
    path.push_back(token_out);

    let res = client.try_swap_tokens_for_exact_tokens(
        &amount_out,
        &amount_in_max,
        &path,
        &to,
        &expired_deadline,
    );
    assert!(res.is_err());
}

#[test]
fn test_swap_tokens_for_exact_tokens_delegates_to_real_router() {
    let env = Env::default();
    env.mock_all_auths();

    let primary_router = env.register(DexRouter, ());
    let client = DexRouterClient::new(&env, &primary_router);

    let real_router = env.register(DexRouter, ());
    let admin = Address::generate(&env);

    client.initialize(&admin, &primary_router);
    client.set_router(&admin, &real_router);
    assert_eq!(client.router(), Some(real_router.clone()));

    let token_admin = Address::generate(&env);
    let token_in = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_out = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let stellar_out_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_out);
    let token_out_client = soroban_sdk::token::Client::new(&env, &token_out);

    // Fund the real router
    stellar_out_client.mint(&real_router, &10000);

    let to = Address::generate(&env);
    let amount_out = 990i128;
    let amount_in_max = 2000i128;
    let deadline = env.ledger().timestamp() + 3600;

    let mut path = soroban_sdk::Vec::new(&env);
    path.push_back(token_in);
    path.push_back(token_out);

    let res = client.swap_tokens_for_exact_tokens(
        &amount_out,
        &amount_in_max,
        &path,
        &to,
        &deadline,
    );
    assert!(res.is_ok());
    assert_eq!(token_out_client.balance(&to), amount_out);
}