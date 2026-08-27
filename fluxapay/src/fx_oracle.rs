use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, String, Symbol, Vec,
};

use crate::access_control::{role_admin, role_oracle, AccessControl};

/// Maximum allowed age of a rate in seconds, regardless of admin-configured threshold.
const MAX_RATE_AGE_SECS: u64 = 86_400; // 24 hours

/// Maximum ledger sequence gap since last rate update (~24 h at ~5 s/ledger).
const MAX_LEDGER_GAP: u32 = 17_280;

/// Maximum number of currency pairs accepted by `set_rates_batch`.
const MAX_BATCH_RATES: u32 = 20;

#[contract]
pub struct FXOracle;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateData {
    pub pair: Symbol,
    pub rate: i128,
    pub decimals: u32,
    pub updated_at: u64,
    pub updated_sequence: u32,
}

#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FXOracleError {
    RateNotFound = 1,
    RateStale = 2,
    Unauthorized = 3,
    /// Batch rate update exceeds the maximum of 20 pairs.
    BatchTooLarge = 4,
    /// Issue #478: Rate deviation exceeds configured limit
    RateDeviationExceeded = 5,
}

#[contracttype]
pub enum OracleDataKey {
    Rate(Symbol),
    StalenessThreshold,
    /// Issue #478: Maximum allowed rate deviation per pair in basis points
    MaxDeviation(Symbol),
}

#[cfg_attr(
    any(not(target_arch = "wasm32"), feature = "contract-fx-oracle"),
    contractimpl
)]
#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
impl FXOracle {
    pub fn version() -> u32 {
        1
    }

    pub fn oracle_initialize(env: Env, admin: Address, staleness_threshold: u64) {
        AccessControl::initialize(&env, admin);
        env.storage()
            .instance()
            .set(&OracleDataKey::StalenessThreshold, &staleness_threshold);
    }

    pub fn oracle_grant_role(
        env: Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), FXOracleError> {
        AccessControl::grant_role(&env, admin, role, account)
            .map_err(|_| FXOracleError::Unauthorized)
    }

    pub fn oracle_has_role(env: Env, role: Symbol, account: Address) -> bool {
        AccessControl::has_role(&env, &role, &account)
    }

    pub fn get_fx_admin(env: Env) -> Option<Address> {
        AccessControl::get_admin(&env)
    }

    pub fn set_rate(
        env: Env,
        operator: Address,
        pair: Symbol,
        rate: i128,
        decimals: u32,
    ) -> Result<(), FXOracleError> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_oracle(&env), &operator) {
            return Err(FXOracleError::Unauthorized);
        }

        Self::store_rate(&env, pair.clone(), rate, decimals)?;

        // Emit event: (RATE, UPDATED), pair
        env.events().publish(
            (Symbol::new(&env, "RATE"), Symbol::new(&env, "UPDATED")),
            pair,
        );

        Ok(())
    }

    /// Atomically update up to 20 currency pairs. Requires the ORACLE role.
    ///
    /// Emits `RATE/BATCH_UPDATED` with the number of pairs updated.
    pub fn set_rates_batch(
        env: Env,
        operator: Address,
        rates: Vec<(Symbol, i128, u32)>,
    ) -> Result<u32, FXOracleError> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_oracle(&env), &operator) {
            return Err(FXOracleError::Unauthorized);
        }

        if rates.len() > MAX_BATCH_RATES {
            return Err(FXOracleError::BatchTooLarge);
        }

        let count = rates.len();
        for (pair, rate, decimals) in rates.iter() {
            Self::store_rate(&env, pair, rate, decimals)?;
        }

        env.events().publish(
            (Symbol::new(&env, "RATE"), Symbol::new(&env, "BATCH_UPDATED")),
            count,
        );

        Ok(count)
    }

    fn store_rate(env: &Env, pair: Symbol, rate: i128, decimals: u32) -> Result<(), FXOracleError> {
        // Issue #478: Check rate deviation against configured limit
        let max_deviation_bps = env
            .storage()
            .persistent()
            .get::<OracleDataKey, u32>(&OracleDataKey::MaxDeviation(pair.clone()))
            .unwrap_or(0);

        if max_deviation_bps > 0 {
            if let Ok(last_rate_data) = env
                .storage()
                .persistent()
                .get::<OracleDataKey, RateData>(&OracleDataKey::Rate(pair.clone()))
                .ok_or(FXOracleError::RateNotFound)
            {
                let last_rate = last_rate_data.rate;
                // Calculate deviation in basis points: (abs(new - old) / old) * 10_000
                let diff = if rate > last_rate {
                    rate - last_rate
                } else {
                    last_rate - rate
                };
                let deviation_bps = ((diff * 10_000) / last_rate.abs().max(1)).max(0) as u32;

                if deviation_bps > max_deviation_bps {
                    return Err(FXOracleError::RateDeviationExceeded);
                }

                // Emit warning if deviation is within 50% of limit
                if deviation_bps * 2 >= max_deviation_bps {
                    env.events().publish(
                        (Symbol::new(&env, "RATE"), Symbol::new(&env, "DEVIATION_WARNING")),
                        (pair.clone(), last_rate, rate, deviation_bps),
                    );
                }
            }
            // On first rate set (no prior rate), skip deviation check
        }

        let rate_data = RateData {
            pair: pair.clone(),
            rate,
            decimals,
            updated_at: env.ledger().timestamp(),
            updated_sequence: env.ledger().sequence(),
        };

        env.storage()
            .persistent()
            .set(&OracleDataKey::Rate(pair), &rate_data);
        Ok(())
    }

    pub fn get_rate(env: Env, pair: Symbol) -> Result<RateData, FXOracleError> {
        let rate_data: RateData = env
            .storage()
            .persistent()
            .get(&OracleDataKey::Rate(pair.clone()))
            .ok_or(FXOracleError::RateNotFound)?;

        Self::check_rate_freshness(&env, &rate_data, &pair)?;

        Ok(rate_data)
    }

    /// Permissionless staleness probe. Returns `true` when the rate is stale
    /// (or missing) and emits `RATE/STALE_ALERT` when stale data is present.
    pub fn check_rate_staleness(env: Env, pair: Symbol) -> bool {
        let rate_data: RateData = match env
            .storage()
            .persistent()
            .get(&OracleDataKey::Rate(pair.clone()))
        {
            Some(data) => data,
            None => return true,
        };

        if Self::is_rate_stale(&env, &rate_data) {
            env.events().publish(
                (Symbol::new(&env, "RATE"), Symbol::new(&env, "STALE_ALERT")),
                pair,
            );
            true
        } else {
            false
        }
    }

    // SECURITY: Rate freshness relies on ledger wall-clock time (`env.ledger().timestamp()`),
    // which Stellar validators can influence within a small window (~±a few seconds).
    // A compromised oracle key or delayed off-chain feed could also leave stale rates in
    // storage. Mitigations enforced here:
    //   1. Hard cap (`MAX_RATE_AGE_SECS`) — rates older than 24 h are always rejected,
    //      even if the admin-configured threshold is higher.
    //   2. Ledger-sequence circuit breaker (`MAX_LEDGER_GAP`) — if no rate update has
    //      occurred within the last N ledgers, settlement is blocked and a STALE_ALERT
    //      event is emitted for off-chain monitoring.
    // Accepted residual risk: timestamp drift within the validator window may delay or
    // accelerate staleness by a few seconds. A dual timestamp+sequence AND-check (reject
    // only when both conditions hold) is tracked as a follow-up to reduce false positives.
    fn check_rate_freshness(
        env: &Env,
        rate_data: &RateData,
        pair: &Symbol,
    ) -> Result<(), FXOracleError> {
        if Self::is_rate_stale(env, rate_data) {
            let ledger_gap = env
                .ledger()
                .sequence()
                .saturating_sub(rate_data.updated_sequence);
            if ledger_gap > MAX_LEDGER_GAP {
                env.events().publish(
                    (Symbol::new(env, "RATE"), Symbol::new(env, "STALE_ALERT")),
                    pair.clone(),
                );
            }
            return Err(FXOracleError::RateStale);
        }
        Ok(())
    }

    fn is_rate_stale(env: &Env, rate_data: &RateData) -> bool {
        let configured_threshold: u64 = env
            .storage()
            .instance()
            .get(&OracleDataKey::StalenessThreshold)
            .unwrap_or(MAX_RATE_AGE_SECS);

        let effective_threshold = configured_threshold.min(MAX_RATE_AGE_SECS);

        let now = env.ledger().timestamp();
        if now > rate_data.updated_at.saturating_add(effective_threshold) {
            return true;
        }

        let ledger_gap = env
            .ledger()
            .sequence()
            .saturating_sub(rate_data.updated_sequence);
        ledger_gap > MAX_LEDGER_GAP
    }

    pub fn get_settlement_amount(
        env: Env,
        usdc_amount: i128,
        target_currency: Symbol,
    ) -> Result<i128, FXOracleError> {
        let rate_data = Self::get_rate(env.clone(), target_currency)?;

        let mut divisor = 1i128;
        for _ in 0..rate_data.decimals {
            divisor *= 10;
        }

        Ok((usdc_amount * rate_data.rate) / divisor)
    }

    pub fn get_staleness_threshold(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&OracleDataKey::StalenessThreshold)
            .unwrap_or(MAX_RATE_AGE_SECS)
    }

    /// Upgrade the contract WASM.
    ///
    /// Only the admin can call this. Emits a `CONTRACT/UPGRADED` event with the
    /// old and new version strings on success.
    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<(), FXOracleError> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(FXOracleError::Unauthorized);
        }

        let old_version = String::from_str(&env, "1.0.0");
        env.deployer().update_current_contract_wasm(new_wasm_hash);

        env.events().publish(
            (Symbol::new(&env, "CONTRACT"), Symbol::new(&env, "UPGRADED")),
            (old_version.clone(), String::from_str(&env, "1.0.1")),
        );

        Ok(())
    }

    pub fn set_staleness_threshold(
        env: Env,
        admin: Address,
        threshold: u64,
    ) -> Result<(), FXOracleError> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(FXOracleError::Unauthorized);
        }

        env.storage()
            .instance()
            .set(&OracleDataKey::StalenessThreshold, &threshold);
        Ok(())
    }

    /// Issue #478: Set maximum allowed rate deviation per currency pair in basis points.
    /// Example: 1000 = 10% max allowed deviation.
    pub fn set_rate_deviation_limit(
        env: Env,
        admin: Address,
        pair: Symbol,
        max_deviation_bps: u32,
    ) -> Result<(), FXOracleError> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(FXOracleError::Unauthorized);
        }

        env.storage()
            .persistent()
            .set(&OracleDataKey::MaxDeviation(pair.clone()), &max_deviation_bps);

        // Emit event: (RATE, DEVIATION_LIMIT_SET), pair
        env.events().publish(
            (Symbol::new(&env, "RATE"), Symbol::new(&env, "DEVIATION_LIMIT_SET")),
            (pair, max_deviation_bps),
        );

        Ok(())
    }

    /// Issue #478: Get the maximum allowed rate deviation for a pair (in basis points).
    /// Returns 0 if no limit is configured (unlimited).
    pub fn get_rate_deviation_limit(env: Env, pair: Symbol) -> u32 {
        env.storage()
            .persistent()
            .get::<OracleDataKey, u32>(&OracleDataKey::MaxDeviation(pair))
            .unwrap_or(0) // 0 = no limit
    }
}
