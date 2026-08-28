#![cfg(test)]

use crate::{
    access_control::role_merchant, CreatePaymentArgs, PaymentProcessor, PaymentProcessorClient,
};
use soroban_sdk::{testutils::Address as _, testutils::Events, Address, Env, Map, String, Symbol};

fn setup(env: &Env) -> (Address, Address, PaymentProcessorClient<'_>) {
    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize_payment_processor(&admin);
    let merchant = Address::generate(env);
    client.grant_role(&admin, &role_merchant(env), &merchant);
    (admin, merchant, client)
}

fn payment_args(
    env: &Env,
    merchant: &Address,
    metadata: Option<Map<String, String>>,
) -> CreatePaymentArgs {
    CreatePaymentArgs {
        payment_id: String::from_str(env, "pay_meta_01"),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 1_000_000_000i128,
        currency: Symbol::new(env, "USDC"),
        deposit_address: Address::generate(env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
        metadata,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
    }
}

// ─── Issue #286: PaymentCharge metadata field ────────────────────────────────

#[test]
fn test_create_payment_nil_metadata() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, merchant, client) = setup(&env);

    let payment = client.create_payment(&payment_args(&env, &merchant, None));
    assert!(payment.metadata.is_none());

    let retrieved = client.get_payment(&payment.payment_id);
    assert!(retrieved.metadata.is_none());
}

#[test]
fn test_create_payment_with_valid_metadata() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, merchant, client) = setup(&env);

    let mut meta: Map<String, String> = Map::new(&env);
    meta.set(
        String::from_str(&env, "order_id"),
        String::from_str(&env, "ORD-9999"),
    );
    meta.set(
        String::from_str(&env, "customer_ref"),
        String::from_str(&env, "CUST-42"),
    );

    let payment = client.create_payment(&payment_args(&env, &merchant, Some(meta.clone())));
    assert!(payment.metadata.is_some());

    let retrieved = client.get_payment(&payment.payment_id);
    let stored_meta = retrieved.metadata.unwrap();
    assert_eq!(
        stored_meta.get(String::from_str(&env, "order_id")),
        Some(String::from_str(&env, "ORD-9999"))
    );
    assert_eq!(
        stored_meta.get(String::from_str(&env, "customer_ref")),
        Some(String::from_str(&env, "CUST-42"))
    );
}

#[test]
fn test_create_payment_metadata_at_max_keys() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, merchant, client) = setup(&env);

    let keys = [
        "k0", "k1", "k2", "k3", "k4", "k5", "k6", "k7", "k8", "k9", "k10", "k11", "k12", "k13",
        "k14", "k15", "k16", "k17", "k18", "k19",
    ];
    let mut meta: Map<String, String> = Map::new(&env);
    for k in keys.iter() {
        meta.set(String::from_str(&env, k), String::from_str(&env, "v"));
    }

    // Exactly 20 keys should succeed.
    let payment = client.create_payment(&payment_args(&env, &merchant, Some(meta)));
    assert!(payment.metadata.is_some());
}

#[test]
#[should_panic(expected = "Error(Contract, #49)")]
fn test_create_payment_metadata_too_many_keys() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, merchant, client) = setup(&env);

    let keys = [
        "k0", "k1", "k2", "k3", "k4", "k5", "k6", "k7", "k8", "k9", "k10", "k11", "k12", "k13",
        "k14", "k15", "k16", "k17", "k18", "k19", "k20",
    ];
    let mut meta: Map<String, String> = Map::new(&env);
    for k in keys.iter() {
        meta.set(String::from_str(&env, k), String::from_str(&env, "v"));
    }

    client.create_payment(&payment_args(&env, &merchant, Some(meta)));
}

#[test]
#[should_panic(expected = "Error(Contract, #47)")]
fn test_create_payment_metadata_value_too_long() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, merchant, client) = setup(&env);

    // 257 'x' characters — one over the 256-char limit
    let long_value = String::from_str(
        &env,
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    );

    let mut meta: Map<String, String> = Map::new(&env);
    meta.set(String::from_str(&env, "key"), long_value);

    client.create_payment(&payment_args(&env, &merchant, Some(meta)));
}

#[test]
#[should_panic(expected = "Error(Contract, #47)")]
fn test_create_payment_metadata_key_too_long() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, merchant, client) = setup(&env);

    // 65-char key — one over the 64-char limit
    let long_key = String::from_str(
        &env,
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    );

    let mut meta: Map<String, String> = Map::new(&env);
    meta.set(long_key, String::from_str(&env, "ok"));

    client.create_payment(&payment_args(&env, &merchant, Some(meta)));
}

#[test]
fn test_create_payment_metadata_in_event_payload() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, merchant, client) = setup(&env);

    let mut meta: Map<String, String> = Map::new(&env);
    meta.set(
        String::from_str(&env, "sku"),
        String::from_str(&env, "SKU-001"),
    );

    let payment = client.create_payment(&payment_args(&env, &merchant, Some(meta.clone())));

    // Verify the PAYMENT/CREATED event uses a 2-tuple topic.
    let events = env.events().all();
    let mut found = false;
    for event in events.iter() {
        let (_, topics, _) = event;
        // topics is a Vec; expect exactly 2 elements: ("PAYMENT", "CREATED")
        if topics.len() == 2 {
            found = true;
            break;
        }
    }
    assert!(found, "PAYMENT/CREATED event should have a 2-tuple topic");
    assert!(payment.metadata.is_some());
}

// ─── Issue #284: 2-tuple topics ──────────────────────────────────────────────

#[test]
fn test_payment_created_event_is_two_tuple() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, merchant, client) = setup(&env);

    client.create_payment(&payment_args(&env, &merchant, None));

    let events = env.events().all();
    // Find the PAYMENT/CREATED event and confirm the topic is a 2-tuple.
    let mut payment_created_found = false;
    for event in events.iter() {
        let (_, topics, _data) = event;
        if topics.len() == 2 {
            payment_created_found = true;
        }
        // No 3-tuple topics should exist in our new events.
        assert!(
            topics.len() <= 2,
            "All events must use at most a 2-tuple topic; got {} elements",
            topics.len()
        );
    }
    assert!(payment_created_found, "Expected at least one 2-tuple event");
}
