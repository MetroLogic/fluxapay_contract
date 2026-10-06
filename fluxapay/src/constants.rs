//! Contract constants for FluxaPay.

pub const PAYMENT_TOLERANCE: i128 = 1;
pub const SHORT_LIVE_TTL: u32 = 120_960; // ~1 week at 5s/ledger
pub const LONG_LIVE_TTL: u32 = 18_921_600; // ~3 years at 5s/ledger
pub const TTL_BUMP_THRESHOLD_DIVISOR: u32 = 5;
/// Issue #768: Minimum persistent storage TTL for completed payment records (~120 days at 5s/ledger).
pub const MIN_PERSISTENT_TTL: u32 = (120 * 24 * 60 * 60) / 5; // 2_073_600 ledgers
/// Issue #768: Minimum TTL threshold before extending persistent payment records (~30 days at 5s/ledger).
pub const MIN_BUMP_TTL: u32 = (30 * 24 * 60 * 60) / 5; // 518_400 ledgers
pub const CREATE_PAYMENT_WINDOW_SECS: u64 = 60;
pub const CREATE_PAYMENT_MAX_PER_WINDOW: u32 = 30;
pub const DEFAULT_PAYMENT_DURATION_SECS: u64 = 3_600;
pub const REFUND_FEE_BPS: i128 = 100;
/// Cooldown period after payment confirmation before refunds can be requested (5 minutes in seconds).
pub const REFUND_COOLDOWN_SECS: u64 = 300;
/// Default refund request expiry period (30 days in seconds).
pub const REFUND_EXPIRY_SECS: u64 = 30 * 24 * 60 * 60;
/// Issue #638: TTL (in ledgers, ~5s each) for a stored refund idempotency key —
/// 30 days, matching the payment `client_token` retention window.
pub const REFUND_IDEMPOTENCY_TTL_LEDGERS: u32 = (30 * 24 * 60 * 60) / 5;
/// Issue #480: Minimum time between daily settlements (24 hours in seconds).
pub const SETTLEMENT_DAILY_INTERVAL_SECS: u64 = 86_400;
/// Issue #480: Minimum time between weekly settlements (7 days in seconds).
pub const SETTLEMENT_WEEKLY_INTERVAL_SECS: u64 = 604_800;
/// Issue #480: Minimum pending balance required to trigger a settlement.
pub const SETTLEMENT_MIN_AMOUNT: i128 = 1_000_000; // 0.1 USDC (7 decimals)
/// Fixed dispute bond in the contract's stablecoin denomination.
pub const DISPUTE_BOND_AMOUNT: i128 = 100_000;
/// Issue #820: Absolute floor for the dispute bond, in the contract's
/// stablecoin denomination (1 USDC, 7 decimals). Configurable by admin via
/// `set_dispute_bond_params`.
pub const ABSOLUTE_MIN_BOND: i128 = 10_000_000;
/// Issue #820: Default proportional minimum bond, in basis points of the
/// disputed payment amount (200 bps = 2%). Configurable by admin via
/// `set_dispute_bond_params`.
pub const MIN_BOND_BPS: i128 = 200;
/// Issue #820: Basis-point denominator used when computing the proportional
/// minimum dispute bond.
pub const BPS_DENOMINATOR: i128 = 10_000;
/// Default threshold separating small and large disputes: 100 USDC (7 decimals).
pub const DEFAULT_DISPUTE_DEADLINE_THRESHOLD_AMOUNT: i128 = 1_000_000_000;
pub const SMALL_DISPUTE_DEADLINE_SECS: u64 = 3 * 24 * 60 * 60;
pub const LARGE_DISPUTE_DEADLINE_SECS: u64 = 7 * 24 * 60 * 60;

/// Issue #797: Window (in ledger seconds) a merchant has to respond to a
/// pending dispute before it can be auto-resolved in the customer's favour.
pub const MERCHANT_RESPONSE_WINDOW: u64 = 72 * 60 * 60;

// Issue #167: Tiered refund fees based on merchant KYC tier
pub const REFUND_FEE_BPS_BASIC: i128 = 100; // 1.0% for Basic tier
pub const REFUND_FEE_BPS_FULL: i128 = 80; // 0.8% for Full tier
pub const REFUND_FEE_BPS_BUSINESS: i128 = 50; // 0.5% for Business tier

/// Default window (Issue #170) during which a pending refund may be processed,
/// measured from `Refund::created_at`. Configurable via `set_refund_expiry`.
pub const DEFAULT_REFUND_EXPIRY_SECS: u64 = 30 * 24 * 60 * 60;
// Issue #63: Monthly processing volume caps per KYC tier (in USDC stroops, 7 decimals)
// Unverified: $500, Basic: $10,000, Full: $100,000, Business: unlimited (i128::MAX)
pub const TIER_CAP_UNVERIFIED: i128 = 5_000_000_000; // $500
pub const TIER_CAP_BASIC: i128 = 100_000_000_000; // $10,000
pub const TIER_CAP_FULL: i128 = 1_000_000_000_000; // $100,000
pub const TIER_CAP_BUSINESS: i128 = i128::MAX; // unlimited

// Issue #777: KYC tier per-payment and monthly volume limits (in USDC stroops, 7 decimals, ADR-0003)
pub const TIER_0_MAX_SINGLE: i128 = 1_000_000_000; // 100 USDC
pub const TIER_0_MAX_MONTHLY: i128 = 5_000_000_000; // 500 USDC
pub const TIER_1_MAX_SINGLE: i128 = 100_000_000_000; // 10,000 USDC
pub const TIER_1_MAX_MONTHLY: i128 = 500_000_000_000; // 50,000 USDC
pub const TIER_2_MAX_SINGLE: i128 = i128::MAX; // Unlimited
pub const TIER_2_MAX_MONTHLY: i128 = i128::MAX; // Unlimited

// Issue #207: Cumulative volume thresholds for automatic KYC tier upgrades (in USDC stroops)
pub const TIER_UPGRADE_THRESHOLD_BASIC: i128 = TIER_CAP_UNVERIFIED; // $500 cumulative → Basic
pub const TIER_UPGRADE_THRESHOLD_FULL: i128 = TIER_CAP_BASIC; // $10,000 cumulative → Full
pub const TIER_UPGRADE_THRESHOLD_BUSINESS: i128 = TIER_CAP_FULL; // $100,000 cumulative → Business

/// Maximum number of payment retries before a subscription is cancelled.
pub const SUBSCRIPTION_MAX_RETRIES: u32 = 3;
/// Spacing between retry attempts in seconds (2 days).
pub const SUBSCRIPTION_RETRY_INTERVAL_SECS: u64 = 2 * 24 * 60 * 60;
/// Issue #900: Maximum number of dunning retries before a subscription is
/// automatically terminated due to repeated payment failure.
pub const MAX_DUNNING_RETRIES: u32 = 3;
/// Issue #900: Minimum backoff window (in seconds) between dunning retry
/// attempts. Retries use exponential backoff starting from this base.
pub const DUNNING_RETRY_BACKOFF_SECS: u64 = 24 * 60 * 60;
/// Issue #900: Default recurring billing interval (30 days in seconds) used
/// when a plan does not specify an explicit `interval_secs`.
pub const DEFAULT_SUBSCRIPTION_INTERVAL_SECS: u64 = 30 * 24 * 60 * 60;
/// Issue #836: Maximum allowed free-trial length for a subscription plan (days).
pub const MAX_TRIAL_DAYS: u32 = 90;
/// Issue #836: Seconds in one trial day (ledger time).
pub const TRIAL_DAY_SECS: u64 = 86_400;

/// Issue #806: Default cancellation grace period after a successful subscription
/// charge (24 hours in seconds). Subscribers may cancel within this window and
/// receive a full refund for the most recent charge.
pub const GRACE_PERIOD_SECS: u64 = 86_400;
/// Issue #806: Maximum configurable grace period per plan (3 days in seconds).
pub const MAX_GRACE_PERIOD_SECS: u64 = 259_200;

// Issue #625: Maximum lengths for user-supplied string fields to prevent ledger bloat.
pub const MAX_REASON_LEN: usize = 256;
pub const MAX_EVIDENCE_LEN: usize = 512;
pub const MAX_NOTES_LEN: usize = 512;
/// Issue #801: Maximum length of a merchant-supplied payment memo, matching
/// the Stellar memo text limit (140 bytes).
pub const MAX_MEMO_LEN: usize = 140;
pub(crate) const ZERO_CONTRACT_STRKEY: &str =
    "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM";

/// Hard cap for dispute batch size.
pub const MAX_DISPUTE_BATCH: u32 = 20;

/// Number of `ARBITRATOR`-role votes (either direction) required to
/// auto-execute a dispute resolution via [`FluxaPayContract::vote_dispute`].
pub const ARBITRATOR_VOTING_THRESHOLD: u32 = 3;

/// Issue #843: Stake amount that yields one unit of arbitration vote weight
/// (100 USDC at 7 decimal places). `vote_weight = stake_amount / VOTE_WEIGHT_UNIT`.
pub const VOTE_WEIGHT_UNIT: i128 = 1_000_000_000;
/// Issue #843: Minimum stake required to participate in weighted arbitration
/// voting (must yield at least 1 weight unit).
pub const MIN_ARBITRATOR_STAKE: i128 = VOTE_WEIGHT_UNIT;
/// Issue #843: Default weighted quorum in basis points of total registered
/// stake weight (5100 = 51%). Admin-configurable via `set_weighted_quorum_bps`.
pub const WEIGHTED_QUORUM_BPS: u32 = 5_100;

/// Issue #846: Default number of ledgers that must elapse between
/// `propose_upgrade` and `execute_upgrade` (~48h at ~5s/ledger).
pub const UPGRADE_TIMELOCK_LEDGERS: u32 = 34_560;

/// Maximum number of withdrawal records retained in `TreasuryWithdrawalHistory`.
pub const TREASURY_WITHDRAWAL_HISTORY_CAP: u32 = 100;

/// Treasury multisig default timelock: 24 hours.
pub const DEFAULT_TREASURY_TIMELOCK_SECS: u64 = 24 * 60 * 60;
/// Treasury multisig maximum timelock: 48 hours.
pub const MAX_TREASURY_TIMELOCK_SECS: u64 = 48 * 60 * 60;
/// Treasury multisig minimum timelock: 1 hour.
pub const MIN_TREASURY_TIMELOCK_SECS: u64 = 60 * 60;

/// Default required approvals for treasury multisig (3 of 5).
pub const DEFAULT_TREASURY_REQUIRED_APPROVALS: u32 = 3;
/// Default maximum signers for treasury multisig.
pub const MAX_TREASURY_SIGNERS: u32 = 10;

/// Issue #628: Maximum number of entries `get_top_merchants` will return,
/// keeping the ledger-read budget bounded regardless of the caller's `limit`.
pub const TOP_MERCHANTS_MAX_LIMIT: u32 = 100;

/// Maximum number of fee-collection records retained in `FeeCollectionHistory`.
/// Kept larger than `TREASURY_WITHDRAWAL_HISTORY_CAP` since fee reporting is
/// meant to cover longer look-back windows (e.g. a full reporting month).
pub const FEE_COLLECTION_HISTORY_CAP: u32 = 5_000;

/// Default: max 5 open disputes per payer.
pub const DEFAULT_DISPUTE_PER_PAYER_OPEN: u32 = 5;
/// Default: max 100 dispute creations per hour globally.
pub const DEFAULT_DISPUTE_GLOBAL_PER_HOUR: u32 = 100;
/// Global dispute creation window length (1 hour).
pub const DISPUTE_GLOBAL_WINDOW_SECS: u64 = 3600;

/// Default initial contract version string.
pub const INITIAL_CONTRACT_VERSION: &str = "1.0.0";

/// Default timelock delay for critical admin operations: 48 hours.
pub const DEFAULT_TIMELOCK_SECS: u64 = 48 * 60 * 60;

/// Issue #822: Default maximum lifetime for delegated merchant API keys
/// (90 days in seconds). Used when the admin has not configured an explicit
/// `max_key_lifetime_secs` via `set_max_key_lifetime_secs`.
pub const DEFAULT_MAX_API_KEY_LIFETIME_SECS: u64 = 90 * 24 * 60 * 60;

/// Issue #823: Maximum number of tags a payment may carry.
pub const MAX_PAYMENT_TAGS: u32 = 10;
/// Issue #823: Maximum length (in bytes) of a single payment tag.
pub const MAX_TAG_LEN: u32 = 32;

/// Issue #825: Maximum number of milestones a payment stream may define.
pub const MAX_STREAM_MILESTONES: u32 = 20;
/// Issue #825: Maximum length (in bytes) of a milestone evidence hash.
pub const MAX_MILESTONE_EVIDENCE_LEN: u32 = 64;
// Issue #804: On-chain fee split — configurable revenue share between the
// FluxaPay platform and merchant affiliates/resellers.
/// Maximum number of affiliate `FeeRecipient` entries a merchant may register.
pub const MAX_FEE_RECIPIENTS: u32 = 3;
/// Maximum total affiliate share (in basis points) across all recipients.
/// 5000 bps = 50%; the remaining share always goes to the platform treasury.
pub const MAX_AFFILIATE_BPS: u32 = 5_000;
