/// Merchant Pre-Authorization Module
///
/// Allows merchants to pull variable amounts from customers up to a
/// pre-authorized limit per billing period. Customers grant an allowance
/// once; the merchant can then charge any amount ≤ `limit_per_period`
/// without requiring a fresh customer signature each time.
///
/// Storage layout
/// ──────────────
/// `MerchantAuthDataKey::Authorization(customer, merchant)` → `MerchantAuthorization`
///
/// Events
/// ──────
/// `MERCHANT_AUTH / GRANTED`  – customer grants a new authorization
/// `MERCHANT_AUTH / REVOKED`  – customer revokes an existing authorization
/// `MERCHANT_AUTH / CHARGED`  – merchant pulls funds against the authorization
use soroban_sdk::{contracterror, contracttype, token, Address, Env, Symbol};

// ─── Data types ───────────────────────────────────────────────────────────────

/// A customer's pre-authorization for a specific merchant.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MerchantAuthorization {
    /// The customer who granted the authorization.
    pub customer: Address,
    /// The merchant who may pull funds.
    pub merchant: Address,
    /// Token contract the merchant is allowed to pull.
    pub token: Address,
    /// Maximum amount the merchant may pull within a single period.
    pub limit_per_period: i128,
    /// Duration of each billing period in seconds.
    pub period_secs: u64,
    /// Ledger timestamp when the current period started.
    pub period_start: u64,
    /// Total amount already pulled in the current period.
    pub pulled_this_period: i128,
    /// Whether the authorization is currently active.
    pub active: bool,
    /// Ledger timestamp when the authorization was created.
    pub created_at: u64,
}

/// Storage keys for merchant authorizations.
#[contracttype]
pub enum MerchantAuthDataKey {
    /// Keyed by (customer, merchant) pair.
    Authorization(Address, Address),
}

// ─── Errors ───────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MerchantAuthError {
    /// No authorization exists for this (customer, merchant) pair.
    AuthorizationNotFound = 1,
    /// The authorization has been revoked or is inactive.
    AuthorizationInactive = 2,
    /// The requested pull amount exceeds the remaining period limit.
    LimitExceeded = 3,
    /// Amount must be positive.
    InvalidAmount = 4,
    /// Caller is not the authorized merchant.
    Unauthorized = 5,
    /// An authorization already exists; revoke it first.
    AuthorizationAlreadyExists = 6,
}

// ─── Implementation ───────────────────────────────────────────────────────────

pub struct MerchantPreAuth;

#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
impl MerchantPreAuth {
    // ─── Grant ────────────────────────────────────────────────────────────────

    /// Customer grants a merchant permission to pull up to `limit_per_period`
    /// tokens per `period_secs`-second window.
    ///
    /// # Parameters
    /// * `customer`         – Account granting the authorization; must sign.
    /// * `merchant`         – Merchant address that may pull funds.
    /// * `token`            – Token contract address.
    /// * `limit_per_period` – Maximum pull amount per period (must be > 0).
    /// * `period_secs`      – Length of each billing period in seconds (must be > 0).
    pub fn pre_authorize_merchant(
        env: Env,
        customer: Address,
        merchant: Address,
        token: Address,
        limit_per_period: i128,
        period_secs: u64,
    ) -> Result<MerchantAuthorization, MerchantAuthError> {
        customer.require_auth();

        if limit_per_period <= 0 {
            return Err(MerchantAuthError::InvalidAmount);
        }
        if period_secs == 0 {
            return Err(MerchantAuthError::InvalidAmount);
        }

        let key = MerchantAuthDataKey::Authorization(customer.clone(), merchant.clone());

        // Reject if an active authorization already exists — customer must revoke first.
        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<MerchantAuthDataKey, MerchantAuthorization>(&key)
        {
            if existing.active {
                return Err(MerchantAuthError::AuthorizationAlreadyExists);
            }
        }

        let now = env.ledger().timestamp();
        let auth = MerchantAuthorization {
            customer: customer.clone(),
            merchant: merchant.clone(),
            token,
            limit_per_period,
            period_secs,
            period_start: now,
            pulled_this_period: 0,
            active: true,
            created_at: now,
        };

        env.storage().persistent().set(&key, &auth);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT_AUTH"),
                Symbol::new(&env, "GRANTED"),
            ),
            (customer, merchant, limit_per_period, period_secs),
        );

        Ok(auth)
    }

    // ─── Revoke ───────────────────────────────────────────────────────────────

    /// Customer revokes a previously granted authorization.
    ///
    /// # Parameters
    /// * `customer` – Must be the original grantor; must sign.
    /// * `merchant` – Merchant whose authorization is being revoked.
    pub fn revoke_authorization(
        env: Env,
        customer: Address,
        merchant: Address,
    ) -> Result<(), MerchantAuthError> {
        customer.require_auth();

        let key = MerchantAuthDataKey::Authorization(customer.clone(), merchant.clone());

        let mut auth: MerchantAuthorization = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(MerchantAuthError::AuthorizationNotFound)?;

        if !auth.active {
            return Err(MerchantAuthError::AuthorizationInactive);
        }

        auth.active = false;
        env.storage().persistent().set(&key, &auth);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT_AUTH"),
                Symbol::new(&env, "REVOKED"),
            ),
            (customer, merchant),
        );

        Ok(())
    }

    // ─── Pull / Charge ────────────────────────────────────────────────────────

    /// Merchant pulls `amount` tokens from the customer's account.
    ///
    /// The contract enforces:
    /// - The authorization is active.
    /// - The period window is respected (resets `pulled_this_period` when a new
    ///   period starts).
    /// - `pulled_this_period + amount ≤ limit_per_period`.
    ///
    /// Tokens are transferred from `customer` to `merchant` via the token contract.
    /// The customer must have previously approved this contract as a spender.
    ///
    /// # Parameters
    /// * `merchant`  – Must be the authorized merchant; must sign.
    /// * `customer`  – Account to pull funds from.
    /// * `amount`    – Amount to pull (must be > 0 and within remaining limit).
    pub fn pull_payment(
        env: Env,
        merchant: Address,
        customer: Address,
        amount: i128,
    ) -> Result<i128, MerchantAuthError> {
        merchant.require_auth();

        if amount <= 0 {
            return Err(MerchantAuthError::InvalidAmount);
        }

        let key = MerchantAuthDataKey::Authorization(customer.clone(), merchant.clone());

        let mut auth: MerchantAuthorization = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(MerchantAuthError::AuthorizationNotFound)?;

        if !auth.active {
            return Err(MerchantAuthError::AuthorizationInactive);
        }

        // Verify the caller is the authorized merchant.
        if auth.merchant != merchant {
            return Err(MerchantAuthError::Unauthorized);
        }

        // ── Period reset ──────────────────────────────────────────────────────
        let now = env.ledger().timestamp();
        let elapsed = now.saturating_sub(auth.period_start);
        if elapsed >= auth.period_secs {
            // One or more full periods have passed — reset the counter.
            let periods_elapsed = elapsed / auth.period_secs;
            auth.period_start = auth
                .period_start
                .saturating_add(periods_elapsed * auth.period_secs);
            auth.pulled_this_period = 0;

            env.events().publish(
                (
                    Symbol::new(&env, "MERCHANT_AUTH"),
                    Symbol::new(&env, "PERIOD_RESET"),
                ),
                (customer.clone(), merchant.clone(), auth.period_start),
            );
        }

        // ── Limit check ───────────────────────────────────────────────────────
        let remaining = auth.limit_per_period.saturating_sub(auth.pulled_this_period);
        if amount > remaining {
            return Err(MerchantAuthError::LimitExceeded);
        }

        // ── Effects ───────────────────────────────────────────────────────────
        auth.pulled_this_period = auth.pulled_this_period.saturating_add(amount);

        // Persist state before interaction (CEI pattern).
        env.storage().persistent().set(&key, &auth);

        // ── Interaction ───────────────────────────────────────────────────────
        let token_client = token::Client::new(&env, &auth.token);
        token_client.transfer_from(
            &env.current_contract_address(),
            &customer,
            &merchant,
            &amount,
        );

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT_AUTH"),
                Symbol::new(&env, "CHARGED"),
            ),
            (merchant, customer, amount, auth.pulled_this_period),
        );

        Ok(auth.pulled_this_period)
    }

    // ─── Read helpers ─────────────────────────────────────────────────────────

    /// Return the stored authorization for a (customer, merchant) pair.
    pub fn get_authorization(
        env: Env,
        customer: Address,
        merchant: Address,
    ) -> Result<MerchantAuthorization, MerchantAuthError> {
        env.storage()
            .persistent()
            .get(&MerchantAuthDataKey::Authorization(customer, merchant))
            .ok_or(MerchantAuthError::AuthorizationNotFound)
    }

    /// Return the remaining pull budget for the current period.
    ///
    /// Accounts for period rollovers without mutating state.
    pub fn remaining_limit(
        env: Env,
        customer: Address,
        merchant: Address,
    ) -> Result<i128, MerchantAuthError> {
        let key = MerchantAuthDataKey::Authorization(customer, merchant);
        let auth: MerchantAuthorization = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(MerchantAuthError::AuthorizationNotFound)?;

        if !auth.active {
            return Ok(0);
        }

        let now = env.ledger().timestamp();
        let elapsed = now.saturating_sub(auth.period_start);
        let pulled = if elapsed >= auth.period_secs {
            // Period has rolled over — full limit is available.
            0
        } else {
            auth.pulled_this_period
        };

        Ok(auth.limit_per_period.saturating_sub(pulled).max(0))
    }
}

#[cfg(test)]
mod period_reset_tests {
    use crate::{PaymentProcessor, PaymentProcessorClient};
    use soroban_sdk::{
        testutils::{Address as _, Events as _, Ledger as _},
        token, Address, Env,
    };

    fn setup(env: &Env) -> (Address, PaymentProcessorClient<'_>) {
        let contract_id = env.register(PaymentProcessor, ());
        let client = PaymentProcessorClient::new(env, &contract_id);
        let admin = Address::generate(env);
        client.initialize_payment_processor(&admin);
        (admin, client)
    }

    fn setup_authorization(
        env: &Env,
        client: &PaymentProcessorClient<'_>,
        limit_per_period: i128,
        period_secs: u64,
    ) -> (Address, Address, Address) {
        let customer = Address::generate(env);
        let merchant = Address::generate(env);
        let token_admin = Address::generate(env);
        let token = env
            .register_stellar_asset_contract_v2(token_admin)
            .address();

        token::StellarAssetClient::new(env, &token).mint(&customer, &1_000_000_000i128);

        client.pre_authorize_merchant(
            &customer,
            &merchant,
            &token,
            &limit_per_period,
            &period_secs,
        );

        (customer, merchant, token)
    }

    #[test]
    fn test_period_reset_on_first_pull_of_new_period() {
        let env = Env::default();
        env.mock_all_auths();
        let (_admin, client) = setup(&env);
        let (customer, merchant, _token) = setup_authorization(&env, &client, 1_000i128, 100u64);

        client.pull_payment(&merchant, &customer, &1_000i128);
        // Fully spent for this period.
        let result = client.try_pull_payment(&merchant, &customer, &1i128);
        assert!(result.is_err());

        // Advance past the period boundary — the first pull of the new
        // period must reset `pulled_this_period` before applying the limit.
        env.ledger().with_mut(|li| li.timestamp += 101);
        let pulled = client.pull_payment(&merchant, &customer, &500i128);
        assert_eq!(pulled, 500i128);

        let auth = client.get_merchant_authorization(&customer, &merchant);
        assert_eq!(auth.pulled_this_period, 500i128);
    }

    #[test]
    fn test_limit_enforced_within_period() {
        let env = Env::default();
        env.mock_all_auths();
        let (_admin, client) = setup(&env);
        let (customer, merchant, _token) = setup_authorization(&env, &client, 1_000i128, 100u64);

        client.pull_payment(&merchant, &customer, &700i128);
        let result = client.try_pull_payment(&merchant, &customer, &400i128);
        assert_eq!(
            result,
            Err(Ok(crate::merchant_auth::MerchantAuthError::LimitExceeded))
        );

        // Remaining budget in the same period is still pullable.
        let pulled = client.pull_payment(&merchant, &customer, &300i128);
        assert_eq!(pulled, 1_000i128);
    }

    #[test]
    fn test_multi_period_pulls_all_succeed() {
        let env = Env::default();
        env.mock_all_auths();
        let (_admin, client) = setup(&env);
        let (customer, merchant, _token) = setup_authorization(&env, &client, 1_000i128, 100u64);

        for _ in 0..3 {
            let pulled = client.pull_payment(&merchant, &customer, &1_000i128);
            assert_eq!(pulled, 1_000i128);
            env.ledger().with_mut(|li| li.timestamp += 101);
        }
    }

    #[test]
    fn test_period_reset_event_emitted_on_rollover() {
        let env = Env::default();
        env.mock_all_auths();
        let (_admin, client) = setup(&env);
        let (customer, merchant, _token) = setup_authorization(&env, &client, 1_000i128, 100u64);

        // Pull within the same period — no reset, no PERIOD_RESET event.
        client.pull_payment(&merchant, &customer, &500i128);
        let events_before = env.events().all().events().len();

        // Roll over into a new period — this pull must emit PERIOD_RESET
        // in addition to the usual CHARGED event.
        env.ledger().with_mut(|li| li.timestamp += 101);
        client.pull_payment(&merchant, &customer, &200i128);
        let events_after = env.events().all().events().len();

        assert!(
            events_after > events_before + 1,
            "expected an extra MERCHANT_AUTH/PERIOD_RESET event on period rollover"
        );
    }
}
