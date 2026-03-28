#![no_std]
use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol};

use crate::access_control::{role_admin, role_oracle, AccessControl};

#[contract]
pub struct FXOracle;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateData {
    pub pair: Symbol,
    pub rate: i128,
    pub decimals: u32,
    pub updated_at: u64,
}

#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FXOracleError {
    RateNotFound = 1,
    RateStale = 2,
    Unauthorized = 3,
}

#[contracttype]
pub enum OracleDataKey {
    Rate(Symbol),
    StalenessThreshold,
}

#[contractimpl]
impl FXOracle {
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

        let rate_data = RateData {
            pair: pair.clone(),
            rate,
            decimals,
            updated_at: env.ledger().timestamp(),
        };

        env.storage()
            .persistent()
            .set(&OracleDataKey::Rate(pair.clone()), &rate_data);

        // Emit event: (RATE, UPDATED), pair
        env.events().publish(
            (Symbol::new(&env, "RATE"), Symbol::new(&env, "UPDATED")),
            pair,
        );

        Ok(())
    }

    pub fn get_rate(env: Env, pair: Symbol) -> Result<RateData, FXOracleError> {
        let rate_data: RateData = env
            .storage()
            .persistent()
            .get(&OracleDataKey::Rate(pair))
            .ok_or(FXOracleError::RateNotFound)?;

        let threshold: u64 = env
            .storage()
            .instance()
            .get(&OracleDataKey::StalenessThreshold)
            .unwrap_or(86400);

        if env.ledger().timestamp() > rate_data.updated_at + threshold {
            return Err(FXOracleError::RateStale);
        }

        Ok(rate_data)
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
            .unwrap_or(86400)
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
}
