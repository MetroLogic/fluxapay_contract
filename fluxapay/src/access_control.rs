use soroban_sdk::{contracterror, contracttype, Address, Env, Symbol};

pub fn role_admin(env: &Env) -> Symbol {
    Symbol::new(env, "ADMIN")
}

pub fn role_oracle(env: &Env) -> Symbol {
    Symbol::new(env, "ORACLE")
}

pub fn role_merchant(env: &Env) -> Symbol {
    Symbol::new(env, "MERCHANT")
}

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
    pub fn initialize(env: &Env, admin: Address) {
        env.storage()
            .persistent()
            .set(&AccessControlDataKey::Admin, &admin);
        Self::grant_role_internal(env, &role_admin(env), &admin);
    }

    pub fn grant_role(
        env: &Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), AccessControlError> {
        if !Self::has_role(env, &role_admin(env), &admin) {
            return Err(AccessControlError::Unauthorized);
        }

        if Self::has_role(env, &role, &account) {
            return Err(AccessControlError::RoleAlreadyGranted);
        }

        Self::grant_role_internal(env, &role, &account);
        Ok(())
    }

    pub fn revoke_role(
        env: &Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), AccessControlError> {
        if !Self::has_role(env, &role_admin(env), &admin) {
            return Err(AccessControlError::Unauthorized);
        }

        if !Self::has_role(env, &role, &account) {
            return Err(AccessControlError::RoleNotGranted);
        }

        Self::revoke_role_internal(env, &role, &account);
        Ok(())
    }

    pub fn has_role(env: &Env, role: &Symbol, account: &Address) -> bool {
        env.storage()
            .persistent()
            .get(&AccessControlDataKey::Role(role.clone(), account.clone()))
            .unwrap_or(false)
    }

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

    pub fn transfer_admin(
        env: &Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), AccessControlError> {
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

    pub fn get_admin(env: &Env) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&AccessControlDataKey::Admin)
    }

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
        env.storage()
            .persistent()
            .set(&AccessControlDataKey::Role(role.clone(), account.clone()), &true);
    }

    fn revoke_role_internal(env: &Env, role: &Symbol, account: &Address) {
        env.storage()
            .persistent()
            .remove(&AccessControlDataKey::Role(role.clone(), account.clone()));
    }
}
