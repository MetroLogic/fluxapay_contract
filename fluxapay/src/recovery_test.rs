//! Issue #1173: full admin key compromise recovery drill.
#![cfg(test)]

use crate::access_control::{
    AccessControl, AccessControlDataKey, AccessControlError, RECOVERY_LOCK_IN_SECS,
    RECOVERY_TIMELOCK_LEDGERS,
};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env};

#[contract]
pub struct RecoveryHarness;

#[contractimpl]
impl RecoveryHarness {
    pub fn initialize_with_recovery(env: Env, admin: Address, recovery: Address) {
        AccessControl::initialize_with_recovery(&env, admin, recovery);
    }

    pub fn set_recovery_key(
        env: Env,
        admin: Address,
        recovery: Address,
    ) -> Result<(), AccessControlError> {
        AccessControl::set_recovery_key(&env, admin, recovery)
    }

    pub fn recovery_initiate_admin_transfer(
        env: Env,
        recovery: Address,
        new_admin: Address,
    ) -> Result<(), AccessControlError> {
        AccessControl::recovery_initiate_admin_transfer(&env, recovery, new_admin)
    }

    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), AccessControlError> {
        AccessControl::accept_admin(&env, new_admin)
    }

    pub fn get_admin(env: Env) -> Option<Address> {
        AccessControl::get_admin(&env)
    }

    pub fn get_recovery_key(env: Env) -> Option<Address> {
        AccessControl::get_recovery_key(&env)
    }

    pub fn get_pending_admin_transfer(env: Env) -> Option<(Address, u32)> {
        AccessControl::get_pending_admin_transfer(&env)
    }

    pub fn get_admin_transfer_lock_in(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&AccessControlDataKey::AdminTransferLockIn)
            .unwrap_or(0)
    }
}

#[test]
fn test_full_admin_key_compromise_recovery_transfer() {
    let env = Env::default();
    env.mock_all_auths();
    let contract = env.register(RecoveryHarness, ());
    let client = crate::RecoveryHarnessClient::new(&env, &contract);

    let compromised_admin = Address::generate(&env);
    let recovery_key = Address::generate(&env);
    let rotated_recovery_key = Address::generate(&env);
    let new_admin = Address::generate(&env);

    // Owner provisions a recovery key when the contract is initialized.
    client.initialize_with_recovery(&compromised_admin, &recovery_key);
    assert_eq!(client.get_recovery_key(), Some(recovery_key.clone()));
    assert_eq!(client.get_admin_transfer_lock_in(), RECOVERY_LOCK_IN_SECS);
    assert_eq!(client.get_admin(), Some(compromised_admin.clone()));

    // Before a compromise, the admin can rotate the recovery key.
    client.set_recovery_key(&compromised_admin, &rotated_recovery_key);
    assert_eq!(client.get_recovery_key(), Some(rotated_recovery_key.clone()));

    // The compromised admin key is no longer trusted; the recovery key proposes
    // a new admin, starting the 30-day lock-in (both seconds and ledgers).
    let start_sequence = env.ledger().sequence();
    client.recovery_initiate_admin_transfer(&rotated_recovery_key, &new_admin);
    assert_eq!(
        client.get_pending_admin_transfer(),
        Some((new_admin.clone(), start_sequence + RECOVERY_TIMELOCK_LEDGERS))
    );
    assert_eq!(client.get_admin_transfer_lock_in(), RECOVERY_LOCK_IN_SECS);

    // Cooldown: early acceptance is rejected; so is acceptance one ledger short.
    assert!(client.try_accept_admin(&new_admin).is_err());
    env.ledger()
        .set_sequence_number(start_sequence + RECOVERY_TIMELOCK_LEDGERS - 1);
    assert!(client.try_accept_admin(&new_admin).is_err());
    assert_eq!(client.get_admin(), Some(compromised_admin));

    // After the full 30-day lock-in the new admin accepts and old roles are gone.
    env.ledger()
        .set_sequence_number(start_sequence + RECOVERY_TIMELOCK_LEDGERS);
    client.accept_admin(&new_admin);
    assert_eq!(client.get_admin(), Some(new_admin));
    assert_eq!(client.get_pending_admin_transfer(), None);
    // Recovery key survives the transfer for the next incident.
    assert_eq!(client.get_recovery_key(), Some(rotated_recovery_key));
}
