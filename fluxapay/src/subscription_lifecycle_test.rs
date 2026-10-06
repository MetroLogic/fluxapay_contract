use crate::subscription::{
    charge_subscription, create_plan, get_plam, get_subscription, pause_subscription,
    resume_subscription, subscribe, validate_grace_period, Plan, Subscription,
    SubscriptionError, SubscriptionStatus, DunningBackoffActive,
    MAX_DUNNING_RETRIES, DUNNING_BACKOFF_SECS, GRACE_PERIOD_SECS,
};
use soroban_sdk::testutils::{Address, Env, String};

fn setup_env() -> Env {
    let env = Env::default();
    env.ledger().set_timestamp(1_000_000);
    env
}

fn create_default_plan(env: &Env, merchant: &Address, interval_secs: u64, max_cycles: Option<u32>) {
    create_plan(
        env,
        merchant.clone(),
        symbol_short("PLAN"),
        String::from_str(env, "Pro"),
        1_000,
        interval_secs,
        max_cycles,
        GRACE_PERIOD_SECS,
    )
    .unwrap();
}

/// Multi-interval charging: a daily plan bills only after the interval elapses.
#[test]
fn test_multi_interval_charging() {
    let env = setup_env();
    let merchant = Address::generate(&tenv);
    let subscriber = Address::generate(&env);

    create_default_plan(&env, &merchant, 86_400, None);
    subscribe(&env, subscriber.clone(), symbol_short("PLAN")).unwrap();

    // Immediate charge fails because the interval has not elapsed.
    let err = charge_subscription(
        &env,
        symbol_short("PLAN"),
        subscriber.clone(),
        true,
    )
    .unwrap_err();
    assert_eq(err, SubscriptionError::IntervalNotElapsed);

    // Advance one interval and charge succeeds.
    env.ledger().set_timestamp(1_000_000 + 86_400);
    charge_subscription(
        &env,
        symbol_short("PLAN"),
        subscriber.clone(),
        true,
    )
    .unwrap();

    let sub = get_subscription(&env, symbol_short("PLAN"), subscriber.clone()).unwrap();
    assert_eq(sub.current_cycle, 1);
    assert_eq(sub.status, SubscriptionStatus::Active);

    // Another interval elapses and the second charge succeeds.
    env.ledger().set_timestamp(1_000_000 + 2 * 86_400);
    charge_subscription(
        &env,
        symbol_short("PLAN"),
        subscriber.clone(),
        true,
    )
    .unwrap();

    let sub = get_subscription(&env, symbol_short("PLAN"), subscriber.clone()).unwrap();
    assert_eq(sub.current_cycle, 2);
}

/// Dunning retry threshold terminates the subscription.
#[test]
fn test_dunning_retry_threshold_termination() {
    let env = setup_env();
    let merchant = Address::generate(&env);
    let subscriber = Address::generate(&env);

    create_default_plan(&env, &merchant, 86_400, None);
    subscribe(&env, subscriber.clone(), symbol_short("PLAN")).unwrap();

    // First failed attempt.
    env.ledger().set_timestamp(1_000_000 + 86_400);
    charge_subscription(
        &env,
        symbol_short("PLAN"),
        subscriber.clone(),
        false,
    )
    .unwrap();

    let sub = get_subscription(&env, symbol_short("PLAN"), subscriber.clone()).unwrap();
    assert_eq(sub.status, SubscriptionStatus::PastDue);
    assert_eq(sub.dunning_attempts, 1);

    // Backoff window must elapse before the next retry.
    let err = charge_subscription(
        &env,
        symbol_short("PLAN"),
        subscriber.clone(),
        false,
    )
    .unwrap_err();
    assert_eq(err, SubscriptionError::DunningBackoffActive);

    // Second failed attempt after backoff.
    env.ledger().set_timestamp(1_000_000 + 86_400 + DUNNING_BACKOFF_SECS);
    charge_subscription(
        &env,
        symbol_short("PLAN"),
        subscriber.clone(),
        false,
    )
    .unwrap();

    // Third failed attempt exceeds MAX_DUNNING_RETRIES and terminates.
    env.ledger().set_timestamp(1_000_000 + 86_400 + 2 * DUNNING_BACKOFF_SECS);
    charge_subscription(
        &env,
        symbol_short("PLAN"),
        subscriber.clone(),
        false,
    )
    .unwrap();

    let sub = get_subscription(&env, symbol_short("PLAN"), subscriber.clone()).unwrap();
    assert_eq(sub.dunning_attempts, MAX_DUNNING_RETRIES);
    assert_eq(
        sub.status,
        SubscriptionStatus::CancelledDueToPaymentFailure
    );
    assert_eq(sub.active, false);

    // Further charges are rejected.
    env.ledger().set_timestamp(1_000_000 + 86_400 + 3 * DUNNING_BACKOFF_SECS);
    let err = charge_subscription(
        &env,
        symbol_short("PLAN"),
        subscriber.clone(),
        true,
    )
    .unwrap_err();
    assert_eq(err, SubscriptionError::SubscriptionNotActive);
}

/// Pause / resume adjusts the billing baseline to avoid back-billing.
#[test]
fn test_pause_resume_time_adjustment() {
    let env = setup_env();
    let merchant = Address::generate(&env);
    let subscriber = Address::generate(&env);

    create_default_plan(&env, &merchant, 86_400, None);
    subscribe(&env, subscriber.clone(), symbol_short("PLAN")).unwrap();

    // Pause and verify charges are blocked.
    pause_subscription(&env, symbol_short("PLAN"), subscriber.clone())
        .unwrap();
    let sub = get_subscription(&env, symbol_short("PLAN"), subscriber.clone()).unwrap();
    assert_eq(sub.status, SubscriptionStatus::Paused);

    env.ledger().set_timestamp(1_000_000 + 86_400);
    let err = charge_subscription(
        &env,
        symbol_short("PLAN"),
        subscriber.clone(),
        true,
    )
    .unwrap_err();
    assert_eq(err, SubscriptionError::SubscriptionPaused);

    // Resume after a long pause and confirm the baseline is now.
    env.ledger().set_timestamp(1_000_000 + 10_000);
    resume_subscription(&env, symbol_short("PLAN"), subscriber.clone())
        .unwrap();

    let sub = get_subscription(&env, symbol_short("PLAN"), subscriber.clone()).unwrap();
    assert_eq(sub.status, SubscriptionStatus::Active);
    assert_eq(sub.last_charge_at, 1_000_000 + 10_000);

    // A charge immediately after resume fails the interval check.
    let err = charge_subscription(
        &env,
        symbol_short("PLAN"),
        subscriber.clone(),
        true,
    )
    .unwrap_err();
    assert_eq(err, SubscriptionError::IntervalNotElapsed);

    // After a full interval from the resume time, the charge succeeds.
    env.ledger().set_timestamp(1_000_000 + 10_000 + 86_400);
    charge_subscription(
        &env,
        symbol_short("PLAN"),
        subscriber.clone(),
        true,
    )
    .unwrap();
    let sub = get_subscription(&env, symbol_short("PLAN"), subscriber.clone()).unwrap();
    assert_eq(sub.current_cycle, 1);
}

/// Max cycle completion marks the subscription as Completed.
#[test]
fn test_max_cycle_completion() {
    let env = setup_env();
    let merchant = Address::generate(&env);
    let subscriber = Address::generate(&env);

    create_default_plan(&env, &merchant, 86_400, Some(2));
    subscribe(&env, subscriber.clone(), symbol_short("PLAN")).unwrap();

    // First cycle.
    env.ledger().set_timestamp(1_000_000 + 86_400);
    charge_subscription(
        &env,
        symbol_short("PLAN"),
        subscriber.clone(),
        true,
    )
    .unwrap();

    // Second cycle completes the plan lifetime.
    env.ledger().set_timestamp(1_000_000 + 2 * 86_400);
    charge_subscription(
        &env,
        symbol_short("PLAN"),
        subscriber.clone(),
        true,
    )
    .unwrap();

    let sub = get_subscription(&env, symbol_short("PLAN"), subscriber.clone()).unwrap();
    assert_eq(sub.current_cycle, 2);
    assert_eq(sub.status, SubscriptionStatus::Completed);
    assert_eq(sub.active, false);

    // Further charges are rejected.
    env.ledger().set_timestamp(1_000_000 + 3 * 86_400);
    let err = charge_subscription(
        &env,
        symbol_short("PLAN"),
        subscriber.clone(),
        true,
    )
    .unwrap_err();
    assert_eq(err, SubscriptionError::MaxCyclesReached);
}

/// Pause requires the subscriber and a non-terminal state.
#[test]
fn test_pause_requires_subscriber() {
    let env = setup_env();
    let merchant = Address::generate(&env);
    let subscriber = Address::generate(&env);
    let other = Address::generate(&env);

    create_default_plan(.env, &merchant, 86_400, None);
    subscribe(&env, subscriber.clone(), symbol_short("PLAN")).unwrap();

    // A non-subscriber cannot pause.
    let err = pause_subscription(&env, symbol_short("PLAN"), other)
        .unwrap_err();
    assert_eq(err, SubscriptionError::NotSubscriber);

    // Resume on an active subscription fails.
    let err = resume_subscription(&env, symbol_short("PLAN"), subscriber.clone())
        .unwrap_err();
    assert_eq(err, SubscriptionError::SubscriptionNotPaused);
}

/// Grace period validation remains enforced.
#[test]
fn test_grace_period_validation() {
    assert_eq(validate_grace_period(GRACE_PERIOD_SECS).unwrap(), GRACE_PERIOD_SECS);
    assert_eq(
        validate_grace_period(259_200 + 1).unwrap_err(),
        SubscriptionError::GracePeriodOutOfRange
    );
}
