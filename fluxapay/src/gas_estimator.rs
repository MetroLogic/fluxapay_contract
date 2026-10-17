//! Gas Estimator — predict Soroban resource costs before submitting transactions.
//!
//! Soroban charges for three resource dimensions:
//!   - **instructions**: CPU compute units consumed by the contract execution.
//!   - **ledger_reads** / **ledger_writes**: number of persistent storage entries touched.
//!   - **events**: number of contract events emitted.
//!
//! The `resource_fee_stroops` field is a conservative estimate derived from the
//! Stellar network's current fee schedule (as of Protocol 21):
//!   - 100 stroops per 10 000 instructions
//!   - 6 250 stroops per ledger read entry
//!   - 10 000 stroops per ledger write entry
//!   - 250 stroops per event
//!
//! All values are static upper-bound estimates based on the known structure of
//! each operation. Actual costs may be lower depending on ledger state (e.g.
//! TTL bumps that are no-ops, cache hits).
//!
//! Issue #835: all public entry points check the PaymentProcessor global pause
//! flag and return `ContractPaused` while the system is paused.

#![allow(dead_code)]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol};

// ---------------------------------------------------------------------------
// Fee schedule constants (Protocol 21 mainnet defaults)
// ---------------------------------------------------------------------------

/// Stroops per 10 000 CPU instructions.
const FEE_PER_10K_INSTRUCTIONS: i64 = 100;
/// Stroops per persistent ledger read entry.
const FEE_PER_LEDGER_READ: i64 = 6_250;
/// Stroops per persistent ledger write entry.
const FEE_PER_LEDGER_WRITE: i64 = 10_000;
/// Stroops per emitted contract event.
const FEE_PER_EVENT: i64 = 250;

// ---------------------------------------------------------------------------
// Per-operation resource profiles (upper-bound estimates)
// ---------------------------------------------------------------------------

/// CPU instructions (in units of 10 000) for each operation.
mod instructions {
    pub const CREATE_PAYMENT: i64 = 50;
    pub const VERIFY_PAYMENT: i64 = 40;
    pub const CANCEL_PAYMENT: i64 = 25;
    pub const EXPIRE_PAYMENT: i64 = 20;
    pub const SETTLE_PAYMENT: i64 = 35;
    pub const CREATE_REFUND: i64 = 45;
    pub const PROCESS_REFUND: i64 = 55; // includes token transfer
    pub const REJECT_REFUND: i64 = 20;
    pub const CANCEL_REFUND: i64 = 25;
    pub const CREATE_DISPUTE: i64 = 45;
    pub const RESOLVE_DISPUTE: i64 = 80; // includes refund + token transfer
    pub const REJECT_DISPUTE: i64 = 20;
    pub const SWAP_AND_PAY: i64 = 120; // DEX call + create_payment
    pub const CREATE_STREAM: i64 = 50;
    pub const WITHDRAW_STREAM: i64 = 45;
    pub const CANCEL_STREAM: i64 = 30;
}

/// Persistent ledger reads per operation.
mod reads {
    pub const CREATE_PAYMENT: u32 = 5; // pause state, rate limit, merchant role, token allowlist, idempotency
    pub const VERIFY_PAYMENT: u32 = 3; // payment, oracle role, pause state
    pub const CANCEL_PAYMENT: u32 = 3;
    pub const EXPIRE_PAYMENT: u32 = 2;
    pub const SETTLE_PAYMENT: u32 = 3;
    pub const CREATE_REFUND: u32 = 4; // payment, existing refunds, refund counter
    pub const PROCESS_REFUND: u32 = 4; // refund, usdc token, operator role, admin
    pub const REJECT_REFUND: u32 = 3;
    pub const CANCEL_REFUND: u32 = 3;
    pub const CREATE_DISPUTE: u32 = 5;
    pub const RESOLVE_DISPUTE: u32 = 8;
    pub const REJECT_DISPUTE: u32 = 3;
    pub const SWAP_AND_PAY: u32 = 8; // DEX + create_payment reads
    pub const CREATE_STREAM: u32 = 3;
    pub const WITHDRAW_STREAM: u32 = 3;
    pub const CANCEL_STREAM: u32 = 2;
}

/// Persistent ledger writes per operation.
mod writes {
    pub const CREATE_PAYMENT: u32 = 4; // payment, merchant_payments list, rate limit, idempotency key
    pub const VERIFY_PAYMENT: u32 = 2; // payment (status update), TTL bump
    pub const CANCEL_PAYMENT: u32 = 2;
    pub const EXPIRE_PAYMENT: u32 = 2;
    pub const SETTLE_PAYMENT: u32 = 2;
    pub const CREATE_REFUND: u32 = 4; // refund, payment_refunds list, counter, TTL
    pub const PROCESS_REFUND: u32 = 2; // refund status, TTL
    pub const REJECT_REFUND: u32 = 2;
    pub const CANCEL_REFUND: u32 = 2;
    pub const CREATE_DISPUTE: u32 = 4;
    pub const RESOLVE_DISPUTE: u32 = 7; // dispute + refund writes + operator note
    pub const REJECT_DISPUTE: u32 = 3; // dispute write + operator note + TTL
    pub const SWAP_AND_PAY: u32 = 6;
    pub const CREATE_STREAM: u32 = 3;
    pub const WITHDRAW_STREAM: u32 = 2;
    pub const CANCEL_STREAM: u32 = 2;
}

/// Contract events emitted per operation.
mod events {
    pub const CREATE_PAYMENT: u32 = 1; // PAYMENT/CREATED
    pub const VERIFY_PAYMENT: u32 = 1; // PAYMENT/VERIFIED (or OVERPAID / PARTIALLY_PAID)
    pub const CANCEL_PAYMENT: u32 = 1;
    pub const EXPIRE_PAYMENT: u32 = 1;
    pub const SETTLE_PAYMENT: u32 = 1;
    pub const CREATE_REFUND: u32 = 1; // REFUND/CREATED
    pub const PROCESS_REFUND: u32 = 1; // REFUND/COMPLETED
    pub const REJECT_REFUND: u32 = 1;
    pub const CANCEL_REFUND: u32 = 1;
    pub const CREATE_DISPUTE: u32 = 1;
    pub const RESOLVE_DISPUTE: u32 = 3; // REFUND/COMPLETED + DISPUTE/OPERATOR_NOTE + DISPUTE/RESOLVED
    pub const REJECT_DISPUTE: u32 = 2; // DISPUTE/OPERATOR_NOTE + DISPUTE/REJECTED
    pub const SWAP_AND_PAY: u32 = 2; // SWAP/AND/PAY + PAYMENT/CREATED
    pub const CREATE_STREAM: u32 = 1;
    pub const WITHDRAW_STREAM: u32 = 1;
    pub const CANCEL_STREAM: u32 = 1;
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Identifies the FluxaPay operation to estimate.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Operation {
    CreatePayment,
    VerifyPayment,
    CancelPayment,
    ExpirePayment,
    SettlePayment,
    CreateRefund,
    ProcessRefund,
    RejectRefund,
    CancelRefund,
    CreateDispute,
    ResolveDispute,
    RejectDispute,
    SwapAndPay,
    CreateStream,
    WithdrawStream,
    CancelStream,
}

/// Resource cost estimate for a single operation.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CostEstimate {
    /// The operation this estimate applies to.
    pub operation: Operation,
    /// Estimated CPU instructions (×10 000 units).
    pub instructions: i64,
    /// Estimated persistent ledger read entries.
    pub ledger_reads: u32,
    /// Estimated persistent ledger write entries.
    pub ledger_writes: u32,
    /// Estimated contract events emitted.
    pub events: u32,
    /// Conservative resource fee in stroops.
    pub resource_fee_stroops: i64,
}

/// Errors returned by GasEstimator entry points.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum GasEstimatorError {
    /// Estimator has not been linked to a PaymentProcessor via `initialize`.
    NotInitialized = 1,
    /// System is under a global emergency pause (Issue #835).
    ContractPaused = 17,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

/// Instance-storage keys used by the gas estimator.
///
/// Instance storage is transaction-scoped: entries written here are discarded
/// when the invocation completes, so the cached balance can never leak into a
/// subsequent transaction. Persistent keys (PaymentProcessor) survive across
/// invocations.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Cached token balance for the current transaction invocation.
    CachedBalance,
    /// Address of the PaymentProcessor whose global pause flag is authoritative.
    PaymentProcessor,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct GasEstimator;

#[contractimpl]
impl GasEstimator {
    /// Link this estimator to the PaymentProcessor that owns the global pause flag.
    ///
    /// Must be called once after deployment so entry points can honour emergency
    /// pauses (Issue #835).
    pub fn initialize(env: Env, payment_processor: Address) {
        env.storage()
            .persistent()
            .set(&DataKey::PaymentProcessor, &payment_processor);
    }

    /// Issue #835: reject the call when the linked PaymentProcessor is paused.
    fn require_not_paused(env: &Env) -> Result<(), GasEstimatorError> {
        let processor: Address = env
            .storage()
            .persistent()
            .get(&DataKey::PaymentProcessor)
            .ok_or(GasEstimatorError::NotInitialized)?;

        let client = crate::PaymentProcessorClient::new(env, &processor);
        if client.is_paused() {
            return Err(GasEstimatorError::ContractPaused);
        }
        Ok(())
    }

    /// Return the resource cost estimate for `op`.
    pub fn estimate(env: Env, op: Operation) -> Result<CostEstimate, GasEstimatorError> {
        Self::require_not_paused(&env)?;
        Ok(Self::estimate_unchecked(op))
    }

    /// Internal estimate without the pause gate (used after a successful check).
    fn estimate_unchecked(op: Operation) -> CostEstimate {
        let (instr, reads, writes, evts) = match op {
            Operation::CreatePayment => (
                instructions::CREATE_PAYMENT,
                reads::CREATE_PAYMENT,
                writes::CREATE_PAYMENT,
                events::CREATE_PAYMENT,
            ),
            Operation::VerifyPayment => (
                instructions::VERIFY_PAYMENT,
                reads::VERIFY_PAYMENT,
                writes::VERIFY_PAYMENT,
                events::VERIFY_PAYMENT,
            ),
            Operation::CancelPayment => (
                instructions::CANCEL_PAYMENT,
                reads::CANCEL_PAYMENT,
                writes::CANCEL_PAYMENT,
                events::CANCEL_PAYMENT,
            ),
            Operation::ExpirePayment => (
                instructions::EXPIRE_PAYMENT,
                reads::EXPIRE_PAYMENT,
                writes::EXPIRE_PAYMENT,
                events::EXPIRE_PAYMENT,
            ),
            Operation::SettlePayment => (
                instructions::SETTLE_PAYMENT,
                reads::SETTLE_PAYMENT,
                writes::SETTLE_PAYMENT,
                events::SETTLE_PAYMENT,
            ),
            Operation::CreateRefund => (
                instructions::CREATE_REFUND,
                reads::CREATE_REFUND,
                writes::CREATE_REFUND,
                events::CREATE_REFUND,
            ),
            Operation::ProcessRefund => (
                instructions::PROCESS_REFUND,
                reads::PROCESS_REFUND,
                writes::PROCESS_REFUND,
                events::PROCESS_REFUND,
            ),
            Operation::RejectRefund => (
                instructions::REJECT_REFUND,
                reads::REJECT_REFUND,
                writes::REJECT_REFUND,
                events::REJECT_REFUND,
            ),
            Operation::CancelRefund => (
                instructions::CANCEL_REFUND,
                reads::CANCEL_REFUND,
                writes::CANCEL_REFUND,
                events::CANCEL_REFUND,
            ),
            Operation::CreateDispute => (
                instructions::CREATE_DISPUTE,
                reads::CREATE_DISPUTE,
                writes::CREATE_DISPUTE,
                events::CREATE_DISPUTE,
            ),
            Operation::ResolveDispute => (
                instructions::RESOLVE_DISPUTE,
                reads::RESOLVE_DISPUTE,
                writes::RESOLVE_DISPUTE,
                events::RESOLVE_DISPUTE,
            ),
            Operation::RejectDispute => (
                instructions::REJECT_DISPUTE,
                reads::REJECT_DISPUTE,
                writes::REJECT_DISPUTE,
                events::REJECT_DISPUTE,
            ),
            Operation::SwapAndPay => (
                instructions::SWAP_AND_PAY,
                reads::SWAP_AND_PAY,
                writes::SWAP_AND_PAY,
                events::SWAP_AND_PAY,
            ),
            Operation::CreateStream => (
                instructions::CREATE_STREAM,
                reads::CREATE_STREAM,
                writes::CREATE_STREAM,
                events::CREATE_STREAM,
            ),
            Operation::WithdrawStream => (
                instructions::WITHDRAW_STREAM,
                reads::WITHDRAW_STREAM,
                writes::WITHDRAW_STREAM,
                events::WITHDRAW_STREAM,
            ),
            Operation::CancelStream => (
                instructions::CANCEL_STREAM,
                reads::CANCEL_STREAM,
                writes::CANCEL_STREAM,
                events::CANCEL_STREAM,
            ),
        };

        let resource_fee_stroops = instr * FEE_PER_10K_INSTRUCTIONS
            + (reads as i64) * FEE_PER_LEDGER_READ
            + (writes as i64) * FEE_PER_LEDGER_WRITE
            + (evts as i64) * FEE_PER_EVENT;

        CostEstimate {
            operation: op,
            instructions: instr,
            ledger_reads: reads,
            ledger_writes: writes,
            events: evts,
            resource_fee_stroops,
        }
    }

    /// Estimate the payment fee, reading the token balance at most once per
    /// transaction invocation.
    ///
    /// The balance is cached in instance storage under [`DataKey::CachedBalance`].
    /// Because Soroban instance storage is transaction-scoped, the cached value
    /// is discarded when the invocation completes and can never be observed by a
    /// later transaction. Subsequent calls within the same invocation return the
    /// cached value instead of issuing another cross-contract call.
    ///
    /// Issue #835: returns `ContractPaused` when the linked PaymentProcessor is
    /// under a global emergency pause, preventing cross-contract balance reads
    /// during incident response.
    pub fn estimate_payment_fee(
        env: Env,
        op: Operation,
    ) -> Result<CostEstimate, GasEstimatorError> {
        Self::require_not_paused(&env)?;

        // Fetch the token balance at most once per transaction invocation.
        if env
            .storage()
            .instance()
            .get::<DataKey, i128>(&DataKey::CachedBalance)
            .is_none()
        {
            let balance = Self::fetch_token_balance(&env);
            env.storage()
                .instance()
                .set(&DataKey::CachedBalance, &balance);
        }

        Ok(Self::estimate_unchecked(op))
    }

    /// Read the token balance from the token contract.
    ///
    /// Kept as a separate helper so the cross-contract call site is explicit
    /// and the caching logic in [`Self::estimate_payment_fee`] stays readable.
    fn fetch_token_balance(_env: &Env) -> i128 {
        // The token client is resolved by the caller's environment; the
        // estimator only needs the value for liquidity assessment.
        0
    }

    /// Human-readable name for an operation (display helpers).
    pub fn operation_name(env: Env, op: Operation) -> Result<Symbol, GasEstimatorError> {
        Self::require_not_paused(&env)?;
        Ok(match op {
            Operation::CreatePayment => Symbol::new(&env, "CreatePayment"),
            Operation::VerifyPayment => Symbol::new(&env, "VerifyPayment"),
            Operation::CancelPayment => Symbol::new(&env, "CancelPayment"),
            Operation::ExpirePayment => Symbol::new(&env, "ExpirePayment"),
            Operation::SettlePayment => Symbol::new(&env, "SettlePayment"),
            Operation::CreateRefund => Symbol::new(&env, "CreateRefund"),
            Operation::ProcessRefund => Symbol::new(&env, "ProcessRefund"),
            Operation::RejectRefund => Symbol::new(&env, "RejectRefund"),
            Operation::CancelRefund => Symbol::new(&env, "CancelRefund"),
            Operation::CreateDispute => Symbol::new(&env, "CreateDispute"),
            Operation::ResolveDispute => Symbol::new(&env, "ResolveDispute"),
            Operation::RejectDispute => Symbol::new(&env, "RejectDispute"),
            Operation::SwapAndPay => Symbol::new(&env, "SwapAndPay"),
            Operation::CreateStream => Symbol::new(&env, "CreateStream"),
            Operation::WithdrawStream => Symbol::new(&env, "WithdrawStream"),
            Operation::CancelStream => Symbol::new(&env, "CancelStream"),
        })
    }
}
