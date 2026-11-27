//! Issue #1167: regression coverage for require_auth on state-mutating entrypoints.

use crate::{RefundManager, RefundManagerClient};
use soroban_sdk::{testutils::Address as _, Address, Env, String, Symbol};

fn setup(env: &Env) -> (RefundManagerClient<'_>, Address) {
    let id = env.register(RefundManager, ());
    let client = RefundManagerClient::new(env, &id);
    let merchant = Address::generate(env);
    (client, merchant)
}

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn register_payment_requires_merchant_auth() {
    let env = Env::default();
    let (client, merchant) = setup(&env);
    client.register_payment(
        &String::from_str(&env, "auth-audit-1"),
        &merchant,
        &1_000i128,
        &Symbol::new(&env, "USDC"),
    );
}

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn register_payment_with_payer_requires_merchant_auth() {
    let env = Env::default();
    let (client, merchant) = setup(&env);
    let payer = Address::generate(&env);
    client.register_payment_with_payer(
        &String::from_str(&env, "auth-audit-2"),
        &merchant,
        &payer,
        &1_000i128,
        &Symbol::new(&env, "USDC"),
    );
}

#[test]
fn registration_succeeds_when_merchant_authenticates() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, merchant) = setup(&env);
    let payment_id = String::from_str(&env, "auth-audit-3");
    client.register_payment(
        &payment_id,
        &merchant,
        &1_000i128,
        &Symbol::new(&env, "USDC"),
    );
    assert_eq!(client.get_merchant_payment_count(&merchant), 1);
}
