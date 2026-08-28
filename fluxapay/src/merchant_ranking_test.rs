#![cfg(test)]

//! Issue #628: unit tests for `PaymentProcessor::get_top_merchants`.

use crate::{
    access_control::role_merchant, CreatePaymentArgs, PaymentProcessor, PaymentProcessorClient,
};
use soroban_sdk::{testutils::Address as _, Address, Env, String, Symbol};

fn setup(env: &Env) -> (Address, PaymentProcessorClient<'_>) {
    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize_payment_processor(&admin);
    (admin, client)
}

fn payment_args(
    env: &Env,
    payment_id: &str,
    merchant: &Address,
    amount: i128,
) -> CreatePaymentArgs {
    CreatePaymentArgs {
        payment_id: String::from_str(env, payment_id),
        merchant_id: merchant.clone(),
        payer: None,
        amount,
        currency: Symbol::new(env, "USDC"),
        deposit_address: Address::generate(env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
    }
}

#[test]
fn test_get_top_merchants_ranks_by_volume_descending() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup(&env);

    // Three merchants with distinct total volumes:
    //   merchant_b: 1_000              (1 payment)
    //   merchant_a: 100 + 300  = 400   (2 payments)
    //   merchant_c: 50 + 50 + 50 = 150 (3 payments)
    let merchant_a = Address::generate(&env);
    let merchant_b = Address::generate(&env);
    let merchant_c = Address::generate(&env);
    for m in [&merchant_a, &merchant_b, &merchant_c] {
        client.grant_role(&admin, &role_merchant(&env), m);
    }

    client.create_payment(&payment_args(&env, "a_1", &merchant_a, 100));
    client.create_payment(&payment_args(&env, "a_2", &merchant_a, 300));
    client.create_payment(&payment_args(&env, "b_1", &merchant_b, 1_000));
    client.create_payment(&payment_args(&env, "c_1", &merchant_c, 50));
    client.create_payment(&payment_args(&env, "c_2", &merchant_c, 50));
    client.create_payment(&payment_args(&env, "c_3", &merchant_c, 50));

    let ranked = client.get_top_merchants(&10);
    assert_eq!(ranked.len(), 3);

    let first = ranked.get(0).unwrap();
    let second = ranked.get(1).unwrap();
    let third = ranked.get(2).unwrap();

    assert_eq!(first.merchant_id, merchant_b);
    assert_eq!(first.total_volume, 1_000);
    assert_eq!(first.payment_count, 1);

    assert_eq!(second.merchant_id, merchant_a);
    assert_eq!(second.total_volume, 400);
    assert_eq!(second.payment_count, 2);

    assert_eq!(third.merchant_id, merchant_c);
    assert_eq!(third.total_volume, 150);
    assert_eq!(third.payment_count, 3);
}

#[test]
fn test_get_top_merchants_respects_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup(&env);

    let merchant_a = Address::generate(&env);
    let merchant_b = Address::generate(&env);
    let merchant_c = Address::generate(&env);
    for m in [&merchant_a, &merchant_b, &merchant_c] {
        client.grant_role(&admin, &role_merchant(&env), m);
    }

    client.create_payment(&payment_args(&env, "a_1", &merchant_a, 400));
    client.create_payment(&payment_args(&env, "b_1", &merchant_b, 1_000));
    client.create_payment(&payment_args(&env, "c_1", &merchant_c, 150));

    // limit smaller than the merchant count -> only the top `limit` entries.
    let top_two = client.get_top_merchants(&2);
    assert_eq!(top_two.len(), 2);
    assert_eq!(top_two.get(0).unwrap().merchant_id, merchant_b);
    assert_eq!(top_two.get(1).unwrap().merchant_id, merchant_a);

    // limit == 0 is treated as the cap -> every tracked merchant is returned.
    let all = client.get_top_merchants(&0);
    assert_eq!(all.len(), 3);

    // limit above the 100 cap is clamped, not rejected.
    let clamped = client.get_top_merchants(&5_000);
    assert_eq!(clamped.len(), 3);
}

#[test]
fn test_get_top_merchants_empty_when_no_payments() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup(&env);

    let ranked = client.get_top_merchants(&10);
    assert_eq!(ranked.len(), 0);
}
