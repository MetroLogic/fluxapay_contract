//! Contract constants for FluxaPay.

pub const PAYMENT_TOLERANCE: i128 = 1;
pub const SHORT_LIVE_TTL: u32 = 120_960; // ~1 week at 5s/ledger
pub const LONG_LIVE_TTL: u32 = 18_921_600; // ~3 years at 5s/ledger
pub const TTL_BUMP_THRESHOLD_DIVISOR: u32 = 5;
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
/// Default threshold separating small and large disputes: 100 USDC (7 decimals).
pub const DEFAULT_DISPUTE_DEADLINE_THRESHOLD_AMOUNT: i128 = 1_000_000_000;
pub const SMALL_DISPUTE_DEADLINE_SECS: u64 = 3 * 24 * 60 * 60;
pub const LARGE_DISPUTE_DEADLINE_SECS: u64 = 7 * 24 * 60 * 60;

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

// Issue #207: Cumulative volume thresholds for automatic KYC tier upgrades (in USDC stroops)
pub const TIER_UPGRADE_THRESHOLD_BASIC: i128 = TIER_CAP_UNVERIFIED; // $500 cumulative → Basic
pub const TIER_UPGRADE_THRESHOLD_FULL: i128 = TIER_CAP_BASIC; // $10,000 cumulative → Full
pub const TIER_UPGRADE_THRESHOLD_BUSINESS: i128 = TIER_CAP_FULL; // $100,000 cumulative → Business

/// Maximum number of payment retries before a subscription is cancelled.
pub const SUBSCRIPTION_MAX_RETRIES: u32 = 3;
/// Spacing between retry attempts in seconds (2 days).
pub const SUBSCRIPTION_RETRY_INTERVAL_SECS: u64 = 2 * 24 * 60 * 60;

// Issue #625: Maximum lengths for user-supplied string fields to prevent ledger bloat.
pub const MAX_REASON_LEN: usize = 256;
pub const MAX_EVIDENCE_LEN: usize = 512;
pub const MAX_NOTES_LEN: usize = 512;
pub(crate) const ZERO_CONTRACT_STRKEY: &str =
    "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM";

/// Hard cap for dispute batch size.
pub const MAX_DISPUTE_BATCH: u32 = 20;

/// Number of `ARBITRATOR`-role votes (either direction) required to
/// auto-execute a dispute resolution via [`FluxaPayContract::vote_dispute`].
pub const ARBITRATOR_VOTING_THRESHOLD: u32 = 3;

/// Maximum number of withdrawal records retained in `TreasuryWithdrawalHistory`.
pub const TREASURY_WITHDRAWAL_HISTORY_CAP: u32 = 100;

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
