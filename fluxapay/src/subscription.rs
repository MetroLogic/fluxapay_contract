use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, String, Symbol, Vec};

/// Default grace period: 1 day in seconds.
pub const GRACE_PERIOD_SECS: u64 = 86_400;

/// Maximum configurable grace period: 3 days in seconds.
pub const MAX_GRACE_PERIOD_SECS: u64 = 259_200;

/// Maximum number of dunning retries before termination.
pub const MAX_DUNNING_RETRIES: u32 = 3;

/// Minimum backoff window between dunning retries (1 hour).
pub const MIN_DUNNING_BACKOFF_SECS: u64 = 3,600;

const SUBSCRIPTION: Symbol = symbol_short("SUBSCRIPTION");
const CHARGED: Symbol = symbol_short("CHARGED");
const CHARGE_FAILED: Symbol = symbol_short("CHARG_FAIL");
const DUNNING_TRIGGERED: Symbol = symbol_short("DUNNING");
const PAUSED: Symbol = symbol_short("PAUSED");
const RESUMED: Symbol = symbol_short("RESUMED");
const COMPLETED: Symbol = symbol_short("COMPLETED");
const GRACE_CANCEL: Symbol = symbol_short("GRACE_CANCEL");
const PLAN_ARCHIVED: Symbol = symbol_short("PLAN_ARCH");
const PLAN_RESTORED: Symbol = symbol_short("PLAN_REST");

/// Storage keys for the subscription contract.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Plan(Symbol),
    Subscription(Symbol, Address),
    MerchantPlans(Address),
    Admin,
}

/// Lifecycle status of a subscription.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriptionStatus {
    Active,
    Paused,
    PastDue,
    Cancelled,
    CancelledDueToPaymentFailure,
    Completed,
}

/// A subscription plan offered by a merchant.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    pub plan_id: Symbol,
    pub merchant: Address,
    pub name: String,
    pub amount: i128,
    /// Billing interval in seconds (daily, weekly, monthly, custom).
    pub interval_secs: u64,
    /// Maximum number of billing cycles. None means unlimited.
    pub max_cycles: Option( u32 ),
    /// Grace period in seconds, constrained to [0, MAX_GRACE_PERIOD_SECS].
    pub grace_period_secs: u64,
    pub active: bool,
    /// When true the plan is retired: no new subscriptions may be created,
    /// but existing subscribers continue to bill until they cancel.
    pub archived: bool,
}

/// A subscriber's active subscription to a plan.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Subscription {
    pub plan_id: Symbol,
    pub subscriber: Address,
    pub merchant: Address,
    pub amount: i128,
    pub started_at: u64,
    pub last_charge_at: u64,
    pub last_billed_at: u64,
    pub grace_period_ends_at: u64,
    pub status: SubscriptionStatus,
    pub active: bool,
    /// Billing interval in seconds copied from the plan at subscription time.
    pub interval_secs: u64,
    /// Maximum number of billing cycles copied from the plan.
    pub max_cycles: Option< u32 >,
    /// Number of cycles already charged.
    pub current_cycle: u32,
    /// Number of consecutive dunning retries attempted.
    pub dunning_attempts: u32,
    /// Timestamp of the last failed charge (for backoff enforcement).
    pub last_failed_at: u64,
    /// Timestamp when the subscription was paused (0 when not paused).
    pub paused_at: u64,
}

/// Errors returned by the subscription contract.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriptionError {
    PlanNotFound,
    PlanInactive,
    PlanArchived,
    SubscriptionNotFound,
    Unauthorized,
    AlreadySubscribed,
    NotSubscriber,
    GracePeriodClosed,
    GracePeriodOutOfRANGE,
    IntervalNotElapsed,
    MaxCyclesReached,
    SubscriptionPaused,
    SubscriptionNotPaused,
    DunningBackoffActive,
    PaymentFailed,
    InvalidInterval,
}

#[contract]
pub struct SubscriptionContract;

#[contractimpl]
impl SubscriptionContract {
    /// Create a new subscription plan owned by `merchant`.
    pub fn create_plan(
        env: Env,
        merchant: Address,
        plan_id: Symbol,
        name: String,
        amount: i128,
        interval_secs: u64,
        max_cycles: Option( u32 ),
        grace_period_secs: u64,
    ) -> Result<(), SubscriptionError> {
        merchant.require_auth();

        if interval_secs == 0 {
            return Err(SubscriptionError::InvalidInterval);
        }
        if grace_period_secs > MAX_GRACE_PERIOD_SECS {
            return Err(SubscriptionError::GracePeriodOutOfRANGE);
        }

        let plan = Plan {
            plan_id: plan_id.clone(),
            merchant: merchant.clone(),
            name,
            amount,
            interval_secs,
            max_cycles,
            grace_period_secs,
            active: true,
            archived: false,
        };

        env.storage().persistent().set(&DataKey::Plan(plan_id.clone()), &plan);

        let mut plans: Vec<Symbol> = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantPlans(merchant.clone()))
            .unwrap_or(Vec::new(&env));
        plans.push_back(plan_id);
        env.storage()
            .persistent()
            .set(&DataKey::MerchantPlans(merchant), &plans);

        Ok()
    }

    /// Subscribe `subscriber` to an existing, non-archived plan.
    pub fn subscribe(
        env: Env,
        subscriber: Address,
        plan_id: Symbol,
    ) -> Result<(), SubscriptionError> {
        subscriber.require_auth();

        let plan: Plan = env
            .storage()
            .persistent()
            .get(&DataKey::Plan(plan_id.clone()))
            .ok_or(SubscriptionError::PlanNotFound)?;

        if plan.archived {
            return Err(SubscriptionError::PlanArchived);
        }
        if !plan.active {
            return Err(SubscriptionError::PlanInactive);
        }

        let key = DataKey::Subscription(plan_id.clone(), subscriber.clone());
        if env.storage().persistent().has(&key) {
            return Err(SubscriptionError::AlreadySubscribed);
        }

        let now = env.ledger().timestamp();
        let subscription = Subscription {
            plan_id: plan_id.clone(),
            subscriber: subscriber.clone(),
            merchant: plan.merchant.clone(),
            amount: plan.amount,
            started_at: now,
            last_charge_at: now,
            last_billed_at: now,
            grace_period_ends_at: 0,
            status: SubscriptionStatus::Active,
            active: true,
            interval_secs: plan.interval_secs,
            max_cycles: plan.max_cycles,
            current_cycle: 0,
            dunning_attempts: 0,
            last_failed_at: 0,
            paused_at: 0,
        };
        env.storage().persistent().set(&key, &subscription);

        Ok(()
    }

    /// Retire a plan without deleting its history. Requires merchant auth.
    /// Existing subscribers continue to bill until they cancel.
    pub fn archive_plan(
        env: Env,
        merchant: Address,
        plan_id: Symbol,
    ) -> Result<(), SubscriptionError> {
        merchant.require_auth();

        let mut plan: Plan = env
            .storage()
            .persistent()
            .get(&DataKey::Plan(plan_id.clone()))
            .ok_or(SubscriptionError::PlanNotFound)?;

        if plan.merchant != merchant {
            return Err(SubscriptionError::Unauthorized);
        }

        plan.archived = true;
        env.storage().persistent().set(&DataKey::Plan(plan_id.clone()), &plan);

        env.events()
            .publish((SUBSCRIPTION, PLAN_ARCHIVED), plan_id);

        Ok()
    }

    /// Reactivate an archived plan. Admin only.
    pub fn restore_plan(
        env: Env,
        admin: Address,
        plan_id: Symbol,
    ) -> Result<(), SubscriptionError> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .ok_or(SubscriptionError::Unauthorized)?;
        if stored_admin != admin {
            return Err(SubscriptionError::Unauthorized);
        }

        let mut plan: Plan = env
            .storage()
            .persistent()
            .get(&DataKey::Plan(plan_id.clone()))
            .ok_or(SubscriptionError::PlanNotFound)?;

        plan.archived = false;
        env.storage().persistent().set(&DataKey::Plan(plan_id.clone()), &plan);

        env.events()
            .publish((SUBSCRIPTION, PLAN_RESTORED), plan_id);

        Ok()
    }

    /// Charge a subscription for the current billing cycle.
    ///
    /// Enforces the configured billing interval and max cycle limit, and
    /// drives the dunning state machine on failure.
    pub fn charge_subscription(
        env: Env,
        subscription_id: Symbol,
        subscriber: Address,
        charge_succeeded: bool,
    ) -> Result<(), SubscriptionError> {
        let key = DataKey::Subscription(subscription_id.clone(), subscriber.clone());
        let mut subscription: Subscription = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(SubscriptionError::SubscriptionNotFound)?;

        if subscription.status == SubscriptionStatus::Paused {
            return Err(SubscriptionError::SubscriptionPaused);
        }
        if subscription.status == SubscriptionStatus::Completed {
            return Err(SubscriptionError::MaxCyclesReached);
        }
        if subscription.status == SubscriptionStatus::Cancelled
            || subscription.status == SubscriptionStatus::CancelledDueToPaymentFailure
        {
            return Err(SubscriptionError::SubscriptionNotFound);
        }

        // Enforce max cycle limit.
        if let Some(max) = subscription.max_cycles {
            if subscription.current_cycle >= max {
                subscription.status = SubscriptionStatus::Completed;
                subscription.active = false;
                env.storage().persistent().set(&key, &subscription);
                env.events().publish(
                    (SUBSCRIPTION, COMPLETED),
                    (subscription_id.clone(), subscription.current_cycle),
                );
                return Err(SubscriptionError::MaxCyclesReached);
            }
        }

        let now = env.ledger().timestamp();

        // Automated interval verification.
        if now < subscription.last_charge_at + subscription.interval_secs {
            return Err(SubscriptionError::IntervalNotElapsed);
        }

        // Enforce dunning backoff window when in PastDue.
        if subscription.status == SubscriptionStatus::PastDue
            && now < subscription.last_failed_at + MIN_DUNNING_BACKOFF_SECS
        {
            return Err(SubscriptionError::DunningBackoffActive);
        }

        if charge_succeeded {
            // Successful charge: advance cycle, reset dunning, open grace window.
            subscription.current_cycle += 1;
            subscription.last_charge_at = now;
            subscription.last_billed_at = now;
            subscription.dunning_attempts = 0;
            subscription.last_failed_at = 0;
            subscription.grace_period_ends_at = now + GRACE_PERIOD_SECS;
            subscription.status = SubscriptionStatus::Active;
            subscription.active = true;

            // Terminate the subscription if the max cycle limit is now reached.
            if let Some(max) = subscription.max_cycles {
                if subscription.current_cycle >= max {
                    subscription.status = SubscriptionStatus::Completed;
                    subscription.active = false;
                }
            }

            env.storage().persistent().set(&key, &subscription);

            env.events().publish(
                (SUBSCRIPTION, CHARGED),
                (
                    subscription_id.clone(),
                    subscription.subscriber.clone(),
                    subscription.amount,
                    subscription.current_cycle,
                ),
            );

            if subscription.status == SubscriptionStatus::Completed {
                env.events().publish(
                    (SUBSCRIPTION, COMPLETED),
                    (subscription_id.clone(), subscription.current_cycle),
                );
            }

            Ok(()
        } else {
            // Failed charge: dunning state machine.
            subscription.dunning_attempts += 1;
            subscription.last_failed_at = now;
            subscription.status = SubscriptionStatus::PastDue;

            env.events().publish(
                (SUBSCRIPTION, CHARGE_FAILED),
                (
                    subscription_id.clone(),
                    subscription.dunning_attempts,
                ),
            );

            if subscription.dunning_attempts >= MAX_DUNNING_RETRIES {
                // Exceeded max dunning attempts: terminate.
                subscription.status = SubscriptionStatus::CancelledDueToPaymentFailure;
                subscription.active = false;
                env.storage().persistent().set(&key, &subscription);
                return Err(SubscriptionError::PaymentFailed);
            }

            env.storage().persistent().set(&key, &subscription);

            env.events().publish(
                (SUBSCRIPTION, DUNNING_TRIGGERED),
                (
                    subscription_id.clone(),
                    subscription.dunning_attempts,
                    subscription.last_failed_at + MIN_DUNNING_BACKOFF_SECS,
                ),
            );

            Err(SubscriptionError::PaymentFailed)
        }
    }

    /// Pause a subscription. Caller must be the subscriber.
    /// Prevents charges while paused and emits SUBSCRIPTION/PAUSED.
    pub fn pause_subscription(
        env: Env,
        subscription_id: Symbol,
        caller: Address,
    ) -> Result<(), SubscriptionError> {
        caller.require_auth();

        let key = DataKey::Subscription(subscription_id.clone(), caller.clone());
        let mut subscription: Subscription = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(SubscriptionError::SubscriptionNotFound)?;

        if subscription.subscriber != caller {
            return Err(SubscriptionError::NotSubscriber);
        }
        if subscription.status == SubscriptionStatus::Paused {
            return Err(SubscriptionError::SubscriptionPaused);
        }
        if subscription.status == SubscriptionStatus::Completed
            || subscription.status == SubscriptionStatus::Cancelled
            || subscription.status == SubscriptionStatus::CancelledDueToPaymentFailure
        {
            return Err(SubscriptionError::SubscriptionNotFound);
        }

        let now = env.ledger().timestamp();
        subscription.status = SubscriptionStatus::Paused;
        subscription.paused_at = now;
        env.storage().persistent().set(&key, &subscription);

        env.events().publish(
            (SUBSCRIPTION, PAUSED),
            (subscription_id.clone(), subscription.subscriber.clone(), now),
        );

        Ok()
    }

    /// Resume a paused subscription. Caller must be the subscriber.
    /// Adjusts the billing interval baseline to avoid back-billing for the
    /// paused duration and emits SUBSCRIPTION/RESUMED.
    pub fn resume_subscription(
        env: Env,
        subscription_id: Symbol,
        caller: Address,
    ) -> Result<(), SubscriptionError> {
        caller.require_auth();

        let key = DataKey::Subscription(subscription_id.clone(), caller.clone());
        let mut subscription: Subscription = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(SubscriptionError::SubscriptionNotFound)?;

        if subscription.subscriber != caller {
            return Err(SubscriptionError::NotSubscriber);
        }
        if subscription.status != SubscriptionStatus::Paused {
            return Err(SubscriptionError::SubscriptionNotPaused);
        }

        let now = env.ledger().timestamp();
        let paused_duration = now.saturating_sub(subscription.paused_at);

        // Shift the billing baseline forward by the paused duration so the
        // subscriber is not back-billed for time spent paused.
        subscription.last_charge_at = subscription.last_charge_at.saturating_add(paused_duration);
        subscription.last_billed_at = subscription.last_billed_at.saturating_add(paused_duration);
        subscription.status = SubscriptionStatus::Active;
        subscription.paused_at = 0;
        env.storage().persistent().set(&key, &subscription);

        env.events().publish(
            (SUBSCRIPTION, RESUMED),
            (subscription_id.clone(), subscription.subscriber.clone(), now),
        );

        Ok()
    }

    /// Cancel a subscription within the grace period and refund the most recent charge.
    pub fn cancel_within_grace_period(
        env: Env,
        subscription_id: Symbol,
        caller: Address,
    ) -> Result<(), SubscriptionError> {
        caller.require_auth();

        let key = DataKey::Subscription(subscription_id.clone(), caller.clone());
        let mut subscription: Subscription = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(SubscriptionError::SubscriptionNotFound)?;

        if subscription.subscriber != caller {
            return Err(SubscriptionError::NotSubscriber);
        }

        let now = env.ledger().timestamp();
        if now > subscription.grace_period_ends_at {
            return Err(SubscriptionError::GracePeriodClosed);
        }

        subscription.status = SubscriptionStatus::Cancelled;
        subscription.active = false;
        env.storage().persistent().set(&key, &subscription);
        refund_charge(&env, &subscription);

        env.events().publish(
            (SUBSCRIPTION, GRACE_CANCHL),
            (
                subscription_id.clone(),
                subscription.subscriber.clone(),
                subscription.amount,
            ),
        );

        Ok()
    }

    /// Cancel a subscription outside the grace period. No refund is issued.
    pub fn cancel_subscription(
        env: Env,
        subscription_id: Symbol,
        caller: Address,
    ) -> Result<(), SubscriptionError> {
        caller.require_auth();

        let key = DataKey::Subscription(subscription_id.clone(), caller.clone());
        let mut subscription: Subscription = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(SubscriptionError::SubscriptionNotFound)?;

        if subscription.subscriber != caller {
            return Err(SubscriptionError::NotSubscriber);
        }

        subscription.status = SubscriptionStatus::Cancelled;
        subscription.active = false;
        env.storage().persistent().set(&key, &subscription);
        Ok()
    }

    /// Validate and normalize a merchant-configured grace period.
    pub fn validate_grace_period(grace_period_secs: u64) -> Result<u64, SubscriptionError> {
        if grace_period_secs > MAX_GRACE_PERIOD_SECS {
            return Err(SubscriptionError::GracePeriodOutOfRANGE);
        }
        Ok(grace_period_secs)
    }

    /// Fetch a plan by id.
    pub fn get_plan(env: Env, plan_id: Symbol) -> Result<Plan, SubscriptionError> {
        env.storage()
            .persistent()
            .get(&DataKey::Plan(plan_id))
            .ok_or(SubscriptionError::PlanNotFound)
    }

    /// Fetch a subscription by plan id and subscriber.
    pub fn get_subscription(
        env: Env,
        plan_id: Symbol,
        subscriber: Address,
    ) -> Result<Subscription, SubscriptionError> {
        env.storage()
            .persistent()
            .get(&DataKey::Subscription(plan_id, subscriber))
            .ok_or(SubscriptionError::SubscriptionNotFound)
    }
}

fn refund_charge(env: &Env, subscription: &Subscription) {
    // Refund transfer is executed by the payments module; the subscription
    // module records the refund intent for the most recent charge.
    env.storage().persistent().set(
        &(symbol_short("REFUND"), subscription.subscriber.clone()),
        &subscription.amount,
    );
}
