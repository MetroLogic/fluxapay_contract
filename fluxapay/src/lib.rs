#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, BytesN,
    Env, String, Symbol,
};

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
    PaymentExpired = 4,
    PaymentAlreadyProcessed = 5,
    Unauthorized = 6,
    InvalidPaymentId = 7,
}

#[contracttype]
pub enum DataKey {
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

        // Emit payment created event
        env.events().publish((Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "CREATED")), payment_id.clone());

        Ok(payment)
    }

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

mod test;
