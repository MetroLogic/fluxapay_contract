//! Data keys for persistent storage in FluxaPay.

use crate::merchant_registry::KycTier;
use soroban_sdk::{contracttype, Address, BytesN, String, Symbol};

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
