#![no_std]
use soroban_sdk::{
    bytes, contract, contracterror, contractimpl, contracttype, token, vec, Address, Bytes,
    BytesN, Env, MuxedAddress, String, Symbol, Vec,
};

mod access_control;
pub mod fx_oracle;
use access_control::{role_merchant, role_oracle, role_settlement_operator, AccessControl};
// Re-export for tests
#[allow(unused_imports)]
pub use access_control::AccessControlDataKey;
use fx_oracle::{FXOracle, FXOracleClient, FXOracleError, RateData};

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
    Settled,
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

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisputeStatus {
    Open,
    UnderReview,
    Resolved,
    Rejected,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dispute {
    pub dispute_id: String,
    pub payment_id: String,
    pub refund_id: Option<String>,
    pub amount: i128,
    pub reason: String,
    pub evidence: String,
    pub status: DisputeStatus,
    pub disputer: Address,
    pub created_at: u64,
    pub resolved_at: Option<u64>,
    pub resolution_notes: Option<String>,
}

#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FluxaError {
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
    DisputeNotFound = 11,
    DisputeAlreadyResolved = 12,
    RefundExceedsPayment = 13,
}

#[contracttype]
pub enum FluxaDataKey {
    Payment(String),
    MerchantPayments(Address),
    Refund(String),
    PaymentRefunds(String),
    RefundCounter,
    Dispute(String),
    PaymentDisputes(String),
    DisputeCounter,
    UsdcToken,
}

const SHORT_LIVE_TTL: u32 = 120_960; // ~1 week at 5s/ledger
const LONG_LIVE_TTL: u32 = 18_921_600; // ~3 years at 5s/ledger
const TTL_BUMP_THRESHOLD_DIVISOR: u32 = 5;

#[contractimpl]
impl RefundManager {
    pub fn initialize_refund_manager(env: Env, admin: Address, usdc_token_address: Address) {
        AccessControl::initialize(&env, admin);
        env.storage()
            .persistent()
            .set(&FluxaDataKey::UsdcToken, &usdc_token_address);
    }

    pub fn refund_grant_role(
        env: Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), FluxaError> {
        AccessControl::grant_role(&env, admin, role, account).map_err(|_| FluxaError::AccessControlError)
    }

    pub fn refund_revoke_role(
        env: Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), FluxaError> {
        AccessControl::revoke_role(&env, admin, role, account)
            .map_err(|_| FluxaError::AccessControlError)
    }

    pub fn refund_has_role(env: Env, role: Symbol, account: Address) -> bool {
        AccessControl::has_role(&env, &role, &account)
    }

    pub fn refund_renounce_role(env: Env, account: Address, role: Symbol) -> Result<(), FluxaError> {
        AccessControl::renounce_role(&env, account, role).map_err(|_| FluxaError::AccessControlError)
    }

    pub fn refund_transfer_admin(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), FluxaError> {
        AccessControl::transfer_admin(&env, current_admin, new_admin)
            .map_err(|_| FluxaError::AccessControlError)
    }

    pub fn refund_get_admin(env: Env) -> Option<Address> {
        AccessControl::get_admin(&env)
    }

    /// Returns all addresses currently holding the given role (issue #37).
    pub fn get_role_members(env: Env, role: Symbol) -> Vec<Address> {
        AccessControl::get_role_members(&env, &role)
    }

    /// Register a payment with the refund manager so refund amounts can be validated.
    pub fn register_payment(
        env: Env,
        payment_id: String,
        merchant_id: Address,
        amount: i128,
        currency: Symbol,
    ) {
        if !env
            .storage()
            .persistent()
            .has(&FluxaDataKey::Payment(payment_id.clone()))
        {
            let payment = PaymentCharge {
                payment_id: payment_id.clone(),
                merchant_id,
                amount,
                currency,
                deposit_address: env.current_contract_address(),
                status: PaymentStatus::Pending,
                payer_address: None,
                transaction_hash: None,
                created_at: env.ledger().timestamp(),
                confirmed_at: None,
                expires_at: 0,
            };
            env.storage()
                .persistent()
                .set(&FluxaDataKey::Payment(payment_id.clone()), &payment);
            Self::bump_payment_ttl(&env, &payment_id, &payment.status);
        }
    }

    pub fn create_refund(
        env: Env,
        payment_id: String,
        refund_amount: i128,
        reason: String,
        requester: Address,
    ) -> Result<String, FluxaError> {
        requester.require_auth();
        Self::create_refund_internal(&env, payment_id, refund_amount, reason, requester)
    }

    fn create_refund_internal(
        env: &Env,
        payment_id: String,
        refund_amount: i128,
        reason: String,
        requester: Address,
    ) -> Result<String, FluxaError> {
        if refund_amount <= 0 {
            return Err(FluxaError::InvalidAmount);
        }

        // Validate refund amount does not exceed original payment amount
        let payment: PaymentCharge = env
            .storage()
            .persistent()
            .get(&FluxaDataKey::Payment(payment_id.clone()))
            .ok_or(FluxaError::PaymentNotFound)?;

        // Sum existing refund amounts for this payment
        let existing_refunds = Self::get_payment_refunds_internal(env, &payment_id);
        let mut total_refunded: i128 = 0;
        for id in existing_refunds.iter() {
            if let Ok(r) = Self::get_refund_internal(env, &id) {
                if r.status != RefundStatus::Rejected {
                    total_refunded += r.amount;
                }
            }
        }

        if total_refunded + refund_amount > payment.amount {
            return Err(FluxaError::RefundExceedsPayment);
        }

        let counter = Self::get_next_refund_id(&env);

        // Build refund ID: "refund_" + counter
        // For simplicity and to avoid complex string manipulation in no_std,
        // we use a match statement for common cases
        let refund_id = format_id(&env, "refund_", counter);

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
            .set(&FluxaDataKey::Refund(refund_id.clone()), &refund);

        let mut payment_refunds = Self::get_payment_refunds_internal(env, &payment_id);
        payment_refunds.push_back(refund_id.clone());
        env.storage()
            .persistent()
            .set(&FluxaDataKey::PaymentRefunds(payment_id.clone()), &payment_refunds);
        Self::bump_ttl(
            &env,
            &FluxaDataKey::PaymentRefunds(payment_id.clone()),
            LONG_LIVE_TTL,
        );

        Self::bump_refund_ttl(&env, &refund_id, &refund.status);

        // Issue #27: emit REFUND/CREATED event
        env.events().publish(
            (Symbol::new(env, "REFUND"), Symbol::new(env, "CREATED")),
            (payment_id, refund_id.clone(), refund_amount),
        );

        Ok(refund_id)
    }

    pub fn process_refund(env: Env, operator: Address, refund_id: String) -> Result<(), FluxaError> {
        operator.require_auth();
        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);

        if !has_settlement && !has_oracle {
            return Err(FluxaError::Unauthorized);
        }

        Self::process_refund_internal(&env, &operator, refund_id)
    }

    fn process_refund_internal(
        env: &Env,
        _operator: &Address,
        refund_id: String,
    ) -> Result<(), FluxaError> {
        let mut refund = Self::get_refund_internal(env, &refund_id)?;

        if refund.status != RefundStatus::Pending {
            return Err(FluxaError::RefundAlreadyProcessed);
        }

        let usdc_token_address: Address = env
            .storage()
            .persistent()
            .get(&FluxaDataKey::UsdcToken)
            .ok_or(FluxaError::Unauthorized)?;
        let token_client = token::TokenClient::new(env, &usdc_token_address);

        let from = env.current_contract_address();
        let to: MuxedAddress = (&refund.requester).into();
        if token_client.try_transfer(&from, &to, &refund.amount).is_err() {
            return Ok(());
        }

        refund.status = RefundStatus::Completed;
        refund.processed_at = Some(env.ledger().timestamp());

        env.storage()
            .persistent()
            .set(&FluxaDataKey::Refund(refund_id.clone()), &refund);
        Self::bump_refund_ttl(&env, &refund_id, &refund.status);

        // Issue #27: emit REFUND/COMPLETED event
        env.events().publish(
            (Symbol::new(env, "REFUND"), Symbol::new(env, "COMPLETED")),
            (refund.payment_id, refund_id, refund.amount),
        );

        Ok(())
    }

    /// Reject a pending refund (operator only). Emits REFUND/REJECTED (issue #27).
    pub fn reject_refund(env: Env, operator: Address, refund_id: String) -> Result<(), FluxaError> {
        operator.require_auth();
        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);

        if !has_settlement && !has_oracle {
            return Err(FluxaError::Unauthorized);
        }

        let mut refund = Self::get_refund_internal(&env, &refund_id)?;

        if refund.status != RefundStatus::Pending {
            return Err(FluxaError::RefundAlreadyProcessed);
        }

        refund.status = RefundStatus::Rejected;
        refund.processed_at = Some(env.ledger().timestamp());

        env.storage()
            .persistent()
            .set(&FluxaDataKey::Refund(refund_id.clone()), &refund);
        Self::bump_refund_ttl(&env, &refund_id, &refund.status);

        // Issue #27: emit REFUND/REJECTED event
        env.events().publish(
            (Symbol::new(&env, "REFUND"), Symbol::new(&env, "REJECTED")),
            (refund.payment_id, refund_id, refund.amount),
        );

        Ok(())
    }

    pub fn get_refund(env: Env, refund_id: String) -> Result<Refund, FluxaError> {
        Self::get_refund_internal(&env, &refund_id)
    }

    pub fn get_payment_refunds(env: Env, payment_id: String) -> Result<Vec<Refund>, FluxaError> {
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
            .get(&FluxaDataKey::RefundCounter)
            .unwrap_or(0);
        counter += 1;
        env.storage()
            .persistent()
            .set(&FluxaDataKey::RefundCounter, &counter);
        counter
    }

    fn get_refund_internal(env: &Env, refund_id: &String) -> Result<Refund, FluxaError> {
        env.storage()
            .persistent()
            .get(&FluxaDataKey::Refund(refund_id.clone()))
            .ok_or(FluxaError::RefundNotFound)
    }

    fn get_payment_refunds_internal(env: &Env, payment_id: &String) -> Vec<String> {
        env.storage()
            .persistent()
            .get(&FluxaDataKey::PaymentRefunds(payment_id.clone()))
            .unwrap_or_else(|| vec![env])
    }

    // Dispute handling functions
    pub fn create_dispute(
        env: Env,
        payment_id: String,
        amount: i128,
        reason: String,
        evidence: String,
        disputer: Address,
    ) -> Result<String, FluxaError> {
        disputer.require_auth();

        if amount <= 0 {
            return Err(FluxaError::InvalidAmount);
        }

        let counter = Self::get_next_dispute_id(&env);
        let dispute_id = Self::build_dispute_id(&env, counter);

        let dispute = Dispute {
            dispute_id: dispute_id.clone(),
            payment_id: payment_id.clone(),
            refund_id: None,
            amount,
            reason,
            evidence,
            status: DisputeStatus::Open,
            disputer,
            created_at: env.ledger().timestamp(),
            resolved_at: None,
            resolution_notes: None,
        };

        env.storage()
            .persistent()
            .set(&FluxaDataKey::Dispute(dispute_id.clone()), &dispute);

        let mut payment_disputes = Self::get_payment_disputes_internal(&env, &payment_id);
        payment_disputes.push_back(dispute_id.clone());
        env.storage()
            .persistent()
            .set(
                &FluxaDataKey::PaymentDisputes(payment_id.clone()),
                &payment_disputes,
            );
        Self::bump_ttl(
            &env,
            &FluxaDataKey::PaymentDisputes(payment_id.clone()),
            LONG_LIVE_TTL,
        );

        Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);

        // Issue #27: emit DISPUTE/OPENED event
        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "OPENED")),
            (payment_id, dispute_id.clone(), amount),
        );

        Ok(dispute_id)
    }

    pub fn review_dispute(env: Env, operator: Address, dispute_id: String) -> Result<(), FluxaError> {
        operator.require_auth();

        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);

        if !has_settlement && !has_oracle {
            return Err(FluxaError::Unauthorized);
        }

        let mut dispute = Self::get_dispute_internal(&env, &dispute_id)?;

        if dispute.status != DisputeStatus::Open {
            return Err(FluxaError::DisputeAlreadyResolved);
        }

        dispute.status = DisputeStatus::UnderReview;

        env.storage()
            .persistent()
            .set(&FluxaDataKey::Dispute(dispute_id.clone()), &dispute);
        Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);

        // Issue #27: emit DISPUTE/UNDER_REVIEW event
        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "UNDER_REVIEW")),
            (dispute.payment_id, dispute_id, dispute.amount),
        );

        Ok(())
    }

    pub fn resolve_dispute_with_refund(
        env: Env,
        operator: Address,
        dispute_id: String,
        resolution_notes: String,
    ) -> Result<String, FluxaError> {
        operator.require_auth();

        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);

        if !has_settlement && !has_oracle {
            return Err(FluxaError::Unauthorized);
        }

        let mut dispute = Self::get_dispute_internal(&env, &dispute_id)?;

        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Err(FluxaError::DisputeAlreadyResolved);
        }

        // Create refund for the disputed amount
        let refund_reason = String::from_str(&env, "Refund issued due to dispute resolution");

        let refund_id = Self::create_refund_internal(
            &env,
            dispute.payment_id.clone(),
            dispute.amount,
            refund_reason,
            dispute.disputer.clone(),
        )?;

        // Process the refund immediately
        Self::process_refund_internal(&env, &operator, refund_id.clone())?;

        // Update dispute status
        dispute.status = DisputeStatus::Resolved;
        dispute.refund_id = Some(refund_id.clone());
        dispute.resolved_at = Some(env.ledger().timestamp());
        dispute.resolution_notes = Some(resolution_notes);

        env.storage()
            .persistent()
            .set(&FluxaDataKey::Dispute(dispute_id.clone()), &dispute);
        Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);

        // Issue #27: emit DISPUTE/RESOLVED event
        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "RESOLVED")),
            (dispute.payment_id, dispute_id, dispute.amount),
        );

        Ok(refund_id)
    }

    pub fn reject_dispute(
        env: Env,
        operator: Address,
        dispute_id: String,
        resolution_notes: String,
    ) -> Result<(), FluxaError> {
        operator.require_auth();

        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);

        if !has_settlement && !has_oracle {
            return Err(FluxaError::Unauthorized);
        }

        let mut dispute = Self::get_dispute_internal(&env, &dispute_id)?;

        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Err(FluxaError::DisputeAlreadyResolved);
        }

        dispute.status = DisputeStatus::Rejected;
        dispute.resolved_at = Some(env.ledger().timestamp());
        dispute.resolution_notes = Some(resolution_notes);

        env.storage()
            .persistent()
            .set(&FluxaDataKey::Dispute(dispute_id.clone()), &dispute);
        Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);

        // Issue #27: emit DISPUTE/REJECTED event
        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "REJECTED")),
            (dispute.payment_id, dispute_id, dispute.amount),
        );

        Ok(())
    }

    pub fn get_dispute(env: Env, dispute_id: String) -> Result<Dispute, FluxaError> {
        Self::get_dispute_internal(&env, &dispute_id)
    }

    pub fn get_payment_disputes(env: Env, payment_id: String) -> Result<Vec<Dispute>, FluxaError> {
        let dispute_ids = Self::get_payment_disputes_internal(&env, &payment_id);
        let mut disputes = vec![&env];
        for id in dispute_ids.iter() {
            if let Ok(dispute) = Self::get_dispute_internal(&env, &id) {
                disputes.push_back(dispute);
            }
        }
        Ok(disputes)
    }

    fn get_next_dispute_id(env: &Env) -> u64 {
        let mut counter: u64 = env
            .storage()
            .persistent()
            .get(&FluxaDataKey::DisputeCounter)
            .unwrap_or(0);
        counter += 1;
        env.storage()
            .persistent()
            .set(&FluxaDataKey::DisputeCounter, &counter);
        counter
    }

    fn build_dispute_id(env: &Env, counter: u64) -> String {
        format_id(env, "dispute_", counter)
    }

    fn get_dispute_internal(env: &Env, dispute_id: &String) -> Result<Dispute, FluxaError> {
        env.storage()
            .persistent()
            .get(&FluxaDataKey::Dispute(dispute_id.clone()))
            .ok_or(FluxaError::DisputeNotFound)
    }

    fn get_payment_disputes_internal(env: &Env, payment_id: &String) -> Vec<String> {
        env.storage()
            .persistent()
            .get(&FluxaDataKey::PaymentDisputes(payment_id.clone()))
            .unwrap_or_else(|| vec![env])
    }

    fn refund_ttl(status: &RefundStatus) -> u32 {
        match status {
            RefundStatus::Pending => SHORT_LIVE_TTL,
            RefundStatus::Completed | RefundStatus::Rejected => LONG_LIVE_TTL,
        }
    }

    fn bump_refund_ttl(env: &Env, refund_id: &String, status: &RefundStatus) {
        let key = FluxaDataKey::Refund(refund_id.clone());
        Self::bump_ttl(env, &key, Self::refund_ttl(status));
    }

    fn dispute_ttl(status: &DisputeStatus) -> u32 {
        match status {
            DisputeStatus::Open | DisputeStatus::UnderReview => SHORT_LIVE_TTL,
            DisputeStatus::Resolved | DisputeStatus::Rejected => LONG_LIVE_TTL,
        }
    }

    fn bump_dispute_ttl(env: &Env, dispute_id: &String, status: &DisputeStatus) {
        let key = FluxaDataKey::Dispute(dispute_id.clone());
        Self::bump_ttl(env, &key, Self::dispute_ttl(status));
    }

    fn payment_ttl(status: &PaymentStatus) -> u32 {
        match status {
            PaymentStatus::Pending => SHORT_LIVE_TTL,
            PaymentStatus::Confirmed
            | PaymentStatus::Settled
            | PaymentStatus::Expired
            | PaymentStatus::Failed => LONG_LIVE_TTL,
        }
    }

    fn bump_payment_ttl(env: &Env, payment_id: &String, status: &PaymentStatus) {
        let key = FluxaDataKey::Payment(payment_id.clone());
        Self::bump_ttl(env, &key, Self::payment_ttl(status));
    }

    fn bump_ttl(env: &Env, key: &FluxaDataKey, ttl: u32) {
        let threshold = core::cmp::max(1, ttl / TTL_BUMP_THRESHOLD_DIVISOR);
        env.storage().persistent().extend_ttl(key, threshold, ttl);
    }
}

#[contractimpl]
impl PaymentProcessor {
    pub fn initialize_payment_processor(env: Env, admin: Address) {
        AccessControl::initialize(&env, admin);
    }

    pub fn payment_grant_role(
        env: Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), FluxaError> {
        AccessControl::grant_role(&env, admin, role, account).map_err(|_| FluxaError::AccessControlError)
    }

    #[allow(deprecated)]
    pub fn create_payment(
        env: Env,
        payment_id: String,
        merchant_id: Address,
        amount: i128,
        currency: Symbol,
        deposit_address: Address,
        expires_at: u64,
    ) -> Result<PaymentCharge, FluxaError> {
        merchant_id.require_auth();

        // Verify that the merchant has the MERCHANT role (granted on verification)
        if !AccessControl::has_role(&env, &role_merchant(&env), &merchant_id) {
            return Err(FluxaError::Unauthorized);
        }

        if amount <= 0 {
            return Err(FluxaError::InvalidAmount);
        }

        if env
            .storage()
            .persistent()
            .has(&FluxaDataKey::Payment(payment_id.clone()))
        {
            return Err(FluxaError::PaymentAlreadyExists);
        }

        if payment_id.is_empty() {
            return Err(FluxaError::InvalidPaymentId);
        }

        let payment = PaymentCharge {
            payment_id: payment_id.clone(),
            merchant_id: merchant_id.clone(),
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
            .set(&FluxaDataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);

        let mut merchant_payments = Self::get_merchant_payments_internal(&env, &merchant_id);
        merchant_payments.push_back(payment_id.clone());
        let merchant_payments_key = FluxaDataKey::MerchantPayments(merchant_id);
        env.storage()
            .persistent()
            .set(&merchant_payments_key, &merchant_payments);
        Self::bump_ttl(&env, &merchant_payments_key, LONG_LIVE_TTL);

        env.events().publish(
            (Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "CREATED")),
            payment_id,
        );

        Ok(payment)
    }

    #[allow(deprecated)]
    pub fn verify_payment(
        env: Env,
        oracle: Address,
        payment_id: String,
        transaction_hash: BytesN<32>,
        payer_address: Address,
        amount_received: i128,
    ) -> Result<PaymentStatus, FluxaError> {
        oracle.require_auth();

        if !AccessControl::has_role(&env, &role_oracle(&env), &oracle)
            && !AccessControl::has_role(&env, &role_settlement_operator(&env), &oracle)
        {
            return Err(FluxaError::Unauthorized);
        }

        let mut payment = Self::get_payment_internal(&env, &payment_id)?;

        if payment.status != PaymentStatus::Pending {
            return Err(FluxaError::PaymentAlreadyProcessed);
        }

        if env.ledger().timestamp() > payment.expires_at {
            return Err(FluxaError::PaymentExpired);
        }

        if amount_received != payment.amount {
            payment.status = PaymentStatus::Failed;
            env.storage()
                .persistent()
                .set(&FluxaDataKey::Payment(payment_id.clone()), &payment);
            Self::bump_payment_ttl(&env, &payment_id, &payment.status);

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
            .set(&FluxaDataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);

        env.events().publish(
            (Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "VERIFIED")),
            payment_id,
        );

        Ok(PaymentStatus::Confirmed)
    }

    pub fn get_payment(env: Env, payment_id: String) -> Result<PaymentCharge, FluxaError> {
        Self::get_payment_internal(&env, &payment_id)
    }

    pub fn get_merchant_payments(env: Env, merchant_id: Address) -> Vec<String> {
        Self::get_merchant_payments_internal(&env, &merchant_id)
    }

    pub fn get_merchant_payments_paginated(
        env: Env,
        merchant_id: Address,
        offset: u32,
        limit: u32,
    ) -> Vec<String> {
        let all = Self::get_merchant_payments_internal(&env, &merchant_id);
        if limit == 0 {
            return vec![&env];
        }

        let mut page = vec![&env];
        let start = offset;
        let end = core::cmp::min(all.len(), start.saturating_add(limit));

        let mut i = start;
        while i < end {
            if let Some(id) = all.get(i) {
                page.push_back(id);
            }
            i += 1;
        }

        page
    }

    #[allow(deprecated)]
    pub fn cancel_payment(env: Env, authority: Address, payment_id: String) -> Result<(), FluxaError> {
        let mut payment = Self::get_payment_internal(&env, &payment_id)?;

        if payment.status != PaymentStatus::Pending {
            return Err(FluxaError::PaymentAlreadyProcessed);
        }

        if env.ledger().timestamp() > payment.expires_at {
            return Err(FluxaError::Unauthorized);
        }

        authority.require_auth();
        let is_merchant = authority == payment.merchant_id;
        let is_oracle = AccessControl::has_role(&env, &role_oracle(&env), &authority);
        if !is_merchant && !is_oracle {
            return Err(FluxaError::Unauthorized);
        }

        payment.status = PaymentStatus::Failed;

        env.storage()
            .persistent()
            .set(&FluxaDataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);

        env.events().publish(
            (Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "CANCELLED")),
            payment_id,
        );

        Ok(())
    }

    #[allow(deprecated)]
    pub fn expire_payment(env: Env, payment_id: String) -> Result<(), FluxaError> {
        let mut payment = Self::get_payment_internal(&env, &payment_id)?;

        if payment.status != PaymentStatus::Pending {
            return Err(FluxaError::PaymentAlreadyProcessed);
        }

        if env.ledger().timestamp() <= payment.expires_at {
            return Err(FluxaError::Unauthorized);
        }

        payment.status = PaymentStatus::Expired;

        env.storage()
            .persistent()
            .set(&FluxaDataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);

        env.events().publish(
            (Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "EXPIRED")),
            payment_id,
        );

        Ok(())
    }

    pub fn settle_payment(
        env: Env,
        operator: Address,
        payment_id: String,
        treasury_address: Address,
    ) -> Result<(), FluxaError> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_settlement_operator(&env), &operator) {
            return Err(FluxaError::Unauthorized);
        }

        let mut payment = Self::get_payment_internal(&env, &payment_id)?;

        if payment.status != PaymentStatus::Confirmed {
            return Err(FluxaError::PaymentAlreadyProcessed); // Or another appropriate error
        }

        payment.status = PaymentStatus::Settled;
        payment.deposit_address = treasury_address; // "Sweep to treasury"

        env.storage()
            .persistent()
            .set(&FluxaDataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);

        env.events().publish(
            (Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "SETTLED")),
            payment_id,
        );

        Ok(())
    }

    fn get_payment_internal(env: &Env, payment_id: &String) -> Result<PaymentCharge, FluxaError> {
        env.storage()
            .persistent()
            .get(&FluxaDataKey::Payment(payment_id.clone()))
            .ok_or(FluxaError::PaymentNotFound)
    }

    fn get_merchant_payments_internal(env: &Env, merchant_id: &Address) -> Vec<String> {
        env.storage()
            .persistent()
            .get(&FluxaDataKey::MerchantPayments(merchant_id.clone()))
            .unwrap_or_else(|| vec![env])
    }

    fn payment_ttl(status: &PaymentStatus) -> u32 {
        match status {
            PaymentStatus::Pending => SHORT_LIVE_TTL,
            PaymentStatus::Confirmed
            | PaymentStatus::Settled
            | PaymentStatus::Expired
            | PaymentStatus::Failed => LONG_LIVE_TTL,
        }
    }

    fn bump_payment_ttl(env: &Env, payment_id: &String, status: &PaymentStatus) {
        let key = FluxaDataKey::Payment(payment_id.clone());
        Self::bump_ttl(env, &key, Self::payment_ttl(status));
    }

    fn bump_ttl(env: &Env, key: &FluxaDataKey, ttl: u32) {
        let threshold = core::cmp::max(1, ttl / TTL_BUMP_THRESHOLD_DIVISOR);
        env.storage().persistent().extend_ttl(key, threshold, ttl);
    }
}

#[cfg(test)]
mod auth_test;
#[cfg(test)]
mod dispute_test;
#[cfg(test)]
mod fx_oracle_test;
#[cfg(test)]
mod integration_test;
pub mod invoice;
pub mod merchant_registry;
#[cfg(test)]
mod merchant_registry_test;
#[cfg(test)]
mod proptests;
mod test;

pub fn format_id(env: &Env, prefix: &str, n: u64) -> String {
    let mut result = Bytes::new(env);
    for byte in prefix.as_bytes() {
        result.push_back(*byte);
    }

    let mut temp = Bytes::new(env);
    let mut num = n;
    loop {
        temp.push_back((num % 10) as u8 + 48);
        num /= 10;
        if num == 0 {
            break;
        }
    }
    let len = temp.len();
    for i in 0..len {
        result.push_back(temp.get(len - i - 1).unwrap());
    }

    let mut arr = [0u8; 64];
    let final_len = result.len().min(64);
    for i in 0..final_len {
        arr[i as usize] = result.get(i).unwrap();
    }
    String::from_bytes(env, &arr[..final_len as usize])
}
