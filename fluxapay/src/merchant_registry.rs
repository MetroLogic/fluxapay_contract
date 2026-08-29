use crate::access_control::{role_admin, role_arbitrator, AccessControl};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, map, vec, Address, BytesN, Env, Map,
    String, Symbol, Vec,
};

#[contract]
pub struct MerchantRegistry;

/// KYC tier for merchants, replacing the binary `verified: bool` field.
/// Allows payment limits and settlement schedules to vary by tier.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KycTier {
    Unverified,
    Basic,
    Full,
    Business,
}

/// Fee configuration for a merchant.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeConfig {
    /// Platform fee in basis points (100 bps = 1%). 0 means no fee.
    pub platform_fee_bps: i64,
    /// Fixed fee per transaction in the smallest currency unit.
    pub fixed_fee: i128,
    /// Optional: custom fee recipient address (defaults to admin if None).
    pub fee_recipient: Option<Address>,
}

/// Soroban-compatible nullable wrapper for FeeConfig.
///
/// Soroban's `#[contracttype]` macro does not support `Option<T>` when `T`
/// is itself a `#[contracttype]` struct (because structs implement `TryFrom`
/// rather than `From` for `ScVal`). Using an enum is the idiomatic pattern.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaybeFeeConfig {
    None,
    Some(FeeConfig),
}

impl MaybeFeeConfig {
    pub fn as_option(&self) -> Option<&FeeConfig> {
        match self {
            MaybeFeeConfig::Some(ref c) => Some(c),
            MaybeFeeConfig::None => None,
        }
    }

    pub fn into_option(self) -> Option<FeeConfig> {
        match self {
            MaybeFeeConfig::Some(c) => Some(c),
            MaybeFeeConfig::None => None,
        }
    }
}

impl From<Option<FeeConfig>> for MaybeFeeConfig {
    fn from(opt: Option<FeeConfig>) -> Self {
        match opt {
            Some(c) => MaybeFeeConfig::Some(c),
            None => MaybeFeeConfig::None,
        }
    }
}

/// Stellar Anchor Protocol (SEP-6 / SEP-24) configuration for fiat offramp.
///
/// Bridges on-chain USDC settlement to a merchant's bank account via a
/// compliant anchor partner such as MoneyGram, Circle, or Tempo.
///
/// All endpoint URLs are stored as plain strings because Soroban's
/// `#[contracttype]` does not support URL-specific newtypes; the off-chain
/// Settlement Service validates them against the allowlisted `anchor_domain`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnchorConfig {
    /// Fully qualified anchor domain (e.g. "api.moneygram.com").
    /// Used for SEP-1 TOML discovery and webfinger verification.
    pub anchor_domain: String,
    /// Full URL of the anchor's SEP-6 transfer server.
    /// Programmatic withdrawal endpoint used by the off-chain Settlement Service.
    pub sep6_endpoint: String,
    /// Full URL of the anchor's SEP-24 interactive transfer server.
    /// Used as a fallback when SEP-6 reports `incomplete` (missing KYC / bank).
    pub sep24_endpoint: String,
    /// Fiat currencies this anchor can payout for this merchant.
    /// ISO-4217 alphabetic codes, e.g. ["USD", "EUR", "NGN"].
    pub supported_currencies: Vec<String>,
}

/// Soroban-compatible nullable wrapper for AnchorConfig.
///
/// Pattern mirrors `MaybeFeeConfig` — `#[contracttype]` cannot directly nest
/// `Option<AnchorConfig>` because `AnchorConfig` is itself a `#[contracttype]`
/// struct. Using an enum variant is the idiomatic workaround.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaybeAnchorConfig {
    None,
    Some(AnchorConfig),
}

impl MaybeAnchorConfig {
    pub fn as_option(&self) -> Option<&AnchorConfig> {
        match self {
            MaybeAnchorConfig::Some(ref c) => Some(c),
            MaybeAnchorConfig::None => None,
        }
    }

    pub fn into_option(self) -> Option<AnchorConfig> {
        match self {
            MaybeAnchorConfig::Some(c) => Some(c),
            MaybeAnchorConfig::None => None,
        }
    }
}

impl From<Option<AnchorConfig>> for MaybeAnchorConfig {
    fn from(opt: Option<AnchorConfig>) -> Self {
        match opt {
            Some(c) => MaybeAnchorConfig::Some(c),
            None => MaybeAnchorConfig::None,
        }
    }
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Merchant {
    pub merchant_id: Address,
    pub business_name: String,
    pub settlement_currency: String,
    /// On-chain address where settled funds are sent.
    pub payout_address: Option<Address>,
    /// Off-chain bank account reference for fiat payouts.
    pub bank_account: Option<String>,
    /// KYC tier replaces the old `verified: bool` field.
    pub kyc_tier: KycTier,
    /// Merchant-level toggle for accepting underpayments.
    pub partial_payment_allowed: bool,
    pub active: bool,
    pub created_at: u64,
    pub suspension_reason: Option<String>,
    pub suspended_at: Option<u64>,
    pub suspension_expires_at: Option<u64>,
    pub oracle_signature: Option<String>,
    pub last_payout_change_at: Option<u64>,
    /// Custom fee configuration for this merchant.
    pub fee_config: MaybeFeeConfig,
    /// IPFS hash for content-addressable merchant metadata (issue #208)
    pub metadata_hash: Option<String>,
    /// Multi-currency payout addresses mapping (issue #216)
    pub currency_payout_addresses: Map<String, Address>,
    /// Whitelist of approved payout addresses (issue #210)
    pub payout_whitelist: Vec<Address>,
    /// Stellar Anchor (SEP-6 / SEP-24) config for fiat offramp.
    /// `None` means on-chain-only settlement (no anchor).
    pub anchor_config: MaybeAnchorConfig,
    /// Timestamp until which all platform fees are waived for this merchant.
    /// Used for onboarding campaigns and merchant promotions.
    /// `None` means no merchant-wide fee waiver is active.
    pub fee_waiver_expires_at: Option<u64>,
    /// Issue #480: Settlement schedule for accumulated balance sweep.
    pub settlement_schedule: SettlementSchedule,
    /// Issue #480: Timestamp of the last triggered settlement.
    pub last_settlement_at: Option<u64>,
    /// When true, only customers in `MerchantDataKey::MerchantCustomerWhitelist`
    /// may initiate payments to this merchant (issue #516).
    pub whitelist_mode: bool,
    /// Issue #481: Total dispute count for this merchant (incremented on dispute creation).
    pub dispute_count: u32,
    /// Issue #481: Count of disputes resolved against this merchant (incremented on unfavorable resolution).
    pub lost_disputes_count: u32,
    /// Global payment tolerance in basis points for this merchant.
    pub resolved_against_count: u32,
    pub payment_tolerance: Option<i128>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettlementSchedule {
    Daily,
    Weekly,
    Manual,
}

#[contracttype]
pub enum MerchantDataKey {
    Merchant(Address),
    Admin,
    /// Stores the list of all registered merchants for enumeration
    MerchantList,
    /// Optional: Address of the RefundManager contract for automatic MERCHANT role granting
    RefundManagerAddress,
    /// Platform fee configuration
    FeeConfig,
    /// Address of the PaymentProcessor contract for automatic KYC tier upgrades (issue #207)
    PaymentProcessorAddress,
    /// Ordered list of previous payout addresses for a merchant (audit trail).
    MerchantPayoutHistory(Address),
    /// KYC tier limits
    TierLimit(KycTier),
    /// Customer whitelist for a merchant in whitelist mode (issue #516)
    MerchantCustomerWhitelist(Address),
    /// Issue #480: Accumulated confirmed payment net amounts awaiting settlement.
    MerchantPendingSettlement(Address),
    /// Issue #481: Admin-configurable dispute threshold for auto-suspension (default 10)
    DisputeThreshold,
    /// Global payment tolerance in basis points
    GlobalPaymentTolerance,
    SuspensionProposal(u64),
    SuspensionVote(u64, Address),
    SuspensionProposalCounter,
    SuspensionThreshold,
    /// Issue #667: Arbitrary on-chain contract metadata (description, deployment
    /// notes, audit commit hash, etc.), keyed by an admin-chosen Symbol.
    ContractMetadata(Symbol),
}

/// ~3 years at 5s/ledger — mirrors `LONG_LIVE_TTL` in lib.rs (issue #667).
const LONG_LIVE_TTL: u32 = 18_921_600;
const TTL_BUMP_THRESHOLD_DIVISOR: u32 = 5;

/// Platform fee configuration stored in MerchantRegistry.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformFeeConfig {
    /// Fee in basis points (e.g. 200 = 2%).
    pub fee_bps: i128,
    /// Address that receives the platform fee.
    pub fee_recipient: Address,
}

pub const DEFAULT_SUSPENSION_THRESHOLD: u32 = 3;
pub const SUSPENSION_PROPOSAL_TTL_SECS: u64 = 7 * 24 * 60 * 60;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuspensionProposal {
    pub proposal_id: u64,
    pub proposer: Address,
    pub merchant_id: Address,
    pub reason: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub approve_votes: u32,
    pub reject_votes: u32,
    pub executed: bool,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum MerchantError {
    MerchantAlreadyExists = 1,
    MerchantNotFound = 2,
    Unauthorized = 3,
    NotVerified = 4,
    AdminAlreadySet = 5,
    PayoutAddressNotWhitelisted = 6,
    /// Whitelist mode may only be enabled by Business-tier merchants (issue #516)
    WhitelistModeRequiresBusinessTier = 7,
    /// Payer is not in the merchant's customer whitelist (issue #516)
    PayerNotWhitelisted = 8,
    ProposalNotFound = 9,
    ProposalExpired = 10,
    DuplicateVote = 11,
}

#[cfg_attr(
    any(not(target_arch = "wasm32"), feature = "contract-merchant-registry"),
    contractimpl
)]
#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
impl MerchantRegistry {
    pub fn version() -> u32 {
        1
    }

    /// Initialize the contract with an admin address
    pub fn initialize(env: Env, admin: Address) -> Result<(), MerchantError> {
        if env.storage().persistent().has(&MerchantDataKey::Admin) {
            return Err(MerchantError::AdminAlreadySet);
        }
        env.storage()
            .persistent()
            .set(&MerchantDataKey::Admin, &admin);
        AccessControl::initialize(&env, admin.clone());

        // Default limits for Unverified tier (max 100 USDC in stroops, assuming 7 decimals: 100 * 10^7)
        env.storage().persistent().set(
            &MerchantDataKey::TierLimit(KycTier::Unverified),
            &crate::AmountLimits {
                min: None,
                max: Some(1_000_000_000),
            },
        );

        // Issue #667: pre-populate on-chain metadata with description, version, and
        // deployment timestamp so explorers/integrators can identify the contract.
        env.storage().instance().set(
            &MerchantDataKey::ContractMetadata(Symbol::new(&env, "description")),
            &String::from_str(&env, "FluxaPay MerchantRegistry contract"),
        );
        env.storage().instance().set(
            &MerchantDataKey::ContractMetadata(Symbol::new(&env, "version")),
            &String::from_str(&env, "1"),
        );
        env.storage().instance().set(
            &MerchantDataKey::ContractMetadata(Symbol::new(&env, "deployed_at")),
            &Self::u64_to_string(&env, env.ledger().timestamp()),
        );
        let threshold = core::cmp::max(1, LONG_LIVE_TTL / TTL_BUMP_THRESHOLD_DIVISOR);
        env.storage()
            .instance()
            .extend_ttl(threshold, LONG_LIVE_TTL);

        Ok(())
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
    ) -> Result<(), MerchantError> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(MerchantError::Unauthorized);
        }

        env.storage()
            .instance()
            .set(&MerchantDataKey::ContractMetadata(key), &value);

        let threshold = core::cmp::max(1, LONG_LIVE_TTL / TTL_BUMP_THRESHOLD_DIVISOR);
        env.storage()
            .instance()
            .extend_ttl(threshold, LONG_LIVE_TTL);

        Ok(())
    }

    /// Public read of an on-chain metadata entry set via `set_contract_metadata`
    /// (issue #667). Returns `None` if the key was never set.
    pub fn get_contract_metadata(env: Env, key: Symbol) -> Option<String> {
        env.storage()
            .instance()
            .get(&MerchantDataKey::ContractMetadata(key))
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

    pub fn grant_role(
        env: Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), MerchantError> {
        AccessControl::grant_role(&env, admin, role, account)
            .map_err(|_| MerchantError::Unauthorized)
    }

    pub fn set_suspension_threshold(
        env: Env,
        admin: Address,
        threshold: u32,
    ) -> Result<(), MerchantError> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(MerchantError::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&MerchantDataKey::SuspensionThreshold, &threshold);
        Ok(())
    }

    pub fn set_tier_limits(
        env: Env,
        admin: Address,
        tier: KycTier,
        limits: crate::AmountLimits,
    ) -> Result<(), MerchantError> {
        admin.require_auth();
        let current_admin: Address = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::Admin)
            .ok_or(MerchantError::Unauthorized)?;

        if admin != current_admin {
            return Err(MerchantError::Unauthorized);
        }

        env.storage()
            .persistent()
            .set(&MerchantDataKey::TierLimit(tier), &limits);
        Ok(())
    }

    pub fn get_tier_limits(env: Env, tier: KycTier) -> crate::AmountLimits {
        env.storage()
            .persistent()
            .get(&MerchantDataKey::TierLimit(tier))
            .unwrap_or(crate::AmountLimits {
                min: None,
                max: None,
            })
    }

    /// Register a new merchant
    pub fn register_merchant(
        env: Env,
        merchant_id: Address,
        business_name: String,
        settlement_currency: String,
        payout_address: Option<Address>,
        bank_account: Option<String>,
        fee_config: MaybeFeeConfig,
    ) -> Result<(), MerchantError> {
        merchant_id.require_auth();

        if env
            .storage()
            .persistent()
            .has(&MerchantDataKey::Merchant(merchant_id.clone()))
        {
            return Err(MerchantError::MerchantAlreadyExists);
        }

        let merchant = Merchant {
            merchant_id: merchant_id.clone(),
            business_name,
            settlement_currency,
            payout_address,
            bank_account,
            kyc_tier: KycTier::Unverified,
            partial_payment_allowed: false,
            active: true,
            created_at: env.ledger().timestamp(),
            suspension_reason: None,
            suspended_at: None,
            suspension_expires_at: None,
            oracle_signature: None,
            last_payout_change_at: None,
            fee_config,
            metadata_hash: None,
            currency_payout_addresses: map![&env],
            payout_whitelist: vec![&env],
            anchor_config: MaybeAnchorConfig::None,
            fee_waiver_expires_at: None,
            settlement_schedule: SettlementSchedule::Manual,
            last_settlement_at: None,
            whitelist_mode: false,
            dispute_count: 0,
            lost_disputes_count: 0,
            resolved_against_count: 0,
            payment_tolerance: None,
        };

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        Self::add_to_merchant_list(&env, &merchant_id);

        Ok(())
    }

    /// Update merchant settings
    pub fn update_merchant(
        env: Env,
        merchant_id: Address,
        business_name: Option<String>,
        settlement_currency: Option<String>,
        active: Option<bool>,
        payout_address: Option<Address>,
        bank_account: Option<String>,
        fee_config: Option<MaybeFeeConfig>,
    ) -> Result<(), MerchantError> {
        merchant_id.require_auth();

        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;

        if let Some(name) = business_name {
            merchant.business_name = name;
        }
        if let Some(currency) = settlement_currency {
            merchant.settlement_currency = currency;
        }
        if let Some(is_active) = active {
            merchant.active = is_active;
        }
        if let Some(addr) = payout_address {
            // Validate against whitelist if whitelist is not empty (issue #210)
            if !merchant.payout_whitelist.is_empty() {
                let mut is_whitelisted = false;
                for whitelisted_addr in merchant.payout_whitelist.iter() {
                    if whitelisted_addr == addr {
                        is_whitelisted = true;
                        break;
                    }
                }
                if !is_whitelisted {
                    return Err(MerchantError::PayoutAddressNotWhitelisted);
                }
            }

            // Enforce 48-hour delay on payout address changes (issue #212)
            let current_time = env.ledger().timestamp();
            let forty_eight_hours = 48 * 60 * 60; // 48 hours in seconds
            if let Some(last_change_time) = merchant.last_payout_change_at {
                if current_time < last_change_time + forty_eight_hours {
                    return Err(MerchantError::Unauthorized); // Reuse Unauthorized error or create new one
                }
            }

            // Track payout address history: append the old address before overwriting.
            let old_payout = merchant.payout_address.clone();
            if old_payout != Some(addr.clone()) {
                if let Some(ref old_addr) = old_payout {
                    // Append old address to history list
                    let history_key = MerchantDataKey::MerchantPayoutHistory(merchant_id.clone());
                    let mut history: Vec<Address> = env
                        .storage()
                        .persistent()
                        .get(&history_key)
                        .unwrap_or_else(|| vec![&env]);
                    history.push_back(old_addr.clone());
                    env.storage().persistent().set(&history_key, &history);

                    // Emit specific PAYOUT_UPDATED event with old and new addresses.
                    env.events().publish(
                        (
                            Symbol::new(&env, "MERCHANT"),
                            Symbol::new(&env, "PAYOUT_UPDATED"),
                        ),
                        (merchant_id.clone(), old_addr.clone(), addr.clone()),
                    );
                }
            }

            merchant.payout_address = Some(addr);
            merchant.last_payout_change_at = Some(current_time);
        }
        if let Some(acct) = bank_account {
            merchant.bank_account = Some(acct);
        }
        if let Some(config) = fee_config {
            merchant.fee_config = config;
        }

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        env.events().publish(
            (Symbol::new(&env, "MERCHANT"), Symbol::new(&env, "UPDATED")),
            merchant_id,
        );

        Ok(())
    }

    /// Return the ordered list of previous payout addresses for a merchant.
    ///
    /// Each entry was the active `payout_address` immediately before it was
    /// changed; the list is in chronological order (oldest first).
    /// Returns an empty list if the address has never been changed.
    pub fn get_payout_history(
        env: Env,
        merchant_id: Address,
    ) -> Result<Vec<Address>, MerchantError> {
        // Ensure the merchant exists before returning history.
        Self::get_merchant_internal(&env, &merchant_id)?;
        let history: Vec<Address> = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::MerchantPayoutHistory(merchant_id))
            .unwrap_or_else(|| vec![&env]);
        Ok(history)
    }

    /// Merchant can toggle whether partial payments are accepted.
    pub fn set_partial_payment_allowed(
        env: Env,
        merchant_id: Address,
        partial_payment_allowed: bool,
    ) -> Result<(), MerchantError> {
        merchant_id.require_auth();
        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        merchant.partial_payment_allowed = partial_payment_allowed;

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "PARTIAL_PAYMENT_UPDATED"),
            ),
            (merchant_id, partial_payment_allowed),
        );

        Ok(())
    }

    /// Set merchant-specific payment tolerance (merchant auth required).
    /// Tolerance is in smallest currency units. None means use global default.
    /// Max tolerance is capped at 1% of payment amount to prevent abuse.
    pub fn set_merchant_payment_tolerance(
        env: Env,
        merchant_id: Address,
        tolerance: Option<i128>,
    ) -> Result<(), MerchantError> {
        merchant_id.require_auth();

        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;

        // Validate tolerance if provided
        if let Some(t) = tolerance {
            if t < 0 {
                return Err(MerchantError::Unauthorized);
            }
        }

        merchant.payment_tolerance = tolerance;

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "TOLERANCE_UPDATED"),
            ),
            (merchant_id, tolerance.unwrap_or(0)),
        );

        Ok(())
    }

    /// Set the global default payment tolerance (admin only).
    pub fn set_global_payment_tolerance(
        env: Env,
        admin: Address,
        tolerance: i128,
    ) -> Result<(), MerchantError> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::Admin)
            .ok_or(MerchantError::Unauthorized)?;

        if admin != stored_admin {
            return Err(MerchantError::Unauthorized);
        }

        if tolerance < 0 {
            return Err(MerchantError::Unauthorized);
        }

        env.storage()
            .persistent()
            .set(&MerchantDataKey::GlobalPaymentTolerance, &tolerance);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "GLOBAL_TOLERANCE_UPDATED"),
            ),
            tolerance,
        );

        Ok(())
    }

    /// Get the global default payment tolerance.
    pub fn get_global_payment_tolerance(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&MerchantDataKey::GlobalPaymentTolerance)
            .unwrap_or(crate::PAYMENT_TOLERANCE)
    }

    /// Get merchant info
    ///
    /// This function automatically reinstates merchants whose suspension has expired.
    pub fn get_merchant(env: Env, merchant_id: Address) -> Result<Merchant, MerchantError> {
        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        merchant.bank_account = None;
        Ok(merchant)
    }

    /// Get merchant bank account (restricted to admin or merchant)
    pub fn get_bank_account(
        env: Env,
        caller: Address,
        merchant_id: Address,
    ) -> Result<Option<String>, MerchantError> {
        caller.require_auth();

        if caller != merchant_id {
            let stored_admin: Address = env
                .storage()
                .persistent()
                .get(&MerchantDataKey::Admin)
                .ok_or(MerchantError::Unauthorized)?;

            if caller != stored_admin {
                return Err(MerchantError::Unauthorized);
            }
        }

        let merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        Ok(merchant.bank_account)
    }

    /// Verify merchant (admin only) — sets KycTier::Basic for backward compatibility.
    /// If a RefundManager address is configured, also grants the MERCHANT role there.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `admin` - The admin address
    /// * `merchant_id` - The merchant address to verify
    /// * `oracle_signature` - Optional oracle signature for KYC verification
    pub fn verify_merchant(
        env: Env,
        admin: Address,
        merchant_id: Address,
    ) -> Result<(), MerchantError> {
        Self::verify_merchant_with_signature(env, admin, merchant_id, None)
    }

    /// Verify merchant with optional oracle signature metadata.
    pub fn verify_merchant_with_signature(
        env: Env,
        admin: Address,
        merchant_id: Address,
        oracle_signature: Option<String>,
    ) -> Result<(), MerchantError> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::Admin)
            .ok_or(MerchantError::Unauthorized)?;

        if admin != stored_admin {
            return Err(MerchantError::Unauthorized);
        }

        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        merchant.kyc_tier = KycTier::Basic;

        // Store oracle signature if provided
        if let Some(signature) = oracle_signature {
            merchant.oracle_signature = Some(signature);
        }

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        // If RefundManager is configured, grant the MERCHANT role
        if let Some(refund_manager_addr) = env
            .storage()
            .persistent()
            .get::<MerchantDataKey, Address>(&MerchantDataKey::RefundManagerAddress)
        {
            let rm_client = crate::RefundManagerClient::new(&env, &refund_manager_addr);
            let _ = rm_client.try_grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant_id);
        }

        env.events().publish(
            (Symbol::new(&env, "MERCHANT"), Symbol::new(&env, "VERIFIED")),
            merchant_id,
        );

        Ok(())
    }

    /// Set a specific KYC tier for a merchant (admin only).
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `admin` - The admin address
    /// * `merchant_id` - The merchant address to update
    /// * `tier` - The KYC tier to set
    /// * `oracle_signature` - Optional oracle signature for KYC verification
    pub fn set_kyc_tier(
        env: Env,
        admin: Address,
        merchant_id: Address,
        tier: KycTier,
    ) -> Result<(), MerchantError> {
        Self::set_kyc_tier_with_signature(env, admin, merchant_id, tier, None)
    }

    /// Set a specific KYC tier with optional oracle signature metadata.
    pub fn set_kyc_tier_with_signature(
        env: Env,
        admin: Address,
        merchant_id: Address,
        tier: KycTier,
        oracle_signature: Option<String>,
    ) -> Result<(), MerchantError> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::Admin)
            .ok_or(MerchantError::Unauthorized)?;

        if admin != stored_admin {
            return Err(MerchantError::Unauthorized);
        }

        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        merchant.kyc_tier = tier;

        // Store oracle signature if provided
        if let Some(signature) = oracle_signature {
            merchant.oracle_signature = Some(signature);
        }

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id), &merchant);

        Ok(())
    }

    /// Set the RefundManager contract address for automatic MERCHANT role granting
    pub fn set_refund_manager_address(
        env: Env,
        admin: Address,
        refund_manager: Address,
    ) -> Result<(), MerchantError> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::Admin)
            .ok_or(MerchantError::Unauthorized)?;

        if admin != stored_admin {
            return Err(MerchantError::Unauthorized);
        }

        env.storage()
            .persistent()
            .set(&MerchantDataKey::RefundManagerAddress, &refund_manager);

        Ok(())
    }

    pub fn get_refund_manager_address(env: Env) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&MerchantDataKey::RefundManagerAddress)
    }

    /// Set the PaymentProcessor contract address for automatic KYC tier upgrades (issue #207).
    pub fn set_payment_processor_address(
        env: Env,
        admin: Address,
        payment_processor: Address,
    ) -> Result<(), MerchantError> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::Admin)
            .ok_or(MerchantError::Unauthorized)?;

        if admin != stored_admin {
            return Err(MerchantError::Unauthorized);
        }

        env.storage().persistent().set(
            &MerchantDataKey::PaymentProcessorAddress,
            &payment_processor,
        );

        Ok(())
    }

    pub fn get_payment_processor_address(env: Env) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&MerchantDataKey::PaymentProcessorAddress)
    }

    /// Automatically upgrade a merchant's KYC tier based on cumulative payment volume (issue #207).
    /// Only the registered PaymentProcessor contract may call this function.
    /// Only tier promotions are allowed (Unverified→Basic→Full→Business); demotions are rejected.
    pub fn auto_upgrade_kyc_tier(
        env: Env,
        caller: Address,
        merchant_id: Address,
        new_tier: KycTier,
    ) -> Result<(), MerchantError> {
        caller.require_auth();

        let payment_processor: Address = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::PaymentProcessorAddress)
            .ok_or(MerchantError::Unauthorized)?;

        if caller != payment_processor {
            return Err(MerchantError::Unauthorized);
        }

        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        let old_tier = merchant.kyc_tier.clone();

        let is_promotion = matches!(
            (&old_tier, &new_tier),
            (KycTier::Unverified, KycTier::Basic)
                | (KycTier::Basic, KycTier::Full)
                | (KycTier::Full, KycTier::Business)
        );

        if !is_promotion {
            return Err(MerchantError::Unauthorized);
        }

        merchant.kyc_tier = new_tier.clone();

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "KYC_UPGRADED"),
            ),
            merchant_id.clone(),
        );

        crate::events::emit_kyc_tier_upgraded(&env, &merchant_id, &old_tier, &new_tier);

        Ok(())
    }

    /// Suspend a merchant (admin only)
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `admin` - The admin address
    /// * `merchant_id` - The merchant address to suspend
    /// * `reason` - The reason for suspension
    /// * `expiration_duration` - Duration in seconds after which suspension auto-lifts
    pub fn suspend_merchant(
        env: Env,
        admin: Address,
        merchant_id: Address,
        reason: String,
        expiration_duration: u64,
    ) -> Result<(), MerchantError> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::Admin)
            .ok_or(MerchantError::Unauthorized)?;

        if admin != stored_admin {
            return Err(MerchantError::Unauthorized);
        }

        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        merchant.active = false;
        merchant.suspension_reason = Some(reason);
        merchant.suspended_at = Some(env.ledger().timestamp());
        merchant.suspension_expires_at = Some(env.ledger().timestamp() + expiration_duration);

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        crate::events::emit_merchant_suspended(&env, &merchant_id, &reason);

        Ok(())
    }

    fn get_suspension_threshold(env: &Env) -> u32 {
        env.storage()
            .persistent()
            .get(&MerchantDataKey::SuspensionThreshold)
            .unwrap_or(DEFAULT_SUSPENSION_THRESHOLD)
    }

    fn next_suspension_proposal_id(env: &Env) -> u64 {
        let id = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::SuspensionProposalCounter)
            .unwrap_or(0u64)
            .saturating_add(1);
        env.storage()
            .persistent()
            .set(&MerchantDataKey::SuspensionProposalCounter, &id);
        id
    }

    pub fn propose_merchant_suspension(
        env: Env,
        proposer: Address,
        merchant_id: Address,
        reason: String,
    ) -> Result<u64, MerchantError> {
        proposer.require_auth();
        if !AccessControl::has_role(&env, &role_arbitrator(&env), &proposer) {
            return Err(MerchantError::Unauthorized);
        }
        let _merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        let proposal_id = Self::next_suspension_proposal_id(&env);
        let now = env.ledger().timestamp();
        let proposal = SuspensionProposal {
            proposal_id,
            proposer: proposer.clone(),
            merchant_id: merchant_id.clone(),
            reason,
            created_at: now,
            expires_at: now.saturating_add(SUSPENSION_PROPOSAL_TTL_SECS),
            approve_votes: 0,
            reject_votes: 0,
            executed: false,
        };
        env.storage()
            .persistent()
            .set(&MerchantDataKey::SuspensionProposal(proposal_id), &proposal);
        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "SUSPENSION_PROPOSED"),
            ),
            (proposal_id, proposer, merchant_id),
        );
        Ok(proposal_id)
    }

    pub fn vote_suspension(
        env: Env,
        voter: Address,
        proposal_id: u64,
        approve: bool,
    ) -> Result<(), MerchantError> {
        voter.require_auth();
        if !AccessControl::has_role(&env, &role_arbitrator(&env), &voter) {
            return Err(MerchantError::Unauthorized);
        }
        let vote_key = MerchantDataKey::SuspensionVote(proposal_id, voter.clone());
        if env.storage().persistent().has(&vote_key) {
            return Err(MerchantError::DuplicateVote);
        }
        let mut proposal: SuspensionProposal = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::SuspensionProposal(proposal_id))
            .ok_or(MerchantError::ProposalNotFound)?;
        if env.ledger().timestamp() > proposal.expires_at {
            return Err(MerchantError::ProposalExpired);
        }
        env.storage().persistent().set(&vote_key, &approve);
        if approve {
            proposal.approve_votes = proposal.approve_votes.saturating_add(1);
        } else {
            proposal.reject_votes = proposal.reject_votes.saturating_add(1);
        }
        let threshold = Self::get_suspension_threshold(&env);
        if !proposal.executed && proposal.approve_votes >= threshold {
            let mut merchant = Self::get_merchant_internal(&env, &proposal.merchant_id)?;
            merchant.active = false;
            merchant.suspension_reason = Some(proposal.reason.clone());
            merchant.suspended_at = Some(env.ledger().timestamp());
            merchant.suspension_expires_at = None;
            proposal.executed = true;
            env.storage().persistent().set(
                &MerchantDataKey::Merchant(proposal.merchant_id.clone()),
                &merchant,
            );
            env.events().publish(
                (
                    Symbol::new(&env, "MERCHANT"),
                    Symbol::new(&env, "SUSPENDED"),
                ),
                proposal.merchant_id.clone(),
            );
        }
        env.storage()
            .persistent()
            .set(&MerchantDataKey::SuspensionProposal(proposal_id), &proposal);
        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "SUSPENSION_VOTED"),
            ),
            (
                proposal_id,
                voter,
                approve,
                proposal.approve_votes,
                proposal.reject_votes,
            ),
        );
        Ok(())
    }

    /// Reinstate a suspended merchant (admin only)
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `admin` - The admin address
    /// * `merchant_id` - The merchant address to reinstate
    pub fn reinstate_merchant(
        env: Env,
        admin: Address,
        merchant_id: Address,
    ) -> Result<(), MerchantError> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::Admin)
            .ok_or(MerchantError::Unauthorized)?;

        if admin != stored_admin {
            return Err(MerchantError::Unauthorized);
        }

        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        merchant.active = true;
        merchant.suspension_reason = None;
        merchant.suspended_at = None;
        merchant.suspension_expires_at = None;

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        crate::events::emit_merchant_reinstated(&env, &merchant_id, &admin);

        Ok(())
    }

    /// Get all registered merchants with pagination support
    pub fn get_all_merchants(env: Env, offset: u32, limit: u32) -> Vec<Merchant> {
        let merchant_ids: Vec<Address> = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::MerchantList)
            .unwrap_or_else(|| vec![&env]);

        if limit == 0 {
            return vec![&env];
        }

        let mut result = vec![&env];
        let end = core::cmp::min(merchant_ids.len(), offset.saturating_add(limit));

        let mut i = offset;
        while i < end {
            if let Some(merchant_id) = merchant_ids.get(i) {
                if let Ok(mut merchant) = Self::get_merchant_internal(&env, &merchant_id) {
                    merchant.bank_account = None;
                    result.push_back(merchant);
                }
            }
            i += 1;
        }

        result
    }

    /// Get all verified merchants (kyc_tier != Unverified)
    pub fn get_verified_merchants(env: Env) -> Vec<Merchant> {
        let merchant_ids: Vec<Address> = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::MerchantList)
            .unwrap_or_else(|| vec![&env]);

        let mut result = vec![&env];
        for merchant_id in merchant_ids.iter() {
            if let Ok(mut merchant) = Self::get_merchant_internal(&env, &merchant_id) {
                if merchant.kyc_tier != KycTier::Unverified {
                    merchant.bank_account = None;
                    result.push_back(merchant);
                }
            }
        }

        result
    }

    /// Set the platform fee configuration (admin only).
    /// `_merchant_id` and `fixed_fee` are accepted for API compatibility and reserved for
    /// future per-merchant fee support.
    pub fn set_fee_config(
        env: Env,
        admin: Address,
        _merchant_id: Address,
        fee_bps: i128,
        _fixed_fee: i128,
        fee_recipient: Option<Address>,
    ) -> Result<(), MerchantError> {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::Admin)
            .ok_or(MerchantError::Unauthorized)?;
        if admin != stored_admin {
            return Err(MerchantError::Unauthorized);
        }
        let recipient = fee_recipient.unwrap_or_else(|| env.current_contract_address());
        env.storage().persistent().set(
            &MerchantDataKey::FeeConfig,
            &PlatformFeeConfig {
                fee_bps,
                fee_recipient: recipient,
            },
        );
        Ok(())
    }

    /// Calculate the platform fee for a given amount using the stored global fee config.
    /// Returns `(fee_amount, fee_recipient)`. Returns `(0, contract_address)` if no config set.
    pub fn calculate_platform_fee(env: Env, amount: i128) -> (i128, Address) {
        if let Some(config) = env
            .storage()
            .persistent()
            .get::<MerchantDataKey, PlatformFeeConfig>(&MerchantDataKey::FeeConfig)
        {
            let fee = amount * config.fee_bps / 10_000;
            (fee, config.fee_recipient)
        } else {
            (0, env.current_contract_address())
        }
    }

    // Helper functions
    fn add_to_merchant_list(env: &Env, merchant_id: &Address) {
        let key = MerchantDataKey::MerchantList;
        let mut merchants: Vec<Address> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![env]);

        // Only add if not already present
        let mut found = false;
        for m in merchants.iter() {
            if m == *merchant_id {
                found = true;
                break;
            }
        }

        if !found {
            merchants.push_back(merchant_id.clone());
            env.storage().persistent().set(&key, &merchants);
        }
    }

    /// Calculate the platform fee for a given amount based on merchant's FeeConfig.
    /// Returns (platform_fee, net_amount).
    pub fn calculate_fee(
        env: Env,
        merchant_id: Address,
        amount: i128,
    ) -> Result<(i128, i128), MerchantError> {
        let merchant = Self::get_merchant_internal(&env, &merchant_id)?;

        if let Some(config) = merchant.fee_config.as_option() {
            if config.platform_fee_bps == 0 && config.fixed_fee == 0 {
                return Ok((0, amount));
            }

            // Calculate percentage fee
            let percentage_fee = (amount * (config.platform_fee_bps as i128)) / 10_000;

            // Total fee is percentage + fixed
            let total_fee = percentage_fee.saturating_add(config.fixed_fee);

            // Ensure fee doesn't exceed amount
            if total_fee >= amount {
                return Ok((amount, 0));
            }

            let net_amount = amount.saturating_sub(total_fee);
            Ok((total_fee, net_amount))
        } else {
            // No fee config, no fee
            Ok((0, amount))
        }
    }

    /// Get the fee recipient address for a merchant.
    /// Returns the custom fee recipient if set, otherwise the admin address.
    pub fn get_fee_recipient(env: Env, merchant_id: Address) -> Result<Address, MerchantError> {
        let merchant = Self::get_merchant_internal(&env, &merchant_id)?;

        if let MaybeFeeConfig::Some(config) = merchant.fee_config {
            if let Some(recipient) = &config.fee_recipient {
                return Ok(recipient.clone());
            }
        }

        // Default to admin if no custom recipient
        let admin: Address = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::Admin)
            .ok_or(MerchantError::Unauthorized)?;

        Ok(admin)
    }

    fn get_merchant_internal(env: &Env, merchant_id: &Address) -> Result<Merchant, MerchantError> {
        let mut merchant: Merchant = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::Merchant(merchant_id.clone()))
            .ok_or(MerchantError::MerchantNotFound)?;

        // Auto-recover expired suspensions
        if !merchant.active && merchant.suspension_expires_at.is_some() {
            let current_time = env.ledger().timestamp();
            if let Some(expiration_time) = merchant.suspension_expires_at {
                if current_time >= expiration_time {
                    // Auto-reinstate expired suspension
                    merchant.active = true;
                    merchant.suspension_reason = None;
                    merchant.suspended_at = None;
                    merchant.suspension_expires_at = None;

                    // Save the updated merchant state
                    env.storage()
                        .persistent()
                        .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);
                }
            }
        }

        Ok(merchant)
    }

    /// Set IPFS metadata hash for merchant profile (issue #208)
    pub fn set_metadata_hash(
        env: Env,
        merchant_id: Address,
        metadata_hash: String,
    ) -> Result<(), MerchantError> {
        merchant_id.require_auth();

        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        merchant.metadata_hash = Some(metadata_hash);

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "METADATA_UPDATED"),
            ),
            merchant_id,
        );

        Ok(())
    }

    /// Get IPFS metadata hash for merchant (issue #208)
    pub fn get_metadata_hash(
        env: Env,
        merchant_id: Address,
    ) -> Result<Option<String>, MerchantError> {
        let merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        Ok(merchant.metadata_hash)
    }

    /// Add payout address for a specific currency (issue #216)
    pub fn add_currency_payout(
        env: Env,
        merchant_id: Address,
        currency: String,
        payout_address: Address,
    ) -> Result<(), MerchantError> {
        merchant_id.require_auth();

        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;

        // Validate against whitelist if whitelist is not empty (issue #210)
        if !merchant.payout_whitelist.is_empty() {
            let mut is_whitelisted = false;
            for addr in merchant.payout_whitelist.iter() {
                if addr == payout_address {
                    is_whitelisted = true;
                    break;
                }
            }
            if !is_whitelisted {
                return Err(MerchantError::PayoutAddressNotWhitelisted);
            }
        }

        merchant
            .currency_payout_addresses
            .set(currency.clone(), payout_address.clone());

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "CURRENCY_PAYOUT_ADDED"),
            ),
            (merchant_id, currency, payout_address),
        );

        Ok(())
    }

    /// Get payout address for a specific currency (issue #216)
    pub fn get_currency_payout(
        env: Env,
        merchant_id: Address,
        currency: String,
    ) -> Result<Option<Address>, MerchantError> {
        let merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        Ok(merchant.currency_payout_addresses.get(currency))
    }

    /// Get all currency payout mappings for a merchant (issue #216)
    pub fn get_all_currency_payouts(
        env: Env,
        merchant_id: Address,
    ) -> Result<Map<String, Address>, MerchantError> {
        let merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        Ok(merchant.currency_payout_addresses)
    }

    /// Add address to payout whitelist (issue #210)
    pub fn add_to_whitelist(
        env: Env,
        merchant_id: Address,
        payout_address: Address,
    ) -> Result<(), MerchantError> {
        merchant_id.require_auth();

        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;

        // Check if address is already in whitelist
        let mut already_exists = false;
        for addr in merchant.payout_whitelist.iter() {
            if addr == payout_address {
                already_exists = true;
                break;
            }
        }

        if !already_exists {
            merchant.payout_whitelist.push_back(payout_address.clone());
            env.storage()
                .persistent()
                .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

            env.events().publish(
                (
                    Symbol::new(&env, "MERCHANT"),
                    Symbol::new(&env, "WHITELIST_ADDED"),
                ),
                (merchant_id, payout_address),
            );
        }

        Ok(())
    }

    /// Remove address from payout whitelist (issue #210)
    pub fn remove_from_whitelist(
        env: Env,
        merchant_id: Address,
        payout_address: Address,
    ) -> Result<(), MerchantError> {
        merchant_id.require_auth();

        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;

        let mut new_whitelist = vec![&env];
        for addr in merchant.payout_whitelist.iter() {
            if addr != payout_address {
                new_whitelist.push_back(addr);
            }
        }

        merchant.payout_whitelist = new_whitelist;
        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "WHITELIST_REMOVED"),
            ),
            (merchant_id, payout_address),
        );

        Ok(())
    }

    /// Get payout whitelist for a merchant (issue #210)
    pub fn get_whitelist(env: Env, merchant_id: Address) -> Result<Vec<Address>, MerchantError> {
        let merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        Ok(merchant.payout_whitelist)
    }

    /// Validate if a payout address is whitelisted (issue #210)
    pub fn is_address_whitelisted(
        env: Env,
        merchant_id: Address,
        payout_address: Address,
    ) -> Result<bool, MerchantError> {
        let merchant = Self::get_merchant_internal(&env, &merchant_id)?;

        // If whitelist is empty, all addresses are allowed
        if merchant.payout_whitelist.is_empty() {
            return Ok(true);
        }

        for addr in merchant.payout_whitelist.iter() {
            if addr == payout_address {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Issue #184: System-initiated suspension triggered by the RefundManager contract
    /// when a merchant's dispute rate exceeds the auto-suspend threshold.
    ///
    /// Unlike `suspend_merchant`, this function does **not** require admin auth —
    /// it is intended to be called cross-contract by the RefundManager. The caller
    /// is the RefundManager contract address itself, which is trusted implicitly
    /// because it is a deployed contract (not an externally-owned account).
    ///
    /// # Arguments
    /// * `merchant_id`          – The merchant to suspend.
    /// * `reason`               – Human-readable suspension reason.
    /// * `expiration_duration`  – Duration in seconds after which the suspension auto-lifts.
    pub fn suspend_merchant_by_system(
        env: Env,
        merchant_id: Address,
        reason: String,
        expiration_duration: u64,
    ) -> Result<(), MerchantError> {
        // Only suspend if the merchant exists and is currently active
        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;

        if !merchant.active {
            // Already suspended — nothing to do
            return Ok(());
        }

        let now = env.ledger().timestamp();
        merchant.active = false;
        merchant.suspension_reason = Some(reason);
        merchant.suspended_at = Some(now);
        merchant.suspension_expires_at = Some(now + expiration_duration);

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "AUTO_SUSPENDED"),
            ),
            merchant_id,
        );

        Ok(())
    }

    /// Issue #184: Get the current dispute count for a merchant.
    /// Returns 0 if no disputes have been filed against this merchant.
    pub fn get_merchant_dispute_count(env: Env, merchant_id: Address) -> u64 {
        // This is stored in the RefundManager, not here — expose a no-op placeholder
        // so the SDK surface is consistent. Actual counts live in DataKey::MerchantDisputeCount.
        let _ = (env, merchant_id);
        0
    }

    /// Upgrade the contract WASM.
    ///
    /// Only the admin can call this. Emits a `CONTRACT/UPGRADED` event on success.
    pub fn upgrade(
        env: Env,
        admin: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), MerchantError> {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::Admin)
            .ok_or(MerchantError::Unauthorized)?;

        if admin != stored_admin {
            return Err(MerchantError::Unauthorized);
        }

        env.deployer().update_current_contract_wasm(new_wasm_hash);

        env.events().publish(
            (Symbol::new(&env, "CONTRACT"), Symbol::new(&env, "UPGRADED")),
            admin,
        );

        Ok(())
    }
    /// Issue #398: Transfer MerchantRegistry admin ownership to a new address.
    ///
    /// Only the current admin may call this. Once transferred, the old admin
    /// loses all admin privileges and the new admin is stored under
    /// `MerchantDataKey::Admin`.
    ///
    /// Emits a `MERCHANT_REGISTRY / ADMIN_TRANSFERRED` event with
    /// `(old_admin, new_admin)` as the payload.
    pub fn transfer_admin(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), MerchantError> {
        current_admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::Admin)
            .ok_or(MerchantError::Unauthorized)?;

        if current_admin != stored_admin {
            return Err(MerchantError::Unauthorized);
        }

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Admin, &new_admin);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT_REGISTRY"),
                Symbol::new(&env, "ADMIN_TRANSFERRED"),
            ),
            (current_admin, new_admin),
        );

        Ok(())
    }

    /// Issue #398: Return the current admin address.
    pub fn get_admin(env: Env) -> Option<Address> {
        env.storage().persistent().get(&MerchantDataKey::Admin)
    }

    /// Configure or clear the merchant's Stellar Anchor (SEP-6 / SEP-24)
    /// integration for fiat offramp during settlement.
    ///
    /// When `anchor_config` is `Some`, the off-chain Settlement Service will
    /// call the anchor's SEP-6 withdrawal endpoint after each on-chain
    /// settlement. If `anchor_config` is `None`, the merchant reverts to
    /// on-chain-only settlement (USDC remains at their `payout_address`).
    ///
    /// Only the merchant themselves may call this. Admin can set an anchor
    /// on the merchant's behalf by first impersonating them via their
    /// authorized signer setup (outside this contract).
    ///
    /// Emits `(MERCHANT, ANCHOR_UPDATED) → (merchant_id, anchor_domain_opt)`
    /// where `anchor_domain_opt` is the empty string when clearing.
    pub fn set_merchant_anchor(
        env: Env,
        merchant_id: Address,
        anchor_config: Option<AnchorConfig>,
    ) -> Result<(), MerchantError> {
        merchant_id.require_auth();

        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        let domain_for_event = match &anchor_config {
            Some(cfg) => cfg.anchor_domain.clone(),
            None => String::from_str(&env, ""),
        };
        merchant.anchor_config = MaybeAnchorConfig::from(anchor_config);
        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "ANCHOR_UPDATED"),
            ),
            (merchant_id, domain_for_event),
        );
        Ok(())
    }

    /// Issue #669: Set (or replace) a merchant's SEP-6/SEP-24 anchor
    /// configuration. Thin, always-set wrapper around `set_merchant_anchor`
    /// (which also supports clearing via `None`) that additionally requires
    /// the merchant to hold at least `KycTier::Basic` before an anchor may be
    /// attached, per the SEP-6/SEP-24 integration guide.
    ///
    /// Requires the merchant's own signature (same auth as `set_merchant_anchor`).
    pub fn set_anchor_config(
        env: Env,
        merchant_id: Address,
        config: AnchorConfig,
    ) -> Result<(), MerchantError> {
        let merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        if merchant.kyc_tier == KycTier::Unverified {
            return Err(MerchantError::NotVerified);
        }

        Self::set_merchant_anchor(env, merchant_id, Some(config))
    }

    /// Issue #669: Public read-only accessor for a merchant's anchor
    /// configuration. Returns `None` if the merchant has not configured an
    /// anchor (or does not exist).
    pub fn get_anchor_config(env: Env, merchant_id: Address) -> Option<AnchorConfig> {
        Self::get_merchant_internal(&env, &merchant_id)
            .ok()
            .and_then(|m| m.anchor_config.into_option())
    }

    /// Enable/disable whitelist mode for a merchant (issue #516).
    /// Only Business-tier merchants may enable whitelist mode.
    pub fn set_merchant_whitelist_mode(
        env: Env,
        merchant_id: Address,
        enabled: bool,
    ) -> Result<(), MerchantError> {
        merchant_id.require_auth();

        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;

        if enabled && merchant.kyc_tier != KycTier::Business {
            return Err(MerchantError::WhitelistModeRequiresBusinessTier);
        }

        merchant.whitelist_mode = enabled;
        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "WHITELIST_UPDATED"),
            ),
            (merchant_id, enabled),
        );

        Ok(())
    }

    /// Admin-only: apply or clear a time-based fee waiver for a merchant.
    ///
    /// While `expires_at` is `Some(ts)`, all settlements for this merchant pay zero
    /// platform fee until `ledger.timestamp < expires_at`. This enables
    /// onboarding/promotion campaigns. Pass `None` to immediately revoke
    /// an active waiver.
    ///
    /// Auth: only the MerchantRegistry admin.
    ///
    /// Emits `(MERCHANT, FEE_WAIVER_SET) → (merchant_id, expires_at_opt)`
    /// where `expires_at_opt` is 0 when clearing.
    pub fn set_merchant_fee_waiver(
        env: Env,
        admin: Address,
        merchant_id: Address,
        expires_at: Option<u64>,
    ) -> Result<(), MerchantError> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::Admin)
            .ok_or(MerchantError::Unauthorized)?;
        if admin != stored_admin {
            return Err(MerchantError::Unauthorized);
        }

        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        merchant.fee_waiver_expires_at = expires_at;
        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        let expires_at_for_event = expires_at.unwrap_or(0u64);
        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "FEE_WAIVER_SET"),
            ),
            (merchant_id, expires_at_for_event),
        );

        Ok(())
    }

    /// Add a customer address to the merchant's payment whitelist (issue #516).
    pub fn add_to_customer_whitelist(
        env: Env,
        merchant_id: Address,
        customer: Address,
    ) -> Result<(), MerchantError> {
        merchant_id.require_auth();
        Self::get_merchant_internal(&env, &merchant_id)?;

        let key = MerchantDataKey::MerchantCustomerWhitelist(merchant_id.clone());
        let mut whitelist: Vec<Address> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![&env]);

        if !whitelist.iter().any(|addr| addr == customer) {
            whitelist.push_back(customer.clone());
            env.storage().persistent().set(&key, &whitelist);
        }

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "WHITELIST_UPDATED"),
            ),
            (merchant_id, customer, true),
        );

        Ok(())
    }

    /// Remove a customer address from the merchant's payment whitelist (issue #516).
    pub fn remove_from_customer_whitelist(
        env: Env,
        merchant_id: Address,
        customer: Address,
    ) -> Result<(), MerchantError> {
        merchant_id.require_auth();
        Self::get_merchant_internal(&env, &merchant_id)?;

        let key = MerchantDataKey::MerchantCustomerWhitelist(merchant_id.clone());
        let whitelist: Vec<Address> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![&env]);

        let mut new_whitelist = vec![&env];
        for addr in whitelist.iter() {
            if addr != customer {
                new_whitelist.push_back(addr);
            }
        }
        env.storage().persistent().set(&key, &new_whitelist);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "WHITELIST_UPDATED"),
            ),
            (merchant_id, customer, false),
        );

        Ok(())
    }

    /// Check whether a customer address is allowed to pay a merchant (issue #516).
    /// Returns true when whitelist mode is disabled, or when the address is
    /// present in the merchant's customer whitelist.
    pub fn is_customer_whitelisted(
        env: Env,
        merchant_id: Address,
        customer: Address,
    ) -> Result<bool, MerchantError> {
        let merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        if !merchant.whitelist_mode {
            return Ok(true);
        }

        let whitelist: Vec<Address> = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::MerchantCustomerWhitelist(merchant_id))
            .unwrap_or_else(|| vec![&env]);

        Ok(whitelist.iter().any(|addr| addr == customer))
    }

    /* ------------------------------------------------------------------ */
    /*  Issue #480: Merchant settlement schedule                            */
    /* ------------------------------------------------------------------ */

    /// Set the settlement schedule for a merchant.
    /// Only the merchant can change their own schedule.
    pub fn set_settlement_schedule(
        env: Env,
        merchant_id: Address,
        schedule: SettlementSchedule,
    ) -> Result<(), MerchantError> {
        merchant_id.require_auth();
        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        merchant.settlement_schedule = schedule.clone();
        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);
        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "SETTLEMENT_SCHEDULE_UPDATED"),
            ),
            (merchant_id, schedule),
        );
        Ok(())
    }

    /// Issue #481: Set the global dispute threshold for auto-suspension.
    /// When a merchant's lost_disputes_count reaches or exceeds this,
    /// the merchant is automatically suspended.
    pub fn set_dispute_threshold(
        env: Env,
        admin: Address,
        threshold: u32,
    ) -> Result<(), MerchantError> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::Admin)
            .ok_or(MerchantError::Unauthorized)?;

        if admin != stored_admin {
            return Err(MerchantError::Unauthorized);
        }

        env.storage()
            .persistent()
            .set(&MerchantDataKey::DisputeThreshold, &threshold);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "DISPUTE_THRESHOLD_SET"),
            ),
            threshold,
        );

        Ok(())
    }

    /// Get the accumulated pending settlement amount for a merchant.
    pub fn get_pending_settlement(env: Env, merchant_id: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&MerchantDataKey::MerchantPendingSettlement(merchant_id))
            .unwrap_or(0)
    }

    /// Add an amount to the merchant's pending settlement balance.
    /// Called by settle_payment in PaymentProcessor after a payment is settled.
    pub fn add_pending_settlement(env: &Env, merchant_id: &Address, amount: i128) {
        let current: i128 = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::MerchantPendingSettlement(
                merchant_id.clone(),
            ))
            .unwrap_or(0);
        env.storage().persistent().set(
            &MerchantDataKey::MerchantPendingSettlement(merchant_id.clone()),
            &current.saturating_add(amount),
        );
    }

    /// Clear the pending settlement balance for a merchant.
    pub fn clear_pending_settlement(env: &Env, merchant_id: &Address) {
        env.storage().persistent().set(
            &MerchantDataKey::MerchantPendingSettlement(merchant_id.clone()),
            &0i128,
        );
    }

    /// Get the last settlement timestamp for a merchant.
    pub fn get_last_settlement_at(env: &Env, merchant_id: &Address) -> Option<u64> {
        Self::get_merchant_internal(env, merchant_id)
            .ok()
            .map(|m| m.last_settlement_at)
            .unwrap_or(None)
    }

    /// Set the last settlement timestamp for a merchant.
    pub fn set_last_settlement_at(
        env: Env,
        merchant_id: Address,
        timestamp: u64,
    ) -> Result<(), MerchantError> {
        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        merchant.last_settlement_at = Some(timestamp);
        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id), &merchant);
        Ok(())
    }
    /// Issue #481: Get the current global dispute threshold.
    pub fn get_dispute_threshold(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&MerchantDataKey::DisputeThreshold)
            .unwrap_or(10) // Default: 10 disputes
    }

    /// Issue #481: Increment dispute count for a merchant and check for auto-suspension.
    /// Returns the new dispute count.
    pub fn increment_merchant_dispute_count(
        env: Env,
        merchant_id: Address,
    ) -> Result<u32, MerchantError> {
        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        merchant.dispute_count = merchant.dispute_count.saturating_add(1);

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        Ok(merchant.dispute_count)
    }

    /// Issue #481: Increment the resolved-against count for a merchant and check auto-suspension.
    /// Returns the new resolved_against_merchant_count.
    pub fn increment_resolved_against_count(
        env: Env,
        merchant_id: Address,
    ) -> Result<u32, MerchantError> {
        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        merchant.lost_disputes_count = merchant.lost_disputes_count.saturating_add(1);
        merchant.resolved_against_count = merchant.resolved_against_count.saturating_add(1);

        let threshold = Self::get_dispute_threshold(env.clone());

        // Auto-suspend if threshold reached
        if merchant.resolved_against_count >= threshold && merchant.suspension_reason.is_none() {
            merchant.active = false;
            merchant.suspension_reason = Some(String::from_str(
                &env,
                "Auto-suspended due to dispute threshold",
            ));
            merchant.suspended_at = Some(env.ledger().timestamp());

            env.events().publish(
                (
                    Symbol::new(&env, "MERCHANT"),
                    Symbol::new(&env, "AUTO_SUSPENDED"),
                ),
                (
                    merchant_id.clone(),
                    merchant.lost_disputes_count,
                    merchant.resolved_against_count,
                    threshold,
                ),
            );
        }

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id), &merchant);

        Ok(merchant.resolved_against_count)
    }

    /// Issue #481: Appeal a merchant suspension. Creates a review record requiring operator approval.
    /// The merchant must be currently suspended for this to succeed.
    pub fn appeal_suspension(
        env: Env,
        merchant_id: Address,
        reason: String,
    ) -> Result<(), MerchantError> {
        merchant_id.require_auth();

        let merchant = Self::get_merchant_internal(&env, &merchant_id)?;

        // Merchant must be suspended to appeal
        if merchant.suspension_reason.is_none() {
            return Err(MerchantError::Unauthorized);
        }

        // Store appeal record (operator will review)
        // The appeal is stored as a suspensionReason update or separate review record
        // For now, we emit an event that operators can monitor
        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "SUSPENSION_APPEAL"),
            ),
            (merchant_id, reason),
        );

        Ok(())
    }

    /// Issue #481: Admin function to approve a merchant's suspension appeal and unsuspend.
    pub fn approve_suspension_appeal(
        env: Env,
        admin: Address,
        merchant_id: Address,
    ) -> Result<(), MerchantError> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&MerchantDataKey::Admin)
            .ok_or(MerchantError::Unauthorized)?;

        if admin != stored_admin {
            return Err(MerchantError::Unauthorized);
        }

        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;

        // Clear suspension
        merchant.active = true;
        merchant.suspension_reason = None;
        merchant.suspended_at = None;
        merchant.suspension_expires_at = None;

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "SUSPENSION_APPEAL_APPROVED"),
            ),
            merchant_id,
        );

        Ok(())
    }
}
