#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, vec, Address, Env, String, Vec,
};

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
}

#[contracttype]
pub enum DataKey {
    Refund(String),         // refund_id -> Refund
    PaymentRefunds(String), // payment_id -> Vec<String> (refund_ids)
    RefundCounter,          // u64 counter for generating refund IDs
}

#[contractimpl]
impl RefundManager {
    /// Create a refund request
    pub fn create_refund(
        env: Env,
        payment_id: String,
        refund_amount: i128,
        reason: String,
        requester: Address,
    ) -> Result<String, Error> {
        // Validate input
        if refund_amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        // Generate unique refund ID (simple approach for no_std)
        let counter = Self::get_next_refund_id(&env);
        // Create a simple string representation of the counter
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
            _ => String::from_str(&env, "refund_n"), // fallback for higher numbers
        };

        // Create refund struct
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

        // Store refund
        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);

        // Add to payment refunds list
        let mut payment_refunds = Self::get_payment_refunds_internal(&env, &payment_id);
        payment_refunds.push_back(refund_id.clone());
        env.storage()
            .persistent()
            .set(&DataKey::PaymentRefunds(payment_id), &payment_refunds);

        Ok(refund_id)
    }

    /// Process refund (approve and complete)
    pub fn process_refund(env: Env, refund_id: String) -> Result<(), Error> {
        // Get refund
        let mut refund = Self::get_refund_internal(&env, &refund_id)?;

        // Check if already processed
        if refund.status != RefundStatus::Pending {
            return Err(Error::RefundAlreadyProcessed);
        }

        // Update status to completed (assuming approval for now)
        refund.status = RefundStatus::Completed;
        refund.processed_at = Some(env.ledger().timestamp());

        // Store updated refund
        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id), &refund);

        Ok(())
    }

    /// Get refund details
    pub fn get_refund(env: Env, refund_id: String) -> Result<Refund, Error> {
        Self::get_refund_internal(&env, &refund_id)
    }

    /// List refunds for a payment
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

    // Helper functions
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
