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
fn test_set_rate_rejects_non_positive_rates_and_unsupported_precision() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    let oracle = Address::generate(&env);
    client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);

    let pair = Symbol::new(&env, "USDC_NGN");
    assert_eq!(
        client.try_set_rate(&oracle, &pair, &0, &7),
        Err(Ok(FXOracleError::InvalidRate))
    );
    assert_eq!(
        client.try_set_rate(&oracle, &pair, &100, &39),
        Err(Ok(FXOracleError::InvalidRate))
    );
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

    // The maximum accepted threshold is 24 hours.
    client.set_staleness_threshold(&admin, &86_400);

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
fn test_staleness_threshold_bounds_are_enforced() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    assert_eq!(
        client.try_set_max_staleness(&admin, &59),
        Err(Ok(FXOracleError::InvalidStalenessThreshold))
    );
    assert_eq!(
        client.try_set_max_staleness(&admin, &86_401),
        Err(Ok(FXOracleError::InvalidStalenessThreshold))
    );
    client.set_max_staleness(&admin, &60);
    assert_eq!(client.get_staleness_threshold(), 60);
}

#[test]
fn test_rate_within_configured_staleness_window_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);
    let oracle = Address::generate(&env);
    client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    client.set_max_staleness(&admin, &60);

    let pair = Symbol::new(&env, "USDC_NGN");
    client.set_rate(&oracle, &pair, &1500i128, &0);
    env.ledger().set_timestamp(env.ledger().timestamp() + 59);

    assert!(client.try_get_rate(&pair).is_ok());
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

// ─── Token allowlist (Issue #811) ────────────────────────────────────────────
//
// `set_rate` accepted any pair symbol, so an admin key — including a
// compromised one — could publish a rate for a fabricated token and have
// `PaymentLinkManager::use_link` settle against it.

/// Oracle with a populated allowlist and an ORACLE-role operator.
fn setup_allowlisted(env: &Env) -> (Address, Address, FXOracleClient<'_>) {
    let contract_id = env.register(FXOracle, ());
    let client = FXOracleClient::new(env, &contract_id);
    let admin = Address::generate(env);

    let mut allowed = soroban_sdk::Vec::new(env);
    allowed.push_back(Symbol::new(env, "USDC"));
    allowed.push_back(Symbol::new(env, "NGN"));
    client.oracle_initialize_with_tokens(&admin, &86400, &allowed);

    let operator = Address::generate(env);
    client.oracle_grant_role(&admin, &Symbol::new(env, "ORACLE"), &operator);

    (admin, operator, client)
}

#[test]
fn allowlist_permits_a_pair_of_allowed_tokens() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, operator, client) = setup_allowlisted(&env);

    client.set_rate(&operator, &Symbol::new(&env, "USDC_NGN"), &1500_0000000i128, &7);

    assert_eq!(client.get_rate(&Symbol::new(&env, "USDC_NGN")).rate, 1500_0000000i128);
}

#[test]
fn allowlist_rejects_a_fabricated_quote_token() {
    // The attack in the issue: invent a token, publish a manipulated rate.
    let env = Env::default();
    env.mock_all_auths();
    let (_, operator, client) = setup_allowlisted(&env);

    let result = client.try_set_rate(
        &operator,
        &Symbol::new(&env, "USDC_FAKECOIN"),
        &1i128,
        &7,
    );

    assert_eq!(result, Err(Ok(FXOracleError::TokenNotAllowed)));
}

#[test]
fn allowlist_rejects_a_fabricated_base_token() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, operator, client) = setup_allowlisted(&env);

    let result = client.try_set_rate(&operator, &Symbol::new(&env, "SCAM_NGN"), &1i128, &7);

    assert_eq!(result, Err(Ok(FXOracleError::TokenNotAllowed)));
}

#[test]
fn allowlist_rejects_a_malformed_pair() {
    // An unparseable pair cannot be checked, and an unparseable pair is exactly
    // the shape a fabricated one takes — so it is rejected, not waved through.
    let env = Env::default();
    env.mock_all_auths();
    let (_, operator, client) = setup_allowlisted(&env);

    for pair in ["USDCNGN", "USDC_NGN_EUR", "_NGN", "USDC_"] {
        let result = client.try_set_rate(&operator, &Symbol::new(&env, pair), &1i128, &7);
        assert_eq!(
            result,
            Err(Ok(FXOracleError::TokenNotAllowed)),
            "pair {pair} should have been rejected"
        );
    }
}

#[test]
fn an_empty_allowlist_permits_everything() {
    // Deliberate: an instance upgraded from before this existed has no
    // allowlist, and failing closed would take FX settlement down as a side
    // effect of a security fix.
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    let operator = Address::generate(&env);
    client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &operator);

    assert!(client.get_allowed_tokens().is_empty());
    client.set_rate(&operator, &Symbol::new(&env, "ANY_THING"), &1i128, &7);
}

#[test]
fn add_allowed_token_activates_enforcement() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_oracle(&env);

    let operator = Address::generate(&env);
    client.oracle_grant_role(&admin, &Symbol::new(&env, "ORACLE"), &operator);

    // Before: anything goes. After: only listed tokens.
    client.set_rate(&operator, &Symbol::new(&env, "USDC_NGN"), &1i128, &7);

    client.add_allowed_token(&admin, &Symbol::new(&env, "USDC"));
    client.add_allowed_token(&admin, &Symbol::new(&env, "NGN"));

    client.set_rate(&operator, &Symbol::new(&env, "USDC_NGN"), &2i128, &7);
    assert_eq!(
        client.try_set_rate(&operator, &Symbol::new(&env, "USDC_FAKE"), &3i128, &7),
        Err(Ok(FXOracleError::TokenNotAllowed))
    );
}

#[test]
fn add_allowed_token_is_idempotent() {
    // A re-run of a deployment script must not fail.
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _, client) = setup_allowlisted(&env);

    client.add_allowed_token(&admin, &Symbol::new(&env, "USDC"));
    client.add_allowed_token(&admin, &Symbol::new(&env, "USDC"));

    assert_eq!(client.get_allowed_tokens().len(), 2);
}

#[test]
fn remove_allowed_token_blocks_new_rates() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, operator, client) = setup_allowlisted(&env);

    client.set_rate(&operator, &Symbol::new(&env, "USDC_NGN"), &1500i128, &7);
    client.remove_allowed_token(&admin, &Symbol::new(&env, "NGN"));

    assert_eq!(
        client.try_set_rate(&operator, &Symbol::new(&env, "USDC_NGN"), &1600i128, &7),
        Err(Ok(FXOracleError::TokenNotAllowed))
    );
}

#[test]
fn remove_allowed_token_leaves_the_existing_rate_readable() {
    // Deleting it would fail an in-flight settlement with RateNotFound
    // mid-payment; the rate ages out through the normal staleness path instead.
    let env = Env::default();
    env.mock_all_auths();
    let (admin, operator, client) = setup_allowlisted(&env);

    client.set_rate(&operator, &Symbol::new(&env, "USDC_NGN"), &1500i128, &7);
    client.remove_allowed_token(&admin, &Symbol::new(&env, "NGN"));

    assert_eq!(client.get_rate(&Symbol::new(&env, "USDC_NGN")).rate, 1500i128);
}

#[test]
fn remove_allowed_token_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _, client) = setup_allowlisted(&env);

    client.remove_allowed_token(&admin, &Symbol::new(&env, "NGN"));
    client.remove_allowed_token(&admin, &Symbol::new(&env, "NGN"));

    assert_eq!(client.get_allowed_tokens().len(), 1);
}

#[test]
fn allowlist_management_requires_the_admin_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, operator, client) = setup_allowlisted(&env);

    // An ORACLE-role operator may publish rates but must not widen the set of
    // tokens it can publish for — that separation is the point of the control.
    assert_eq!(
        client.try_add_allowed_token(&operator, &Symbol::new(&env, "SCAM")),
        Err(Ok(FXOracleError::Unauthorized))
    );
    assert_eq!(
        client.try_remove_allowed_token(&operator, &Symbol::new(&env, "USDC")),
        Err(Ok(FXOracleError::Unauthorized))
    );
}

#[test]
fn batch_rejects_the_whole_batch_when_one_pair_is_disallowed() {
    // Validated up front: store_rate has already mutated state by the time a
    // later entry fails, and Soroban gives no partial rollback inside a call.
    let env = Env::default();
    env.mock_all_auths();
    let (_, operator, client) = setup_allowlisted(&env);

    let mut rates = soroban_sdk::Vec::new(&env);
    rates.push_back((Symbol::new(&env, "USDC_NGN"), 1500i128, 7u32));
    rates.push_back((Symbol::new(&env, "USDC_FAKE"), 1i128, 7u32));

    assert_eq!(
        client.try_set_rates_batch(&operator, &rates),
        Err(Ok(FXOracleError::TokenNotAllowed))
    );

    // The allowed pair in the batch was not written either.
    assert!(client.try_get_rate(&Symbol::new(&env, "USDC_NGN")).is_err());
}

#[test]
fn batch_accepts_an_all_allowed_batch() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, operator, client) = setup_allowlisted(&env);
    client.add_allowed_token(&admin, &Symbol::new(&env, "EUR"));

    let mut rates = soroban_sdk::Vec::new(&env);
    rates.push_back((Symbol::new(&env, "USDC_NGN"), 1500i128, 7u32));
    rates.push_back((Symbol::new(&env, "USDC_EUR"), 92i128, 7u32));

    assert_eq!(client.set_rates_batch(&operator, &rates), 2);
}

#[test]
fn is_token_allowed_reports_the_configured_set() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, client) = setup_allowlisted(&env);

    assert!(client.is_token_allowed(&Symbol::new(&env, "USDC")));
    assert!(client.is_token_allowed(&Symbol::new(&env, "NGN")));
    assert!(!client.is_token_allowed(&Symbol::new(&env, "SCAM")));
}

#[test]
fn a_disallowed_pair_cannot_accumulate_quorum_submissions() {
    // Checked before the quorum bookkeeping, so a disallowed pair cannot build
    // up submissions and be rejected only once quorum is reached.
    let env = Env::default();
    env.mock_all_auths();
    let (admin, operator, client) = setup_allowlisted(&env);
    client.set_oracle_quorum(&admin, &2);

    assert_eq!(
        client.try_set_rate(&operator, &Symbol::new(&env, "USDC_FAKE"), &1i128, &7),
        Err(Ok(FXOracleError::TokenNotAllowed))
    );
    assert!(client
        .get_oracle_submissions(&Symbol::new(&env, "USDC_FAKE"))
        .is_empty());
}
