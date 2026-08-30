//! Contract types and struct definitions for FluxaPay.

use crate::merchant_registry::KycTier;
use soroban_sdk::{
    contracterror, contracttype, Env, Address, BytesN, Map, String, Symbol, Vec,
};

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
    pub fee_waiver_code: Option<String>,
    /// Issue #482: Payment ID of the original payment if this is a retry; None if original or not retried.
    pub retry_of_payment_id: Option<String>,
    /// Issue #484: Muxed account ID from payer M-address; None for G-addresses or on-chain payments.
    pub payer_muxed_id: Option<u64>,
    /// Issue #668: ID of the payment link that created this payment via `use_link`
    pub payment_link_id: Option<String>,
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

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefundIdempotencyRecord {
    pub refund_id: String,
    pub payment_id: String,
    pub amount: i128,
    pub reason: String,
}

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

pub struct ReentrancyGuard<'a> {
    pub env: &'a Env,
}

impl<'a> Drop for ReentrancyGuard<'a> {
    fn drop(&mut self) {
        self.env
            .storage()
            .persistent()
            .set(&crate::data_keys::DataKey::ReentrancyLock, &false);
    }
}

/// Per-refund reentrancy lock cleared on drop (checks-effects-interactions).
pub struct RefundLockGuard<'a> {
    pub env: &'a Env,
    pub refund_id: String,
}

impl<'a> Drop for RefundLockGuard<'a> {
    fn drop(&mut self) {
        self.env
            .storage()
            .persistent()
            .remove(&crate::data_keys::DataKey::RefundLock(self.refund_id.clone()));
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
