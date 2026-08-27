use crate::PaymentProcessor;
use soroban_sdk::{
    testutils::Address as _,
    Address, Env, Symbol,
};

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
fn test_swap_and_pay_after_router_removed_is_rejected() {
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
