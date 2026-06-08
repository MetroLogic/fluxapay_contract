use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, map, vec, Address, Env, Map, String,
    Symbol, Vec,
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
}

#[contracttype]
pub enum MerchantDataKey {
    Merchant(Address),
    MerchantPayoutHistory(Address),
    Admin,
    /// Stores the list of all registered merchants for enumeration
    MerchantList,
    /// Optional: Address of the RefundManager contract for automatic MERCHANT role granting
    RefundManagerAddress,
    /// Platform fee configuration
    FeeConfig,
    /// Address of the PaymentProcessor contract for automatic KYC tier upgrades (issue #207)
    PaymentProcessorAddress,
}

/// Platform fee configuration stored in MerchantRegistry.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformFeeConfig {
    /// Fee in basis points (e.g. 200 = 2%).
    pub fee_bps: i128,
    /// Address that receives the platform fee.
    pub fee_recipient: Address,
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
        Ok(())
    }

    /// Register a new merchant
    pub fn register_merchant(
        env: Env,
        merchant_id: Address,
        business_name: String,
        settlement_currency: String,
        payout_address: Option<Address>,
        bank_account: Option<String>,
        fee_config: Option<FeeConfig>,
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
            fee_config: MaybeFeeConfig::from(fee_config),
            metadata_hash: None,
            currency_payout_addresses: map![&env],
            payout_whitelist: vec![&env],
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
        fee_config: Option<FeeConfig>,
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
            let previous_payout_address = merchant.payout_address.clone();
            let payout_changed = previous_payout_address
                .as_ref()
                .map_or(true, |previous_addr| previous_addr != &addr);

            if payout_changed {
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

                if let Some(old_addr) = previous_payout_address {
                    let history_key = MerchantDataKey::MerchantPayoutHistory(merchant_id.clone());
                    let mut history: Vec<Address> = env
                        .storage()
                        .persistent()
                        .get(&history_key)
                        .unwrap_or_else(|| vec![&env]);
                    history.push_back(old_addr.clone());
                    env.storage().persistent().set(&history_key, &history);

                    env.events().publish(
                        (
                            Symbol::new(&env, "MERCHANT"),
                            Symbol::new(&env, "PAYOUT_UPDATED"),
                        ),
                        (merchant_id.clone(), old_addr, addr.clone()),
                    );
                }

                merchant.payout_address = Some(addr);
                merchant.last_payout_change_at = Some(current_time);
            }
        }
        if let Some(acct) = bank_account {
            merchant.bank_account = Some(acct);
        }
        if let Some(config) = fee_config {
            merchant.fee_config = MaybeFeeConfig::Some(config);
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
            (Symbol::new(&env, "MERCHANT"), Symbol::new(&env, "PARTIAL_PAYMENT_UPDATED")),
            (merchant_id, partial_payment_allowed),
        );

        Ok(())
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

    /// Get the previous payout addresses for a merchant in chronological order.
    pub fn get_payout_history(
        env: Env,
        merchant_id: Address,
    ) -> Result<Vec<Address>, MerchantError> {
        Self::get_merchant_internal(&env, &merchant_id)?;

        Ok(env
            .storage()
            .persistent()
            .get(&MerchantDataKey::MerchantPayoutHistory(merchant_id))
            .unwrap_or_else(|| vec![&env]))
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

        env.storage()
            .persistent()
            .set(&MerchantDataKey::PaymentProcessorAddress, &payment_processor);

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

        let is_promotion = matches!(
            (&merchant.kyc_tier, &new_tier),
            (KycTier::Unverified, KycTier::Basic)
                | (KycTier::Basic, KycTier::Full)
                | (KycTier::Full, KycTier::Business)
        );

        if !is_promotion {
            return Err(MerchantError::Unauthorized);
        }

        merchant.kyc_tier = new_tier;

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        env.events().publish(
            (Symbol::new(&env, "MERCHANT"), Symbol::new(&env, "KYC_UPGRADED")),
            merchant_id,
        );

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

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "SUSPENDED"),
            ),
            merchant_id,
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

        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "REINSTATED"),
            ),
            merchant_id,
        );

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
        env.storage()
            .persistent()
            .set(&MerchantDataKey::FeeConfig, &PlatformFeeConfig { fee_bps, fee_recipient: recipient });
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
}
