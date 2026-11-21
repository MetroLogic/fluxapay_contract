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
}