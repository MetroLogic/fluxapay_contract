//! Integration-style tests for the MerchantPreAuth pull-payment lifecycle:
//! pre-authorize → pull_payment → remaining_limit decrease → revoke.
//!
//! All tests run against the Soroban test environment (`Env::default()`); no
//! network calls are made.

use crate::{MerchantAuthError, PaymentProcessor, PaymentProcessorClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token, Address, Env,
};

/// Register a fresh PaymentProcessor with an admin.
fn setup(env: &Env) -> (Address, PaymentProcessorClient<'_>) {
    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize_payment_processor(&admin);
    (admin, client)
}

/// Mint funds to a customer and grant `merchant` a pull authorization whose
/// period budget is `limit_per_period` every `period_secs` seconds.
fn setup_authorization(
    env: &Env,
    client: &PaymentProcessorClient<'_>,
    limit_per_period: i128,
    period_secs: u64,
) -> (Address, Address, Address) {
    let customer = Address::generate(env);
    let merchant = Address::generate(env);
    let token_admin = Address::generate(env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    token::StellarAssetClient::new(env, &token).mint(&customer, &1_000_000_000i128);

    client.pre_authorize_merchant(
        &customer,
        &merchant,
        &token,
        &limit_per_period,
        &period_secs,
    );

    (customer, merchant, token)
}

#[test]
fn test_pre_authorize_and_pull_payment_success() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup(&env);
    let (customer, merchant, token) = setup_authorization(&env, &client, 1_000i128, 100u64);

    let pulled = client.pull_payment(&merchant, &customer, &400i128);
    assert_eq!(pulled, 400i128);

    let auth = client.get_merchant_authorization(&customer, &merchant);
    assert!(auth.active);
    assert_eq!(auth.pulled_this_period, 400i128);

    // Funds moved customer -> merchant.
    let token_client = token::TokenClient::new(&env, &token);
    assert_eq!(token_client.balance(&customer), 1_000_000_000i128 - 400i128);
    assert_eq!(token_client.balance(&merchant), 400i128);
}

#[test]
fn test_pull_payment_exceeds_authorization_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup(&env);
    let (customer, merchant, _token) = setup_authorization(&env, &client, 1_000i128, 100u64);

    let result = client.try_pull_payment(&merchant, &customer, &1_001i128);
    assert_eq!(result, Err(Ok(MerchantAuthError::LimitExceeded)));

    // The full authorized amount is still pullable in the same period.
    let pulled = client.pull_payment(&merchant, &customer, &1_000i128);
    assert_eq!(pulled, 1_000i128);
}

#[test]
fn test_revoke_authorization_blocks_future_pulls() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup(&env);
    let (customer, merchant, _token) = setup_authorization(&env, &client, 1_000i128, 100u64);

    client.revoke_merchant_authorization(&customer, &merchant);

    let auth = client.get_merchant_authorization(&customer, &merchant);
    assert!(!auth.active);

    let result = client.try_pull_payment(&merchant, &customer, &100i128);
    assert_eq!(result, Err(Ok(MerchantAuthError::AuthorizationInactive)));

    // A revoked authorization has no remaining budget.
    assert_eq!(
        client.merchant_authorization_remaining(&customer, &merchant),
        0i128
    );
}

#[test]
fn test_remaining_limit_decreases_after_each_pull() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup(&env);
    let (customer, merchant, _token) = setup_authorization(&env, &client, 1_000i128, 100u64);

    assert_eq!(
        client.merchant_authorization_remaining(&customer, &merchant),
        1_000i128
    );

    client.pull_payment(&merchant, &customer, &300i128);
    assert_eq!(
        client.merchant_authorization_remaining(&customer, &merchant),
        700i128
    );

    client.pull_payment(&merchant, &customer, &200i128);
    assert_eq!(
        client.merchant_authorization_remaining(&customer, &merchant),
        500i128
    );

    // Exhausting the limit reports zero remaining.
    client.pull_payment(&merchant, &customer, &500i128);
    assert_eq!(
        client.merchant_authorization_remaining(&customer, &merchant),
        0i128
    );
}

#[test]
fn test_pre_auth_period_reset() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup(&env);
    let (customer, merchant, _token) = setup_authorization(&env, &client, 1_000i128, 100u64);

    // Exhaust the current period budget.
    client.pull_payment(&merchant, &customer, &1_000i128);
    assert_eq!(
        client.merchant_authorization_remaining(&customer, &merchant),
        0i128
    );
    assert!(client.try_pull_payment(&merchant, &customer, &1i128).is_err());

    // After the period rolls over the authorization renews.
    env.ledger().with_mut(|ledger| ledger.timestamp += 101);
    assert_eq!(
        client.merchant_authorization_remaining(&customer, &merchant),
        1_000i128
    );

    let pulled = client.pull_payment(&merchant, &customer, &800i128);
    assert_eq!(pulled, 800i128);

    let auth = client.get_merchant_authorization(&customer, &merchant);
    assert_eq!(auth.pulled_this_period, 800i128);
}

#[test]
fn test_pre_authorize_same_merchant_twice_replaces() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup(&env);
    let (customer, merchant, token) = setup_authorization(&env, &client, 1_000i128, 100u64);

    // Consume part of the first authorization's budget.
    client.pull_payment(&merchant, &customer, &400i128);

    // A second pre-authorization replaces the first grant in place: the
    // pulled-budget counter resets and the new terms take effect.
    client.pre_authorize_merchant(&customer, &merchant, &token, &2_000i128, &200u64);

    let auth = client.get_merchant_authorization(&customer, &merchant);
    assert!(auth.active);
    assert_eq!(auth.limit_per_period, 2_000i128);
    assert_eq!(auth.period_secs, 200u64);
    assert_eq!(auth.pulled_this_period, 0i128);

    // The replacement allowance is fully available under the new limit.
    assert_eq!(
        client.merchant_authorization_remaining(&customer, &merchant),
        2_000i128
    );
    let pulled = client.pull_payment(&merchant, &customer, &2_000i128);
    assert_eq!(pulled, 2_000i128);
}