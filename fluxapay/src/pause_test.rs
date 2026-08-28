#![cfg(test)]

use crate::{CreatePaymentArgs, PauseInfo, PauseState, PaymentProcessorClient};
use soroban_sdk::{
    testutils::Address as _, testutils::Ledger as _, vec, Address, Env, String, Symbol,
};

fn assert_pause_state(
    state: &PauseState,
    paused: bool,
    reason: &String,
    admin: Option<&Address>,
    timestamp: u64,
) {
    assert_eq!(state.paused, paused);
    assert_eq!(&state.reason, reason);
    assert_eq!(state.admin.as_ref(), admin);
    assert_eq!(state.timestamp, timestamp);
}

#[test]
fn test_pause_initial_state() {
    let env = Env::default();
    env.mock_all_auths();

    let processor_id = env.register(crate::PaymentProcessor, ());
    let client = PaymentProcessorClient::new(&env, &processor_id);

    let admin = Address::generate(&env);

    client.initialize_payment_processor(&admin);

    let info: PauseInfo = client.get_pause_info();
    let empty = String::from_str(&env, "");
    assert_pause_state(&info.global, false, &empty, None, 0);
    assert_pause_state(&info.creation, false, &empty, None, 0);
}

#[test]
fn test_global_pause_blocks_creation() {
    let env = Env::default();
    env.mock_all_auths();

    let merchant = Address::generate(&env);
    let processor_id = env.register(crate::PaymentProcessor, ());
    let client = PaymentProcessorClient::new(&env, &processor_id);

    let admin = Address::generate(&env);

    client.initialize_payment_processor(&admin);

    // Grant merchant role
    client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);

    // Set global pause
    let pause_ts = env.ledger().timestamp();
    let reason = String::from_str(&env, "Global maintenance");
    client.set_global_pause(&admin, &true, &reason);

    let info: PauseInfo = client.get_pause_info();
    assert_pause_state(&info.global, true, &reason, Some(&admin), pause_ts);
    assert_pause_state(&info.creation, false, &String::from_str(&env, ""), None, 0);

    // Try to create payment
    let res = client.try_create_payment(&CreatePaymentArgs {
        payment_id: String::from_str(&env, "p1"),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 100,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: None,
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
    });

    assert!(res.is_err());
}

#[test]
fn test_creation_pause_blocks_only_creation() {
    let env = Env::default();
    env.mock_all_auths();

    let merchant = Address::generate(&env);
    let oracle = Address::generate(&env);
    let processor_id = env.register(crate::PaymentProcessor, ());
    let client = PaymentProcessorClient::new(&env, &processor_id);

    let admin = Address::generate(&env);

    client.initialize_payment_processor(&admin);

    client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);

    // Set creation pause
    let pause_ts = env.ledger().timestamp();
    let reason = String::from_str(&env, "High load");
    client.set_creation_pause(&admin, &true, &reason);

    let info: PauseInfo = client.get_pause_info();
    assert_pause_state(&info.global, false, &String::from_str(&env, ""), None, 0);
    assert_pause_state(&info.creation, true, &reason, Some(&admin), pause_ts);

    // create_payment should fail
    let res = client.try_create_payment(&CreatePaymentArgs {
        payment_id: String::from_str(&env, "p1"),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 100,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: None,
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
    });
    assert!(res.is_err());

    // verify_payment should still work (won't actually succeed because payment doesn't exist, but won't fail with ContractPaused)
    let res_verify = client.try_verify_payment(
        &oracle,
        &String::from_str(&env, "p1"),
        &soroban_sdk::BytesN::from_array(&env, &[0u8; 32]),
        &Address::generate(&env),
        &100,
    );

    // It should fail with PaymentNotFound (404), not ContractPaused (17)
    // We check the error by seeing if it's NOT the pause error
    if let Err(Ok(crate::Error::ContractPaused)) = res_verify {
        panic!("Should not be blocked by pause");
    }
}

#[test]
fn test_all_write_ops_blocked_when_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let merchant = Address::generate(&env);
    let operator = Address::generate(&env);
    let requester = Address::generate(&env);
    let processor_id = env.register(crate::PaymentProcessor, ());
    let client = PaymentProcessorClient::new(&env, &processor_id);
    let admin = Address::generate(&env);

    client.initialize_payment_processor(&admin);

    // Grant roles
    client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    // Create a payment first (before pause)
    let payment_id = String::from_str(&env, "pay1");
    let deposit = Address::generate(&env);
    let _ = client.create_payment(&CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 1000,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: deposit.clone(),
        expires_at: None,
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
    });

    // Set global pause
    let reason = String::from_str(&env, "Emergency pause");
    client.set_global_pause(&admin, &true, &reason);

    // Test settle_payment is blocked
    let settle_res = client.try_settle_payment(&operator, &payment_id, &vec![&env]);
    assert!(
        settle_res.is_err(),
        "settle_payment should be blocked by pause"
    );

    // Test process_refund is blocked (after creating a refund)
    let _ = client.try_create_refund(
        &requester,
        &payment_id,
        &100,
        &String::from_str(&env, "Too much paid"),
    );
    let refund_id = String::from_str(&env, "ref1");
    let process_res = client.try_process_refund(&operator, &refund_id);
    assert!(
        process_res.is_err(),
        "process_refund should be blocked by pause"
    );

    // Test create_dispute is blocked
    let dispute_res = client.try_create_dispute(
        &payment_id,
        &100,
        &String::from_str(&env, "Disputed"),
        &String::from_str(&env, "Evidence"),
        &requester,
        &vec![&env],
    );
    assert!(
        dispute_res.is_err(),
        "create_dispute should be blocked by pause"
    );

    // Test batch_expire_payments is blocked
    let expire_res = client.try_batch_expire_payments(&vec![&env, &payment_id]);
    assert!(
        expire_res.is_err(),
        "batch_expire_payments should be blocked by pause"
    );

    // Test swap_and_pay is blocked
    let swap_res = client.try_swap_and_pay(&crate::SwapAndPayArgs {
        payer: Address::generate(&env),
        payment_id: String::from_str(&env, "pay2"),
        merchant_id: merchant.clone(),
        amount: 500,
        amount_in: 600,
        amount_out_min: 500,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: deposit.clone(),
        expires_at: None,
        token_in: Address::generate(&env),
        path: vec![&env],
        dex_router: Address::generate(&env),
        fx_oracle: None,
        oracle_pair: None,
        max_deviation_bps: 100,
    });
    assert!(swap_res.is_err(), "swap_and_pay should be blocked by pause");
}

#[test]
fn test_pause_unpause_cycle_exposes_pause_state_fields() {
    let env = Env::default();
    env.mock_all_auths();

    let processor_id = env.register(crate::PaymentProcessor, ());
    let client = PaymentProcessorClient::new(&env, &processor_id);
    let admin = Address::generate(&env);
    client.initialize_payment_processor(&admin);

    let empty = String::from_str(&env, "");
    let initial: PauseInfo = client.get_pause_info();
    assert_pause_state(&initial.global, false, &empty, None, 0);
    assert_pause_state(&initial.creation, false, &empty, None, 0);

    let pause_ts = 1_700_000_000u64;
    env.ledger().set_timestamp(pause_ts);
    let pause_reason = String::from_str(&env, "Scheduled maintenance");
    client.set_global_pause(&admin, &true, &pause_reason);

    let paused: PauseInfo = client.get_pause_info();
    assert_pause_state(&paused.global, true, &pause_reason, Some(&admin), pause_ts);
    assert_pause_state(&paused.creation, false, &empty, None, 0);

    let unpause_ts = pause_ts + 120;
    env.ledger().set_timestamp(unpause_ts);
    let unpause_reason = String::from_str(&env, "Maintenance complete");
    client.set_global_pause(&admin, &false, &unpause_reason);

    let unpaused: PauseInfo = client.get_pause_info();
    assert_pause_state(
        &unpaused.global,
        false,
        &unpause_reason,
        Some(&admin),
        unpause_ts,
    );
    assert_pause_state(&unpaused.creation, false, &empty, None, 0);
}
