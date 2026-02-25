#![no_std]
use soroban_sdk::{
    bytes, contract, contracterror, contractimpl, contracttype, vec, Address, Bytes, BytesN, Env,
    String, Symbol, Vec,
};

mod access_control;
use access_control::{role_oracle, role_settlement_operator, AccessControl};

#[contract]
pub struct PaymentProcessor;

#[contract]
pub struct RefundManager;

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

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refund {
    pub refund_id: String,
    pub payment_id: String,
    pub amount: i128,
    pub reason: String,
    pub status: RefundStatus,
    pub requester: Address,
    pub created_at: u64,
    pub processed_at: Option<u64>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RefundStatus {
    Pending,
    Completed,
    Rejected,
}

#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    PaymentNotFound = 1,
    PaymentAlreadyExists = 2,
    InvalidAmount = 3,
    AccessControlError = 4,
    PaymentExpired = 5,
    PaymentAlreadyProcessed = 6,
    InvalidPaymentId = 7,
    RefundNotFound = 8,
    RefundAlreadyProcessed = 9,
    Unauthorized = 10,
}

#[contracttype]
pub enum DataKey {
    Payment(String),
    Refund(String),
    PaymentRefunds(String),
    RefundCounter,
}

#[contractimpl]
impl RefundManager {
    pub fn initialize_refund_manager(env: Env, admin: Address) {
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
        requester.require_auth();

        if refund_amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let counter = Self::get_next_refund_id(&env);

        // Build refund ID: "refund_" + counter
        // For simplicity and to avoid complex string manipulation in no_std,
        // we use a match statement for common cases
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
            11 => String::from_str(&env, "refund_11"),
            12 => String::from_str(&env, "refund_12"),
            13 => String::from_str(&env, "refund_13"),
            14 => String::from_str(&env, "refund_14"),
            15 => String::from_str(&env, "refund_15"),
            16 => String::from_str(&env, "refund_16"),
            17 => String::from_str(&env, "refund_17"),
            18 => String::from_str(&env, "refund_18"),
            19 => String::from_str(&env, "refund_19"),
            20 => String::from_str(&env, "refund_20"),
            _ => {
                // For numbers > 20, construct manually using bytes
                let prefix = bytes!(&env, 0x726566756e645f); // "refund_" in ASCII hex
                let mut result = Bytes::new(&env);
                result.append(&prefix);

                // Convert number to ASCII digits (collect in reverse, then reverse)
                let mut temp = Bytes::new(&env);
                let mut n = counter;
                loop {
                    temp.push_back((n % 10) as u8 + 48); // 48 is ASCII '0'
                    n /= 10;
                    if n == 0 {
                        break;
                    }
                }
                // Reverse the digits
                let len = temp.len();
                for i in 0..len {
                    result.push_back(temp.get(len - 1 - i).unwrap());
                }

                // Convert bytes to string using a fixed-size array
                // We know refund IDs won't exceed 64 bytes
                let mut arr = [0u8; 64];
                for i in 0..result.len().min(64) {
                    arr[i as usize] = result.get(i).unwrap();
                }
                String::from_bytes(&env, &arr[..result.len() as usize])
            }
        };

        let refund = Refund {
            refund_id: refund_id.clone(),
            payment_id: payment_id.clone(),
            amount: refund_amount,
            reason,
            status: RefundStatus::Pending,
            requester,
            created_at: env.ledger().timestamp(),
            processed_at: None,
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
        for id in refund_ids.iter() {
            if let Ok(refund) = Self::get_refund_internal(&env, &id) {
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

#[contractimpl]
impl PaymentProcessor {
    #[allow(deprecated)]
    pub fn create_payment(
        env: Env,
        payment_id: String,
        merchant_id: Address,
        amount: i128,
        currency: Symbol,
        deposit_address: Address,
        expires_at: u64,
    ) -> Result<PaymentCharge, Error> {
        merchant_id.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        if env
            .storage()
            .persistent()
            .has(&DataKey::Payment(payment_id.clone()))
        {
            return Err(Error::PaymentAlreadyExists);
        }

        if payment_id.is_empty() {
            return Err(Error::InvalidPaymentId);
        }

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

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);

        env.events().publish(
            (Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "CREATED")),
            payment_id,
        );

        Ok(payment)
    }

    #[allow(deprecated)]
    pub fn verify_payment(
        env: Env,
        payment_id: String,
        transaction_hash: BytesN<32>,
        payer_address: Address,
        amount_received: i128,
    ) -> Result<PaymentStatus, Error> {
        let mut payment = Self::get_payment_internal(&env, &payment_id)?;

        if payment.status != PaymentStatus::Pending {
            return Err(Error::PaymentAlreadyProcessed);
        }

        if env.ledger().timestamp() > payment.expires_at {
            return Err(Error::PaymentExpired);
        }

        if amount_received != payment.amount {
            payment.status = PaymentStatus::Failed;
            env.storage()
                .persistent()
                .set(&DataKey::Payment(payment_id.clone()), &payment);

            env.events().publish(
                (Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "FAILED")),
                payment_id,
            );

            return Ok(PaymentStatus::Failed);
        }

        payment.status = PaymentStatus::Confirmed;
        payment.payer_address = Some(payer_address);
        payment.transaction_hash = Some(transaction_hash);
        payment.confirmed_at = Some(env.ledger().timestamp());

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);

        env.events().publish(
            (Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "VERIFIED")),
            payment_id,
        );

        Ok(PaymentStatus::Confirmed)
    }

    pub fn get_payment(env: Env, payment_id: String) -> Result<PaymentCharge, Error> {
        Self::get_payment_internal(&env, &payment_id)
    }

    #[allow(deprecated)]
    pub fn cancel_payment(env: Env, payment_id: String) -> Result<(), Error> {
        let mut payment = Self::get_payment_internal(&env, &payment_id)?;

        if payment.status != PaymentStatus::Pending {
            return Err(Error::PaymentAlreadyProcessed);
        }

        if env.ledger().timestamp() <= payment.expires_at {
            return Err(Error::Unauthorized);
        }

        payment.status = PaymentStatus::Expired;

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);

        env.events().publish(
            (Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "CANCELLED")),
            payment_id,
        );

        Ok(())
    }

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
