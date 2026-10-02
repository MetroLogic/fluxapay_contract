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