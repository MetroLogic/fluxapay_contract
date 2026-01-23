#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, vec, Address, Env, String, Symbol, Vec,
};

mod access_control;
use access_control::{AccessControl, role_oracle, role_settlement_operator};

#[contract]
pub struct RefundManager;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refund {
    pub refund_id: String,
    pub payment_id: String,
    pub amount: i128,
    pub reason: String,
    pub status: RefundStatus,
    pub created_at: u64,
    pub processed_at: Option<u64>,
    pub requester: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RefundStatus {
    Pending,
    Approved,
    Completed,
    Rejected,
}

#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    RefundNotFound = 1,
    RefundAlreadyProcessed = 2,
    InvalidAmount = 3,
    Unauthorized = 4,
    PaymentNotFound = 5,
    AccessControlError = 6,
}

#[contracttype]
pub enum DataKey {
    Refund(String),
    PaymentRefunds(String),
    RefundCounter,
}

#[contractimpl]
impl RefundManager {
    pub fn initialize(env: Env, admin: Address) {
        AccessControl::initialize(&env, admin);
    }

    pub fn grant_role(
        env: Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), Error> {
        AccessControl::grant_role(&env, admin, role, account)
            .map_err(|_| Error::AccessControlError)
    }

    pub fn revoke_role(
        env: Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), Error> {
        AccessControl::revoke_role(&env, admin, role, account)
            .map_err(|_| Error::AccessControlError)
    }

    pub fn has_role(env: Env, role: Symbol, account: Address) -> bool {
        AccessControl::has_role(&env, &role, &account)
    }

    pub fn renounce_role(env: Env, account: Address, role: Symbol) -> Result<(), Error> {
        AccessControl::renounce_role(&env, account, role)
            .map_err(|_| Error::AccessControlError)
    }

    pub fn transfer_admin(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), Error> {
        AccessControl::transfer_admin(&env, current_admin, new_admin)
            .map_err(|_| Error::AccessControlError)
    }

    pub fn get_admin(env: Env) -> Option<Address> {
        AccessControl::get_admin(&env)
    }

    pub fn create_refund(
        env: Env,
        payment_id: String,
        refund_amount: i128,
        reason: String,
        requester: Address,
    ) -> Result<String, Error> {
        if refund_amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let counter = Self::get_next_refund_id(&env);
        let refund_id = match counter {
            1 => String::from_str(&env, "refund_1"),
            2 => String::from_str(&env, "refund_2"),
            3 => String::from_str(&env, "refund_3"),
            4 => String::from_str(&env, "refund_4"),
            5 => String::from_str(&env, "refund_5"),
            6 => String::from_str(&env, "refund_6"),
            7 => String::from_str(&env, "refund_7"),
            8 => String::from_str(&env, "refund_8"),
            9 => String::from_str(&env, "refund_9"),
            10 => String::from_str(&env, "refund_10"),
            _ => String::from_str(&env, "refund_n"),
        };

        let refund = Refund {
            refund_id: refund_id.clone(),
            payment_id: payment_id.clone(),
            amount: refund_amount,
            reason,
            status: RefundStatus::Pending,
            created_at: env.ledger().timestamp(),
            processed_at: None,
            requester,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);

        let mut payment_refunds = Self::get_payment_refunds_internal(&env, &payment_id);
        payment_refunds.push_back(refund_id.clone());
        env.storage()
            .persistent()
            .set(&DataKey::PaymentRefunds(payment_id), &payment_refunds);

        Ok(refund_id)
    }

    pub fn process_refund(
        env: Env,
        operator: Address,
        refund_id: String,
    ) -> Result<(), Error> {
        let has_settlement = AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);
        
        if !has_settlement && !has_oracle {
            return Err(Error::Unauthorized);
        }

        let mut refund = Self::get_refund_internal(&env, &refund_id)?;

        if refund.status != RefundStatus::Pending {
            return Err(Error::RefundAlreadyProcessed);
        }

        refund.status = RefundStatus::Completed;
        refund.processed_at = Some(env.ledger().timestamp());

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id), &refund);

        Ok(())
    }

    pub fn get_refund(env: Env, refund_id: String) -> Result<Refund, Error> {
        Self::get_refund_internal(&env, &refund_id)
    }

    pub fn get_payment_refunds(env: Env, payment_id: String) -> Result<Vec<Refund>, Error> {
        let refund_ids = Self::get_payment_refunds_internal(&env, &payment_id);
        let mut refunds = vec![&env];

        for refund_id in refund_ids.iter() {
            if let Ok(refund) = Self::get_refund_internal(&env, &refund_id) {
                refunds.push_back(refund);
            }
        }

        Ok(refunds)
    }

    fn get_next_refund_id(env: &Env) -> u64 {
        let mut counter: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::RefundCounter)
            .unwrap_or(0);
        counter += 1;
        env.storage()
            .persistent()
            .set(&DataKey::RefundCounter, &counter);
        counter
    }

    fn get_refund_internal(env: &Env, refund_id: &String) -> Result<Refund, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Refund(refund_id.clone()))
            .ok_or(Error::RefundNotFound)
    }

    fn get_payment_refunds_internal(env: &Env, payment_id: &String) -> Vec<String> {
        env.storage()
            .persistent()
            .get(&DataKey::PaymentRefunds(payment_id.clone()))
            .unwrap_or_else(|| vec![env])
    }
}

mod test;
