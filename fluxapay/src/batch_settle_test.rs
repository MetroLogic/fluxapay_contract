//! Issue #1200: `PaymentProcessor::batch_settle_payments` — settle up to
//! `MAX_BATCH_SETTLE_PAYMENTS` confirmed payments in one invocation, with
//! per-item results instead of a whole-batch revert on a single failure.

use crate::payment_processor::{PaymentProcessor, PaymentProcessorClient};
use crate::{
    payment_id_to_key, BatchSettlePaymentItem, DataKey, Error, PaymentCharge, PaymentStatus,
};
use soroban_sdk::{testutils::Address as _, vec, Address, Env, String, Symbol, Vec};

fn setup(env: &Env) -> (Address, PaymentProcessorClient<'_>, Address, Address) {
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, PaymentProcessor);
    let client = PaymentProcessorClient::new(env, &contract_id);
    client.initialize_payment_processor(&admin);

    let operator = Address::generate(env);
    client.grant_role(&admin, &Symbol::new(env, "SETTLEMENT_OPERATOR"), &operator);

    (admin, client, contract_id, operator)
}

/// Writes a minimal `Confirmed` `PaymentCharge` directly into the
/// `PaymentProcessor` contract's own storage (mirroring the direct-storage
/// seeding pattern already used elsewhere in this crate's tests), so
/// `settle_payment` has something real to settle without going through the
/// full `create_payment` / `verify_payment` flow. `token_address` is left
/// `None` so settlement resolves no token client and skips any actual
/// transfer, letting this test focus purely on `batch_settle_payments`'
/// per-item control flow rather than token mechanics already covered by
/// `settlement_test.rs`.
fn seed_confirmed_payment(
    env: &Env,
    contract_id: &Address,
    payment_id: &String,
    merchant_id: &Address,
    amount: i128,
) {
    let payment = PaymentCharge {
        payment_id: payment_id.clone(),
        merchant_id: merchant_id.clone(),
        amount,
        currency: Symbol::new(env, "USDC"),
        deposit_address: Address::generate(env),
        status: PaymentStatus::Confirmed,
        payer_address: None,
        transaction_hash: None,
        created_at: 100_000,
        confirmed_at: Some(100_000),
        expires_at: 200_000,
        amount_received: Some(amount),
        memo: None,
        memo_type: None,
        token_address: None,
        metadata_hash: None,
        fx_rate: None,
        fx_rate_at: None,
        original_token: None,
        swap_path: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
        payment_link_id: None,
        allow_partial: None,
        tip_enabled: false,
        tip_amount: None,
    };

    env.as_contract(contract_id, || {
        env.storage().persistent().set(
            &DataKey::Payment(payment_id_to_key(env, payment_id)),
            &payment,
        );
    });
}

#[test]
fn test_batch_settle_payments_all_succeed() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client, contract_id, operator) = setup(&env);

    let merchant = Address::generate(&env);
    let pay_1 = String::from_str(&env, "pay_1");
    let pay_2 = String::from_str(&env, "pay_2");
    let pay_3 = String::from_str(&env, "pay_3");
    seed_confirmed_payment(&env, &contract_id, &pay_1, &merchant, 1_000);
    seed_confirmed_payment(&env, &contract_id, &pay_2, &merchant, 2_000);
    seed_confirmed_payment(&env, &contract_id, &pay_3, &merchant, 3_000);

    let items: Vec<BatchSettlePaymentItem> = vec![
        &env,
        BatchSettlePaymentItem {
            payment_id: pay_1.clone(),
            splits: vec![&env],
        },
        BatchSettlePaymentItem {
            payment_id: pay_2.clone(),
            splits: vec![&env],
        },
        BatchSettlePaymentItem {
            payment_id: pay_3.clone(),
            splits: vec![&env],
        },
    ];

    let results = client.batch_settle_payments(&operator, &items);
    assert_eq!(results.len(), 3);
    for result in results.iter() {
        assert!(result.success);
        assert_eq!(result.error_code, None);
    }

    // Each settled payment actually transitioned to `Settled`.
    for payment_id in [&pay_1, &pay_2, &pay_3] {
        let payment = client.get_payment(payment_id);
        assert_eq!(payment.status, PaymentStatus::Settled);
    }
}

#[test]
fn test_batch_settle_payments_partial_failure_does_not_revert_batch() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client, contract_id, operator) = setup(&env);

    let merchant = Address::generate(&env);
    let pay_ok_1 = String::from_str(&env, "pay_ok_1");
    let pay_missing = String::from_str(&env, "pay_missing");
    let pay_ok_2 = String::from_str(&env, "pay_ok_2");
    seed_confirmed_payment(&env, &contract_id, &pay_ok_1, &merchant, 1_000);
    // `pay_missing` is intentionally never seeded, so `settle_payment` fails
    // for it with `Error::PaymentNotFound`.
    seed_confirmed_payment(&env, &contract_id, &pay_ok_2, &merchant, 2_000);

    let items: Vec<BatchSettlePaymentItem> = vec![
        &env,
        BatchSettlePaymentItem {
            payment_id: pay_ok_1.clone(),
            splits: vec![&env],
        },
        BatchSettlePaymentItem {
            payment_id: pay_missing.clone(),
            splits: vec![&env],
        },
        BatchSettlePaymentItem {
            payment_id: pay_ok_2.clone(),
            splits: vec![&env],
        },
    ];

    let results = client.batch_settle_payments(&operator, &items);
    assert_eq!(results.len(), 3);

    assert!(results.get(0).unwrap().success);
    assert_eq!(results.get(0).unwrap().error_code, None);

    assert!(!results.get(1).unwrap().success);
    assert_eq!(
        results.get(1).unwrap().error_code,
        Some(Error::PaymentNotFound as u32)
    );

    // The failure of item 1 did not revert or block item 2.
    assert!(results.get(2).unwrap().success);
    assert_eq!(results.get(2).unwrap().error_code, None);

    // The two valid payments settled; the batch call itself still returned Ok.
    let settled_1 = client.get_payment(&pay_ok_1);
    assert_eq!(settled_1.status, PaymentStatus::Settled);
    let settled_2 = client.get_payment(&pay_ok_2);
    assert_eq!(settled_2.status, PaymentStatus::Settled);
}

#[test]
fn test_batch_settle_payments_rejects_batches_over_the_cap() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client, _contract_id, operator) = setup(&env);

    // The cap check happens before any per-item processing, so the items
    // don't need to reference real payments to exercise this boundary.
    let mut items: Vec<BatchSettlePaymentItem> = vec![&env];
    let over_cap = PaymentProcessor::MAX_BATCH_SETTLE_PAYMENTS + 1;
    for i in 0..over_cap {
        items.push_back(BatchSettlePaymentItem {
            payment_id: crate::format_id(&env, "overcap_", i as u64),
            splits: vec![&env],
        });
    }

    let result = client.try_batch_settle_payments(&operator, &items);
    assert_eq!(result, Err(Ok(Error::BatchTooLarge)));
}

#[test]
fn test_batch_settle_payments_requires_settlement_operator_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client, contract_id, _operator) = setup(&env);

    let merchant = Address::generate(&env);
    let pay_1 = String::from_str(&env, "pay_1");
    seed_confirmed_payment(&env, &contract_id, &pay_1, &merchant, 1_000);

    let not_an_operator = Address::generate(&env);
    let items: Vec<BatchSettlePaymentItem> = vec![
        &env,
        BatchSettlePaymentItem {
            payment_id: pay_1,
            splits: vec![&env],
        },
    ];

    let result = client.try_batch_settle_payments(&not_an_operator, &items);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}
