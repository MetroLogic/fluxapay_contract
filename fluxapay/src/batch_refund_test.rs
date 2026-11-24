#![cfg(test)]

//! Tests for `RefundManager::batch_process_refunds` (Issue #1199).
//!
//! Mirrors the batch-boundary / partial-failure coverage already in place for
//! `batch_create_disputes` / `batch_resolve_disputes` (see `dispute_test.rs`).

use super::*;
use access_control::role_settlement_operator;
use soroban_sdk::{
    testutils::{Address as _, Events as _},
    token, Address, Env, String, Symbol, TryIntoVal,
};

fn setup_refund_manager(env: &Env) -> (Address, RefundManagerClient<'_>) {
    let contract_id = env.register(RefundManager, ());
    let client = RefundManagerClient::new(env, &contract_id);
    let admin = Address::generate(env);

    let token_admin = Address::generate(env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    client.initialize_refund_manager(&admin, &usdc_token);

    let token_admin_client = token::StellarAssetClient::new(env, &usdc_token);
    token_admin_client.mint(&contract_id, &1_000_000_000_000i128);

    (admin, client)
}

/// Registers a backing payment and creates a pending refund against it,
/// returning the new `refund_id`.
fn create_pending_refund(
    env: &Env,
    client: &RefundManagerClient,
    idx: u32,
    amount: i128,
) -> String {
    let payment_id = crate::utils::format_id(env, "batch_refund_pay_", idx as u64);
    let merchant_id = Address::generate(env);
    client.register_payment(
        &payment_id,
        &merchant_id,
        &(amount * 10),
        &Symbol::new(env, "USDC"),
    );
    client.create_refund(
        &payment_id,
        &amount,
        &String::from_str(env, "batch test refund"),
        &merchant_id,
    )
}

#[test]
fn test_batch_process_refunds_full_success() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);
    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    let mut ids = soroban_sdk::vec![&env];
    for i in 0..3u32 {
        ids.push_back(create_pending_refund(&env, &client, i, 100i128));
    }

    let results = client.batch_process_refunds(&operator, &ids, &20u32);
    assert_eq!(results.len(), 3);
    for r in results.iter() {
        match r {
            crate::RefundBatchItemResult::Ok(_) => {}
            crate::RefundBatchItemResult::Err(id, code) => {
                panic!("unexpected err for {id}: {code}")
            }
        }
    }

    for id in ids.iter() {
        let refund = client.get_refund(&id);
        assert_eq!(refund.status, RefundStatus::Completed);
    }

    assert!(env.events().all().iter().any(|(_, topics, _)| {
        if topics.len() < 2 {
            return false;
        }
        let ns: Result<Symbol, _> = topics.get(0).unwrap().try_into_val(&env);
        let name: Result<Symbol, _> = topics.get(1).unwrap().try_into_val(&env);
        matches!(
            (ns, name),
            (Ok(a), Ok(b))
                if a == Symbol::new(&env, "REFUND") && b == Symbol::new(&env, "BATCH_PROCESSED")
        )
    }));
}

#[test]
fn test_batch_process_refunds_mixed_success_fail() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);
    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    let ok_id = create_pending_refund(&env, &client, 0, 100i128);
    let missing_id = String::from_str(&env, "does_not_exist_refund");

    let ids = soroban_sdk::vec![&env, ok_id.clone(), missing_id.clone()];
    let results = client.batch_process_refunds(&operator, &ids, &20u32);
    assert_eq!(results.len(), 2);

    match results.get(0).unwrap() {
        crate::RefundBatchItemResult::Ok(id) => assert_eq!(id, ok_id),
        crate::RefundBatchItemResult::Err(id, code) => {
            panic!("expected success for {id}, got err {code}")
        }
    }
    match results.get(1).unwrap() {
        crate::RefundBatchItemResult::Ok(id) => panic!("expected failure for {id}"),
        crate::RefundBatchItemResult::Err(id, code) => {
            assert_eq!(id, missing_id);
            assert_eq!(code, Error::RefundNotFound as u32);
        }
    }

    // The failing second item must not have reverted the first item's success.
    let refund = client.get_refund(&ok_id);
    assert_eq!(refund.status, RefundStatus::Completed);
}

#[test]
fn test_batch_process_refunds_full_failure() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);
    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    let ids = soroban_sdk::vec![
        &env,
        String::from_str(&env, "nope_1"),
        String::from_str(&env, "nope_2"),
    ];

    let results = client.batch_process_refunds(&operator, &ids, &20u32);
    assert_eq!(results.len(), 2);
    assert!(matches!(
        results.get(0).unwrap(),
        crate::RefundBatchItemResult::Err(_, _)
    ));
    assert!(matches!(
        results.get(1).unwrap(),
        crate::RefundBatchItemResult::Err(_, _)
    ));
}

#[test]
fn test_batch_process_refunds_rejects_oversized_batch() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);
    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);

    let mut ids = soroban_sdk::vec![&env];
    for i in 0..21u32 {
        ids.push_back(crate::utils::format_id(&env, "x", i as u64));
    }
    let result = client.try_batch_process_refunds(&operator, &ids, &20u32);
    assert_eq!(result, Err(Ok(Error::BatchTooLarge)));

    // `max_batch` itself above the hard cap is rejected even with an empty batch.
    let result2 = client.try_batch_process_refunds(&operator, &soroban_sdk::vec![&env], &21u32);
    assert_eq!(result2, Err(Ok(Error::BatchTooLarge)));
}

#[test]
fn test_batch_process_refunds_requires_authorized_caller() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_refund_manager(&env);
    let impostor = Address::generate(&env);

    let refund_id = create_pending_refund(&env, &client, 1, 75i128);
    let ids = soroban_sdk::vec![&env, refund_id.clone()];

    // `impostor` holds no settlement-operator/oracle role and is not the
    // refund's own requester, so the item fails per-item rather than the
    // whole call panicking.
    let results = client.batch_process_refunds(&impostor, &ids, &20u32);
    assert_eq!(results.len(), 1);
    match results.get(0).unwrap() {
        crate::RefundBatchItemResult::Ok(id) => panic!("expected failure for {id}"),
        crate::RefundBatchItemResult::Err(id, code) => {
            assert_eq!(id, refund_id);
            assert_eq!(code, Error::Unauthorized as u32);
        }
    }
}
