use crate::PaymentProcessor;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_admin_can_add_approved_router() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let client = crate::PaymentProcessorClient::new(&env, &payment_processor);
    let admin = Address::generate(&env);
    client.initialize_payment_processor(&admin);

    let router = Address::generate(&env);

    client.add_approved_router(&admin, &router);

    let allowed = client.is_router_allowed(&router);
    assert!(allowed);
}

#[test]
fn test_unregistered_router_is_not_approved() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let client = crate::PaymentProcessorClient::new(&env, &payment_processor);
    let admin = Address::generate(&env);
    client.initialize_payment_processor(&admin);

    let unregistered_router = Address::generate(&env);

    assert!(client.get_approved_routers().is_empty());
    assert!(!client.is_router_allowed(&unregistered_router));
}

#[test]
fn test_router_is_no_longer_approved_after_removal() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let client = crate::PaymentProcessorClient::new(&env, &payment_processor);
    let admin = Address::generate(&env);
    client.initialize_payment_processor(&admin);

    let router = Address::generate(&env);

    client.add_approved_router(&admin, &router);
    assert!(client.is_router_allowed(&router));

    client.remove_approved_router(&admin, &router);
    assert!(!client.is_router_allowed(&router));
    assert!(client.get_approved_routers().is_empty());
}

#[test]
fn test_get_approved_routers_reflects_add_and_remove() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let client = crate::PaymentProcessorClient::new(&env, &payment_processor);
    let admin = Address::generate(&env);
    client.initialize_payment_processor(&admin);

    let router1 = Address::generate(&env);
    let router2 = Address::generate(&env);

    let initial_list = client.get_approved_routers();
    assert_eq!(initial_list.len(), 0);

    client.add_approved_router(&admin, &router1);
    let after_add1 = client.get_approved_routers();
    assert_eq!(after_add1.len(), 1);
    assert!(after_add1.contains(&router1));

    client.add_approved_router(&admin, &router2);
    let after_add2 = client.get_approved_routers();
    assert_eq!(after_add2.len(), 2);
    assert!(after_add2.contains(&router1));
    assert!(after_add2.contains(&router2));

    client.remove_approved_router(&admin, &router1);
    let after_remove = client.get_approved_routers();
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

    let result = client.try_add_approved_router(&non_admin, &router);
    assert!(result.is_err());

    client.add_approved_router(&admin, &router);
    let result = client.try_remove_approved_router(&non_admin, &router);
    assert!(result.is_err());
    assert!(client.is_router_allowed(&router));
}
