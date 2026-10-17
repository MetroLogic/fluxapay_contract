#![cfg(test)]

use super::stream::{PaymentStreaming, PaymentStreamingClient, StreamError};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger as _},
    token, Address, Env, String, Symbol, TryIntoVal, Vec,
};

fn setup(env: &Env) -> (PaymentStreamingClient<'_>, Address, Address, Address) {
    let contract_id = env.register(PaymentStreaming, ());
    let client = PaymentStreamingClient::new(env, &contract_id);
    let token_admin = Address::generate(env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let sender = Address::generate(env);
    let receiver = Address::generate(env);

    token::StellarAssetClient::new(env, &token_id).mint(&sender, &10_000i128);

    (client, sender, receiver, token_id)
}

fn has_cliff_event(env: &Env) -> bool {
    env.events().all().events().iter().any(|event| {
        let topics: Vec<soroban_sdk::Val> = event.1;
        if topics.len() < 2 {
            return false;
        }
        let namespace: Result<Symbol, _> = topics.get(0).unwrap().try_into_val(env);
        let name: Result<Symbol, _> = topics.get(1).unwrap().try_into_val(env);
        matches!(
            (namespace, name),
            (Ok(namespace), Ok(name))
                if namespace == Symbol::new(env, "STREAM")
                    && name == Symbol::new(env, "CLIFF_REACHED")
        )
    })
}

fn count_cliff_events(env: &Env) -> usize {
    env.events()
        .all()
        .events()
        .iter()
        .filter(|event| {
            let topics: Vec<soroban_sdk::Val> = event.1;
            if topics.len() < 2 {
                return false;
            }
            let namespace: Result<Symbol, _> = topics.get(0).unwrap().try_into_val(env);
            let name: Result<Symbol, _> = topics.get(1).unwrap().try_into_val(env);
            matches!(
                (namespace, name),
                (Ok(namespace), Ok(name))
                    if namespace == Symbol::new(env, "STREAM")
                        && name == Symbol::new(env, "CLIFF_REACHED")
            )
        })
        .count()
}

#[test]
fn test_vesting_stream_rejects_withdrawal_before_cliff() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);
    let stream_id = String::from_str(&env, "vesting_before_cliff");
    let destination = Address::generate(&env);
    let cliff_at = env.ledger().timestamp() + 100;

    client.create_stream_with_cliff(
        &sender,
        &receiver,
        &token,
        &10i128,
        &5_000i128,
        &stream_id,
        &None::<i128>,
        &cliff_at,
    );
    client.set_stream_destination(&receiver, &stream_id, &destination);
    client.approve_stream_milestone(&sender, &stream_id);

    env.ledger().set_timestamp(cliff_at - 1);
    client.decrease_rate_per_second(&sender, &stream_id, &5i128);

    assert_eq!(client.get_accrued_amount(&stream_id), 0);
    assert_eq!(
        client.try_trigger_withdrawal(&stream_id),
        Err(Ok(StreamError::CliffNotReached))
    );
    assert!(!has_cliff_event(&env));

    client.cancel_stream(&sender, &stream_id);
    assert_eq!(
        token::StellarAssetClient::new(&env, &token).balance(&receiver),
        0
    );
    assert_eq!(
        token::StellarAssetClient::new(&env, &token).balance(&sender),
        10_000
    );
}

#[test]
fn test_vesting_stream_releases_accrual_at_cliff_and_emits_once() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);
    let token_client = token::StellarAssetClient::new(&env, &token);
    let stream_id = String::from_str(&env, "vesting_at_cliff");
    let destination = Address::generate(&env);
    let started_at = env.ledger().timestamp();
    let cliff_at = started_at + 100;

    client.create_stream_with_cliff(
        &sender,
        &receiver,
        &token,
        &10i128,
        &5_000i128,
        &stream_id,
        &None::<i128>,
        &cliff_at,
    );
    client.set_stream_destination(&receiver, &stream_id, &destination);
    client.approve_stream_milestone(&sender, &stream_id);

    env.ledger().set_timestamp(cliff_at);

    assert_eq!(client.get_accrued_amount(&stream_id), 1_000);
    client.trigger_withdrawal(&stream_id);
    assert_eq!(token_client.balance(&destination), 1_000);
    assert_eq!(count_cliff_events(&env), 1);

    env.ledger().set_timestamp(cliff_at + 10);
    assert_eq!(client.get_accrued_amount(&stream_id), 100);
    client.trigger_withdrawal(&stream_id);
    assert_eq!(token_client.balance(&destination), 1_100);
    assert_eq!(count_cliff_events(&env), 1);
}
