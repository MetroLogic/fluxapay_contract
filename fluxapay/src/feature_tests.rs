//! Unit tests for four feature additions:
//!
//! * #638 — idempotency-key support in `create_refund` / `create_refund_idempotent`
//! * #637 — `PaymentLinkManager::batch_create_links`
//! * #635 — `SUBSCRIPTION/PLAN_CREATED` / `SUBSCRIPTION/PLAN_DEACTIVATED` events
//! * #632 — `PaymentProcessor::create_payment_link_invoice`

use crate::{
    CreateLinkArgs, Error, Invoice, LineItem, MaybeFiatConfig, PaymentLink, PaymentLinkManager,
    PaymentLinkManagerClient, PaymentProcessor, PaymentProcessorClient, RefundManager,
    RefundManagerClient,
};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger as _},
    vec, Address, Env, IntoVal, String, Symbol, TryIntoVal, Vec,
};

// ─────────────────────────────────────────────────────────────────────────────
// helpers
// ─────────────────────────────────────────────────────────────────────────────

fn setup_refund_manager(env: &Env) -> (Address, RefundManagerClient<'_>) {
    let contract_id = env.register(RefundManager, ());
    let client = RefundManagerClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let token_admin = Address::generate(env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    client.initialize_refund_manager(&admin, &usdc_token);
    (admin, client)
}

/// Register a `Confirmed` payment on the RefundManager and advance the ledger
/// past the refund cooldown window so `create_refund*` can succeed.
fn refundable_payment(env: &Env, client: &RefundManagerClient, payment_id: &String, amount: i128) {
    let merchant = Address::generate(env);
    client.register_payment(payment_id, &merchant, &amount, &Symbol::new(env, "USDC"));
    // cooldown default is 300s; jump well past it.
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 30 * 24 * 60 * 60);
}

fn events_contain(env: &Env, topic0: &str, topic1: &str) -> bool {
    env.events().all().events().iter().any(|e| {
        let topics: Vec<soroban_sdk::Val> = e.1;
        if topics.len() < 2 {
            return false;
        }
        let t0: Result<Symbol, _> = topics.get(0).unwrap().try_into_val(env);
        let t1: Result<Symbol, _> = topics.get(1).unwrap().try_into_val(env);
        matches!(
            (t0, t1),
            (Ok(a), Ok(b)) if a == Symbol::new(env, topic0) && b == Symbol::new(env, topic1)
        )
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// #638 — refund idempotency key
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn refund_no_idempotency_key_still_works() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_refund_manager(&env);
    let pid = String::from_str(&env, "pay_no_key");
    refundable_payment(&env, &client, &pid, 1_000);
    let requester = Address::generate(&env);

    let rid = client.create_refund(&pid, &400i128, &String::from_str(&env, "r"), &requester);
    assert_eq!(client.get_refund(&rid).amount, 400);
}

#[test]
fn refund_duplicate_key_same_params_returns_existing_id() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_refund_manager(&env);
    let pid = String::from_str(&env, "pay_dup_ok");
    refundable_payment(&env, &client, &pid, 1_000);
    let requester = Address::generate(&env);
    let key = Some(String::from_str(&env, "idem-1"));
    let reason = String::from_str(&env, "duplicate submit");

    let first = client.create_refund_idempotent(&pid, &400i128, &reason, &requester, &key);
    let second = client.create_refund_idempotent(&pid, &400i128, &reason, &requester, &key);

    assert_eq!(first, second);
    // exactly one refund exists for the payment
    assert_eq!(client.get_payment_refunds(&pid).len(), 1);
}

#[test]
fn refund_duplicate_key_different_params_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_refund_manager(&env);
    let pid = String::from_str(&env, "pay_dup_bad");
    refundable_payment(&env, &client, &pid, 1_000);
    let requester = Address::generate(&env);
    let key = Some(String::from_str(&env, "idem-2"));

    let _ = client.create_refund_idempotent(
        &pid,
        &400i128,
        &String::from_str(&env, "first"),
        &requester,
        &key,
    );
    // Same key, different amount → DuplicateIdempotencyKey
    let res = client.try_create_refund_idempotent(
        &pid,
        &500i128,
        &String::from_str(&env, "first"),
        &requester,
        &key,
    );
    assert_eq!(res, Err(Ok(Error::DuplicateIdempotencyKey)));
}

#[test]
fn refund_unique_keys_create_distinct_refunds() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client) = setup_refund_manager(&env);
    let pid = String::from_str(&env, "pay_unique");
    refundable_payment(&env, &client, &pid, 10_000);
    let requester = Address::generate(&env);
    let reason = String::from_str(&env, "r");

    let a = client.create_refund_idempotent(
        &pid,
        &400i128,
        &reason,
        &requester,
        &Some(String::from_str(&env, "k-a")),
    );
    let b = client.create_refund_idempotent(
        &pid,
        &400i128,
        &reason,
        &requester,
        &Some(String::from_str(&env, "k-b")),
    );
    assert_ne!(a, b);
    assert_eq!(client.get_payment_refunds(&pid).len(), 2);
}

// ─────────────────────────────────────────────────────────────────────────────
// #637 — batch_create_links
// ─────────────────────────────────────────────────────────────────────────────

fn link_args(env: &Env, id: &str) -> CreateLinkArgs {
    CreateLinkArgs {
        link_id: String::from_str(env, id),
        amount: Some(1_000i128),
        currency: Symbol::new(env, "USDC"),
        description: String::from_str(env, "batch"),
        expires_at: None,
        max_uses: None,
        direct_transfer: false,
        metadata: None,
        fiat: MaybeFiatConfig::None,
        base_url: None,
    }
}

#[test]
fn batch_create_links_creates_all_and_they_are_retrievable() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(PaymentLinkManager, ());
    let client = PaymentLinkManagerClient::new(&env, &contract_id);
    let merchant = Address::generate(&env);

    let batch = vec![
        &env,
        link_args(&env, "batch_1"),
        link_args(&env, "batch_2"),
        link_args(&env, "batch_3"),
    ];
    let ids = client.batch_create_links(&merchant, &batch);

    assert_eq!(ids.len(), 3);
    assert_eq!(ids.get(0).unwrap(), String::from_str(&env, "batch_1"));
    assert_eq!(ids.get(2).unwrap(), String::from_str(&env, "batch_3"));
    for id in ids.iter() {
        let link = client.get_link(&id);
        assert!(link.active);
        assert_eq!(link.merchant_id, merchant);
    }
}

#[test]
fn batch_create_links_rejects_more_than_50() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(PaymentLinkManager, ());
    let client = PaymentLinkManagerClient::new(&env, &contract_id);
    let merchant = Address::generate(&env);

    let mut batch: Vec<CreateLinkArgs> = vec![&env];
    for i in 0..51u32 {
        batch.push_back(link_args(&env, "x"));
        // give each a unique id so only the cap check can trip
        let last = batch.len() - 1;
        let mut a = batch.get(last).unwrap();
        a.link_id = crate::format_id(&env, "cap_", i as u64);
        batch.set(last, a);
    }
    let res = client.try_batch_create_links(&merchant, &batch);
    assert_eq!(res, Err(Ok(Error::BatchTooLarge)));
}

#[test]
fn batch_create_links_rejects_duplicate_id_atomically() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(PaymentLinkManager, ());
    let client = PaymentLinkManagerClient::new(&env, &contract_id);
    let merchant = Address::generate(&env);

    let batch = vec![
        &env,
        link_args(&env, "dup_a"),
        link_args(&env, "dup_a"), // duplicate within the batch
    ];
    let res = client.try_batch_create_links(&merchant, &batch);
    assert_eq!(res, Err(Ok(Error::PaymentAlreadyExists)));
    // atomicity: nothing was persisted
    assert!(client
        .try_get_link(&String::from_str(&env, "dup_a"))
        .is_err());
}

// ─────────────────────────────────────────────────────────────────────────────
// #635 — subscription plan events
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn create_and_deactivate_subscription_plan_emit_events() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_refund_manager(&env);
    let merchant = Address::generate(&env);
    client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);

    let plan_id = String::from_str(&env, "plan_events");
    client.create_subscription_plan(
        &merchant,
        &plan_id,
        &String::from_str(&env, "Plan"),
        &String::from_str(&env, "desc"),
        &1_000_000i128,
        &Symbol::new(&env, "USDC"),
        &crate::BillingInterval::Weekly,
    );
    assert!(
        events_contain(&env, "SUBSCRIPTION", "PLAN_CREATED"),
        "PLAN_CREATED not emitted"
    );

    client.deactivate_subscription_plan(&merchant, &plan_id);
    assert!(
        events_contain(&env, "SUBSCRIPTION", "PLAN_DEACTIVATED"),
        "PLAN_DEACTIVATED not emitted"
    );
    assert!(!client.get_subscription_plan(&plan_id).active);
}

// ─────────────────────────────────────────────────────────────────────────────
// #632 — create_payment_link_invoice
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn create_payment_link_invoice_links_both_records() {
    let env = Env::default();
    env.mock_all_auths();

    let pp_id = env.register(PaymentProcessor, ());
    let pp = PaymentProcessorClient::new(&env, &pp_id);
    let admin = Address::generate(&env);
    pp.initialize_payment_processor(&admin);

    let plm_id = env.register(PaymentLinkManager, ());
    let plm = PaymentLinkManagerClient::new(&env, &plm_id);

    let merchant = Address::generate(&env);
    let line_items = vec![
        &env,
        LineItem {
            description: String::from_str(&env, "Widget"),
            amount: 2_500i128,
            quantity: 2,
        },
    ];

    let (invoice, link): (Invoice, PaymentLink) = pp.create_payment_link_invoice(
        &merchant,
        &plm_id,
        &String::from_str(&env, "buyer@example.com"),
        &line_items,
        &5_000i128,
        &Symbol::new(&env, "USDC"),
        &(env.ledger().timestamp() + 86_400),
        &CreateLinkArgs {
            link_id: String::from_str(&env, "inv_link_1"),
            amount: Some(5_000i128),
            currency: Symbol::new(&env, "USDC"),
            description: String::from_str(&env, "Invoice INV"),
            expires_at: None,
            max_uses: Some(1),
            direct_transfer: false,
            metadata: None,
            fiat: MaybeFiatConfig::None,
            base_url: None,
        },
    );

    assert_eq!(link.link_id, String::from_str(&env, "inv_link_1"));
    assert_eq!(
        invoice.payment_link_id,
        Some(String::from_str(&env, "inv_link_1"))
    );

    // both records are independently retrievable afterwards
    let fetched_invoice = pp.get_invoice(&invoice.invoice_id);
    assert_eq!(
        fetched_invoice.payment_link_id,
        Some(String::from_str(&env, "inv_link_1"))
    );
    let fetched_link = plm.get_link(&String::from_str(&env, "inv_link_1"));
    assert!(fetched_link.active);
    assert_eq!(fetched_link.merchant_id, merchant);
}

#[test]
fn create_payment_link_invoice_rejects_bad_link_atomically() {
    let env = Env::default();
    env.mock_all_auths();

    let pp_id = env.register(PaymentProcessor, ());
    let pp = PaymentProcessorClient::new(&env, &pp_id);
    let admin = Address::generate(&env);
    pp.initialize_payment_processor(&admin);
    let plm_id = env.register(PaymentLinkManager, ());

    let merchant = Address::generate(&env);
    let res = pp.try_create_payment_link_invoice(
        &merchant,
        &plm_id,
        &String::from_str(&env, "buyer@example.com"),
        &vec![&env],
        &5_000i128,
        &Symbol::new(&env, "USDC"),
        &(env.ledger().timestamp() + 86_400),
        &CreateLinkArgs {
            // invalid link id (spaces) → link creation fails
            link_id: String::from_str(&env, "bad id"),
            amount: None,
            currency: Symbol::new(&env, "USDC"),
            description: String::from_str(&env, "x"),
            expires_at: None,
            max_uses: None,
            direct_transfer: false,
            metadata: None,
            fiat: MaybeFiatConfig::None,
            base_url: None,
        },
    );
    assert!(res.is_err());
    // no invoice was persisted
    assert_eq!(pp.get_merchant_invoices(&merchant).len(), 0);
}

// keep the unused-import checker quiet if a feature test is removed
#[allow(unused_imports)]
use crate as _fluxapay;
#[allow(dead_code)]
fn _use_into_val(env: &Env, a: Address) -> soroban_sdk::Val {
    a.into_val(env)
}
