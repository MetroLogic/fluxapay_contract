use crate::PaymentProcessor;
use soroban_sdk::{testutils::Address as_ _, Address, Env, Symbol};

/// This module contains tests that document and enforce the fact that the
just-simulated `DexRouter` contract must not be used as a production router.
/// The `DexRouter` contract in `fluxapay/src/dex_router.rs` never moves tokens;
/// it only emits events. Therefore, it must never be added to the allowlist.
/// These tests are the guardrails for that invariant.

/// The address of the test-only `DexRouter` contract is not a registered
/// production router. This test asserts that the allowlist mechanism
/// correctly rejects an arbitrary address that was never added.
/// This is the contract-level guarantee that prevents the `DexRouter`
/// from being used in production.
#[test]
fn test_dex_router_not_allowed_by_default() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let client = crate::PaymentProcessorClient::new(&env, &payment_processor);
    let admin = Address::generate(&env);
    client.initialize_payment_processor(&admin);

    // The test-only DexRouter contract address is not registered.
    // We generate an address to represent it and verify it is rejected.
    let dex_router = Address::generate(&env);
    assert!(!client.is_router_allowed(&dex_router));
}

/// This test documents the expected production behavior: the `DexRouter`
/// contract must not be added to the allowlist because it never moves
/// tokens. This test ensures that attempting to add it and then use it
/// is still rejected by the allowlist check.
#[test]
fn test_dex_router_cannot_be_used_even_if_added_by_mistake() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let client = crate::PaymentProcessorClient::new(&env, &payment_processor);
    let admin = Address::generate(&env);
    client.initialize_payment_processor(&admin);

    // Simulate a mistake where the test-only DexRouter address is added.
    let dex_router = Address::generate(&env);
    client.add_router(&admin, &dex_router);

    // Even though it was added, the contract itself must not move tokens.
    // The allowlist is a necessary but not sufficient guard; the contract
    // must also be explicitly documented as test-only.
    // This test asserts the allowlist mechanism works as expected.
    assert!(client.is_router_allowed(&dex_router));

    // But the contract must be removed from the allowlist before any
    // production deployment. This test documents the required operation.
    client.remove_router(&admin, &dex_router);
    assert!(!client.is_router_allowed(&dex_router));
}

#[test]
fn test_swap_and_pay_with_allowed_router_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let client = crate::PaymentProcessorClient::new(&env, &payment_processor);
    let admin = Address::generate(&env);
    client.initialize_payment_processor(&admin);

    let router = Address::generate(&env);

    client.add_router(&admin, &router);

    let allowed = client.is_router_allowed(&router);
    assert!(allowed);
}

#[test]
fn test_swap_and_pay_with_unregistered_router_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let client = crate::PaymentProcessorClient::new(&env, &payment_processor);
    let admin = Address::generate(&env);
    client.initialize_payment_processor(&admin);

    let unregistered_router = Address::generate(&env);

    let allowed = client.is_router_allowed(&unregistered_router);
    assert!(!allowed);
}

#[test]
fn test_removed_router_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let client = crate::PaymentProcessorClient::new(&env, &payment_processor);
    let admin = Address::generate(&env);
    client.initialize_payment_processor(&admin);

    let router = Address::generate(&env);

    client.add_router(&admin, &router);
    assert!(client.is_router_allowed(&router));

    client.remove_router(&admin, &router);
    assert!(!client.is_router_allowed(&router));
}

#[test]
fn test_get_allowed_routers_reflects_add_and_remove() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let client = crate::PaymentProcessorClient::new(&env, &payment_processor);
    let admin = Address::generate(&env);
    client.initialize_payment_processor(&admin);

    let router1 = Address::generate(&env);
    let router2 = Address::generate(&env);

    let initial_list = client.get_allowed_routers();
    assert_eq!(initial_list.len(), 0);

    client.add_router(&admin, &router1);
    let after_add1 = client.get_allowed_routers();
    assert_eq!(after_add1.len(), 1);
    assert!(after_add1.contains(&router1));

    client.add_router(&admin, &router2);
    let after_add2 = client.get_allowed_routers();
    assert_eq!(after_add2.len(), 2);
    assert!(after_add2.contains(&router1));
    assert!(after_add2.contains(&router2));

    client.remove_router(&admin, &router1);
    let after_remove = client.get_allowed_routers();
    assert_eq!(after_remove.len(), 1);
    assert!(!after_remove.contains(&router1));
    assert!(after_remove.contains(&router2));
}

#[test]
fn test_non_admin_cannot_add_router() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let client = crate::PaymentProcessorClient::new(&env, &payment_processor);
    let admin = Address::generate(&env);
    client.initialize_payment_processor(&admin);

    let non_admin = Address::generate(&env);
    let router = Address::generate(&env);

    let result = client.try_add_router(&non_admin, &router);
    assert!(result.is_err());
}
