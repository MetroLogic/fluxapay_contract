#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, vec, Address, Env, String, Symbol, Vec,
   Address, BytesN,
    Env, String, Symbol,
};

mod access_control;
use access_control::{role_oracle, role_settlement_operator, AccessControl};

#[contract]
pub struct PaymentProcessor;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentCharge {
    pub payment_id: String,
    pub merchant_id: Address,
    pub amount: i128,
    pub currency: Symbol,
    pub deposit_address: Address,
    pub status: PaymentStatus,
    pub payer_address: Option<Address>,
    pub transaction_hash: Option<BytesN<32>>,
    pub created_at: u64,
    pub confirmed_at: Option<u64>,
    pub expires_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaymentStatus {
    Pending,
    Confirmed,
    Expired,
    Failed,
}

#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    PaymentNotFound = 1,
    PaymentAlreadyExists = 2,
    InvalidAmount = 3,
    Unauthorized = 4,
    PaymentNotFound = 5,
    AccessControlError = 6,
    PaymentExpired = 4,
    PaymentAlreadyProcessed = 5,
    Unauthorized = 6,
    InvalidPaymentId = 7,
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
        AccessControl::grant_role(&env, admin, role, account).map_err(|_| Error::AccessControlError)
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
        AccessControl::renounce_role(&env, account, role).map_err(|_| Error::AccessControlError)
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
    Payment(String),     // payment_id -> PaymentCharge
    PaymentCounter,      // u64 counter for generating payment IDs
}

#[contractimpl]
impl PaymentProcessor {
    /// Create a new payment
    pub fn create_payment(
        env: Env,
        payment_id: String,
        merchant_id: Address,
        amount: i128,
        currency: Symbol,
        deposit_address: Address,
        expires_at: u64,
    ) -> Result<PaymentCharge, Error> {
        // Validate input
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        // Check if payment already exists
        if env.storage().persistent().has(&DataKey::Payment(payment_id.clone())) {
            return Err(Error::PaymentAlreadyExists);
        }

        // Validate payment_id is not empty
        if payment_id.is_empty() {
            return Err(Error::InvalidPaymentId);
        }

        // Create payment struct
        let payment = PaymentCharge {
            payment_id: payment_id.clone(),
            merchant_id,
            amount,
            currency,
            deposit_address,
            status: PaymentStatus::Pending,
            payer_address: None,
            transaction_hash: None,
            created_at: env.ledger().timestamp(),
            confirmed_at: None,
            expires_at,
        };

        // Store payment
        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);

        let mut payment_refunds = Self::get_payment_refunds_internal(&env, &payment_id);
        payment_refunds.push_back(refund_id.clone());
        env.storage()
            .persistent()
            .set(&DataKey::PaymentRefunds(payment_id), &payment_refunds);
        // Emit payment created event
        env.events().publish((Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "CREATED")), payment_id.clone());

        Ok(payment)
    }

    pub fn process_refund(env: Env, operator: Address, refund_id: String) -> Result<(), Error> {
        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
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

    /// Verify payment after customer sends USDC
    pub fn verify_payment(
        env: Env,
        payment_id: String,
        transaction_hash: BytesN<32>,
        payer_address: Address,
        amount_received: i128,
    ) -> Result<PaymentStatus, Error> {
        // Get payment
        let mut payment = Self::get_payment_internal(&env, &payment_id)?;

        // Check if payment is still pending
        if payment.status != PaymentStatus::Pending {
            return Err(Error::PaymentAlreadyProcessed);
        }

        // Check if payment has expired
        if env.ledger().timestamp() > payment.expires_at {
            return Err(Error::PaymentExpired);
        }

        // Verify amount matches (exact match for now)
        if amount_received != payment.amount {
            // Update status to failed
            payment.status = PaymentStatus::Failed;
            env.storage()
                .persistent()
                .set(&DataKey::Payment(payment_id.clone()), &payment);

            // Emit payment failed event
            env.events().publish((Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "FAILED")), payment_id.clone());

            return Ok(PaymentStatus::Failed);
        }

        // Update payment with verification details
        payment.status = PaymentStatus::Confirmed;
        payment.payer_address = Some(payer_address);
        payment.transaction_hash = Some(transaction_hash);
        payment.confirmed_at = Some(env.ledger().timestamp());

        // Store updated payment
        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);

        // Emit payment verified event
        env.events().publish((Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "VERIFIED")), payment_id.clone());

        Ok(PaymentStatus::Confirmed)
    }

    pub fn get_refund(env: Env, refund_id: String) -> Result<Refund, Error> {
        Self::get_refund_internal(&env, &refund_id)
    }

    pub fn get_payment_refunds(env: Env, payment_id: String) -> Result<Vec<Refund>, Error> {
        let refund_ids = Self::get_payment_refunds_internal(&env, &payment_id);
        let mut refunds = vec![&env];
    /// Get payment details
    pub fn get_payment(env: Env, payment_id: String) -> Result<PaymentCharge, Error> {
        Self::get_payment_internal(&env, &payment_id)
    }

    /// Cancel expired payment
    pub fn cancel_payment(env: Env, payment_id: String) -> Result<(), Error> {
        // Get payment
        let mut payment = Self::get_payment_internal(&env, &payment_id)?;

        // Check if payment is pending
        if payment.status != PaymentStatus::Pending {
            return Err(Error::PaymentAlreadyProcessed);
        }

        // Check if payment has expired
        if env.ledger().timestamp() <= payment.expires_at {
            return Err(Error::Unauthorized); // Not expired yet
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
        // Update status to expired
        payment.status = PaymentStatus::Expired;

        // Store updated payment
        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);

        // Emit payment cancelled event
        env.events().publish((Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "CANCELLED")), payment_id.clone());

        Ok(())
    }

    // Helper functions
    fn get_payment_internal(env: &Env, payment_id: &String) -> Result<PaymentCharge, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Payment(payment_id.clone()))
            .ok_or(Error::PaymentNotFound)
    }
}

pub mod merchant_registry;
#[cfg(test)]
mod merchant_registry_test;
mod test;
