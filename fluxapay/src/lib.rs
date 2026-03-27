#![no_std]
use soroban_sdk::{
    bytes, contract, contracterror, contractimpl, contracttype, token, vec, Address, Bytes,
    BytesN, Env, MuxedAddress, String, Symbol, Vec,
};

mod access_control;
pub mod fx_oracle;
use access_control::{role_oracle, role_settlement_operator, AccessControl};
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
    DisputeNotFound = 11,
    DisputeAlreadyResolved = 12,
}

#[contracttype]
pub enum DataKey {
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
    /// Initializes the Refund Manager with an administrator and the USDC token address.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `admin`: The address to be set as the administrator.
    /// - `usdc_token_address`: The address of the USDC token contract.
    pub fn initialize_refund_manager(env: Env, admin: Address, usdc_token_address: Address) {
        AccessControl::initialize(&env, admin);
        env.storage()
            .persistent()
            .set(&DataKey::UsdcToken, &usdc_token_address);
    }

    /// Grants a specific role to an account.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `admin`: The administrative address authorizing the grant.
    /// - `role`: The symbol of the role to grant.
    /// - `account`: The address to receive the role.
    ///
    /// ### Errors
    /// - `Error::AccessControlError`: If the underlying access control operation fails.
    pub fn grant_role(
        env: Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), Error> {
        AccessControl::grant_role(&env, admin, role, account).map_err(|_| Error::AccessControlError)
    }

    /// Revokes a specific role from an account.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `admin`: The administrative address authorizing the revocation.
    /// - `role`: The symbol of the role to revoke.
    /// - `account`: The address to lose the role.
    ///
    /// ### Errors
    /// - `Error::AccessControlError`: If the underlying access control operation fails.
    pub fn revoke_role(
        env: Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), Error> {
        AccessControl::revoke_role(&env, admin, role, account)
            .map_err(|_| Error::AccessControlError)
    }

    /// Checks if an account has a specific role.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `role`: The symbol of the role to check.
    /// - `account`: The address to check for the role.
    ///
    /// ### Returns
    /// - `bool`: True if the account has the role, false otherwise.
    pub fn has_role(env: Env, role: Symbol, account: Address) -> bool {
        AccessControl::has_role(&env, &role, &account)
    }

    /// Allows an account to renounce a role they currently hold.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `account`: The address renouncing the role.
    /// - `role`: The symbol of the role to renounce.
    ///
    /// ### Errors
    /// - `Error::AccessControlError`: If the underlying access control operation fails.
    pub fn renounce_role(env: Env, account: Address, role: Symbol) -> Result<(), Error> {
        AccessControl::renounce_role(&env, account, role).map_err(|_| Error::AccessControlError)
    }

    /// Transfers the administrator role to a new address.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `current_admin`: The current administrative address.
    /// - `new_admin`: The new address to become the administrator.
    ///
    /// ### Errors
    /// - `Error::AccessControlError`: If the underlying access control operation fails.
    pub fn transfer_admin(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), Error> {
        AccessControl::transfer_admin(&env, current_admin, new_admin)
            .map_err(|_| Error::AccessControlError)
    }

    /// Returns the address of the current administrator.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    ///
    /// ### Returns
    /// - `Option<Address>`: The administrator address if it has been set.
    pub fn get_admin(env: Env) -> Option<Address> {
        AccessControl::get_admin(&env)
    }

    /// Creates a new refund request for a given payment.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `payment_id`: The ID of the payment to be refunded.
    /// - `refund_amount`: The amount to be refunded.
    /// - `reason`: The reason for the refund.
    /// - `requester`: The address requesting the refund.
    ///
    /// ### Authorization
    /// - Requires `requester` to provide authentication.
    ///
    /// ### Errors
    /// - `Error::InvalidAmount`: If the refund amount is less than or equal to zero.
    pub fn create_refund(
        env: Env,
        payment_id: String,
        refund_amount: i128,
        reason: String,
        requester: Address,
    ) -> Result<String, Error> {
        requester.require_auth();
        Self::create_refund_internal(&env, payment_id, refund_amount, reason, requester)
    }

    fn create_refund_internal(
        env: &Env,
        payment_id: String,
        refund_amount: i128,
        reason: String,
        requester: Address,
    ) -> Result<String, Error> {
        if refund_amount <= 0 {
            return Err(Error::InvalidAmount);
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
            .set(&DataKey::Refund(refund_id.clone()), &refund);

        let mut payment_refunds = Self::get_payment_refunds_internal(env, &payment_id);
        payment_refunds.push_back(refund_id.clone());
        env.storage()
            .persistent()
            .set(&DataKey::PaymentRefunds(payment_id), &payment_refunds);

        Ok(refund_id)
    }

    /// Processes a pending refund, transferring funds to the requester.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `operator`: The address of the operator processing the refund.
    /// - `refund_id`: The ID of the refund to process.
    ///
    /// ### Authorization
    /// - Requires `operator` to provide authentication.
    /// - `operator` must have either the `SETTLEMENT_OPERATOR` or `ORACLE` role.
    ///
    /// ### Errors
    /// - `Error::Unauthorized`: If the operator does not have the required role.
    /// - `Error::RefundNotFound`: If the refund ID does not exist.
    /// - `Error::RefundAlreadyProcessed`: If the refund has already been completed or rejected.
    pub fn process_refund(env: Env, operator: Address, refund_id: String) -> Result<(), Error> {
        operator.require_auth();
        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);

        if !has_settlement && !has_oracle {
            return Err(Error::Unauthorized);
        }

        Self::process_refund_internal(&env, &operator, refund_id)
    }

    fn process_refund_internal(
        env: &Env,
        _operator: &Address,
        refund_id: String,
    ) -> Result<(), Error> {
        let mut refund = Self::get_refund_internal(env, &refund_id)?;

        if refund.status != RefundStatus::Pending {
            return Err(Error::RefundAlreadyProcessed);
        }

        let usdc_token_address: Address = env
            .storage()
            .persistent()
            .get(&DataKey::UsdcToken)
            .ok_or(Error::Unauthorized)?;
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
            .set(&DataKey::Refund(refund_id), &refund);

        Ok(())
    }

    /// Retrieves the details of a specific refund.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `refund_id`: The ID of the refund to retrieve.
    ///
    /// ### Returns
    /// - `Result<Refund, Error>`: The refund data or an error if not found.
    pub fn get_refund(env: Env, refund_id: String) -> Result<Refund, Error> {
        Self::get_refund_internal(&env, &refund_id)
    }

    /// Retrieves all refunds associated with a specific payment.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `payment_id`: The ID of the payment.
    ///
    /// ### Returns
    /// - `Result<Vec<Refund>, Error>`: A vector of refunds or an error.
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

    // Dispute handling functions
    /// Creates a new dispute for a given payment.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `payment_id`: The ID of the payment being disputed.
    /// - `amount`: The amount in dispute.
    /// - `reason`: The reason for the dispute.
    /// - `evidence`: A string containing evidence or a reference to it.
    /// - `disputer`: The address initiating the dispute.
    ///
    /// ### Authorization
    /// - Requires `disputer` to provide authentication.
    ///
    /// ### Errors
    /// - `Error::InvalidAmount`: If the disputed amount is less than or equal to zero.
    pub fn create_dispute(
        env: Env,
        payment_id: String,
        amount: i128,
        reason: String,
        evidence: String,
        disputer: Address,
    ) -> Result<String, Error> {
        disputer.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
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
            .set(&DataKey::Dispute(dispute_id.clone()), &dispute);

        let mut payment_disputes = Self::get_payment_disputes_internal(&env, &payment_id);
        payment_disputes.push_back(dispute_id.clone());
        env.storage()
            .persistent()
            .set(&DataKey::PaymentDisputes(payment_id), &payment_disputes);

        Ok(dispute_id)
    }

    /// Moves a dispute into the "Under Review" status.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `operator`: The operator address reviewing the dispute.
    /// - `dispute_id`: The ID of the dispute to review.
    ///
    /// ### Authorization
    /// - Requires `operator` to provide authentication.
    /// - `operator` must have either the `SETTLEMENT_OPERATOR` or `ORACLE` role.
    ///
    /// ### Errors
    /// - `Error::Unauthorized`: If the caller is not authorized.
    /// - `Error::DisputeNotFound`: If the dispute does not exist.
    /// - `Error::DisputeAlreadyResolved`: If the dispute is already resolved or rejected.
    pub fn review_dispute(env: Env, operator: Address, dispute_id: String) -> Result<(), Error> {
        operator.require_auth();

        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);

        if !has_settlement && !has_oracle {
            return Err(Error::Unauthorized);
        }

        let mut dispute = Self::get_dispute_internal(&env, &dispute_id)?;

        if dispute.status != DisputeStatus::Open {
            return Err(Error::DisputeAlreadyResolved);
        }

        dispute.status = DisputeStatus::UnderReview;

        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id), &dispute);

        Ok(())
    }

    /// Resolves a dispute by issuing a refund to the disputer.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `operator`: The operator address resolving the dispute.
    /// - `dispute_id`: The ID of the dispute to resolve.
    /// - `resolution_notes`: Notes explaining the resolution.
    ///
    /// ### Authorization
    /// - Requires `operator` to provide authentication.
    /// - `operator` must have either the `SETTLEMENT_OPERATOR` or `ORACLE` role.
    ///
    /// ### Errors
    /// - `Error::Unauthorized`: If the caller is not authorized.
    /// - `Error::DisputeNotFound`: If the dispute does not exist.
    /// - `Error::DisputeAlreadyResolved`: If the dispute is already resolved.
    pub fn resolve_dispute_with_refund(
        env: Env,
        operator: Address,
        dispute_id: String,
        resolution_notes: String,
    ) -> Result<String, Error> {
        operator.require_auth();

        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);

        if !has_settlement && !has_oracle {
            return Err(Error::Unauthorized);
        }

        let mut dispute = Self::get_dispute_internal(&env, &dispute_id)?;

        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Err(Error::DisputeAlreadyResolved);
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
            .set(&DataKey::Dispute(dispute_id), &dispute);

        Ok(refund_id)
    }

    /// Rejects a dispute without issuing a refund.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `operator`: The operator address rejecting the dispute.
    /// - `dispute_id`: The ID of the dispute to reject.
    /// - `resolution_notes`: Notes explaining the rejection.
    ///
    /// ### Authorization
    /// - Requires `operator` to provide authentication.
    /// - `operator` must have either the `SETTLEMENT_OPERATOR` or `ORACLE` role.
    ///
    /// ### Errors
    /// - `Error::Unauthorized`: If the caller is not authorized.
    /// - `Error::DisputeNotFound`: If the dispute does not exist.
    /// - `Error::DisputeAlreadyResolved`: If the dispute is already resolved or rejected.
    pub fn reject_dispute(
        env: Env,
        operator: Address,
        dispute_id: String,
        resolution_notes: String,
    ) -> Result<(), Error> {
        operator.require_auth();

        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);

        if !has_settlement && !has_oracle {
            return Err(Error::Unauthorized);
        }

        let mut dispute = Self::get_dispute_internal(&env, &dispute_id)?;

        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Err(Error::DisputeAlreadyResolved);
        }

        dispute.status = DisputeStatus::Rejected;
        dispute.resolved_at = Some(env.ledger().timestamp());
        dispute.resolution_notes = Some(resolution_notes);

        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id), &dispute);

        Ok(())
    }

    /// Retrieves the details of a specific dispute.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `dispute_id`: The ID of the dispute to retrieve.
    ///
    /// ### Returns
    /// - `Result<Dispute, Error>`: The dispute data or an error if not found.
    pub fn get_dispute(env: Env, dispute_id: String) -> Result<Dispute, Error> {
        Self::get_dispute_internal(&env, &dispute_id)
    }

    /// Retrieves all disputes associated with a specific payment.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `payment_id`: The ID of the payment.
    ///
    /// ### Returns
    /// - `Result<Vec<Dispute>, Error>`: A vector of disputes or an error.
    pub fn get_payment_disputes(env: Env, payment_id: String) -> Result<Vec<Dispute>, Error> {
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
            .get(&DataKey::DisputeCounter)
            .unwrap_or(0);
        counter += 1;
        env.storage()
            .persistent()
            .set(&DataKey::DisputeCounter, &counter);
        counter
    }

    fn build_dispute_id(env: &Env, counter: u64) -> String {
        format_id(env, "dispute_", counter)
    }

    fn get_dispute_internal(env: &Env, dispute_id: &String) -> Result<Dispute, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Dispute(dispute_id.clone()))
            .ok_or(Error::DisputeNotFound)
    }

    fn get_payment_disputes_internal(env: &Env, payment_id: &String) -> Vec<String> {
        env.storage()
            .persistent()
            .get(&DataKey::PaymentDisputes(payment_id.clone()))
            .unwrap_or_else(|| vec![env])
    }
}

#[contractimpl]
impl PaymentProcessor {
    /// Initializes the Payment Processor with an administrator.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `admin`: The address to be set as the administrator.
    pub fn initialize_payment_processor(env: Env, admin: Address) {
        AccessControl::initialize(&env, admin);
    }

    /// Grants a specific role to an account.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `admin`: The administrative address authorizing the grant.
    /// - `role`: The symbol of the role to grant.
    /// - `account`: The address to receive the role.
    ///
    /// ### Errors
    /// - `Error::AccessControlError`: If the underlying access control operation fails.
    pub fn grant_role(
        env: Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), Error> {
        AccessControl::grant_role(&env, admin, role, account).map_err(|_| Error::AccessControlError)
    }

    /// Creates a new payment charge.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `payment_id`: A unique identifier for the payment.
    /// - `merchant_id`: The address of the merchant creating the payment.
    /// - `amount`: The amount to be paid.
    /// - `currency`: The symbol of the currency for the payment.
    /// - `deposit_address`: The address where funds should be deposited.
    /// - `expires_at`: The ledger timestamp when the payment expires.
    ///
    /// ### Returns
    /// - `Result<PaymentCharge, Error>`: The created payment charge or an error.
    ///
    /// ### Authorization
    /// - Requires `merchant_id` to provide authentication.
    ///
    /// ### Errors
    /// - `Error::InvalidAmount`: If the amount is less than or equal to zero.
    /// - `Error::PaymentAlreadyExists`: If the payment ID is already in use.
    /// - `Error::InvalidPaymentId`: If the payment ID is empty.
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
            .set(&DataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);

        let mut merchant_payments = Self::get_merchant_payments_internal(&env, &merchant_id);
        merchant_payments.push_back(payment_id.clone());
        let merchant_payments_key = DataKey::MerchantPayments(merchant_id);
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

    /// Verifies that a payment has been made on-chain.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `oracle`: The address of the oracle or operator verifying the payment.
    /// - `payment_id`: The ID of the payment to verify.
    /// - `transaction_hash`: The hash of the transaction that fulfilled the payment.
    /// - `payer_address`: The address of the payer.
    /// - `amount_received`: The actual amount received.
    ///
    /// ### Returns
    /// - `Result<PaymentStatus, Error>`: The new status of the payment or an error.
    ///
    /// ### Authorization
    /// - Requires `oracle` to provide authentication.
    /// - `oracle` must have either the `ORACLE` or `SETTLEMENT_OPERATOR` role.
    ///
    /// ### Errors
    /// - `Error::Unauthorized`: If the caller is not authorized.
    /// - `Error::PaymentNotFound`: If the payment ID does not exist.
    /// - `Error::PaymentAlreadyProcessed`: If the payment is no longer in `Pending` status.
    /// - `Error::PaymentExpired`: If the payment has already expired.
    #[allow(deprecated)]
    pub fn verify_payment(
        env: Env,
        oracle: Address,
        payment_id: String,
        transaction_hash: BytesN<32>,
        payer_address: Address,
        amount_received: i128,
    ) -> Result<PaymentStatus, Error> {
        oracle.require_auth();

        if !AccessControl::has_role(&env, &role_oracle(&env), &oracle)
            && !AccessControl::has_role(&env, &role_settlement_operator(&env), &oracle)
        {
            return Err(Error::Unauthorized);
        }

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
            .set(&DataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);

        env.events().publish(
            (Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "VERIFIED")),
            payment_id,
        );

        Ok(PaymentStatus::Confirmed)
    }

    /// Retrieves the details of a specific payment.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `payment_id`: The ID of the payment to retrieve.
    ///
    /// ### Returns
    /// - `Result<PaymentCharge, Error>`: The payment data or an error if not found.
    pub fn get_payment(env: Env, payment_id: String) -> Result<PaymentCharge, Error> {
        Self::get_payment_internal(&env, &payment_id)
    }

    /// Retrieves all payment IDs associated with a specific merchant.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `merchant_id`: The address of the merchant.
    ///
    /// ### Returns
    /// - `Vec<String>`: A vector of payment IDs.
    pub fn get_merchant_payments(env: Env, merchant_id: Address) -> Vec<String> {
        Self::get_merchant_payments_internal(&env, &merchant_id)
    }

    /// Retrieves payment IDs for a merchant with pagination.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `merchant_id`: The address of the merchant.
    /// - `offset`: The starting index for pagination.
    /// - `limit`: The maximum number of IDs to return.
    ///
    /// ### Returns
    /// - `Vec<String>`: A vector of payment IDs for the requested page.
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

    /// Cancels a pending payment.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `authority`: The merchant address or an oracle address authorizing the cancellation.
    /// - `payment_id`: The ID of the payment to cancel.
    ///
    /// ### Authorization
    /// - Requires `authority` to provide authentication.
    /// - `authority` must be either the merchant who created the payment or have the `ORACLE` role.
    ///
    /// ### Errors
    /// - `Error::Unauthorized`: If the caller is not authorized.
    /// - `Error::PaymentNotFound`: If the payment does not exist.
    /// - `Error::PaymentAlreadyProcessed`: If the payment is not in `Pending` status.
    #[allow(deprecated)]
    pub fn cancel_payment(env: Env, authority: Address, payment_id: String) -> Result<(), Error> {
        let mut payment = Self::get_payment_internal(&env, &payment_id)?;

        if payment.status != PaymentStatus::Pending {
            return Err(Error::PaymentAlreadyProcessed);
        }

        if env.ledger().timestamp() > payment.expires_at {
            return Err(Error::Unauthorized);
        }

        authority.require_auth();
        let is_merchant = authority == payment.merchant_id;
        let is_oracle = AccessControl::has_role(&env, &role_oracle(&env), &authority);
        if !is_merchant && !is_oracle {
            return Err(Error::Unauthorized);
        }

        payment.status = PaymentStatus::Failed;

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);

        env.events().publish(
            (Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "CANCELLED")),
            payment_id,
        );

        Ok(())
    }

    /// Marks a pending payment as expired if its expiration time has passed.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `payment_id`: The ID of the payment to expire.
    ///
    /// ### Errors
    /// - `Error::Unauthorized`: If the payment has not yet reached its expiration time.
    /// - `Error::PaymentNotFound`: If the payment does not exist.
    /// - `Error::PaymentAlreadyProcessed`: If the payment is not in `Pending` status.
    #[allow(deprecated)]
    pub fn expire_payment(env: Env, payment_id: String) -> Result<(), Error> {
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
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);

        env.events().publish(
            (Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "EXPIRED")),
            payment_id,
        );

        Ok(())
    }

    /// Settles a confirmed payment, sweeping funds to a treasury address.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `operator`: The address of the operator settling the payment.
    /// - `payment_id`: The ID of the payment to settle.
    /// - `treasury_address`: The address where funds should be swept.
    ///
    /// ### Authorization
    /// - Requires `operator` to provide authentication.
    /// - `operator` must have the `SETTLEMENT_OPERATOR` role.
    ///
    /// ### Errors
    /// - `Error::Unauthorized`: If the caller is not authorized.
    /// - `Error::PaymentNotFound`: If the payment does not exist.
    /// - `Error::PaymentAlreadyProcessed`: If the payment is not in `Confirmed` status.
    pub fn settle_payment(
        env: Env,
        operator: Address,
        payment_id: String,
        treasury_address: Address,
    ) -> Result<(), Error> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_settlement_operator(&env), &operator) {
            return Err(Error::Unauthorized);
        }

        let mut payment = Self::get_payment_internal(&env, &payment_id)?;

        if payment.status != PaymentStatus::Confirmed {
            return Err(Error::PaymentAlreadyProcessed); // Or another appropriate error
        }

        payment.status = PaymentStatus::Settled;
        payment.deposit_address = treasury_address; // "Sweep to treasury"

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);

        env.events().publish(
            (Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "SETTLED")),
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

    fn get_merchant_payments_internal(env: &Env, merchant_id: &Address) -> Vec<String> {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantPayments(merchant_id.clone()))
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
        let key = DataKey::Payment(payment_id.clone());
        Self::bump_ttl(env, &key, Self::payment_ttl(status));
    }

    fn bump_ttl(env: &Env, key: &DataKey, ttl: u32) {
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
pub mod merchant_registry;
#[cfg(test)]
mod merchant_registry_test;
#[cfg(test)]
mod proptests;
mod test;

/// Formats a unique ID from a prefix and a counter.
///
/// ### Parameters
/// - `env`: The Soroban environment.
/// - `prefix`: The string prefix for the ID (e.g., "refund_").
/// - `n`: The counter value.
///
/// ### Returns
/// - `String`: The formatted ID string.
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
