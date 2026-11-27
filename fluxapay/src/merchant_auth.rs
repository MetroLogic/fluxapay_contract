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
use soroban_sdk::{contracterror, contracttype, token, Address, BytesN, Env, String, Symbol, Vec};

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

/// Issue #854: Scoped API key record for a merchant.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiKeyRecord {
    pub key_hash: BytesN<32>,
    pub merchant: Address,
    pub scopes: Vec<String>,
    pub created_at: u64,
    pub revoked: bool,
    /// Issue #822: Optional Unix timestamp after which the key is invalid.
    pub expires_at: Option<u64>,
}

/// Storage keys for merchant authorizations.
#[contracttype]
pub enum MerchantAuthDataKey {
    /// Keyed by (customer, merchant) pair.
    Authorization(Address, Address),
    /// Keyed by API key hash.
    ApiKey(BytesN<32>),
    /// Issue #822: Admin-configured maximum API key lifetime in seconds.
    MaxKeyLifetimeSecs,
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
    /// API key was not found.
    ApiKeyNotFound = 7,
    /// API key has already been revoked.
    ApiKeyRevoked = 8,
    /// Issue #822: API key has passed its expiry timestamp.
    ApiKeyExpired = 9,
    /// Issue #822: Requested lifetime exceeds the admin-configured maximum.
    KeyLifetimeExceedsMax = 10,
}

// ─── Implementation ───────────────────────────────────────────────────────────

pub struct MerchantPreAuth;

#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
impl MerchantPreAuth {
    // ─── Grant ────────────────────────────────────────────────────────────────

    /// Customer grants a merchant permission to pull up to `limit_per_period`
    /// tokens per `period_secs`-second window.
    ///
    /// A re-authorization for the same (customer, merchant) pair replaces any
    /// prior grant: the pulled-budget counter resets and the new limit, token,
    /// and period length take effect immediately.
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

        let now = env.ledger().timestamp();
        if now >= auth.period_start + auth.period_secs {
            auth.period_start = now;
            auth.pulled_this_period = 0;
        }

        if auth.pulled_this_period + amount > auth.limit_per_period {
            return Err(MerchantAuthError::LimitExceeded);
        }

        auth.pulled_this_period += amount;
        env.storage().persistent().set(&key, &auth);

        let token_client = token::Client::new(&env, &auth.token);
        token_client.transfer(&customer, &merchant, &amount);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT_AUTH"),
                Symbol::new(&env, "CHARGED"),
            ),
            (customer, merchant, amount),
        );

        Ok(auth.pulled_this_period)
    }

    // ─── API keys (Issue #854) ────────────────────────────────────────────────

    /// Issue #822: Admin sets the maximum allowed API key lifetime in seconds.
    ///
    /// When set, `create_api_key` rejects any key whose requested lifetime
    /// (from `now` to `expires_at`) exceeds this value. A value of `0` disables
    /// the cap.
    ///
    /// # Parameters
    /// * `admin` – Contract admin; must sign.
    /// * `max_lifetime_secs` – Maximum lifetime in seconds (0 = unlimited).
    pub fn set_max_key_lifetime_secs(
        env: Env,
        admin: Address,
        max_lifetime_secs: u64,
    ) -> Result<(), MerchantAuthError> {
        admin.require_auth();

        env.storage()
            .persistent()
            .set(&MerchantAuthDataKey::MaxKeyLifetimeSecs, &max_lifetime_secs);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT_AUTH"),
                Symbol::new(&env, "MAX_KEY_LIFETIME"),
            ),
            (admin, max_lifetime_secs),
        );

        Ok(())
    }

    /// Issue #854: Create a scoped API key for a merchant.
    ///
    /// Issue #822: `expires_at` is an optional Unix timestamp after which the
    /// key is rejected by `verify_api_key`. When the admin has configured a
    /// maximum lifetime via `set_max_key_lifetime_secs`, the requested lifetime
    /// must not exceed it.
    ///
    /// # Parameters
    /// * `merchant`   – Merchant the key belongs to; must sign.
    /// * `key_hash`   – Hash of the API key secret.
    /// * `scopes`     – Scopes granted to the key.
    /// * `expires_at` – Optional Unix timestamp when the key expires.
    pub fn create_api_key(
        env: Env,
        merchant: Address,
        key_hash: BytesN<32>,
        scopes: Vec<String>,
        expires_at: Option<u64>,
    ) -> Result<ApiKeyRecord, MerchantAuthError> {
        merchant.require_auth();

        let now = env.ledger().timestamp();

        if let Some(expiry) = expires_at {
            if expiry <= now {
                return Err(MerchantAuthError::ApiKeyExpired);
            }

            let max_lifetime: u64 = env
                .storage()
                .persistent()
                .get(&MerchantAuthDataKey::MaxKeyLifetimeSecs)
                .unwrap_or(0);

            if max_lifetime != 0 && expiry - now > max_lifetime {
                return Err(MerchantAuthError::KeyLifetimeExceedsMax);
            }
        }

        let record = ApiKeyRecord {
            key_hash: key_hash.clone(),
            merchant: merchant.clone(),
            scopes,
            created_at: now,
            revoked: false,
            expires_at,
        };

        env.storage()
            .persistent()
            .set(&MerchantAuthDataKey::ApiKey(key_hash.clone()), &record);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT_AUTH"),
                Symbol::new(&env, "API_KEY_CREATED"),
            ),
            (merchant, key_hash, expires_at),
        );

        Ok(record)
    }

    /// Issue #854: Verify an API key is valid (exists, not revoked).
    ///
    /// Issue #822: Also rejects keys whose `expires_at` has passed with
    /// `ApiKeyExpired`.
    ///
    /// # Parameters
    /// * `key_hash` – Hash of the API key secret to verify.
    pub fn verify_api_key(
        env: Env,
        key_hash: BytesN<32>,
    ) -> Result<ApiKeyRecord, MerchantAuthError> {
        let record: ApiKeyRecord = env
            .storage()
            .persistent()
            .get(&MerchantAuthDataKey::ApiKey(key_hash))
            .ok_or(MerchantAuthError::ApiKeyNotFound)?;

        if record.revoked {
            return Err(MerchantAuthError::ApiKeyRevoked);
        }

        if let Some(expiry) = record.expires_at {
            if env.ledger().timestamp() >= expiry {
                return Err(MerchantAuthError::ApiKeyExpired);
            }
        }

        Ok(record)
    }

    /// Issue #854: Revoke an API key so it can no longer be verified.
    ///
    /// # Parameters
    /// * `merchant` – Owner of the key; must sign.
    /// * `key_hash` – Hash of the API key secret to revoke.
    pub fn revoke_api_key(
        env: Env,
        merchant: Address,
        key_hash: BytesN<32>,
    ) -> Result<(), MerchantAuthError> {
        merchant.require_auth();

        let key = MerchantAuthDataKey::ApiKey(key_hash.clone());

        let mut record: ApiKeyRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(MerchantAuthError::ApiKeyNotFound)?;

        if record.merchant != merchant {
            return Err(MerchantAuthError::Unauthorized);
        }

        if record.revoked {
            return Err(MerchantAuthError::ApiKeyRevoked);
        }

        record.revoked = true;
        env.storage().persistent().set(&key, &record);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT_AUTH"),
                Symbol::new(&env, "API_KEY_REVOKED"),
            ),
            (merchant, key_hash),
        );

        Ok(())
    }

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

    /// Remaining pull budget for the current period.
    ///
    /// When the stored period has elapsed, the budget is the full limit. This
    /// read does not persist the period reset; `pull_payment` does that.
    pub fn remaining_limit(
        env: Env,
        customer: Address,
        merchant: Address,
    ) -> Result<i128, MerchantAuthError> {
        let auth: MerchantAuthorization = env
            .storage()
            .persistent()
            .get(&MerchantAuthDataKey::Authorization(customer, merchant))
            .ok_or(MerchantAuthError::AuthorizationNotFound)?;

        if !auth.active {
            return Err(MerchantAuthError::AuthorizationInactive);
        }

        let now = env.ledger().timestamp();
        if now >= auth.period_start + auth.period_secs {
            return Ok(auth.limit_per_period);
        }
        Ok(auth
            .limit_per_period
            .saturating_sub(auth.pulled_this_period))
    }

    /// Return the stored API key record, including revoked keys.
    pub fn get_api_key(env: Env, key_hash: BytesN<32>) -> Result<ApiKeyRecord, MerchantAuthError> {
        env.storage()
            .persistent()
            .get(&MerchantAuthDataKey::ApiKey(key_hash))
            .ok_or(MerchantAuthError::ApiKeyNotFound)
    }
}
