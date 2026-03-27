use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, String};

#[contract]
pub struct MerchantRegistry;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Merchant {
    pub merchant_id: Address,
    pub business_name: String,
    pub settlement_currency: String,
    pub verified: bool,
    pub active: bool,
    pub created_at: u64,
}

#[contracttype]
pub enum DataKey {
    Merchant(Address),
    Admin,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    MerchantAlreadyExists = 1,
    MerchantNotFound = 2,
    Unauthorized = 3,
    NotVerified = 4,
    AdminAlreadySet = 5,
}

#[contractimpl]
impl MerchantRegistry {
    /// Initializes the Merchant Registry contract with an administrative address.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `admin`: The address to be set as the contract administrator.
    ///
    /// ### Errors
    /// - `Error::AdminAlreadySet`: If the administrator has already been initialized.
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().persistent().has(&DataKey::Admin) {
            return Err(Error::AdminAlreadySet);
        }
        env.storage().persistent().set(&DataKey::Admin, &admin);
        Ok(())
    }

    /// Registers a new merchant in the registry.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `merchant_id`: The address of the merchant to register.
    /// - `business_name`: The name of the merchant's business.
    /// - `settlement_currency`: The preferred currency for settlement.
    ///
    /// ### Authorization
    /// - Requires `merchant_id` to provide authentication.
    ///
    /// ### Errors
    /// - `Error::MerchantAlreadyExists`: If a merchant with the given address is already registered.
    pub fn register_merchant(
        env: Env,
        merchant_id: Address,
        business_name: String,
        settlement_currency: String,
    ) -> Result<(), Error> {
        merchant_id.require_auth();

        if env
            .storage()
            .persistent()
            .has(&DataKey::Merchant(merchant_id.clone()))
        {
            return Err(Error::MerchantAlreadyExists);
        }

        let merchant = Merchant {
            merchant_id: merchant_id.clone(),
            business_name,
            settlement_currency,
            verified: false,
            active: true,
            created_at: env.ledger().timestamp(),
        };

        env.storage()
            .persistent()
            .set(&DataKey::Merchant(merchant_id), &merchant);

        Ok(())
    }

    /// Updates the details of an existing merchant.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `merchant_id`: The address of the merchant to update.
    /// - `business_name`: Optional new business name.
    /// - `settlement_currency`: Optional new settlement currency.
    /// - `active`: Optional status indicating if the merchant is active.
    ///
    /// ### Authorization
    /// - Requires `merchant_id` to provide authentication.
    ///
    /// ### Errors
    /// - `Error::MerchantNotFound`: If the merchant is not found in the registry.
    pub fn update_merchant(
        env: Env,
        merchant_id: Address,
        business_name: Option<String>,
        settlement_currency: Option<String>,
        active: Option<bool>,
    ) -> Result<(), Error> {
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

        env.storage()
            .persistent()
            .set(&DataKey::Merchant(merchant_id), &merchant);

        Ok(())
    }

    /// Retrieves the information for a specific merchant.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `merchant_id`: The address of the merchant to retrieve.
    ///
    /// ### Returns
    /// - `Result<Merchant, Error>`: The merchant's data or an error if not found.
    pub fn get_merchant(env: Env, merchant_id: Address) -> Result<Merchant, Error> {
        Self::get_merchant_internal(&env, &merchant_id)
    }

    /// Verifies a merchant, allowing them to fulfill certain protocol requirements.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `admin`: The administrative address authorizing the verification.
    /// - `merchant_id`: The address of the merchant to verify.
    ///
    /// ### Authorization
    /// - Requires `admin` to provide authentication.
    /// - `admin` must match the stored contract administrator.
    ///
    /// ### Errors
    /// - `Error::Unauthorized`: If the caller is not the administrator.
    /// - `Error::MerchantNotFound`: If the merchant is not found in the registry.
    pub fn verify_merchant(env: Env, admin: Address, merchant_id: Address) -> Result<(), Error> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .ok_or(Error::Unauthorized)?;

        if admin != stored_admin {
            return Err(Error::Unauthorized);
        }

        let mut merchant = Self::get_merchant_internal(&env, &merchant_id)?;
        merchant.verified = true;

        env.storage()
            .persistent()
            .set(&DataKey::Merchant(merchant_id), &merchant);

        Ok(())
    }

    // Helper functions
    fn get_merchant_internal(env: &Env, merchant_id: &Address) -> Result<Merchant, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Merchant(merchant_id.clone()))
            .ok_or(Error::MerchantNotFound)
    }
}
