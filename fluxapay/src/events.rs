//! Contract events using the modern `#[contractevent]` pattern.
//!
//! This module defines typed event structs that replace the deprecated
//! `env.events().publish()` pattern with better ABI introspection,
//! indexer compatibility, and compile-time topic verification.
//!
//! # Migration Notes
//!
//! All events previously emitted via:
//! ```ignore
//! env.events().publish((Symbol::new(&env, "TOPIC1"), Symbol::new(&env, "TOPIC2")), data);
//! ```
//!
//! Should be migrated to:
//! ```ignore
//! env.events().publish_event(&EventStruct { ... });
//! ```

use soroban_sdk::{contractevent, Address, BytesN, Env, String, Symbol, Vec};

// ============================================================================
// Payment Events
// ============================================================================

/// Emitted when a new payment charge is created.
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentCreated {
    pub payment_id: String,
    pub merchant_id: Address,
    pub amount: i128,
}

/// Emitted when a payment is verified (amount received within tolerance).
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentVerified {
    pub payment_id: String,
    pub merchant_id: Address,
    pub amount: i128,
    pub amount_received: i128,
}

/// Emitted when a payment is partially paid (amount below tolerance).
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentPartiallyPaid {
    pub payment_id: String,
    pub merchant_id: Address,
    pub amount: i128,
    pub amount_received: i128,
}

/// Emitted when a payment is overpaid (amount above tolerance).
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentOverpaid {
    pub payment_id: String,
    pub merchant_id: Address,
    pub amount: i128,
    pub amount_received: i128,
}

/// Emitted when a payment verification fails.
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentFailed {
    pub payment_id: String,
    pub merchant_id: Address,
    pub amount: i128,
    pub amount_received: i128,
}

/// Emitted when a payment is cancelled.
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentCancelled {
    pub payment_id: String,
    pub merchant_id: Address,
    pub amount: i128,
}

/// Emitted when a payment expires.
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentExpired {
    pub payment_id: String,
    pub merchant_id: Address,
    pub amount: i128,
}

/// Emitted when a payment is settled.
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentSettled {
    pub payment_id: String,
    pub merchant_id: Address,
    pub amount: i128,
}

/// Emitted when a batch of payments is created.
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentBatchCreated {
    pub merchant_id: Address,
    pub count: u32,
}

/// Emitted when a fee is collected from a payment.
#[contractevent]
#[derive(Clone, Debug)]
pub struct FeeCollected {
    pub payment_id: String,
    pub merchant_id: Address,
    pub fee_amount: i128,
}

/// Emitted when KYC tier is upgraded.
#[contractevent]
#[derive(Clone, Debug)]
pub struct KycTierUpgraded {
    pub merchant_id: Address,
    pub old_tier: String,
    pub new_tier: String,
}

// ============================================================================
// Refund Events
// ============================================================================

/// Emitted when a refund request is created.
#[contractevent]
#[derive(Clone, Debug)]
pub struct RefundCreated {
    pub refund_id: String,
    pub payment_id: String,
    pub amount: i128,
}

/// Emitted when a refund is completed.
#[contractevent]
#[derive(Clone, Debug)]
pub struct RefundCompleted {
    pub refund_id: String,
    pub payment_id: String,
    pub amount: i128,
}

/// Emitted when a refund is rejected.
#[contractevent]
#[derive(Clone, Debug)]
pub struct RefundRejected {
    pub refund_id: String,
    pub payment_id: String,
    pub amount: i128,
}

/// Emitted when a refund is approved.
#[contractevent]
#[derive(Clone, Debug)]
pub struct RefundApproved {
    pub refund_id: String,
    pub payment_id: String,
    pub amount: i128,
}

/// Emitted when a refund is cancelled.
#[contractevent]
#[derive(Clone, Debug)]
pub struct RefundCancelled {
    pub refund_id: String,
    pub payment_id: String,
    pub amount: i128,
}

// ============================================================================
// Dispute Events
// ============================================================================

/// Emitted when a dispute is created.
#[contractevent]
#[derive(Clone, Debug)]
pub struct DisputeCreated {
    pub dispute_id: String,
    pub payment_id: String,
}

/// Emitted when a dispute is reviewed.
#[contractevent]
#[derive(Clone, Debug)]
pub struct DisputeReviewed {
    pub dispute_id: String,
    pub payment_id: String,
}

/// Emitted when a dispute is resolved.
#[contractevent]
#[derive(Clone, Debug)]
pub struct DisputeResolved {
    pub dispute_id: String,
    pub payment_id: String,
}

/// Emitted when a dispute is rejected.
#[contractevent]
#[derive(Clone, Debug)]
pub struct DisputeRejected {
    pub dispute_id: String,
    pub payment_id: String,
}

/// Emitted when a dispute is escalated.
#[contractevent]
#[derive(Clone, Debug)]
pub struct DisputeEscalated {
    pub dispute_id: String,
    pub payment_id: String,
}

/// Emitted when a dispute stake is locked.
#[contractevent]
#[derive(Clone, Debug)]
pub struct DisputeStakeLocked {
    pub dispute_id: String,
    pub arbitrator: Address,
    pub amount: i128,
}

/// Emitted when an arbitrator vote is cast.
#[contractevent]
#[derive(Clone, Debug)]
pub struct DisputeVoteCast {
    pub dispute_id: String,
    pub arbitrator: Address,
    pub vote: String,
}

/// Emitted when an arbitrator votes on a dispute.
#[contractevent]
#[derive(Clone, Debug)]
pub struct ArbitratorVote {
    pub dispute_id: String,
    pub arbitrator: Address,
    pub vote: String,
}

/// Emitted when a dispute bond is collected.
#[contractevent]
#[derive(Clone, Debug)]
pub struct DisputeBondCollected {
    pub dispute_id: String,
    pub disputer: Address,
    pub amount: i128,
}

/// Emitted when a dispute bond is returned.
#[contractevent]
#[derive(Clone, Debug)]
pub struct DisputeBondReturned {
    pub dispute_id: String,
    pub disputer: Address,
    pub amount: i128,
}

/// Emitted when a dispute bond is forfeited.
#[contractevent]
#[derive(Clone, Debug)]
pub struct DisputeBondForfeited {
    pub dispute_id: String,
    pub disputer: Address,
    pub recipient: Address,
    pub amount: i128,
}

/// Issue #677: Emit a `DISPUTE/BOND_RETURNED` event when a dispute bond is
/// released back to `recipient` after a dispute is resolved in their favor.
pub fn emit_dispute_bond_returned(env: &Env, dispute_id: String, recipient: Address, amount: i128) {
    env.events().publish(
        (Symbol::new(env, "DISPUTE"), Symbol::new(env, "BOND_RETURNED")),
        (dispute_id, recipient, amount),
    );
}

/// Issue #677: Emit a `DISPUTE/BOND_FORFEITED` event when a dispute bond is
/// forfeited to `recipient` (the treasury/collector) after a dispute is
/// rejected.
pub fn emit_dispute_bond_forfeited(
    env: &Env,
    dispute_id: String,
    recipient: Address,
    amount: i128,
) {
    env.events().publish(
        (Symbol::new(env, "DISPUTE"), Symbol::new(env, "BOND_FORFEITED")),
        (dispute_id, recipient, amount),
    );
}

// ============================================================================
// Subscription Events
// ============================================================================

/// Emitted when a subscription is created.
#[contractevent]
#[derive(Clone, Debug)]
pub struct SubscriptionCreated {
    pub subscription_id: String,
    pub payer: Address,
    pub plan_id: String,
}

/// Emitted when a subscription is cancelled.
#[contractevent]
#[derive(Clone, Debug)]
pub struct SubscriptionCancelled {
    pub subscription_id: String,
    pub payer: Address,
}

/// Emitted when a subscription expires.
#[contractevent]
#[derive(Clone, Debug)]
pub struct SubscriptionExpired {
    pub subscription_id: String,
    pub payer: Address,
}

/// Emitted when a subscription payment is created.
#[contractevent]
#[derive(Clone, Debug)]
pub struct SubscriptionPaymentCreated {
    pub subscription_id: String,
    pub payment_id: String,
    pub amount: i128,
}

// ============================================================================
// Stream Events
// ============================================================================

/// Emitted when a stream is created.
#[contractevent]
#[derive(Clone, Debug)]
pub struct StreamCreated {
    pub stream_id: String,
    pub sender: Address,
    pub amount: i128,
}

/// Emitted when tokens are withdrawn from a stream.
#[contractevent]
#[derive(Clone, Debug)]
pub struct StreamWithdrawn {
    pub stream_id: String,
    pub receiver: Address,
    pub destination: Address,
    pub amount: i128,
    pub remaining_deposit: i128,
}

/// Emitted when a stream destination is set.
#[contractevent]
#[derive(Clone, Debug)]
pub struct StreamDestinationSet {
    pub stream_id: String,
    pub destination: Address,
}

/// Emitted when a stream milestone is approved.
#[contractevent]
#[derive(Clone, Debug)]
pub struct StreamMilestoneApproved {
    pub stream_id: String,
    pub milestone_index: u32,
}

/// Emitted when a stream rate is decreased.
#[contractevent]
#[derive(Clone, Debug)]
pub struct StreamRateDecreased {
    pub stream_id: String,
    pub old_rate: i128,
    pub new_rate: i128,
}

/// Emitted when a stream is topped up.
#[contractevent]
#[derive(Clone, Debug)]
pub struct StreamToppedUp {
    pub stream_id: String,
    pub additional_deposit: i128,
}

/// Emitted when a stream is cancelled.
#[contractevent]
#[derive(Clone, Debug)]
pub struct StreamCancelled {
    pub stream_id: String,
    pub refunded_amount: i128,
}

/// Emitted when a stream is paused.
#[contractevent]
#[derive(Clone, Debug)]
pub struct StreamPaused {
    pub stream_id: String,
}

/// Emitted when a stream is resumed.
#[contractevent]
#[derive(Clone, Debug)]
pub struct StreamResumed {
    pub stream_id: String,
}

/// Emitted when a stream rate is updated.
#[contractevent]
#[derive(Clone, Debug)]
pub struct StreamRateUpdated {
    pub stream_id: String,
    pub new_rate: i128,
}

/// Emitted when a stream is closed.
#[contractevent]
#[derive(Clone, Debug)]
pub struct StreamClosed {
    pub stream_id: String,
    pub final_amount: i128,
}

// ============================================================================
// Payment Link Events
// ============================================================================

/// Emitted when a payment link is created.
#[contractevent]
#[derive(Clone, Debug)]
pub struct LinkCreated {
    pub link_id: String,
    pub merchant_id: Address,
    pub metadata: Option<soroban_sdk::Map<String, String>>,
}

/// Emitted when a payment link is used.
#[contractevent]
#[derive(Clone, Debug)]
pub struct LinkUsed {
    pub link_id: String,
    pub payer: Address,
    pub amount: i128,
    pub payment_id: String,
    pub metadata: Option<soroban_sdk::Map<String, String>>,
}

/// Emitted when a payment link is deactivated.
#[contractevent]
#[derive(Clone, Debug)]
pub struct LinkDeactivated {
    pub link_id: String,
}

/// Emitted when a payment link is viewed (record_link_view).
#[contractevent]
#[derive(Clone, Debug)]
pub struct LinkViewed {
    pub link_id: String,
}

/// Emitted when a direct transfer payment link is used (issue #485).
/// Provides mandatory audit trail for direct transfers bypassing escrow.
#[contractevent]
#[derive(Clone, Debug)]
pub struct DirectTransferUsed {
    pub link_id: String,
    pub payer: Address,
    pub amount: i128,
}

// ============================================================================
// Merchant Events
// ============================================================================

/// Emitted when a merchant is registered.
#[contractevent]
#[derive(Clone, Debug)]
pub struct MerchantRegistered {
    pub merchant_id: Address,
    pub settlement_currency: String,
}

/// Emitted when a merchant is verified.

#[contractevent]
#[derive(Clone, Debug)]
pub struct MerchantVerified {
    pub merchant_id: Address,
}

/// Emitted when a merchant is updated.
#[contractevent]
#[derive(Clone, Debug)]
pub struct MerchantUpdated {
    pub merchant_id: Address,
}

/// Emitted when a merchant's partial payment setting is updated.
#[contractevent]
#[derive(Clone, Debug)]
pub struct MerchantPartialPaymentUpdated {
    pub merchant_id: Address,
    pub allowed: bool,
}

// ============================================================================
// Access Control Events
// ============================================================================

/// Emitted when a role is granted.
#[contractevent]
#[derive(Clone, Debug)]
pub struct RoleGranted {
    pub role: Symbol,
    pub account: Address,
    pub admin: Address,
}

/// Emitted when a role is revoked.
#[contractevent]
#[derive(Clone, Debug)]
pub struct RoleRevoked {
    pub role: Symbol,
    pub account: Address,
    pub admin: Address,
}

/// Emitted when admin is proposed.
#[contractevent]
#[derive(Clone, Debug)]
pub struct AdminProposed {
    pub current_admin: Address,
    pub new_admin: Address,
}

/// Emitted when a multi-sig admin proposal is fully executed.
#[contractevent]
#[derive(Clone, Debug)]
pub struct AdminProposalExecuted {
    pub proposal_id: u64,
    pub action_tag: Symbol,
    pub executor: Address,
}

/// Emitted when admin is claimed.
#[contractevent]
#[derive(Clone, Debug)]
pub struct AdminClaimed {
    pub new_admin: Address,
}

/// Emitted when roles are synced across contracts.
#[contractevent]
#[derive(Clone, Debug)]
pub struct AccessControlSyncGrant {
    pub role: Symbol,
    pub account: Address,
}

/// Emitted when roles are unsynced across contracts.
#[contractevent]
#[derive(Clone, Debug)]
pub struct AccessControlSyncRevoke {
    pub role: Symbol,
    pub account: Address,
}

// ============================================================================
// Contract Events
// ============================================================================

/// Emitted when a contract is upgraded.
#[contractevent]
#[derive(Clone, Debug)]
pub struct ContractUpgraded {
    pub old_version: String,
    pub new_version: String,
}

// ============================================================================
// Treasury Events
// ============================================================================

/// Emitted when tokens are withdrawn from treasury.
#[contractevent]
#[derive(Clone, Debug)]
pub struct TreasuryWithdrawn {
    pub amount: i128,
    pub recipient: Address,
}

// ============================================================================
// Token Events
// ============================================================================

/// Emitted when a token is removed from the supported list.
#[contractevent]
#[derive(Clone, Debug)]
pub struct TokenRemoved {
    pub token_address: Address,
}

// ============================================================================
// Fee Events
// ============================================================================

/// Emitted when fee split configuration is set.
#[contractevent]
#[derive(Clone, Debug)]
pub struct FeeSplitConfigured {
    pub treasury_bps: u32,
    pub developer_bps: u32,
}

// ============================================================================
// Merchant Auth Events
// ============================================================================

/// Emitted when merchant authorization is granted.
#[contractevent]
#[derive(Clone, Debug)]
pub struct MerchantAuthGranted {
    pub merchant_id: Address,
    pub delegate: Address,
}

/// Emitted when merchant authorization is revoked.
#[contractevent]
#[derive(Clone, Debug)]
pub struct MerchantAuthRevoked {
    pub merchant_id: Address,
    pub delegate: Address,
}

/// Emitted when merchant authorization is used.
#[contractevent]
#[derive(Clone, Debug)]
pub struct MerchantAuthUsed {
    pub merchant_id: Address,
    pub delegate: Address,
}

// ============================================================================
// DEX Router Events
// ============================================================================

/// Emitted when a fallback swap is executed.
#[contractevent]
#[derive(Clone, Debug)]
pub struct SwapFallback {
    pub amount_in: i128,
    pub amount_out: i128,
}

/// Emitted when a swap is executed.
#[contractevent]
#[derive(Clone, Debug)]
pub struct SwapExecuted {
    pub amount_in: i128,
    pub amount_out: i128,
}

/// Emitted when a refund is sent to the caller.
#[contractevent]
#[derive(Clone, Debug)]
pub struct SwapRefundCaller {
    pub recipient: Address,
    pub amount: i128,
}

// ============================================================================
// FX Oracle Events
// ============================================================================

/// Emitted when an FX rate is updated.
#[contractevent]
#[derive(Clone, Debug)]
pub struct RateUpdated {
    pub currency: Symbol,
}

// ============================================================================
// Swap & Pay Events
// ============================================================================

/// Emitted when a swap-and-pay operation completes.
#[contractevent]
#[derive(Clone, Debug)]
pub struct SwapAndPayExecuted {
    pub payment_id: String,
    pub merchant_id: Address,
    pub amount: i128,
    pub token_in: Address,
    pub amount_in: i128,
}

// ============================================================================
// Payment Retry Events (Issue #482)
// ============================================================================

/// Emitted when a payment retry is created.
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentRetryCreated {
    pub original_id: String,
    pub new_id: String,
}

// ============================================================================
// FX Oracle Rate Deviation Events (Issue #478)
// ============================================================================

/// Emitted when a rate update is within 50% of deviation limit.
#[contractevent]
#[derive(Clone, Debug)]
pub struct RateDeviationWarning {
    pub currency: Symbol,
    pub current_rate: i128,
    pub previous_rate: i128,
    pub deviation_bps: u32,
}

// ============================================================================
// Merchant Dispute Events (Issue #481)
// ============================================================================

/// Emitted when a merchant is auto-suspended due to dispute threshold.
#[contractevent]
#[derive(Clone, Debug)]
pub struct MerchantAutoSuspended {
    pub merchant_id: Address,
    pub resolved_against_count: u32,
    pub threshold: u32,
}

/// Emitted when a merchant appeals a suspension.
#[contractevent]
#[derive(Clone, Debug)]
pub struct MerchantSuspensionAppealed {
    pub merchant_id: Address,
    pub reason: String,
}

// ============================================================================
// Router & Session Events (Issue #437, Issue #435)
// ============================================================================

/// Emitted when a DEX router is added to the allowlist.
#[contractevent]
#[derive(Clone, Debug)]
pub struct RouterAdded {
    pub router: Address,
}

/// Emitted when a DEX router is removed from the allowlist.
#[contractevent]
#[derive(Clone, Debug)]
pub struct RouterRemoved {
    pub router: Address,
}

/// Emitted when a session key is registered.
#[contractevent]
#[derive(Clone, Debug)]
pub struct SessionRegistered {
    pub account: Address,
    pub session_key: Address,
    pub expires_at: u64,
}

/// Emitted when a session key is revoked.
#[contractevent]
#[derive(Clone, Debug)]
pub struct SessionRevoked {
    pub account: Address,
    pub session_key: Address,
}

// ============================================================================
// Account Abstraction Events
// ============================================================================

/// Emitted when a session key executes a payload.
#[contractevent]
#[derive(Clone, Debug)]
pub struct SessionExecutedEvent {
    pub account: Address,
    pub session_key: Address,
    pub payload_hash: BytesN<32>,
}
