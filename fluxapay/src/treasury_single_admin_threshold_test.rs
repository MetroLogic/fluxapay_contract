//! Tests for issue #1013's missing piece: `withdraw_treasury` previously let
//! any single admin withdraw *any* amount, completely bypassing the M-of-N
//! multisig governance that `configure_treasury_multisig` /
//! `propose_treasury_withdrawal` already implemented. These tests cover the
//! new configurable single-admin threshold and signer management added to
//! close that gap, on `crate::refund_manager::RefundManager` (the contract
//! `treasury_multisig_test.rs` already exercises the rest of the multisig
//! flow against).

use crate::refund_manager::{RefundManager, RefundManagerClient};
use crate::types::TreasuryMultisigConfig;
use crate::Error;
use soroban_sdk::{testutils::Address as _, vec, Address, Env};

fn setup(env: &Env) -> (Address, RefundManagerClient<'_>, Address) {
    let admin = Address::generate(env);
    let usdc_admin = Address::generate(env);
    let usdc_addr = env.register_stellar_asset_contract(usdc_admin);

    let refund_manager_addr = env.register_contract(None, RefundManager);
    let client = RefundManagerClient::new(env, &refund_manager_addr);
    client.initialize_refund_manager(&admin, &usdc_addr).unwrap();

    (admin, client, usdc_addr)
}

#[test]
fn test_default_single_admin_threshold_is_1000_usdc() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client, _usdc) = setup(&env);

    // 1000 USDC at 7 decimals, matching the issue's stated default.
    assert_eq!(
        client.get_treasury_single_admin_threshold(),
        10_000_000_000i128
    );
}

#[test]
fn test_withdrawal_above_threshold_rejected_without_touching_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client, _usdc) = setup(&env);

    let destination = Address::generate(&env);
    // Above the 1000 USDC default threshold; must be rejected by the
    // threshold gate before even checking the (here: empty) treasury
    // balance, so this is deterministic without any token funding.
    let result =
        client.try_withdraw_treasury(&admin, &20_000_000_000i128, &destination);
    assert_eq!(
        result,
        Err(Ok(Error::TreasuryExceedsSingleAdminThreshold))
    );
}

#[test]
fn test_withdrawal_at_or_below_threshold_passes_the_gate() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client, _usdc) = setup(&env);

    let destination = Address::generate(&env);
    // Small amount, well under the default threshold: the threshold gate
    // must let it through, so the *next* check (insufficient treasury
    // balance, since nothing was funded) is what rejects it — proving the
    // gate isn't blocking small withdrawals.
    let result = client.try_withdraw_treasury(&admin, &1_000_000i128, &destination);
    assert_eq!(result, Err(Ok(Error::InsufficientTreasuryBalance)));
}

#[test]
fn test_configurable_threshold_blocks_previously_allowed_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client, _usdc) = setup(&env);

    // Lower the threshold to 500 stroops.
    client.set_treasury_single_admin_threshold(&admin, &500i128);
    assert_eq!(client.get_treasury_single_admin_threshold(), 500i128);

    let destination = Address::generate(&env);
    let result = client.try_withdraw_treasury(&admin, &600i128, &destination);
    assert_eq!(
        result,
        Err(Ok(Error::TreasuryExceedsSingleAdminThreshold))
    );
}

#[test]
fn test_non_admin_cannot_set_threshold() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, client, _usdc) = setup(&env);

    let not_admin = Address::generate(&env);
    let result = client.try_set_treasury_single_admin_threshold(&not_admin, &1i128);
    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn test_add_and_remove_treasury_signer() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client, _usdc) = setup(&env);

    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);
    client.configure_treasury_multisig(
        &admin,
        &2,
        &vec![&env, signer1.clone(), signer2.clone()],
        &86400,
        &172800,
    );

    let signer3 = Address::generate(&env);
    client.add_treasury_signer(&admin, &signer3);
    let config: TreasuryMultisigConfig = client.get_treasury_multisig_config().unwrap();
    assert_eq!(config.admin_signers.len(), 3);
    assert!(config.admin_signers.iter().any(|s| s == signer3));

    // Removing down to 2 signers is still >= required_approvals (2), so it succeeds.
    client.remove_treasury_signer(&admin, &signer1);
    let config: TreasuryMultisigConfig = client.get_treasury_multisig_config().unwrap();
    assert_eq!(config.admin_signers.len(), 2);
    assert!(!config.admin_signers.iter().any(|s| s == signer1));
}

#[test]
fn test_remove_signer_rejected_if_quorum_becomes_unreachable() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client, _usdc) = setup(&env);

    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);
    // required_approvals == signer count, so removing anyone breaks quorum.
    client.configure_treasury_multisig(
        &admin,
        &2,
        &vec![&env, signer1.clone(), signer2.clone()],
        &86400,
        &172800,
    );

    let result = client.try_remove_treasury_signer(&admin, &signer1);
    assert_eq!(result, Err(Ok(Error::InvalidTreasuryThreshold)));
}

#[test]
fn test_remove_nonexistent_signer_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client, _usdc) = setup(&env);

    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);
    let stranger = Address::generate(&env);
    client.configure_treasury_multisig(
        &admin,
        &1,
        &vec![&env, signer1.clone(), signer2.clone()],
        &86400,
        &172800,
    );

    let result = client.try_remove_treasury_signer(&admin, &stranger);
    assert_eq!(result, Err(Ok(Error::NotAuthorizedTreasurySigner)));
}
