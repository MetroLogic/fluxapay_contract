use soroban_sdk::{contracttype, symbol_short, Address, Env, Symbol};

/// Default grace period: 1 day in seconds.
pub const GRACE_PERIOD_SECS: u64 = 86_400;
/// Maximum configurable grace period: 3 days in seconds.
pub const MAX_GRACE_PERIOD_SECS: u64 = 259_200;

pub const SUBSCRIPTION: Symbol = symbol_short!("SUBSCRIPTION");
pub const GRACE_CANCEL: Symbol = symbol_short!("GRACE_CANCEL");

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Subscription {
    pub subscriber: Address,
    pub merchant: Address,
    pub amount: i128,
    pub active: bool,
    pub last_charge_at: u64,
    pub grace_period_ends_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    pub merchant: Address,
    pub amount: i128,
    /// Grace period in seconds, constrained to `[0, MAX_GRACE_PERIOD_SECS]`.
    pub grace_period_secs: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubscriptionError {
    NotFound,
    NotSubscriber,
    GracePeriodClosed,
    GracePeriodOutOfRange,
}

/// Charge an active subscription and open a fresh cancellation grace window.
pub fn charge_subscription(env: &Env, subscription_id: u64) -> Result<(), SubscriptionError> {
    let mut subscription = load_subscription(env, subscription_id)?;
    let now = env.ledger().timestamp();

    // Process the charge (payment transfer handled by the billing module).
    subscription.last_charge_at = now;
    subscription.grace_period_ends_at = now + GRACE_PERIOD_SECS;

    save_subscription(env, subscription_id, &subscription);
    Ok(())
}

/// Cancel a subscription within the grace period and refund the most recent charge.
///
/// The caller must be the subscriber and the grace window must still be open.
/// The refund is issued atomically with the cancellation.
pub fn cancel_within_grace_period(
    env: &Env,
    subscription_id: u64,
    caller: Address,
) -> Result<(), SubscriptionError> {
    caller.require_auth();

    let mut subscription = load_subscription(env, subscription_id)?;

    if subscription.subscriber != caller {
        return Err(SubscriptionError::NotSubscriber);
    }

    let now = env.ledger().timestamp();
    if now > subscription.grace_period_ends_at {
        return Err(SubscriptionError::GracePeriodClosed);
    }

    // Cancel and refund the most recent charge atomically.
    subscription.active = false;
    save_subscription(env, subscription_id, &subscription);
    refund_charge(env, &subscription);

    env.events().publish(
        (SUBSCRIPTION, GRACE_CANCEL),
        (subscription_id, subscription.subscriber.clone(), subscription.amount),
    );

    Ok(())
}

/// Cancel a subscription outside the grace period. No refund is issued.
pub fn cancel_subscription(
    env: &Env,
    subscription_id: u64,
    caller: Address,
) -> Result<(), SubscriptionError> {
    caller.require_auth();

    let mut subscription = load_subscription(env, subscription_id)?;
    if subscription.subscriber != caller {
        return Err(SubscriptionError::NotSubscriber);
    }

    subscription.active = false;
    save_subscription(env, subscription_id, &subscription);
    Ok(())
}

/// Validate and normalize a merchant-configured grace period.
pub fn validate_grace_period(grace_period_secs: u64) -> Result<u64, SubscriptionError> {
    if grace_period_secs > MAX_GRACE_PERIOD_SECS {
        return Err(SubscriptionError::GracePeriodOutOfRange);
    }
    Ok(grace_period_secs)
}

fn load_subscription(env: &Env, subscription_id: u64) -> Result<Subscription, SubscriptionError> {
    env.storage()
        .persistent()
        .get(&subscription_id)
        .ok_or(SubscriptionError::NotFound)
}

fn save_subscription(env: &Env, subscription_id: u64, subscription: &Subscription) {
    env.storage().persistent().set(&subscription_id, subscription);
}

fn refund_charge(env: &Env, subscription: &Subscription) {
    // Refund transfer is executed by the payments module; the subscription
    // module records the refund intent for the most recent charge.
    env.storage().persistent().set(
        &(symbol_short!("REFUND"), subscription.subscriber.clone()),
        &subscription.amount,
    );
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, String, Symbol, Vec};

/// Storage keys for the subscription contract.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Plan(Symbol),
    Subscription(Symbol, Address),
    MerchantPlans(Address),
    Admin,
}

/// A subscription plan offered by a merchant.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubscriptionPlan {
    pub plan_id: Symbol,
    pub merchant: Address,
    pub name: String,
    pub price: i128,
    pub interval: u64,
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
    pub started_at: u64,
    pub last_billed_at: u64,
    pub active: bool,
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
}

const PLAN_ARCHIVED: Symbol = symbol_short!("PLAN_ARCHIVED");
const PLAN_RESTORED: Symbol = symbol_short!("PLAN_RESTORED");

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
        price: i128,
        interval: u64,
    ) -> Result<(), SubscriptionError> {
        merchant.require_auth();

        let plan = SubscriptionPlan {
            plan_id: plan_id.clone(),
            merchant: merchant.clone(),
            name,
            price,
            interval,
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

        Ok(())
    }

    /// Subscribe `subscriber` to an existing, non-archived plan.
    pub fn subscribe(
        env: Env,
        subscriber: Address,
        plan_id: Symbol,
    ) -> Result<(), SubscriptionError> {
        subscriber.require_auth();

        let plan: SubscriptionPlan = env
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
            plan_id,
            subscriber,
            started_at: now,
            last_billed_at: now,
            active: true,
        };
        env.storage().persistent().set(&key, &subscription);

        Ok(())
    }

    /// Retire a plan without deleting its history. Requires merchant auth.
    /// Existing subscribers continue to bill until they cancel.
    pub fn archive_plan(
        env: Env,
        merchant: Address,
        plan_id: Symbol,
    ) -> Result<(), SubscriptionError> {
        merchant.require_auth();

        let mut plan: SubscriptionPlan = env
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
            .publish((symbol_short!("SUBSCRIPTION"), PLAN_ARCHIVED), plan_id);

        Ok(())
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

        let mut plan: SubscriptionPlan = env
            .storage()
            .persistent()
            .get(&DataKey::Plan(plan_id.clone()))
            .ok_or(SubscriptionError::PlanNotFound)?;

        plan.archived = false;
        env.storage().persistent().set(&DataKey::Plan(plan_id.clone()), &plan);

        env.events()
            .publish((symbol_short!("SUBSCRIPTION"), PLAN_RESTORED), plan_id);

        Ok(())
    }

    /// Bill an existing subscription. Archived plans still bill active subscribers.
    pub fn bill(
        env: Env,
        plan_id: Symbol,
        subscriber: Address,
    ) -> Result<(), SubscriptionError> {
        let key = DataKey::Subscription(plan_id.clone(), subscriber.clone());
        let mut subscription: Subscription = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(SubscriptionError::SubscriptionNotFound)?;

        if !subscription.active {
            return Err(SubscriptionError::SubscriptionNotFound);
        }

        // Note: archived plans intentionally continue to bill existing subscribers.
        subscription.last_billed_at = env.ledger().timestamp();
        env.storage().persistent().set(&key, &subscription);

        Ok(())
    }

    /// Fetch a plan by id.
    pub fn get_plan(env: Env, plan_id: Symbol) -> Result<SubscriptionPlan, SubscriptionError> {
        env.storage()
            .persistent()
            .get(&DataKey::Plan(plan_id))
            .ok_or(SubscriptionError::PlanNotFound)
    }
}
