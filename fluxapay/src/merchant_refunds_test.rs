//! Issue #1201: `RefundManager::get_merchant_refunds` — a paginated,
//! optionally status-filtered view of every refund across a merchant's
//! payments, for dashboard reconciliation (the refund-side counterpart to
//! `PaymentProcessor::get_merchant_payments_full`).

use crate::refund_manager::{RefundManager, RefundManagerClient};
use crate::{payment_id_to_key, DataKey, PaymentCharge, PaymentStatus, RefundStatus};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    vec, Address, Env, String, Symbol, Vec,
};

fn setup(env: &Env) -> (Address, RefundManagerClient<'_>, Address) {
    let admin = Address::generate(env);
    let usdc_admin = Address::generate(env);
    let usdc_addr = env.register_stellar_asset_contract(usdc_admin);

    let refund_manager_addr = env.register_contract(None, RefundManager);
    let client = RefundManagerClient::new(env, &refund_manager_addr);
    client.initialize_refund_manager(&admin, &usdc_addr);

    (admin, client, refund_manager_addr)
}

/// Writes a minimal `Confirmed` `PaymentCharge` directly into the
/// `RefundManager` contract's own storage, and appends it to the
/// `DataKey::MerchantPayments` index `get_merchant_refunds` walks — mirroring
/// the direct-storage-seeding pattern already used elsewhere in this crate's
/// tests (see `fluxapay/src/test.rs`), since in this standalone deployment
/// there is no separate `PaymentProcessor` contract to create the payment for us.
fn seed_confirmed_payment(
    env: &Env,
    contract_id: &Address,
    payment_id: &String,
    merchant_id: &Address,
    amount: i128,
    confirmed_at: u64,
) {
    let id = payment_id.clone();
    let payment = PaymentCharge {
        payment_id: id.clone(),
        merchant_id: merchant_id.clone(),
        amount,
        currency: Symbol::new(env, "USDC"),
        deposit_address: Address::generate(env),
        status: PaymentStatus::Confirmed,
        payer_address: None,
        transaction_hash: None,
        created_at: confirmed_at,
        confirmed_at: Some(confirmed_at),
        expires_at: confirmed_at + 3600,
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
        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id_to_key(env, &id)), &payment);

        let mut ids: Vec<String> = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantPayments(merchant_id.clone()))
            .unwrap_or_else(|| vec![env]);
        ids.push_back(id);
        env.storage()
            .persistent()
            .set(&DataKey::MerchantPayments(merchant_id.clone()), &ids);
    });
}

#[test]
fn test_get_merchant_refunds_filters_by_status() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client, contract_id) = setup(&env);
    // Comfortably past confirmed_at (100_000) + the default refund cooldown
    // (300s), so `create_refund` below doesn't reject on cooldown.
    env.ledger().with_mut(|li| li.timestamp = 200_000);

    let merchant = Address::generate(&env);
    seed_confirmed_payment(
        &env,
        &contract_id,
        &String::from_str(&env, "pay_1"),
        &merchant,
        1_000,
        100_000,
    );
    seed_confirmed_payment(
        &env,
        &contract_id,
        &String::from_str(&env, "pay_2"),
        &merchant,
        2_000,
        100_000,
    );
    seed_confirmed_payment(
        &env,
        &contract_id,
        &String::from_str(&env, "pay_3"),
        &merchant,
        3_000,
        100_000,
    );

    let reason = String::from_str(&env, "not as described");
    let _refund_1 = client.create_refund(
        &String::from_str(&env, "pay_1"),
        &100,
        &reason,
        &merchant,
    );
    let refund_2 = client.create_refund(
        &String::from_str(&env, "pay_2"),
        &200,
        &reason,
        &merchant,
    );
    let _refund_3 = client.create_refund(
        &String::from_str(&env, "pay_3"),
        &300,
        &reason,
        &merchant,
    );

    // Cancel one of the three, leaving two Pending and one Cancelled.
    client.cancel_refund(&merchant, &refund_2);

    // No status filter: all three refunds come back.
    let all = client.get_merchant_refunds(&merchant, &0, &50, &None);
    assert_eq!(all.len(), 3);

    // Status filter: only the two still-Pending refunds.
    let pending = client.get_merchant_refunds(&merchant, &0, &50, &Some(RefundStatus::Pending));
    assert_eq!(pending.len(), 2);
    for refund in pending.iter() {
        assert_eq!(refund.status, RefundStatus::Pending);
    }

    // Status filter: the single Cancelled refund.
    let cancelled =
        client.get_merchant_refunds(&merchant, &0, &50, &Some(RefundStatus::Cancelled));
    assert_eq!(cancelled.len(), 1);
    assert_eq!(cancelled.get(0).unwrap().refund_id, refund_2);
    assert_eq!(cancelled.get(0).unwrap().status, RefundStatus::Cancelled);

    // A different merchant with no payments at all gets an empty vec, not an error.
    let other_merchant = Address::generate(&env);
    let none = client.get_merchant_refunds(&other_merchant, &0, &50, &None);
    assert_eq!(none.len(), 0);
}

#[test]
fn test_get_merchant_refunds_pagination_offset_and_limit_cap() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client, contract_id) = setup(&env);
    env.ledger().with_mut(|li| li.timestamp = 200_000);

    let merchant = Address::generate(&env);
    let reason = String::from_str(&env, "duplicate charge");

    // Create 5 payments, each with exactly one refund, for a merchant.
    let mut refund_ids: Vec<String> = vec![&env];
    for i in 0..5u32 {
        let payment_id = crate::format_id(&env, "pay_", i as u64);
        seed_confirmed_payment(&env, &contract_id, &payment_id, &merchant, 1_000, 100_000);
        let refund_id = client.create_refund(&payment_id, &50, &reason, &merchant);
        refund_ids.push_back(refund_id);
    }

    // limit is silently capped at 50 — asking for 1000 with 5 total refunds
    // just returns all 5, it does not error.
    let capped = client.get_merchant_refunds(&merchant, &0, &1000, &None);
    assert_eq!(capped.len(), 5);

    // A 2-item page starting at offset 0, matching the first two refunds
    // created (insertion order, since each payment has exactly one refund).
    let page_1 = client.get_merchant_refunds(&merchant, &0, &2, &None);
    assert_eq!(page_1.len(), 2);
    assert_eq!(page_1.get(0).unwrap().refund_id, refund_ids.get(0).unwrap());
    assert_eq!(page_1.get(1).unwrap().refund_id, refund_ids.get(1).unwrap());

    // The next 2-item page, starting at offset 2.
    let page_2 = client.get_merchant_refunds(&merchant, &2, &2, &None);
    assert_eq!(page_2.len(), 2);
    assert_eq!(page_2.get(0).unwrap().refund_id, refund_ids.get(2).unwrap());
    assert_eq!(page_2.get(1).unwrap().refund_id, refund_ids.get(3).unwrap());

    // Offset at the exact total: empty, not an error.
    let page_at_end = client.get_merchant_refunds(&merchant, &5, &2, &None);
    assert_eq!(page_at_end.len(), 0);

    // Offset well beyond the total: still empty, not an error.
    let page_beyond = client.get_merchant_refunds(&merchant, &999, &2, &None);
    assert_eq!(page_beyond.len(), 0);

    // A zero limit returns an empty page rather than panicking or erroring.
    let zero_limit = client.get_merchant_refunds(&merchant, &0, &0, &None);
    assert_eq!(zero_limit.len(), 0);
}
