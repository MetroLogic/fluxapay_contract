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

#![allow(dead_code)]

use soroban_sdk::{contract, contractimpl, contracttype, Symbol};

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

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct GasEstimator;

#[contractimpl]
impl GasEstimator {
    /// Return the resource cost estimate for `op`.
    pub fn estimate(_env: soroban_sdk::Env, op: Operation) -> CostEstimate {
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

        let fee = instr * FEE_PER_10K_INSTRUCTIONS
            + reads as i64 * FEE_PER_LEDGER_READ
            + writes as i64 * FEE_PER_LEDGER_WRITE
            + evts as i64 * FEE_PER_EVENT;

        CostEstimate {
            operation: op,
            instructions: instr,
            ledger_reads: reads,
            ledger_writes: writes,
            events: evts,
            resource_fee_stroops: fee,
        }
    }

    /// Return estimates for all operations at once.
    pub fn estimate_all(env: soroban_sdk::Env) -> soroban_sdk::Vec<CostEstimate> {
        let ops = [
            Operation::CreatePayment,
            Operation::VerifyPayment,
            Operation::CancelPayment,
            Operation::ExpirePayment,
            Operation::SettlePayment,
            Operation::CreateRefund,
            Operation::ProcessRefund,
            Operation::RejectRefund,
            Operation::CancelRefund,
            Operation::CreateDispute,
            Operation::ResolveDispute,
            Operation::RejectDispute,
            Operation::SwapAndPay,
            Operation::CreateStream,
            Operation::WithdrawStream,
            Operation::CancelStream,
        ];

        let mut out = soroban_sdk::vec![&env];
        for op in ops {
            out.push_back(Self::estimate(env.clone(), op));
        }
        out
    }

    /// Return the name of the operation as a Symbol (useful for off-chain display).
    pub fn operation_name(env: soroban_sdk::Env, op: Operation) -> Symbol {
        match op {
            Operation::CreatePayment => Symbol::new(&env, "create_payment"),
            Operation::VerifyPayment => Symbol::new(&env, "verify_payment"),
            Operation::CancelPayment => Symbol::new(&env, "cancel_payment"),
            Operation::ExpirePayment => Symbol::new(&env, "expire_payment"),
            Operation::SettlePayment => Symbol::new(&env, "settle_payment"),
            Operation::CreateRefund => Symbol::new(&env, "create_refund"),
            Operation::ProcessRefund => Symbol::new(&env, "process_refund"),
            Operation::RejectRefund => Symbol::new(&env, "reject_refund"),
            Operation::CancelRefund => Symbol::new(&env, "cancel_refund"),
            Operation::CreateDispute => Symbol::new(&env, "create_dispute"),
            Operation::ResolveDispute => Symbol::new(&env, "resolve_dispute"),
            Operation::RejectDispute => Symbol::new(&env, "reject_dispute"),
            Operation::SwapAndPay => Symbol::new(&env, "swap_and_pay"),
            Operation::CreateStream => Symbol::new(&env, "create_stream"),
            Operation::WithdrawStream => Symbol::new(&env, "withdraw_stream"),
            Operation::CancelStream => Symbol::new(&env, "cancel_stream"),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::Env;

    fn client(env: &Env) -> GasEstimatorClient<'_> {
        let id = env.register(GasEstimator, ());
        GasEstimatorClient::new(env, &id)
    }

    #[test]
    fn estimate_returns_correct_fields() {
        let env = Env::default();
        let c = client(&env);

        let est = c.estimate(&Operation::CreatePayment);

        assert_eq!(est.operation, Operation::CreatePayment);
        assert_eq!(est.instructions, instructions::CREATE_PAYMENT);
        assert_eq!(est.ledger_reads, reads::CREATE_PAYMENT);
        assert_eq!(est.ledger_writes, writes::CREATE_PAYMENT);
        assert_eq!(est.events, events::CREATE_PAYMENT);

        let expected_fee = instructions::CREATE_PAYMENT * FEE_PER_10K_INSTRUCTIONS
            + reads::CREATE_PAYMENT as i64 * FEE_PER_LEDGER_READ
            + writes::CREATE_PAYMENT as i64 * FEE_PER_LEDGER_WRITE
            + events::CREATE_PAYMENT as i64 * FEE_PER_EVENT;
        assert_eq!(est.resource_fee_stroops, expected_fee);
    }

    #[test]
    fn estimate_all_returns_all_operations() {
        let env = Env::default();
        let c = client(&env);

        let all = c.estimate_all();
        assert_eq!(all.len(), 16);
    }

    #[test]
    fn swap_and_pay_is_most_expensive() {
        let env = Env::default();
        let c = client(&env);

        let swap = c.estimate(&Operation::SwapAndPay);
        let create = c.estimate(&Operation::CreatePayment);
        assert!(swap.resource_fee_stroops > create.resource_fee_stroops);
    }

    #[test]
    fn resolve_dispute_higher_than_reject_dispute() {
        let env = Env::default();
        let c = client(&env);

        let resolve = c.estimate(&Operation::ResolveDispute);
        let reject = c.estimate(&Operation::RejectDispute);
        assert!(resolve.resource_fee_stroops > reject.resource_fee_stroops);
    }

    #[test]
    fn operation_name_matches() {
        let env = Env::default();
        let c = client(&env);

        assert_eq!(
            c.operation_name(&Operation::CreatePayment),
            soroban_sdk::Symbol::new(&env, "create_payment")
        );
        assert_eq!(
            c.operation_name(&Operation::SwapAndPay),
            soroban_sdk::Symbol::new(&env, "swap_and_pay")
        );
    }

    // ── Issue #588: expanded gas estimator test coverage ──────────────────────

    #[test]
    fn verify_payment_estimate_instructions_nonzero() {
        let env = Env::default();
        let c = client(&env);

        let est = c.estimate(&Operation::VerifyPayment);
        assert!(
            est.instructions > 0,
            "verify_payment instructions must be > 0, got {}",
            est.instructions
        );
        assert!(est.ledger_reads > 0, "verify_payment ledger_reads must be > 0");
        assert!(est.ledger_writes > 0, "verify_payment ledger_writes must be > 0");
    }

    #[test]
    fn all_operations_have_positive_resource_fee() {
        let env = Env::default();
        let c = client(&env);

        let all = c.estimate_all();
        for est in all.iter() {
            assert!(
                est.resource_fee_stroops > 0,
                "Operation {:?} has non-positive resource_fee_stroops: {}",
                est.operation,
                est.resource_fee_stroops
            );
        }
    }

    #[test]
    fn estimate_fields_are_consistent_for_all_operations() {
        let env = Env::default();
        let c = client(&env);

        let all = c.estimate_all();
        for est in all.iter() {
            let expected_fee = est.instructions * FEE_PER_10K_INSTRUCTIONS
                + est.ledger_reads as i64 * FEE_PER_LEDGER_READ
                + est.ledger_writes as i64 * FEE_PER_LEDGER_WRITE
                + est.events as i64 * FEE_PER_EVENT;
            assert_eq!(
                est.resource_fee_stroops, expected_fee,
                "Operation {:?}: resource_fee_stroops ({}) != computed sum ({})",
                est.operation, est.resource_fee_stroops, expected_fee
            );
        }
    }
}
