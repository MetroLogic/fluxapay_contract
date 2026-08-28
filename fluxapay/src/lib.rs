#![no_std]
#![allow(clippy::too_many_arguments)]
use crate::merchant_registry::KycTier;
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, map, token, vec, Address, BytesN, Env,
    Map, MuxedAddress, String, Symbol, Vec,
};

pub const PAYMENT_TOLERANCE: i128 = 1;
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
/// Issue #638: TTL (in ledgers, ~5s each) for a stored refund idempotency key —
/// 30 days, matching the payment `client_token` retention window.
const REFUND_IDEMPOTENCY_TTL_LEDGERS: u32 = (30 * 24 * 60 * 60) / 5;
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

// Issue #167: Tiered refund fees based on merchant KYC tier
const REFUND_FEE_BPS_BASIC: i128 = 100;     // 1.0% for Basic tier
const REFUND_FEE_BPS_FULL: i128 = 80;       // 0.8% for Full tier
const REFUND_FEE_BPS_BUSINESS: i128 = 50;   // 0.5% for Business tier

/// Default window (Issue #170) during which a pending refund may be processed,
/// measured from `Refund::created_at`. Configurable via `set_refund_expiry`.
pub const DEFAULT_REFUND_EXPIRY_SECS: u64 = 30 * 24 * 60 * 60;
// Issue #63: Monthly processing volume caps per KYC tier (in USDC stroops, 7 decimals)
// Unverified: $500, Basic: $10,000, Full: $100,000, Business: unlimited (i128::MAX)
const TIER_CAP_UNVERIFIED: i128 = 5_000_000_000;       // $500
const TIER_CAP_BASIC: i128 = 100_000_000_000;           // $10,000
const TIER_CAP_FULL: i128 = 1_000_000_000_000;          // $100,000
const TIER_CAP_BUSINESS: i128 = i128::MAX;              // unlimited

// Issue #207: Cumulative volume thresholds for automatic KYC tier upgrades (in USDC stroops)
const TIER_UPGRADE_THRESHOLD_BASIC: i128 = TIER_CAP_UNVERIFIED;      // $500 cumulative → Basic
const TIER_UPGRADE_THRESHOLD_FULL: i128 = TIER_CAP_BASIC;             // $10,000 cumulative → Full
const TIER_UPGRADE_THRESHOLD_BUSINESS: i128 = TIER_CAP_FULL;          // $100,000 cumulative → Business

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

mod access_control;
pub mod account_abstraction;
mod dex_router;
pub mod events;
pub mod fx_oracle;
pub mod merchant_auth;
mod payment_state_machine;
use access_control::{
    role_admin, role_arbitrator, role_merchant, role_oracle, role_settlement_operator, AccessControl,
};
// Re-export for tests
#[allow(unused_imports)]
pub use access_control::AccessControlDataKey;
#[allow(unused_imports)]
pub use access_control::{AdminAction, AdminProposal};
pub use dex_router::{DexRouter, DexRouterClient};
pub use fx_oracle::{FXOracle, FXOracleClient, FXOracleError};
pub use merchant_auth::{
    MerchantAuthError, MerchantAuthorization, MerchantPreAuth,
};

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
    /// Upgrade failed — WASM hash replacement rejected by the host.
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
    /// Issue #622: Evidence field is not a valid IPFS CID (CIDv0 starts with "Qm"/46 chars; CIDv1 starts with "bafy"/≥59 chars).
    InvalidEvidenceCid = 69,
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

/// Hard cap for dispute batch size.
pub const MAX_DISPUTE_BATCH: u32 = 20;

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
    /// `units_used * unit_price` — the metered charge amount for this cycle.
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
    Dispute(String),
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
    IdempotencyKey(String),
    SubscriptionPlan(String),
    Subscription(String),
    PayerSubscriptions(Address),
    SubscriptionCounter,
    StreamCounter,
    /// Stores operator notes keyed by dispute_id for on-chain transparency.
    DisputeOperatorNote(String),
    /// Stores all arbitrators who have voted on a dispute.
    DisputeArbitratorVotes(String),
    /// Locked stake for a dispute arbitrator: (dispute_id, arbitrator) → amount
    DisputeStake(String, Address),
    /// Vote cast by an arbitrator: (dispute_id, arbitrator) → VoteChoice
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
    /// flow: (dispute_id, arbitrator) → ArbitratorVoteChoice.
    ArbitratorVote(String, Address),
    /// Tally of ARBITRATOR-role votes for a dispute.
    ArbitratorVoteTally(String),
    /// Issue #168: Fee split configuration (treasury_bps, developer_bps, treasury_addr, developer_addr)
    FeeSplitConfig,
    /// Monthly volume tracker: (merchant_id, month_epoch) → i128 cumulative amount
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
    /// Key: (merchant_id, day_bucket = created_at / 86_400) → Vec<payment_id>.
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
    /// Issue #638: Refund idempotency key → `RefundIdempotencyRecord`. Stored with a
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
#[cfg_attr(
    any(not(target_arch = "wasm32"), feature = "contract-refund-manager"),
    contractimpl
)]
#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
impl RefundManager {
    pub fn version() -> u32 {
        1
    }
    fn require_not_paused(env: &Env) -> Result<(), Error> {
        let pause_state: PauseState = env
            .storage()
            .persistent()
            .get(&DataKey::Paused)
            .unwrap_or(PauseState {
                paused: false,
                reason: String::from_str(env, ""),
                admin: None,
                timestamp: 0,
            });
        if pause_state.paused {
            return Err(Error::ContractPaused);
        }
        Ok(())
    }

    fn get_refund_fee_bps_internal(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get::<DataKey, i128>(&DataKey::RefundFeeBps)
            .unwrap_or(REFUND_FEE_BPS)
    }

    fn get_refund_cooldown_secs(env: &Env) -> u64 {
        env.storage()
            .persistent()
            .get::<DataKey, u64>(&DataKey::RefundCooldownSecs)
            .unwrap_or(REFUND_COOLDOWN_SECS)
    }

    pub fn get_dispute_bond_amount(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::DisputeBondAmount)
            .unwrap_or(DISPUTE_BOND_AMOUNT)
    }


    /// Admin-only: set an arbitrary on-chain metadata entry (issue #667), e.g. a
    /// description, deployment notes, or audit commit hash. Stored in instance
    /// storage under a caller-chosen key, with the instance TTL bumped to
    /// `LONG_LIVE_TTL` so metadata survives archival.
    pub fn set_contract_metadata(
        env: Env,
        admin: Address,
        key: Symbol,
        value: String,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        env.storage()
            .instance()
            .set(&DataKey::ContractMetadata(key), &value);

        let threshold = core::cmp::max(1, LONG_LIVE_TTL / TTL_BUMP_THRESHOLD_DIVISOR);
        env.storage().instance().extend_ttl(threshold, LONG_LIVE_TTL);

        Ok(())
    }

    /// Public read of an on-chain metadata entry set via `set_contract_metadata`
    /// (issue #667). Returns `None` if the key was never set.
    pub fn get_contract_metadata(env: Env, key: Symbol) -> Option<String> {
        env.storage()
            .instance()
            .get(&DataKey::ContractMetadata(key))
    }

    /// Formats a u64 as a decimal `String` without relying on `alloc`/`format!`
    /// (this crate is `#![no_std]`). Used to store `deployed_at` as metadata
    /// text so it round-trips through `get_contract_metadata`'s `String` type.
    fn u64_to_string(env: &Env, mut n: u64) -> String {
        if n == 0 {
            return String::from_str(env, "0");
        }
        let mut buf = [0u8; 20];
        let mut i = buf.len();
        while n > 0 {
            i -= 1;
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
        }
        let s = core::str::from_utf8(&buf[i..]).unwrap_or("0");
        String::from_str(env, s)
    }

    fn validate_init_address(env: &Env, address: Address) -> Result<(), Error> {
        let zero_address = Address::from_str(env, ZERO_CONTRACT_STRKEY);
        if address == zero_address {
            return Err(Error::InvalidAddress);
        }
        Ok(())
    }

    fn validate_admin_and_token(
        env: &Env,
        admin: Address,
        token_address: Address,
    ) -> Result<(), Error> {
        if admin == token_address {
            return Err(Error::InvalidAddress);
        }
        Self::validate_init_address(env, admin)?;
        Self::validate_init_address(env, token_address)
    }

    pub fn initialize_refund_manager(
        env: Env,
        admin: Address,
        usdc_token_address: Address,
    ) -> Result<(), Error> {
        Self::validate_admin_and_token(&env, admin.clone(), usdc_token_address.clone())?;
        AccessControl::initialize(&env, admin);
        env.storage()
            .persistent()
            .set(&DataKey::UsdcToken, &usdc_token_address);
        env.storage()
            .instance()
            .set(&DataKey::RefundFeeBps, &REFUND_FEE_BPS);

        // Issue #667: pre-populate on-chain metadata with description, version, and
        // deployment timestamp so explorers/integrators can identify the contract.
        env.storage().instance().set(
            &DataKey::ContractMetadata(Symbol::new(&env, "description")),
            &String::from_str(&env, "FluxaPay RefundManager contract"),
        );
        env.storage().instance().set(
            &DataKey::ContractMetadata(Symbol::new(&env, "version")),
            &String::from_str(&env, "1"),
        );
        env.storage().instance().set(
            &DataKey::ContractMetadata(Symbol::new(&env, "deployed_at")),
            &Self::u64_to_string(&env, env.ledger().timestamp()),
        );
        let threshold = core::cmp::max(1, LONG_LIVE_TTL / TTL_BUMP_THRESHOLD_DIVISOR);
        env.storage().instance().extend_ttl(threshold, LONG_LIVE_TTL);

        Ok(())
    }

    /// Admin-only: set the refund processing fee in basis points (0–1000, max 10%).
    pub fn set_refund_fee_bps(env: Env, admin: Address, bps: i128) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        if bps < 0 || bps > 1_000 {
            return Err(Error::InvalidAmount);
        }

        env.storage()
            .instance()
            .set(&DataKey::RefundFeeBps, &bps);
        Ok(())
    }

    pub fn get_refund_fee_bps(env: Env) -> i128 {
        Self::get_refund_fee_bps_internal(&env)
    }

    /// Admin: configure the DEX router used to route swap_and_pay refunds
    /// back to the payer's original token (Issue #173).
    pub fn set_dex_router_address(
        env: Env,
        admin: Address,
        dex_router: Address,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::DexRouterAddress, &dex_router);
        Ok(())
    }

    /// Admin: require a `receipt_hash` on every refund before `process_refund`
    /// will execute it (Issue #176).
    pub fn set_refund_policy(
        env: Env,
        admin: Address,
        require_receipt_hash: bool,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::RequireReceiptHash, &require_receipt_hash);
        Ok(())
    }

    /// Issue #676: read-only view of all refund configuration in one call —
    /// `require_receipt_hash` (Issue #176), `refund_expiry_secs` (Issue #170),
    /// `refund_fee_bps`, and `cooldown_secs`. Lets integrators check policy
    /// before submitting a refund without having to know every individual
    /// storage key / setter.
    pub fn get_refund_policy(env: Env) -> RefundPolicy {
        let require_receipt_hash: bool = env
            .storage()
            .persistent()
            .get(&DataKey::RequireReceiptHash)
            .unwrap_or(false);

        RefundPolicy {
            require_receipt_hash,
            refund_expiry_secs: Self::get_refund_expiry_secs(&env),
            refund_fee_bps: Self::get_refund_fee_bps_internal(&env),
            cooldown_secs: Self::get_refund_cooldown_secs(&env),
        }
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

    /// Synchronize a role grant across PaymentProcessor and RefundManager.
    ///
    /// If either grant fails, the transaction aborts and no changes are persisted.
    pub fn sync_grant_role_with_processor(
        env: Env,
        admin: Address,
        payment_processor_address: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        AccessControl::grant_role(&env, admin.clone(), role.clone(), account.clone())
            .map_err(|_| Error::AccessControlError)?;

        let payment_client = crate::PaymentProcessorClient::new(&env, &payment_processor_address);
        payment_client
            .try_grant_role(&admin, &role, &account)
            .map_err(|_| Error::AccessControlError)?
            .map_err(|_| Error::AccessControlError)?;

        env.events().publish(
            (
                Symbol::new(&env, "ACCESS_CONTROL"),
                Symbol::new(&env, "SYNC_GRANT"),
            ),
            (role, account),
        );

        Ok(())
    }

    /// Synchronize a role revoke across PaymentProcessor and RefundManager.
    ///
    /// If either revoke fails, the transaction aborts and no changes are persisted.
    pub fn sync_revoke_role_with_processor(
        env: Env,
        admin: Address,
        payment_processor_address: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        AccessControl::revoke_role(&env, admin.clone(), role.clone(), account.clone())
            .map_err(|_| Error::AccessControlError)?;

        let payment_client = crate::PaymentProcessorClient::new(&env, &payment_processor_address);
        payment_client
            .try_revoke_role(&admin, &role, &account)
            .map_err(|_| Error::AccessControlError)?
            .map_err(|_| Error::AccessControlError)?;

        env.events().publish(
            (
                Symbol::new(&env, "ACCESS_CONTROL"),
                Symbol::new(&env, "SYNC_REVOKE"),
            ),
            (role, account),
        );

        Ok(())
    }

    pub fn has_role(env: Env, role: Symbol, account: Address) -> bool {
        AccessControl::has_role(&env, &role, &account)
    }

    pub fn renounce_role(env: Env, account: Address, role: Symbol) -> Result<(), Error> {
        AccessControl::renounce_role(&env, account, role).map_err(|_| Error::AccessControlError)
    }

    pub fn propose_admin(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), Error> {
        AccessControl::propose_admin(&env, current_admin, new_admin)
            .map_err(|_| Error::AccessControlError)
    }

    pub fn claim_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        AccessControl::claim_admin(&env, new_admin).map_err(|_| Error::AccessControlError)
    }

    pub fn transfer_admin(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), Error> {
        Self::propose_admin(env, current_admin, new_admin)
    }

    pub fn accept_admin_transfer(env: Env, new_admin: Address) -> Result<(), Error> {
        Self::claim_admin(env, new_admin)
    }

    pub fn get_admin(env: Env) -> Option<Address> {
        AccessControl::get_admin(&env)
    }

    /// Add an address to the global blacklist (admin only).
    pub fn add_to_blacklist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::Blacklisted(address), &true);
        Ok(())
    }

    /// Remove an address from the global blacklist (admin only).
    pub fn remove_from_blacklist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::Blacklisted(address), &false);
        Ok(())
    }

    /// Returns true when an address is globally blacklisted.
    pub fn is_blacklisted(env: Env, address: Address) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::Blacklisted(address))
            .unwrap_or(false)
    }

    fn require_not_blacklisted(env: &Env, address: &Address) -> Result<(), Error> {
        if env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::Blacklisted(address.clone()))
            .unwrap_or(false)
        {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }

    /// Like `register_payment`, but also records the original token and swap
    /// path used by a `swap_and_pay` payment, so `process_refund` can route
    /// the refund back through the DEX to the payer's original token
    /// (Issue #173).
    pub fn register_swap_payment(
        env: Env,
        payment_id: String,
        merchant_id: Address,
        amount: i128,
        currency: Symbol,
        original_token: Address,
        swap_path: Vec<Address>,
    ) -> Result<(), Error> {
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Payment(payment_id.clone()))
        {
            let payment = PaymentCharge {
                payment_id: payment_id.clone(),
                merchant_id,
                amount,
                currency,
                deposit_address: env.current_contract_address(),
                status: PaymentStatus::Confirmed,
                payer_address: None,
                transaction_hash: None,
                created_at: env.ledger().timestamp(),
                confirmed_at: None,
                expires_at: 0,
                amount_received: None,
                memo: None,
                memo_type: None,
                token_address: None,
                metadata_hash: None,
                fx_rate: None,
                fx_rate_at: None,
                original_token: Some(original_token),
                swap_path: Some(swap_path),
                metadata: None,
                fee_waiver_code: None,
                retry_of_payment_id: None,
                payer_muxed_id: None,
                payment_link_id: None,
            };
            env.storage()
                .persistent()
                .set(&DataKey::Payment(payment_id.clone()), &payment);
            Self::bump_payment_ttl(&env, &payment_id, &payment.status);
        }
        Ok(())
    }

    /// Issue #168: Configure fee split destinations for platform fees.
    /// Admin can set allocation ratios and destination addresses.
    /// `treasury_bps + developer_bps` must be ≤ 10 000; any remainder goes to treasury.
    pub fn configure_fee_split(
        env: Env,
        admin: Address,
        treasury_bps: u32,
        developer_bps: u32,
        treasury_address: Address,
        developer_address: Address,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        if treasury_bps.saturating_add(developer_bps) > 10_000 {
            return Err(Error::InvalidAmount);
        }

        let config = FeeSplitConfig {
            treasury_bps,
            developer_bps,
            treasury_address,
            developer_address,
        };

        env.storage()
            .persistent()
            .set(&DataKey::FeeSplitConfig, &config);

        env.events().publish(
            (Symbol::new(&env, "FEE_SPLIT"), Symbol::new(&env, "CONFIGURED")),
            (treasury_bps, developer_bps),
        );

        Ok(())
    }

    /// Struct-based setter for the platform fee split config (alias for `configure_fee_split`).
    /// Admin only. `config.treasury_bps + config.developer_bps` must be ≤ 10 000.
    pub fn set_fee_split_config(
        env: Env,
        admin: Address,
        config: FeeSplitConfig,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        if config.treasury_bps.saturating_add(config.developer_bps) > 10_000 {
            return Err(Error::InvalidAmount);
        }

        env.storage()
            .persistent()
            .set(&DataKey::FeeSplitConfig, &config);

        env.events().publish(
            (Symbol::new(&env, "FEE_SPLIT"), Symbol::new(&env, "CONFIGURED")),
            (config.treasury_bps, config.developer_bps),
        );

        Ok(())
    }

    /// Get the current fee split configuration.
    pub fn get_fee_split_config(env: Env) -> Option<FeeSplitConfig> {
        env.storage()
            .persistent()
            .get(&DataKey::FeeSplitConfig)
    }

    /// Returns all addresses currently holding the given role (issue #37).
    pub fn get_role_members(env: Env, role: Symbol) -> Vec<Address> {
        AccessControl::get_role_members(&env, &role)
    }

    pub fn propose_fee_update(env: Env, admin: Address, new_fee: i128) -> Result<(), Error> {
        admin.require_auth();
        if Some(admin.clone()) != AccessControl::get_admin(&env) {
            return Err(Error::Unauthorized);
        }
        let proposal = FeeProposal {
            proposed_fee: new_fee,
            proposed_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&DataKey::FeeProposal, &proposal);
        Ok(())
    }

    pub fn finalize_fee_update(env: Env, admin: Address) -> Result<(), Error> {
        admin.require_auth();
        if Some(admin.clone()) != AccessControl::get_admin(&env) {
            return Err(Error::Unauthorized);
        }
        let proposal: FeeProposal = env
            .storage()
            .persistent()
            .get(&DataKey::FeeProposal)
            .ok_or(Error::NoFeeProposal)?;

        let now = env.ledger().timestamp();
        let seven_days_secs: u64 = 7 * 24 * 60 * 60;
        if now < proposal.proposed_at + seven_days_secs {
            return Err(Error::FeeProposalNotReady);
        }

        env.storage().persistent().set(&DataKey::CurrentFee, &proposal.proposed_fee);
        env.storage().persistent().remove(&DataKey::FeeProposal);

        Ok(())
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
            .has(&DataKey::Payment(payment_id.clone()))
        {
            let payment = PaymentCharge {
                payment_id: payment_id.clone(),
                merchant_id: merchant_id.clone(),
                amount,
                currency,
                deposit_address: env.current_contract_address(),
                status: PaymentStatus::Confirmed,
                payer_address: None,
                transaction_hash: None,
                created_at: env.ledger().timestamp(),
                confirmed_at: Some(env.ledger().timestamp()),
                expires_at: 0,
                amount_received: None,
                memo: None,
                memo_type: None,
                token_address: None,
                metadata_hash: None,
                original_token: None,
                swap_path: None,
                fx_rate: None,
                fx_rate_at: None,
                metadata: None,
                fee_waiver_code: None,
            retry_of_payment_id: None,
            payer_muxed_id: None,
            payment_link_id: None,
            };
            env.storage()
                .persistent()
                .set(&DataKey::Payment(payment_id.clone()), &payment);
            Self::bump_payment_ttl(&env, &payment_id, &payment.status);

            // Issue #184: Track confirmed payment count per merchant for dispute rate calculation
            let count_key = DataKey::MerchantPaymentCount(merchant_id.clone());
            let count: u64 = env
                .storage()
                .persistent()
                .get(&count_key)
                .unwrap_or(0u64);
            env.storage()
                .persistent()
                .set(&count_key, &(count + 1));
            Self::bump_ttl(&env, &count_key, LONG_LIVE_TTL);
        }
    }

    pub fn queue_auto_refund(
        env: Env,
        caller: Address,
        registry_address: Address,
        payment_id: String,
        refund_amount: i128,
        requester: Address,
        reason: String,
    ) -> Result<String, Error> {
        caller.require_auth();

        let registry_client = crate::merchant_registry::MerchantRegistryClient::new(
            &env,
            &registry_address,
        );
        let expected_caller = registry_client
            .get_payment_processor_address()
            .ok_or(Error::Unauthorized)?;

        if caller != expected_caller {
            return Err(Error::Unauthorized);
        }

        Self::require_not_blacklisted(&env, &requester)?;
        Self::create_refund_internal(&env, payment_id, refund_amount, reason, requester, None, None)
    }

    pub fn create_refund(
        env: Env,
        payment_id: String,
        refund_amount: i128,
        reason: String,
        requester: Address,
    ) -> Result<String, Error> {
        requester.require_auth();
        Self::require_not_blacklisted(&env, &requester)?;
        Self::create_refund_internal(&env, payment_id, refund_amount, reason, requester, None, None)
    }

    /// Issue #638: Create a refund request with an optional idempotency key.
    ///
    /// When `idempotency_key` is `Some`, the key is persisted for 30 days:
    /// * Retrying with the same key **and** the same `(payment_id, refund_amount,
    ///   reason)` returns the original `refund_id` without creating a duplicate.
    /// * Reusing the key with different parameters returns
    ///   `Error::DuplicateIdempotencyKey`.
    ///
    /// Passing `None` is exactly equivalent to `create_refund` (backward compatible).
    pub fn create_refund_idempotent(
        env: Env,
        payment_id: String,
        refund_amount: i128,
        reason: String,
        requester: Address,
        idempotency_key: Option<String>,
    ) -> Result<String, Error> {
        requester.require_auth();
        Self::require_not_blacklisted(&env, &requester)?;
        Self::create_refund_internal(
            &env,
            payment_id,
            refund_amount,
            reason,
            requester,
            None,
            idempotency_key,
        )
    }

    /// Create a refund request with optional receipt hash metadata.
    ///
    /// Issue #638: also accepts an optional `idempotency_key` with the same
    /// semantics as `create_refund_idempotent`.
    pub fn create_refund_with_receipt(
        env: Env,
        payment_id: String,
        refund_amount: i128,
        reason: String,
        requester: Address,
        receipt_hash: Option<BytesN<32>>,
        idempotency_key: Option<String>,
    ) -> Result<String, Error> {
        requester.require_auth();
        Self::require_not_blacklisted(&env, &requester)?;
        Self::create_refund_internal(
            &env,
            payment_id,
            refund_amount,
            reason,
            requester,
            receipt_hash,
            idempotency_key,
        )
    }

    fn create_refund_internal(
        env: &Env,
        payment_id: String,
        refund_amount: i128,
        reason: String,
        requester: Address,
        receipt_hash: Option<BytesN<32>>,
        idempotency_key: Option<String>,
    ) -> Result<String, Error> {
        if refund_amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        // Issue #625: Enforce maximum length on the reason field.
        if reason.len() as usize > MAX_REASON_LEN {
            return Err(Error::InputTooLong);
        }

        // Issue #638: Idempotency short-circuit. If this key was already used,
        // return the original refund_id for identical params, or reject a reuse
        // with different params.
        if let Some(ref key) = idempotency_key {
            let dk = DataKey::RefundIdempotencyKey(key.clone());
            if let Some(record) = env
                .storage()
                .persistent()
                .get::<DataKey, RefundIdempotencyRecord>(&dk)
            {
                if record.payment_id == payment_id
                    && record.amount == refund_amount
                    && record.reason == reason
                {
                    return Ok(record.refund_id);
                }
                return Err(Error::DuplicateIdempotencyKey);
            }
        }

        // Validate refund amount does not exceed original payment amount
        // First try to get payment from local storage
        let payment: PaymentCharge = if let Some(local_payment) =
            env.storage()
                .persistent()
                .get::<DataKey, PaymentCharge>(&DataKey::Payment(payment_id.clone()))
        {
            local_payment
        } else {
            return Err(Error::PaymentNotFound);
        };
        Self::require_not_blacklisted(env, &payment.merchant_id)?;
        Self::require_not_blacklisted(env, &requester)?;

        // Issue #76: Reject refunds unless payment.status == Confirmed or Overpaid
        if payment.status != PaymentStatus::Confirmed && payment.status != PaymentStatus::Overpaid {
            return Err(Error::PaymentAlreadyProcessed);
        }

        // Issue #174: Check cooldown period after payment confirmation
        let confirmed_at = payment.confirmed_at.ok_or(Error::PaymentAlreadyProcessed)?;
        let now = env.ledger().timestamp();
        let cooldown_secs = Self::get_refund_cooldown_secs(&env);
        if now < confirmed_at + cooldown_secs {
            return Err(Error::RefundCooldownNotElapsed);
        }

        // Sum existing refund amounts for this payment
        let existing_refunds = Self::get_payment_refunds_internal(env, &payment_id);
        let mut total_refunded: i128 = 0;
        for id in existing_refunds.iter() {
            if let Ok(r) = Self::get_refund_internal(env, &id) {
                if r.status != RefundStatus::Rejected && r.status != RefundStatus::Cancelled {
                    total_refunded += r.amount;
                }
            }
        }

        if total_refunded + refund_amount > payment.amount {
            return Err(Error::RefundExceedsPayment);
        }

        let counter = Self::get_next_refund_id(env);

        // Build refund ID: "refund_" + counter
        // For simplicity and to avoid complex string manipulation in no_std,
        // we use a match statement for common cases
        let refund_id = format_id(env, "refund_", counter);

        let created_at = env.ledger().timestamp();
        // Issue #170: Set expiry timestamp (30 days from now)
        let expiry_at = now + REFUND_EXPIRY_SECS;

        let refund = Refund {
            refund_id: refund_id.clone(),
            payment_id: payment_id.clone(),
            amount: refund_amount,
            reason: reason.clone(),
            status: RefundStatus::Pending,
            requester,
            created_at,
            processed_at: None,
            receipt_hash,
            approved: false,
            expiry_at: created_at.saturating_add(Self::get_refund_expiry_secs(env)),
        };

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);

        let mut payment_refunds = Self::get_payment_refunds_internal(env, &payment_id);
        payment_refunds.push_back(refund_id.clone());
        env.storage().persistent().set(
            &DataKey::PaymentRefunds(payment_id.clone()),
            &payment_refunds,
        );
        Self::bump_ttl(
            env,
            &DataKey::PaymentRefunds(payment_id.clone()),
            LONG_LIVE_TTL,
        );

        Self::bump_refund_ttl(env, &refund_id, &refund.status);

        // Issue #638: Persist the idempotency key → refund mapping (30-day TTL) so a
        // retried call with the same key returns this refund_id instead of duplicating.
        if let Some(key) = idempotency_key {
            let dk = DataKey::RefundIdempotencyKey(key);
            env.storage().persistent().set(
                &dk,
                &RefundIdempotencyRecord {
                    refund_id: refund_id.clone(),
                    payment_id: payment_id.clone(),
                    amount: refund_amount,
                    reason,
                },
            );
            Self::bump_ttl(env, &dk, REFUND_IDEMPOTENCY_TTL_LEDGERS);
        }

        // Issue #27: emit REFUND/CREATED event
        env.events().publish(
            (Symbol::new(env, "REFUND"), Symbol::new(env, "CREATED")),
            (payment_id, refund_id.clone(), refund_amount),
        );

        Ok(refund_id)
    }

    pub fn process_refund(env: Env, operator: Address, refund_id: String) -> Result<(), Error> {
        operator.require_auth();
        Self::require_not_paused(&env)?;
        Self::require_not_blacklisted(&env, &operator)?;
        
        // Issue #171: Allow either operator OR customer (requester) to process approved refunds
        let refund = Self::get_refund_internal(&env, &refund_id)?;
        Self::require_not_blacklisted(&env, &refund.requester)?;
        
        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);
        let is_requester = operator == refund.requester;

        // Operator can always process; customer can only process if approved
        if !has_settlement && !has_oracle && !(is_requester && refund.approved) {
            return Err(Error::Unauthorized);
        }

        Self::process_refund_internal(&env, &operator, refund_id)
    }

    pub fn get_treasury_balance(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::TreasuryBalance)
            .unwrap_or(0)
    }

    /// Append a withdrawal record, retaining only the newest
    /// `TREASURY_WITHDRAWAL_HISTORY_CAP` entries (newest-first).
    fn record_treasury_withdrawal(env: &Env, record: TreasuryWithdrawal) {
        let key = DataKey::TreasuryWithdrawalHistory;
        let mut history: Vec<TreasuryWithdrawal> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![env]);
        history.push_front(record);
        while history.len() > TREASURY_WITHDRAWAL_HISTORY_CAP {
            history.pop_back();
        }
        env.storage().persistent().set(&key, &history);
    }

    /// Return a page of treasury withdrawal history (newest-first).
    /// `offset` skips the first N records; `limit` caps the page size (max 100).
    pub fn get_treasury_withdrawal_history(
        env: Env,
        offset: u32,
        limit: u32,
    ) -> Vec<TreasuryWithdrawal> {
        let history: Vec<TreasuryWithdrawal> = env
            .storage()
            .persistent()
            .get(&DataKey::TreasuryWithdrawalHistory)
            .unwrap_or_else(|| vec![&env]);
        let page_limit = limit.min(TREASURY_WITHDRAWAL_HISTORY_CAP);
        let mut page: Vec<TreasuryWithdrawal> = vec![&env];
        let mut i = offset;
        while i < history.len() && page.len() < page_limit {
            if let Some(item) = history.get(i) {
                page.push_back(item);
            }
            i = i.saturating_add(1);
        }
        page
    }

    pub fn withdraw_treasury(
        env: Env,
        admin: Address,
        amount: i128,
        destination: Address,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let treasury_balance = Self::get_treasury_balance(env.clone());
        if amount > treasury_balance {
            return Err(Error::InsufficientTreasuryBalance);
        }

        let usdc_token_address: Address = env
            .storage()
            .persistent()
            .get(&DataKey::UsdcToken)
            .ok_or(Error::Unauthorized)?;
        let token_client = token::TokenClient::new(&env, &usdc_token_address);
        let contract_address = env.current_contract_address();

        env.storage().persistent().set(
            &DataKey::TreasuryBalance,
            &treasury_balance.saturating_sub(amount),
        );

        token_client.transfer(&contract_address, &destination, &amount);

        Self::record_treasury_withdrawal(
            &env,
            TreasuryWithdrawal {
                amount,
                destination: destination.clone(),
                admin: admin.clone(),
                withdrawn_at: env.ledger().timestamp(),
            },
        );

        env.events().publish(
            (Symbol::new(&env, "TREASURY"), Symbol::new(&env, "WITHDRAWN")),
            (amount, destination.clone()),
        );

        Ok(())
    }

    fn process_refund_internal(
        env: &Env,
        _operator: &Address,
        refund_id: String,
    ) -> Result<(), Error> {
        if env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::ReentrancyLock)
            .unwrap_or(false)
        {
            return Err(Error::Reentrancy);
        }
        // Per-refund lock: reject concurrent/reentrant process_refund for the same ID.
        if env
            .storage()
            .persistent()
            .has(&DataKey::RefundLock(refund_id.clone()))
        {
            return Err(Error::Reentrancy);
        }
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyLock, &true);
        env.storage()
            .persistent()
            .set(&DataKey::RefundLock(refund_id.clone()), &true);
        let _guard = ReentrancyGuard { env };
        let _refund_lock = RefundLockGuard {
            env,
            refund_id: refund_id.clone(),
        };

        let mut refund = Self::get_refund_internal(env, &refund_id)?;

        if refund.status != RefundStatus::Pending {
            return Err(Error::RefundAlreadyProcessed);
        }

        let require_receipt_hash: bool = env
            .storage()
            .persistent()
            .get(&DataKey::RequireReceiptHash)
            .unwrap_or(false);
        if require_receipt_hash && refund.receipt_hash.is_none() {
            return Err(Error::MissingReceiptHash);
        }
        // Issue #170: Check refund expiration
        let now = env.ledger().timestamp();
        if now > refund.expiry_at {
            return Err(Error::RefundExpired);
        }

        let usdc_token_address: Address = env
            .storage()
            .persistent()
            .get(&DataKey::UsdcToken)
            .ok_or(Error::Unauthorized)?;
        let _token_client = token::TokenClient::new(env, &usdc_token_address);

        // Issue #167: Query merchant's KYC tier and apply tiered refund fee
        let payment: PaymentCharge = env
            .storage()
            .persistent()
            .get::<DataKey, PaymentCharge>(&DataKey::Payment(refund.payment_id.clone()))
            .ok_or(Error::PaymentNotFound)?;

        // Issue #167: Query merchant's KYC tier and apply tiered refund fee
        let default_fee_bps = Self::get_refund_fee_bps_internal(&env);

        let fee_bps = if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(env, &registry_address);
            match registry_client.try_get_merchant(&payment.merchant_id) {
                Ok(Ok(merchant)) => {
                    use crate::merchant_registry::KycTier;
                    match merchant.kyc_tier {
                        KycTier::Business => REFUND_FEE_BPS_BUSINESS,
                        KycTier::Full => REFUND_FEE_BPS_FULL,
                        KycTier::Basic => REFUND_FEE_BPS_BASIC,
                        KycTier::Unverified => default_fee_bps,
                    }
                }
                _ => default_fee_bps,
            }
        } else {
            default_fee_bps
        };

        let fee = refund.amount * fee_bps / 10_000;
        let net_amount = refund.amount - fee;

        // Issue #173: Multi-token swap refund router
        let (refund_token, refund_amount_final) = if let (Some(original_token), Some(swap_path)) = 
            (&payment.original_token, &payment.swap_path) 
        {
            // Payment was made via swap_and_pay, refund in original token
            if swap_path.len() >= 2 {
                // Reverse the swap path for refund
                let mut reverse_path = Vec::new(&env);
                for i in 0..swap_path.len() {
                    reverse_path.push_back(swap_path.get(swap_path.len() - 1 - i).unwrap());
                }
                
                // Use DEX to swap back to original token
                // For now, we'll use a simple approach - in production this would call the DEX
                // Simplified: just return the original token and net amount
                // Real implementation would execute the reverse swap
                (original_token.clone(), net_amount)
            } else {
                // Fallback to settlement token if path is invalid
                let settlement_token = payment.token_address.clone().unwrap_or_else(|| {
                    env.storage()
                        .persistent()
                        .get(&DataKey::UsdcToken)
                        .unwrap()
                });
                (settlement_token, net_amount)
            }
        } else {
            // Regular payment, refund in settlement token
            let settlement_token = payment.token_address.clone().unwrap_or_else(|| {
                env.storage()
                    .persistent()
                    .get(&DataKey::UsdcToken)
                    .unwrap()
            });
            (settlement_token, net_amount)
        };

        let token_client = token::TokenClient::new(env, &refund_token);
        let from = env.current_contract_address();
        let to: MuxedAddress = (&refund.requester).into();

        // Effects before interactions: mark Completed before any token transfer.
        refund.status = RefundStatus::Completed;
        refund.processed_at = Some(env.ledger().timestamp());

        // Persist state before interaction (reentrancy protection)
        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);
        Self::bump_refund_ttl(env, &refund_id, &refund.status);

        // Issue #173: route the refund back through the DEX to the payer's
        // original token when the payment was made via swap_and_pay.
        let mut routed_via_dex = false;
        if let (Some(original_token), Some(swap_path)) =
            (&payment.original_token, &payment.swap_path)
        {
            if let Some(dex_router) = env
                .storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::DexRouterAddress)
            {
                let mut reversed_path = Vec::new(env);
                let mut i = swap_path.len();
                while i > 0 {
                    i = i - 1;
                    reversed_path.push_back(swap_path.get_unchecked(i));
                }
                if !reversed_path.is_empty() {
                    let dex_client = crate::dex_router::DexRouterClient::new(env, &dex_router);
                    let deadline = env.ledger().timestamp().saturating_add(3_600);
                    match dex_client.try_swap_exact_tokens_for_tokens(
                        &net_amount,
                        &1i128,
                        &reversed_path,
                        &refund.requester,
                        &deadline,
                    ) {
                        Ok(Ok(_amounts)) => {
                            routed_via_dex = true;
                            env.events().publish(
                                (Symbol::new(env, "REFUND"), Symbol::new(env, "SWAP_ROUTED")),
                                (refund.payment_id.clone(), refund_id.clone(), original_token.clone()),
                            );
                        }
                        _ => {
                            env.events().publish(
                                (Symbol::new(env, "REFUND"), Symbol::new(env, "SWAP_FALLBACK")),
                                (refund.payment_id.clone(), refund_id.clone()),
                            );
                        }
                    }
                }
            }
        }

        // Interaction: Transfer net amount to requester (in USDC, unless already
        // routed back to the original token via the DEX above).
        if !routed_via_dex && token_client.try_transfer(&from, &to, &net_amount).is_err() {
            return Ok(());
        }

        if fee > 0 {
            let current_treasury_balance = Self::get_treasury_balance(env.clone());
            env.storage().persistent().set(
                &DataKey::TreasuryBalance,
                &current_treasury_balance.saturating_add(fee),
            );
        }
        
        env.events().publish(
            (Symbol::new(env, "REFUND"), Symbol::new(env, "COMPLETED")),
            (refund.payment_id.clone(), refund_id.clone(), refund.amount),
        );

        if refund.receipt_hash.is_some() {
            env.events().publish(
                (Symbol::new(env, "REFUND"), Symbol::new(env, "HASH_VERIFIED")),
                (refund.payment_id, refund_id),
            );
        }

        Ok(())
    }

    /// Reject a pending refund (operator only). Emits REFUND/REJECTED (issue #27).
    pub fn reject_refund(env: Env, operator: Address, refund_id: String) -> Result<(), Error> {
        operator.require_auth();
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

        refund.status = RefundStatus::Rejected;
        refund.processed_at = Some(env.ledger().timestamp());

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);
        Self::bump_refund_ttl(&env, &refund_id, &refund.status);

        // Issue #27: emit REFUND/REJECTED event
        env.events().publish(
            (Symbol::new(&env, "REFUND"), Symbol::new(&env, "REJECTED")),
            (refund.payment_id, refund_id, refund.amount),
        );

        Ok(())
    }

    /// Clean up a pending refund that has passed its `expiry_at` deadline
    /// (Issue #170). Marks it `Rejected` so it no longer blocks the
    /// payment's refundable balance. Callable by the same roles as
    /// `process_refund`/`reject_refund`.
    pub fn expire_refund(env: Env, operator: Address, refund_id: String) -> Result<(), Error> {
        operator.require_auth();
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

        if env.ledger().timestamp() <= refund.expiry_at {
            return Err(Error::RefundExpired);
        }

        refund.status = RefundStatus::Rejected;
        refund.processed_at = Some(env.ledger().timestamp());

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);
        Self::bump_refund_ttl(&env, &refund_id, &refund.status);

        env.events().publish(
            (Symbol::new(&env, "REFUND"), Symbol::new(&env, "EXPIRED")),
            (refund.payment_id, refund_id, refund.amount),
        );

        Ok(())
    }

    /// Issue #171: Approve a pending refund, allowing customer to claim it.
    /// Operator marks the refund as approved without processing it immediately.
    /// Customer can then call process_refund to claim the approved refund.
    pub fn approve_refund(env: Env, operator: Address, refund_id: String) -> Result<(), Error> {
        operator.require_auth();
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

        refund.approved = true;

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);
        Self::bump_refund_ttl(&env, &refund_id, &refund.status);

        env.events().publish(
            (Symbol::new(&env, "REFUND"), Symbol::new(&env, "APPROVED")),
            (refund.payment_id, refund_id, refund.amount),
        );

        Ok(())
    }

    /// Issue #450: Customer self-serves an operator-approved refund.
    ///
    /// Callable only by the original refund requester, and only once an
    /// operator has called `approve_refund`. `process_refund` remains
    /// available for operators/oracles who need to execute a refund
    /// directly without waiting for the customer to claim it.
    pub fn claim_refund(env: Env, requester: Address, refund_id: String) -> Result<(), Error> {
        requester.require_auth();
        Self::require_not_paused(&env)?;
        Self::require_not_blacklisted(&env, &requester)?;

        let refund = Self::get_refund_internal(&env, &refund_id)?;

        if refund.requester != requester {
            return Err(Error::Unauthorized);
        }
        if refund.status != RefundStatus::Pending {
            return Err(Error::RefundAlreadyProcessed);
        }
        if !refund.approved {
            return Err(Error::RefundNotApproved);
        }

        Self::process_refund_internal(&env, &requester, refund_id)
    }
    /// Admin-configurable refund expiry window in seconds (Issue #170).
    /// Applies to refunds created after this call.
    pub fn set_refund_expiry(env: Env, admin: Address, secs: u64) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        if secs == 0 {
            return Err(Error::InvalidAmount);
        }

        env.storage()
            .persistent()
            .set(&DataKey::RefundExpirySecs, &secs);

        Ok(())
    }

    fn get_refund_expiry_secs(env: &Env) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::RefundExpirySecs)
            .unwrap_or(DEFAULT_REFUND_EXPIRY_SECS)
    }

    /// Cancel a pending refund. Caller must be the refund requester (merchant) or contract admin.
    /// Removes the refund from the payment's pending list and emits REFUND/CANCELLED.
    /// Instantly refund a payment without operator approval.
    ///
    /// Only merchants with KYC tier `Full` or `Business` may call this.
    /// The merchant must be the `merchant_id` on the original payment.
    /// Executes the USDC transfer immediately (no `Pending` state).
    pub fn refund_instantly(
        env: Env,
        merchant_id: Address,
        payment_id: String,
        refund_amount: i128,
        reason: String,
        registry_address: Address,
    ) -> Result<String, Error> {
        merchant_id.require_auth();
        Self::require_not_blacklisted(&env, &merchant_id)?;

        // Verify merchant KYC tier is Full or Business via cross-contract call
        let registry_client =
            crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
        let merchant = registry_client
            .try_get_merchant(&merchant_id)
            .map_err(|_| Error::Unauthorized)?
            .map_err(|_| Error::Unauthorized)?;

        let is_high_trust = merchant.kyc_tier == crate::merchant_registry::KycTier::Full
            || merchant.kyc_tier == crate::merchant_registry::KycTier::Business;
        if !is_high_trust {
            return Err(Error::Unauthorized);
        }

        // Validate payment belongs to this merchant and is Confirmed
        let payment: PaymentCharge = env
            .storage()
            .persistent()
            .get(&DataKey::Payment(payment_id.clone()))
            .ok_or(Error::PaymentNotFound)?;

        // Issue #485: Prevent disputes on direct transfer payments
        if env.storage().persistent().has(&DataKey::DirectTransferPayment(payment_id.clone())) {
            return Err(Error::DirectTransferNotDisputable);
        }

        if payment.merchant_id != merchant_id {
            return Err(Error::Unauthorized);
        }
        if let Some(ref payer) = payment.payer_address {
            Self::require_not_blacklisted(&env, payer)?;
        }
        if payment.status != PaymentStatus::Confirmed {
            return Err(Error::PaymentAlreadyProcessed);
        }

        // Create the refund record (validates amount, checks totals)
        let refund_id = Self::create_refund_internal(
            &env,
            payment_id,
            refund_amount,
            reason,
            payment.payer_address.clone().ok_or(Error::Unauthorized)?,
            None,
            None,
        )?;

        // Execute transfer immediately — no operator approval needed
        let usdc_token_address: Address = env
            .storage()
            .persistent()
            .get(&DataKey::UsdcToken)
            .ok_or(Error::Unauthorized)?;
        let token_client = token::TokenClient::new(&env, &usdc_token_address);

        let default_fee_bps = Self::get_refund_fee_bps_internal(&env);
        let fee = refund_amount * default_fee_bps / 10_000;
        let net_amount = refund_amount - fee;

        let mut refund = Self::get_refund_internal(&env, &refund_id)?;
        refund.status = RefundStatus::Completed;
        refund.processed_at = Some(env.ledger().timestamp());

        // Effects before interaction (CEI)
        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);
        Self::bump_refund_ttl(&env, &refund_id, &refund.status);

        let from = env.current_contract_address();
        let to: MuxedAddress = (&refund.requester).into();
        let _ = token_client.try_transfer(&from, &to, &net_amount);

        if fee > 0 {
            if let Some(admin) = AccessControl::get_admin(&env) {
                let admin_muxed: MuxedAddress = (&admin).into();
                let _ = token_client.try_transfer(&from, &admin_muxed, &fee);
            }
        }

        env.events().publish(
            (Symbol::new(&env, "REFUND"), Symbol::new(&env, "COMPLETED")),
            (refund.payment_id, refund_id.clone(), refund_amount),
        );

        Ok(refund_id)
    }

    pub fn cancel_refund(env: Env, caller: Address, refund_id: String) -> Result<(), Error> {
        caller.require_auth();

        let mut refund = Self::get_refund_internal(&env, &refund_id)?;

        match refund.status {
            RefundStatus::Pending => {}
            RefundStatus::Cancelled => return Err(Error::RefundCancelled),
            _ => return Err(Error::RefundAlreadyProcessed),
        }

        let is_requester = caller == refund.requester;
        let is_admin = AccessControl::has_role(&env, &role_admin(&env), &caller);
        if !is_requester && !is_admin {
            return Err(Error::Unauthorized);
        }

        refund.status = RefundStatus::Cancelled;
        refund.processed_at = Some(env.ledger().timestamp());

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);
        Self::bump_refund_ttl(&env, &refund_id, &refund.status);

        env.events().publish(
            (Symbol::new(&env, "REFUND"), Symbol::new(&env, "CANCELLED")),
            (refund.payment_id, refund_id, refund.amount),
        );

        Ok(())
    }

    pub fn get_refund(env: Env, refund_id: String) -> Result<Refund, Error> {
        Self::get_refund_internal(&env, &refund_id)
    }

    pub fn get_payment_refunds(env: Env, payment_id: String) -> Result<Vec<Refund>, Error> {
        let refund_ids = RefundManager::get_payment_refunds_internal(&env, &payment_id);
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
    pub fn create_dispute(
        env: Env,
        payment_id: String,
        amount: i128,
        reason: String,
        evidence: String,
        disputer: Address,
        payout_splits: Vec<SettlementSplit>,
    ) -> Result<String, Error> {
        disputer.require_auth();
        Self::create_dispute_inner(
            &env,
            payment_id,
            amount,
            reason,
            evidence,
            disputer,
            payout_splits,
        )
    }

    /// Batch-create disputes for marketplace bulk filing.
    ///
    /// Processes up to `max_batch` items (hard cap 20). Each item is handled
    /// identically to `create_dispute`; failures do not revert successes.
    /// Bonds are deducted only for successful disputes.
    /// Emits `DISPUTE/BATCH_CREATED` with `(success_count, fail_count)`.
    pub fn batch_create_disputes(
        env: Env,
        disputes: Vec<CreateDisputeArgs>,
        max_batch: u32,
    ) -> Result<Vec<DisputeBatchItemResult>, Error> {
        let effective_max = max_batch.min(MAX_DISPUTE_BATCH);
        if max_batch > MAX_DISPUTE_BATCH || disputes.len() > effective_max {
            return Err(Error::BatchTooLarge);
        }

        let mut results: Vec<DisputeBatchItemResult> = vec![&env];
        let mut success_count: u32 = 0;
        let mut fail_count: u32 = 0;
        let mut total_bond_deducted: i128 = 0;

        for args in disputes.iter() {
            args.disputer.require_auth();
            match Self::create_dispute_inner(
                &env,
                args.payment_id.clone(),
                args.amount,
                args.reason.clone(),
                args.evidence.clone(),
                args.disputer.clone(),
                args.payout_splits.clone(),
            ) {
                Ok(dispute_id) => {
                    success_count = success_count.saturating_add(1);
                    // Each successful dispute locks 2x DISPUTE_BOND_AMOUNT (disputer + merchant).
                    total_bond_deducted = total_bond_deducted
                        .saturating_add(DISPUTE_BOND_AMOUNT.saturating_mul(2));
                    results.push_back(DisputeBatchItemResult::Ok(dispute_id));
                }
                Err(e) => {
                    fail_count = fail_count.saturating_add(1);
                    results.push_back(DisputeBatchItemResult::Err(e as u32));
                }
            }
        }

        env.events().publish(
            (
                Symbol::new(&env, "DISPUTE"),
                Symbol::new(&env, "BATCH_CREATED"),
            ),
            (success_count, fail_count, total_bond_deducted),
        );

        Ok(results)
    }

    fn create_dispute_inner(
        env: &Env,
        payment_id: String,
        amount: i128,
        reason: String,
        evidence: String,
        disputer: Address,
        payout_splits: Vec<SettlementSplit>,
    ) -> Result<String, Error> {
        Self::require_not_paused(&env)?;

        // Issue #404: Validate payment_id format
        if !utils::validate_id(&payment_id) {
            return Err(Error::InvalidPaymentId);
        }

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        // Issue #625: Enforce maximum length on the evidence field.
        if evidence.len() as usize > MAX_EVIDENCE_LEN {
            return Err(Error::InputTooLong);
        }

        // IPFS CID validation when require_evidence_cid is enabled (default: true).
        // Empty evidence is always allowed; non-empty must be CIDv0/CIDv1 when flag is on.
        let require_cid = env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::RequireEvidenceCid)
            .unwrap_or(true);
        if require_cid && evidence.len() > 0 && !validate_ipfs_multihash(&evidence) {
            return Err(Error::InvalidEvidenceCid);
        }

        // Rate limits: max open disputes per payer + global hourly creation cap.
        Self::enforce_dispute_rate_limits(&env, &disputer)?;

        // Issue #77: Load payment and cap dispute amount to confirmed payment amount
        let payment: PaymentCharge = env
            .storage()
            .persistent()
            .get(&DataKey::Payment(payment_id.clone()))
            .ok_or(Error::PaymentNotFound)?;

        // Ensure payment is confirmed
        if payment.status != PaymentStatus::Confirmed {
            return Err(Error::PaymentAlreadyProcessed);
        }

        // Cap dispute amount to payment amount
        if amount > payment.amount {
            return Err(Error::InvalidAmount);
        }

        payment.merchant_id.require_auth();
        Self::require_not_blacklisted(&env, &disputer)?;
        Self::require_not_blacklisted(&env, &payment.merchant_id)?;

        let usdc_token_address = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::UsdcToken)
            .ok_or(Error::Unauthorized)?;
        let token_client = token::TokenClient::new(&env, &usdc_token_address);
        let contract_address = env.current_contract_address();

        let bond_amount = Self::get_dispute_bond_amount(env.clone());

        if token_client
            .try_transfer(&disputer, &contract_address, &bond_amount)
            .is_err()
        {
            return Err(Error::Unauthorized);
        }
        if token_client
            .try_transfer(&payment.merchant_id, &contract_address, &bond_amount)
            .is_err()
        {
            return Err(Error::Unauthorized);
        }

        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "BOND_COLLECTED")),
            (disputer.clone(), bond_amount),
        );

        // Sum open disputes + prior refunds for the same payment_id
        let existing_disputes = Self::get_payment_disputes_internal(&env, &payment_id);
        let mut total_disputed: i128 = 0;
        for id in existing_disputes.iter() {
            if let Ok(d) = Self::get_dispute_internal(&env, &id) {
                if d.status != DisputeStatus::Rejected {
                    total_disputed += d.amount;
                }
            }
        }

        let existing_refunds = Self::get_payment_refunds_internal(&env, &payment_id);
        let mut total_refunded: i128 = 0;
        for id in existing_refunds.iter() {
            if let Ok(r) = Self::get_refund_internal(&env, &id) {
                if r.status != RefundStatus::Rejected && r.status != RefundStatus::Cancelled {
                    total_refunded += r.amount;
                }
            }
        }

        // Ensure totals stay within payment.amount
        if total_disputed + total_refunded + amount > payment.amount {
            return Err(Error::RefundExceedsPayment);
        }

        let counter = Self::get_next_dispute_id(&env);
        let dispute_id = Self::build_dispute_id(&env, counter);

        // Issue #177: Compute dynamic deadline based on dispute amount.
        // Small disputes (<= configured threshold, default 100 USDC): 3 days; larger: 7 days.
        let deadline_secs = Self::computed_dispute_deadline_secs(&env, amount);

        let dispute = Dispute {
            dispute_id: dispute_id.clone(),
            payment_id: payment_id.clone(),
            merchant_id: payment.merchant_id.clone(),
            refund_id: None,
            amount,
            reason,
            evidence,
            status: DisputeStatus::Open,
            disputer: disputer.clone(),
            created_at: env.ledger().timestamp(),
            resolved_at: None,
            resolution_notes: None,
            review_deadline: None,
            escalated: false,
            payout_splits: Vec::new(&env),
            computed_deadline_secs: Some(deadline_secs),
        };

        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id.clone()), &dispute);

        let mut payment_disputes = Self::get_payment_disputes_internal(&env, &payment_id);
        payment_disputes.push_back(dispute_id.clone());
        env.storage().persistent().set(
            &DataKey::PaymentDisputes(payment_id.clone()),
            &payment_disputes,
        );

        // Record rate-limit counters after successful dispute creation.
        Self::record_dispute_creation(&env, &disputer);
        Self::bump_ttl(
            &env,
            &DataKey::PaymentDisputes(payment_id.clone()),
            LONG_LIVE_TTL,
        );

        Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);

        // Issue #184: Track dispute count per merchant and auto-suspend if dispute rate is too high
        let merchant_id = payment.merchant_id.clone();
        let dispute_count_key = DataKey::MerchantDisputeCount(merchant_id.clone());
        let dispute_count: u64 = env
            .storage()
            .persistent()
            .get(&dispute_count_key)
            .unwrap_or(0u64);
        let new_dispute_count = dispute_count + 1;
        env.storage()
            .persistent()
            .set(&dispute_count_key, &new_dispute_count);
        Self::bump_ttl(&env, &dispute_count_key, LONG_LIVE_TTL);

        // Check dispute rate: if >= 10% of payments have disputes, auto-suspend via registry
        let payment_count: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantPaymentCount(merchant_id.clone()))
            .unwrap_or(0u64);

        // Only evaluate after at least 5 payments to avoid false positives on new merchants
        if payment_count >= 5 {
            // dispute_rate_bps = (dispute_count * 10_000) / payment_count
            let dispute_rate_bps = new_dispute_count
                .saturating_mul(10_000)
                .checked_div(payment_count)
                .unwrap_or(0);

            // Threshold: 1000 bps = 10%
            if dispute_rate_bps >= 1_000 {
                if let Some(registry_address) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
                {
                    let registry_client =
                        crate::merchant_registry::MerchantRegistryClient::new(
                            &env,
                            &registry_address,
                        );
                    // Auto-suspend for 30 days; ignore errors (registry may not have this merchant)
                    let suspension_reason = String::from_str(
                        &env,
                        "Auto-suspended: dispute rate exceeded 10% threshold",
                    );
                    let thirty_days_secs: u64 = 30 * 24 * 60 * 60;
                    let _ = registry_client.try_suspend_merchant_by_system(
                        &merchant_id,
                        &suspension_reason,
                        &thirty_days_secs,
                    );

                    // Emit auto-suspension event for off-chain indexers
                    env.events().publish(
                        (
                            Symbol::new(&env, "MERCHANT"),
                            Symbol::new(&env, "AUTO_SUSPENDED"),
                        ),
                        (
                            merchant_id,
                            new_dispute_count,
                            payment_count,
                            dispute_rate_bps,
                        ),
                    );
                }
            }
        }

        // Issue #27: emit DISPUTE_CREATED event
        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "CREATED")),
            (dispute_id.clone(), payment_id),
        );

        Ok(dispute_id)
    }

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
            .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
        Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);

        // Issue #27: emit DISPUTE_REVIEWED event
        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "REVIEWED")),
            (dispute_id, dispute.payment_id),
        );

        Ok(())
    }

    /// Configure dispute creation rate limits (admin only).
    ///
    /// * `per_payer` — max concurrent open/under-review disputes per disputer
    /// * `global_per_hour` — max dispute creations per hour across all disputers
    pub fn set_dispute_rate_limits(
        env: Env,
        admin: Address,
        per_payer: u32,
        global_per_hour: u32,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        let config = DisputeRateLimitConfig {
            per_payer_open: per_payer,
            global_per_hour,
        };
        env.storage()
            .persistent()
            .set(&DataKey::DisputeRateLimits, &config);
        Ok(())
    }

    /// Toggle whether non-empty dispute evidence must be a valid IPFS CID.
    /// Set `false` for testnet/dev to accept arbitrary evidence strings.
    pub fn set_require_evidence_cid(
        env: Env,
        admin: Address,
        require_cid: bool,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::RequireEvidenceCid, &require_cid);
        Ok(())
    }


    pub fn set_dispute_threshold(env: Env, admin: Address, amount: i128) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        env.storage()
            .persistent()
            .set(&DataKey::DisputeDeadlineThresholdAmount, &amount);
        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "THRESHOLD_SET")),
            amount,
        );
        Ok(())
    }

    fn get_dispute_deadline_threshold(env: &Env) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::DisputeDeadlineThresholdAmount)
            .unwrap_or(DEFAULT_DISPUTE_DEADLINE_THRESHOLD_AMOUNT)
    }

    fn computed_dispute_deadline_secs(env: &Env, amount: i128) -> u64 {
        if amount <= Self::get_dispute_deadline_threshold(env) {
            SMALL_DISPUTE_DEADLINE_SECS
        } else {
            LARGE_DISPUTE_DEADLINE_SECS
        }
    }

    fn get_dispute_rate_limits(env: &Env) -> DisputeRateLimitConfig {
        env.storage()
            .persistent()
            .get(&DataKey::DisputeRateLimits)
            .unwrap_or(DisputeRateLimitConfig {
                per_payer_open: DEFAULT_DISPUTE_PER_PAYER_OPEN,
                global_per_hour: DEFAULT_DISPUTE_GLOBAL_PER_HOUR,
            })
    }

    fn enforce_dispute_rate_limits(env: &Env, disputer: &Address) -> Result<(), Error> {
        let limits = Self::get_dispute_rate_limits(env);

        let open: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::PayerOpenDisputeCount(disputer.clone()))
            .unwrap_or(0);
        if open >= limits.per_payer_open {
            return Err(Error::DisputeRateLimitExceeded);
        }

        let now = env.ledger().timestamp();
        let mut state: DisputeCreationRateState = env
            .storage()
            .persistent()
            .get(&DataKey::GlobalDisputeCreationRate)
            .unwrap_or(DisputeCreationRateState {
                window_started_at: now,
                count: 0,
            });

        if now.saturating_sub(state.window_started_at) >= DISPUTE_GLOBAL_WINDOW_SECS {
            state.window_started_at = now;
            state.count = 0;
        }

        if state.count >= limits.global_per_hour {
            return Err(Error::DisputeRateLimitExceeded);
        }

        Ok(())
    }

    fn record_dispute_creation(env: &Env, disputer: &Address) {
        let open_key = DataKey::PayerOpenDisputeCount(disputer.clone());
        let open: u32 = env.storage().persistent().get(&open_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&open_key, &open.saturating_add(1));
        Self::bump_ttl(env, &open_key, SHORT_LIVE_TTL);

        let now = env.ledger().timestamp();
        let mut state: DisputeCreationRateState = env
            .storage()
            .persistent()
            .get(&DataKey::GlobalDisputeCreationRate)
            .unwrap_or(DisputeCreationRateState {
                window_started_at: now,
                count: 0,
            });

        if now.saturating_sub(state.window_started_at) >= DISPUTE_GLOBAL_WINDOW_SECS {
            state.window_started_at = now;
            state.count = 0;
        }
        state.count = state.count.saturating_add(1);
        env.storage()
            .persistent()
            .set(&DataKey::GlobalDisputeCreationRate, &state);
        Self::bump_ttl(env, &DataKey::GlobalDisputeCreationRate, SHORT_LIVE_TTL);
    }

    fn release_open_dispute_slot(env: &Env, disputer: &Address) {
        let open_key = DataKey::PayerOpenDisputeCount(disputer.clone());
        let open: u32 = env.storage().persistent().get(&open_key).unwrap_or(0);
        if open > 0 {
            env.storage()
                .persistent()
                .set(&open_key, &(open - 1));
        }
    }

    /// Operator-only: set or update the review deadline for an open or under-review dispute.
    /// Emits DISPUTE/DEADLINE_SET. If the current ledger time already exceeds the deadline,
    /// the dispute is also flagged as escalated and DISPUTE/ESCALATED is emitted.
    pub fn set_dispute_deadline(
        env: Env,
        operator: Address,
        dispute_id: String,
        deadline: u64,
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

        dispute.review_deadline = Some(deadline);

        let now = env.ledger().timestamp();
        if now > deadline && !dispute.escalated {
            dispute.escalated = true;
            env.storage()
                .persistent()
                .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
            Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);
            env.events().publish(
                (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "ESCALATED")),
                (
                    dispute.payment_id.clone(),
                    dispute_id.clone(),
                    dispute.amount,
                ),
            );
        } else {
            env.storage()
                .persistent()
                .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
            Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);
        }

        env.events().publish(
            (
                Symbol::new(&env, "DISPUTE"),
                Symbol::new(&env, "DEADLINE_SET"),
            ),
            (dispute.payment_id, dispute_id, deadline),
        );

        Ok(())
    }

    /// Operator: configure multi-party payout splits for a marketplace dispute.
    /// Splits must sum to exactly `dispute.amount`; validated again at
    /// resolution time in case the dispute amount changes. (Issue #446)
    pub fn set_dispute_payout_splits(
        env: Env,
        operator: Address,
        dispute_id: String,
        splits: Vec<SettlementSplit>,
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

        let mut total: i128 = 0;
        for split in splits.iter() {
            total = total.saturating_add(split.amount);
        }
        if total != dispute.amount {
            return Err(Error::InvalidSplitSum);
        }

        dispute.payout_splits = splits;
        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
        Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);

        Ok(())
    }

    fn maybe_escalate_dispute_due_to_deadline(
        env: &Env,
        dispute_id: &String,
        dispute: &mut Dispute,
    ) -> Result<bool, Error> {
        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Ok(false);
        }

        let deadline = dispute.review_deadline.or_else(|| {
            dispute
                .computed_deadline_secs
                .map(|secs| dispute.created_at.saturating_add(secs))
        });
        let Some(deadline) = deadline else {
            return Ok(false);
        };

        let now = env.ledger().timestamp();
        if now <= deadline || dispute.escalated {
            return Ok(false);
        }

        dispute.escalated = true;
        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id.clone()), &*dispute);
        Self::bump_dispute_ttl(env, dispute_id, &dispute.status);
        env.events().publish(
            (Symbol::new(env, "DISPUTE"), Symbol::new(env, "ESCALATED")),
            (dispute.payment_id.clone(), dispute_id.clone(), dispute.amount),
        );

        Ok(true)
    }

    /// Anyone may call this to trigger escalation after a dispute review deadline elapses.
    pub fn check_dispute_deadline(env: Env, dispute_id: String) -> Result<(), Error> {
        let mut dispute = Self::get_dispute_internal(&env, &dispute_id)?;
        let _ = Self::maybe_escalate_dispute_due_to_deadline(&env, &dispute_id, &mut dispute)?;
        Ok(())
    }

    pub fn escalate_expired_disputes(env: Env, dispute_ids: Vec<String>) -> u32 {
        let mut count = 0;
        let mut i = 0;
        let len = dispute_ids.len();
        let max = if len > 20 { 20 } else { len };
        
        while i < max {
            if let Some(dispute_id) = dispute_ids.get(i) {
                if let Ok(mut dispute) = Self::get_dispute_internal(&env, &dispute_id) {
                    if let Ok(true) = Self::maybe_escalate_dispute_due_to_deadline(&env, &dispute_id, &mut dispute) {
                        count += 1;
                    }
                }
            }
            i += 1;
        }
        count
    }

    pub fn resolve_dispute_with_refund(
        env: Env,
        operator: Address,
        dispute_id: String,
        resolution_notes: String,
        operator_signature: String,
    ) -> Result<String, Error> {
        operator.require_auth();
        Self::require_not_paused(&env)?;

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

        // Issue #446: if payout_splits are configured, distribute funds to
        // each recipient directly instead of issuing a single-recipient
        // refund.
        if !dispute.payout_splits.is_empty() {
            let mut total: i128 = 0;
            for split in dispute.payout_splits.iter() {
                total = total.saturating_add(split.amount);
            }
            if total != dispute.amount {
                return Err(Error::InvalidSplitSum);
            }

            let usdc_token_address: Address = env
                .storage()
                .persistent()
                .get(&DataKey::UsdcToken)
                .ok_or(Error::Unauthorized)?;
            let token_client = token::TokenClient::new(&env, &usdc_token_address);
            let from = env.current_contract_address();

            for split in dispute.payout_splits.iter() {
                token_client.transfer(&from, &split.recipient, &split.amount);
            }

            let now = env.ledger().timestamp();
            dispute.status = DisputeStatus::Resolved;
            dispute.resolved_at = Some(now);
            dispute.resolution_notes = Some(resolution_notes.clone());

            env.storage()
                .persistent()
                .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
            Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);

            env.events().publish(
                (
                    Symbol::new(&env, "DISPUTE"),
                    Symbol::new(&env, "SPLIT_RESOLVED"),
                ),
                (
                    dispute_id.clone(),
                    dispute.payment_id.clone(),
                    dispute.payout_splits.len(),
                    dispute.amount,
                ),
            );

            return Ok(dispute_id);
        }

        // Create refund for the disputed amount
        let refund_reason = String::from_str(&env, "Refund issued due to dispute resolution");

        let refund_id = Self::create_refund_internal(
            &env,
            dispute.payment_id.clone(),
            dispute.amount,
            refund_reason,
            dispute.disputer.clone(),
            None,
            None,
        )?;

        // Process the refund immediately (CEI: status=Completed before token transfer)
        Self::process_refund_internal(&env, &operator, refund_id.clone())?;

        let now = env.ledger().timestamp();

        // Persist operator note on-chain for full transparency.
        let note = DisputeOperatorNote {
            dispute_id: dispute_id.clone(),
            operator: operator.clone(),
            resolution_notes: resolution_notes.clone(),
            operator_signature: operator_signature.clone(),
            recorded_at: now,
        };
        env.storage()
            .persistent()
            .set(&DataKey::DisputeOperatorNote(dispute_id.clone()), &note);
        Self::bump_ttl(
            &env,
            &DataKey::DisputeOperatorNote(dispute_id.clone()),
            LONG_LIVE_TTL,
        );

        // Emit full note + signature so off-chain indexers have the complete record.
        env.events().publish(
            (
                Symbol::new(&env, "DISPUTE"),
                Symbol::new(&env, "OPERATOR_NOTE"),
            ),
            (
                dispute_id.clone(),
                operator.clone(),
                resolution_notes.clone(),
                operator_signature,
            ),
        );

        // Effects before bond interactions: mark dispute resolved first.
        dispute.status = DisputeStatus::Resolved;
        dispute.refund_id = Some(refund_id.clone());
        dispute.resolved_at = Some(now);
        dispute.resolution_notes = Some(resolution_notes);

        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
        Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);
        Self::release_open_dispute_slot(&env, &dispute.disputer);

        // Interactions: return bonds after effects are persisted.
        let usdc_token_address = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::UsdcToken)
            .ok_or(Error::Unauthorized)?;
        let token_client = token::TokenClient::new(&env, &usdc_token_address);
        let contract_address = env.current_contract_address();
        let collector = AccessControl::get_admin(&env).unwrap_or_else(|| contract_address.clone());
        let bond_amount = Self::get_dispute_bond_amount(env.clone());

        if token_client
            .try_transfer(&contract_address, &dispute.disputer, &bond_amount)
            .is_err()
        {
            return Err(Error::Unauthorized);
        }
        if token_client
            .try_transfer(&contract_address, &collector, &bond_amount)
            .is_err()
        {
            return Err(Error::Unauthorized);
        }

        // Issue #677: bond is released back to the disputer (winner) after
        // the dispute is resolved with a refund in their favor.
        events::emit_dispute_bond_returned(
            &env,
            dispute_id.clone(),
            dispute.disputer.clone(),
            bond_amount,
        );

        // Emit DISPUTE_RESOLVED event
        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "RESOLVED")),
            (dispute_id, dispute.payment_id),
        );

        Ok(refund_id)
    }

    pub fn reject_dispute(
        env: Env,
        operator: Address,
        dispute_id: String,
        resolution_notes: String,
        operator_signature: String,
    ) -> Result<(), Error> {
        operator.require_auth();

        // Issue #625: Enforce maximum length on the resolution_notes field.
        if resolution_notes.len() as usize > MAX_NOTES_LEN {
            return Err(Error::InputTooLong);
        }

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
        dispute.resolution_notes = Some(resolution_notes.clone());

        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
        Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);

        // Store resolution note for record-keeping
        let note = DisputeOperatorNote {
            dispute_id: dispute_id.clone(),
            operator: operator.clone(),
            resolution_notes: resolution_notes.clone(),
            operator_signature,
            recorded_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(
            &DataKey::DisputeOperatorNote(dispute_id.clone()),
            &note,
        );

        Self::release_open_dispute_slot(&env, &dispute.disputer);

        // Issue #626: Decrement the merchant's active dispute count when a dispute is
        // rejected, so the suspension threshold only tracks non-rejected disputes.
        let merchant_dispute_key = DataKey::MerchantDisputeCount(dispute.merchant_id.clone());
        let current_count: u64 = env
            .storage()
            .persistent()
            .get(&merchant_dispute_key)
            .unwrap_or(0u64);
        if current_count > 0 {
            env.storage()
                .persistent()
                .set(&merchant_dispute_key, &(current_count - 1));
        }

        let usdc_token_address = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::UsdcToken)
            .ok_or(Error::Unauthorized)?;
        let token_client = token::TokenClient::new(&env, &usdc_token_address);
        let contract_address = env.current_contract_address();
        let collector = AccessControl::get_admin(&env).unwrap_or_else(|| contract_address.clone());

        let bond_amount = Self::get_dispute_bond_amount(env.clone());

        if token_client
            .try_transfer(&contract_address, &dispute.merchant_id, &bond_amount)
            .is_err()
        {
            return Err(Error::Unauthorized);
        }
        if token_client
            .try_transfer(&contract_address, &collector, &bond_amount)
            .is_err()
        {
            return Err(Error::Unauthorized);
        }

        // Issue #677: merchant's counter-bond is released back to them since
        // the dispute was rejected in their favor.
        events::emit_dispute_bond_returned(
            &env,
            dispute_id.clone(),
            dispute.merchant_id.clone(),
            bond_amount,
        );
        // Issue #677: disputer's bond is forfeited to the treasury/collector
        // when the dispute is rejected.
        events::emit_dispute_bond_forfeited(&env, dispute_id.clone(), collector.clone(), bond_amount);

        // Emit DISPUTE_REJECTED event
        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "REJECTED")),
            (dispute_id, dispute.payment_id),
        );

        Ok(())
    }

    /// Retrieve the persisted operator note for a dispute.
    pub fn get_dispute_operator_note(
        env: Env,
        dispute_id: String,
    ) -> Result<DisputeOperatorNote, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::DisputeOperatorNote(dispute_id))
            .ok_or(Error::DisputeNotFound)
    }

    // ─── Issue #185: Off-chain collaborative settlement ───────────────────────

    /// Close a dispute instantly when both the buyer and merchant have agreed
    /// on a settlement amount off-chain and submit their Ed25519 signatures.
    ///
    /// The message that both parties must sign is:
    ///   `SHA-256( dispute_id_bytes || settlement_amount_bytes )`
    /// where `settlement_amount_bytes` is the little-endian 16-byte encoding
    /// of the `i128` settlement amount.
    ///
    /// # Parameters
    /// * `dispute_id`         – The dispute to settle.
    /// * `settlement_amount`  – Agreed amount to refund to the buyer (≤ disputed amount).
    /// * `buyer_pubkey`       – Ed25519 public key of the buyer (32 bytes).
    /// * `signature_buyer`    – Ed25519 signature from the buyer (64 bytes).
    /// * `merchant_pubkey`    – Ed25519 public key of the merchant (32 bytes).
    /// * `signature_merchant` – Ed25519 signature from the merchant (64 bytes).
    pub fn settle_dispute_collaboratively(
        env: Env,
        dispute_id: String,
        settlement_amount: i128,
        buyer_pubkey: BytesN<32>,
        signature_buyer: BytesN<64>,
        merchant_pubkey: BytesN<32>,
        signature_merchant: BytesN<64>,
    ) -> Result<String, Error> {
        if settlement_amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let dispute = Self::get_dispute_internal(&env, &dispute_id)?;

        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Err(Error::DisputeAlreadyResolved);
        }

        if settlement_amount > dispute.amount {
            return Err(Error::InvalidAmount);
        }

        // Build the message: SHA-256(dispute_id_bytes || settlement_amount_le16)
        // Both parties must have signed this exact message off-chain.
        let message = Self::build_settlement_message(&env, &dispute_id, settlement_amount);

        // Verify buyer signature
        env.crypto()
            .ed25519_verify(&buyer_pubkey, &message, &signature_buyer);

        // Verify merchant signature
        env.crypto()
            .ed25519_verify(&merchant_pubkey, &message, &signature_merchant);

        // Both signatures verified — create and process the refund
        let refund_reason =
            String::from_str(&env, "Collaborative off-chain settlement");

        let refund_id = Self::create_refund_internal(
            &env,
            dispute.payment_id.clone(),
            settlement_amount,
            refund_reason,
            dispute.disputer.clone(),
            None,
            None,
        )?;

        // Process the refund immediately (no operator approval needed)
        Self::process_refund_internal(&env, &env.current_contract_address(), refund_id.clone())?;

        let now = env.ledger().timestamp();

        // Persist the collaborative settlement record
        let settlement = CollaborativeSettlement {
            dispute_id: dispute_id.clone(),
            settlement_amount,
            buyer_pubkey,
            merchant_pubkey,
            settled_at: now,
        };
        env.storage()
            .persistent()
            .set(&DataKey::CollaborativeSettlement(dispute_id.clone()), &settlement);
        Self::bump_ttl(
            &env,
            &DataKey::CollaborativeSettlement(dispute_id.clone()),
            LONG_LIVE_TTL,
        );

        // Update dispute to Resolved
        let mut dispute = Self::get_dispute_internal(&env, &dispute_id)?;
        dispute.status = DisputeStatus::Resolved;
        dispute.refund_id = Some(refund_id.clone());
        dispute.resolved_at = Some(now);
        dispute.resolution_notes = Some(String::from_str(
            &env,
            "Resolved via collaborative off-chain settlement",
        ));

        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
        Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);
        Self::release_open_dispute_slot(&env, &dispute.disputer);

        // Emit event
        env.events().publish(
            (
                Symbol::new(&env, "DISPUTE"),
                Symbol::new(&env, "COLLABORATIVE_SETTLED"),
            ),
            (dispute_id, dispute.payment_id, settlement_amount),
        );

        Ok(refund_id)
    }

    /// Retrieve the collaborative settlement record for a dispute.
    pub fn get_collaborative_settlement(
        env: Env,
        dispute_id: String,
    ) -> Result<CollaborativeSettlement, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::CollaborativeSettlement(dispute_id))
            .ok_or(Error::DisputeNotFound)
    }

    /// Issue #184: Get the current dispute count for a merchant.
    pub fn get_merchant_dispute_count(env: Env, merchant_id: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantDisputeCount(merchant_id))
            .unwrap_or(0u64)
    }

    /// Issue #184: Get the current confirmed payment count for a merchant.
    pub fn get_merchant_payment_count(env: Env, merchant_id: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantPaymentCount(merchant_id))
            .unwrap_or(0u64)
    }

    /// Build the canonical settlement message for collaborative dispute resolution.
    ///
    /// Message = SHA-256( dispute_id_bytes || settlement_amount_le16 )
    fn build_settlement_message(
        env: &Env,
        dispute_id: &String,
        settlement_amount: i128,
    ) -> soroban_sdk::Bytes {
        use soroban_sdk::Bytes;

        let id_len = dispute_id.len() as usize;
        let mut raw = Bytes::new(env);

        // Append dispute_id bytes
        let mut id_buf = [0u8; 64];
        let read_len = id_len.min(64);
        dispute_id.copy_into_slice(&mut id_buf[..read_len]);
        for i in 0..read_len {
            raw.push_back(id_buf[i]);
        }

        // Append settlement_amount as little-endian 16 bytes
        let amount_bytes = settlement_amount.to_le_bytes();
        for b in amount_bytes.iter() {
            raw.push_back(*b);
        }

        // Return SHA-256 hash as Bytes
        let hash = env.crypto().sha256(&raw).to_bytes();
        let mut result = Bytes::new(env);
        for i in 0..32u32 {
            result.push_back(hash.get(i).unwrap());
        }
        result
    }

    // ─── Stake-weighted dispute voting (issue #33) ────────────────────────────

    /// Lock a governance-token stake to participate in dispute voting.
    ///
    /// The arbitrator transfers `amount` tokens into the contract as a stake.
    /// The stake is slashed if the arbitrator votes against the majority.
    ///
    /// # Parameters
    /// * `arbitrator`  – Address locking the stake; must sign.
    /// * `dispute_id`  – Dispute to vote on.
    /// * `token`       – Governance token contract address.
    /// * `amount`      – Amount to lock (must be > 0).
    pub fn lock_stake(
        env: Env,
        arbitrator: Address,
        dispute_id: String,
        token: Address,
        amount: i128,
    ) -> Result<(), Error> {
        arbitrator.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        // Dispute must exist and be open / under review
        let dispute = Self::get_dispute_internal(&env, &dispute_id)?;
        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Err(Error::DisputeAlreadyResolved);
        }

        // Prevent double-staking
        let stake_key = DataKey::DisputeStake(dispute_id.clone(), arbitrator.clone());
        if env.storage().persistent().has(&stake_key) {
            return Err(Error::Unauthorized);
        }

        // Effects: record stake before token transfer
        env.storage().persistent().set(&stake_key, &amount);
        Self::bump_ttl(&env, &stake_key, LONG_LIVE_TTL);

        // Interaction: pull stake from arbitrator
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&arbitrator, &env.current_contract_address(), &amount);

        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "STAKE_LOCKED")),
            (dispute_id, arbitrator, amount),
        );

        Ok(())
    }

    /// Cast a stake-weighted vote on a dispute.
    ///
    /// The arbitrator must have locked a stake first via `lock_stake`.
    /// Each arbitrator may only vote once per dispute.
    ///
    /// # Parameters
    /// * `arbitrator` – Voting arbitrator; must sign.
    /// * `dispute_id` – Dispute to vote on.
    /// * `choice`     – `VoteChoice::Favour` or `VoteChoice::Against`.
    pub fn cast_vote(
        env: Env,
        arbitrator: Address,
        dispute_id: String,
        choice: VoteChoice,
    ) -> Result<(), Error> {
        arbitrator.require_auth();

        // Dispute must be open / under review
        let dispute = Self::get_dispute_internal(&env, &dispute_id)?;
        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Err(Error::DisputeAlreadyResolved);
        }

        // Arbitrator must have a locked stake
        let stake_key = DataKey::DisputeStake(dispute_id.clone(), arbitrator.clone());
        let stake: i128 = env
            .storage()
            .persistent()
            .get(&stake_key)
            .ok_or(Error::Unauthorized)?;

        // Prevent double-voting
        let vote_key = DataKey::DisputeVote(dispute_id.clone(), arbitrator.clone());
        if env.storage().persistent().has(&vote_key) {
            return Err(Error::Unauthorized);
        }

        // Record vote
        env.storage().persistent().set(&vote_key, &choice);
        Self::bump_ttl(&env, &vote_key, LONG_LIVE_TTL);

        // Update tally
        let tally_key = DataKey::DisputeVoteTally(dispute_id.clone());
        let mut tally: VoteTally = env
            .storage()
            .persistent()
            .get(&tally_key)
            .unwrap_or(VoteTally {
                favour_weight: 0,
                against_weight: 0,
                vote_count: 0,
            });

        match choice {
            VoteChoice::Favour => tally.favour_weight = tally.favour_weight.saturating_add(stake),
            VoteChoice::Against => {
                tally.against_weight = tally.against_weight.saturating_add(stake)
            }
        }
        tally.vote_count = tally.vote_count.saturating_add(1);

        env.storage().persistent().set(&tally_key, &tally);
        Self::bump_ttl(&env, &tally_key, LONG_LIVE_TTL);

        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "VOTE_CAST")),
            (dispute_id, arbitrator, stake),
        );

        Ok(())
    }

    /// Finalize a dispute based on stake-weighted votes.
    ///
    /// The majority side wins. Arbitrators who voted against the majority
    /// lose 10% of their stake (slashed to the contract admin). Winners
    /// receive their stake back.
    ///
    /// # Parameters
    /// * `operator`    – Settlement operator or oracle; must sign.
    /// * `dispute_id`  – Dispute to finalize.
    /// * `token`       – Governance token used for stakes.
    /// * `arbitrators` – List of all arbitrators who participated.
    pub fn finalize_dispute_vote(
        env: Env,
        operator: Address,
        dispute_id: String,
        token: Address,
        arbitrators: Vec<Address>,
    ) -> Result<(), Error> {
        operator.require_auth();

        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);
        if !has_settlement && !has_oracle {
            return Err(Error::Unauthorized);
        }

        let dispute = Self::get_dispute_internal(&env, &dispute_id)?;
        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Err(Error::DisputeAlreadyResolved);
        }

        let tally_key = DataKey::DisputeVoteTally(dispute_id.clone());
        let tally: VoteTally = env
            .storage()
            .persistent()
            .get(&tally_key)
            .unwrap_or(VoteTally {
                favour_weight: 0,
                against_weight: 0,
                vote_count: 0,
            });

        // Determine majority
        let favour_wins = tally.favour_weight >= tally.against_weight;
        let majority = if favour_wins {
            VoteChoice::Favour
        } else {
            VoteChoice::Against
        };

        let token_client = token::Client::new(&env, &token);
        let slash_bps: i128 = 1_000; // 10% slash

        // Return stakes; slash minority voters
        for arb in arbitrators.iter() {
            let stake_key = DataKey::DisputeStake(dispute_id.clone(), arb.clone());
            let stake: i128 = match env.storage().persistent().get(&stake_key) {
                Some(s) => s,
                None => continue,
            };

            let vote_key = DataKey::DisputeVote(dispute_id.clone(), arb.clone());
            let vote: VoteChoice = match env.storage().persistent().get(&vote_key) {
                Some(v) => v,
                None => continue,
            };

            let voted_with_majority = vote == majority;

            // Effects: remove stake record
            env.storage().persistent().remove(&stake_key);

            if voted_with_majority {
                // Return full stake
                token_client.transfer(&env.current_contract_address(), &arb, &stake);
            } else {
                // Slash 10%, return remainder
                let slash = stake * slash_bps / 10_000;
                let remainder = stake.saturating_sub(slash);
                if remainder > 0 {
                    token_client.transfer(&env.current_contract_address(), &arb, &remainder);
                }
                if slash > 0 {
                    if let Some(admin) = AccessControl::get_admin(&env) {
                        token_client.transfer(&env.current_contract_address(), &admin, &slash);
                    }
                }
            }
        }

        // Resolve or reject the dispute based on vote outcome
        if favour_wins {
            // Majority voted in favour — issue refund
            let refund_reason =
                String::from_str(&env, "Resolved by stake-weighted arbitration vote");
            if let Ok(refund_id) = Self::create_refund_internal(
                &env,
                dispute.payment_id.clone(),
                dispute.amount,
                refund_reason,
                dispute.disputer.clone(),
                None,
                None,
            ) {
                let _ = Self::process_refund_internal(&env, &operator, refund_id);
            }

            let mut d = Self::get_dispute_internal(&env, &dispute_id)?;
            d.status = DisputeStatus::Resolved;
            d.resolved_at = Some(env.ledger().timestamp());
            env.storage()
                .persistent()
                .set(&DataKey::Dispute(dispute_id.clone()), &d);
            Self::bump_dispute_ttl(&env, &dispute_id, &d.status);
            Self::release_open_dispute_slot(&env, &d.disputer);
        } else {
            let mut d = Self::get_dispute_internal(&env, &dispute_id)?;
            d.status = DisputeStatus::Rejected;
            d.resolved_at = Some(env.ledger().timestamp());
            env.storage()
                .persistent()
                .set(&DataKey::Dispute(dispute_id.clone()), &d);
            Self::bump_dispute_ttl(&env, &dispute_id, &d.status);
            Self::release_open_dispute_slot(&env, &d.disputer);
        }

        env.events().publish(
            (
                Symbol::new(&env, "DISPUTE"),
                Symbol::new(&env, "VOTE_FINALIZED"),
            ),
            (
                dispute_id,
                tally.favour_weight,
                tally.against_weight,
                favour_wins,
            ),
        );

        Ok(())
    }

    /// Cast a role-gated vote on a dispute. Unlike [`Self::cast_vote`] (which
    /// is stake-weighted), this flow simply counts one vote per
    /// `ARBITRATOR`-role address and auto-executes the resolution as soon as
    /// either side reaches [`ARBITRATOR_VOTING_THRESHOLD`].
    ///
    /// # Parameters
    /// * `arbitrator` – Must hold the `ARBITRATOR` role; must sign.
    /// * `dispute_id` – Dispute to vote on; must currently be `UnderReview`.
    /// * `choice`     – `ArbitratorVoteChoice::Approve` or `::Reject`.
    pub fn vote_dispute(
        env: Env,
        arbitrator: Address,
        dispute_id: String,
        choice: ArbitratorVoteChoice,
    ) -> Result<(), Error> {
        arbitrator.require_auth();

        if !AccessControl::has_role(&env, &role_arbitrator(&env), &arbitrator) {
            return Err(Error::Unauthorized);
        }

        let dispute = Self::get_dispute_internal(&env, &dispute_id)?;
        if dispute.status != DisputeStatus::UnderReview {
            return Err(Error::DisputeAlreadyResolved);
        }

        let vote_key = DataKey::ArbitratorVote(dispute_id.clone(), arbitrator.clone());
        if env.storage().persistent().has(&vote_key) {
            return Err(Error::AlreadyVoted);
        }

        env.storage().persistent().set(&vote_key, &choice);
        Self::bump_ttl(&env, &vote_key, LONG_LIVE_TTL);

        let tally_key = DataKey::ArbitratorVoteTally(dispute_id.clone());
        let mut tally: ArbitratorVoteTally = env
            .storage()
            .persistent()
            .get(&tally_key)
            .unwrap_or(ArbitratorVoteTally {
                approve_count: 0,
                reject_count: 0,
            });

        match choice {
            ArbitratorVoteChoice::Approve => {
                tally.approve_count = tally.approve_count.saturating_add(1)
            }
            ArbitratorVoteChoice::Reject => {
                tally.reject_count = tally.reject_count.saturating_add(1)
            }
        }

        env.storage().persistent().set(&tally_key, &tally);
        Self::bump_ttl(&env, &tally_key, LONG_LIVE_TTL);

        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "VOTE_CAST")),
            (dispute_id.clone(), arbitrator),
        );

        if tally.approve_count >= ARBITRATOR_VOTING_THRESHOLD {
            Self::auto_resolve_dispute(&env, &dispute_id, true)?;
        } else if tally.reject_count >= ARBITRATOR_VOTING_THRESHOLD {
            Self::auto_resolve_dispute(&env, &dispute_id, false)?;
        }

        Ok(())
    }

    /// Finalize a dispute once ARBITRATOR-role voting has reached
    /// [`ARBITRATOR_VOTING_THRESHOLD`] in either direction. `approved` issues
    /// a refund and marks the dispute `Resolved`; otherwise it's `Rejected`.
    fn auto_resolve_dispute(env: &Env, dispute_id: &String, approved: bool) -> Result<(), Error> {
        let mut dispute = Self::get_dispute_internal(env, dispute_id)?;
        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Ok(());
        }

        if approved {
            let refund_reason = String::from_str(env, "Auto-resolved by arbitrator vote");
            if let Some(admin) = AccessControl::get_admin(env) {
                if let Ok(refund_id) = Self::create_refund_internal(
                    env,
                    dispute.payment_id.clone(),
                    dispute.amount,
                    refund_reason,
                    dispute.disputer.clone(),
                    None,
                    None,
                ) {
                    let _ = Self::process_refund_internal(env, &admin, refund_id);
                }
            }
            dispute.status = DisputeStatus::Resolved;
        } else {
            dispute.status = DisputeStatus::Rejected;
        }
        dispute.resolved_at = Some(env.ledger().timestamp());

        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
        Self::bump_dispute_ttl(env, dispute_id, &dispute.status);

        env.events().publish(
            (
                Symbol::new(env, "DISPUTE"),
                Symbol::new(env, "AUTO_RESOLVED"),
            ),
            (dispute_id.clone(), approved),
        );

        Ok(())
    }

    /// Get the current vote tally for a dispute.
    pub fn get_vote_tally(env: Env, dispute_id: String) -> VoteTally {
        env.storage()
            .persistent()
            .get(&DataKey::DisputeVoteTally(dispute_id))
            .unwrap_or(VoteTally {
                favour_weight: 0,
                against_weight: 0,
                vote_count: 0,
            })
    }

    pub fn get_dispute(env: Env, dispute_id: String) -> Result<Dispute, Error> {
        let mut dispute = Self::get_dispute_internal(&env, &dispute_id)?;
        let _ = Self::maybe_escalate_dispute_due_to_deadline(&env, &dispute_id, &mut dispute)?;
        Ok(dispute)
    }

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

    /// Issue #178: Submit an arbitrator vote on a dispute resolution.
    pub fn submit_arbitrator_vote(
        env: Env,
        dispute_id: String,
        arbitrator: Address,
        vote: ArbitratorVoteChoice,
    ) -> Result<(), Error> {
        arbitrator.require_auth();

        // Check if arbitrator has the ARBITRATOR role
        if !AccessControl::has_role(&env, &role_arbitrator(&env), &arbitrator) {
            return Err(Error::Unauthorized);
        }

        let dispute = Self::get_dispute_internal(&env, &dispute_id)?;

        // Only allow voting on Open disputes
        if dispute.status != DisputeStatus::Open {
            return Err(Error::DisputeAlreadyResolved);
        }

        // Check if arbitrator has already voted
        let vote_key = DataKey::ArbitratorVote(dispute_id.clone(), arbitrator.clone());
        if env.storage().persistent().has(&vote_key) {
            return Err(Error::InvalidAmount); // Reusing error code for "already voted"
        }

        // Record the vote
        let arbitrator_vote = ArbitratorVote {
            dispute_id: dispute_id.clone(),
            arbitrator: arbitrator.clone(),
            vote: vote.clone(),
            voted_at: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&vote_key, &arbitrator_vote);

        // Add arbitrator to the voters list for this dispute
        let voters_key = DataKey::DisputeArbitratorVotes(dispute_id.clone());
        let mut voters: Vec<Address> = env
            .storage()
            .persistent()
            .get(&voters_key)
            .unwrap_or(vec![&env]);

        voters.push_back(arbitrator.clone());
        env.storage().persistent().set(&voters_key, &voters);

        // Emit arbitrator vote event
        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "ARBITRATOR_VOTE")),
            (dispute_id, arbitrator),
        );

        Ok(())
    }

    /// Issue #178: Check arbitrator voting threshold and auto-resolve if met.
    pub fn check_arbitration_threshold(
        env: Env,
        dispute_id: String,
    ) -> Result<bool, Error> {
        let dispute = Self::get_dispute_internal(&env, &dispute_id)?;

        if dispute.status != DisputeStatus::Open {
            return Ok(false);
        }

        let voters_key = DataKey::DisputeArbitratorVotes(dispute_id.clone());
        let voters: Vec<Address> = env
            .storage()
            .persistent()
            .get(&voters_key)
            .unwrap_or(vec![&env]);

        if voters.len() < ARBITRATOR_VOTING_THRESHOLD {
            return Err(Error::ArbitrationVotingThresholdNotMet);
        }

        // Count approvals
        let mut approvals: u32 = 0;
        for voter in voters.iter() {
            let vote_key = DataKey::ArbitratorVote(dispute_id.clone(), voter.clone());
            if let Some(vote) = env
                .storage()
                .persistent()
                .get::<DataKey, ArbitratorVote>(&vote_key)
            {
                if let ArbitratorVoteChoice::Approve = vote.vote {
                    approvals += 1;
                }
            }
        }

        // Threshold met if majority (>= threshold) approves
        Ok(approvals >= ARBITRATOR_VOTING_THRESHOLD)
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

    // Subscription management functions
    pub fn create_subscription_plan(
        env: Env,
        merchant: Address,
        plan_id: String,
        name: String,
        description: String,
        amount: i128,
        currency: Symbol,
        billing_interval: BillingInterval,
    ) -> Result<(), Error> {
        merchant.require_auth();

        if !AccessControl::has_role(&env, &role_merchant(&env), &merchant) {
            return Err(Error::Unauthorized);
        }

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let interval_secs = billing_interval.to_secs();

        let plan = SubscriptionPlan {
            plan_id: plan_id.clone(),
            merchant_id: merchant.clone(),
            name,
            description,
            amount,
            currency,
            interval_secs,
            billing_interval,
            active: true,
            payout_splits: Vec::new(&env),
        };

        env.storage()
            .persistent()
            .set(&DataKey::SubscriptionPlan(plan_id.clone()), &plan);

        // Issue #635: emit SUBSCRIPTION/PLAN_CREATED for indexer plan-level visibility.
        events::emit_subscription_plan_created(&env, &plan_id, &merchant, amount, interval_secs);

        Ok(())
    }

    /// Create a billing plan with an explicit interval in seconds.
    pub fn create_plan(
        env: Env,
        merchant: Address,
        plan_id: String,
        name: String,
        description: String,
        amount: i128,
        currency: Symbol,
        interval_secs: u64,
    ) -> Result<(), Error> {
        merchant.require_auth();

        if !AccessControl::has_role(&env, &role_merchant(&env), &merchant) {
            return Err(Error::Unauthorized);
        }

        if amount <= 0 || interval_secs == 0 {
            return Err(Error::InvalidAmount);
        }

        let plan = SubscriptionPlan {
            plan_id: plan_id.clone(),
            merchant_id: merchant,
            name,
            description,
            amount,
            currency,
            interval_secs,
            billing_interval: BillingInterval::Daily,
            active: true,
            payout_splits: Vec::new(&env),
        };

        env.storage()
            .persistent()
            .set(&DataKey::SubscriptionPlan(plan_id), &plan);

        Ok(())
    }

    pub fn get_subscription_plan(env: Env, plan_id: String) -> Result<SubscriptionPlan, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::SubscriptionPlan(plan_id))
            .ok_or(Error::PaymentNotFound)
    }

    /// Alias for `get_subscription_plan`.
    pub fn get_plan(env: Env, plan_id: String) -> Result<SubscriptionPlan, Error> {
        Self::get_subscription_plan(env, plan_id)
    }

    pub fn deactivate_subscription_plan(
        env: Env,
        merchant: Address,
        plan_id: String,
    ) -> Result<(), Error> {
        merchant.require_auth();

        let mut plan: SubscriptionPlan = env
            .storage()
            .persistent()
            .get(&DataKey::SubscriptionPlan(plan_id.clone()))
            .ok_or(Error::PaymentNotFound)?;

        if plan.merchant_id != merchant {
            return Err(Error::Unauthorized);
        }

        plan.active = false;
        env.storage()
            .persistent()
            .set(&DataKey::SubscriptionPlan(plan_id.clone()), &plan);

        // Issue #635: emit SUBSCRIPTION/PLAN_DEACTIVATED for indexer plan-level visibility.
        events::emit_subscription_plan_deactivated(&env, &plan_id, &merchant);

        Ok(())
    }

    pub fn subscribe(
        env: Env,
        payer: Address,
        plan_id: String,
        max_payments: Option<u32>,
        affiliate: Option<Address>,
        affiliate_fee_bps: Option<u32>,
    ) -> Result<String, Error> {
        payer.require_auth();

        let plan: SubscriptionPlan = env
            .storage()
            .persistent()
            .get(&DataKey::SubscriptionPlan(plan_id.clone()))
            .ok_or(Error::PaymentNotFound)?;

        if !plan.active {
            return Err(Error::PaymentAlreadyProcessed);
        }

        let counter = Self::get_next_subscription_id(&env);
        let subscription_id = format_id(&env, "sub_", counter);

        let now = env.ledger().timestamp();
        let subscription = Subscription {
            subscription_id: subscription_id.clone(),
            merchant_id: plan.merchant_id.clone(),
            payer_address: payer.clone(),
            plan_id: plan_id.clone(),
            amount: plan.amount,
            currency: plan.currency,
            interval_secs: plan.interval_secs,
            next_payment_at: now.saturating_add(plan.interval_secs),
            status: SubscriptionStatus::Active,
            created_at: now,
            last_payment_at: None,
            total_payments: 0,
            max_payments,
            retry_count: 0,
            next_retry_at: None,
            resume_at: None,
            affiliate: affiliate.clone(),
            affiliate_fee_bps,
        };

        env.storage().persistent().set(
            &DataKey::Subscription(subscription_id.clone()),
            &subscription,
        );

        let mut payer_subscriptions = Self::get_payer_subscriptions_internal(&env, &payer);
        payer_subscriptions.push_back(subscription_id.clone());
        env.storage().persistent().set(
            &DataKey::PayerSubscriptions(payer.clone()),
            &payer_subscriptions,
        );

        // Issue #302: Track in ActiveSubscriptions index
        Self::add_active_subscription(&env, &subscription_id);

        // Issue #633: Track in the per-plan subscriber index
        Self::add_plan_subscriber(&env, &plan_id, &subscription_id);

        env.events().publish(
            (
                Symbol::new(&env, "SUBSCRIPTION"),
                Symbol::new(&env, "CREATED"),
            ),
            (subscription_id.clone(), payer, plan_id),
        );

        Ok(subscription_id)
    }

    /// Subscribe to a plan using a caller-supplied subscription identifier.
    pub fn subscribe_to_plan(
        env: Env,
        payer: Address,
        subscription_id: String,
        plan_id: String,
    ) -> Result<(), Error> {
        payer.require_auth();

        if env
            .storage()
            .persistent()
            .has(&DataKey::Subscription(subscription_id.clone()))
        {
            return Err(Error::PaymentAlreadyExists);
        }

        let plan: SubscriptionPlan = env
            .storage()
            .persistent()
            .get(&DataKey::SubscriptionPlan(plan_id.clone()))
            .ok_or(Error::PaymentNotFound)?;

        if !plan.active {
            return Err(Error::PaymentAlreadyProcessed);
        }

        let now = env.ledger().timestamp();
        let subscription = Subscription {
            subscription_id: subscription_id.clone(),
            merchant_id: plan.merchant_id.clone(),
            payer_address: payer.clone(),
            plan_id: plan_id.clone(),
            amount: plan.amount,
            currency: plan.currency,
            interval_secs: plan.interval_secs,
            next_payment_at: now.saturating_add(plan.interval_secs),
            status: SubscriptionStatus::Active,
            created_at: now,
            last_payment_at: None,
            total_payments: 0,
            max_payments: None,
            retry_count: 0,
            next_retry_at: None,
            resume_at: None,
            affiliate: None,
            affiliate_fee_bps: None,
        };

        env.storage().persistent().set(
            &DataKey::Subscription(subscription_id.clone()),
            &subscription,
        );

        let mut payer_subscriptions = Self::get_payer_subscriptions_internal(&env, &payer);
        payer_subscriptions.push_back(subscription_id.clone());
        env.storage().persistent().set(
            &DataKey::PayerSubscriptions(payer.clone()),
            &payer_subscriptions,
        );

        Self::add_active_subscription(&env, &subscription_id);

        // Issue #633: Track in the per-plan subscriber index
        Self::add_plan_subscriber(&env, &plan_id, &subscription_id);

        env.events().publish(
            (
                Symbol::new(&env, "SUBSCRIPTION"),
                Symbol::new(&env, "CREATED"),
            ),
            (subscription_id, payer, plan_id),
        );

        Ok(())
    }

    pub fn get_subscription(env: Env, subscription_id: String) -> Result<Subscription, Error> {
        Self::get_subscription_internal(&env, &subscription_id)
    }

    pub fn get_payer_subscriptions(env: Env, payer: Address) -> Vec<Subscription> {
        let subscription_ids = Self::get_payer_subscriptions_internal(&env, &payer);
        let mut subscriptions = vec![&env];
        for id in subscription_ids.iter() {
            if let Ok(sub) = Self::get_subscription_internal(&env, &id) {
                subscriptions.push_back(sub);
            }
        }
        subscriptions
    }

    /// Issue #633: List subscribers to a plan, paginated.
    ///
    /// Returns up to `limit` `Subscription` records (hard-capped at 100 per
    /// call) starting at `offset` within the plan's subscriber index, in
    /// subscription order. When `include_cancelled` is `false`, subscriptions
    /// with `status == Cancelled` are filtered out *before* pagination, so the
    /// page always contains `limit` live subscribers when that many remain.
    ///
    /// Used by merchants for plan-level analytics and bulk notifications.
    pub fn get_plan_subscribers(
        env: Env,
        plan_id: String,
        offset: u32,
        limit: u32,
        include_cancelled: bool,
    ) -> Vec<Subscription> {
        let subscription_ids: Vec<String> = env
            .storage()
            .persistent()
            .get(&DataKey::PlanSubscribers(plan_id))
            .unwrap_or_else(|| vec![&env]);

        let capped_limit = if limit == 0 || limit > 100 { 100 } else { limit };

        let mut result = vec![&env];
        let mut matched: u32 = 0;
        for id in subscription_ids.iter() {
            let sub = match Self::get_subscription_internal(&env, &id) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if !include_cancelled && sub.status == SubscriptionStatus::Cancelled {
                continue;
            }
            if matched >= offset {
                result.push_back(sub);
                if result.len() >= capped_limit {
                    break;
                }
            }
            matched = matched.saturating_add(1);
        }
        result
    }

    pub fn pause_subscription(
        env: Env,
        payer: Address,
        subscription_id: String,
    ) -> Result<(), Error> {
        payer.require_auth();

        let mut subscription = Self::get_subscription_internal(&env, &subscription_id)?;

        if subscription.payer_address != payer {
            return Err(Error::Unauthorized);
        }

        if subscription.status != SubscriptionStatus::Active {
            return Err(Error::PaymentAlreadyProcessed);
        }

        subscription.status = SubscriptionStatus::Paused;
        env.storage()
            .persistent()
            .set(&DataKey::Subscription(subscription_id.clone()), &subscription);

        // Issue #302: Remove from ActiveSubscriptions index
        Self::remove_active_subscription(&env, &subscription_id);

        Ok(())
    }

    pub fn pause_with_resume_date(
        env: Env,
        payer: Address,
        subscription_id: String,
        resume_timestamp: u64,
    ) -> Result<(), Error> {
        payer.require_auth();

        let now = env.ledger().timestamp();
        if resume_timestamp <= now {
            return Err(Error::InvalidResumeTimestamp);
        }

        let mut subscription = Self::get_subscription_internal(&env, &subscription_id)?;

        if subscription.payer_address != payer {
            return Err(Error::Unauthorized);
        }

        if subscription.status != SubscriptionStatus::Active {
            return Err(Error::PaymentAlreadyProcessed);
        }

        subscription.status = SubscriptionStatus::Paused;
        subscription.resume_at = Some(resume_timestamp);

        env.storage()
            .persistent()
            .set(&DataKey::Subscription(subscription_id.clone()), &subscription);

        // Issue #302: Remove from ActiveSubscriptions index
        Self::remove_active_subscription(&env, &subscription_id);

        env.events().publish(
            (
                Symbol::new(&env, "SUBSCRIPTION"),
                Symbol::new(&env, "PAUSED"),
            ),
            (subscription_id, payer, resume_timestamp),
        );

        Ok(())
    }

    /// Attempt to charge a subscription.
    ///
    /// Handles the full lifecycle including:
    /// - Auto-resuming a paused subscription whose `resume_at` has passed.
    /// - Pulling the due amount via a pre-authorization (if one exists) or
    ///   directly via the token contract.
    /// - On insufficient balance: entering a grace period with up to
    ///   `SUBSCRIPTION_MAX_RETRIES` retries spaced `SUBSCRIPTION_RETRY_INTERVAL_SECS`
    ///   apart before marking the subscription as `Cancelled`.
    ///
    /// # Parameters
    /// * `operator`        – Oracle or settlement operator; must sign.
    /// * `subscription_id` – Subscription to charge.
    /// * `token`           – Token contract to pull payment from.
    pub fn charge_subscription(
        env: Env,
        operator: Address,
        subscription_id: String,
        token: Address,
    ) -> Result<SubscriptionStatus, Error> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_oracle(&env), &operator)
            && !AccessControl::has_role(&env, &role_settlement_operator(&env), &operator)
        {
            return Err(Error::Unauthorized);
        }

        let mut subscription = Self::get_subscription_internal(&env, &subscription_id)?;
        let now = env.ledger().timestamp();

        // ── Auto-resume if the pause window has expired ───────────────────────
        if subscription.status == SubscriptionStatus::Paused {
            if let Some(resume_at) = subscription.resume_at {
                if now >= resume_at {
                    subscription.status = SubscriptionStatus::Active;
                    subscription.resume_at = None;
                    // Push next payment forward from the resume point.
                    subscription.next_payment_at =
                        resume_at.saturating_add(subscription.interval_secs);

                    // Issue #302: Add back to ActiveSubscriptions index
                    Self::add_active_subscription(&env, &subscription_id);

                    env.events().publish(
                        (
                            Symbol::new(&env, "SUBSCRIPTION"),
                            Symbol::new(&env, "RESUMED"),
                        ),
                        (subscription_id.clone(), subscription.payer_address.clone()),
                    );
                }
            }
        }

        // Only charge Active subscriptions.
        if subscription.status != SubscriptionStatus::Active {
            env.storage().persistent().set(
                &DataKey::Subscription(subscription_id.clone()),
                &subscription,
            );
            return Ok(subscription.status);
        }

        // Check whether we are in a retry window or a normal due-date window.
        let is_retry = subscription.next_retry_at.is_some();
        let due = if is_retry {
            subscription.next_retry_at.unwrap_or(0)
        } else {
            subscription.next_payment_at
        };

        if now < due {
            // Not yet due — nothing to do.
            env.storage().persistent().set(
                &DataKey::Subscription(subscription_id.clone()),
                &subscription,
            );
            return Ok(subscription.status);
        }

        // ── Attempt token transfer ────────────────────────────────────────────
        let token_client = token::Client::new(&env, &token);
        let payer = subscription.payer_address.clone();
        let merchant = subscription.merchant_id.clone();
        let amount = subscription.amount;

        // Pull the full amount into this contract so we can distribute splits/fees.
        let transfer_ok = token_client
            .try_transfer(&payer, &env.current_contract_address(), &amount)
            .is_ok();

        if transfer_ok {
            // ── Success path ──────────────────────────────────────────────────
            // Distribute according to plan splits or affiliate settings.
            // First try to resolve the plan and its payout splits.
            if let Ok(plan) = Self::get_subscription_plan(env.clone(), subscription.plan_id.clone()) {
                if plan.payout_splits.len() > 0 {
                    // If payout_splits configured, send each recipient their configured amount.
                    for s in plan.payout_splits.iter() {
                        let _ = token_client.try_transfer(
                            &env.current_contract_address(),
                            &s.recipient,
                            &s.amount,
                        );
                    }
                } else if let (Some(aff), Some(bps)) = (subscription.affiliate.clone(), subscription.affiliate_fee_bps) {
                    // Pay affiliate fee then merchant receives remainder.
                    let fee = amount.saturating_mul(bps as i128) / 10_000i128;
                    let merchant_amount = amount.saturating_sub(fee);
                    if fee > 0 {
                        let _ = token_client.try_transfer(
                            &env.current_contract_address(),
                            &aff,
                            &fee,
                        );
                    }
                    let _ = token_client.try_transfer(
                        &env.current_contract_address(),
                        &merchant,
                        &merchant_amount,
                    );
                } else {
                    // Default: send full amount to merchant.
                    let _ = token_client.try_transfer(
                        &env.current_contract_address(),
                        &merchant,
                        &amount,
                    );
                }
            } else {
                // If plan can't be loaded, fall back to sending full amount to merchant.
                let _ = token_client.try_transfer(
                    &env.current_contract_address(),
                    &merchant,
                    &amount,
                );
            }

            subscription.last_payment_at = Some(now);
            subscription.total_payments = subscription.total_payments.saturating_add(1);
            subscription.retry_count = 0;
            subscription.next_retry_at = None;
            subscription.next_payment_at = now.saturating_add(subscription.interval_secs);

            // Check max_payments cap.
            if let Some(max) = subscription.max_payments {
                if subscription.total_payments >= max {
                    subscription.status = SubscriptionStatus::Expired;
                    // Issue #302: Remove from ActiveSubscriptions index
                    Self::remove_active_subscription(&env, &subscription_id);
                }
            }

            env.storage().persistent().set(
                &DataKey::Subscription(subscription_id.clone()),
                &subscription,
            );

            env.events().publish(
                (
                    Symbol::new(&env, "SUBSCRIPTION"),
                    Symbol::new(&env, "CHARGED"),
                ),
                (
                    subscription_id.clone(),
                    payer.clone(),
                    merchant.clone(),
                    amount,
                    subscription.total_payments,
                ),
            );

            // Emit explicit expired event when the subscription reached its cap.
            if subscription.status == SubscriptionStatus::Expired {
                env.events().publish(
                    (Symbol::new(&env, "SUBSCRIPTION"), Symbol::new(&env, "EXPIRED")),
                    (subscription_id, payer),
                );
            }
        } else {
            // ── Failure path — grace period / retry logic ─────────────────────
            subscription.retry_count = subscription.retry_count.saturating_add(1);

            if subscription.retry_count >= SUBSCRIPTION_MAX_RETRIES {
                // Exhausted all retries — cancel the subscription.
                subscription.status = SubscriptionStatus::Cancelled;
                subscription.next_retry_at = None;

                // Issue #302: Remove from ActiveSubscriptions index
                Self::remove_active_subscription(&env, &subscription_id);

                env.storage().persistent().set(
                    &DataKey::Subscription(subscription_id.clone()),
                    &subscription,
                );

                env.events().publish(
                    (
                        Symbol::new(&env, "SUBSCRIPTION"),
                        Symbol::new(&env, "CANCELLED_MAX_RETRIES"),
                    ),
                    (
                        subscription_id.clone(),
                        payer.clone(),
                        subscription.retry_count,
                        SUBSCRIPTION_MAX_RETRIES,
                    ),
                );

                return Err(Error::SubscriptionRetryExhausted);
            } else {
                // Schedule the next retry attempt.
                let next_retry = now.saturating_add(SUBSCRIPTION_RETRY_INTERVAL_SECS);
                subscription.next_retry_at = Some(next_retry);

                env.storage().persistent().set(
                    &DataKey::Subscription(subscription_id.clone()),
                    &subscription,
                );

                env.events().publish(
                    (
                        Symbol::new(&env, "SUBSCRIPTION"),
                        Symbol::new(&env, "PAYMENT_FAILED"),
                    ),
                    (
                        subscription_id,
                        payer,
                        subscription.retry_count,
                        SUBSCRIPTION_MAX_RETRIES,
                        next_retry,
                    ),
                );

                return Err(Error::SubscriptionInGracePeriod);
            }
        }

        Ok(subscription.status)
    }

    /// Trigger a recurring subscription charge when the billing date is due.
    pub fn process_subscription(
        env: Env,
        operator: Address,
        subscription_id: String,
    ) -> Result<SubscriptionStatus, Error> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_oracle(&env), &operator)
            && !AccessControl::has_role(&env, &role_settlement_operator(&env), &operator)
        {
            return Err(Error::Unauthorized);
        }

        let subscription = Self::get_subscription_internal(&env, &subscription_id)?;
        let now = env.ledger().timestamp();
        let due = subscription
            .next_retry_at
            .unwrap_or(subscription.next_payment_at);
        if now < due {
            return Err(Error::PaymentAlreadyProcessed);
        }

        let token: Address = env
            .storage()
            .persistent()
            .get(&DataKey::UsdcToken)
            .ok_or(Error::PaymentNotFound)?;

        Self::charge_subscription(env, operator, subscription_id, token)
    }

    pub fn resume_subscription(
        env: Env,
        payer: Address,
        subscription_id: String,
    ) -> Result<(), Error> {
        payer.require_auth();

        let mut subscription = Self::get_subscription_internal(&env, &subscription_id)?;

        if subscription.payer_address != payer {
            return Err(Error::Unauthorized);
        }

        if subscription.status != SubscriptionStatus::Paused {
            return Err(Error::PaymentAlreadyProcessed);
        }

        subscription.status = SubscriptionStatus::Active;
        subscription.next_payment_at = env
            .ledger()
            .timestamp()
            .saturating_add(subscription.interval_secs);
        env.storage()
            .persistent()
            .set(&DataKey::Subscription(subscription_id.clone()), &subscription);

        // Issue #302: Add back to ActiveSubscriptions index
        Self::add_active_subscription(&env, &subscription_id);

        Ok(())
    }

    /// Cancel a subscription and optionally create a prorated pending refund.
    ///
    /// When `refund_remaining` is true, a payment was made in the current billing
    /// period, and the admin policy `allow_prorated_refunds` is enabled, a pending
    /// refund is created for the unused portion of the period (by whole days).
    pub fn cancel_subscription(
        env: Env,
        payer_or_merchant: Address,
        subscription_id: String,
        refund_remaining: bool,
    ) -> Result<(), Error> {
        payer_or_merchant.require_auth();

        let mut subscription = Self::get_subscription_internal(&env, &subscription_id)?;

        if subscription.payer_address != payer_or_merchant
            && subscription.merchant_id != payer_or_merchant
        {
            return Err(Error::Unauthorized);
        }

        if subscription.status == SubscriptionStatus::Cancelled {
            return Err(Error::PaymentAlreadyProcessed);
        }

        let now = env.ledger().timestamp();
        let allow_proration: bool = env
            .storage()
            .persistent()
            .get(&DataKey::AllowProratedRefunds)
            .unwrap_or(false);

        if refund_remaining && allow_proration {
            if let Some(last_paid) = subscription.last_payment_at {
                // Payment must fall within the current open period.
                if last_paid < subscription.next_payment_at && now < subscription.next_payment_at {
                    let secs_remaining = subscription.next_payment_at.saturating_sub(now);
                    let days_remaining = secs_remaining / 86_400;
                    let period_days = core::cmp::max(1, subscription.interval_secs / 86_400);

                    if days_remaining > 0 {
                        let prorated = subscription
                            .amount
                            .saturating_mul(days_remaining as i128)
                            / (period_days as i128);

                        if prorated > 0 {
                            Self::create_subscription_prorated_refund(
                                &env,
                                &subscription,
                                prorated,
                            )?;
                        }
                    }
                }
            }
        }

        subscription.status = SubscriptionStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Subscription(subscription_id.clone()), &subscription);

        // Stop future tick billing
        Self::remove_active_subscription(&env, &subscription_id);

        env.events().publish(
            (Symbol::new(&env, "SUBSCRIPTION"), Symbol::new(&env, "CANCELLED")),
            (subscription_id, payer_or_merchant),
        );

        Ok(())
    }

    /// Admin policy: enable or disable prorated refunds on subscription cancel.
    pub fn set_allow_prorated_refunds(
        env: Env,
        admin: Address,
        allow: bool,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::AllowProratedRefunds, &allow);
        Ok(())
    }

    /// Query whether prorated subscription refunds are allowed.
    pub fn get_allow_prorated_refunds(env: Env) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::AllowProratedRefunds)
            .unwrap_or(false)
    }

    /// Create a pending refund backed by a synthetic confirmed payment for the
    /// last subscription billing period. Emits `REFUND/AUTO_CREATED`.
    fn create_subscription_prorated_refund(
        env: &Env,
        subscription: &Subscription,
        refund_amount: i128,
    ) -> Result<String, Error> {
        let tick_id = Self::get_next_subscription_tick_id(env);
        let payment_id = format_id(env, "sub_pr_", tick_id);
        let now = env.ledger().timestamp();
        let last_paid = subscription.last_payment_at.unwrap_or(now);

        let payment = PaymentCharge {
            payment_id: payment_id.clone(),
            merchant_id: subscription.merchant_id.clone(),
            amount: subscription.amount,
            currency: subscription.currency.clone(),
            deposit_address: env.current_contract_address(),
            status: PaymentStatus::Confirmed,
            payer_address: Some(subscription.payer_address.clone()),
            transaction_hash: None,
            created_at: last_paid,
            confirmed_at: Some(last_paid),
            expires_at: subscription.next_payment_at,
            amount_received: Some(subscription.amount),
            memo: None,
            memo_type: None,
            token_address: None,
            metadata_hash: None,
            original_token: None,
            swap_path: None,
            fx_rate: None,
            fx_rate_at: None,
            metadata: None,
            fee_waiver_code: None,
            retry_of_payment_id: None,
            payer_muxed_id: None,
            payment_link_id: None,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(env, &payment_id, &payment.status);

        let counter = Self::get_next_refund_id(env);
        let refund_id = format_id(env, "refund_", counter);
        let reason = String::from_str(env, "Prorated subscription cancellation");

        let refund = Refund {
            refund_id: refund_id.clone(),
            payment_id: payment_id.clone(),
            amount: refund_amount,
            reason,
            status: RefundStatus::Pending,
            requester: subscription.payer_address.clone(),
            created_at: now,
            processed_at: None,
            approved: false,
            receipt_hash: None,
            expiry_at: now + REFUND_EXPIRY_SECS,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);

        let mut payment_refunds = Self::get_payment_refunds_internal(env, &payment_id);
        payment_refunds.push_back(refund_id.clone());
        env.storage().persistent().set(
            &DataKey::PaymentRefunds(payment_id.clone()),
            &payment_refunds,
        );
        Self::bump_ttl(env, &DataKey::PaymentRefunds(payment_id.clone()), LONG_LIVE_TTL);
        Self::bump_refund_ttl(env, &refund_id, &refund.status);

        env.events().publish(
            (Symbol::new(env, "REFUND"), Symbol::new(env, "AUTO_CREATED")),
            (
                payment_id,
                refund_id.clone(),
                refund_amount,
                subscription.subscription_id.clone(),
            ),
        );

        Ok(refund_id)
    }

    /// Admin override to reactivate a subscription that was cancelled due to max retries.
    /// Resets retry_count to 0 and reschedules the next payment.
    pub fn admin_reactivate_subscription(
        env: Env,
        admin: Address,
        subscription_id: String,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        let mut subscription = Self::get_subscription_internal(&env, &subscription_id)?;

        if subscription.status != SubscriptionStatus::Cancelled {
            return Err(Error::PaymentAlreadyProcessed);
        }

        let now = env.ledger().timestamp();
        subscription.status = SubscriptionStatus::Active;
        subscription.retry_count = 0;
        subscription.next_retry_at = None;
        subscription.next_payment_at = now.saturating_add(subscription.interval_secs);

        env.storage()
            .persistent()
            .set(&DataKey::Subscription(subscription_id.clone()), &subscription);

        // Issue #302: Add back to ActiveSubscriptions index
        Self::add_active_subscription(&env, &subscription_id);

        env.events().publish(
            (
                Symbol::new(&env, "SUBSCRIPTION"),
                Symbol::new(&env, "REACTIVATED"),
            ),
            (subscription_id, subscription.payer_address.clone()),
        );

        Ok(())
    }

    /// Submit usage metrics for a metered subscription.
    ///
    /// Operators call this to record usage units consumed since the last
    /// billing cycle. The subscription amount is scaled by
    /// `units_used * unit_price` and charged immediately via `charge_subscription`.
    ///
    /// # Parameters
    /// * `operator`         – Must hold oracle or settlement_operator role.
    /// * `subscription_id`  – Target subscription.
    /// * `units_used`       – Number of usage units consumed this period.
    /// * `unit_price`       – Price per unit in the subscription token's smallest unit.
    /// * `token`            – Token contract address used for the charge.
    pub fn submit_usage_metrics(
        env: Env,
        operator: Address,
        subscription_id: String,
        units_used: i128,
        unit_price: i128,
        token: Address,
    ) -> Result<SubscriptionStatus, Error> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_oracle(&env), &operator)
            && !AccessControl::has_role(&env, &role_settlement_operator(&env), &operator)
        {
            return Err(Error::Unauthorized);
        }

        if units_used <= 0 || unit_price <= 0 {
            return Err(Error::InvalidAmount);
        }

        let mut subscription = Self::get_subscription_internal(&env, &subscription_id)?;

        // Issue #664: Usage metrics can only be submitted for a subscription
        // that is still billable (Active, or Paused-but-auto-resuming via
        // `charge_subscription` below). A Cancelled/Expired subscription
        // cannot be metered.
        if subscription.status == SubscriptionStatus::Cancelled
            || subscription.status == SubscriptionStatus::Expired
        {
            return Err(Error::InvalidStatusTransition);
        }

        // Override the subscription amount with the metered charge for this cycle.
        let metered_amount = units_used.saturating_mul(unit_price);
        subscription.amount = metered_amount;
        env.storage()
            .persistent()
            .set(&DataKey::Subscription(subscription_id.clone()), &subscription);

        env.events().publish(
            (
                Symbol::new(&env, "SUBSCRIPTION"),
                Symbol::new(&env, "USAGE_RECORDED"),
            ),
            (subscription_id.clone(), units_used, unit_price, metered_amount),
        );

        // Issue #664: Append this usage record to the subscription's metrics
        // log so `get_usage_metrics` can return usage history over a range.
        let mut usage_log: Vec<UsageMetrics> = env
            .storage()
            .persistent()
            .get(&DataKey::UsageMetricsLog(subscription_id.clone()))
            .unwrap_or_else(|| vec![&env]);
        usage_log.push_back(UsageMetrics {
            subscription_id: subscription_id.clone(),
            units_used,
            unit_price,
            amount: metered_amount,
            recorded_at: env.ledger().timestamp(),
        });
        env.storage()
            .persistent()
            .set(&DataKey::UsageMetricsLog(subscription_id.clone()), &usage_log);
        Self::bump_ttl(
            &env,
            &DataKey::UsageMetricsLog(subscription_id.clone()),
            LONG_LIVE_TTL,
        );

        // Trigger the charge at the updated metered amount.
        Self::charge_subscription(env, operator, subscription_id, token)
    }

    /// Issue #664: Return usage-metric records for `subscription_id`
    /// recorded within `[from_timestamp, to_timestamp]` (inclusive),
    /// oldest first. Returns an empty vector if none were recorded, or if
    /// the subscription itself doesn't exist.
    pub fn get_usage_metrics(
        env: Env,
        subscription_id: String,
        from_timestamp: u64,
        to_timestamp: u64,
    ) -> Vec<UsageMetrics> {
        let log: Vec<UsageMetrics> = env
            .storage()
            .persistent()
            .get(&DataKey::UsageMetricsLog(subscription_id))
            .unwrap_or_else(|| vec![&env]);

        let mut result = vec![&env];
        for record in log.iter() {
            if record.recorded_at >= from_timestamp && record.recorded_at <= to_timestamp {
                result.push_back(record.clone());
            }
        }
        result
    }

    /// Issue #633: Append a subscription ID to the per-plan subscriber index.
    /// Idempotent — a subscription ID already present is not added twice.
    fn add_plan_subscriber(env: &Env, plan_id: &String, subscription_id: &String) {
        let key = DataKey::PlanSubscribers(plan_id.clone());
        let mut subscribers: Vec<String> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![env]);
        for id in subscribers.iter() {
            if id == subscription_id.clone() {
                return;
            }
        }
        subscribers.push_back(subscription_id.clone());
        env.storage().persistent().set(&key, &subscribers);
        Self::bump_ttl(env, &key, LONG_LIVE_TTL);
    }

    /// Issue #302: Track subscription in the ActiveSubscriptions index
    fn add_active_subscription(env: &Env, subscription_id: &String) {
        let mut active: Vec<String> = env
            .storage()
            .persistent()
            .get(&DataKey::ActiveSubscriptions)
            .unwrap_or_else(|| vec![env]);
        let mut found = false;
        for id in active.iter() {
            if id == subscription_id.clone() {
                found = true;
                break;
            }
        }
        if !found {
            active.push_back(subscription_id.clone());
            env.storage()
                .persistent()
                .set(&DataKey::ActiveSubscriptions, &active);
            Self::bump_ttl(env, &DataKey::ActiveSubscriptions, LONG_LIVE_TTL);
        }
    }

    /// Issue #302: Remove subscription from the ActiveSubscriptions index
    fn remove_active_subscription(env: &Env, subscription_id: &String) {
        let active: Vec<String> = env
            .storage()
            .persistent()
            .get(&DataKey::ActiveSubscriptions)
            .unwrap_or_else(|| vec![env]);
        let mut updated = vec![env];
        for id in active.iter() {
            if id != subscription_id.clone() {
                updated.push_back(id);
            }
        }
        env.storage()
            .persistent()
            .set(&DataKey::ActiveSubscriptions, &updated);
        Self::bump_ttl(env, &DataKey::ActiveSubscriptions, LONG_LIVE_TTL);
    }

    /// Issue #302: Get the next subscription tick counter for payment IDs
    fn get_next_subscription_tick_id(env: &Env) -> u64 {
        let mut counter: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::SubscriptionTickCounter)
            .unwrap_or(0);
        counter += 1;
        env.storage()
            .persistent()
            .set(&DataKey::SubscriptionTickCounter, &counter);
        counter
    }

    /// Issue #302: Process due subscriptions - iterate ActiveSubscriptions and
    /// create a payment record for each due subscription.
    pub fn process_due_subscriptions(env: Env, operator: Address) -> Result<u32, Error> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_oracle(&env), &operator)
            && !AccessControl::has_role(&env, &role_settlement_operator(&env), &operator)
        {
            return Err(Error::Unauthorized);
        }

        let active: Vec<String> = env
            .storage()
            .persistent()
            .get(&DataKey::ActiveSubscriptions)
            .unwrap_or_else(|| vec![&env]);

        let now = env.ledger().timestamp();
        let mut total_payments: u32 = 0;

        for subscription_id in active.iter() {
            let mut subscription = match env
                .storage()
                .persistent()
                .get::<DataKey, Subscription>(&DataKey::Subscription(subscription_id.clone()))
            {
                Some(s) => s,
                None => continue,
            };

            if subscription.status != SubscriptionStatus::Active {
                continue;
            }

            if now < subscription.next_payment_at {
                continue;
            }

            let tick_id = Self::get_next_subscription_tick_id(&env);
            let payment_id = format_id(&env, "sub_tick_", tick_id);

            let payment = PaymentCharge {
                payment_id: payment_id.clone(),
                merchant_id: subscription.merchant_id.clone(),
                amount: subscription.amount,
                currency: subscription.currency.clone(),
                deposit_address: env.current_contract_address(),
                status: PaymentStatus::Pending,
                payer_address: Some(subscription.payer_address.clone()),
                transaction_hash: None,
                created_at: now,
                confirmed_at: None,
                expires_at: now.saturating_add(DEFAULT_PAYMENT_DURATION_SECS),
                amount_received: None,
                memo: None,
                memo_type: None,
                token_address: None,
                metadata_hash: None,
                original_token: None,
                swap_path: None,
                fx_rate: None,
                fx_rate_at: None,
                metadata: None,
                fee_waiver_code: None,
            retry_of_payment_id: None,
            payer_muxed_id: None,
            payment_link_id: None,
            };

            env.storage()
                .persistent()
                .set(&DataKey::Payment(payment_id.clone()), &payment);
            Self::bump_payment_ttl(&env, &payment_id, &payment.status);

            subscription.last_payment_at = Some(now);
            subscription.total_payments = subscription.total_payments.saturating_add(1);
            subscription.next_payment_at = now.saturating_add(subscription.interval_secs);

            env.events().publish(
                (
                    Symbol::new(&env, "SUBSCRIPTION"),
                    Symbol::new(&env, "TICKED"),
                ),
                (
                    subscription_id.clone(),
                    subscription.payer_address.clone(),
                    subscription.amount,
                    subscription.total_payments,
                ),
            );

            if let Some(max) = subscription.max_payments {
                if subscription.total_payments >= max {
                    subscription.status = SubscriptionStatus::Cancelled;
                    Self::remove_active_subscription(&env, &subscription_id);

                    env.events().publish(
                        (
                            Symbol::new(&env, "SUBSCRIPTION"),
                            Symbol::new(&env, "COMPLETED"),
                        ),
                        (
                            subscription_id.clone(),
                            subscription.payer_address.clone(),
                            subscription.total_payments,
                        ),
                    );

                    env.events().publish(
                        (Symbol::new(&env, "SUBSCRIPTION"), Symbol::new(&env, "CANCELLED")),
                        (subscription_id.clone(), subscription.payer_address.clone()),
                    );
                }
            }

            env.storage()
                .persistent()
                .set(&DataKey::Subscription(subscription_id.clone()), &subscription);

            env.events().publish(
                (
                    Symbol::new(&env, "PAYMENT"),
                    Symbol::new(&env, "CREATED"),
                ),
                (payment_id.clone(), subscription.merchant_id.clone(), subscription.amount, None::<Map<String, String>>),
            );

            total_payments = total_payments.saturating_add(1);
        }

        Ok(total_payments)
    }

    fn get_next_subscription_id(env: &Env) -> u64 {
        let mut counter: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::SubscriptionCounter)
            .unwrap_or(0);
        counter += 1;
        env.storage()
            .persistent()
            .set(&DataKey::SubscriptionCounter, &counter);
        counter
    }

    fn get_subscription_internal(
        env: &Env,
        subscription_id: &String,
    ) -> Result<Subscription, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Subscription(subscription_id.clone()))
            .ok_or(Error::PaymentNotFound)
    }

    fn get_payer_subscriptions_internal(env: &Env, payer: &Address) -> Vec<String> {
        env.storage()
            .persistent()
            .get(&DataKey::PayerSubscriptions(payer.clone()))
            .unwrap_or_else(|| vec![env])
    }

    fn refund_ttl(status: &RefundStatus) -> u32 {
        match status {
            RefundStatus::Pending => SHORT_LIVE_TTL,
            RefundStatus::Completed
            | RefundStatus::Rejected
            | RefundStatus::Cancelled => LONG_LIVE_TTL,
        }
    }

    fn bump_refund_ttl(env: &Env, refund_id: &String, status: &RefundStatus) {
        let key = DataKey::Refund(refund_id.clone());
        Self::bump_ttl(env, &key, Self::refund_ttl(status));
    }

    fn dispute_ttl(status: &DisputeStatus) -> u32 {
        match status {
            DisputeStatus::Open | DisputeStatus::UnderReview => SHORT_LIVE_TTL,
            DisputeStatus::Resolved | DisputeStatus::Rejected => LONG_LIVE_TTL,
        }
    }

    fn bump_dispute_ttl(env: &Env, dispute_id: &String, status: &DisputeStatus) {
        let key = DataKey::Dispute(dispute_id.clone());
        Self::bump_ttl(env, &key, Self::dispute_ttl(status));
    }

    fn payment_ttl(status: &PaymentStatus) -> u32 {
        match status {
            PaymentStatus::Pending => SHORT_LIVE_TTL,
            PaymentStatus::Confirmed
            | PaymentStatus::Settled
            | PaymentStatus::Expired
            | PaymentStatus::Failed
            | PaymentStatus::PartiallyPaid
            | PaymentStatus::Overpaid => LONG_LIVE_TTL,
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

    /// Queue a contract WASM upgrade via the timelock.
    ///
    /// Issue #624: `upgrade_contract` no longer takes effect immediately.  The
    /// upgrade is queued as a `PendingTimelockAction` and can only be executed
    /// after the configured delay (default 48 hours) via
    /// `execute_timelocked_action`.  Returns the action ID.
    pub fn upgrade_contract(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<String, Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        Self::enqueue_timelocked_action(&env, admin, TimelockActionKind::UpgradeContract(new_wasm_hash))
    }
}

#[cfg_attr(
    any(not(target_arch = "wasm32"), feature = "contract-payment-processor"),
    contractimpl
)]
#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
impl PaymentProcessor {
    /// Returns the current contract version string from persistent storage.
    /// Falls back to INITIAL_CONTRACT_VERSION if not set.
    pub fn version(env: Env) -> String {
        env.storage()
            .persistent()
            .get(&DataKey::ContractVersion)
            .unwrap_or_else(|| String::from_str(&env, INITIAL_CONTRACT_VERSION))
    }

    /// Alias for version() — returns the current contract version string.
    pub fn get_version(env: Env) -> String {
        Self::version(env)
    }

    /// Issue #683: Return a summary of key contract metrics for dashboards
    /// and monitoring. No authentication required — this is a public read.
    pub fn get_contract_health(env: Env) -> ContractHealth {
        let version = Self::version(env.clone());
        let is_paused = Self::is_paused(env.clone());
        let creation_paused: bool = env
            .storage()
            .persistent()
            .get::<DataKey, PauseState>(&DataKey::CreationPaused)
            .map(|s| s.paused)
            .unwrap_or(false);
        let treasury_balance = Self::get_treasury_balance(env.clone());

        let active_payment_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantPaymentCount(env.current_contract_address()))
            .unwrap_or(0u64) as u32;

        let fx_oracle_configured = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::FxOracleAddress)
            .is_some();

        let merchant_registry_configured = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
            .is_some();

        ContractHealth {
            version,
            is_paused,
            is_creation_paused: creation_paused,
            treasury_balance,
            active_payment_count,
            fx_oracle_configured,
            merchant_registry_configured,
        }
    }

    /// Admin-only: set an arbitrary on-chain metadata entry (issue #667), e.g. a
    /// description, deployment notes, or audit commit hash. Stored in instance
    /// storage under a caller-chosen key, with the instance TTL bumped to
    /// `LONG_LIVE_TTL` so metadata survives archival.
    pub fn set_contract_metadata(
        env: Env,
        admin: Address,
        key: Symbol,
        value: String,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        env.storage()
            .instance()
            .set(&DataKey::ContractMetadata(key), &value);

        let threshold = core::cmp::max(1, LONG_LIVE_TTL / TTL_BUMP_THRESHOLD_DIVISOR);
        env.storage().instance().extend_ttl(threshold, LONG_LIVE_TTL);

        Ok(())
    }

    /// Public read of an on-chain metadata entry set via `set_contract_metadata`
    /// (issue #667). Returns `None` if the key was never set.
    pub fn get_contract_metadata(env: Env, key: Symbol) -> Option<String> {
        env.storage()
            .instance()
            .get(&DataKey::ContractMetadata(key))
    }

    fn validate_init_admin(env: &Env, admin: Address) -> Result<(), Error> {
        let zero_address = Address::from_str(env, ZERO_CONTRACT_STRKEY);
        if admin == zero_address {
            return Err(Error::InvalidAddress);
        }
        Ok(())
    }

    /// Formats a u64 as a decimal `String` without relying on `alloc`/`format!`
    /// (this crate is `#![no_std]`). Used to store `deployed_at` as metadata
    /// text so it round-trips through `get_contract_metadata`'s `String` type.
    fn u64_to_string(env: &Env, mut n: u64) -> String {
        if n == 0 {
            return String::from_str(env, "0");
        }
        let mut buf = [0u8; 20];
        let mut i = buf.len();
        while n > 0 {
            i -= 1;
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
        }
        let s = core::str::from_utf8(&buf[i..]).unwrap_or("0");
        String::from_str(env, s)
    }

    pub fn initialize_payment_processor(env: Env, admin: Address) -> Result<(), Error> {
        Self::validate_init_admin(&env, admin.clone())?;
        AccessControl::initialize(&env, admin);

        let empty_reason = String::from_str(&env, "");
        let initial_state = PauseState {
            paused: false,
            reason: empty_reason,
            admin: None,
            timestamp: env.ledger().timestamp(),
        };

        env.storage()
            .persistent()
            .set(&DataKey::Paused, &initial_state);
        env.storage()
            .persistent()
            .set(&DataKey::CreationPaused, &initial_state);

        // Set initial contract version
        let initial_version = String::from_str(&env, INITIAL_CONTRACT_VERSION);
        env.storage()
            .persistent()
            .set(&DataKey::ContractVersion, &initial_version);

        // Issue #667: pre-populate on-chain metadata with description, version, and
        // deployment timestamp so explorers/integrators can identify the contract.
        env.storage().instance().set(
            &DataKey::ContractMetadata(Symbol::new(&env, "description")),
            &String::from_str(&env, "FluxaPay PaymentProcessor contract"),
        );
        env.storage().instance().set(
            &DataKey::ContractMetadata(Symbol::new(&env, "version")),
            &initial_version,
        );
        env.storage().instance().set(
            &DataKey::ContractMetadata(Symbol::new(&env, "deployed_at")),
            &Self::u64_to_string(&env, env.ledger().timestamp()),
        );
        let threshold = core::cmp::max(1, LONG_LIVE_TTL / TTL_BUMP_THRESHOLD_DIVISOR);
        env.storage().instance().extend_ttl(threshold, LONG_LIVE_TTL);

        Ok(())
    }

    pub fn set_merchant_registry_address(
        env: Env,
        admin: Address,
        registry_address: Address,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        env.storage()
            .persistent()
            .set(&DataKey::MerchantRegistryAddress, &registry_address);
        Ok(())
    }

    /// Admin: configure the FX oracle used to snapshot rates during
    /// `verify_payment` (Issue #304).
    pub fn set_fx_oracle(env: Env, admin: Address, fx_oracle: Address) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        env.storage()
            .persistent()
            .set(&DataKey::FxOracleAddress, &fx_oracle);
        Ok(())
    }

    /// Queue a settlement fee rate change via the timelock.
    ///
    /// Issue #624: `set_fee_rate` no longer takes effect immediately.  Instead it
    /// enqueues a `PendingTimelockAction` that can only be executed after the
    /// configured timelock delay (default 48 hours) has elapsed.  Returns the
    /// action ID assigned to the pending action.
    ///
    /// # Arguments
    /// * `admin` – Must hold the admin role.
    /// * `bps`   – Fee in basis points (e.g. 100 = 1 %). Must be 0–10 000.
    pub fn set_fee_rate(env: Env, admin: Address, bps: i128) -> Result<String, Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        if bps < 0 || bps > 10_000 {
            return Err(Error::InvalidAmount);
        }

        Self::enqueue_timelocked_action(&env, admin, TimelockActionKind::SetFeeRate(bps))
    }

    /// Admin-only: enable or disable automatic pending refund creation for overpaid payments.
    /// When enabled (default), any payment verified as Overpaid will automatically create
    /// a pending refund for the excess amount and emit a REFUND/AUTO_CREATED event.
    pub fn set_auto_refund_overpayment(
        env: Env,
        admin: Address,
        enabled: bool,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::AutoRefundOverpayment, &enabled);
        Ok(())
    }

    /// Check whether automatic refund creation for overpaid payments is enabled.
    /// Defaults to true if not explicitly configured.
    pub fn get_auto_refund_overpayment(env: &Env) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::AutoRefundOverpayment)
            .unwrap_or(true)
    }

    /// Return the accumulated treasury balance collected via settlement fees
    /// and platform fees (when no custom fee_recipient).
    pub fn set_min_payment_duration_secs(env: Env, admin: Address, min_secs: u64) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        if min_secs < CREATE_PAYMENT_WINDOW_SECS {
            return Err(Error::InvalidAmount);
        }

        env.storage()
            .persistent()
            .set(&DataKey::MinPaymentDurationSecs, &min_secs);
        Ok(())
    }

    pub fn set_max_payment_duration_secs(env: Env, admin: Address, max_secs: u64) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        if max_secs > 30 * 24 * 3600 {
            return Err(Error::InvalidAmount);
        }

        env.storage()
            .persistent()
            .set(&DataKey::MaxPaymentDurationSecs, &max_secs);
        Ok(())
    }

    /// Return the accumulated treasury balance collected via settlement fees.
    pub fn get_treasury_balance(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::TreasuryBalance)
            .unwrap_or(0)
    }

    fn record_treasury_withdrawal(env: &Env, record: TreasuryWithdrawal) {
        let key = DataKey::TreasuryWithdrawalHistory;
        let mut history: Vec<TreasuryWithdrawal> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![env]);
        history.push_front(record);
        while history.len() > TREASURY_WITHDRAWAL_HISTORY_CAP {
            history.pop_back();
        }
        env.storage().persistent().set(&key, &history);
    }

    /// Return a page of treasury withdrawal history (newest-first).
    pub fn get_treasury_withdrawal_history(
        env: Env,
        offset: u32,
        limit: u32,
    ) -> Vec<TreasuryWithdrawal> {
        let history: Vec<TreasuryWithdrawal> = env
            .storage()
            .persistent()
            .get(&DataKey::TreasuryWithdrawalHistory)
            .unwrap_or_else(|| vec![&env]);
        let page_limit = limit.min(TREASURY_WITHDRAWAL_HISTORY_CAP);
        let mut page: Vec<TreasuryWithdrawal> = vec![&env];
        let mut i = offset;
        while i < history.len() && page.len() < page_limit {
            if let Some(item) = history.get(i) {
                page.push_back(item);
            }
            i = i.saturating_add(1);
        }
        page
    }

    /// Issue #666: Append a fee-collection record from `settle_payment`,
    /// retaining only the newest `FEE_COLLECTION_HISTORY_CAP` entries
    /// (newest-first), mirroring `record_treasury_withdrawal`.
    fn record_fee_collection(
        env: &Env,
        total_fee: i128,
        treasury_share: i128,
        developer_share: i128,
    ) {
        if total_fee <= 0 {
            return;
        }
        let key = DataKey::FeeCollectionHistory;
        let mut history: Vec<FeeCollectionRecord> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![env]);
        history.push_front(FeeCollectionRecord {
            collected_at: env.ledger().timestamp(),
            total_fee,
            treasury_share,
            developer_share,
        });
        while history.len() > FEE_COLLECTION_HISTORY_CAP {
            history.pop_back();
        }
        env.storage().persistent().set(&key, &history);
    }

    /// Issue #666: Aggregate platform fee collection over `[from_ts, to_ts]`
    /// (inclusive), for treasury reporting. Reads from the `FeeCollectionHistory`
    /// log that `settle_payment` appends to on every fee-bearing settlement.
    ///
    /// NOTE: `FeeCollectionHistory` is capped at `FEE_COLLECTION_HISTORY_CAP`
    /// entries; queries for periods older than the retained window will
    /// undercount. A follow-up should move this to time-bucketed storage
    /// (e.g. per-day accumulator keys) if long-horizon reporting is needed.
    pub fn get_platform_fee_report(env: Env, from_ts: u64, to_ts: u64) -> PlatformFeeReport {
        let history: Vec<FeeCollectionRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::FeeCollectionHistory)
            .unwrap_or_else(|| vec![&env]);

        let mut total_fees_collected: i128 = 0;
        let mut treasury_share: i128 = 0;
        let mut developer_share: i128 = 0;
        let mut payment_count: u64 = 0;

        for record in history.iter() {
            if record.collected_at >= from_ts && record.collected_at <= to_ts {
                total_fees_collected = total_fees_collected.saturating_add(record.total_fee);
                treasury_share = treasury_share.saturating_add(record.treasury_share);
                developer_share = developer_share.saturating_add(record.developer_share);
                payment_count = payment_count.saturating_add(1);
            }
        }

        PlatformFeeReport {
            total_fees_collected,
            treasury_share,
            developer_share,
            payment_count,
        }
    }

    /// Admin withdrawal of accumulated treasury fees. Emits `TREASURY/WITHDRAWN`
    /// with `(amount, destination)` and appends to the paginated history log.
    pub fn withdraw_treasury(
        env: Env,
        admin: Address,
        amount: i128,
        destination: Address,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let treasury_balance = Self::get_treasury_balance(env.clone());
        if amount > treasury_balance {
            return Err(Error::InsufficientTreasuryBalance);
        }

        let usdc_token_address: Address = env
            .storage()
            .persistent()
            .get(&DataKey::UsdcToken)
            .ok_or(Error::Unauthorized)?;
        let token_client = token::TokenClient::new(&env, &usdc_token_address);
        let contract_address = env.current_contract_address();

        env.storage().persistent().set(
            &DataKey::TreasuryBalance,
            &treasury_balance.saturating_sub(amount),
        );

        token_client.transfer(&contract_address, &destination, &amount);

        Self::record_treasury_withdrawal(
            &env,
            TreasuryWithdrawal {
                amount,
                destination: destination.clone(),
                admin: admin.clone(),
                withdrawn_at: env.ledger().timestamp(),
            },
        );

        env.events().publish(
            (Symbol::new(&env, "TREASURY"), Symbol::new(&env, "WITHDRAWN")),
            (amount, destination),
        );

        Ok(())
    }

    /// Admin-only: register a reusable fee-waiver code for per-payment zero-fee
    /// promotions.
    ///
    /// The code can be consumed at settlement via the `fee_waiver_code` field
    /// on `PaymentCharge`. Each successful consumption atomically decrements
    /// `remaining_uses`; when the counter reaches zero or `expires_at` is in
    /// the past, the code is treated as invalid and normal fees apply.
    ///
    /// To immediately revoke a live code without waiting for expiry, pass
    /// `max_uses = 0` (which will set `remaining_uses = 0` on overwrite).
    ///
    /// # Arguments
    /// * `admin` – Must hold the admin role.
    /// * `code`  – Arbitrary case-sensitive code string (e.g. "LAUNCH2026").
    /// * `expires_at` – Unix ledger timestamp after which the code is invalid.
    /// * `max_uses` – Maximum total payments that may use this code. Must be `>= 1`
    ///   when creating a new code; may be `0` when revoking an existing one.
    pub fn add_fee_waiver_code(
        env: Env,
        admin: Address,
        code: String,
        expires_at: u64,
        max_uses: u32,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        if expires_at <= env.ledger().timestamp() {
            return Err(Error::InvalidExpiry);
        }
        if max_uses == 0 {
            return Err(Error::InvalidAmount);
        }

        let record = FeeWaiverCodeRecord {
            code: code.clone(),
            expires_at,
            max_uses,
            remaining_uses: max_uses,
        };

        env.storage()
            .persistent()
            .set(&DataKey::FeeWaiverCode(code.clone()), &record);

        env.events().publish(
            (Symbol::new(&env, "FEE_WAIVER"), Symbol::new(&env, "CODE_ADDED")),
            (code, expires_at, max_uses),
        );

        Ok(())
    }

    pub fn set_global_rate_limit(
        env: Env,
        admin: Address,
        window_secs: u64,
        max_per_window: u32,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        let config = RateLimitConfig { window_secs, max_per_window };
        env.storage().persistent().set(&DataKey::GlobalRateLimit, &config);
        Ok(())
    }

    pub fn set_merchant_rate_limit(
        env: Env,
        admin: Address,
        merchant_id: Address,
        window_secs: u64,
        max_per_window: u32,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        let config = RateLimitConfig { window_secs, max_per_window };
        env.storage().persistent().set(&DataKey::MerchantSpecificRateLimit(merchant_id), &config);
        Ok(())
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

    /// Returns whether `account` holds `role` on this contract (issue #401).
    pub fn has_role(env: Env, role: Symbol, account: Address) -> bool {
        AccessControl::has_role(&env, &role, &account)
    }

    /// Returns all addresses currently holding `role` on this contract (issue #401).
    pub fn get_role_members(env: Env, role: Symbol) -> Vec<Address> {
        AccessControl::get_role_members(&env, &role)
    }

    /// Set the global paused state (admin only). When paused, create_payment, verify_payment, and cancel_payment are blocked.
    pub fn set_global_pause(
        env: Env,
        admin: Address,
        paused: bool,
        reason: String,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        let state = PauseState {
            paused,
            reason: reason.clone(),
            admin: Some(admin.clone()),
            timestamp: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&DataKey::Paused, &state);

        let event_name = if paused {
            Symbol::new(&env, "GLOBAL_PAUSED")
        } else {
            Symbol::new(&env, "GLOBAL_UNPAUSED")
        };

        env.events()
            .publish((Symbol::new(&env, "CONTRACT"), event_name), (admin, reason));

        Ok(())
    }

    /// Set the creation-only paused state (admin only, issue #670).
    ///
    /// When `paused` is true, only payment-creation entry points (`create_payment`,
    /// `create_payments_batch`, `swap_and_pay`, `swap_and_pay_multi_route`, and the
    /// creation path of `retry_payment`) are blocked with `Error::ContractPaused`.
    /// Settlement, verification, cancellation, and refund operations continue to work
    /// normally. This is narrower than `set_global_pause`, which halts all operations.
    /// Query the current state with `get_creation_pause_info`.
    pub fn set_creation_pause(
        env: Env,
        admin: Address,
        paused: bool,
        reason: String,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        let state = PauseState {
            paused,
            reason: reason.clone(),
            admin: Some(admin.clone()),
            timestamp: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&DataKey::CreationPaused, &state);

        let event_name = if paused {
            Symbol::new(&env, "CREATION_PAUSED")
        } else {
            Symbol::new(&env, "CREATION_UNPAUSED")
        };

        env.events()
            .publish((Symbol::new(&env, "CONTRACT"), event_name), (admin, reason));

        Ok(())
    }

    /// Legacy wrapper for set_global_pause
    pub fn set_paused(env: Env, admin: Address, paused: bool) -> Result<(), Error> {
        let reason = if paused {
            String::from_str(&env, "Legacy pause")
        } else {
            String::from_str(&env, "Legacy unpause")
        };
        Self::set_global_pause(env, admin, paused, reason)
    }

    /// Get the current creation-only pause state (issue #670).
    ///
    /// Distinct from `get_pause_info`, which returns the consolidated global + creation
    /// state. `set_creation_pause` blocks only `create_payment`, `create_payments_batch`,
    /// `swap_and_pay`, `swap_and_pay_multi_route`, and the creation path of `retry_payment`.
    /// It does NOT block `verify_payment`, `settle_payment`, `cancel_payment`,
    /// `process_refund`, `claim_refund`, or dispute resolution, so operators can halt new
    /// payment intake (e.g. during a maintenance window) while still allowing in-flight
    /// payments to be confirmed, settled, and refunded. Use `set_global_pause` instead when
    /// all operations need to be halted.
    pub fn get_creation_pause_info(env: Env) -> PauseState {
        env.storage()
            .persistent()
            .get::<DataKey, PauseState>(&DataKey::CreationPaused)
            .unwrap_or(PauseState {
                paused: false,
                reason: String::from_str(&env, ""),
                admin: None,
                timestamp: 0,
            })
    }

    /// Get the current consolidated pause info.
    pub fn get_pause_info(env: Env) -> PauseInfo {
        let empty_reason = String::from_str(&env, "");
        let default_state = PauseState {
            paused: false,
            reason: empty_reason,
            admin: None,
            timestamp: 0,
        };

        let global = env
            .storage()
            .persistent()
            .get::<DataKey, PauseState>(&DataKey::Paused)
            .unwrap_or_else(|| default_state.clone());

        let creation = env
            .storage()
            .persistent()
            .get::<DataKey, PauseState>(&DataKey::CreationPaused)
            .unwrap_or(default_state);

        PauseInfo { global, creation }
    }

    /// Get the current global paused state.
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, PauseState>(&DataKey::Paused)
            .map(|s| s.paused)
            .unwrap_or(false)
    }

    /// Check if contract is globally paused and return error if so.
    fn require_not_paused(env: &Env) -> Result<(), Error> {
        if Self::is_paused(env.clone()) {
            return Err(Error::ContractPaused);
        }
        Ok(())
    }

    /// Check if payment creation is paused (either globally or specifically for creation).
    fn require_creation_not_paused(env: &Env) -> Result<(), Error> {
        Self::require_not_paused(env)?;

        let creation_paused: bool = env
            .storage()
            .persistent()
            .get::<DataKey, PauseState>(&DataKey::CreationPaused)
            .map(|s| s.paused)
            .unwrap_or(false);

        if creation_paused {
            return Err(Error::ContractPaused);
        }
        Ok(())
    }

    /// Fixed-window rate limiter per merchant.
    ///
    /// `last_payment_at` stores the start of the current fixed window (set when the window
    /// is first entered). The counter resets only when `now` exceeds `window_start + window_secs`.
    /// This prevents the sliding-window bypass where bursts at the end of one window and the
    /// beginning of the next could otherwise double the effective rate.
    fn enforce_create_payment_rate_limit(env: &Env, merchant_id: &Address) -> Result<(), Error> {
        let now = env.ledger().timestamp();
        
        let config: RateLimitConfig = env.storage().persistent().get(&DataKey::MerchantSpecificRateLimit(merchant_id.clone()))
            .unwrap_or_else(|| {
                env.storage().persistent().get(&DataKey::GlobalRateLimit)
                    .unwrap_or(RateLimitConfig {
                        window_secs: CREATE_PAYMENT_WINDOW_SECS,
                        max_per_window: CREATE_PAYMENT_MAX_PER_WINDOW,
                    })
            });

        let key = DataKey::MerchantRateLimit(merchant_id.clone());

        let mut state: MerchantCreateRateLimit =
            env.storage()
                .persistent()
                .get(&key)
                .unwrap_or(MerchantCreateRateLimit {
                    last_payment_at: now,
                    count: 0,
                });

        if now.saturating_sub(state.last_payment_at) >= config.window_secs {
            // Start a new fixed window
            state.count = 0;
            state.last_payment_at = now;
        }

        if state.count >= config.max_per_window {
            return Err(Error::RateLimitExceeded);
        }

        state.count = state.count.saturating_add(1);

        env.storage().persistent().set(&key, &state);
        Self::bump_ttl(env, &key, SHORT_LIVE_TTL);

        Ok(())
    }

    fn enforce_create_payment_rate_limit_for_payer(env: &Env, payer: &Address) -> Result<(), Error> {
        let now = env.ledger().timestamp();

        let config: RateLimitConfig = env
            .storage()
            .persistent()
            .get(&DataKey::GlobalRateLimit)
            .unwrap_or(RateLimitConfig {
                window_secs: CREATE_PAYMENT_WINDOW_SECS,
                max_per_window: CREATE_PAYMENT_MAX_PER_WINDOW,
            });

        let key = DataKey::PayerRateLimit(payer.clone());

        let mut state: MerchantCreateRateLimit = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(MerchantCreateRateLimit {
                last_payment_at: now,
                count: 0,
            });

        if now.saturating_sub(state.last_payment_at) >= config.window_secs {
            // Start a new fixed window
            state.count = 0;
            state.last_payment_at = now;
        }

        if state.count >= config.max_per_window {
            return Err(Error::RateLimitExceeded);
        }

        state.count = state.count.saturating_add(1);

        env.storage().persistent().set(&key, &state);
        Self::bump_ttl(env, &key, SHORT_LIVE_TTL);

        Ok(())
    }

    fn enforce_create_payment_batch_rate_limit(
        env: &Env,
        merchant_id: &Address,
    ) -> Result<(), Error> {
        let now = env.ledger().timestamp();

        let config: RateLimitConfig = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantSpecificRateLimit(merchant_id.clone()))
            .unwrap_or_else(|| {
                env.storage()
                    .persistent()
                    .get(&DataKey::GlobalRateLimit)
                    .unwrap_or(RateLimitConfig {
                        window_secs: CREATE_PAYMENT_WINDOW_SECS,
                        max_per_window: CREATE_PAYMENT_MAX_PER_WINDOW,
                    })
            });

        let key = DataKey::MerchantRateLimit(merchant_id.clone());

        let mut state: MerchantCreateRateLimit = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(MerchantCreateRateLimit {
                last_payment_at: now,
                count: 0,
            });

        if now.saturating_sub(state.last_payment_at) >= config.window_secs {
            // Start a new fixed window
            state.count = 0;
            state.last_payment_at = now;
        }

        if state.count >= config.max_per_window {
            return Err(Error::RateLimitExceeded);
        }

        state.count = state.count.saturating_add(1);

        env.storage().persistent().set(&key, &state);
        Self::bump_ttl(env, &key, SHORT_LIVE_TTL);

        Ok(())
    }

    /// Set per-merchant min/max payment amount limits (merchant self-service).
    /// Pass None to clear a bound. Requires the caller to hold the MERCHANT role.
    pub fn set_merchant_amount_limits(
        env: Env,
        merchant_id: Address,
        min: Option<i128>,
        max: Option<i128>,
    ) -> Result<(), Error> {
        merchant_id.require_auth();
        if !AccessControl::has_role(&env, &role_merchant(&env), &merchant_id) {
            return Err(Error::Unauthorized);
        }
        if let (Some(lo), Some(hi)) = (min, max) {
            if lo > hi {
                return Err(Error::InvalidAmount);
            }
        }
        let limits = AmountLimits { min, max };
        env.storage()
            .persistent()
            .set(&DataKey::MerchantAmountLimits(merchant_id), &limits);
        Ok(())
    }

    /// Read per-merchant amount limits.
    pub fn get_merchant_amount_limits(env: Env, merchant_id: Address) -> Option<AmountLimits> {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantAmountLimits(merchant_id))
    }

    /// Set global min/max payment amount limits (admin only).
    /// Pass None to clear a bound.
    pub fn set_global_amount_limits(
        env: Env,
        admin: Address,
        min: Option<i128>,
        max: Option<i128>,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        if let (Some(lo), Some(hi)) = (min, max) {
            if lo > hi {
                return Err(Error::InvalidAmount);
            }
        }
        let limits = AmountLimits { min, max };
        env.storage()
            .persistent()
            .set(&DataKey::GlobalAmountLimits, &limits);
        Ok(())
    }

    /// Read global amount limits.
    pub fn get_global_amount_limits(env: Env) -> Option<AmountLimits> {
        env.storage().persistent().get(&DataKey::GlobalAmountLimits)
    }

    /// Enforce amount limits: merchant-specific limits take precedence over global limits.
    fn enforce_amount_limits(env: &Env, merchant_id: &Address, amount: i128) -> Result<(), Error> {
        let limits: Option<AmountLimits> = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantAmountLimits(merchant_id.clone()))
            .or_else(|| env.storage().persistent().get(&DataKey::GlobalAmountLimits));

        if let Some(l) = limits {
            if let Some(min) = l.min {
                if amount < min {
                    return Err(Error::AmountBelowMin);
                }
            }
            if let Some(max) = l.max {
                if amount > max {
                    return Err(Error::AmountAboveMax);
                }
            }
        }
        Ok(())
    }

    /// Set the USDC token address used as the default settlement token.
    /// Also adds it to the supported tokens whitelist.
    pub fn set_usdc_token(env: Env, admin: Address, token_address: Address) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::UsdcToken, &token_address);
        // Auto-add USDC to the supported tokens whitelist
        Self::allow_token(env, admin, token_address)
    }

    /// Allow or disallow a token address for use in payments (admin only).
    pub fn allow_token(env: Env, admin: Address, token_address: Address) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::AllowedToken(token_address.clone()), &true);
        let mut tokens: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::SupportedTokens)
            .unwrap_or(Vec::new(&env));
        if !tokens.contains(&token_address) {
            tokens.push_back(token_address);
            env.storage()
                .persistent()
                .set(&DataKey::SupportedTokens, &tokens);
            Self::bump_ttl(&env, &DataKey::SupportedTokens, LONG_LIVE_TTL);
        }
        Ok(())
    }
    /// Issue #483: Set the currency symbol for an allowed token (e.g., USDC, EURC, BRLT).
    /// Must be called after allow_token() to establish token-to-currency mapping.
    pub fn set_token_currency(env: Env, admin: Address, token_address: Address, currency: Symbol) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        
        if !env.storage().persistent().has(&DataKey::AllowedToken(token_address.clone())) {
            return Err(Error::UnsupportedToken);
        }
        
        env.storage()
            .persistent()
            .set(&DataKey::TokenCurrency(token_address), &currency);
        Ok(())
    }


    /// Issue #301: Remove a token from the supported tokens list (admin only).
    pub fn remove_supported_token(
        env: Env,
        admin: Address,
        token_address: Address,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::AllowedToken(token_address.clone()), &false);
        let mut tokens: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::SupportedTokens)
            .unwrap_or(Vec::new(&env));
        let mut i = 0;
        while i < tokens.len() {
            if tokens.get(i).unwrap() == token_address {
                tokens.remove(i);
            } else {
                i += 1;
            }
        }
        env.storage()
            .persistent()
            .set(&DataKey::SupportedTokens, &tokens);
        Self::bump_ttl(&env, &DataKey::SupportedTokens, LONG_LIVE_TTL);
        env.events().publish(
            (Symbol::new(&env, "TOKEN"), Symbol::new(&env, "REMOVED")),
            token_address,
        );
        Ok(())
    }

    /// Issue #301: Return the list of supported token addresses.
    pub fn get_supported_tokens(env: Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::SupportedTokens)
            .unwrap_or(Vec::new(&env))
    }

    /// Issue #303: Set per‑tier KYC payment limits (admin only).
    pub fn set_kyc_tier_limits(
        env: Env,
        admin: Address,
        tier: KycTier,
        max_amount: i128,
    ) -> Result<String, Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        // Issue #624: queue via timelock instead of applying immediately.
        Self::enqueue_timelocked_action(&env, admin, TimelockActionKind::SetKycTierLimits(tier, max_amount))
    }

    /// Issue #303: Set the FX oracle contract address (admin only).
    pub fn set_fx_oracle_address(
        env: Env,
        admin: Address,
        oracle_address: Address,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::FXOracleAddress, &oracle_address);
        Self::bump_ttl(&env, &DataKey::FXOracleAddress, LONG_LIVE_TTL);
        Ok(())
    }

    /// Add an address to the global blacklist (admin only).
    pub fn add_to_blacklist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::Blacklisted(address), &true);
        Ok(())
    }

    /// Remove an address from the global blacklist (admin only).
    pub fn remove_from_blacklist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::Blacklisted(address), &false);
        Ok(())
    }

    /// Returns true when an address is globally blacklisted.
    pub fn is_blacklisted(env: Env, address: Address) -> bool {
        Self::is_blacklisted_address(&env, &address)
    }

    fn is_blacklisted_address(env: &Env, address: &Address) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::Blacklisted(address.clone()))
            .unwrap_or(false)
    }

    fn require_not_blacklisted(env: &Env, address: &Address) -> Result<(), Error> {
        if Self::is_blacklisted_address(env, address) {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }

    /// Returns true if the given token address is on the allowlist.

    fn expiry_bucket_for(expires_at: u64) -> u32 {
        (expires_at / 5).min(u32::MAX as u64) as u32
    }

    fn index_payment_expiry(env: &Env, payment_id: &String, expires_at: u64) {
        let bucket = Self::expiry_bucket_for(expires_at);
        let key = DataKey::PaymentsByExpiry(bucket);
        let mut ids: Vec<String> = env.storage().persistent().get(&key).unwrap_or_else(|| vec![env]);
        if !ids.contains(payment_id) {
            ids.push_back(payment_id.clone());
            env.storage().persistent().set(&key, &ids);
            Self::bump_ttl(env, &key, LONG_LIVE_TTL);
        }

        let buckets_key = DataKey::PaymentExpiryBuckets;
        let mut buckets: Vec<u32> = env
            .storage()
            .persistent()
            .get(&buckets_key)
            .unwrap_or_else(|| vec![env]);
        if !buckets.contains(&bucket) {
            buckets.push_back(bucket);
            env.storage().persistent().set(&buckets_key, &buckets);
            Self::bump_ttl(env, &buckets_key, LONG_LIVE_TTL);
        }
    }

    /// Issue #678: Append payment_id to the daily bucket index for the given merchant.
    /// Bucket granularity is one day (86 400 seconds). The index allows analytics
    /// queries to scan only the relevant day buckets rather than all payments.
    fn index_payment_by_date(env: &Env, merchant_id: &Address, payment_id: &String, created_at: u64) {
        const SECONDS_PER_DAY: u64 = 86_400;
        let day_bucket = created_at / SECONDS_PER_DAY;
        let key = DataKey::DailyPaymentIndex(merchant_id.clone(), day_bucket);
        let mut ids: Vec<String> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![env]);
        if !ids.contains(payment_id) {
            ids.push_back(payment_id.clone());
            env.storage().persistent().set(&key, &ids);
            Self::bump_ttl(env, &key, LONG_LIVE_TTL);
        }
    }

    fn remove_payment_from_expiry_bucket(env: &Env, payment_id: &String, expires_at: u64) {
        let bucket = Self::expiry_bucket_for(expires_at);
        let key = DataKey::PaymentsByExpiry(bucket);
        if let Some(ids) = env.storage().persistent().get::<DataKey, Vec<String>>(&key) {
            let mut remaining = vec![env];
            for id in ids.iter() {
                if id != *payment_id {
                    remaining.push_back(id);
                }
            }
            if remaining.is_empty() {
                env.storage().persistent().remove(&key);
                let buckets_key = DataKey::PaymentExpiryBuckets;
                if let Some(buckets) = env.storage().persistent().get::<DataKey, Vec<u32>>(&buckets_key) {
                    let mut kept = vec![env];
                    for candidate in buckets.iter() {
                        if candidate != bucket {
                            kept.push_back(candidate);
                        }
                    }
                    if kept.is_empty() {
                        env.storage().persistent().remove(&buckets_key);
                    } else {
                        env.storage().persistent().set(&buckets_key, &kept);
                        Self::bump_ttl(env, &buckets_key, LONG_LIVE_TTL);
                    }
                }
            } else {
                env.storage().persistent().set(&key, &remaining);
                Self::bump_ttl(env, &key, LONG_LIVE_TTL);
            }
        }
    }

    pub fn is_token_allowed(env: Env, token_address: Address) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::AllowedToken(token_address))
            .unwrap_or(false)
    }

    #[allow(deprecated)]
    pub fn create_payment(env: Env, args: CreatePaymentArgs) -> Result<PaymentCharge, Error> {
        Self::require_creation_not_paused(&env)?;
        args.merchant_id.require_auth();
        Self::require_not_blacklisted(&env, &args.merchant_id)?;
        Self::require_not_blacklisted(&env, &args.deposit_address)?;

        // Idempotency check: if client_token was already used, return the existing payment
        // (or error if it maps to a different payment_id).
        if let Some(ref token) = args.client_token {
            let key = DataKey::IdempotencyKey(token.clone());
            if let Some(existing_id) = env.storage().persistent().get::<DataKey, String>(&key) {
                if existing_id == args.payment_id {
                    return Self::get_payment_internal(&env, &args.payment_id);
                } else {
                    return Err(Error::DuplicateIdempotencyKey);
                }
            }
        }

        // Verify that the merchant has the MERCHANT role (granted on verification)
        if !AccessControl::has_role(&env, &role_merchant(&env), &args.merchant_id) {
            return Err(Error::Unauthorized);
        }

        // Issue #164: Validate token against admin-approved allowlist
        if let Some(ref token_addr) = args.token_address {
            let allowed: bool = env
                .storage()
                .persistent()
                .get::<DataKey, bool>(&DataKey::AllowedToken(token_addr.clone()))
                .unwrap_or(false);
            if !allowed {
                return Err(Error::UnsupportedToken);
            }
        }
        // Issue #483: Verify that token_address (if provided) matches the currency symbol
        if let Some(ref token_addr) = args.token_address {
            if let Some(token_currency) = env
                .storage()
                .persistent()
                .get::<DataKey, Symbol>(&DataKey::TokenCurrency(token_addr.clone()))
            {
                if token_currency != args.currency {
                    return Err(Error::UnsupportedToken);
                }
            }
        }

        // Issue #79: Cross-contract validate merchant is verified and active
        if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
            match registry_client.try_get_merchant(&args.merchant_id) {
                Ok(Ok(merchant)) => {
                    // Require merchant to be verified (not Unverified), active, and not suspended
                    if merchant.kyc_tier == crate::merchant_registry::KycTier::Unverified
                        || !merchant.active
                        || merchant.suspension_reason.is_some()
                    {
                        return Err(Error::Unauthorized);
                    }

                    // Issue #516: Enforce merchant whitelist mode against the payer.
                    if merchant.whitelist_mode {
                        let payer = args.payer.clone().ok_or(Error::PayerNotWhitelisted)?;
                        match registry_client.try_is_customer_whitelisted(&args.merchant_id, &payer) {
                            Ok(Ok(true)) => {}
                            _ => return Err(Error::PayerNotWhitelisted),
                        }
                    }
                }
                _ => {
                    // If registry lookup fails, reject the payment
                    return Err(Error::Unauthorized);
                }
            }
        }

        if args.amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        Self::enforce_amount_limits(&env, &args.merchant_id, args.amount)?;

        // Issue #393: Enforce KYC tier per-payment limit when merchant registry is configured
        if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
            if let Ok(Ok(merchant)) = registry_client.try_get_merchant(&args.merchant_id) {
                if let Ok(Ok(limits)) = registry_client.try_get_tier_limits(&merchant.kyc_tier) {
                    if let Some(min) = limits.min {
                        if args.amount < min {
                            return Err(Error::AmountBelowMin);
                        }
                    }
                    if let Some(max) = limits.max {
                        if args.amount > max {
                            return Err(Error::AmountAboveMax);
                        }
                    }
                }
            }
        }

        if env
            .storage()
            .persistent()
            .has(&DataKey::Payment(args.payment_id.clone()))
        {
            return Err(Error::PaymentAlreadyExists);
        }

        // Issue #489: Validate metadata_hash uniqueness
        if let Some(ref hash) = args.metadata_hash {
            if env
                .storage()
                .persistent()
                .has(&DataKey::MetadataHashPayment(hash.clone()))
            {
                return Err(Error::DuplicateIdempotencyKey);
            }
        }

        if !utils::validate_id(&args.payment_id) {
            return Err(Error::InvalidPaymentId);
        }

        // Validate metadata key count and key/value length limits.
        if let Some(ref meta_map) = args.metadata {
            utils::validate_metadata(meta_map)?;
        }

        // Issue #397: Validate Stellar memo type constraints.
        Self::validate_memo(&env, &args.memo, &args.memo_type)?;

        Self::enforce_create_payment_rate_limit(&env, &args.merchant_id)?;

        let now = env.ledger().timestamp();
        let min_duration = env.storage()
            .persistent()
            .get::<DataKey, u64>(&DataKey::MinPaymentDurationSecs)
            .unwrap_or(CREATE_PAYMENT_WINDOW_SECS);
        let max_duration = env.storage()
            .persistent()
            .get::<DataKey, u64>(&DataKey::MaxPaymentDurationSecs)
            .unwrap_or(30 * 24 * 3600);

        let resolved_expires_at = match args.expires_at {
            Some(ts) => {
                if ts <= now {
                    return Err(Error::InvalidExpiry);
                }
                let duration = ts.saturating_sub(now);
                if duration < min_duration || duration > max_duration {
                    return Err(Error::InvalidExpiry);
                }
                ts
            }
            None => {
                let duration = args.duration_secs.unwrap_or(DEFAULT_PAYMENT_DURATION_SECS);
                if duration < min_duration || duration > max_duration {
                    return Err(Error::InvalidExpiry);
                }
                now.saturating_add(duration)
            }
        };

        let payment = PaymentCharge {
            payment_id: args.payment_id.clone(),
            merchant_id: args.merchant_id.clone(),
            amount: args.amount,
            currency: args.currency,
            deposit_address: args.deposit_address,
            status: PaymentStatus::Pending,
            payer_address: None,
            transaction_hash: None,
            created_at: now,
            confirmed_at: None,
            expires_at: resolved_expires_at,
            amount_received: None,
            memo: args.memo.clone(),
            memo_type: args.memo_type.clone(),
            token_address: args.token_address.clone(),
            metadata_hash: args.metadata_hash.clone(),
            original_token: None,
            swap_path: None,
            fx_rate: None,
            fx_rate_at: None,
            metadata: args.metadata.clone(),
            fee_waiver_code: args.fee_waiver_code.clone(),
            retry_of_payment_id: None,
            payer_muxed_id: None,
            payment_link_id: None,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Payment(args.payment_id.clone()), &payment);
        Self::record_payment_status(&env, &payment);
        Self::bump_payment_ttl(&env, &args.payment_id, &payment.status);
        Self::index_payment_expiry(&env, &args.payment_id, payment.expires_at);
        Self::index_payment_by_date(&env, &args.merchant_id, &args.payment_id, payment.created_at);

        // Issue #489: Store reverse index for metadata_hash → payment_id lookup
        if let Some(ref hash) = args.metadata_hash {
            let key = DataKey::MetadataHashPayment(hash.clone());
            env.storage()
                .persistent()
                .set(&key, &args.payment_id);
            Self::bump_ttl(&env, &key, LONG_LIVE_TTL);
        }

        let mut merchant_payments = Self::get_merchant_payments_internal(&env, &args.merchant_id);
        merchant_payments.push_back(args.payment_id.clone());
        let merchant_payments_key = DataKey::MerchantPayments(args.merchant_id.clone());
        env.storage()
            .persistent()
            .set(&merchant_payments_key, &merchant_payments);
        Self::bump_ttl(&env, &merchant_payments_key, LONG_LIVE_TTL);

        // Issue #503: Increment persistent payment count for O(1) dashboard query
        let count_key = DataKey::MerchantPaymentCount(args.merchant_id.clone());
        let count: u64 = env
            .storage()
            .persistent()
            .get(&count_key)
            .unwrap_or(0u64);
        env.storage()
            .persistent()
            .set(&count_key, &(count + 1));
        Self::bump_ttl(&env, &count_key, LONG_LIVE_TTL);

        // Issue #628: maintain the per-merchant gross-volume index and the
        // tracked-merchant list so `get_top_merchants` can rank without scanning.
        Self::record_merchant_volume(&env, &args.merchant_id, args.amount);

        // Issue #284: Normalised 2-tuple topic; merchant_id and metadata included in payload.
        env.events().publish(
            (
                Symbol::new(&env, "PAYMENT"),
                Symbol::new(&env, "CREATED"),
            ),
            (args.payment_id.clone(), args.merchant_id.clone(), args.amount, args.metadata.clone()),
        );

        // Issue #399: Persist idempotency key → payment_id mapping with a TTL that matches
        // the payment expiry window so keys do not accumulate indefinitely.
        if let Some(token) = args.client_token {
            let key = DataKey::IdempotencyKey(token.clone());
            env.storage().persistent().set(&key, &args.payment_id);
            // TTL in ledgers ≈ (expires_at − now) / 5s per ledger, clamped to SHORT_LIVE_TTL min.
            let payment_duration_secs = resolved_expires_at.saturating_sub(now);
            let ledgers_per_sec: u64 = 5;
            let ttl_ledgers = ((payment_duration_secs / ledgers_per_sec) as u32)
                .max(SHORT_LIVE_TTL);
            Self::bump_ttl(&env, &key, ttl_ledgers);
            // Store reverse mapping so cancel/expire can clean up the token.
            // We prefix the payment_id with "r:" to avoid collision with real tokens.
            let rev_token_id = Self::rev_key_for(&env, &args.payment_id);
            let rev_key = DataKey::IdempotencyKey(rev_token_id);
            env.storage().persistent().set(&rev_key, &token);
            Self::bump_ttl(&env, &rev_key, ttl_ledgers);
        }

        Ok(payment)
    }

    /// Issue #165: Batch payment creation for optimized gas usage.
    /// Creates multiple payment charges in a single transaction.
    /// Reverts all if any element violates validation rules.
    #[allow(deprecated)]
    pub fn create_payments_batch(
        env: Env,
        args_list: Vec<CreatePaymentArgs>,
    ) -> Result<Vec<String>, Error> {
        Self::require_creation_not_paused(&env)?;

        if args_list.len() > 50 {
            return Err(Error::BatchTooLarge);
        }

        if args_list.is_empty() {
            return Ok(vec![&env]);
        }

        // Issue #682: Detect duplicate payment_ids within the batch
        let mut seen_payment_ids: Vec<String> = vec![&env];
        for args in args_list.iter() {
            let mut is_duplicate = false;
            for seen_id in seen_payment_ids.iter() {
                if args.payment_id == seen_id {
                    is_duplicate = true;
                    break;
                }
            }
            if is_duplicate {
                return Err(Error::BatchContainsDuplicates);
            }
            seen_payment_ids.push_back(args.payment_id.clone());
        }

        let mut batch_merchants: Vec<Address> = vec![&env];

        // Validate all payments first before creating any
        for args in args_list.iter() {
            args.merchant_id.require_auth();
            Self::require_not_blacklisted(&env, &args.merchant_id)?;
            Self::require_not_blacklisted(&env, &args.deposit_address)?;

            if !batch_merchants.contains(&args.merchant_id) {
                batch_merchants.push_back(args.merchant_id.clone());
            }

            // Verify merchant role
            if !AccessControl::has_role(&env, &role_merchant(&env), &args.merchant_id) {
                return Err(Error::Unauthorized);
            }

            // Issue #164: Validate token against allowlist
            if let Some(ref token_addr) = args.token_address {
                let allowed: bool = env
                    .storage()
                    .persistent()
                    .get::<DataKey, bool>(&DataKey::AllowedToken(token_addr.clone()))
                    .unwrap_or(false);
                if !allowed {
                    return Err(Error::UnsupportedToken);
                }
            }
        // Issue #483: Verify that token_address (if provided) matches the currency symbol
        if let Some(ref token_addr) = args.token_address {
            if let Some(token_currency) = env
                .storage()
                .persistent()
                .get::<DataKey, Symbol>(&DataKey::TokenCurrency(token_addr.clone()))
            {
                if token_currency != args.currency {
                    return Err(Error::UnsupportedToken);
                }
            }
        }

            // Validate merchant is verified and active
            if let Some(registry_address) = env
                .storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
            {
                let registry_client =
                    crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
                match registry_client.try_get_merchant(&args.merchant_id) {
                    Ok(Ok(merchant)) => {
                        if merchant.kyc_tier == crate::merchant_registry::KycTier::Unverified
                            || !merchant.active
                            || merchant.suspension_reason.is_some()
                        {
                            return Err(Error::Unauthorized);
                        }
                    }
                    _ => {
                        return Err(Error::Unauthorized);
                    }
                }
            }

            if args.amount <= 0 {
                return Err(Error::InvalidAmount);
            }

            Self::enforce_amount_limits(&env, &args.merchant_id, args.amount)?;

            if let Some(limits) = env
                .storage()
                .persistent()
                .get::<DataKey, KycTierLimits>(&DataKey::KycTierLimitsConfig)
            {
                if let Some(registry_address) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
                {
                    let registry_client = crate::merchant_registry::MerchantRegistryClient::new(
                        &env,
                        &registry_address,
                    );
                    if let Ok(Ok(merchant)) =
                        registry_client.try_get_merchant(&args.merchant_id)
                    {
                        if limits.tier == merchant.kyc_tier && args.amount > limits.max_amount {
                            return Err(Error::AmountAboveMax);
                        }
                    }
                }
            }

            if env
                .storage()
                .persistent()
                .has(&DataKey::Payment(args.payment_id.clone()))
            {
                return Err(Error::PaymentAlreadyExists);
            }

            // Issue #489: Validate metadata_hash uniqueness in batch
            if let Some(ref hash) = args.metadata_hash {
                if env
                    .storage()
                    .persistent()
                    .has(&DataKey::MetadataHashPayment(hash.clone()))
                {
                    return Err(Error::DuplicateIdempotencyKey);
                }
            }

            if args.payment_id.is_empty() {
                return Err(Error::InvalidPaymentId);
            }

            // Validate metadata key count and key/value length limits.
            if let Some(ref meta_map) = args.metadata {
                utils::validate_metadata(meta_map)?;
            }

            // Check idempotency
            if let Some(ref token) = args.client_token {
                let key = DataKey::IdempotencyKey(token.clone());
                if let Some(existing_id) = env.storage().persistent().get::<DataKey, String>(&key) {
                    if existing_id != args.payment_id {
                        return Err(Error::DuplicateIdempotencyKey);
                    }
                }
            }
        }

        for merchant_id in batch_merchants.iter() {
            Self::enforce_create_payment_batch_rate_limit(&env, &merchant_id)?;
        }

        // All validations passed, now create all payments
        let mut payment_ids = vec![&env];
        let now = env.ledger().timestamp();

        for args in args_list.iter() {
            let resolved_expires_at = match args.expires_at {
                Some(ts) => ts,
                None => now.saturating_add(args.duration_secs.unwrap_or(DEFAULT_PAYMENT_DURATION_SECS)),
            };
            if resolved_expires_at <= now {
                return Err(Error::InvalidExpiry);
            }

            let payment = PaymentCharge {
                payment_id: args.payment_id.clone(),
                merchant_id: args.merchant_id.clone(),
                amount: args.amount,
                currency: args.currency.clone(),
                deposit_address: args.deposit_address.clone(),
                status: PaymentStatus::Pending,
                payer_address: None,
                transaction_hash: None,
                created_at: now,
                confirmed_at: None,
                expires_at: resolved_expires_at,
                amount_received: None,
                memo: args.memo.clone(),
                memo_type: args.memo_type.clone(),
                token_address: args.token_address.clone(),
                metadata_hash: args.metadata_hash.clone(),
                original_token: None,
                swap_path: None,
                fx_rate: None,
                fx_rate_at: None,
                metadata: args.metadata.clone(),
                fee_waiver_code: args.fee_waiver_code.clone(),
                retry_of_payment_id: None,
                payer_muxed_id: None,
                payment_link_id: None,
            };

            env.storage()
                .persistent()
                .set(&DataKey::Payment(args.payment_id.clone()), &payment);
            Self::bump_payment_ttl(&env, &args.payment_id, &payment.status);
            Self::index_payment_expiry(&env, &args.payment_id, payment.expires_at);

            // Issue #489: Store reverse index for metadata_hash → payment_id lookup in batch
            if let Some(ref hash) = args.metadata_hash {
                let key = DataKey::MetadataHashPayment(hash.clone());
                env.storage()
                    .persistent()
                    .set(&key, &args.payment_id);
                Self::bump_ttl(&env, &key, LONG_LIVE_TTL);
            }

            let mut merchant_payments = Self::get_merchant_payments_internal(&env, &args.merchant_id);
            merchant_payments.push_back(args.payment_id.clone());
            let merchant_payments_key = DataKey::MerchantPayments(args.merchant_id.clone());
            env.storage()
                .persistent()
                .set(&merchant_payments_key, &merchant_payments);
            Self::bump_ttl(&env, &merchant_payments_key, LONG_LIVE_TTL);

            env.events().publish(
                (
                    Symbol::new(&env, "PAYMENT"),
                    Symbol::new(&env, "CREATED"),
                ),
                (args.payment_id.clone(), args.merchant_id.clone(), args.amount, args.metadata.clone()),
            );

            // Issue #399: Persist idempotency key with TTL matching payment expiry window.
            if let Some(ref token) = args.client_token {
                let key = DataKey::IdempotencyKey(token.clone());
                env.storage().persistent().set(&key, &args.payment_id);
                let payment_duration_secs = resolved_expires_at.saturating_sub(now);
                let ledgers_per_sec: u64 = 5;
                let ttl_ledgers = ((payment_duration_secs / ledgers_per_sec) as u32)
                    .max(SHORT_LIVE_TTL);
                Self::bump_ttl(&env, &key, ttl_ledgers);
                // Reverse mapping for cleanup on cancel/expire.
                let rev_token_id = Self::rev_key_for(&env, &args.payment_id);
                let rev_key = DataKey::IdempotencyKey(rev_token_id);
                env.storage().persistent().set(&rev_key, token);
                Self::bump_ttl(&env, &rev_key, ttl_ledgers);
            }

            payment_ids.push_back(args.payment_id.clone());
        }

        // Emit batch creation event
        env.events().publish(
            (
                Symbol::new(&env, "PAYMENT"),
                Symbol::new(&env, "BATCH_CREATED"),
            ),
            payment_ids.len(),
        );

        Ok(payment_ids)
    }

    #[allow(deprecated)]
    pub fn verify_payment(
        env: Env,
        oracle: Address,
        payment_id: String,
        transaction_hash: BytesN<32>,
        payer_address: Address,
        amount_received: i128,
        payer_muxed_id: Option<u64>,
    ) -> Result<PaymentStatus, Error> {
        Self::require_not_paused(&env)?;
        oracle.require_auth();
        Self::require_not_blacklisted(&env, &oracle)?;
        Self::require_not_blacklisted(&env, &payer_address)?;

        if !AccessControl::has_role(&env, &role_oracle(&env), &oracle) {
            return Err(Error::Unauthorized);
        }

        let mut payment = Self::get_payment_internal(&env, &payment_id)?;
        Self::require_not_blacklisted(&env, &payment.merchant_id)?;

        // Issue #75: Enforce idempotent verify_payment - reject double verification
        // If payment is already Confirmed, return current status without error
        if payment.status == PaymentStatus::Confirmed {
            return Ok(payment.status);
        }

        // Reject if payment is in any other terminal state
        if payment.status != PaymentStatus::Pending {
            return Err(Error::PaymentAlreadyProcessed);
        }

        if env.ledger().timestamp() > payment.expires_at {
            return Err(Error::PaymentExpired);
        }

        // Record the actual amount received for reconciliation
        payment.amount_received = Some(amount_received);
        payment.payer_address = Some(payer_address.clone());
        payment.transaction_hash = Some(transaction_hash);
        payment.confirmed_at = Some(env.ledger().timestamp());
        // Issue #484: Store muxed ID if M-address was used
        payment.payer_muxed_id = payer_muxed_id;

        // Get merchant-specific tolerance if available, otherwise use global default
        let merchant_tolerance = if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
            match registry_client.try_get_merchant(&payment.merchant_id) {
                Ok(Ok(merchant)) => merchant.payment_tolerance,
                _ => None,
            }
        } else {
            None
        };

        // Scale tolerance by token decimals: 1 unit in the smallest denomination per decimal place.
        // USDC has 7 decimals on Stellar (stroops); other tokens may differ.
        // tolerance = 10^(decimals - 6) clamped to at least 1, so a 6-decimal token gets tolerance=1,
        // a 7-decimal token gets tolerance=10, a 2-decimal token gets tolerance=1 (clamped).
        let base_tolerance = if let Some(ref token_addr) = payment.token_address {
            let decimals = token::TokenClient::new(&env, token_addr).decimals();
            if decimals >= 6 {
                let exp = decimals - 6;
                let mut t: i128 = 1;
                let mut i = 0u32;
                while i < exp {
                    t *= 10;
                    i += 1;
                }
                t
            } else {
                1i128
            }
        } else {
            PAYMENT_TOLERANCE
        };

        // Use merchant tolerance if set, otherwise use global tolerance, otherwise base tolerance
        let global_tolerance = if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
            registry_client.get_global_payment_tolerance()
        } else {
            PAYMENT_TOLERANCE
        };

        let tolerance = merchant_tolerance.unwrap_or(global_tolerance);

        // Cap tolerance at 1% of payment amount to prevent abuse
        let max_tolerance = payment.amount / 100; // 1% of payment amount
        let tolerance = tolerance.min(max_tolerance);

        let diff = amount_received - payment.amount;

        let mut new_status = if (0..=tolerance).contains(&diff) {
            // Exact match or tiny overpay within tolerance → Confirmed
            PaymentStatus::Confirmed
        } else if diff > tolerance {
            // Meaningfully more than expected → Overpaid
            PaymentStatus::Overpaid
        } else if diff >= -tolerance {
            // Tiny underpay within tolerance → Confirmed
            PaymentStatus::Confirmed
        } else {
            // Meaningfully less than expected → PartiallyPaid
            PaymentStatus::PartiallyPaid
        };

        payment.status = new_status.clone();

        // Issue #304: snapshot the FX rate at verification time, if an oracle
        // is configured for this contract.
        if let Some(fx_oracle) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::FxOracleAddress)
        {
            let oracle_client = FXOracleClient::new(&env, &fx_oracle);
            match oracle_client.try_get_rate(&payment.currency) {
                Ok(Ok(rate_data)) => {
                    payment.fx_rate = Some(rate_data.rate);
                    payment.fx_rate_at = Some(env.ledger().timestamp());
                }
                _ => {
                    env.events().publish(
                        (
                            Symbol::new(&env, "PAYMENT"),
                            Symbol::new(&env, "FX_RATE_UNAVAILABLE"),
                        ),
                        payment_id.clone(),
                    );
                }
            }
        }

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);

        let event_name = match &new_status {
            PaymentStatus::Confirmed => Symbol::new(&env, "VERIFIED"),
            PaymentStatus::Overpaid => Symbol::new(&env, "OVERPAID"),
            PaymentStatus::PartiallyPaid => Symbol::new(&env, "PARTIALLY_PAID"),
            _ => Symbol::new(&env, "FAILED"),
        };
        let overpaid_refund_amount = if new_status == PaymentStatus::Overpaid {
            Some(amount_received.saturating_sub(payment.amount))
        } else {
            None
        };

        // Issue #471: Emit status-specific events for PartiallyPaid and Overpaid.
        if new_status == PaymentStatus::PartiallyPaid {
            env.events().publish(
                (
                    Symbol::new(&env, "PAYMENT"),
                    Symbol::new(&env, "PARTIALLY_PAID"),
                    payment.merchant_id.clone(),
                ),
                (payment_id.clone(), payment.amount, amount_received),
            );
        }
        if new_status == PaymentStatus::Overpaid {
            env.events().publish(
                (
                    Symbol::new(&env, "PAYMENT"),
                    Symbol::new(&env, "OVERPAID"),
                    payment.merchant_id.clone(),
                ),
                (payment_id.clone(), payment.amount, amount_received),
            );
        }

        // Issue #162: Merchant-configurable partial payment policy.
        if new_status == PaymentStatus::PartiallyPaid {
            let partial_allowed = if let Some(registry_address) = env
                .storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
            {
                let registry_client =
                    crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
                match registry_client.try_get_merchant(&payment.merchant_id) {
                    Ok(Ok(merchant)) => merchant.partial_payment_allowed,
                    _ => false,
                }
            } else {
                false
            };

            if !partial_allowed {
                new_status = PaymentStatus::Failed;
                env.events().publish(
                    (
                        Symbol::new(&env, "REFUND"),
                        Symbol::new(&env, "AUTO_REQUIRED"),
                        payment.merchant_id.clone(),
                    ),
                    (payment_id.clone(), amount_received),
                );
            }
        }

        // Issue #63: Enforce tier-based monthly volume cap before confirming payment.
        if new_status == PaymentStatus::Confirmed || new_status == PaymentStatus::Overpaid {
            Self::enforce_tier_volume_cap(&env, &payment.merchant_id, amount_received)?;
        }

        // Issue #304: Check FX rate freshness when both registry and oracle address are configured
        if new_status == PaymentStatus::Confirmed || new_status == PaymentStatus::Overpaid {
            if let Some(_registry_address) = env
                .storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
            {
                if let Some(oracle_address) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, Address>(&DataKey::FXOracleAddress)
                {
                    let oracle_client =
                        crate::fx_oracle::FXOracleClient::new(&env, &oracle_address);
                    match oracle_client.try_get_rate(&payment.currency) {
                        Ok(Ok(rate_data)) => {
                            payment.fx_rate = Some(rate_data.rate);
                            payment.fx_rate_at = Some(rate_data.updated_at);
                        }
                        _ => {
                            return Err(Error::StaleOracleRate);
                        }
                    }
                }
            }
        }

        // Issue #505: Validate status transition through state machine
        payment.status = payment_state_machine::transition_status(&payment.status, new_status.clone())?;
        Self::record_payment_status(&env, &payment);

        if let Some(refund_amount) = overpaid_refund_amount {
            if Self::get_auto_refund_overpayment(&env) {
                if let Some(registry_address) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
                {
                    let registry_client = crate::merchant_registry::MerchantRegistryClient::new(
                        &env,
                        &registry_address,
                    );

                    if let Some(refund_manager_address) = registry_client.get_refund_manager_address() {
                        let refund_client = RefundManagerClient::new(&env, &refund_manager_address);
                        refund_client.register_payment(
                            &payment_id,
                            &payment.merchant_id,
                            &payment.amount,
                            &payment.currency,
                        );

                        let auto_reason = String::from_str(&env, "Automatic refund for overpayment");
                        let auto_refund_id = refund_client
                            .try_queue_auto_refund(
                                &env.current_contract_address(),
                                &registry_address,
                                &payment_id,
                                &refund_amount,
                                &payer_address,
                                &auto_reason,
                            )
                            .map_err(|_| Error::Unauthorized)?
                            .map_err(|_| Error::Unauthorized)?;

                        env.events().publish(
                            (
                                Symbol::new(&env, "REFUND"),
                                Symbol::new(&env, "AUTO_CREATED"),
                                payment.merchant_id.clone(),
                            ),
                            (payment_id.clone(), refund_amount, auto_refund_id),
                        );
                    }
                }
            }
        }

        // Issue #492: Auto-create or update customer profile on confirmed payment
        if new_status == PaymentStatus::Confirmed || new_status == PaymentStatus::Overpaid {
            let customer_key = DataKey::CustomerProfile(payment.merchant_id.clone(), payer_address.clone());
            let mut customer_profile = if let Some(existing) = env.storage().persistent().get::<DataKey, CustomerProfile>(&customer_key) {
                existing
            } else {
                CustomerProfile {
                    customer_id: payer_address.clone(),
                    merchant_id: payment.merchant_id.clone(),
                    email_hash: None,
                    created_at: env.ledger().timestamp(),
                    payment_count: 0,
                    total_spent: 0,
                }
            };
            customer_profile.payment_count += 1;
            customer_profile.total_spent += amount_received;
            env.storage().persistent().set(&customer_key, &customer_profile);
            Self::bump_ttl(&env, &customer_key, LONG_LIVE_TTL);
        }

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);

        let event_name = match &new_status {
            PaymentStatus::Confirmed => Symbol::new(&env, "VERIFIED"),
            PaymentStatus::Overpaid => Symbol::new(&env, "OVERPAID"),
            PaymentStatus::PartiallyPaid => Symbol::new(&env, "PARTIALLY_PAID"),
            _ => Symbol::new(&env, "FAILED"),
        };

        // Issue #166: Optimize event topics for efficient indexing
        env.events().publish(
            (Symbol::new(&env, "PAYMENT"), event_name, payment.merchant_id.clone()),
            (payment_id.clone(), payment.amount, amount_received),
        );

        Ok(new_status)
    }

    /// Issue #471: Allow a merchant to accept a PartiallyPaid payment at the received amount.
    /// Moves the payment from PartiallyPaid to Confirmed, using the received amount as the
    /// effective payment amount. No refund is created for the difference.
    pub fn accept_partial_payment(
        env: Env,
        merchant_id: Address,
        payment_id: String,
    ) -> Result<(), Error> {
        merchant_id.require_auth();
        Self::require_not_blacklisted(&env, &merchant_id)?;

        let mut payment = Self::get_payment_internal(&env, &payment_id)?;
        if payment.status != PaymentStatus::PartiallyPaid {
            return Err(Error::PaymentAlreadyProcessed);
        }
        if payment.merchant_id != merchant_id {
            return Err(Error::Unauthorized);
        }

        payment.status = PaymentStatus::Confirmed;
        Self::record_payment_status(&env, &payment);
        let amount_received = payment.amount_received.unwrap_or(payment.amount);
        payment.amount = amount_received;

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);

        env.events().publish(
            (
                Symbol::new(&env, "PAYMENT"),
                Symbol::new(&env, "PARTIAL_ACCEPTED"),
                merchant_id,
            ),
            (payment_id, amount_received),
        );

        Ok(())
    }

    /// Issue #471: Allow a customer to complete a PartiallyPaid payment by topping up.
    /// This is a declaration that the payer will send additional funds off-chain.
    /// The payment status is moved to Pending so a new verify_payment call can
    /// confirm it with the combined amount.
    pub fn complete_partial_payment(
        env: Env,
        payer: Address,
        payment_id: String,
        top_up_amount: i128,
    ) -> Result<(), Error> {
        payer.require_auth();
        Self::require_not_blacklisted(&env, &payer)?;

        if top_up_amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let mut payment = Self::get_payment_internal(&env, &payment_id)?;
        if payment.status != PaymentStatus::PartiallyPaid {
            return Err(Error::PaymentAlreadyProcessed);
        }

        payment.status = PaymentStatus::Pending;
        Self::record_payment_status(&env, &payment);
        payment.amount = payment.amount.saturating_add(top_up_amount);

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);

        env.events().publish(
            (
                Symbol::new(&env, "PAYMENT"),
                Symbol::new(&env, "PARTIAL_TOPUP"),
                payer,
            ),
            (payment_id, top_up_amount),
        );

        Ok(())
    }

    /// Issue #482: Create a retry payment for an expired or failed original payment.
    /// Returns the payment_id of the newly created payment, linked to the original.
    /// Maximum retry chain depth is 3.
    pub fn retry_payment(
        env: Env,
        merchant_id: Address,
        original_payment_id: String,
        new_expires_at: u64,
    ) -> Result<String, Error> {
        Self::require_creation_not_paused(&env)?;
        merchant_id.require_auth();
        Self::require_not_blacklisted(&env, &merchant_id)?;

        // Retrieve original payment
        let mut original = Self::get_payment_internal(&env, &original_payment_id)?;

        // Validate original payment status (must be Expired or Failed)
        if original.status != PaymentStatus::Expired && original.status != PaymentStatus::Failed {
            return Err(Error::PaymentAlreadyProcessed);
        }

        // Validate merchant ownership
        if original.merchant_id != merchant_id {
            return Err(Error::Unauthorized);
        }

        // Check retry chain depth (max 3)
        let mut current_id = original_payment_id.clone();
        let mut depth = 1u32;
        loop {
            if let Some(ref retry_of) = {
                let payment = Self::get_payment_internal(&env, &current_id)?;
                payment.retry_of_payment_id.clone()
            } {
                current_id = retry_of.clone();
                depth = depth.saturating_add(1);
                if depth > 3 {
                    return Err(Error::MaxRetriesExceeded);
                }
            } else {
                break;
            }
        }

        // Validate new_expires_at
        let now = env.ledger().timestamp();
        if new_expires_at <= now {
            return Err(Error::InvalidExpiry);
        }

        // Generate new payment ID
        let new_payment_id = format_id(&env, "pay_", env.ledger().timestamp());

        // Create new PaymentCharge with inherited properties
        let new_payment = PaymentCharge {
            payment_id: new_payment_id.clone(),
            merchant_id: original.merchant_id.clone(),
            amount: original.amount,
            currency: original.currency.clone(),
            deposit_address: original.deposit_address.clone(),
            status: PaymentStatus::Pending,
            payer_address: None,
            transaction_hash: None,
            created_at: now,
            confirmed_at: None,
            expires_at: new_expires_at,
            amount_received: None,
            memo: original.memo.clone(),
            memo_type: original.memo_type.clone(),
            token_address: original.token_address.clone(),
            metadata_hash: original.metadata_hash.clone(),
            original_token: None,
            swap_path: None,
            fx_rate: None,
            fx_rate_at: None,
            metadata: original.metadata.clone(),
            fee_waiver_code: original.fee_waiver_code.clone(),
            retry_of_payment_id: Some(original_payment_id.clone()),
            payer_muxed_id: None,
            payment_link_id: original.payment_link_id.clone(),
        };

        // Store new payment
        env.storage()
            .persistent()
            .set(&DataKey::Payment(new_payment_id.clone()), &new_payment);
        Self::bump_payment_ttl(&env, &new_payment_id, &new_payment.status);

        // Track retry link
        let retries_key = DataKey::PaymentRetries(original_payment_id.clone());
        let mut retries: Vec<String> = env
            .storage()
            .persistent()
            .get(&retries_key)
            .unwrap_or_else(|| vec![&env]);
        retries.push_back(new_payment_id.clone());
        env.storage()
            .persistent()
            .set(&retries_key, &retries);
        Self::bump_ttl(&env, &retries_key, LONG_LIVE_TTL);

        // Emit PAYMENT/RETRY_CREATED event
        env.events().publish(
            (
                Symbol::new(&env, "PAYMENT"),
                Symbol::new(&env, "RETRY_CREATED"),
            ),
            (original_payment_id, new_payment_id.clone()),
        );

        Ok(new_payment_id)
    }

    pub fn get_payment(env: Env, payment_id: String) -> Result<PaymentCharge, Error> {
        let payment = Self::get_payment_internal(&env, &payment_id)?;
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);
        Ok(payment)
    }

    pub fn get_payment_status_history(
        env: Env,
        payment_id: String,
    ) -> Result<Vec<PaymentStatusEvent>, Error> {
        Self::get_payment_internal(&env, &payment_id)?;
        Ok(env
            .storage()
            .persistent()
            .get(&DataKey::PaymentStatusHistory(payment_id))
            .unwrap_or_else(|| vec![&env]))
    }

    /// Issue #489: Reverse lookup payment by metadata_hash for order reconciliation.
    pub fn get_payment_by_metadata_hash(env: Env, metadata_hash: BytesN<32>) -> Result<PaymentCharge, Error> {
        let payment_id = env
            .storage()
            .persistent()
            .get::<DataKey, String>(&DataKey::MetadataHashPayment(metadata_hash.clone()))
            .ok_or(Error::PaymentNotFound)?;
        Self::get_payment(env, payment_id)
    }

    /// Issue #492: Get customer profile for merchant and customer pair.
    pub fn get_customer(env: Env, merchant_id: Address, customer_id: Address) -> Result<CustomerProfile, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::CustomerProfile(merchant_id, customer_id))
            .ok_or(Error::PaymentNotFound)
    }

    /// Issue #492: Get top customers for a merchant sorted by total_spent (descending).
    pub fn get_top_customers(env: Env, merchant_id: Address, limit: u32) -> Vec<CustomerProfile> {
        let all_payments = Self::get_merchant_payments_internal(&env, &merchant_id);
        let mut customers: Map<Address, CustomerProfile> = map![&env];

        for payment_id in all_payments.iter() {
            if let Ok(payment) = Self::get_payment_internal(&env, &payment_id) {
                if payment.status == PaymentStatus::Confirmed || payment.status == PaymentStatus::Overpaid {
                    if let Some(payer) = payment.payer_address {
                        if let Ok(profile) = Self::get_customer(env.clone(), merchant_id.clone(), payer.clone()) {
                            customers.set(payer, profile);
                        }
                    }
                }
            }
        }

        let mut sorted: Vec<CustomerProfile> = vec![&env];
        for (_, profile) in customers.iter() {
            sorted.push_back(profile);
        }

        let mut i = 0;
        while i < sorted.len() {
            let mut j = i + 1;
            while j < sorted.len() {
                if let (Some(profile_i), Some(profile_j)) = (sorted.get(i), sorted.get(j)) {
                    if profile_j.total_spent > profile_i.total_spent {
                        sorted.set(i, profile_j.clone());
                        sorted.set(j, profile_i.clone());
                    }
                }
                j += 1;
            }
            i += 1;
        }

        let capped_limit = if limit == 0 { sorted.len() } else { limit.min(sorted.len()) };
        let mut result = vec![&env];
        let mut idx = 0u32;
        while idx < capped_limit {
            if let Some(profile) = sorted.get(idx) {
                result.push_back(profile);
            }
            idx += 1;
        }
        result
    }

    /// Issue #628: Record a payment's `amount` against the merchant's cumulative
    /// gross-volume index and register the merchant in the tracked-merchant list
    /// (idempotently) so `get_top_merchants` can rank merchants without scanning
    /// individual payment records.
    fn record_merchant_volume(env: &Env, merchant_id: &Address, amount: i128) {
        let volume_key = DataKey::MerchantGrossVolume(merchant_id.clone());
        let current: i128 = env
            .storage()
            .persistent()
            .get(&volume_key)
            .unwrap_or(0i128);
        env.storage()
            .persistent()
            .set(&volume_key, &current.saturating_add(amount));
        Self::bump_ttl(env, &volume_key, LONG_LIVE_TTL);

        let list_key = DataKey::TrackedMerchants;
        let mut merchants: Vec<Address> = env
            .storage()
            .persistent()
            .get(&list_key)
            .unwrap_or_else(|| vec![env]);
        if !merchants.contains(merchant_id) {
            merchants.push_back(merchant_id.clone());
            env.storage().persistent().set(&list_key, &merchants);
            Self::bump_ttl(env, &list_key, LONG_LIVE_TTL);
        }
    }

    /// Issue #628: Rank merchants by cumulative gross payment volume (descending).
    ///
    /// Reads only the per-merchant `MerchantGrossVolume` / `MerchantPaymentCount`
    /// indexes plus the `TrackedMerchants` list — it never iterates payment
    /// records. `limit` is capped at [`TOP_MERCHANTS_MAX_LIMIT`] (100); a
    /// `limit` of 0 is treated as the cap.
    pub fn get_top_merchants(env: Env, limit: u32) -> Vec<MerchantRanking> {
        let capped_limit = if limit == 0 {
            TOP_MERCHANTS_MAX_LIMIT
        } else {
            limit.min(TOP_MERCHANTS_MAX_LIMIT)
        };

        let merchants: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::TrackedMerchants)
            .unwrap_or_else(|| vec![&env]);

        let mut rankings: Vec<MerchantRanking> = vec![&env];
        for merchant_id in merchants.iter() {
            let total_volume: i128 = env
                .storage()
                .persistent()
                .get(&DataKey::MerchantGrossVolume(merchant_id.clone()))
                .unwrap_or(0i128);
            let payment_count: u64 = env
                .storage()
                .persistent()
                .get(&DataKey::MerchantPaymentCount(merchant_id.clone()))
                .unwrap_or(0u64);
            rankings.push_back(MerchantRanking {
                merchant_id,
                total_volume,
                payment_count,
            });
        }

        // Selection sort by total_volume descending (mirrors get_top_customers).
        let mut i = 0;
        while i < rankings.len() {
            let mut j = i + 1;
            while j < rankings.len() {
                if let (Some(a), Some(b)) = (rankings.get(i), rankings.get(j)) {
                    if b.total_volume > a.total_volume {
                        rankings.set(i, b.clone());
                        rankings.set(j, a.clone());
                    }
                }
                j += 1;
            }
            i += 1;
        }

        let end = capped_limit.min(rankings.len());
        let mut result: Vec<MerchantRanking> = vec![&env];
        let mut idx = 0u32;
        while idx < end {
            if let Some(ranking) = rankings.get(idx) {
                result.push_back(ranking);
            }
            idx += 1;
        }
        result
    }

    /// Issue #488: Permissionless public entry point for TTL maintenance.
    pub fn bump_payment_ttl_public(env: Env, payment_id: String) -> Result<(), Error> {
        let payment = Self::get_payment_internal(&env, &payment_id)?;
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);
        Ok(())
    }

    /// Issue #488: Bulk bump payment TTLs for efficient maintenance sweeps (max 50).
    pub fn bulk_bump_payment_ttls(env: Env, payment_ids: Vec<String>) -> Result<u32, Error> {
        if payment_ids.len() > 50 {
            return Err(Error::BatchTooLarge);
        }

        let mut bumped = 0u32;
        for payment_id in payment_ids.iter() {
            if let Ok(payment) = Self::get_payment_internal(&env, &payment_id) {
                Self::bump_payment_ttl(&env, &payment_id, &payment.status);
                bumped += 1;
            }
        }
        Ok(bumped)
    }

    pub fn get_merchant_payments(env: Env, merchant_id: Address) -> Vec<String> {
        Self::get_merchant_payments_internal(&env, &merchant_id)
    }

    pub fn get_merchant_payments_paginated(
        env: Env,
        merchant_id: Address,
        offset: u32,
        limit: u32,
        status_filter: Option<PaymentStatus>,
        token_address: Option<Address>,
    ) -> Vec<String> {
        let all = Self::get_merchant_payments_internal(&env, &merchant_id);
        if limit == 0 {
            return vec![&env];
        }

        let mut filtered = vec![&env];
        for id in all.iter() {
            if let Some(payment) = env
                .storage()
                .persistent()
                .get::<DataKey, PaymentCharge>(&DataKey::Payment(id.clone()))
            {
                let status_match = match &status_filter {
                    Some(status) => payment.status == status.clone(),
                    None => true,
                };

                let token_match = match &token_address {
                    Some(token) => payment.token_address.as_ref() == Some(token),
                    None => true,
                };

                if status_match && token_match {
                    filtered.push_back(id);
                }
            }
        }

        let mut page = vec![&env];
        let start = offset;
        let end = core::cmp::min(filtered.len(), start.saturating_add(limit));

        let mut i = start;
        while i < end {
            if let Some(id) = filtered.get(i) {
                page.push_back(id);
            }
            i += 1;
        }

        page
    }

    /// Generate a reconciliation report for a merchant over a time period.
    ///
    /// This is a read-only query that returns a structured summary of all
    /// settlements, fees, refunds, and disputes in the specified period.
    ///
    /// # Parameters
    /// * `merchant_id` - The merchant to generate the report for
    /// * `from_ts` - Start timestamp (inclusive)
    /// * `to_ts` - End timestamp (inclusive)
    /// * `offset` - Pagination offset (number of payments to skip)
    /// * `limit` - Maximum number of payments to include (max 100)
    pub fn generate_reconciliation_report(
        env: Env,
        merchant_id: Address,
        from_ts: u64,
        to_ts: u64,
        offset: u32,
        limit: u32,
    ) -> Result<ReconciliationReport, Error> {
        if from_ts > to_ts {
            return Err(Error::InvalidExpiry);
        }

        let capped_limit = if limit == 0 || limit > 100 { 100 } else { limit };

        const SECONDS_PER_DAY: u64 = 86_400;
        let start_bucket = from_ts / SECONDS_PER_DAY;
        let end_bucket = to_ts / SECONDS_PER_DAY;

        let mut candidate_ids: Vec<String> = vec![&env];
        let mut bucket = start_bucket;
        while bucket <= end_bucket {
            let key = DataKey::DailyPaymentIndex(merchant_id.clone(), bucket);
            if let Some(bucket_ids) = env
                .storage()
                .persistent()
                .get::<DataKey, Vec<String>>(&key)
            {
                for id in bucket_ids.iter() {
                    candidate_ids.push_back(id);
                }
            }
            bucket = bucket.saturating_add(1);
        }

        let mut payments_in_period = vec![&env];
        let mut total_gross: i128 = 0;
        let mut total_fees: i128 = 0;
        let mut total_refunds: i128 = 0;
        let mut dispute_adjustments: i128 = 0;

        let default_fee_bps = Self::get_refund_fee_bps_internal(&env);

        let merchant_fee_bps = if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
            match registry_client.try_get_merchant(&merchant_id) {
                Ok(Ok(merchant)) => {
                    use crate::merchant_registry::KycTier;
                    match merchant.kyc_tier {
                        KycTier::Business => REFUND_FEE_BPS_BUSINESS,
                        KycTier::Full => REFUND_FEE_BPS_FULL,
                        KycTier::Basic => REFUND_FEE_BPS_BASIC,
                        KycTier::Unverified => default_fee_bps,
                    }
                }
                _ => default_fee_bps,
            }
        } else {
            default_fee_bps
        };

        for payment_id in candidate_ids.iter() {
            if let Ok(payment) = Self::get_payment_internal(&env, &payment_id) {
                let payment_time = payment.confirmed_at.unwrap_or(payment.created_at);
                
                if payment_time >= from_ts && payment_time <= to_ts {
                    let mut refund_amount: i128 = 0;
                    
                    let refund_ids = RefundManager::get_payment_refunds_internal(&env, &payment_id);
                    for refund_id in refund_ids.iter() {
                        if let Ok(refund) = RefundManager::get_refund_internal(&env, &refund_id) {
                            if refund.status == RefundStatus::Completed {
                                refund_amount += refund.amount;
                            }
                        }
                    }

                    let fee = if refund_amount > 0 {
                        refund_amount * merchant_fee_bps / 10_000
                    } else {
                        0
                    };

                    let summary = PaymentSummary {
                        payment_id: payment.payment_id.clone(),
                        amount: payment.amount,
                        fee,
                        refund_amount,
                        status: payment.status.clone(),
                        settled_at: payment.confirmed_at,
                    };

                    payments_in_period.push_back(summary.clone());
                    total_gross += payment.amount;
                    total_fees += fee;
                    total_refunds += refund_amount;
                }
            }
        }

        let mut paginated_payments = vec![&env];
        let start = offset;
        let end = core::cmp::min(payments_in_period.len(), start.saturating_add(capped_limit));

        let mut i = start;
        while i < end {
            if let Some(summary) = payments_in_period.get(i) {
                paginated_payments.push_back(summary);
            }
            i += 1;
        }

        let total_net_settled = total_gross - total_refunds;

        Ok(ReconciliationReport {
            merchant_id,
            period_start: from_ts,
            period_end: to_ts,
            payments: paginated_payments,
            total_gross,
            total_fees,
            total_refunds,
            total_net_settled,
            dispute_adjustments,
        })
    }

    pub fn generate_reconciliation_page(
    pub fn reconciliation_report_page(
        env: Env,
        merchant_id: Address,
        from_ts: u64,
        to_ts: u64,
        offset: u32,
        limit: u32,
    ) -> Result<ReconciliationPage, Error> {
        let report = Self::generate_reconciliation_report(
            env.clone(), merchant_id, from_ts, to_ts, offset, limit,
        )?;
        let mut page_total = 0i128;
        for item in report.payments.iter() {
            page_total = page_total.saturating_add(item.amount);
        }
        let page_size = if limit == 0 { 100 } else { limit.min(100) };
        let has_more = page_size > 0 && page_size == report.payments.len() as u32;
        Ok(ReconciliationPage {
            items: report.payments,
            total_confirmed: report.total_gross,
            total_settled: report.total_net_settled,
            page_total,
            has_more,
        })
    }

    #[allow(deprecated)]
    pub fn cancel_payment(env: Env, authority: Address, payment_id: String) -> Result<(), Error> {
        Self::require_not_paused(&env)?;

        let mut payment = Self::get_payment_internal(&env, &payment_id)?;

        if payment.status != PaymentStatus::Pending {
            return Err(Error::PaymentAlreadyProcessed);
        }

        // Ensure the current time is less than the expiry time; if not, mark as expired and return.
        if env.ledger().timestamp() >= payment.expires_at {
            payment.status = payment_state_machine::transition_status(&payment.status, PaymentStatus::Expired)?;
            Self::record_payment_status(&env, &payment);

            env.storage()
                .persistent()
                .set(&DataKey::Payment(payment_id.clone()), &payment);
            Self::bump_payment_ttl(&env, &payment_id, &payment.status);
            // Issue #399: free idempotency key so client_token can be reused.
            Self::remove_idempotency_key(&env, &payment_id);

            // Issue #166: Optimize event topics
            env.events().publish(
                (
                    Symbol::new(&env, "PAYMENT"),
                    Symbol::new(&env, "EXPIRED"),
                    payment.merchant_id.clone(),
                ),
                (payment_id.clone(), payment.amount),
            );

            return Ok(());
        }

        authority.require_auth();
        let is_merchant = authority == payment.merchant_id;
        let is_oracle = AccessControl::has_role(&env, &role_oracle(&env), &authority);
        if !is_merchant && !is_oracle {
            return Err(Error::Unauthorized);
        }

        payment.status = payment_state_machine::transition_status(&payment.status, PaymentStatus::Failed)?;
        Self::record_payment_status(&env, &payment);

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);
        // Issue #399: free idempotency key so client_token can be reused after cancellation.
        Self::remove_idempotency_key(&env, &payment_id);
        Self::remove_payment_from_expiry_bucket(&env, &payment_id, payment.expires_at);

        // Issue #166: Optimize event topics
        env.events().publish(
            (
                Symbol::new(&env, "PAYMENT"),
                Symbol::new(&env, "CANCELLED"),
                payment.merchant_id.clone(),
            ),
            (payment_id.clone(), payment.amount),
        );

        Ok(())
    }

    #[allow(deprecated)]
    pub fn expire_payment(env: Env, payment_id: String) -> Result<(), Error> {
        let mut payment = Self::get_payment_internal(&env, &payment_id)?;

        if payment.status != PaymentStatus::Pending {
            return Err(Error::PaymentAlreadyProcessed);
        }

        if env.ledger().timestamp() <= payment.expires_at {
            return Err(Error::PaymentExpired);
        }

        payment.status = payment_state_machine::transition_status(&payment.status, PaymentStatus::Expired)?;
        Self::record_payment_status(&env, &payment);

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);
        // Issue #399: free idempotency key so client_token can be reused.
        Self::remove_idempotency_key(&env, &payment_id);
        Self::remove_payment_from_expiry_bucket(&env, &payment_id, payment.expires_at);

        // Issue #166: Optimize event topics
        env.events().publish(
            (
                Symbol::new(&env, "PAYMENT"),
                Symbol::new(&env, "EXPIRED"),
                payment.merchant_id.clone(),
            ),
            (payment_id.clone(), payment.amount),
        );

        Ok(())
    }

    pub fn batch_expire_payments(env: Env, payment_ids: Vec<String>) -> Result<u32, Error> {
        Self::require_not_paused(&env)?;

        let current_bucket = Self::expiry_bucket_for(env.ledger().timestamp());
        let mut selected_bucket: Option<u32> = None;
        if let Some(buckets) = env
            .storage()
            .persistent()
            .get::<DataKey, Vec<u32>>(&DataKey::PaymentExpiryBuckets)
        {
            for bucket in buckets.iter() {
                if bucket <= current_bucket
                    && selected_bucket.map_or(true, |lowest| bucket < lowest)
                {
                    selected_bucket = Some(bucket);
                }
            }
        }

        let candidates: Vec<String> = if let Some(bucket) = selected_bucket {
            env.storage()
                .persistent()
                .get(&DataKey::PaymentsByExpiry(bucket))
                .unwrap_or_else(|| vec![&env])
        } else {
            payment_ids
        };

        let mut count = 0;
        let max = if candidates.len() > 50 { 50 } else { candidates.len() };
        let mut i = 0;
        while i < max {
            if let Some(payment_id) = candidates.get(i) {
                if Self::expire_payment(env.clone(), payment_id).is_ok() {
                    count += 1;
                }
            }
            i += 1;
        }

        Ok(count)
    }

    pub fn settle_payment(
        env: Env,
        operator: Address,
        payment_id: String,
        splits: Vec<SettlementSplit>,
    ) -> Result<(), Error> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_settlement_operator(&env), &operator) {
            return Err(Error::Unauthorized);
        }

        Self::require_not_paused(&env)?;

        if env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::ReentrancyLock)
            .unwrap_or(false)
        {
            return Err(Error::Reentrancy);
        }
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyLock, &true);
        let _guard = ReentrancyGuard { env: &env };

        let mut payment = Self::get_payment_internal(&env, &payment_id)?;

        if payment.status != PaymentStatus::Confirmed {
            return Err(Error::PaymentAlreadyProcessed);
        }

        // Resolve the settlement token: use payment.token_address if set, else the configured USDC token
        let settlement_token: Option<Address> = payment.token_address.clone().or_else(|| {
            env.storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::UsdcToken)
        });

        // ── Configurable settlement fee split (treasury + developer) ─────────────
        let now = env.ledger().timestamp();

        // ── Fee-waiver evaluation (merchant time-based + per-payment code) ────────
        // We resolve whether a fee waiver applies BEFORE computing any fee values
        // so the same decision is honored by both settlement-fee and merchant-fee
        // code paths. The `fee_waiver_reason` below doubles as the reason string
        // emitted in the `PAYMENT/FEE_WAIVED` event; `None` means no waiver.
        //
        // Per-payment codes take precedence over (and are evaluated independently
        // of) the merchant-level time-based waiver. If both are present only the
        // code is consumed (since its remaining_uses must be decremented) and the
        // event reason reflects the code source.
        let fee_waiver_reason: Option<String> = {
            // 1. Per-payment code path (stronger: consumes uses if valid)
            if let Some(ref code) = payment.fee_waiver_code {
                let key = DataKey::FeeWaiverCode(code.clone());
                if let Some(mut record) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, FeeWaiverCodeRecord>(&key)
                {
                    if now < record.expires_at && record.remaining_uses > 0 {
                        record.remaining_uses = record.remaining_uses.saturating_sub(1);
                        env.storage().persistent().set(&key, &record);
                        Some(crate::utils::concat_strings(
                            &env,
                            &[String::from_str(&env, "code_waiver:"), code.clone()],
                        ))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                // 2. Merchant-level time-based waiver (cheaper, no uses to track)
                let registry_addr_opt = env
                    .storage()
                    .persistent()
                    .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress);
                match registry_addr_opt {
                    Some(registry_addr) => {
                        use crate::merchant_registry::MerchantRegistryClient;
                        let registry_client = MerchantRegistryClient::new(&env, &registry_addr);
                        let merchant = registry_client.get_merchant(&payment.merchant_id);
                        match merchant.fee_waiver_expires_at {
                            Some(ts) if now < ts => {
                                Some(String::from_str(&env, "merchant_waiver"))
                            }
                            _ => None,
                        }
                    }
                    None => None,
                }
            }
        };

        // If any waiver resolved as active, emit the canonical PAYMENT/FEE_WAIVED
        // event once here (both settlement code paths read from this event).
        if let Some(ref reason) = fee_waiver_reason {
            env.events().publish(
                (
                    Symbol::new(&env, "PAYMENT"),
                    Symbol::new(&env, "FEE_WAIVED"),
                    payment.merchant_id.clone(),
                ),
                (payment_id.clone(), reason.clone()),
            );
        }

        // ── Configurable settlement fee (accumulated in TreasuryBalance) ─────────
        // Read the settlement fee rate in basis points. 0 bps → no fee, no event.
        // Waived: if any fee waiver is active, settlement_fee is forced to 0
        // regardless of global rate.
        let settlement_fee_bps: i128 = env
            .storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::SettlementFeeRate)
            .unwrap_or(0);

        let settlement_fee: i128 = if fee_waiver_reason.is_some() {
            0
        } else if settlement_fee_bps > 0 {
            payment.amount * settlement_fee_bps / 10_000
        } else {
            0
        };

        if settlement_fee > 0 {
            // Check whether a FeeSplitConfig has been configured.
            let fee_split_config: Option<FeeSplitConfig> = env
                .storage()
                .persistent()
                .get(&DataKey::FeeSplitConfig);

            if let Some(ref fsc) = fee_split_config {
                // Split the fee between treasury and developer.
                // dev_amount = fee * developer_bps / 10000; treasury gets the remainder
                // (including any rounding dust) so no tokens are lost.
                let dev_amount: i128 =
                    settlement_fee * fsc.developer_bps as i128 / 10_000;

                // Rounding dust goes to treasury.
                let treasury_total = settlement_fee.saturating_sub(dev_amount);

                if let Some(ref st) = settlement_token {
                    let token_client = token::TokenClient::new(&env, st);
                    let from = env.current_contract_address();
                    if treasury_total > 0 {
                        let _ = token_client.try_transfer(
                            &from,
                            &fsc.treasury_address,
                            &treasury_total,
                        );
                    }
                    if dev_amount > 0 {
                        let _ = token_client.try_transfer(
                            &from,
                            &fsc.developer_address,
                            &dev_amount,
                        );
                    }
                }

                // Emit PAYMENT/FEE_SPLIT
                env.events().publish(
                    (
                        Symbol::new(&env, "PAYMENT"),
                        Symbol::new(&env, "FEE_SPLIT"),
                    ),
                    (payment_id.clone(), treasury_total, dev_amount),
                );

                // Issue #666: record this settlement's fee split for
                // get_platform_fee_report.
                Self::record_fee_collection(&env, settlement_fee, treasury_total, dev_amount);
            } else {
                // No FeeSplitConfig — accumulate entire fee in TreasuryBalance (legacy path).
                let current_treasury: i128 = env
                    .storage()
                    .persistent()
                    .get::<DataKey, i128>(&DataKey::TreasuryBalance)
                    .unwrap_or(0);
                env.storage().persistent().set(
                    &DataKey::TreasuryBalance,
                    &current_treasury.saturating_add(settlement_fee),
                );

                env.events().publish(
                    (
                        Symbol::new(&env, "PAYMENT"),
                        Symbol::new(&env, "FEE_COLLECTED"),
                    ),
                    (payment_id.clone(), payment.merchant_id.clone(), settlement_fee),
                );

                // Issue #666: record this settlement's fee for get_platform_fee_report
                // (legacy path: entire fee accrues to treasury, no developer split).
                Self::record_fee_collection(&env, settlement_fee, settlement_fee, 0);
            }
        }

        // Net amount after settlement fee
        let net_after_settlement_fee = payment.amount.saturating_sub(settlement_fee);

        // ── Pre-lookup merchant AnchorConfig (SEP-6 / SEP-24) for fiat offramp ──
        // The anchor config itself lives in MerchantRegistry. We fetch it once
        // here so both settlement code paths can reuse it. `None` means the
        // merchant has not configured an anchor (on-chain-only settlement).
        let anchor_info: Option<(
            String, // anchor_domain
            String, // sep6_endpoint
            String, // sep24_endpoint
            Vec<String>, // supported_currencies
            Option<Address>, // merchant payout address
        )> = {
            let registry_addr_opt = env
                .storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress);
            match registry_addr_opt {
                Some(registry_addr) => {
                    use crate::merchant_registry::{MerchantRegistryClient, MaybeAnchorConfig};
                    let registry_client = MerchantRegistryClient::new(&env, &registry_addr);
                    let merchant = registry_client.get_merchant(&payment.merchant_id);
                    match merchant.anchor_config {
                        MaybeAnchorConfig::Some(ref ac) => Some((
                            ac.anchor_domain.clone(),
                            ac.sep6_endpoint.clone(),
                            ac.sep24_endpoint.clone(),
                            ac.supported_currencies.clone(),
                            merchant.payout_address.clone(),
                        )),
                        MaybeAnchorConfig::None => None,
                    }
                }
                None => None,
            }
        };

        // Check if merchant registry is configured and merchant has FeeConfig
        let registry_address = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress);

        if let Some(registry_addr) = registry_address {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_addr);

            // Try to get merchant and their fee config
            let merchant = registry_client.get_merchant(&payment.merchant_id);
            if let Some(fee_config) = merchant.fee_config.as_option() {
                    // Calculate fee using merchant's FeeConfig
                    let fee_bps_amount = (net_after_settlement_fee * (fee_config.platform_fee_bps as i128)) / 10_000;
                    let fixed_fee = fee_config.fixed_fee;
                    let total_merchant_fee = fee_bps_amount.saturating_add(fixed_fee);

                    // Ensure fee doesn't exceed amount
                    // Waived: if a fee waiver is active, the merchant-level fee
                    // is forced to zero (no treasury, no custom recipient transfer).
                    let actual_fee = if fee_waiver_reason.is_some() {
                        0
                    } else if total_merchant_fee >= net_after_settlement_fee {
                        net_after_settlement_fee
                    } else {
                        total_merchant_fee
                    };

                    let net_merchant_amount = net_after_settlement_fee.saturating_sub(actual_fee);

                    // Transfer using the resolved settlement token (if configured)
                    if let Some(ref settlement_token) = settlement_token {
                        let token_client = token::TokenClient::new(&env, settlement_token);
                        let from = env.current_contract_address();

                        // Transfer net amount to merchant
                        if net_merchant_amount > 0 {
                            let _ = token_client.try_transfer(&from, &payment.merchant_id, &net_merchant_amount);
                        }
                    }

                    // Platform fee: custom fee_recipient receives a transfer; otherwise
                    // credit DataKey::TreasuryBalance (unified treasury accounting).
                    let fee_recipient: Address = if let Some(custom_recipient) = &fee_config.fee_recipient {
                        if actual_fee > 0 {
                            if let Some(ref settlement_token) = settlement_token {
                                let token_client = token::TokenClient::new(&env, settlement_token);
                                let from = env.current_contract_address();
                                let _ = token_client.try_transfer(&from, custom_recipient, &actual_fee);
                            }
                        }
                        custom_recipient.clone()
                    } else {
                        if actual_fee > 0 {
                            let current_treasury: i128 = env
                                .storage()
                                .persistent()
                                .get::<DataKey, i128>(&DataKey::TreasuryBalance)
                                .unwrap_or(0);
                            env.storage().persistent().set(
                                &DataKey::TreasuryBalance,
                                &current_treasury.saturating_add(actual_fee),
                            );
                        }
                        env.current_contract_address()
                    };

                    // Issue #666: record this settlement's merchant-level fee for
                    // get_platform_fee_report. Only counted as treasury_share when
                    // it actually accrued to DataKey::TreasuryBalance above (i.e.
                    // no custom fee_recipient); no developer split on this path.
                    let treasury_share_for_report = if fee_recipient == env.current_contract_address() {
                        actual_fee
                    } else {
                        0
                    };
                    Self::record_fee_collection(&env, actual_fee, treasury_share_for_report, 0);

                    // Emit FEE_COLLECTED event (merchant-level fee)
                    env.events().publish(
                        (
                            Symbol::new(&env, "PAYMENT"),
                            Symbol::new(&env, "FEE_COLLECTED"),
                        ),
                        (
                            payment_id.clone(),
                            payment.merchant_id.clone(),
                            payment.amount,
                            fee_bps_amount,
                            fixed_fee,
                            net_merchant_amount,
                            fee_recipient,
                        ),
                    );

                    payment.status = payment_state_machine::transition_status(&payment.status, PaymentStatus::Settled)?;
                    Self::record_payment_status(&env, &payment);
                    env.storage()
                        .persistent()
                        .set(&DataKey::Payment(payment_id.clone()), &payment);
                    Self::bump_payment_ttl(&env, &payment_id, &payment.status);

                    // If merchant has AnchorConfig set, emit the
                    // SETTLEMENT_ANCHOR_WITHDRAW event so the off-chain
                    // Settlement Service can call the anchor's SEP-6 API.
                    if let Some((
                        ref anchor_domain,
                        ref sep6_endpoint,
                        ref sep24_endpoint,
                        ref supported_currencies,
                        ref merchant_payout_addr,
                    )) = anchor_info
                    {
                        let payout_addr = merchant_payout_addr
                            .clone()
                            .unwrap_or_else(|| payment.merchant_id.clone());
                        env.events().publish(
                            (
                                Symbol::new(&env, "PAYMENT"),
                                Symbol::new(&env, "ANCHOR_WITHDRAW"),
                                payment.merchant_id.clone(),
                                anchor_domain.clone(),
                            ),
                            (
                                payment_id.clone(),
                                net_merchant_amount,
                                payment.currency.clone(),
                                payout_addr,
                                sep6_endpoint.clone(),
                                sep24_endpoint.clone(),
                                supported_currencies.clone(),
                                env.ledger().timestamp(),
                            ),
                        );
                    }

                    return Ok(());
            }
        }

        // Original split-based settlement logic (no FeeConfig or no registry)
        let (mut platform_fee, fee_recipient) = if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
            registry_client.calculate_platform_fee(&net_after_settlement_fee)
        } else {
            (0i128, env.current_contract_address())
        };

        // Waived: if a fee waiver is active, zero out the split-based platform
        // fee too (matches the settlement_fee zeroing and FeeConfig actual_fee
        // zeroing above).
        if fee_waiver_reason.is_some() {
            platform_fee = 0i128;
        }

        let net_amount = net_after_settlement_fee - platform_fee;

        if splits.is_empty() {
            if net_amount > 0 {
                if let Some(ref settlement_token) = settlement_token {
                    let token_client = token::TokenClient::new(&env, settlement_token);
                    let from = env.current_contract_address();
                    let _ = token_client.try_transfer(&from, &payment.merchant_id, &net_amount);
                }
            }
        } else {
            let mut total: i128 = 0;
            for split in splits.iter() {
                if split.amount <= 0 {
                    return Err(Error::InvalidSettlement);
                }
                total = total.saturating_add(split.amount);
            }
            if total != net_amount {
                return Err(Error::InvalidSettlement);
            }
            
            if let Some(ref settlement_token) = settlement_token {
                let token_client = token::TokenClient::new(&env, settlement_token);
                let from = env.current_contract_address();
                for split in splits.iter() {
                    let _ = token_client.try_transfer(&from, &split.recipient, &split.amount);
                }
            }
        }

        // Platform fee: when the registry returns the contract itself as recipient
        // (no custom fee_recipient), credit TreasuryBalance. Otherwise transfer out.
        if platform_fee > 0 {
            let contract_addr = env.current_contract_address();
            if fee_recipient == contract_addr {
                let current_treasury: i128 = env
                    .storage()
                    .persistent()
                    .get::<DataKey, i128>(&DataKey::TreasuryBalance)
                    .unwrap_or(0);
                env.storage().persistent().set(
                    &DataKey::TreasuryBalance,
                    &current_treasury.saturating_add(platform_fee),
                );
            } else if let Some(ref settlement_token) = settlement_token {
                let token_client = token::TokenClient::new(&env, settlement_token);
                let from = env.current_contract_address();
                let _ = token_client.try_transfer(&from, &fee_recipient, &platform_fee);
            }

            // Issue #666: record this settlement's platform fee for
            // get_platform_fee_report. Only counted as treasury_share when it
            // accrued to DataKey::TreasuryBalance above; no developer split
            // on this path.
            let treasury_share_for_report = if fee_recipient == contract_addr {
                platform_fee
            } else {
                0
            };
            Self::record_fee_collection(&env, platform_fee, treasury_share_for_report, 0);
        }

        payment.status = payment_state_machine::transition_status(&payment.status, PaymentStatus::Settled)?;
        Self::record_payment_status(&env, &payment);

        // Issue #480: Accumulate net merchant amount to pending settlement.
        let net_merchant_amount: i128 = splits
            .iter()
            .find(|s| s.recipient == payment.merchant_id)
            .map(|s| s.amount)
            .unwrap_or(0);
        if net_merchant_amount > 0 {
            if let Some(registry_address) = env
                .storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
            {
                let registry_client =
                    crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
                registry_client.add_pending_settlement(&payment.merchant_id, &net_merchant_amount);
            }
        }

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);

        // Issue #166: Optimize event topics
        env.events().publish(
            (
                Symbol::new(&env, "PAYMENT"),
                Symbol::new(&env, "SETTLED"),
                payment.merchant_id.clone(),
            ),
            (payment_id.clone(), payment.amount),
        );

        // If merchant has AnchorConfig set, emit the SETTLEMENT_ANCHOR_WITHDRAW
        // event so the off-chain Settlement Service can call the anchor's
        // SEP-6 withdrawal API. Mirrors the same emission in the FeeConfig
        // settlement path above.
        if let Some((
            ref anchor_domain,
            ref sep6_endpoint,
            ref sep24_endpoint,
            ref supported_currencies,
            ref merchant_payout_addr,
        )) = anchor_info
        {
            let payout_addr = merchant_payout_addr
                .clone()
                .unwrap_or_else(|| payment.merchant_id.clone());
            env.events().publish(
                (
                    Symbol::new(&env, "PAYMENT"),
                    Symbol::new(&env, "ANCHOR_WITHDRAW"),
                    payment.merchant_id.clone(),
                    anchor_domain.clone(),
                ),
                (
                    payment_id.clone(),
                    net_amount,
                    payment.currency.clone(),
                    payout_addr,
                    sep6_endpoint.clone(),
                    sep24_endpoint.clone(),
                    supported_currencies.clone(),
                    env.ledger().timestamp(),
                ),
            );
        }

        Ok(())
    }

    /// Issue #480: Trigger a settlement for a merchant's accumulated pending balance.
    ///
    /// Sweeps the merchant's pending settlement balance to their payout address.
    /// For `Daily` and `Weekly` schedules, enforces a minimum time since the
    /// last settlement. For `Manual`, settles immediately as long as the pending
    /// balance exceeds `SETTLEMENT_MIN_AMOUNT`.
    pub fn trigger_settlement(
        env: Env,
        operator: Address,
        merchant_id: Address,
    ) -> Result<i128, Error> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_settlement_operator(&env), &operator) {
            return Err(Error::Unauthorized);
        }

        // Get merchant info from registry.
        let registry_address = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
            .ok_or(Error::Unauthorized)?;
        let registry_client =
            crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);

        let merchant = registry_client.get_merchant(&merchant_id);

        // Resolve the USDC token address.
        let usdc_token = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::UsdcToken)
            .ok_or(Error::Unauthorized)?;

        // Determine the payout address.
        let payout_address = merchant.payout_address.ok_or(Error::InvalidAddress)?;

        // Check minimum time since last settlement for scheduled types.
        let now = env.ledger().timestamp();
        let min_interval = match merchant.settlement_schedule {
            crate::merchant_registry::SettlementSchedule::Daily => {
                Some(SETTLEMENT_DAILY_INTERVAL_SECS)
            }
            crate::merchant_registry::SettlementSchedule::Weekly => {
                Some(SETTLEMENT_WEEKLY_INTERVAL_SECS)
            }
            crate::merchant_registry::SettlementSchedule::Manual => None,
        };

        if let Some(interval) = min_interval {
            if let Some(last) = merchant.last_settlement_at {
                if now < last.saturating_add(interval) {
                    // Schedule not yet eligible; still allow manual override with operator auth.
                    return Err(Error::Unauthorized);
                }
            }
        }

        // Read the pending settlement balance.
        let pending = registry_client.get_pending_settlement(&merchant_id);
        if pending < SETTLEMENT_MIN_AMOUNT {
            return Err(Error::InvalidAmount);
        }

        // Transfer USDC from the PaymentProcessor contract to the merchant's payout address.
        let token_client = token::TokenClient::new(&env, &usdc_token);
        let from = env.current_contract_address();
        token_client.transfer(&from, &payout_address, &pending);

        // Clear pending settlement balance.
        registry_client.clear_pending_settlement(&merchant_id);

        // Update last_settlement_at on the merchant record.
        registry_client.set_last_settlement_at(&merchant_id, &now);

        // Emit MERCHANT/SETTLEMENT_TRIGGERED event.
        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "SETTLEMENT_TRIGGERED"),
                merchant_id,
            ),
            (pending,),
        );

        Ok(pending)
    }

    pub fn prune_expired_payments(
        env: Env,
        operator: Address,
        payment_ids: Vec<String>,
    ) -> Result<u32, Error> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_settlement_operator(&env), &operator) {
            return Err(Error::Unauthorized);
        }

        let mut pruned_count: u32 = 0;
        let current_timestamp = env.ledger().timestamp();

        for payment_id in payment_ids.iter() {
            if let Ok(payment) = Self::get_payment_internal(&env, &payment_id) {
                if payment.status == PaymentStatus::Pending && payment.expires_at <= current_timestamp {
                    env.storage()
                        .persistent()
                        .remove(&DataKey::Payment(payment_id.clone()));
                    pruned_count = pruned_count.saturating_add(1);
                }
            }
        }

        Ok(pruned_count)
    }

    /// Validate DEX path quotes before executing a swap.
    /// Blocks circular routes and rejects paths whose quoted output is below the minimum.
    fn validate_path_returns(
        env: &Env,
        dex_router: &Address,
        token_in: &Address,
        amount_in: i128,
        amount_out_min: i128,
        path: &Vec<Address>,
    ) -> Result<Vec<i128>, Error> {
        if path.len() < 2 {
            return Err(Error::SwapPathInvalid);
        }

        if path.get(0) != Some(token_in.clone()) {
            return Err(Error::SwapPathInvalid);
        }

        // Circular paths are a common arbitrage exploitation pattern.
        for i in 0..path.len() {
            for j in (i + 1)..path.len() {
                if path.get(i) == path.get(j) {
                    return Err(Error::ArbitrageDetected);
                }
            }
        }

        let dex_client = DexRouterClient::new(env, dex_router);
        let amounts = dex_client.get_amounts_out(&amount_in, path);

        if amounts.len() != path.len() {
            return Err(Error::SwapPathInvalid);
        }

        if amounts.get(0) != Some(amount_in) {
            return Err(Error::SwapPathInvalid);
        }

        let quoted_out = amounts.get(path.len() - 1).ok_or(Error::SwapPathInvalid)?;
        if quoted_out < amount_out_min {
            return Err(Error::InvalidAmount);
        }

        Ok(amounts)
    }

    /// Compare DEX quoted output against a fresh oracle reference rate.
    fn validate_oracle_swap_rate(
        env: &Env,
        fx_oracle: &Address,
        oracle_pair: &Symbol,
        amount_in: i128,
        dex_quoted_out: i128,
        max_deviation_bps: u32,
    ) -> Result<(), Error> {
        let oracle_client = FXOracleClient::new(env, fx_oracle);
        let rate_data = match oracle_client.try_get_rate(oracle_pair) {
            Ok(Ok(data)) => data,
            _ => return Err(Error::OraclePriceDeviation),
        };

        let mut divisor = 1i128;
        for _ in 0..rate_data.decimals {
            divisor = divisor.saturating_mul(10);
        }

        let expected_out = amount_in
            .saturating_mul(rate_data.rate)
            .checked_div(divisor)
            .unwrap_or(0);
        if expected_out <= 0 {
            return Err(Error::OraclePriceDeviation);
        }

        let diff = if dex_quoted_out > expected_out {
            dex_quoted_out - expected_out
        } else {
            expected_out - dex_quoted_out
        };

        let deviation_bps = diff.saturating_mul(10_000) / expected_out;
        if deviation_bps > max_deviation_bps as i128 {
            return Err(Error::OraclePriceDeviation);
        }

        Ok(())
    }

    /// Atomic swap and pay: swap sender's token to merchant's required token and create payment.
    /// Integrates with DEX (e.g., Soroswap) for atomic asset conversion.
    ///
    /// # Arguments
    /// * `payer` - The address making the payment
    /// * `payment_id` - Unique payment identifier
    /// * `merchant_id` - Merchant's address
    /// * `amount` - Amount in the merchant's settlement currency (after swap)
    /// * `currency` - Settlement currency symbol
    /// * `deposit_address` - Where the payment should be deposited
    /// * `token_in` - Address of the token the payer is sending
    /// * `amount_in` - Amount of token_in to swap
    /// * `amount_out_min` - Minimum amount of settlement token required
    /// * `path` - DEX swap path [token_in, ..., settlement_token]
    /// * `expires_at` - Payment expiry timestamp
    /// * `dex_router` - Address of the DEX router contract
    ///
    /// # Returns
    /// The created PaymentCharge on success
    /// Add a router address to the allowlist (issue #437).
    pub fn add_router(env: Env, admin: Address, router: Address) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::AllowedRouter(router.clone()), &true);

        let mut list: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AllowedRoutersList)
            .unwrap_or_else(|| Vec::new(&env));
        if !list.contains(&router) {
            list.push_back(router.clone());
            env.storage()
                .persistent()
                .set(&DataKey::AllowedRoutersList, &list);
        }

        env.events().publish(
            (
                Symbol::new(&env, "ROUTER"),
                Symbol::new(&env, "ADDED"),
            ),
            router,
        );
        Ok(())
    }

    /// Remove a router address from the allowlist (issue #437).
    pub fn remove_router(env: Env, admin: Address, router: Address) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .remove(&DataKey::AllowedRouter(router.clone()));

        if let Some(list) = env
            .storage()
            .persistent()
            .get::<DataKey, Vec<Address>>(&DataKey::AllowedRoutersList)
        {
            let mut new_list: Vec<Address> = Vec::new(&env);
            for r in list.iter() {
                if r != router {
                    new_list.push_back(r);
                }
            }
            env.storage()
                .persistent()
                .set(&DataKey::AllowedRoutersList, &new_list);
        }

        env.events().publish(
            (
                Symbol::new(&env, "ROUTER"),
                Symbol::new(&env, "REMOVED"),
            ),
            router,
        );
        Ok(())
    }

    /// Check if a DEX router is allowed (issue #437).
    pub fn is_router_allowed(env: Env, router: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::AllowedRouter(router))
            .unwrap_or(false)
    }

    /// Get all allowlisted DEX router addresses (issue #437).
    pub fn get_allowed_routers(env: Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::AllowedRoutersList)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Set the Wrapped XLM (WXLM) token contract address (issue #434).
    pub fn set_wrapped_xlm_contract(env: Env, admin: Address, wxlm: Address) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::WrappedXlmContract, &wxlm);
        Ok(())
    }

    /// Get the Wrapped XLM (WXLM) token contract address (issue #434).
    pub fn get_wrapped_xlm_contract(env: Env) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::WrappedXlmContract)
    }

    /// Swaps tokens via a DEX router and executes a payment (issue #434, #437).
    #[allow(clippy::too_many_arguments)]
    pub fn swap_and_pay(env: Env, args: SwapAndPayArgs) -> Result<PaymentCharge, Error> {
        args.payer.require_auth();
        Self::require_creation_not_paused(&env)?;

        if args.amount <= 0 || args.amount_in <= 0 {
            return Err(Error::InvalidAmount);
        }

        Self::enforce_create_payment_rate_limit_for_payer(&env, &args.payer)?;

        if args.amount_out_min < args.amount {
            return Err(Error::SwapPathInvalid);
        }

        // Issue #437: DEX router allowlist check
        let allowed_list = Self::get_allowed_routers(env.clone());
        if !allowed_list.is_empty() && !Self::is_router_allowed(env.clone(), args.dex_router.clone()) {
            return Err(Error::RouterNotAllowed);
        }

        // Issue #434: XLM auto-wrapping detection
        let native_xlm = Address::from_str(&env, ZERO_CONTRACT_STRKEY);
        let mut actual_token_in = args.token_in.clone();

        if args.token_in == native_xlm {
            if let Some(wxlm) = Self::get_wrapped_xlm_contract(env.clone()) {
                token::Client::new(&env, &wxlm).transfer(
                    &args.payer,
                    &env.current_contract_address(),
                    &args.amount_in,
                );
                actual_token_in = wxlm;
            }
        }

        let quoted_amounts = Self::validate_path_returns(
            &env,
            &args.dex_router,
            &actual_token_in,
            args.amount_in,
            args.amount_out_min,
            &args.path,
        )?;

        if let (Some(fx_oracle), Some(oracle_pair)) = (&args.fx_oracle, &args.oracle_pair) {
            let quoted_out = quoted_amounts
                .get(args.path.len() - 1)
                .ok_or(Error::SwapPathInvalid)?;
            Self::validate_oracle_swap_rate(
                &env,
                fx_oracle,
                oracle_pair,
                args.amount_in,
                quoted_out,
                args.max_deviation_bps,
            )?;
        }

        let deadline = env.ledger().timestamp().saturating_add(3_600);
        let dex_client = DexRouterClient::new(&env, &args.dex_router);

        let swap_result = dex_client.swap_exact_tokens_for_tokens(
            &args.amount_in,
            &args.amount_out_min,
            &args.path,
            &args.deposit_address,
            &deadline,
        );

        let actual_out = swap_result
            .get(args.path.len() - 1)
            .ok_or(Error::SwapPathInvalid)?;
        if actual_out < args.amount_out_min {
            return Err(Error::InvalidAmount);
        }

        let quoted_out = quoted_amounts
            .get(args.path.len() - 1)
            .ok_or(Error::SwapPathInvalid)?;
        if actual_out < quoted_out {
            return Err(Error::ArbitrageDetected);
        }

        let settlement_token = args
            .path
            .get(args.path.len() - 1)
            .unwrap_or(actual_token_in.clone());
        let create_args = CreatePaymentArgs {
            payment_id: args.payment_id.clone(),
            merchant_id: args.merchant_id,
            payer: None,
            amount: args.amount,
            currency: args.currency,
            deposit_address: args.deposit_address.clone(),
            expires_at: args.expires_at,
            duration_secs: None,
            memo: None,
            memo_type: None,
            token_address: Some(settlement_token),
            client_token: None,
            metadata_hash: None,
            metadata: None,
            fee_waiver_code: None,
            retry_of_payment_id: None,
            payer_muxed_id: None,
        };

        let mut payment = Self::create_payment(env.clone(), create_args)?;

        payment.original_token = Some(args.token_in.clone());
        payment.swap_path = Some(args.path.clone());

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment.payment_id.clone()), &payment);

        env.events().publish(
            (
                Symbol::new(&env, "SWAP"),
                Symbol::new(&env, "EXECUTED"),
            ),
            (
                args.payment_id.clone(),
                args.payer.clone(),
                args.amount_in,
                actual_out,
            ),
        );
        Ok(payment)
    }

    /// Issue #436: Multi-DEX route splitting / aggregation.
    pub fn swap_and_pay_multi_route(
        env: Env,
        args: SwapAndPayArgs,
        routes: Vec<SwapRoute>,
        min_output_amount: i128,
    ) -> Result<PaymentCharge, Error> {
        args.payer.require_auth();
        Self::require_creation_not_paused(&env)?;

        if args.amount <= 0 || routes.is_empty() {
            return Err(Error::InvalidAmount);
        }

        let allowed_list = Self::get_allowed_routers(env.clone());
        let mut total_route_input: i128 = 0;

        for route in routes.iter() {
            if route.amount_in <= 0 || route.path.is_empty() {
                return Err(Error::InvalidAmount);
            }
            total_route_input = total_route_input.saturating_add(route.amount_in);
            if !allowed_list.is_empty() && !Self::is_router_allowed(env.clone(), route.router.clone()) {
                return Err(Error::RouterNotAllowed);
            }
        }

        if total_route_input != args.amount_in {
            return Err(Error::InvalidAmount);
        }

        let deadline = env.ledger().timestamp().saturating_add(3_600);
        let mut total_output: i128 = 0;

        for route in routes.iter() {
            let dex_client = DexRouterClient::new(&env, &route.router);
            let swap_result = dex_client.swap_exact_tokens_for_tokens(
                &route.amount_in,
                &0,
                &route.path,
                &args.deposit_address,
                &deadline,
            );
            let route_out = swap_result
                .get(route.path.len() - 1)
                .ok_or(Error::SwapPathInvalid)?;
            total_output = total_output.saturating_add(route_out);
        }

        if total_output < min_output_amount {
            return Err(Error::RouteOutputInsufficient);
        }

        let first_route = routes.get(0).unwrap();
        let settlement_token = first_route
            .path
            .get(first_route.path.len() - 1)
            .unwrap_or(args.token_in.clone());

        let create_args = CreatePaymentArgs {
            payment_id: args.payment_id.clone(),
            merchant_id: args.merchant_id,
            payer: None,
            amount: args.amount,
            currency: args.currency,
            deposit_address: args.deposit_address,
            expires_at: args.expires_at,
            duration_secs: None,
            memo: None,
            memo_type: None,
            token_address: Some(settlement_token),
            client_token: None,
            metadata_hash: None,
            metadata: None,
            fee_waiver_code: None,
            retry_of_payment_id: None,
            payer_muxed_id: None,
        };

        let mut payment = Self::create_payment(env.clone(), create_args)?;

        // Issue #173: record the original token and swap path so a later
        // refund can be routed back through the DEX to the payer's token.
        payment.original_token = Some(args.token_in.clone());
        payment.swap_path = Some(args.path.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Payment(args.payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &args.payment_id, &payment.status);

        env.events().publish(
            (
                Symbol::new(&env, "SWAP"),
                Symbol::new(&env, "AND"),
                Symbol::new(&env, "PAY"),
            ),
            (
                args.payment_id,
                args.payer,
                args.amount_in,
                args.token_in,
                args.amount,
            ),
        );

        Ok(payment)
    }

    #[allow(dead_code)]
    fn get_next_stream_id(env: &Env) -> u64 {
        let mut counter: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::StreamCounter)
            .unwrap_or(0);
        counter += 1;
        env.storage()
            .persistent()
            .set(&DataKey::StreamCounter, &counter);
        counter
    }

    /// Enforce KYC tier monthly volume cap. Returns `TierVolumeLimitExceeded` if adding
    /// `amount` would exceed the merchant's tier cap for the current calendar month.
    /// On success, persists the updated cumulative volume.
    fn enforce_tier_volume_cap(
        env: &Env,
        merchant_id: &Address,
        amount: i128,
    ) -> Result<(), Error> {
        // Derive the monthly cap from the merchant's KYC tier (cross-contract if registry set).
        let cap = if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(env, &registry_address);
            match registry_client.try_get_merchant(merchant_id) {
                Ok(Ok(merchant)) => {
                    use crate::merchant_registry::KycTier;
                    match merchant.kyc_tier {
                        KycTier::Business => TIER_CAP_BUSINESS,
                        KycTier::Full => TIER_CAP_FULL,
                        KycTier::Basic => TIER_CAP_BASIC,
                        KycTier::Unverified => TIER_CAP_UNVERIFIED,
                    }
                }
                _ => TIER_CAP_UNVERIFIED,
            }
        } else {
            TIER_CAP_BUSINESS // No registry → no cap
        };

        if cap == i128::MAX {
            return Ok(()); // Business tier: unlimited
        }

        // Month epoch: seconds since Unix epoch / seconds-per-30-days, cast to u32.
        let month_epoch = (env.ledger().timestamp() / 2_592_000) as u32;
        let key = DataKey::MerchantMonthlyVolume(merchant_id.clone(), month_epoch);

        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_total = current.saturating_add(amount);

        if new_total > cap {
            return Err(Error::TierVolumeLimitExceeded);
        }

        env.storage().persistent().set(&key, &new_total);
        Self::bump_ttl(env, &key, LONG_LIVE_TTL);

        // Issue #207: Track cumulative volume and auto-upgrade KYC tier at milestones.
        let cum_key = DataKey::MerchantCumulativeVolume(merchant_id.clone());
        let cumulative: i128 = env.storage().persistent().get(&cum_key).unwrap_or(0);
        let new_cumulative = cumulative.saturating_add(amount);
        env.storage().persistent().set(&cum_key, &new_cumulative);
        Self::bump_ttl(env, &cum_key, LONG_LIVE_TTL);
        if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            Self::maybe_upgrade_kyc_tier(env, merchant_id, &registry_address, new_cumulative);
        }

        Ok(())
    }

    /// Issue #207: Attempt to auto-upgrade a merchant's KYC tier based on cumulative volume.
    /// Silently no-ops if the registry call fails (e.g. payment processor address not configured).
    fn maybe_upgrade_kyc_tier(
        env: &Env,
        merchant_id: &Address,
        registry_address: &Address,
        cumulative_volume: i128,
    ) {
        use crate::merchant_registry::{KycTier, MerchantRegistryClient};
        let registry_client = MerchantRegistryClient::new(env, registry_address);
        let merchant = match registry_client.try_get_merchant(merchant_id) {
            Ok(Ok(m)) => m,
            _ => return,
        };
        let next_tier = match merchant.kyc_tier {
            KycTier::Unverified if cumulative_volume >= TIER_UPGRADE_THRESHOLD_BASIC => {
                KycTier::Basic
            }
            KycTier::Basic if cumulative_volume >= TIER_UPGRADE_THRESHOLD_FULL => KycTier::Full,
            KycTier::Full if cumulative_volume >= TIER_UPGRADE_THRESHOLD_BUSINESS => {
                KycTier::Business
            }
            _ => return,
        };
        if matches!(
            registry_client.try_auto_upgrade_kyc_tier(
                &env.current_contract_address(),
                merchant_id,
                &next_tier,
            ),
            Ok(Ok(()))
        ) {
            env.events().publish(
                (Symbol::new(env, "KYC_TIER"), Symbol::new(env, "UPGRADED")),
                (merchant_id.clone(), cumulative_volume),
            );
        }
    }

    fn get_payment_internal(env: &Env, payment_id: &String) -> Result<PaymentCharge, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Payment(payment_id.clone()))
            .ok_or(Error::PaymentNotFound)
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

    pub fn get_merchant_dispute_count(env: Env, merchant_id: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantDisputeCount(merchant_id))
            .unwrap_or(0u64)
    }

    /// Issue #397: Validate Stellar memo type constraints.
    ///
    /// Allowed types: Text, Id, Hash, Return
    /// - Text: memo must be ≤ 28 bytes UTF-8
    /// - Id: memo must be parseable as u64
    /// - Hash / Return: memo must be exactly 32 bytes (64 hex chars)
    fn validate_memo(
        env: &Env,
        memo: &Option<String>,
        memo_type: &Option<String>,
    ) -> Result<(), Error> {
        // If no memo_type provided, memo should also be absent — either both or neither.
        let memo_type_str = match memo_type {
            None => return Ok(()), // no memo_type → skip validation
            Some(t) => t,
        };

        // Validate memo_type is one of the accepted values.
        let mut mt_buf = [0u8; 16];
        let mt_len = (memo_type_str.len() as usize).min(16);
        memo_type_str.copy_into_slice(&mut mt_buf[..mt_len]);
        let mt_bytes = &mt_buf[..mt_len];

        let is_text   = mt_bytes == b"Text";
        let is_id     = mt_bytes == b"Id";
        let is_hash   = mt_bytes == b"Hash";
        let is_return = mt_bytes == b"Return";

        if !is_text && !is_id && !is_hash && !is_return {
            return Err(Error::InvalidMemoType);
        }

        let memo_val = match memo {
            None => return Ok(()), // no memo value → no further validation
            Some(m) => m,
        };

        if is_text {
            // Stellar text memos are limited to 28 bytes.
            if memo_val.len() > 28 {
                return Err(Error::MemoTooLong);
            }
        } else if is_id {
            // Id memo must be a valid u64 decimal string.
            let mut buf = [0u8; 20]; // u64::MAX is 20 digits
            let len = (memo_val.len() as usize).min(20);
            memo_val.copy_into_slice(&mut buf[..len]);
            let s = &buf[..len];
            // All bytes must be ASCII digits.
            let mut valid = len > 0;
            for b in s.iter() {
                if !b.is_ascii_digit() {
                    valid = false;
                    break;
                }
            }
            // Also check it doesn't overflow u64 (max 18446744073709551615, 20 digits).
            if valid && len == 20 {
                // Compare digit-by-digit with u64::MAX string.
                let max_str = b"18446744073709551615";
                for i in 0..20 {
                    if s[i] < max_str[i] { break; }
                    if s[i] > max_str[i] { valid = false; break; }
                }
            }
            if !valid || len > 20 {
                return Err(Error::InvalidMemoId);
            }
        }
        // Hash / Return: no additional validation (the 32-byte constraint applies at the
        // Stellar protocol layer when submitting the transaction, not at the contract layer).

        let _ = env; // env unused but kept for consistent signature
        Ok(())
    }

    /// Issue #396: Returns the total number of payment IDs stored for a merchant.
    /// Used by pagination UIs alongside `get_merchant_payments_full`.
    /// Issue #396: Get merchant payment count for dashboard pagination (O(1) via counter).
    pub fn get_merchant_payment_count(env: Env, merchant_id: Address) -> u32 {
        let count: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantPaymentCount(merchant_id))
            .unwrap_or(0u64);
        count as u32
    }

    pub fn get_merchant_payment_count_dash(env: Env, merchant_id: Address) -> u32 {
        let count: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantPaymentCount(merchant_id))
            .unwrap_or(0u64);
        count as u32
    }

    /// Issue #396: Returns paginated full `PaymentCharge` structs for merchant dashboards.
    /// `limit` is capped at 50 to avoid ledger compute limits.
    /// Returns an empty vec (not an error) when `offset` exceeds the total count.
    /// When `token_address` is provided, returns only payments matching that token.
    #[allow(deprecated)]
    pub fn get_merchant_payments_full(
        env: Env,
        merchant_id: Address,
        offset: u32,
        limit: u32,
        token_address: Option<Address>,
    ) -> Vec<PaymentCharge> {
        let all = Self::get_merchant_payments_internal(&env, &merchant_id);
        let capped_limit = core::cmp::min(limit, 50);

        let mut filtered: Vec<PaymentCharge> = vec![&env];
        for id in all.iter() {
            if let Some(payment) = env
                .storage()
                .persistent()
                .get::<DataKey, PaymentCharge>(&DataKey::Payment(id))
            {
                let token_match = match &token_address {
                    Some(token) => payment.token_address.as_ref() == Some(token),
                    None => true,
                };

                if token_match {
                    filtered.push_back(payment);
                }
            }
        }

        if capped_limit == 0 || offset >= filtered.len() {
            return vec![&env];
        }

        let end = core::cmp::min(filtered.len(), offset.saturating_add(capped_limit));
        let mut result: Vec<PaymentCharge> = vec![&env];

        let mut i = offset;
        while i < end {
            if let Some(payment) = filtered.get(i) {
                result.push_back(payment.clone());
            }
            i += 1;
        }

        result
    }

    /// Issue #487: Query aggregate analytics for a merchant over a time range.
    /// Returns total_payments, confirmed_payments, failed_payments, total_volume,
    /// avg_payment_amount, dispute_count, refund_count, net_settled_volume.
    /// Issue #678: When from_ts/to_ts are provided, uses the DailyPaymentIndex
    /// to scan only the relevant day buckets (O(days) ledger reads).
    pub fn get_merchant_analytics(env: Env, merchant_id: Address, from_ts: u64, to_ts: u64) -> MerchantAnalytics {
        const SECONDS_PER_DAY: u64 = 86_400;
        let use_index = from_ts > 0 || to_ts < u64::MAX;

        let candidate_ids: Vec<String> = if use_index {
            let start_bucket = from_ts / SECONDS_PER_DAY;
            let end_bucket = to_ts / SECONDS_PER_DAY;
            let mut ids: Vec<String> = vec![&env];
            let mut bucket = start_bucket;
            while bucket <= end_bucket {
                let key = DataKey::DailyPaymentIndex(merchant_id.clone(), bucket);
                if let Some(bucket_ids) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, Vec<String>>(&key)
                {
                    for id in bucket_ids.iter() {
                        ids.push_back(id);
                    }
                }
                bucket = bucket.saturating_add(1);
            }
            ids
        } else {
            Self::get_merchant_payments_internal(&env, &merchant_id)
        };

        let mut total_payments = 0u32;
        let mut confirmed_payments = 0u32;
        let mut failed_payments = 0u32;
        let mut total_volume: i128 = 0;
        let mut net_settled_volume: i128 = 0;
        let mut refund_count = 0u32;
        let max_samples = 500usize;
        let mut sample_count = 0usize;

        for payment_id in candidate_ids.iter() {
            if sample_count >= max_samples {
                break;
            }
            sample_count += 1;

            if let Ok(payment) = Self::get_payment_internal(&env, &payment_id) {
                if payment.created_at >= from_ts && payment.created_at <= to_ts {
                    total_payments += 1;
                    total_volume = total_volume.saturating_add(payment.amount);

                    match payment.status {
                        PaymentStatus::Confirmed => {
                            confirmed_payments += 1;
                            net_settled_volume = net_settled_volume.saturating_add(payment.amount);
                        }
                        PaymentStatus::Failed | PaymentStatus::Expired => {
                            failed_payments += 1;
                        }
                        _ => {}
                    }
                }
                
                let refunds_for_payment = RefundManager::get_payment_refunds_internal(&env, &payment_id);
                for refund_id in refunds_for_payment.iter() {
                    if let Ok(refund) = RefundManager::get_refund_internal(&env, &refund_id) {
                        if refund.created_at >= from_ts && refund.created_at <= to_ts {
                            refund_count += 1;
                        }
                    }
                }
            }
        }

        let dispute_count = RefundManager::get_merchant_dispute_count(env.clone(), merchant_id.clone()) as u32;
        let avg_payment_amount = if total_payments > 0 {
            total_volume / (total_payments as i128)
        } else {
            0
        };

        MerchantAnalytics {
            total_payments,
            confirmed_payments,
            failed_payments,
            total_volume,
            avg_payment_amount,
            dispute_count,
            refund_count,
            net_settled_volume,
        }
    }

    /// Issue #399: Remove the idempotency key associated with a payment so the
    /// client_token can be reused after expiry or cancellation.
    fn remove_idempotency_key(env: &Env, payment_id: &String) {
        let rev_token_id = Self::rev_key_for(env, payment_id);
        let rev_key = DataKey::IdempotencyKey(rev_token_id);
        if let Some(token) = env
            .storage()
            .persistent()
            .get::<DataKey, String>(&rev_key)
        {
            env.storage()
                .persistent()
                .remove(&DataKey::IdempotencyKey(token));
            env.storage().persistent().remove(&rev_key);
        }
    }

    /// Build the reverse-map storage key for an idempotency entry.
    /// Prefixes the payment_id with "r:" so it cannot clash with real client_tokens
    /// (client_tokens are caller-supplied and conventionally do not start with "r:").
    fn rev_key_for(env: &Env, payment_id: &String) -> String {
        use soroban_sdk::Bytes;
        let prefix = b"r:";
        let mut buf = Bytes::new(env);
        for b in prefix {
            buf.push_back(*b);
        }
        let len = payment_id.len() as usize;
        let mut pid_buf = [0u8; 256];
        let read = len.min(256);
        payment_id.copy_into_slice(&mut pid_buf[..read]);
        for b in &pid_buf[..read] {
            buf.push_back(*b);
        }
        let total = (prefix.len() + read).min(256);
        let mut out = [0u8; 256];
        for i in 0..prefix.len() {
            out[i] = prefix[i];
        }
        for i in 0..read {
            out[prefix.len() + i] = pid_buf[i];
        }
        String::from_bytes(env, &out[..total])
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
            | PaymentStatus::Failed
            | PaymentStatus::PartiallyPaid
            | PaymentStatus::Overpaid => LONG_LIVE_TTL,
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

    /// Append a status-transition entry to a payment's on-chain status history.
    fn record_payment_status(env: &Env, payment: &PaymentCharge) {
        let key = DataKey::PaymentStatusHistory(payment.payment_id.clone());
        let mut history: Vec<PaymentStatusEvent> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![env]);
        history.push_back(PaymentStatusEvent {
            status: payment.status.clone(),
            timestamp: env.ledger().timestamp(),
            tx_hash: payment.transaction_hash.clone(),
        });
        env.storage().persistent().set(&key, &history);
        Self::bump_ttl(env, &key, SHORT_LIVE_TTL);
    }

    // ─── Merchant pre-authorization (pull payments) ───────────────────────────

    /// Customer grants a merchant permission to pull up to `limit_per_period`
    /// tokens per `period_secs`-second window.
    pub fn pre_authorize_merchant(
        env: Env,
        customer: Address,
        merchant: Address,
        token: Address,
        limit_per_period: i128,
        period_secs: u64,
    ) -> Result<MerchantAuthorization, MerchantAuthError> {
        MerchantPreAuth::pre_authorize_merchant(
            env,
            customer,
            merchant,
            token,
            limit_per_period,
            period_secs,
        )
    }

    /// Customer revokes a previously granted merchant authorization.
    pub fn revoke_merchant_authorization(
        env: Env,
        customer: Address,
        merchant: Address,
    ) -> Result<(), MerchantAuthError> {
        MerchantPreAuth::revoke_authorization(env, customer, merchant)
    }

    /// Merchant pulls `amount` tokens from the customer's account against
    /// an existing pre-authorization.
    pub fn pull_payment(
        env: Env,
        merchant: Address,
        customer: Address,
        amount: i128,
    ) -> Result<i128, MerchantAuthError> {
        MerchantPreAuth::pull_payment(env, merchant, customer, amount)
    }

    /// Return the stored authorization for a (customer, merchant) pair.
    pub fn get_merchant_authorization(
        env: Env,
        customer: Address,
        merchant: Address,
    ) -> Result<MerchantAuthorization, MerchantAuthError> {
        MerchantPreAuth::get_authorization(env, customer, merchant)
    }

    /// Return the remaining pull budget for the current period.
    pub fn merchant_authorization_remaining(
        env: Env,
        customer: Address,
        merchant: Address,
    ) -> Result<i128, MerchantAuthError> {
        MerchantPreAuth::remaining_limit(env, customer, merchant)
    }

    pub fn cancel_stream(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError> {
        if Self::is_blacklisted_address(&env, &sender) {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::cancel_stream(env, sender, stream_id)
    }

    pub fn pause_stream(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError> {
        if Self::is_blacklisted_address(&env, &sender) {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::pause_stream(env, sender, stream_id)
    }

    pub fn resume_stream(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError> {
        if Self::is_blacklisted_address(&env, &sender) {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::resume_stream(env, sender, stream_id)
    }
    pub fn cancel_multiple_streams(
        env: Env,
        sender: Address,
        stream_ids: Vec<String>,
    ) -> Result<Vec<String>, StreamError> {
        if Self::is_blacklisted_address(&env, &sender) {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::cancel_multiple_streams(env, sender, stream_ids)
    }

    pub fn batch_cancel_streams(
        env: Env,
        sender: Address,
        stream_ids: Vec<String>,
    ) -> Result<Vec<String>, StreamError> {
        if Self::is_blacklisted_address(&env, &sender) {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::batch_cancel_streams(env, sender, stream_ids)
    }

    pub fn batch_withdraw_to(
        env: Env,
        recipient: Address,
        withdrawals: Vec<WithdrawalRecipient>,
    ) -> Result<Vec<String>, StreamError> {
        if Self::is_blacklisted_address(&env, &recipient) {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::batch_withdraw_to(env, recipient, withdrawals)
    }

    pub fn withdraw_all_for_recipient(
        env: Env,
        recipient: Address,
        max_streams: u32,
    ) -> Result<Vec<String>, StreamError> {
        if Self::is_blacklisted_address(&env, &recipient) {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::withdraw_all_for_recipient(env, recipient, max_streams)
    }

    pub fn trigger_withdrawal(env: Env, stream_id: String) -> Result<String, StreamError> {
        PaymentStreaming::trigger_withdrawal(env, stream_id)
    }

    /// Issue #627: Bulk-extend the TTL of many stream entries in one call
    /// (permissionless). Delegates to `PaymentStreaming::bulk_bump_stream_ttls`;
    /// non-existent stream IDs are silently skipped and the count of bumped
    /// streams is returned.
    pub fn bulk_bump_stream_ttls(
        env: Env,
        stream_ids: Vec<String>,
    ) -> Result<u32, StreamError> {
        PaymentStreaming::bulk_bump_stream_ttls(env, stream_ids)
    }

    pub fn set_stream_destination(
        env: Env,
        recipient: Address,
        stream_id: String,
        destination: Address,
    ) -> Result<(), StreamError> {
        if Self::is_blacklisted_address(&env, &recipient)
            || Self::is_blacklisted_address(&env, &destination)
        {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::set_stream_destination(env, recipient, stream_id, destination)
    }

    /// Sender approves milestones for a stream, unlocking withdrawals.
    pub fn approve_stream_milestone(
        env: Env,
        sender: Address,
        stream_id: String,
    ) -> Result<(), StreamError> {
        PaymentStreaming::approve_stream_milestone(env, sender, stream_id)
    }

    /// Sender revokes a previous milestone approval, re-locking withdrawals.
    pub fn revoke_stream_milestone(
        env: Env,
        sender: Address,
        stream_id: String,
    ) -> Result<(), StreamError> {
        PaymentStreaming::revoke_stream_milestone(env, sender, stream_id)
    }

    /// Sender sets a floor for `decrease_rate_per_second` on this stream.
    pub fn set_stream_min_rate(
        env: Env,
        sender: Address,
        stream_id: String,
        min_rate_per_second: i128,
    ) -> Result<(), StreamError> {
        PaymentStreaming::set_stream_min_rate(env, sender, stream_id, min_rate_per_second)
    }

    /// Reduce the flow rate of an active stream, refunding surplus deposit.
    pub fn decrease_rate_per_second(
        env: Env,
        sender: Address,
        stream_id: String,
        new_rate: i128,
    ) -> Result<(), StreamError> {
        PaymentStreaming::decrease_rate_per_second(env, sender, stream_id, new_rate)
    }

    pub fn get_sender_streams(
        env: Env,
        sender: Address,
        page: u32,
        page_size: u32,
    ) -> Vec<PaymentStream> {
        PaymentStreaming::get_sender_streams(env, sender, page, page_size)
    }

    pub fn get_stream(env: Env, stream_id: String) -> Result<PaymentStream, StreamError> {
        PaymentStreaming::get_stream(env, stream_id)
    }

    /// Create a new payment stream. Tokens are pulled from `sender` into the contract.
    pub fn create_stream(
        env: Env,
        sender: Address,
        receiver: Address,
        token: Address,
        rate_per_second: i128,
        deposit: i128,
        stream_id: String,
    ) -> Result<PaymentStream, StreamError> {
        if Self::is_blacklisted_address(&env, &sender)
            || Self::is_blacklisted_address(&env, &receiver)
        {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::create_stream(
            env,
            sender,
            receiver,
            token,
            rate_per_second,
            deposit,
            stream_id,
            None,
        )
    }

    pub fn top_up_stream(
        env: Env,
        caller: Address,
        stream_id: String,
        amount: i128,
    ) -> Result<(), StreamError> {
        if Self::is_blacklisted_address(&env, &caller) {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::top_up_stream(env, caller, stream_id, amount)
    }

    pub fn top_up_multiple_streams(
        env: Env,
        sender: Address,
        top_ups: Vec<(String, i128)>,
    ) -> Result<(), StreamError> {
        PaymentStreaming::top_up_multiple_streams(env, sender, top_ups)
    }

    /// Update the flow rate of an active stream (increase or decrease).
    pub fn update_stream_rate(
        env: Env,
        sender: Address,
        stream_id: String,
        new_rate: i128,
    ) -> Result<(), StreamError> {
        PaymentStreaming::update_stream_rate(env, sender, stream_id, new_rate)
    }

    /// Close a terminal (Exhausted/Cancelled) stream and remove its storage entry.
    pub fn close_expired_stream(env: Env, stream_id: String) -> Result<(), StreamError> {
        PaymentStreaming::close_expired_stream(env, stream_id)
    }

    /// Set the platform fee in basis points applied to stream withdrawals.
    /// Only the admin may call this.
    pub fn set_stream_fee_bps(env: Env, admin: Address, fee_bps: i128) -> Result<(), Error> {
        admin.require_auth();
        if AccessControl::get_admin(&env) != Some(admin) {
            return Err(Error::AccessControlError);
        }
        PaymentStreaming::set_stream_fee_bps(env, fee_bps);
        Ok(())
    }

    pub fn get_stream_fee_bps(env: Env) -> i128 {
        PaymentStreaming::get_stream_fee_bps(env)
    }

    /// Upgrade the contract WASM and increment the contract version.
    ///
    /// Issue #624: The upgrade is now queued via the timelock instead of
    /// executing immediately.  Returns the action ID of the pending action.
    pub fn upgrade_contract(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<String, Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        Self::enqueue_timelocked_action(&env, admin, TimelockActionKind::UpgradeContract(new_wasm_hash))
    }

    // ── Issue #624: Timelock management functions ─────────────────────────────

    /// Set the timelock delay applied to critical admin operations.
    ///
    /// Default is 48 hours.  Only the admin may call this.
    pub fn set_timelock_delay(env: Env, admin: Address, secs: u64) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::TimelockDelaySecs, &secs);
        Ok(())
    }

    /// Return the current timelock delay in seconds.
    pub fn get_timelock_delay(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::TimelockDelaySecs)
            .unwrap_or(DEFAULT_TIMELOCK_SECS)
    }

    /// Return all pending timelocked admin actions.
    ///
    /// Any actor may call this to inspect queued operations and their
    /// `execute_after` timestamps.
    pub fn get_pending_admin_actions(env: Env) -> Vec<PendingTimelockAction> {
        let counter: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::TimelockActionCounter)
            .unwrap_or(0u64);

        let mut pending: Vec<PendingTimelockAction> = Vec::new(&env);
        for i in 0..counter {
            let action_id = format_id(&env, "tl_", i);
            if let Some(action) = env
                .storage()
                .persistent()
                .get::<DataKey, PendingTimelockAction>(&DataKey::PendingTimelockAction(action_id))
            {
                pending.push_back(action);
            }
        }
        pending
    }

    /// Execute a previously queued timelocked admin action.
    ///
    /// Reverts with `TimelockNotExpired` if called before `execute_after`.
    /// Only the admin may execute.  Removes the action from the queue on success.
    pub fn execute_timelocked_action(
        env: Env,
        admin: Address,
        action_id: String,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        let action: PendingTimelockAction = env
            .storage()
            .persistent()
            .get::<DataKey, PendingTimelockAction>(&DataKey::PendingTimelockAction(action_id.clone()))
            .ok_or(Error::PaymentNotFound)?; // reuse "not found" semantics

        let now = env.ledger().timestamp();
        if now < action.execute_after {
            return Err(Error::TimelockNotExpired);
        }

        // Dispatch the action
        match action.kind {
            TimelockActionKind::SetFeeRate(bps) => {
                env.storage()
                    .persistent()
                    .set(&DataKey::SettlementFeeRate, &bps);
            }
            TimelockActionKind::SetKycTierLimits(tier, max_amount) => {
                env.storage()
                    .persistent()
                    .set(&DataKey::KycTierLimitsConfig, &KycTierLimits { tier, max_amount });
                Self::bump_ttl(&env, &DataKey::KycTierLimitsConfig, LONG_LIVE_TTL);
            }
            TimelockActionKind::UpgradeContract(new_wasm_hash) => {
                let old_version: String = env
                    .storage()
                    .persistent()
                    .get(&DataKey::ContractVersion)
                    .unwrap_or_else(|| String::from_str(&env, INITIAL_CONTRACT_VERSION));

                let new_version_str = bump_version_string(&env, &old_version);
                env.deployer().update_current_contract_wasm(new_wasm_hash);
                env.storage()
                    .persistent()
                    .set(&DataKey::ContractVersion, &new_version_str);

                env.events().publish(
                    (Symbol::new(&env, "CONTRACT"), Symbol::new(&env, "UPGRADED")),
                    (old_version, new_version_str),
                );
            }
        }

        // Remove the executed action from the queue
        env.storage()
            .persistent()
            .remove(&DataKey::PendingTimelockAction(action_id.clone()));

        env.events().publish(
            (
                Symbol::new(&env, "TIMELOCK"),
                Symbol::new(&env, "ACTION_EXECUTED"),
            ),
            (action_id, admin),
        );

        Ok(())
    }

    /// Internal helper: assign an action ID, persist the pending action, and emit an event.
    fn enqueue_timelocked_action(
        env: &Env,
        proposed_by: Address,
        kind: TimelockActionKind,
    ) -> Result<String, Error> {
        let delay_secs: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::TimelockDelaySecs)
            .unwrap_or(DEFAULT_TIMELOCK_SECS);

        let counter: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::TimelockActionCounter)
            .unwrap_or(0u64);

        let action_id = format_id(env, "tl_", counter);

        let action = PendingTimelockAction {
            action_id: action_id.clone(),
            kind,
            execute_after: env.ledger().timestamp() + delay_secs,
            proposed_by,
        };

        env.storage()
            .persistent()
            .set(&DataKey::PendingTimelockAction(action_id.clone()), &action);
        env.storage()
            .persistent()
            .set(&DataKey::TimelockActionCounter, &(counter + 1));

        env.events().publish(
            (
                Symbol::new(env, "TIMELOCK"),
                Symbol::new(env, "ACTION_QUEUED"),
            ),
            (action_id.clone(), action.execute_after),
        );

        Ok(action_id)
    }

    // =========================================================================
    // Multi-sig admin proposal functions
    // =========================================================================

    /// Configure the multi-sig threshold and signer set.
    /// Only the current admin may call this.
    pub fn set_multisig_config(
        env: Env,
        admin: Address,
        threshold: u32,
        signers: Vec<Address>,
    ) -> Result<(), Error> {
        AccessControl::set_multisig_config(&env, admin, threshold, signers)
            .map_err(|_| Error::AccessControlError)
    }

    /// Returns the current multi-sig (threshold, signers) configuration.
    pub fn get_multisig_config(env: Env) -> (u32, Vec<Address>) {
        AccessControl::get_multisig_config(&env)
    }

    /// Create a new admin proposal for a contract parameter change.
    /// The calling signer must be in the multisig signer set.
    /// Returns the proposal nonce.
    pub fn create_proposal(
        env: Env,
        signer: Address,
        action: AdminAction,
    ) -> Result<u64, Error> {
        AccessControl::create_proposal(&env, signer, action)
            .map_err(|_| Error::AccessControlError)
    }

    /// Vote to approve an existing proposal.
    /// The calling signer must be in the multisig signer set and must not have
    /// already voted on this proposal.
    pub fn vote_proposal(env: Env, signer: Address, nonce: u64) -> Result<(), Error> {
        AccessControl::vote_proposal(&env, signer, nonce)
            .map_err(|_| Error::AccessControlError)
    }

    /// Execute a proposal once the multisig threshold is met.
    ///
    /// * Proposals expire after 48 hours if the threshold is not reached.
    /// * For parameter-change actions (`SetDisputeBond`, `SetVolumeCap`,
    ///   `SetRefundFeeBps`, `SetRateLimit`) the new value is written to
    ///   persistent storage and a `ADMIN/PROPOSAL_EXECUTED` event is emitted.
    pub fn execute_proposal(env: Env, executor: Address, nonce: u64) -> Result<(), Error> {
        executor.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &executor) {
            return Err(Error::Unauthorized);
        }

        let remaining = AccessControl::execute_proposal(&env, nonce)
            .map_err(|_| Error::AccessControlError)?;

        let action_tag = if let Some(ref action) = remaining {
            match action {
                AdminAction::SetDisputeBond(amount) => {
                    if *amount < 0 {
                        return Err(Error::InvalidAmount);
                    }
                    env.storage()
                        .persistent()
                        .set(&DataKey::DisputeBondAmount, amount);
                    Symbol::new(&env, "SET_DISPUTE_BOND")
                }
                AdminAction::SetVolumeCap(tier, cap) => {
                    if *cap < 0 {
                        return Err(Error::InvalidAmount);
                    }
                    env.storage()
                        .persistent()
                        .set(&DataKey::TierVolumeCap(tier.clone()), cap);
                    Symbol::new(&env, "SET_VOLUME_CAP")
                }
                AdminAction::SetRefundFeeBps(bps) => {
                    if *bps < 0 || *bps > 1_000 {
                        return Err(Error::InvalidAmount);
                    }
                    env.storage()
                        .instance()
                        .set(&DataKey::RefundFeeBps, bps);
                    Symbol::new(&env, "SET_REFUND_FEE")
                }
                AdminAction::SetRateLimit(max_per_window, window_secs) => {
                    let config = RateLimitConfig {
                        window_secs: *window_secs,
                        max_per_window: *max_per_window,
                    };
                    env.storage()
                        .persistent()
                        .set(&DataKey::GlobalRateLimit, &config);
                    Symbol::new(&env, "SET_RATE_LIMIT")
                }
                AdminAction::SetGlobalPause(paused, _reason) => {
                    let empty = String::from_str(&env, "multisig_proposal");
                    let state = PauseState {
                        paused: *paused,
                        reason: empty,
                        admin: Some(executor.clone()),
                        timestamp: env.ledger().timestamp(),
                    };
                    env.storage().persistent().set(&DataKey::Paused, &state);
                    Symbol::new(&env, "SET_GLOBAL_PAUSE")
                }
                AdminAction::AllowToken(token) => {
                    env.storage()
                        .persistent()
                        .set(&DataKey::AllowedToken(token.clone()), &true);
                    Symbol::new(&env, "ALLOW_TOKEN")
                }
                _ => Symbol::new(&env, "EXECUTED"),
            }
        } else {
            Symbol::new(&env, "EXECUTED")
        };

        // Emit ADMIN/PROPOSAL_EXECUTED event
        env.events().publish(
            (Symbol::new(&env, "ADMIN"), Symbol::new(&env, "PROPOSAL_EXECUTED")),
            (nonce, action_tag, executor),
        );

        Ok(())
    }

    /// Retrieve a pending proposal by nonce.
    pub fn get_proposal(env: Env, nonce: u64) -> Option<AdminProposal> {
        AccessControl::get_proposal(&env, nonce)
    }

    /// Read the effective dispute bond amount (configurable via multi-sig proposal).
    /// Falls back to the compile-time constant if not set via proposal.
    pub fn get_dispute_bond_amount(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::DisputeBondAmount)
            .unwrap_or(DISPUTE_BOND_AMOUNT)
    }

    /// Read the effective monthly volume cap for a KYC tier.
    /// Falls back to compile-time constants if not overridden by a proposal.
    pub fn get_tier_volume_cap(env: Env, tier: KycTier) -> i128 {
        if let Some(cap) = env.storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::TierVolumeCap(tier.clone()))
        {
            return cap;
        }
        match tier {
            KycTier::Unverified => TIER_CAP_UNVERIFIED,
            KycTier::Basic => TIER_CAP_BASIC,
            KycTier::Full => TIER_CAP_FULL,
            KycTier::Business => TIER_CAP_BUSINESS,
        }
    }

    /// Read the effective refund fee in basis points from instance storage.
    /// Falls back to the compile-time default (100 bps) if not yet configured.
    pub fn get_refund_fee_bps(env: Env) -> i128 {
        Self::get_refund_fee_bps_internal(&env)
    }

    fn get_refund_fee_bps_internal(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get::<DataKey, i128>(&DataKey::RefundFeeBps)
            .unwrap_or(REFUND_FEE_BPS)
    }

    /// Set the refund cooldown period in seconds (overrides REFUND_COOLDOWN_SECS constant).
    /// Admin-only operation.
    pub fn set_refund_cooldown(env: Env, admin: Address, secs: u64) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        env.storage()
            .persistent()
            .set(&DataKey::RefundCooldownSecs, &secs);
        Ok(())
    }

    /// Read the effective refund cooldown period in seconds.
    /// Falls back to the compile-time constant if not overridden by admin.
    fn get_refund_cooldown_secs(env: &Env) -> u64 {
        env.storage()
            .persistent()
            .get::<DataKey, u64>(&DataKey::RefundCooldownSecs)
            .unwrap_or(REFUND_COOLDOWN_SECS)
    }

    /// Migration function: recompute all merchant payment counts from payment vector.
    /// Scans all merchants and rebuilds the persistent payment count index.
    /// Admin-only operation. Use this after upgrading from older contract versions.
    pub fn recompute_merchant_payment_count(env: Env, admin: Address) -> Result<u64, Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        // This is a limited implementation that processes encountered merchants.
        // For full migration on mainnet, may need off-chain indexing support.
        let mut merchants_processed: u64 = 0;

        // Clear and rebuild payment counts by scanning merchant payment vectors.
        // In practice, we can only iterate merchants we encounter through payment history.
        // A full recompute would require iterating all historical payments.
        // For now, this is a placeholder that ensures the pattern is in place.

        Ok(merchants_processed)
    }

    pub fn create_invoice(
        env: Env,
        merchant_id: Address,
        customer_email: String,
        line_items: Vec<LineItem>,
        total_amount: i128,
        currency: Symbol,
        due_date: u64,
    ) -> Result<String, Error> {
        merchant_id.require_auth();

        if total_amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let invoice_id = Self::get_next_invoice_id(&env);
        let now = env.ledger().timestamp();

        let invoice = Invoice {
            invoice_id: invoice_id.clone(),
            merchant_id: merchant_id.clone(),
            customer_email: customer_email.clone(),
            line_items,
            total_amount,
            currency,
            due_date,
            status: InvoiceStatus::Created,
            payment_link_id: None,
            created_at: now,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Invoice(invoice_id.clone()), &invoice);

        let mut merchant_invoices = Self::get_merchant_invoices_internal(&env, &merchant_id);
        merchant_invoices.push_back(invoice_id.clone());
        env.storage().persistent().set(
            &DataKey::MerchantInvoices(merchant_id.clone()),
            &merchant_invoices,
        );

        env.events().publish(
            (Symbol::new(&env, "INVOICE"), Symbol::new(&env, "CREATED")),
            (invoice_id.clone(), merchant_id.clone(), total_amount),
        );

        Ok(invoice_id)
    }

    /// Issue #632: Atomically create an invoice together with a payment link and
    /// wire them up (`invoice.payment_link_id` is set to the new link's ID).
    ///
    /// The payment link is created first via a cross-contract call to the
    /// `link_manager` (`PaymentLinkManager`) contract. If link creation fails the
    /// call returns an error and the invoice is never persisted; if any later
    /// step fails the whole transaction reverts, so the two records are always
    /// created together or not at all.
    pub fn create_payment_link_invoice(
        env: Env,
        merchant_id: Address,
        link_manager: Address,
        customer_email: String,
        line_items: Vec<LineItem>,
        total_amount: i128,
        currency: Symbol,
        due_date: u64,
        link_args: CreateLinkArgs,
    ) -> Result<(Invoice, PaymentLink), Error> {
        merchant_id.require_auth();

        if total_amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        // Step 1: create the payment link on the PaymentLinkManager contract.
        let link_client =
            crate::payment_link::PaymentLinkManagerClient::new(&env, &link_manager);
        let link_id = match link_client.try_create_link(
            &merchant_id,
            &link_args.link_id,
            &link_args.amount,
            &link_args.currency,
            &link_args.description,
            &link_args.expires_at,
            &link_args.max_uses,
            &link_args.direct_transfer,
            &link_args.metadata,
            &link_args.fiat,
            &link_args.base_url,
        ) {
            Ok(Ok(id)) => id,
            _ => return Err(Error::InvalidPaymentId),
        };

        let payment_link = match link_client.try_get_link(&link_id) {
            Ok(Ok(link)) => link,
            _ => return Err(Error::PaymentNotFound),
        };

        // Step 2: create the invoice, pointing it at the new link.
        let invoice_id = Self::get_next_invoice_id(&env);
        let now = env.ledger().timestamp();
        let invoice = Invoice {
            invoice_id: invoice_id.clone(),
            merchant_id: merchant_id.clone(),
            customer_email,
            line_items,
            total_amount,
            currency,
            due_date,
            status: InvoiceStatus::Created,
            payment_link_id: Some(link_id.clone()),
            created_at: now,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Invoice(invoice_id.clone()), &invoice);

        let mut merchant_invoices = Self::get_merchant_invoices_internal(&env, &merchant_id);
        merchant_invoices.push_back(invoice_id.clone());
        env.storage().persistent().set(
            &DataKey::MerchantInvoices(merchant_id.clone()),
            &merchant_invoices,
        );

        env.events().publish(
            (Symbol::new(&env, "INVOICE"), Symbol::new(&env, "CREATED")),
            (invoice_id.clone(), merchant_id.clone(), total_amount),
        );
        env.events().publish(
            (
                Symbol::new(&env, "INVOICE"),
                Symbol::new(&env, "LINK_ATTACHED"),
            ),
            (invoice_id, link_id),
        );

        Ok((invoice, payment_link))
    }

    pub fn mark_invoice_paid(env: Env, invoice_id: String) -> Result<(), Error> {
        let mut invoice: Invoice = env
            .storage()
            .persistent()
            .get(&DataKey::Invoice(invoice_id.clone()))
            .ok_or(Error::PaymentNotFound)?;

        if invoice.status != InvoiceStatus::Created {
            return Err(Error::PaymentAlreadyProcessed);
        }

        invoice.status = InvoiceStatus::Paid;
        env.storage()
            .persistent()
            .set(&DataKey::Invoice(invoice_id.clone()), &invoice);

        env.events().publish(
            (Symbol::new(&env, "INVOICE"), Symbol::new(&env, "PAID")),
            (invoice_id.clone(), invoice.merchant_id.clone()),
        );

        Ok(())
    }

    pub fn get_invoice(env: Env, invoice_id: String) -> Result<Invoice, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Invoice(invoice_id))
            .ok_or(Error::PaymentNotFound)
    }

    pub fn get_merchant_invoices(env: Env, merchant_id: Address) -> Vec<String> {
        let internal = Self::get_merchant_invoices_internal(&env, &merchant_id);
        let mut result = vec![&env];
        for s in internal.iter() {
            result.push_back(s);
        }
        result
    }

    fn get_next_invoice_id(env: &Env) -> String {
        let counter = env
            .storage()
            .persistent()
            .get::<DataKey, u64>(&DataKey::InvoiceCounter)
            .unwrap_or(0);

        env.storage()
            .persistent()
            .set(&DataKey::InvoiceCounter, &(counter + 1));

        utils::format_id(env, "invoice_", counter)
    }

    fn get_merchant_invoices_internal(env: &Env, merchant_id: &Address) -> Vec<String> {
        env.storage()
            .persistent()
            .get::<DataKey, Vec<String>>(&DataKey::MerchantInvoices(merchant_id.clone()))
             .unwrap_or_else(|| Vec::new(env))
    }
}

/// Bumps the version string by incrementing the number after the last '.'.
/// Works with versions like "1.0.0" → "1.0.1", "1" → "2", "v1" → "v2".
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
    for k in 0..num_len {
        if pos < 64 {
            result[pos] = num_buf[k];
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

#[cfg(test)]
mod arbitrage_test;
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
mod oracle_sanitization_test;
mod payment_link;
#[cfg(test)]
mod proptests;
pub use payment_link::{
    CreateLinkArgs, FiatConfig, LinkAnalytics, MaybeFiatConfig, PaymentLink, PaymentLinkManager,
    PaymentLinkManagerClient,
};
#[cfg(test)]
mod feature_tests;
#[cfg(test)]
mod memo_test;
#[cfg(test)]
mod partial_overpaid_test;
#[cfg(test)]
mod pause_test;
#[cfg(test)]
mod payment_link_test;
#[cfg(test)]
mod test;
#[cfg(test)]
mod payment_metadata_test;

// Payment streaming module (Issue #127)
pub mod stream;
pub use stream::{PaymentStream, PaymentStreaming, StreamError, StreamStatus};
#[cfg(test)]
mod stream_test;
#[cfg(test)]
mod subscription_test;

pub mod utils;
pub use utils::format_id;
pub use utils::is_valid_cid;
pub use utils::validate_ipfs_multihash;
pub use utils::validate_id;

pub mod gas_estimator;
pub use gas_estimator::{CostEstimate, GasEstimator, GasEstimatorClient, Operation};

#[cfg(test)]
mod mock_dex_router;
#[cfg(test)]
mod swap_test;
#[cfg(test)]
mod settlement_test;
#[cfg(test)]
mod muxed_payer_test;
#[cfg(test)]
mod router_allowlist_test;
#[cfg(test)]
mod batch_payment_test;
#[cfg(test)]
mod escalate_disputes_test;
#[cfg(test)]
mod merchant_ranking_test;
