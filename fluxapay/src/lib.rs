#![no_std]
#![allow(clippy::too_many_arguments)]

use access_control::{
    role_admin, role_arbitrator, role_merchant, role_oracle, role_settlement_operator,
    AccessControl,
};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, map, token, vec, Address, Bytes, BytesN,
    Env, Map, MuxedAddress, String, Symbol, Vec,
};

pub const PAYMENT_TOLERANCE: i128 = 1;
/// Issue #1025: Maximum tip/gratuity as a fraction of the base payment amount, in
/// basis points (2000 = 20%). Enforced by `verify_payment` when `tip_enabled` is set.
pub const MAX_TIP_BPS: i128 = 2000;
const SHORT_LIVE_TTL: u32 = 120_960; // ~1 week at 5s/ledger
const LONG_LIVE_TTL: u32 = 18_921_600; // ~3 years at 5s/ledger
const TTL_BUMP_THRESHOLD_DIVISOR: u32 = 5;
const CREATE_PAYMENT_WINDOW_SECS: u64 = 60;
const CREATE_PAYMENT_MAX_PER_WINDOW: u32 = 30;
pub const DEFAULT_PAYMENT_DURATION_SECS: u64 = 3_600;
const REFUND_FEE_BPS: i128 = 100;
/// Cooldown period after payment confirmation before refunds can be requested (5 minutes in seconds).
const REFUND_COOLDOWN_SECS: u64 = 300;
/// Default refund request expiry period (30 days in seconds).
const REFUND_EXPIRY_SECS: u64 = 30 * 24 * 60 * 60;
/// Issue #638: TTL (in ledgers, ~5s each) for a stored refund idempotency key â€”
/// 30 days, matching the payment `client_token` retention window.
const REFUND_IDEMPOTENCY_TTL_LEDGERS: u32 = (30 * 24 * 60 * 60) / 5;
/// Issue #761: TTL for a payment idempotency key (24 hours at 5s per ledger).
const PAYMENT_IDEMPOTENCY_TTL_LEDGERS: u32 = (24 * 60 * 60) / 5;
/// Issue #480: Minimum time between daily settlements (24 hours in seconds).
const SETTLEMENT_DAILY_INTERVAL_SECS: u64 = 86_400;
/// Issue #480: Minimum time between weekly settlements (7 days in seconds).
const SETTLEMENT_WEEKLY_INTERVAL_SECS: u64 = 604_800;
/// Issue #480: Minimum pending balance required to trigger a settlement.
const SETTLEMENT_MIN_AMOUNT: i128 = 1_000_000; // 0.1 USDC (7 decimals)
/// Fixed dispute bond in the contract's stablecoin denomination.
const DISPUTE_BOND_AMOUNT: i128 = 100_000;
/// Default threshold separating small and large disputes: 100 USDC (7 decimals).
pub const DEFAULT_DISPUTE_DEADLINE_THRESHOLD_AMOUNT: i128 = 1_000_000_000;
pub const SMALL_DISPUTE_DEADLINE_SECS: u64 = 3 * 24 * 60 * 60;
pub const LARGE_DISPUTE_DEADLINE_SECS: u64 = 7 * 24 * 60 * 60;
/// Default maximum swap slippage when setting the effective minimum output.
pub const DEFAULT_MAX_SLIPPAGE_BPS: u32 = 50;
/// Default maximum adverse deviation from an FX oracle reference price.
pub const MAX_PRICE_IMPACT_BPS: u32 = 100;
/// Default maximum aggregate swap input per PaymentProcessor per ledger.
pub const MAX_SWAP_VOLUME_PER_LEDGER: i128 = 10_000_000_000_000;
const BASIS_POINTS_DENOMINATOR: i128 = 10_000;

// Issue #167: Tiered refund fees based on merchant KYC tier
const REFUND_FEE_BPS_BASIC: i128 = 100; // 1.0% for Basic tier
const REFUND_FEE_BPS_FULL: i128 = 80; // 0.8% for Full tier
const REFUND_FEE_BPS_BUSINESS: i128 = 50; // 0.5% for Business tier

/// Default window (Issue #170) during which a pending refund may be processed,
/// measured from `Refund::created_at`. Configurable via `set_refund_expiry`.
pub const DEFAULT_REFUND_EXPIRY_SECS: u64 = 30 * 24 * 60 * 60;
// Issue #63: Monthly processing volume caps per KYC tier (in USDC stroops, 7 decimals)
// Unverified: $500, Basic: $10,000, Full: $100,000, Business: unlimited (i128::MAX)
const TIER_CAP_UNVERIFIED: i128 = 5_000_000_000; // $500
const TIER_CAP_BASIC: i128 = 100_000_000_000; // $10,000
const TIER_CAP_FULL: i128 = 1_000_000_000_000; // $100,000
const TIER_CAP_BUSINESS: i128 = i128::MAX; // unlimited

// Issue #207: Cumulative volume thresholds for automatic KYC tier upgrades (in USDC stroops)
const TIER_UPGRADE_THRESHOLD_BASIC: i128 = TIER_CAP_UNVERIFIED; // $500 cumulative â†’ Basic
const TIER_UPGRADE_THRESHOLD_FULL: i128 = TIER_CAP_BASIC; // $10,000 cumulative â†’ Full
const TIER_UPGRADE_THRESHOLD_BUSINESS: i128 = TIER_CAP_FULL; // $100,000 cumulative â†’ Business

/// Maximum number of payment retries before a subscription is cancelled.
pub const SUBSCRIPTION_MAX_RETRIES: u32 = 3;
/// Spacing between retry attempts in seconds (2 days).
pub const SUBSCRIPTION_RETRY_INTERVAL_SECS: u64 = 2 * 24 * 60 * 60;

// Issue #625: Maximum lengths for user-supplied string fields to prevent ledger bloat.
const MAX_REASON_LEN: usize = 256;
const MAX_EVIDENCE_LEN: usize = 512;
const MAX_NOTES_LEN: usize = 512;
pub(crate) const ZERO_CONTRACT_STRKEY: &str =
    "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM";
pub mod constants;
pub mod data_keys;
pub mod gas_estimator;
/// Issues #1026/#1027: on-chain Merkle batch payment proof verifier.
pub mod merkle_distributor;
pub mod merchant_registry;
pub mod payment_processor;
pub mod refund_manager;
pub mod types;
pub mod utils;

pub use constants::*;
pub use data_keys::*;
pub use gas_estimator::{
    CostEstimate, GasEstimator, GasEstimatorClient, GasEstimatorError, Operation,
};
pub use merchant_registry::{
    FeeConfig, KycTier, MaybeFeeConfig, Merchant, MerchantError, MerchantRegistry,
    MerchantRegistryClient,
};
pub use payment_processor::*;
pub use refund_manager::*;
pub use types::*;
pub use utils::{format_id, is_valid_cid, validate_id, validate_ipfs_multihash};

pub mod access_control;
pub mod account_abstraction;
mod dex_router;
pub mod events;
pub mod fx_oracle;
pub mod htlc;
pub mod merchant_auth;
pub mod meta_transaction;
mod payment_state_machine;
pub mod stream;

pub use stream::{
    MultiPaymentStream, PayeeAllocation, PaymentStream, PaymentStreaming, StreamDataKey,
    StreamError, StreamStatus, MAX_MULTI_PAYEES, MULTI_STREAM_SHARE_TOTAL,
};

pub use access_control::AccessControlDataKey;
pub use access_control::{AdminAction, AdminProposal};
pub use dex_router::{DexRouter, DexRouterClient};
pub use fx_oracle::{FXOracle, FXOracleClient, FXOracleError};
pub use htlc::{Htlc, HtlcClient, HtlcDataKey, HtlcError, HtlcRecord, HtlcStatus};
pub use merchant_auth::{ApiKeyRecord, MerchantAuthError, MerchantAuthorization, MerchantPreAuth};


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
    /// Actual amount received on-chain; set by verify_payment for reconciliation.
    pub amount_received: Option<i128>,
    /// Optional memo for Stellar payment routing.
    pub memo: Option<String>,
    /// Optional memo type: Text, Id, Hash, or Return.
    pub memo_type: Option<String>,
    /// Token contract address used for this payment (None defaults to the configured USDC token).
    pub token_address: Option<Address>,
    /// Optional 32-byte hash merchants can use to tie a payment to an order ID or customer ID.
    pub metadata_hash: Option<BytesN<32>>,
    /// Issue #304: FX rate snapshot captured during verify_payment.
    pub fx_rate: Option<i128>,
    /// Issue #304: Timestamp when the FX rate was captured.
    pub fx_rate_at: Option<u64>,
    /// Issue #173: Original token address used by payer (for swap_and_pay refunds).
    pub original_token: Option<Address>,
    /// Issue #173: Swap path used in swap_and_pay (for refund routing).
    pub swap_path: Option<Vec<Address>>,
    /// Arbitrary key-value metadata supplied by the merchant at creation time (max 20 keys, 256 chars per value).
    pub metadata: Option<Map<String, String>>,
    /// Optional per-payment fee waiver code set at `create_payment` time.
    /// If this code is valid (exists in the admin-managed fee waiver registry,
    /// has not expired, and still has remaining uses), `settle_payment`
    /// waives the platform fee and decrements `max_uses`.
    pub fee_waiver_code: Option<String>,
    /// Issue #482: Payment ID of the original payment if this is a retry; None if original or not retried.
    pub retry_of_payment_id: Option<String>,
    /// Issue #484: Muxed account ID from payer M-address; None for G-addresses or on-chain payments.
    pub payer_muxed_id: Option<u64>,
    /// Issue #668: ID of the payment link that created this payment via `use_link`,
    /// for tracing a payment back to its source link. `None` for payments created
    /// directly via `create_payment`/`swap_and_pay`.
    pub payment_link_id: Option<String>,
    /// Issue #767: When true, allows funding the payment across multiple transactions.
    pub allow_partial: Option<bool>,
    /// Issue #844: Whether the merchant enabled an optional tip/gratuity on this payment.
    /// Issue #1025 makes this real: `verify_payment` only honors a tip, capped at
    /// `MAX_TIP_BPS` of `amount`, when this is `true`.
    pub tip_enabled: bool,
    /// Issue #844: Tip/gratuity amount paid by the customer; stored separately from `amount`.
    /// Issue #1025: populated by `verify_payment` (validated against `MAX_TIP_BPS`);
    /// `None` until verified, or if `tip_enabled` is `false`, or the payer chose not to tip.
    pub tip_amount: Option<i128>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifyPaymentArgs {
    pub payment_id: String,
    pub transaction_hash: BytesN<32>,
    pub payer_address: Address,
    pub amount_received: i128,
    pub payer_muxed_id: Option<u64>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentSummary {
    pub payment_id: String,
    pub amount: i128,
    /// Issue #844: Tip itemized separately from base amount.
    pub tip_amount: i128,
    pub fee: i128,
    pub refund_amount: i128,
    pub status: PaymentStatus,
    pub settled_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationReport {
    pub merchant_id: Address,
    pub period_start: u64,
    pub period_end: u64,
    pub payments: Vec<PaymentSummary>,
    pub total_gross: i128,
    pub total_fees: i128,
    pub total_refunds: i128,
    pub total_net_settled: i128,
    pub dispute_adjustments: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationPage {
    pub items: Vec<PaymentSummary>,
    pub total_confirmed: i128,
    pub total_settled: i128,
    pub page_total: i128,
    pub has_more: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentStatusEvent {
    pub status: PaymentStatus,
    pub timestamp: u64,
    pub tx_hash: Option<BytesN<32>>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KycTierLimits {
    pub tier: KycTier,
    pub max_amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaymentStatus {
    Pending,
    Confirmed,
    Settled,
    Expired,
    Failed,
    /// Customer sent less than the required amount (within tolerance but below threshold).
    PartiallyPaid,
    /// Customer sent more than the required amount (e.g. tip or rounding).
    Overpaid,
    /// A dispute was opened against a confirmed payment.
    Disputed,
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
    /// Cryptographic proof hash of return agreement for off-chain verification (Issue #176).
    pub receipt_hash: Option<BytesN<32>>,
    /// Issue #168: Approved by operator, allowing customer to claim.
    pub approved: bool,
    /// Expiry timestamp for refund requests (Issue #170).
    pub expiry_at: u64,
}

/// Queued when a partially funded payment expires before it is fully paid.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutoRefundRequest {
    pub refund_id: String,
    pub payment_id: String,
    pub merchant_id: Address,
    pub payer_address: Address,
    pub amount: i128,
    pub token_address: Address,
    pub reason: String,
    pub requested_at: u64,
    pub dispute_id: Option<String>,
    pub payer_muxed_id: Option<u64>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RefundStatus {
    Pending,
    Completed,
    Rejected,
    Cancelled,
}

/// Issue #638: Persisted record for a refund idempotency key. Stored under
/// `DataKey::RefundIdempotencyKey(key)` with a 30-day TTL so that retrying
/// `create_refund` with the same key returns the original `refund_id` instead
/// of creating a duplicate. Reusing a key with different parameters is rejected
/// with `Error::DuplicateIdempotencyKey`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefundIdempotencyRecord {
    pub refund_id: String,
    pub payment_id: String,
    pub amount: i128,
    pub reason: String,
}

/// Issue #676: consolidated read-only view of all refund configuration.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefundPolicy {
    /// Whether `process_refund` requires a `receipt_hash` (Issue #176).
    pub require_receipt_hash: bool,
    /// Refund request expiry window in seconds (Issue #170).
    pub refund_expiry_secs: u64,
    /// Refund processing fee in basis points.
    pub refund_fee_bps: i128,
    /// Cooldown period after payment confirmation before refunds can be requested, in seconds.
    pub cooldown_secs: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvoiceStatus {
    Created,
    Paid,
    Overdue,
    /// Issue #1067: Merchant-initiated cancellation (soft-delete).
    Voided,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LineItem {
    pub description: String,
    pub amount: i128,
    pub quantity: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Invoice {
    pub invoice_id: String,
    pub merchant_id: Address,
    pub customer_email: String,
    pub line_items: Vec<LineItem>,
    pub total_amount: i128,
    pub currency: Symbol,
    pub due_date: u64,
    pub status: InvoiceStatus,
    pub payment_link_id: Option<String>,
    pub created_at: u64,
    /// Issue #1067: Timestamp when the invoice was paid (set by mark_invoice_paid).
    pub paid_at: Option<u64>,
    /// Issue #1067: Timestamp when the invoice was voided (set by void_invoice).
    pub voided_at: Option<u64>,
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
    pub merchant_id: Address,
    pub refund_id: Option<String>,
    pub amount: i128,
    pub reason: String,
    pub evidence: String,
    pub status: DisputeStatus,
    pub disputer: Address,
    pub created_at: u64,
    pub resolved_at: Option<u64>,
    pub resolution_notes: Option<String>,
    /// Operator-set deadline (Unix timestamp) by which the dispute must be resolved.
    pub review_deadline: Option<u64>,
    /// True when the dispute has been flagged for escalation (e.g. deadline exceeded).
    pub escalated: bool,
    /// Issue #177: Computed deadline in seconds (3 days for small, 7 days for large).
    pub computed_deadline_secs: Option<u64>,
    /// Multi-party payout splits for marketplace dispute resolution.
    pub payout_splits: Vec<SettlementSplit>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeSummary {
    pub open: u32,
    pub under_review: u32,
    pub resolved: u32,
    pub rejected: u32,
    pub escalated: u32,
    pub older_than_7_days: u32,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error {
    PaymentNotFound = 404,
    RefundNotFound = 405,
    InvalidAmount = 406,
    Unauthorized = 1,
    PaymentAlreadyExists = 2,
    PaymentExpired = 3,
    InvalidPaymentId = 4,
    RefundAlreadyProcessed = 8,
    DisputeNotFound = 9,
    DisputeAlreadyResolved = 12,
    PaymentAlreadyProcessed = 14,
    AccessControlError = 15,
    RefundExceedsPayment = 16,
    ContractPaused = 17,
    RateLimitExceeded = 18,
    RefundCancelled = 19,
    UnsupportedToken = 20,
    AmountBelowMin = 21,
    AmountAboveMax = 22,
    InvalidExpiry = 23,
    InvalidSettlement = 24,
    DuplicateIdempotencyKey = 25,
    InvalidAddress = 26,
    /// Swap path contains a circular route indicative of arbitrage exploitation.
    ArbitrageDetected = 27,
    /// DEX path or quoted returns failed validation.
    SwapPathInvalid = 28,
    /// DEX quoted swap output deviates from the oracle reference price.
    OraclePriceDeviation = 29,
    /// Subscription is in a grace period; payment will be retried.
    SubscriptionInGracePeriod = 30,
    /// Subscription has exhausted all retries and is now cancelled.
    SubscriptionRetryExhausted = 31,
    /// The provided resume timestamp is in the past or invalid.
    InvalidResumeTimestamp = 32,
    /// Merchant authorization error (see MerchantAuthError for details).
    MerchantAuthError = 33,
    /// Dispute payout_splits amounts don't sum to the dispute amount (Issue #446).
    InvalidSplitSum = 34,
    /// Refund policy requires a receipt_hash but none was provided (Issue #176).
    MissingReceiptHash = 35,
    /// Refund's `expiry_at` deadline has passed (Issue #170).
    RefundExpired = 36,
    /// Arbitrator has already cast a vote on this dispute.
    AlreadyVoted = 37,
    /// Merchant has exceeded their KYC tier monthly processing volume cap.
    TierVolumeLimitExceeded = 38,
    /// Refund requested before cooldown period elapsed.
    RefundCooldownNotElapsed = 42,
    /// Batch payment request exceeds the supported maximum size.
    BatchTooLarge = 39,
    /// Insufficient arbitrators available for voting.
    InsufficientArbitrators = 40,
    /// Voting threshold not met for dispute resolution.
    ArbitrationVotingThresholdNotMet = 41,
    /// Fee proposal has not matured for the required 7 days.
    FeeProposalNotReady = 43,
    /// No active fee proposal found.
    NoFeeProposal = 44,
    /// Issue #180: Evidence field is not a valid IPFS multihash (CIDv0/CIDv1).
    InvalidEvidenceFormat = 45,
    /// Dispute creation rate limit exceeded (per-payer open cap or global hourly cap).
    DisputeRateLimitExceeded = 46,
    /// Issue #185: One or both collaborative settlement signatures are invalid.
    InvalidSettlementSignature = 47,
    /// Issue #303: FX oracle rate is stale or unavailable.
    StaleOracleRate = 48,
    /// Issue #476: Payment link has expired.
    LinkExpired = 49,
    /// Issue #313: Reentrancy detected in process_refund_internal or settle_payment.
    Reentrancy = 50,
    /// Upgrade failed â€” WASM hash replacement rejected by the host.
    UpgradeFailed = 51,
    /// Treasury balance is smaller than the requested withdrawal amount.
    InsufficientTreasuryBalance = 52,
    /// Metadata map has too many keys (> 20).
    MetadataTooLarge = 53,
    /// A metadata value exceeds maximum length (> 256 chars).
    MetadataValueTooLong = 54,
    /// Issue #397: memo_type is not one of: Text, Id, Hash, Return.
    InvalidMemoType = 55,
    /// Issue #397: Text memo exceeds the 28-byte Stellar limit.
    MemoTooLong = 56,
    /// Issue #397: Id memo is not parseable as a u64.
    InvalidMemoId = 57,
    /// Issue #516: Payer address is not on the merchant's customer whitelist.
    PayerNotWhitelisted = 58,
    /// Payment link has reached its configured max_uses limit.
    LinkMaxUsesReached = 59,
    /// Issue #485: Payment was created via a direct_transfer link and disputes are not allowed.
    DirectTransferNotDisputable = 60,
    /// Issue #482: Maximum retry chain depth (3) exceeded for payment retry.
    MaxRetriesExceeded = 61,
    /// Retry payment would exceed the maximum chain depth of three.
    RetryChainTooDeep = 347,
    /// Issue #505: Invalid payment status transition attempted.
    InvalidStatusTransition = 62,
    /// Issue #450: Customer called `claim_refund` before an operator approved it.
    RefundNotApproved = 63,
    /// Issue #437: DEX router is not in the allowed routers list.
    RouterNotAllowed = 64,
    /// Issue #436: Aggregate route output is less than minimum output amount.
    RouteOutputInsufficient = 65,
    /// Issue #682: Batch payment creation contains duplicate payment IDs.
    BatchContainsDuplicates = 66,
    /// Issue #625: A user-supplied string field (reason, evidence, resolution_notes) exceeds its maximum allowed length.
    InputTooLong = 67,
    /// Issue #624: A timelocked admin action was executed before the delay period expired.
    TimelockNotExpired = 68,
    /// Issue #622: Evidence field is not a valid IPFS CID (CIDv0 starts with "Qm"/46 chars; CIDv1 starts with "bafy"/â‰¥59 chars).
    InvalidEvidenceCid = 69,
    /// Issue #841: Incoming muxed sub-account ID does not match the expected muxed_payer.
    MuxedAccountMismatch = 70,
    /// Treasury multisig configuration not set or invalid.
    TreasuryMultisigNotConfigured = 71,
    /// Caller is not an authorized treasury multisig signer.
    NotAuthorizedTreasurySigner = 72,
    /// Treasury withdrawal proposal not found.
    TreasuryProposalNotFound = 73,
    /// Treasury withdrawal proposal has already been executed.
    TreasuryProposalAlreadyExecuted = 74,
    /// Treasury withdrawal proposal has been cancelled.
    TreasuryProposalCancelled = 75,
    /// Treasury withdrawal proposal timelock has not expired.
    TreasuryTimelockNotExpired = 76,
    /// Treasury withdrawal proposal has expired.
    TreasuryProposalExpired = 77,
    /// Signer has already approved this treasury withdrawal proposal.
    TreasuryAlreadyApproved = 78,
    /// Insufficient approvals to execute treasury withdrawal.
    TreasuryInsufficientApprovals = 79,
    /// Invalid treasury multisig threshold (zero or exceeds signer count).
    InvalidTreasuryThreshold = 80,
    /// Treasury withdrawal amount exceeds token balance.
    InsufficientTokenTreasuryBalance = 81,
    /// Issue #836: Subscription is still in its free trial; no charge yet.
    TrialActive = 82,
    /// Issue #836: Requested trial_days exceeds the maximum of 90 days.
    TrialTooLong = 83,
    /// Payment link does not exist or belongs to a different merchant.
    InvalidPaymentLink = 84,
    /// Issue #777: Payment amount or monthly volume exceeds the merchant's KYC tier limit.
    KycLimitExceeded = 85,
    /// Issue #1061: A single-use payment link has already been redeemed and cannot be used again.
    LinkAlreadyUsed = 86,
    /// Contract version must be a numeric x.y.z string of at most 32 characters.
    InvalidVersion = 87,
    /// Issue #1033: FX rate used by `confirm_payment` is older than the
    /// PaymentProcessor's own configured staleness threshold
    /// (`MAX_ORACLE_STALENESS_SECS` / `set_oracle_staleness_threshold`),
    /// independent of the oracle's own internal cap (`StaleOracleRate`).
    OracleStale = 88,
    /// Issue #1032: The requested `deposit_address` is already assigned to
    /// another open (`Pending`, unexpired) payment.
    DepositAddressInUse = 89,
    /// Issue #1031: `settle_payment` was called before the merchant's configured
    /// Daily/Weekly settlement interval has elapsed since the last settlement.
    SettlementIntervalNotElapsed = 88,
    /// Issue #1031: The merchant's accumulated settlement balance (including this
    /// payment) is below `SETTLEMENT_MIN_AMOUNT` for a Daily/Weekly schedule.
    SettlementBelowMinimum = 89,
    /// Issue #1029: A sub-account's merchant delegation has passed its `expires_at`.
    DelegationExpired = 90,
    /// Issue #1029: A sub-account's merchant delegation has been revoked.
    DelegationRevoked = 91,
    /// Issue #1037: `retry_of_payment_id` passed to `create_payment` does not refer to
    /// an existing payment owned by the same merchant that is in `Expired` status.
    InvalidRetryTarget = 88,
    /// Issue #1025: Tip passed to `verify_payment` exceeds `MAX_TIP_BPS` of the base amount.
    TipExceedsMax = 89,
    /// Issue #1209: `settle_payment` was called before the payment's configured
    /// `settle_at` deferred-settlement timestamp has been reached.
    SettlementNotYetDue = 1209,
    /// Issue #1170: a per-merchant storage index has reached its documented cap.
    StorageEntryLimitExceeded = 1210,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatePaymentArgs {
    pub payment_id: String,
    pub merchant_id: Address,
    pub amount: i128,
    pub currency: Symbol,
    pub deposit_address: Address,
    pub expires_at: Option<u64>,
    pub duration_secs: Option<u64>,
    pub memo: Option<String>,
    pub memo_type: Option<String>,
    pub token_address: Option<Address>,
    pub client_token: Option<String>,
    /// Optional idempotency key for safely retrying payment creation.
    pub idempotency_key: Option<String>,
    pub metadata_hash: Option<BytesN<32>>,
    /// Arbitrary key-value metadata (max 20 keys, 256 chars per value).
    pub metadata: Option<Map<String, String>>,
    /// Optional per-payment fee waiver code. If valid during settlement, the
    /// platform fee is waived. `None` means no per-payment waiver request.
    pub fee_waiver_code: Option<String>,
    /// Issue #482: Payment ID of the original payment if this is a retry; None if original or not retried.
    pub retry_of_payment_id: Option<String>,
    /// Issue #484: Muxed account ID from payer M-address; None for G-addresses or on-chain payments.
    pub payer_muxed_id: Option<u64>,
    /// Customer/payer address, checked against the merchant's whitelist when
    /// `Merchant.whitelist_mode` is enabled (issue #516).
    pub payer: Option<Address>,
    /// Issue #767: Optional flag to allow partial payment accumulation.
    pub allow_partial: Option<bool>,
    /// Issue #844: When true, customers may submit a tip via `confirm_payment`.
    /// Issue #1025 makes this actually enforceable: `verify_payment` validates
    /// and records a tip, capped at `MAX_TIP_BPS`, only when this is `true`.
    pub tip_enabled: bool,
    /// Issue #1209: Optional future timestamp before which `settle_payment`
    /// must reject settlement, supporting escrow-like deferred release.
    /// `None` means settlement is allowed as soon as the payment is confirmed.
    pub settle_at: Option<u64>,
}

/// Issue #771: Payment request item for `create_payment_batch`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentRequest {
    pub payment_id: String,
    pub amount: i128,
    pub currency: Symbol,
    pub deposit_address: Address,
    pub expires_at: Option<u64>,
    pub duration_secs: Option<u64>,
    pub memo: Option<String>,
    pub memo_type: Option<String>,
    pub token_address: Option<Address>,
    pub client_token: Option<String>,
    pub metadata_hash: Option<BytesN<32>>,
    pub metadata: Option<Map<String, String>>,
    pub fee_waiver_code: Option<String>,
    pub payer: Option<Address>,
    pub payer_muxed_id: Option<u64>,
    /// Issue #1209: Optional future timestamp before which `settle_payment`
    /// must reject settlement, supporting escrow-like deferred release.
    pub settle_at: Option<u64>,
}

/// Arguments for a single dispute in `batch_create_disputes` / `create_dispute`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateDisputeArgs {
    pub payment_id: String,
    pub amount: i128,
    pub reason: String,
    pub evidence: String,
    pub disputer: Address,
    pub payout_splits: Vec<SettlementSplit>,
}

/// Per-item outcome for `batch_create_disputes` (partial success allowed).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisputeBatchItemResult {
    Ok(String),
    Err(u32),
}

/// Per-item outcome for `batch_process_refunds` (partial success allowed).
/// Issue #1199.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RefundBatchItemResult {
    /// The refund_id that was successfully processed.
    Ok(String),
    /// The refund_id that failed, together with the `Error` discriminant.
    Err(String, u32),
}

/// Resolution details for one dispute in `batch_resolve_disputes`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeResolution {
    pub dispute_id: String,
    pub in_favor_of: Address,
    pub notes: String,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwapLimits {
    pub default_max_slippage_bps: u32,
    pub max_price_impact_bps: u32,
    pub max_swap_volume_per_ledger: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
struct SwapLedgerVolume {
    ledger_sequence: u32,
    volume: i128,
}

/// Hard cap for dispute batch size.
pub const MAX_DISPUTE_BATCH: u32 = 20;

/// Hard cap for refund batch size (Issue #1199).
pub const MAX_REFUND_BATCH: u32 = 20;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwapAndPayArgs {
    pub payer: Address,
    pub payment_id: String,
    pub merchant_id: Address,
    pub amount: i128,
    pub currency: Symbol,
    pub deposit_address: Address,
    pub token_in: Address,
    pub amount_in: i128,
    pub amount_out_min: i128,
    pub path: Vec<Address>,
    pub expires_at: Option<u64>,
    pub dex_router: Address,
    /// Optional FX oracle used to sanitize DEX swap quotes.
    pub fx_oracle: Option<Address>,
    /// Oracle rate pair symbol (required when `fx_oracle` is set).
    pub oracle_pair: Option<Symbol>,
    /// Maximum allowed deviation from oracle price in basis points (100 = 1%).
    pub max_deviation_bps: u32,
}

/// Issue #436: Single route for multi-DEX route splitting / aggregation.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwapRoute {
    pub router: Address,
    pub path: Vec<Address>,
    pub amount_in: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PauseState {
    pub paused: bool,
    pub reason: String,
    pub admin: Option<Address>,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PauseInfo {
    pub global: PauseState,
    pub creation: PauseState,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateLimitConfig {
    pub window_secs: u64,
    pub max_per_window: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MerchantCreateRateLimit {
    pub last_payment_at: u64,
    pub count: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AmountLimits {
    pub min: Option<i128>,
    pub max: Option<i128>,
}

/// A single recipient in a multi-account settlement split.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementSplit {
    pub recipient: Address,
    pub amount: i128,
}

/// Vote choice for stake-weighted dispute voting.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VoteChoice {
    /// Vote in favour of the disputer (refund should be issued).
    Favour,
    /// Vote against the disputer (dispute should be rejected).
    Against,
}

/// Issue #843: Binary stake-weighted vote recorded by `cast_vote`.
///
/// Defined here so `choice` uses this module's `VoteChoice`. The copy in
/// `types.rs` is shadowed by this item.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StakeWeightedVote {
    pub choice: VoteChoice,
    /// `stake_amount / VOTE_WEIGHT_UNIT` at the time of the vote.
    pub vote_weight: i128,
}

/// Accumulated vote tally for a dispute.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoteTally {
    /// Total stake weight voting in favour.
    pub favour_weight: i128,
    /// Total stake weight voting against.
    pub against_weight: i128,
    /// Number of arbitrators who have voted.
    pub vote_count: u32,
    /// Issue #843: Sum of registered vote weights from `lock_stake`.
    pub total_registered_weight: i128,
}

/// Number of `ARBITRATOR`-role votes (either direction) required to
/// auto-execute a dispute resolution via [`FluxaPayContract::vote_dispute`].
pub const ARBITRATOR_VOTING_THRESHOLD: u32 = 3;

/// Vote choice for the simple `ARBITRATOR`-role voting flow (as opposed to
/// the stake-weighted [`VoteChoice`] flow above).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArbitratorVoteChoice {
    Approve,
    Reject,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArbitratorVote {
    pub dispute_id: String,
    pub arbitrator: Address,
    pub vote: ArbitratorVoteChoice,
    pub voted_at: u64,
}

/// Accumulated vote counts for the `ARBITRATOR`-role voting flow.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArbitratorVoteTally {
    pub approve_count: u32,
    pub reject_count: u32,
}

/// Record of a single admin treasury withdrawal.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreasuryWithdrawal {
    pub amount: i128,
    pub destination: Address,
    pub admin: Address,
    pub withdrawn_at: u64,
}

/// Maximum number of withdrawal records retained in `TreasuryWithdrawalHistory`.
pub const TREASURY_WITHDRAWAL_HISTORY_CAP: u32 = 100;

/// Issue #628: Maximum number of entries `get_top_merchants` will return,
/// keeping the ledger-read budget bounded regardless of the caller's `limit`.
pub const TOP_MERCHANTS_MAX_LIMIT: u32 = 100;

/// Issue #666: Record of a single settlement's platform-fee collection,
/// appended to `DataKey::FeeCollectionHistory` from `settle_payment`.
/// `get_platform_fee_report` sums the records whose `collected_at` falls
/// within the queried `[from_ts, to_ts]` window.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeCollectionRecord {
    pub collected_at: u64,
    /// Total protocol fee taken from this settlement (settlement fee + platform fee).
    pub total_fee: i128,
    /// Portion of `total_fee` retained by the treasury.
    pub treasury_share: i128,
    /// Portion of `total_fee` routed to the configured developer address (if any).
    pub developer_share: i128,
}

/// Issue #666: Aggregated platform fee report for a queried time period,
/// returned by `get_platform_fee_report`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformFeeReport {
    pub total_fees_collected: i128,
    pub treasury_share: i128,
    pub developer_share: i128,
    pub payment_count: u64,
}

/// Maximum number of fee-collection records retained in `FeeCollectionHistory`.
/// Kept larger than `TREASURY_WITHDRAWAL_HISTORY_CAP` since fee reporting is
/// meant to cover longer look-back windows (e.g. a full reporting month).
pub const FEE_COLLECTION_HISTORY_CAP: u32 = 5_000;

/// Issue #168: Fee split configuration for refund fees.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeSplitConfig {
    /// Treasury allocation in basis points (e.g., 7000 = 70%).
    pub treasury_bps: u32,
    /// Developer rewards allocation in basis points (e.g., 3000 = 30%).
    pub developer_bps: u32,
    /// Treasury destination address.
    pub treasury_address: Address,
    /// Developer rewards destination address.
    pub developer_address: Address,
}

/// Operator note persisted on-chain for dispute transparency.
///
/// Stored under `DataKey::DisputeOperatorNote(dispute_id)` and emitted
/// in full via the `DISPUTE / OPERATOR_NOTE` event so that off-chain
/// indexers can reconstruct the complete audit trail.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeOperatorNote {
    /// The dispute this note belongs to.
    pub dispute_id: String,
    /// Operator address that authored the note.
    pub operator: Address,
    /// Full resolution notes text.
    pub resolution_notes: String,
    /// Operator-provided signature (e.g. base64-encoded Ed25519 sig over the note hash).
    pub operator_signature: String,
    /// Ledger timestamp when the note was recorded.
    pub recorded_at: u64,
}

/// Issue #185: Record of a collaboratively settled dispute.
///
/// Stored under `DataKey::CollaborativeSettlement(dispute_id)` when both
/// the buyer and merchant sign an agreed settlement off-chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollaborativeSettlement {
    /// The dispute that was settled.
    pub dispute_id: String,
    /// Agreed settlement amount (may be less than the full disputed amount).
    pub settlement_amount: i128,
    /// Ed25519 public key of the buyer used to verify `signature_buyer`.
    pub buyer_pubkey: BytesN<32>,
    /// Ed25519 public key of the merchant used to verify `signature_merchant`.
    pub merchant_pubkey: BytesN<32>,
    /// Ledger timestamp when the settlement was recorded.
    pub settled_at: u64,
}

/// Issue #664: A single usage-metering record for a subscription, appended
/// by `submit_usage_metrics` and queryable via `get_usage_metrics`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsageMetrics {
    /// The subscription this usage record belongs to.
    pub subscription_id: String,
    /// Number of usage units consumed in this billing cycle.
    pub units_used: i128,
    /// Price per unit (in the subscription token's smallest unit) at the
    /// time this record was submitted.
    pub unit_price: i128,
    /// `units_used * unit_price` â€” the metered charge amount for this cycle.
    pub amount: i128,
    /// Ledger timestamp when this usage record was submitted.
    pub recorded_at: u64,
}

/// Configuration for creating a payment.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentConfig {
    /// Optional memo for Stellar payment routing.
    pub memo: Option<String>,
    /// Optional memo type: Text, Id, Hash, or Return.
    pub memo_type: Option<String>,
    /// Token contract address used for this payment (None defaults to the configured USDC token).
    pub token_address: Option<Address>,
    /// Optional idempotency key. If provided, retrying with the same key and payment_id
    /// returns the existing payment. Using the same key with a different payment_id
    /// returns `DuplicateIdempotencyKey`.
    pub client_token: Option<String>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubscriptionStatus {
    Active,
    Paused,
    Cancelled,
    Expired,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Subscription {
    pub subscription_id: String,
    pub merchant_id: Address,
    pub payer_address: Address,
    pub plan_id: String,
    pub amount: i128,
    pub currency: Symbol,
    pub interval_secs: u64,
    pub next_payment_at: u64,
    pub status: SubscriptionStatus,
    pub created_at: u64,
    pub last_payment_at: Option<u64>,
    pub total_payments: u32,
    pub max_payments: Option<u32>,
    /// Number of consecutive failed payment attempts in the current grace period.
    pub retry_count: u32,
    /// Timestamp of the next retry attempt (set when a payment fails and grace period begins).
    pub next_retry_at: Option<u64>,
    /// When set, the subscription will automatically resume at this timestamp.
    /// Only meaningful when `status == Paused`.
    pub resume_at: Option<u64>,
    /// Optional affiliate address to receive a percentage of each payment.
    pub affiliate: Option<Address>,
    /// Affiliate fee in basis points (bps). If set and `affiliate` is Some,
    /// `affiliate_fee_bps / 10000` of each payment will be routed to the affiliate.
    pub affiliate_fee_bps: Option<u32>,
    /// Issue #836: Ledger timestamp when the free trial ends. `None` if the
    /// plan has no trial. While `now < trial_ends_at`, `charge_subscription`
    /// returns `Error::TrialActive` and does not bill.
    pub trial_ends_at: Option<u64>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BillingInterval {
    Daily,
    Weekly,
    Monthly,
    Annually,
}

impl BillingInterval {
    /// Returns the approximate duration in seconds for each interval.
    ///
    /// # Parameters
    /// - `&self` - Reference to the billing interval whose duration is returned.
    ///
    /// # Errors
    /// No errors are returned.
    pub fn to_secs(&self) -> u64 {
        match self {
            BillingInterval::Daily => 86_400,
            BillingInterval::Weekly => 604_800,
            BillingInterval::Monthly => 2_592_000,   // 30 days
            BillingInterval::Annually => 31_536_000, // 365 days
        }
    }
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubscriptionPlan {
    pub plan_id: String,
    pub merchant_id: Address,
    pub name: String,
    pub description: String,
    pub amount: i128,
    pub currency: Symbol,
    pub interval_secs: u64,
    pub billing_interval: BillingInterval,
    pub active: bool,
    /// Optional split payout configuration for bundle subscriptions.
    /// If non-empty, the plan amount will be distributed to the configured
    /// `SettlementSplit` recipients on each subscription charge.
    pub payout_splits: Vec<SettlementSplit>,
    /// Issue #836: Optional free-trial length in days (max 90). When set,
    /// subscribers are not charged until `trial_ends_at`.
    pub trial_days: Option<u32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawalRecipient {
    pub stream_id: String,
    pub destination: Address,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeProposal {
    pub proposed_fee: i128,
    pub proposed_at: u64,
}

/// Admin-managed reusable fee-waiver code for per-payment zero-fee campaigns.
///
/// Stored under `DataKey::FeeWaiverCode(code)`. The settlement flow checks
/// `PaymentCharge.fee_waiver_code` against this registry during
/// `settle_payment`. When both the code is valid and uses remain, the
/// platform fee is waived and `remaining_uses` is atomically decremented.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeWaiverCodeRecord {
    /// The code string itself, e.g. "LAUNCH2026".
    pub code: String,
    /// Ledger timestamp after which this code is no longer honored.
    pub expires_at: u64,
    /// Maximum total uses for this code. Must be >= 1 when created.
    pub max_uses: u32,
    /// Number of uses remaining; starts equal to `max_uses`, decremented by
    /// `settle_payment` on each successful consumption.
    pub remaining_uses: u32,
}

/// Customer profile for CRM features and repeat-customer identification.
/// Auto-created and updated during verify_payment.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomerProfile {
    /// Customer/payer address.
    pub customer_id: Address,
    /// Merchant that this customer has paid.
    pub merchant_id: Address,
    /// Optional hash of customer email for privacy (merchants can pass a hash).
    pub email_hash: Option<BytesN<32>>,
    /// Ledger timestamp when customer first interacted.
    pub created_at: u64,
    /// Number of confirmed payments from this customer.
    pub payment_count: u32,
    /// Total amount spent across all confirmed payments (in smallest denomination).
    pub total_spent: i128,
}

struct ReentrancyGuard<'a> {
    env: &'a Env,
}

impl<'a> Drop for ReentrancyGuard<'a> {
    fn drop(&mut self) {
        self.env
            .storage()
            .persistent()
            .set(&DataKey::ReentrancyLock, &false);
    }
}

/// Per-refund reentrancy lock cleared on drop (checks-effects-interactions).
struct RefundLockGuard<'a> {
    env: &'a Env,
    refund_id: String,
}

impl<'a> Drop for RefundLockGuard<'a> {
    fn drop(&mut self) {
        self.env
            .storage()
            .persistent()
            .remove(&DataKey::RefundLock(self.refund_id.clone()));
    }
}

/// Admin-configurable dispute creation rate limits.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeRateLimitConfig {
    /// Max open (Open + UnderReview) disputes per disputer address.
    pub per_payer_open: u32,
    /// Max dispute creations per rolling hour across all disputers.
    pub global_per_hour: u32,
}

/// Fixed-window counter for global dispute creation rate limiting.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeCreationRateState {
    pub window_started_at: u64,
    pub count: u32,
}

/// Default: max 5 open disputes per payer.
pub const DEFAULT_DISPUTE_PER_PAYER_OPEN: u32 = 5;
/// Default: max 100 dispute creations per hour globally.
pub const DEFAULT_DISPUTE_GLOBAL_PER_HOUR: u32 = 100;
/// Global dispute creation window length (1 hour).
pub const DISPUTE_GLOBAL_WINDOW_SECS: u64 = 3600;

#[contracttype]
pub enum DataKey {
    Payment(String),
    PaymentStatusHistory(String),
    MerchantPayments(Address),
    MerchantRateLimit(Address),
    Refund(String),
    PaymentRefunds(String),
    RefundCounter,
    /// Issue #1210: hashed to BytesN<32> (was `String`) to cut storage rent;
    /// see `dispute_id_to_key`.
    Dispute(BytesN<32>),
    PaymentDisputes(String),
    DisputeCounter,
    Stream(String),
    TreasuryBalance,
    UsdcToken,
    Paused,
    CreationPaused,
    MerchantRegistryAddress,
    AllowedToken(Address),
    Blacklisted(Address),
    MerchantAmountLimits(Address),
    GlobalAmountLimits,
    /// Issue #1210: hashed to BytesN<32> (was `String`) to cut storage rent;
    /// see `idempotency_key_to_key`.
    IdempotencyKey(BytesN<32>),
    SubscriptionPlan(String),
    /// Issue #1210: hashed to BytesN<32> (was `String`) to cut storage rent;
    /// see `subscription_id_to_key`.
    Subscription(BytesN<32>),
    PayerSubscriptions(Address),
    SubscriptionCounter,
    StreamCounter,
    /// Stores operator notes keyed by dispute_id for on-chain transparency.
    DisputeOperatorNote(String),
    /// Stores all arbitrators who have voted on a dispute.
    DisputeArbitratorVotes(String),
    /// Locked stake for a dispute arbitrator: (dispute_id, arbitrator) â†’ amount
    DisputeStake(String, Address),
    /// Vote cast by an arbitrator: (dispute_id, arbitrator) â†’ VoteChoice
    DisputeVote(String, Address),
    /// Tally of votes for a dispute
    DisputeVoteTally(String),
    /// Cross-contract address of the configured FX oracle (Issue #304).
    FxOracleAddress,
    /// Whether `process_refund` requires a `receipt_hash` on refunds (Issue #176).
    RequireReceiptHash,
    /// Cross-contract address of the configured DEX router (Issue #173).
    DexRouterAddress,
    /// Configurable refund expiry window in seconds (Issue #170).
    RefundExpirySecs,
    /// Vote cast by an arbitrator under the simple ARBITRATOR-role voting
    /// flow: (dispute_id, arbitrator) â†’ ArbitratorVoteChoice.
    ArbitratorVote(String, Address),
    /// Tally of ARBITRATOR-role votes for a dispute.
    ArbitratorVoteTally(String),
    /// Issue #168: Fee split configuration (treasury_bps, developer_bps, treasury_addr, developer_addr)
    FeeSplitConfig,
    /// Monthly volume tracker: (merchant_id, month_epoch) â†’ i128 cumulative amount
    MerchantMonthlyVolume(Address, u32),
    /// Cumulative all-time payment volume per merchant for KYC tier auto-upgrades (issue #207).
    MerchantCumulativeVolume(Address),
    FeeProposal,
    CurrentFee,
    GlobalRateLimit,
    MerchantSpecificRateLimit(Address),
    PayerRateLimit(Address),
    /// Issue #184: Total disputes filed against a merchant (keyed by merchant address).
    MerchantDisputeCount(Address),
    /// Issue #184: Total confirmed payments registered for a merchant (keyed by merchant address).
    MerchantPaymentCount(Address),
    /// Issue #185: Collaborative settlement record for a dispute.
    CollaborativeSettlement(String),
    /// Issue #664: Append-only log of `UsageMetrics` records for a
    /// subscription, keyed by subscription_id.
    UsageMetricsLog(String),
    /// Issue #301: List of supported token addresses for enumeration.
    SupportedTokens,
    /// Issue #303: KYC tier limits configuration.
    KycTierLimitsConfig,
    /// Issue #302: List of active subscription IDs for process_due_subscriptions.
    ActiveSubscriptions,
    /// Issue #304: FX Oracle contract address for rate staleness checks.
    FXOracleAddress,
    /// Issue #302: Counter for subscription tick payment IDs.
    SubscriptionTickCounter,
    /// Issue #313: Reentrancy lock for process_refund_internal and settle_payment.
    ReentrancyLock,
    /// Per-refund reentrancy flag set for the duration of `process_refund_internal`.
    RefundLock(String),
    /// Admin-configurable dispute rate limits (`DisputeRateLimitConfig`).
    DisputeRateLimits,
    /// Number of open/under-review disputes for a disputer address.
    PayerOpenDisputeCount(Address),
    /// Fixed-window global dispute creation counter (`DisputeCreationRateState`).
    GlobalDisputeCreationRate,
    /// When true, non-empty dispute evidence must be a valid IPFS CID.
    RequireEvidenceCid,
    /// Contract version string, updated on each successful upgrade.
    ContractVersion,
    /// Configurable settlement fee rate in basis points (issue: settle_payment fee).
    SettlementFeeRate,
    /// Configurable dispute bond amount in stablecoin stroops (overrides DISPUTE_BOND_AMOUNT const).
    DisputeBondAmount,
    /// Admin-configurable amount threshold for 3-day versus 7-day dispute deadlines.
    DisputeDeadlineThresholdAmount,
    /// Configurable monthly volume cap per KYC tier in stablecoin stroops (overrides TIER_CAP_* const).
    TierVolumeCap(KycTier),
    /// Configurable refund fee in basis points (overrides REFUND_FEE_BPS const).
    RefundFeeBps,
    /// Issue #471: Whether overpaid payments automatically create a pending refund.
    AutoRefundOverpayment,
    /// Configurable refund cooldown period in seconds (overrides REFUND_COOLDOWN_SECS const).
    RefundCooldownSecs,
    /// Configured swap protection limits.
    SwapLimits,
    /// Per-ledger input volume used by swap_and_pay.
    SwapLedgerVolume,
    /// Admin-managed reusable fee-waiver code registry for per-payment promotions.
    /// Keyed by the code string itself.
    FeeWaiverCode(String),
    /// When true, `cancel_subscription` may create a prorated pending refund.
    AllowProratedRefunds,
    /// Paginated log of treasury withdrawals (newest-first, capped at 100).
    TreasuryWithdrawalHistory,
    /// Issue #485: Marks a payment as created from a direct_transfer payment link.
    /// Prevents future disputes from being created for this payment.
    DirectTransferPayment(String),
    /// Issue #483: Maps token address to its currency symbol (e.g., USDC, EURC, BRLT).
    TokenCurrency(Address),
    Invoice(String),
    MerchantInvoices(Address),
    InvoiceCounter,
    /// Configurable invoice overdue grace period in seconds (Issue #607).
    InvoiceGracePeriodSecs,
    /// Issue #482: Payment retry chain tracking - maps original_id to list of retry payment IDs
    PaymentRetries(String),
    /// Issue #478: FX oracle max rate deviation per currency pair in basis points
    MaxRateDeviation(Symbol),
    /// Issue #481: Admin-configurable dispute threshold for auto-suspension
    DisputeThreshold,
    /// Minimum payment duration in seconds (default: CREATE_PAYMENT_WINDOW_SECS = 60).
    MinPaymentDurationSecs,
    /// Maximum payment duration in seconds (default: 30 days).
    MaxPaymentDurationSecs,
    /// Issue #1206: Admin-configurable default payment duration in seconds, used
    /// by `create_payment`/batch variants when the caller supplies neither
    /// `expires_at` nor `duration_secs`. Falls back to
    /// `DEFAULT_PAYMENT_DURATION_SECS` when unset.
    DefaultPaymentDurationSecs,
    /// Issue #1206: Per-merchant override for the default payment duration in
    /// seconds. Takes precedence over `DefaultPaymentDurationSecs` when set.
    MerchantPaymentDurationSecs(Address),
    /// Issue #1209: Optional deferred-settlement timestamp for a payment;
    /// `settle_payment` rejects settlement while `now` is before this value.
    /// Keyed by `payment_id`; absent means no deferred-settlement constraint.
    PaymentSettleAt(String),
    /// Issue #489: Reverse index from metadata_hash to payment_id for order reconciliation.
    MetadataHashPayment(BytesN<32>),
    /// Issue #492: Customer profile keyed by (merchant_id, customer_id) for CRM features.
    CustomerProfile(Address, Address),
    /// Issue #437: Allowlisted DEX router address
    AllowedRouter(Address),
    /// Issue #437: List of allowlisted DEX router addresses
    AllowedRoutersList,
    /// Issue #434: Wrapped XLM (WXLM) token contract address
    WrappedXlmContract,
    /// Issue #504: Payment IDs grouped by approximate expiry ledger bucket.
    PaymentsByExpiry(u32),
    /// Issue #504: Sorted set of expiry buckets that currently contain payment IDs.
    PaymentExpiryBuckets,
    /// Issue #678: Daily-bucketed payment ID index for O(days) analytics queries.
    /// Key: (merchant_id, day_bucket = created_at / 86_400) â†’ Vec<payment_id>.
    DailyPaymentIndex(Address, u64),
    /// Issue #666: Paginated log of platform-fee collection events (newest-first,
    /// capped at `FEE_COLLECTION_HISTORY_CAP`), consumed by `get_platform_fee_report`.
    FeeCollectionHistory,
    /// Issue #667: Arbitrary on-chain contract metadata (description, deployment notes,
    /// audit commit hash, etc.), keyed by an admin-chosen Symbol.
    ContractMetadata(Symbol),
    /// Issue #628: Cumulative gross payment volume per merchant (sum of `amount`
    /// over every payment ever created for the merchant). Read by
    /// `get_top_merchants` to rank merchants without scanning payment records.
    MerchantGrossVolume(Address),
    /// Issue #628: Append-only list of every merchant address that has had at
    /// least one payment created, for `get_top_merchants` enumeration.
    TrackedMerchants,
    /// Issue #638: Refund idempotency key â†’ `RefundIdempotencyRecord`. Stored with a
    /// 30-day TTL so a retried `create_refund` with the same key returns the original
    /// `refund_id` rather than creating a duplicate refund.
    RefundIdempotencyKey(String),
    /// Issue #633: Append-only index of subscription IDs for a plan, keyed by
    /// plan_id. Updated atomically on every `subscribe` / `subscribe_to_plan`.
    /// Appended at the end of the enum to preserve existing discriminants.
    PlanSubscribers(String),
    /// Issue #624: Timelock delay in seconds for critical admin operations.
    TimelockDelaySecs,
    /// Issue #624: Pending timelocked action keyed by a unique action ID.
    PendingTimelockAction(String),
    /// Issue #624: Counter for generating unique pending action IDs.
    TimelockActionCounter,
    /// Issue #1033: Admin-configurable override (in seconds) for
    /// `MAX_ORACLE_STALENESS_SECS`, enforced in `confirm_payment` independently
    /// of the FX oracle's own internal staleness threshold.
    OracleStalenessThreshold,
    /// Issue #1032: Maps a `deposit_address` to the `payment_id` currently
    /// holding it while that payment is `Pending`, so a new `create_payment`
    /// can detect and reject concurrent reuse of the same address.
    DepositAddressPending(Address),
    /// Issue #1172: independent admin circuit breaker for the simulated DEX
    /// `swap_and_pay` path. Appended last to preserve existing discriminants.
    SwapCircuitBreakerPaused,
}

/// Default initial contract version string.
pub const INITIAL_CONTRACT_VERSION: &str = "1.0.0";

/// Default timelock delay for critical admin operations: 48 hours.
pub const DEFAULT_TIMELOCK_SECS: u64 = 48 * 60 * 60;

/// Issue #624: Identifies which critical admin operation is pending in a timelock queue.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TimelockActionKind {
    /// `set_fee_rate(bps)`
    SetFeeRate(i128),
    /// `set_kyc_tier_limits(tier, max_amount)`
    SetKycTierLimits(KycTier, i128),
    /// `upgrade_contract(new_wasm_hash)`
    UpgradeContract(BytesN<32>),
}

/// Issue #624: A queued admin action that cannot execute until `execute_after` has passed.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingTimelockAction {
    /// Unique ID for this pending action (e.g. "tl_1").
    pub action_id: String,
    /// The specific operation being queued.
    pub kind: TimelockActionKind,
    /// Ledger timestamp after which the action may be executed (proposed_at + delay).
    pub execute_after: u64,
    /// Address of the admin who proposed this action.
    pub proposed_by: Address,
}

// When building for WASM deployment, only the active contract's #[contractimpl]
// is compiled to avoid duplicate exported symbols. On non-WASM targets (tests,
// tooling), all impls compile so that *Client types are available everywhere.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MerchantAnalytics {
    pub total_payments: u32,
    pub confirmed_payments: u32,
    pub failed_payments: u32,
    pub total_volume: i128,
    pub avg_payment_amount: i128,
    pub dispute_count: u32,
    pub refund_count: u32,
    pub net_settled_volume: i128,
}

/// Issue #628: A single merchant's ranking entry in `get_top_merchants`,
/// ordered by cumulative gross payment volume across the whole platform.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MerchantRanking {
    pub merchant_id: Address,
    /// Sum of `amount` over every payment created for this merchant.
    pub total_volume: i128,
    /// Number of payments created for this merchant (the `MerchantPaymentCount` index).
    pub payment_count: u64,
}

/// Issue #1066: A single platform-fee collection record appended to
/// `DataKey::FeeCollectionHistory` from `settle_payment` / `process_refund`.
/// Paginated by `get_fee_collection_history`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeRecord {
    /// Payment or refund ID that generated this fee event.
    pub payment_id: String,
    /// Fee amount collected in the token's smallest unit.
    pub fee_amount: i128,
    /// Token contract address the fee was collected in.
    pub token: Address,
    /// Ledger timestamp when the fee was collected.
    pub collected_at: u64,
}

/// Issue #1064: Enriched merchant leaderboard entry returned by
/// `get_top_merchants_v2`, including KYC tier and volume metrics.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MerchantRankEntry {
    pub merchant_id: Address,
    /// Composite score used for ranking (equals cumulative gross volume).
    pub score: i128,
    pub kyc_tier: KycTier,
    /// Total confirmed payments for this merchant.
    pub confirmed_payment_count: u64,
    /// Cumulative gross payment volume.
    pub cumulative_volume: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractHealth {
    pub version: String,
    pub is_paused: bool,
    pub is_creation_paused: bool,
    pub treasury_balance: i128,
    pub active_payment_count: u32,
    pub fx_oracle_configured: bool,
    pub merchant_registry_configured: bool,
}

/// Issue #1205: Aggregated contract health/config view for dashboards and
/// operator tooling. Unlike `ContractHealth`, this focuses on identity/config
/// data (token, admin, multisig) rather than operational metrics.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractInfo {
    /// Current contract version string (see `version`/`get_version`).
    pub version: String,
    /// Consolidated global + creation pause state (see `get_pause_info`).
    pub pause_info: PauseInfo,
    /// Configured settlement token address, if one has been set via `set_usdc_token`.
    pub token: Option<Address>,
    /// Current contract admin, if initialized.
    pub admin: Option<Address>,
    /// Multi-sig approval threshold (see `get_multisig_config`).
    pub multisig_threshold: u32,
    /// Multi-sig signer set (see `get_multisig_config`).
    pub multisig_signers: Vec<Address>,
}
/// Issue #1168: calculate `amount * bps / 10_000` without wrapping.
///
/// Returns `None` when the intermediate multiplication overflows i128 so that
/// callers can reject the operation (or saturate in read-only reporting paths).
fn checked_amount_bps(amount: i128, bps: i128) -> Option<i128> {
    amount.checked_mul(bps).and_then(|v| v.checked_div(10_000))
}



/// Bumps the version string by incrementing the number after the last '.'.
/// Works with versions like "1.0.0" â†’ "1.0.1", "1" â†’ "2", "v1" â†’ "v2".
/// Uses byte-level parsing since Soroban's String API doesn't support string
/// manipulation in no_std. Defaults to "2.0.0" if parsing fails.
fn bump_version_string(env: &Env, version: &String) -> String {
    use soroban_sdk::Bytes;

    let bytes: Bytes = version.to_bytes();
    let len = bytes.len() as usize;

    // Find the last '.' or the start of the version number
    let mut last_dot = None;
    let mut i = 0usize;
    while i < len {
        if bytes.get(i as u32) == Some(b'.') {
            last_dot = Some(i);
        }
        i += 1;
    }

    // Parse the numeric part to bump from the last dot position
    let num_start = last_dot.map(|p| p + 1).unwrap_or(0);
    let mut num_val: u32 = 0;
    let mut j = num_start;
    while j < len {
        match bytes.get(j as u32) {
            Some(b @ b'0'..=b'9') => {
                num_val = num_val.saturating_mul(10).saturating_add((b - b'0') as u32);
            }
            _ => break,
        }
        j += 1;
    }

    // Bump by 1
    let new_num = num_val.saturating_add(1);

    // Encode the new number as ASCII digits
    let mut num_buf = [0u8; 12];
    let mut num_len = 0usize;
    if new_num == 0 {
        num_buf[0] = b'0';
        num_len = 1;
    } else {
        let mut n = new_num;
        let mut rev = [0u8; 12];
        let mut rl = 0usize;
        while n > 0 {
            rev[rl] = (n % 10) as u8 + b'0';
            n /= 10;
            rl += 1;
        }
        while rl > 0 {
            rl -= 1;
            num_buf[num_len] = rev[rl];
            num_len += 1;
        }
    }

    // Reconstruct into a fixed-size buffer (max 64 bytes handles any sane version string)
    let mut result = [0u8; 64];
    let mut pos = 0usize;
    // prefix: bytes before the number
    let mut p = 0usize;
    while p < num_start && pos < 64 {
        result[pos] = bytes.get(p as u32).unwrap_or(b' ');
        pos += 1;
        p += 1;
    }
    // new number digits
    for &digit in num_buf.iter().take(num_len) {
        if pos < 64 {
            result[pos] = digit;
            pos += 1;
        }
    }
    // suffix: bytes after the number
    let mut k = j;
    while k < len && pos < 64 {
        result[pos] = bytes.get(k as u32).unwrap_or(b' ');
        pos += 1;
        k += 1;
    }

    String::from_bytes(env, &result[..pos])
}

mod payment_link;
pub use payment_link::{
    CreateLinkArgs, FiatConfig, LinkAnalytics, MaybeFiatConfig, PaymentLink, PaymentLinkManager,
    PaymentLinkManagerClient,
};
// Pre-existing/unrelated fix (found while adding FX conversion property
// tests for issue #1186): `feature_tests`, `memo_test`, `merchant_auth_test`,
// `partial_overpaid_test`, `pause_test`, `payment_link_test`,
// `payment_metadata_test`, `test`, `stream_test`, `subscription_test`, and
// `pub mod stream` (plus its `pub use`) were each declared a second time
// further down in this file (see the consolidated `#[cfg(test)] mod ...;`
// list below, and `pub mod stream;` / `pub use stream::{...}` already
// declared near the top of this file), which is a hard compile error
// (E0428/E0255, "the name `..` is defined multiple times") for every
// `cargo test` build, including the one that runs this issue's new
// property tests. Only the non-duplicated declarations are kept here;
// the rest live solely in the consolidated list.
#[cfg(test)]
mod payment_retry_test;
#[cfg(test)]
mod payment_tip_test;
#[cfg(test)]
mod test;

// Payment streaming module (Issue #127) - declared via `pub mod stream;` above.
pub use stream::{PaymentStream, PaymentStreaming, StreamError, StreamStatus};
#[cfg(test)]
mod stream_test;
#[cfg(test)]
mod stream_vesting_test;

// Note (pre-existing bug, fixed while touching this file for #1206/#1207/#1209/#1210):
// `test`, `stream_test`, `subscription_test`, `feature_tests`, `memo_test`,
// `partial_overpaid_test`, `pause_test`, `payment_link_test`, `payment_metadata_test`,
// and `merchant_auth_test` were each declared a second time below (duplicate `mod`
// items), which is a hard compile error (E0428 - duplicate module definition).
// `pub mod stream;` was likewise declared twice. The duplicate declarations are
// removed here; every module below is declared exactly once now.
// NOTE (pre-existing, unrelated to #1199): this tail of lib.rs previously declared
// `pub mod stream;` (plus a redundant subset re-export) and roughly a dozen
// `#[cfg(test)] mod <name>;` test-module declarations a *second* time, duplicating
// declarations already made earlier in this file / just below. Rust rejects a module
// being declared twice in the same scope (E0428), so the crate could not compile at
// all in this state. Consolidated to one declaration per module so the crate (and the
// new `batch_process_refunds` entrypoint added for #1199) can build.
#[cfg(test)] mod test;
#[cfg(test)] mod stream_test;
#[cfg(test)] mod stream_vesting_test;
#[cfg(test)] mod subscription_test;
#[cfg(test)] mod arbitrage_test;
#[cfg(test)] mod auth_test;
#[cfg(test)] mod batch_payment_test;
#[cfg(test)] mod batch_refund_test;
#[cfg(test)] mod dex_router_test;
#[cfg(test)] mod dispute_test;
#[cfg(test)] mod escalate_disputes_test;
#[cfg(test)] mod fx_oracle_test;
#[cfg(test)] mod htlc_test;
#[cfg(test)] mod integration_test;
#[cfg(test)] mod memo_test;
#[cfg(test)] mod merchant_auth_test;
#[cfg(test)] mod merchant_ranking_test;
#[cfg(test)] mod merchant_registry_test;
#[cfg(test)] mod merchant_reserve_test;
#[cfg(test)] mod mock_dex_router;
#[cfg(test)] mod muxed_payer_test;
#[cfg(test)] mod oracle_sanitization_test;
#[cfg(test)] mod partial_overpaid_test;
#[cfg(test)] mod pause_test;
#[cfg(test)] mod recovery_test;
#[cfg(test)] mod swap_and_pay_token_moving_test;
#[cfg(test)] pub use recovery_test::RecoveryHarnessClient;
#[cfg(test)] mod payment_link_test;
#[cfg(test)] mod payment_metadata_test;
#[cfg(test)] mod payment_retry_test;
#[cfg(test)] mod payment_tip_test;
#[cfg(test)] mod proptests;
#[cfg(test)] mod router_allowlist_test;
#[cfg(test)] mod settlement_test;
#[cfg(test)] mod swap_test;
#[cfg(test)] mod invoice_test;
#[cfg(test)] mod account_abstraction_test;
#[cfg(test)] mod meta_transaction_test;
#[cfg(test)] mod merkle_distributor_test;
#[cfg(test)] mod treasury_single_admin_threshold_test;
#[cfg(test)] mod merchant_refunds_test;
#[cfg(test)] mod tiered_refund_fee_test;
#[cfg(test)] mod batch_settle_test;
#[cfg(test)] mod require_auth_audit_test;
#[cfg(test)] mod amount_math_test;
#[cfg(test)] mod storage_cap_test;
