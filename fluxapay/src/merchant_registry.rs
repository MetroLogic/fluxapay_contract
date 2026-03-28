use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, vec, Address, Env, String, Symbol, Vec,
};

use crate::RefundManagerClient;

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
    pub active: bool,
    pub created_at: u64,
}

#[contracttype]
pub enum MerchantDataKey {
    Merchant(Address),
    Admin,
    /// Stores the list of all registered merchants for enumeration
    MerchantList,
    /// Optional: Address of the RefundManager contract for automatic MERCHANT role granting
    RefundManagerAddress,
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
}

#[contractimpl]
impl MerchantRegistry {
    /// Initialize the contract with an admin address
    pub fn merchant_initialize(env: Env, admin: Address) -> Result<(), MerchantError> {
        if env.storage().persistent().has(&MerchantDataKey::Admin) {
            return Err(MerchantError::AdminAlreadySet);
        }
        env.storage().persistent().set(&MerchantDataKey::Admin, &admin);
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
            active: true,
            created_at: env.ledger().timestamp(),
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
            merchant.payout_address = Some(addr);
        }
        if let Some(acct) = bank_account {
            merchant.bank_account = Some(acct);
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

    /// Get merchant info
    pub fn get_merchant(env: Env, merchant_id: Address) -> Result<Merchant, MerchantError> {
        Self::get_merchant_internal(&env, &merchant_id)
    }

    /// Verify merchant (admin only) — sets KycTier::Basic for backward compatibility.
    /// If a RefundManager address is configured, also grants the MERCHANT role there.
    pub fn verify_merchant(env: Env, admin: Address, merchant_id: Address) -> Result<(), MerchantError> {
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

        env.storage()
            .persistent()
            .set(&MerchantDataKey::Merchant(merchant_id.clone()), &merchant);

        if let Some(refund_manager_addr) = env
            .storage()
            .persistent()
            .get::<MerchantDataKey, Address>(&MerchantDataKey::RefundManagerAddress)
        {
            let rm_client = RefundManagerClient::new(&env, &refund_manager_addr);
            let _ = rm_client.try_refund_grant_role(
                &admin,
                &Symbol::new(&env, "MERCHANT"),
                &merchant_id,
            );
        }

        env.events().publish(
            (Symbol::new(&env, "MERCHANT"), Symbol::new(&env, "VERIFIED")),
            merchant_id,
        );

        Ok(())
    }

    /// Set a specific KYC tier for a merchant (admin only).
    pub fn set_kyc_tier(
        env: Env,
        admin: Address,
        merchant_id: Address,
        tier: KycTier,
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

        env.storage().persistent().set(
            &MerchantDataKey::RefundManagerAddress,
            &refund_manager,
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
        let start = offset;
        let end = core::cmp::min(merchant_ids.len(), start.saturating_add(limit));

        let mut i = start;
        while i < end {
            if let Some(merchant_id) = merchant_ids.get(i) {
                if let Ok(merchant) = Self::get_merchant_internal(&env, &merchant_id) {
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
            if let Ok(merchant) = Self::get_merchant_internal(&env, &merchant_id) {
                if merchant.kyc_tier != KycTier::Unverified {
                    result.push_back(merchant);
                }
            }
        }

        result
    }

    fn add_to_merchant_list(env: &Env, merchant_id: &Address) {
        let key = MerchantDataKey::MerchantList;
        let mut merchants: Vec<Address> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![env]);

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

    fn get_merchant_internal(env: &Env, merchant_id: &Address) -> Result<Merchant, MerchantError> {
        env.storage()
            .persistent()
            .get(&MerchantDataKey::Merchant(merchant_id.clone()))
            .ok_or(MerchantError::MerchantNotFound)
    }
}
