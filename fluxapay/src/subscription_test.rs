use crate::{
    access_control::role_oracle, BillingInterval, Error, RefundManager, RefundManagerClient,
    SubscriptionStatus,
};
use soroban_sdk::{
    testutils::Address as _, testutils::Events, testutils::Ledger as _, token, Address, Env,
    String, Symbol, vec, TryIntoVal,
};

// ── Shared setup helpers ──────────────────────────────────────────────────────

fn setup_refund_manager(env: &Env) -> (Address, RefundManagerClient<'_>, Address) {
    let contract_id = env.register(RefundManager, ());
    let client = RefundManagerClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let token_admin = Address::generate(env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    client.initialize_refund_manager(&admin, &usdc_token);

    (admin, client, usdc_token)
}

/// Create a merchant with MERCHANT role and a subscription plan, returning
/// `(client, admin, merchant, plan_id)`.
fn setup_with_plan(env: &Env) -> (RefundManagerClient<'_>, Address, Address, String, Address) {
    let (admin, client, usdc_token) = setup_refund_manager(env);

    let merchant = Address::generate(env);
    client.grant_role(&admin, &Symbol::new(env, "MERCHANT"), &merchant);

    let plan_id = String::from_str(env, "plan_weekly");
    client.create_subscription_plan(
        &merchant,
        &plan_id,
        &String::from_str(env, "Weekly Plan"),
        &String::from_str(env, "Billed weekly"),
        &1_000_000_i128,
        &Symbol::new(env, "USDC"),
        &BillingInterval::Weekly,
    );

    (client, admin, merchant, plan_id, usdc_token)
}

// ── process_due_subscriptions stub tests ─────────────────────────────────────

/// Operator calling process_due_subscriptions with no due subscriptions gets 0.
/// TODO(#302): add a full due-subscription processing test once the
/// subscription processor implementation is complete.
#[test]
fn test_process_due_subscriptions_operator_gets_zero_when_none_due() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client, _usdc_token) = setup_refund_manager(&env);
    let operator = Address::generate(&env);

    client.grant_role(&admin, &role_oracle(&env), &operator);

    // TODO(#302): add a full due-subscription processing test once the
    // subscription processor implementation is complete.
    let processed = client.process_due_subscriptions(&operator);

    assert_eq!(processed, 0);
}

/// Non-operator callers must get Error::Unauthorized.
#[test]
fn test_process_due_subscriptions_rejects_non_operator() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client, _usdc_token) = setup_refund_manager(&env);
    let non_operator = Address::generate(&env);

    let result = client.try_process_due_subscriptions(&non_operator);

    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

// ── Full subscription subsystem tests ────────────────────────────────────────

/// A merchant with the MERCHANT role can create a subscription plan and it is stored.
#[test]
fn test_create_subscription_plan_by_merchant_stores_plan() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, merchant, plan_id, _usdc_token) = setup_with_plan(&env);

    let plan = client.get_subscription_plan(&plan_id);
    assert_eq!(plan.plan_id, plan_id);
    assert_eq!(plan.merchant_id, merchant);
    assert_eq!(plan.amount, 1_000_000_i128);
    assert!(plan.active);
}

/// A caller without the MERCHANT role cannot create a subscription plan.
#[test]
fn test_create_subscription_plan_by_non_merchant_is_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client, _usdc_token) = setup_refund_manager(&env);
    let non_merchant = Address::generate(&env);

    let result = client.try_create_subscription_plan(
        &non_merchant,
        &String::from_str(&env, "plan_bad"),
        &String::from_str(&env, "Bad Plan"),
        &String::from_str(&env, "desc"),
        &500_i128,
        &Symbol::new(&env, "USDC"),
        &BillingInterval::Monthly,
    );

    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

/// Subscribing to an active plan creates a subscription and emits SUBSCRIPTION/CREATED.
#[test]
fn test_subscribe_to_active_plan_creates_subscription_and_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _merchant, plan_id, _usdc_token) = setup_with_plan(&env);

    let payer = Address::generate(&env);
    let sub_id = client.subscribe(&payer, &plan_id, &None, &None, &None);

    // Subscription exists and is Active
    let sub = client.get_subscription(&sub_id);
    assert_eq!(sub.subscription_id, sub_id);
    assert_eq!(sub.payer_address, payer);
    assert_eq!(sub.status, SubscriptionStatus::Active);

    // SUBSCRIPTION/CREATED event was emitted
    let events = env.events().all();
    let found = events.iter().any(|e| {
        let topics: soroban_sdk::Vec<soroban_sdk::Val> = e.1;
        if topics.len() < 2 {
            return false;
        }
        let Some(v0) = topics.get(0) else {
            return false;
        };
        let Some(v1) = topics.get(1) else {
            return false;
        };
        let t0: Result<Symbol, _> = v0.try_into_val(&env);
        let t1: Result<Symbol, _> = v1.try_into_val(&env);
        matches!(
            (t0, t1),
            (Ok(a), Ok(b)) if a == Symbol::new(&env, "SUBSCRIPTION") && b == Symbol::new(&env, "CREATED")
        )
    });
    assert!(found, "SUBSCRIPTION/CREATED event not emitted");
}

/// Subscribing to an inactive plan must fail.
#[test]
fn test_subscribe_to_inactive_plan_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, merchant, plan_id, _usdc_token) = setup_with_plan(&env);

    // Deactivate the plan first
    client.deactivate_subscription_plan(&merchant, &plan_id);

    let payer = Address::generate(&env);
    let result = client.try_subscribe(&payer, &plan_id, &None, &None, &None);
    assert!(
        result.is_err(),
        "Expected error when subscribing to inactive plan"
    );
}

/// Payer can pause an active subscription and it becomes Paused.
#[test]
fn test_pause_subscription_by_payer_becomes_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _merchant, plan_id, _usdc_token) = setup_with_plan(&env);

    let payer = Address::generate(&env);
    let sub_id = client.subscribe(&payer, &plan_id, &None, &None, &None);

    client.pause_subscription(&payer, &sub_id);

    let sub = client.get_subscription(&sub_id);
    assert_eq!(sub.status, SubscriptionStatus::Paused);
}

/// Payer can resume a paused subscription; status becomes Active and next_payment_at is updated.
#[test]
fn test_resume_subscription_by_payer_becomes_active_with_updated_payment_at() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _merchant, plan_id, _usdc_token) = setup_with_plan(&env);

    let payer = Address::generate(&env);
    let sub_id = client.subscribe(&payer, &plan_id, &None, &None, &None);

    // Pause first
    client.pause_subscription(&payer, &sub_id);

    // Advance time
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 1_000);

    let before_resume = env.ledger().timestamp();
    client.resume_subscription(&payer, &sub_id);

    let sub = client.get_subscription(&sub_id);
    assert_eq!(sub.status, SubscriptionStatus::Active);
    // next_payment_at must be after the resume timestamp
    assert!(
        sub.next_payment_at > before_resume,
        "next_payment_at ({}) should be after resume timestamp ({})",
        sub.next_payment_at,
        before_resume
    );
}

/// Payer can cancel an active subscription; status becomes Cancelled.
#[test]
fn test_cancel_subscription_by_payer_becomes_cancelled() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _merchant, plan_id, _usdc_token) = setup_with_plan(&env);

    let payer = Address::generate(&env);
    let sub_id = client.subscribe(&payer, &plan_id, &None, &None, &None);

    client.cancel_subscription(&payer, &sub_id, &false);

    let sub = client.get_subscription(&sub_id);
    assert_eq!(sub.status, SubscriptionStatus::Cancelled);
}

/// get_payer_subscriptions returns all subscriptions for the payer.
#[test]
fn test_get_payer_subscriptions_returns_all_for_payer() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _merchant, plan_id, _usdc_token) = setup_with_plan(&env);

    let payer = Address::generate(&env);

    // Subscribe twice
    let sub_id_1 = client.subscribe(&payer, &plan_id, &None, &None, &None);
    let sub_id_2 = client.subscribe(&payer, &plan_id, &None, &None, &None);

    let subs = client.get_payer_subscriptions(&payer);
    assert_eq!(subs.len(), 2);

    let ids: soroban_sdk::Vec<String> = {
        let mut v = vec![&env];
        for s in subs.iter() {
            v.push_back(s.subscription_id.clone());
        }
        v
    };
    assert!(
        ids.contains(&sub_id_1),
        "sub_id_1 not found in payer subscriptions"
    );
    assert!(
        ids.contains(&sub_id_2),
        "sub_id_2 not found in payer subscriptions"
    );
}

/// Merchant can deactivate a plan; subsequent subscribe attempts fail.
#[test]
fn test_deactivate_subscription_plan_by_merchant_marks_inactive() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, merchant, plan_id, _usdc_token) = setup_with_plan(&env);

    client.deactivate_subscription_plan(&merchant, &plan_id);

    let plan = client.get_subscription_plan(&plan_id);
    assert!(!plan.active, "Plan should be inactive after deactivation");
}

/// Mid-period cancel with proration creates a pending refund and emits events.
#[test]
fn test_cancel_subscription_mid_period_with_proration() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _merchant, plan_id, _) = setup_with_plan(&env);

    client.set_allow_prorated_refunds(&admin, &true);

    let payer = Address::generate(&env);
    let sub_id = client.subscribe(&payer, &plan_id, &None, &None, &None);

    // Simulate a successful billing tick by advancing and processing due subs.
    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &operator);
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 7 * 86_400);
    assert_eq!(client.process_due_subscriptions(&operator), 1);

    let sub_before = client.get_subscription(&sub_id);
    assert!(sub_before.last_payment_at.is_some());

    // Advance 3 days into the 7-day period → 4 days remaining → 4/7 of amount.
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 3 * 86_400);

    client.cancel_subscription(&payer, &sub_id, &true);

    let sub = client.get_subscription(&sub_id);
    assert_eq!(sub.status, SubscriptionStatus::Cancelled);

    // Synthetic payment + pending prorated refund must exist.
    let payment_id = String::from_str(&env, "sub_pr_2");
    let refunds = client.get_payment_refunds(&payment_id);
    assert_eq!(refunds.len(), 1);
    let refund = refunds.get(0).unwrap();
    assert_eq!(refund.status, crate::RefundStatus::Pending);
    // 4/7 of 1_000_000 = 571_428
    assert_eq!(refund.amount, 571_428_i128);
}

/// Mid-period cancel without proration flag cancels but creates no refund.
#[test]
fn test_cancel_subscription_mid_period_without_proration() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _merchant, plan_id, _) = setup_with_plan(&env);

    client.set_allow_prorated_refunds(&admin, &true);

    let payer = Address::generate(&env);
    let sub_id = client.subscribe(&payer, &plan_id, &None, &None, &None);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &operator);
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 7 * 86_400);
    assert_eq!(client.process_due_subscriptions(&operator), 1);

    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 3 * 86_400);

    client.cancel_subscription(&payer, &sub_id, &false);

    let sub = client.get_subscription(&sub_id);
    assert_eq!(sub.status, SubscriptionStatus::Cancelled);

    let payment_id = String::from_str(&env, "sub_pr_2");
    let refunds = client.get_payment_refunds(&payment_id);
    assert_eq!(refunds.len(), 0);
}

/// End-of-period cancel (no days remaining) creates no prorated refund.
#[test]
fn test_cancel_subscription_end_of_period_no_refund() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _merchant, plan_id, _) = setup_with_plan(&env);

    client.set_allow_prorated_refunds(&admin, &true);

    let payer = Address::generate(&env);
    let sub_id = client.subscribe(&payer, &plan_id, &None, &None, &None);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &operator);
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 7 * 86_400);
    assert_eq!(client.process_due_subscriptions(&operator), 1);

    // Jump to exactly next_payment_at (end of period).
    let sub_before = client.get_subscription(&sub_id);
    env.ledger().set_timestamp(sub_before.next_payment_at);

    client.cancel_subscription(&payer, &sub_id, &true);

    let sub = client.get_subscription(&sub_id);
    assert_eq!(sub.status, SubscriptionStatus::Cancelled);

    let payment_id = String::from_str(&env, "sub_pr_2");
    let refunds = client.get_payment_refunds(&payment_id);
    assert_eq!(refunds.len(), 0);
}
