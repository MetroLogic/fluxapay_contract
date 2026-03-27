use soroban_sdk::{contracterror, contracttype, Address, Env, Symbol};

/// Returns the symbol for the administrator role.
///
/// ### Parameters
/// - `env`: The Soroban environment.
pub fn role_admin(env: &Env) -> Symbol {
    Symbol::new(env, "ADMIN")
}

/// Returns the symbol for the oracle role.
///
/// ### Parameters
/// - `env`: The Soroban environment.
pub fn role_oracle(env: &Env) -> Symbol {
    Symbol::new(env, "ORACLE")
}

/// Returns the symbol for the merchant role.
///
/// ### Parameters
/// - `env`: The Soroban environment.
#[allow(dead_code)]
pub fn role_merchant(env: &Env) -> Symbol {
    Symbol::new(env, "MERCHANT")
}

/// Returns the symbol for the settlement operator role.
///
/// ### Parameters
/// - `env`: The Soroban environment.
pub fn role_settlement_operator(env: &Env) -> Symbol {
    Symbol::new(env, "SETTLEMENT_OPERATOR")
}

#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccessControlError {
    Unauthorized = 1,
    RoleAlreadyGranted = 2,
    RoleNotGranted = 3,
    CannotRenounceAdmin = 4,
    InvalidAdmin = 5,
}

#[contracttype]
pub enum AccessControlDataKey {
    Role(Symbol, Address),
    Admin,
}

pub struct AccessControl;

impl AccessControl {
    /// Initializes the Access Control state with an initial administrator.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `admin`: The address to be granted the initial administrator role.
    pub fn initialize(env: &Env, admin: Address) {
        env.storage()
            .persistent()
            .set(&AccessControlDataKey::Admin, &admin);
        Self::grant_role_internal(env, &role_admin(env), &admin);
    }

    /// Grants a specific role to an account.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `admin`: The administrative address authorizing the grant.
    /// - `role`: The symbol of the role to grant.
    /// - `account`: The address to receive the role.
    ///
    /// ### Authorization
    /// - Requires `admin` to provide authentication.
    /// - `admin` must have the administrator role.
    ///
    /// ### Errors
    /// - `AccessControlError::Unauthorized`: If the caller is not an administrator.
    /// - `AccessControlError::RoleAlreadyGranted`: If the account already has the role.
    pub fn grant_role(
        env: &Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), AccessControlError> {
        admin.require_auth();
        if !Self::has_role(env, &role_admin(env), &admin) {
            return Err(AccessControlError::Unauthorized);
        }

        if Self::has_role(env, &role, &account) {
            return Err(AccessControlError::RoleAlreadyGranted);
        }

        Self::grant_role_internal(env, &role, &account);
        Ok(())
    }

    /// Revokes a specific role from an account.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `admin`: The administrative address authorizing the revocation.
    /// - `role`: The symbol of the role to revoke.
    /// - `account`: The address to lose the role.
    ///
    /// ### Authorization
    /// - Requires `admin` to provide authentication.
    /// - `admin` must have the administrator role.
    ///
    /// ### Errors
    /// - `AccessControlError::Unauthorized`: If the caller is not an administrator.
    /// - `AccessControlError::RoleNotGranted`: If the account does not have the role.
    pub fn revoke_role(
        env: &Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), AccessControlError> {
        admin.require_auth();
        if !Self::has_role(env, &role_admin(env), &admin) {
            return Err(AccessControlError::Unauthorized);
        }

        if !Self::has_role(env, &role, &account) {
            return Err(AccessControlError::RoleNotGranted);
        }

        Self::revoke_role_internal(env, &role, &account);
        Ok(())
    }

    /// Checks if an account has a specific role.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `role`: The symbol of the role to check.
    /// - `account`: The address to check for the role.
    ///
    /// ### Returns
    /// - `bool`: True if the account has the role, false otherwise.
    pub fn has_role(env: &Env, role: &Symbol, account: &Address) -> bool {
        env.storage()
            .persistent()
            .get(&AccessControlDataKey::Role(role.clone(), account.clone()))
            .unwrap_or(false)
    }

    /// Allows an account to renounce a role they currently hold.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `account`: The address renouncing the role.
    /// - `role`: The symbol of the role to renounce.
    ///
    /// ### Errors
    /// - `AccessControlError::CannotRenounceAdmin`: If the account tries to renounce the admin role.
    /// - `AccessControlError::RoleNotGranted`: If the account does not have the role.
    pub fn renounce_role(
        env: &Env,
        account: Address,
        role: Symbol,
    ) -> Result<(), AccessControlError> {
        if role == role_admin(env) {
            return Err(AccessControlError::CannotRenounceAdmin);
        }

        if !Self::has_role(env, &role, &account) {
            return Err(AccessControlError::RoleNotGranted);
        }

        Self::revoke_role_internal(env, &role, &account);
        Ok(())
    }

    /// Transfers the administrator role to a new address.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `current_admin`: The current administrative address.
    /// - `new_admin`: The new address to become the administrator.
    ///
    /// ### Authorization
    /// - Requires `current_admin` to provide authentication.
    /// - `current_admin` must have the administrator role.
    ///
    /// ### Errors
    /// - `AccessControlError::Unauthorized`: If the caller is not the administrator.
    pub fn transfer_admin(
        env: &Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), AccessControlError> {
        current_admin.require_auth();
        if !Self::has_role(env, &role_admin(env), &current_admin) {
            return Err(AccessControlError::Unauthorized);
        }

        Self::revoke_role_internal(env, &role_admin(env), &current_admin);
        Self::grant_role_internal(env, &role_admin(env), &new_admin);

        env.storage()
            .persistent()
            .set(&AccessControlDataKey::Admin, &new_admin);

        Ok(())
    }

    /// Returns the address of the current administrator.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    ///
    /// ### Returns
    /// - `Option<Address>`: The administrator address if it has been set.
    pub fn get_admin(env: &Env) -> Option<Address> {
        env.storage().persistent().get(&AccessControlDataKey::Admin)
    }

    /// Reverts if the account does not have the specified role.
    ///
    /// ### Parameters
    /// - `env`: The Soroban environment.
    /// - `role`: The symbol of the role to require.
    /// - `account`: The address to check for the role.
    ///
    /// ### Errors
    /// - `AccessControlError::Unauthorized`: If the account does not have the role.
    #[allow(dead_code)]
    pub fn require_role(
        env: &Env,
        role: &Symbol,
        account: &Address,
    ) -> Result<(), AccessControlError> {
        if !Self::has_role(env, role, account) {
            return Err(AccessControlError::Unauthorized);
        }
        Ok(())
    }

    fn grant_role_internal(env: &Env, role: &Symbol, account: &Address) {
        env.storage().persistent().set(
            &AccessControlDataKey::Role(role.clone(), account.clone()),
            &true,
        );
    }

    fn revoke_role_internal(env: &Env, role: &Symbol, account: &Address) {
        env.storage()
            .persistent()
            .remove(&AccessControlDataKey::Role(role.clone(), account.clone()));
    }
}
