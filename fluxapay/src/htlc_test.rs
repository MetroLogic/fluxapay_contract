//! Tests for the HTLC escrow module (`htlc.rs`), including the Issue #1021
//! permissionless timeout-sweep entry point `claim_htlc_timeout`.

use crate::{Htlc, HtlcClient, HtlcError, HtlcStatus};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, Bytes, BytesN, Env, String,
};

fn setup(env: &Env) -> HtlcClient<'_> {
    let contract_id = env.register(Htlc, ());
    HtlcClient::new(env, &contract_id)
}

/// Registers a Stellar asset token and mints `amount` to `holder`.
fn setup_token(env: &Env, holder: &Address, amount: i128) -> Address {
    let token_admin = Address::generate(env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let token_admin_client = token::StellarAssetClient::new(env, &token_id);
    token_admin_client.mint(holder, &amount);
    token_id
}

struct Fixture<'a> {
    client: HtlcClient<'a>,
    token_id: Address,
    alice: Address, // sender
    bob: Address,   // receiver
    amount: i128,
    secret: Bytes,
    hashlock: BytesN<32>,
    timeout_ledger: u32,
    htlc_id: String,
}

/// Creates a contract, a funded token, and one Active HTLC from `alice` to
/// `bob` with a 100-ledger timeout. `env` must outlive the returned fixture.
fn create_fixture(env: &Env) -> Fixture<'_> {
    env.mock_all_auths();
    let client = setup(env);

    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let amount = 10_000_000i128;
    let token_id = setup_token(env, &alice, amount);

    let secret = Bytes::from_array(env, &[1, 2, 3, 4]);
    let hashlock: BytesN<32> = env.crypto().sha256(&secret).to_bytes();
    let timeout_ledger = env.ledger().sequence() + 100;

    let htlc_id = client.create_htlc(&alice, &bob, &token_id, &amount, &hashlock, &timeout_ledger);

    Fixture {
        client,
        token_id,
        alice,
        bob,
        amount,
        secret,
        hashlock,
        timeout_ledger,
        htlc_id,
    }
}

#[test]
fn test_create_locks_tokens_in_escrow() {
    let env = Env::default();
    let f = create_fixture(&env);
    let token_client = token::Client::new(&env, &f.token_id);

    assert_eq!(token_client.balance(&f.alice), 0);
    assert_eq!(token_client.balance(&f.client.address), f.amount);

    let record = f.client.get_htlc(&f.htlc_id);
    assert_eq!(record.status, HtlcStatus::Active);
    assert_eq!(record.sender, f.alice);
    assert_eq!(record.receiver, f.bob);
    assert_eq!(record.amount, f.amount);
    assert_eq!(record.hashlock, f.hashlock);
    assert_eq!(record.timeout_ledger, f.timeout_ledger);
    assert!(record.resolved_at.is_none());
}

#[test]
fn test_create_htlc_rejects_non_positive_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let token_id = setup_token(&env, &alice, 100);
    let secret = Bytes::from_array(&env, &[1, 2, 3, 4]);
    let hashlock: BytesN<32> = env.crypto().sha256(&secret).to_bytes();

    let result = client.try_create_htlc(
        &alice,
        &bob,
        &token_id,
        &0i128,
        &hashlock,
        &(env.ledger().sequence() + 10),
    );
    assert_eq!(result, Err(Ok(HtlcError::InvalidAmount)));
}

#[test]
fn test_create_htlc_rejects_past_timeout() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let token_id = setup_token(&env, &alice, 100);
    let secret = Bytes::from_array(&env, &[1, 2, 3, 4]);
    let hashlock: BytesN<32> = env.crypto().sha256(&secret).to_bytes();

    let result = client.try_create_htlc(
        &alice,
        &bob,
        &token_id,
        &100i128,
        &hashlock,
        &env.ledger().sequence(),
    );
    assert_eq!(result, Err(Ok(HtlcError::InvalidTimeout)));
}

#[test]
fn test_claim_htlc_happy_path_releases_to_receiver() {
    let env = Env::default();
    let f = create_fixture(&env);
    let token_client = token::Client::new(&env, &f.token_id);

    f.client.claim_htlc(&f.htlc_id, &f.bob, &f.secret);

    assert_eq!(token_client.balance(&f.bob), f.amount);
    assert_eq!(token_client.balance(&f.client.address), 0);

    let record = f.client.get_htlc(&f.htlc_id);
    assert_eq!(record.status, HtlcStatus::Claimed);
    assert!(record.resolved_at.is_some());
}

#[test]
fn test_claim_after_timeout_fails() {
    let env = Env::default();
    let f = create_fixture(&env);
    env.ledger().set_sequence(f.timeout_ledger);

    let result = f.client.try_claim_htlc(&f.htlc_id, &f.bob, &f.secret);
    assert_eq!(result, Err(Ok(HtlcError::Expired)));
}

#[test]
fn test_claim_with_wrong_preimage_fails() {
    let env = Env::default();
    let f = create_fixture(&env);
    let wrong_secret = Bytes::from_array(&env, &[9, 9, 9, 9]);

    let result = f.client.try_claim_htlc(&f.htlc_id, &f.bob, &wrong_secret);
    assert_eq!(result, Err(Ok(HtlcError::PreimageMismatch)));
}

#[test]
fn test_claim_by_non_receiver_fails() {
    let env = Env::default();
    let f = create_fixture(&env);
    let mallory = Address::generate(&env);

    let result = f.client.try_claim_htlc(&f.htlc_id, &mallory, &f.secret);
    assert_eq!(result, Err(Ok(HtlcError::Unauthorized)));
}

#[test]
fn test_refund_before_timeout_fails() {
    let env = Env::default();
    let f = create_fixture(&env);

    let result = f.client.try_refund_htlc(&f.htlc_id, &f.alice);
    assert_eq!(result, Err(Ok(HtlcError::NotExpired)));
}

#[test]
fn test_refund_after_timeout_returns_funds_to_sender() {
    let env = Env::default();
    let f = create_fixture(&env);
    env.ledger().set_sequence(f.timeout_ledger);
    let token_client = token::Client::new(&env, &f.token_id);

    f.client.refund_htlc(&f.htlc_id, &f.alice);

    assert_eq!(token_client.balance(&f.alice), f.amount);
    let record = f.client.get_htlc(&f.htlc_id);
    assert_eq!(record.status, HtlcStatus::Refunded);
}

#[test]
fn test_refund_by_non_sender_fails() {
    let env = Env::default();
    let f = create_fixture(&env);
    env.ledger().set_sequence(f.timeout_ledger);
    let mallory = Address::generate(&env);

    let result = f.client.try_refund_htlc(&f.htlc_id, &mallory);
    assert_eq!(result, Err(Ok(HtlcError::Unauthorized)));
}

#[test]
fn test_claim_or_refund_after_claim_fails() {
    let env = Env::default();
    let f = create_fixture(&env);
    f.client.claim_htlc(&f.htlc_id, &f.bob, &f.secret);

    let claim_again = f.client.try_claim_htlc(&f.htlc_id, &f.bob, &f.secret);
    assert_eq!(claim_again, Err(Ok(HtlcError::AlreadyClaimed)));

    env.ledger().set_sequence(f.timeout_ledger);
    let refund_after_claim = f.client.try_refund_htlc(&f.htlc_id, &f.alice);
    assert_eq!(refund_after_claim, Err(Ok(HtlcError::AlreadyClaimed)));
}

// ---- Issue #1021: claim_htlc_timeout (permissionless sweep) ----

#[test]
fn test_claim_htlc_timeout_before_expiry_fails() {
    let env = Env::default();
    let f = create_fixture(&env);

    let result = f.client.try_claim_htlc_timeout(&f.htlc_id);
    assert_eq!(result, Err(Ok(HtlcError::NotExpired)));
}

#[test]
fn test_claim_htlc_timeout_at_exact_ledger_succeeds_for_any_caller() {
    let env = Env::default();
    let f = create_fixture(&env);
    // Exactly at timeout_ledger, not past it - the boundary must succeed.
    env.ledger().set_sequence(f.timeout_ledger);
    let token_client = token::Client::new(&env, &f.token_id);

    // No caller argument at all, matching the issue's
    // `pub fn claim_htlc_timeout(env: Env, htlc_id: String)` signature -
    // anyone (or no one in particular) can trigger it. Funds must land on
    // the original sender (alice), not on whoever submitted the call.
    f.client.claim_htlc_timeout(&f.htlc_id);

    assert_eq!(token_client.balance(&f.alice), f.amount);
    assert_eq!(token_client.balance(&f.client.address), 0);

    let record = f.client.get_htlc(&f.htlc_id);
    assert_eq!(record.status, HtlcStatus::Refunded);
    assert!(record.resolved_at.is_some());
}

#[test]
fn test_claim_htlc_timeout_called_twice_fails() {
    let env = Env::default();
    let f = create_fixture(&env);
    env.ledger().set_sequence(f.timeout_ledger);

    f.client.claim_htlc_timeout(&f.htlc_id);
    let second = f.client.try_claim_htlc_timeout(&f.htlc_id);
    assert_eq!(second, Err(Ok(HtlcError::AlreadyClaimed)));
}

#[test]
fn test_claim_htlc_timeout_does_not_allow_double_spend_with_refund_htlc() {
    let env = Env::default();
    let f = create_fixture(&env);
    env.ledger().set_sequence(f.timeout_ledger);
    let token_client = token::Client::new(&env, &f.token_id);

    f.client.claim_htlc_timeout(&f.htlc_id);
    assert_eq!(token_client.balance(&f.alice), f.amount);

    // A subsequent manual refund attempt by the sender must not pay out
    // a second time.
    let result = f.client.try_refund_htlc(&f.htlc_id, &f.alice);
    assert_eq!(result, Err(Ok(HtlcError::AlreadyClaimed)));
    assert_eq!(token_client.balance(&f.alice), f.amount);
}

#[test]
fn test_claim_htlc_timeout_on_unknown_id_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup(&env);

    let result = client.try_claim_htlc_timeout(&String::from_str(&env, "htlc_999"));
    assert_eq!(result, Err(Ok(HtlcError::NotFound)));
}
