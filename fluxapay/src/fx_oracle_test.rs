use crate::{FXOracle, FXOracleClient, FXOracleError};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol,
};

fn setup_oracle(env: &Env) -> (Address, FXOracleClient<'_>) {
    let contract_id = env.register(FXOracle, ());
    let client = FXOracleClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.oracle_initialize(&admin, &86400); // 24 hour threshold
    (admin, client)
}

#[test]
fn test_set_and_get_rate() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    let oracle = Address::generate(&env);
    client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);

    let pair = Symbol::new(&env, "USDC_NGN");
    let rate = 1500_0000000i128; // 1500 NGN/USDC
    let decimals = 7;

    client.set_rate(&oracle, &pair, &rate, &decimals);

    let rate_data = client.get_rate(&pair);
    assert_eq!(rate_data.rate, rate);
    assert_eq!(rate_data.decimals, decimals);
    assert_eq!(rate_data.pair, pair);
    assert_eq!(rate_data.updated_at, env.ledger().timestamp());
}

#[test]
fn test_unauthorized_set_rate() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_oracle(&env);

    let unauthorized_user = Address::generate(&env);
    let pair = Symbol::new(&env, "USDC_NGN");

    let result = client.try_set_rate(&unauthorized_user, &pair, &1000i128, &2);
    assert_eq!(result, Err(Ok(FXOracleError::Unauthorized)));
}

#[test]
fn test_staleness_check() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    let oracle = Address::generate(&env);
    client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);

    let pair = Symbol::new(&env, "USDC_NGN");
    client.set_rate(&oracle, &pair, &1500i128, &0);

    // Jump forward 25 hours (threshold is 24)
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 25 * 3600);

    let result = client.try_get_rate(&pair);
    assert_eq!(result, Err(Ok(FXOracleError::RateStale)));
}

#[test]
fn test_hard_staleness_cap_despite_high_threshold() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    let oracle = Address::generate(&env);
    client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);

    // Admin sets a permissive 7-day threshold; hard cap still applies at 24 h.
    client.set_staleness_threshold(&admin, &(7 * 86_400));

    let pair = Symbol::new(&env, "USDC_NGN");
    client.set_rate(&oracle, &pair, &1500i128, &0);

    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 25 * 3600);

    let result = client.try_get_rate(&pair);
    assert_eq!(result, Err(Ok(FXOracleError::RateStale)));
}

#[test]
fn test_circuit_breaker_rejects_rate_by_ledger_gap() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    let oracle = Address::generate(&env);
    client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);

    let pair = Symbol::new(&env, "USDC_NGN");
    client.set_rate(&oracle, &pair, &1500i128, &0);

    let seq_at_update = env.ledger().sequence();
    env.ledger().set_sequence_number(seq_at_update + 17_281);

    let result = client.try_get_rate(&pair);
    assert_eq!(result, Err(Ok(FXOracleError::RateStale)));
}

#[test]
fn test_settlement_amount_calculation() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    let oracle = Address::generate(&env);
    client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);

    // 1 USDC = 1500.50 NGN (2 decimals: 150050)
    let pair = Symbol::new(&env, "NGN");
    client.set_rate(&oracle, &pair, &150050i128, &2);

    // 100 USDC -> 150050 NGN
    let usdc_amount = 100i128;
    let expected_fiat = 150050i128; // (100 * 150050) / 100

    let amount = client.get_settlement_amount(&usdc_amount, &pair);
    assert_eq!(amount, expected_fiat);
}

#[test]
fn test_update_staleness_threshold() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    client.set_staleness_threshold(&admin, &3600);
    assert_eq!(client.get_staleness_threshold(), 3600);
}

#[test]
fn test_oracle_grant_role_by_admin_grants_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);
    let oracle = Address::generate(&env);
    let role = Symbol::new(&env, "ORACLE");

    client.oracle_grant_role(&admin, &role, &oracle);
    assert!(client.oracle_has_role(&role, &oracle));
}

#[test]
fn test_oracle_grant_role_by_non_admin_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_oracle(&env);
    let non_admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let role = Symbol::new(&env, "ORACLE");

    let result = client.try_oracle_grant_role(&non_admin, &role, &oracle);
    assert_eq!(result, Err(Ok(FXOracleError::Unauthorized)));
}

#[test]
fn test_get_fx_admin_returns_initialized_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    assert_eq!(client.get_fx_admin(), Some(admin));
}

#[test]
fn test_get_fx_admin_before_initialization_returns_none() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(FXOracle, ());
    let client = FXOracleClient::new(&env, &contract_id);

    assert_eq!(client.get_fx_admin(), None);
}

#[test]
fn test_check_rate_staleness_emits_alert() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    let oracle = Address::generate(&env);
    client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);

    let pair = Symbol::new(&env, "USDC_NGN");
    client.set_rate(&oracle, &pair, &1500i128, &0);

    assert!(!client.check_rate_staleness(&pair));

    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 25 * 3600);

    // Stale → returns true (and emits RATE/STALE_ALERT on-chain).
    assert!(client.check_rate_staleness(&pair));
}

#[test]
fn test_set_rates_batch_stores_all_rates() {
    use soroban_sdk::vec;

    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    let oracle = Address::generate(&env);
    client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);

    let rates = vec![
        &env,
        (Symbol::new(&env, "USD"), 1_0000000i128, 7u32),
        (Symbol::new(&env, "NGN"), 1500_0000000i128, 7u32),
        (Symbol::new(&env, "EUR"), 9200000i128, 7u32),
    ];

    let count = client.set_rates_batch(&oracle, &rates);
    assert_eq!(count, 3);

    assert_eq!(
        client.get_rate(&Symbol::new(&env, "USD")).rate,
        1_0000000i128
    );
    assert_eq!(
        client.get_rate(&Symbol::new(&env, "NGN")).rate,
        1500_0000000i128
    );
    assert_eq!(client.get_rate(&Symbol::new(&env, "EUR")).rate, 9200000i128);
}

#[test]
fn test_set_rates_batch_rejects_non_oracle() {
    use soroban_sdk::vec;

    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_oracle(&env);

    let unauthorized = Address::generate(&env);
    let rates = vec![&env, (Symbol::new(&env, "USD"), 1i128, 0u32)];

    let result = client.try_set_rates_batch(&unauthorized, &rates);
    assert_eq!(result, Err(Ok(FXOracleError::Unauthorized)));
}

// ─── Issue #636: get_rate_or_inverse / get_settlement_amount_for_pair ─────────

#[test]
fn test_get_rate_or_inverse_direct_lookup() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    let oracle = Address::generate(&env);
    client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);

    // EUR_USD stored directly: 1 EUR = 1.08 USD (7 decimals).
    let pair = Symbol::new(&env, "EUR_USD");
    let rate = 1_0800000i128;
    client.set_rate(&oracle, &pair, &rate, &7u32);

    let data = client.get_rate_or_inverse(&pair);
    assert_eq!(data.pair, pair);
    assert_eq!(data.rate, rate);
    assert_eq!(data.decimals, 7);
}

#[test]
fn test_get_rate_or_inverse_falls_back_to_inverse() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    let oracle = Address::generate(&env);
    client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);

    // Only USD_EUR is stored: 1 USD = 0.90 EUR (7 decimals).
    let stored = Symbol::new(&env, "USD_EUR");
    client.set_rate(&oracle, &stored, &9_000000i128, &7u32);

    // Asking for EUR_USD must return 1 / 0.90 ≈ 1.1111111 scaled to 14 decimals.
    let requested = Symbol::new(&env, "EUR_USD");
    let data = client.get_rate_or_inverse(&requested);

    assert_eq!(data.pair, requested);
    assert_eq!(data.decimals, 14);
    // 10^(7+14) / 9_000000 = 10^21 / 9e6 = 111_111_111_111_111 (integer division)
    assert_eq!(data.rate, 111_111_111_111_111i128);

    // Sanity: rate / 10^14 ≈ 1.11111111111111
    let one = 100_000_000_000_000i128; // 10^14
    assert!(data.rate > one && data.rate < 2 * one);
}

#[test]
fn test_get_rate_or_inverse_neither_found() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_oracle(&env);

    let result = client.try_get_rate_or_inverse(&Symbol::new(&env, "GBP_JPY"));
    assert_eq!(result, Err(Ok(FXOracleError::PairNotFound)));
}

#[test]
fn test_get_rate_or_inverse_malformed_pair_without_separator() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_oracle(&env);

    // No '_' separator and no stored rate → PairNotFound (cannot invert).
    let result = client.try_get_rate_or_inverse(&Symbol::new(&env, "EURUSD"));
    assert_eq!(result, Err(Ok(FXOracleError::PairNotFound)));
}

#[test]
fn test_get_rate_or_inverse_direct_stale_does_not_fall_through() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    let oracle = Address::generate(&env);
    client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);

    let pair = Symbol::new(&env, "EUR_USD");
    client.set_rate(&oracle, &pair, &1_0800000i128, &7u32);
    // Also store the inverse so a fall-through would otherwise succeed.
    client.set_rate(&oracle, &Symbol::new(&env, "USD_EUR"), &9_000000i128, &7u32);

    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 25 * 3600);

    let result = client.try_get_rate_or_inverse(&pair);
    assert_eq!(result, Err(Ok(FXOracleError::RateStale)));
}

#[test]
fn test_get_settlement_amount_for_pair_uses_inverse_automatically() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    let oracle = Address::generate(&env);
    client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);

    // Store only NGN_USD: 1 NGN = 0.00065 USD (7 decimals → 6500).
    client.set_rate(&oracle, &Symbol::new(&env, "NGN_USD"), &6500i128, &7u32);

    // Convert 1_000_000 USD → NGN via the (missing) USD_NGN pair, which the
    // contract derives as the inverse of NGN_USD.
    let ngn = client.get_settlement_amount_for_pair(
        &Symbol::new(&env, "USD"),
        &Symbol::new(&env, "NGN"),
        &1_000_000i128,
    );

    // inverse rate = 10^(7+14) / 6500 = 153_846_153_846_153_846 (14 decimals)
    // amount = 1_000_000 * rate / 10^14 = 1_538_461_538 NGN
    assert_eq!(ngn, 1_538_461_538i128);
}

#[test]
fn test_get_settlement_amount_for_pair_direct() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    let oracle = Address::generate(&env);
    client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);

    // USD_NGN stored directly: 1 USD = 1500 NGN (7 decimals).
    client.set_rate(
        &oracle,
        &Symbol::new(&env, "USD_NGN"),
        &1500_0000000i128,
        &7u32,
    );

    let ngn = client.get_settlement_amount_for_pair(
        &Symbol::new(&env, "USD"),
        &Symbol::new(&env, "NGN"),
        &100i128,
    );
    assert_eq!(ngn, 150_000i128); // 100 * 1500
}

#[test]
fn test_get_settlement_amount_for_pair_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_oracle(&env);

    let result = client.try_get_settlement_amount_for_pair(
        &Symbol::new(&env, "USD"),
        &Symbol::new(&env, "CHF"),
        &100i128,
    );
    assert_eq!(result, Err(Ok(FXOracleError::PairNotFound)));
}

#[test]
fn test_set_rates_batch_rejects_oversized_batch() {
    use soroban_sdk::vec;

    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    let oracle = Address::generate(&env);
    client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);

    let mut rates = vec![&env];
    for _ in 0..21u32 {
        rates.push_back((Symbol::new(&env, "USD"), 1i128, 0u32));
    }

    let result = client.try_set_rates_batch(&oracle, &rates);
    assert_eq!(result, Err(Ok(FXOracleError::BatchTooLarge)));
}
