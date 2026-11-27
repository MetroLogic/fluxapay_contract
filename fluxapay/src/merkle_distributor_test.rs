//! Tests for the Merkle batch payment distributor (issues #1026 / #1027).
//!
//! Covers the acceptance criteria from both (duplicate) issues: batch
//! creation/funding, proof verification against a real multi-leaf tree,
//! replay protection, malformed/tampered proof rejection, and
//! expiry + defund.

use crate::merkle_distributor::{compute_merkle_root, leaf_hash, node_hash};
use crate::{Error, PaymentProcessor, PaymentProcessorClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    token, Address, BytesN, Env, Vec,
};

fn setup(env: &Env) -> (Address, PaymentProcessorClient<'_>) {
    let processor_addr = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(env, &processor_addr);
    let admin = Address::generate(env);
    client.initialize_payment_processor(&admin);
    (processor_addr, client)
}

fn setup_token(env: &Env, funder: &Address, amount: i128) -> (token::Client<'_>, Address) {
    let token_admin = Address::generate(env);
    let token_addr = env.register_stellar_asset_contract(token_admin);
    let asset_client = token::StellarAssetClient::new(env, &token_addr);
    asset_client.mint(funder, &amount);
    (token::Client::new(env, &token_addr), token_addr)
}

/// Build a Merkle tree over `(index, recipient, amount)` leaves using the
/// contract's own `leaf_hash`/`node_hash` so proofs are guaranteed
/// compatible with `claim_batch_payment`. Mirrors the pairing strategy
/// `sdk/src/merkle.ts` uses off-chain (promote an odd trailing node
/// unchanged to the next level, so it needs no proof entry at that level).
fn build_tree(
    env: &Env,
    token: &Address,
    recipients: &[Address],
    amounts: &[i128],
) -> (BytesN<32>, Vec<Vec<BytesN<32>>>) {
    let n = recipients.len();
    let mut layer: Vec<BytesN<32>> = Vec::new(env);
    for i in 0..n {
        layer.push_back(leaf_hash(env, i as u32, &recipients[i], amounts[i], token));
    }

    let mut layers: Vec<Vec<BytesN<32>>> = Vec::new(env);
    layers.push_back(layer.clone());

    while layer.len() > 1 {
        let mut next: Vec<BytesN<32>> = Vec::new(env);
        let mut i = 0u32;
        while i < layer.len() {
            if i + 1 < layer.len() {
                next.push_back(node_hash(env, &layer.get(i).unwrap(), &layer.get(i + 1).unwrap()));
            } else {
                next.push_back(layer.get(i).unwrap());
            }
            i += 2;
        }
        layers.push_back(next.clone());
        layer = next;
    }

    let root = layer.get(0).unwrap();

    let mut proofs: Vec<Vec<BytesN<32>>> = Vec::new(env);
    for leaf_idx in 0..n {
        let mut proof: Vec<BytesN<32>> = Vec::new(env);
        let mut cur = leaf_idx as u32;
        let mut level = 0usize;
        while level + 1 < layers.len() {
            let this_layer = layers.get(level as u32).unwrap();
            let sibling_idx = if cur % 2 == 0 { cur + 1 } else { cur - 1 };
            if sibling_idx < this_layer.len() {
                proof.push_back(this_layer.get(sibling_idx).unwrap());
            }
            cur /= 2;
            level += 1;
        }
        proofs.push_back(proof);
    }

    (root, proofs)
}

#[test]
fn test_create_batch_and_all_five_recipients_claim() {
    let env = Env::default();
    env.mock_all_auths();
    let (_addr, client) = setup(&env);

    let creator = Address::generate(&env);
    let recipients: [Address; 5] = [
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    let amounts: [i128; 5] = [100, 200, 300, 400, 500];
    let total: i128 = amounts.iter().sum();

    let (token_client, token_addr) = setup_token(&env, &creator, total);
    let (root, proofs) = build_tree(&env, &token_addr, &recipients, &amounts);

    let now = env.ledger().timestamp();
    let distribution_id =
        client.create_batch(&creator, &token_addr, &root, &total, &5u32, &(now + 1000));
    assert_eq!(distribution_id, 1u64);
    assert_eq!(token_client.balance(&creator), 0);

    for i in 0..5usize {
        let proof = proofs.get(i as u32).unwrap();
        client.claim_batch_payment(
            &distribution_id,
            &(i as u32),
            &recipients[i],
            &amounts[i],
            &proof,
        );
        assert_eq!(token_client.balance(&recipients[i]), amounts[i]);
        assert!(client.is_batch_claimed(&distribution_id, &(i as u32)));
    }

    let batch = client.get_batch(&distribution_id);
    assert_eq!(batch.claimed_amount, total);
}

#[test]
fn test_double_claim_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_addr, client) = setup(&env);

    let creator = Address::generate(&env);
    let r0 = Address::generate(&env);
    let r1 = Address::generate(&env);
    let amounts: [i128; 2] = [100, 200];
    let (_, token_addr) = setup_token(&env, &creator, 300);
    let recipients: [Address; 2] = [r0.clone(), r1.clone()];
    let (root, proofs) = build_tree(&env, &token_addr, &recipients, &amounts);

    let now = env.ledger().timestamp();
    let distribution_id =
        client.create_batch(&creator, &token_addr, &root, &300i128, &2u32, &(now + 1000));

    let proof0 = proofs.get(0).unwrap();
    client.claim_batch_payment(&distribution_id, &0u32, &r0, &100i128, &proof0);

    let result = client.try_claim_batch_payment(&distribution_id, &0u32, &r0, &100i128, &proof0);
    assert_eq!(result, Err(Ok(Error::MerkleAlreadyClaimed)));
}

#[test]
fn test_tampered_proof_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_addr, client) = setup(&env);

    let creator = Address::generate(&env);
    let r0 = Address::generate(&env);
    let r1 = Address::generate(&env);
    let amounts: [i128; 2] = [100, 200];
    let (_, token_addr) = setup_token(&env, &creator, 300);
    let recipients: [Address; 2] = [r0.clone(), r1.clone()];
    let (root, proofs) = build_tree(&env, &token_addr, &recipients, &amounts);

    let now = env.ledger().timestamp();
    let distribution_id =
        client.create_batch(&creator, &token_addr, &root, &300i128, &2u32, &(now + 1000));

    // Flip a byte in the sibling hash: the recomputed root must no longer match.
    let real_proof = proofs.get(0).unwrap();
    let sibling = real_proof.get(0).unwrap();
    let mut tampered_bytes = sibling.to_array();
    tampered_bytes[0] ^= 0xFF;
    let tampered_sibling = BytesN::from_array(&env, &tampered_bytes);
    let mut tampered_proof: Vec<BytesN<32>> = Vec::new(&env);
    tampered_proof.push_back(tampered_sibling);

    let result =
        client.try_claim_batch_payment(&distribution_id, &0u32, &r0, &100i128, &tampered_proof);
    assert_eq!(result, Err(Ok(Error::InvalidMerkleProof)));
}

#[test]
fn test_wrong_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_addr, client) = setup(&env);

    let creator = Address::generate(&env);
    let r0 = Address::generate(&env);
    let r1 = Address::generate(&env);
    let amounts: [i128; 2] = [100, 200];
    let (_, token_addr) = setup_token(&env, &creator, 300);
    let recipients: [Address; 2] = [r0.clone(), r1.clone()];
    let (root, proofs) = build_tree(&env, &token_addr, &recipients, &amounts);

    let now = env.ledger().timestamp();
    let distribution_id =
        client.create_batch(&creator, &token_addr, &root, &300i128, &2u32, &(now + 1000));

    let proof0 = proofs.get(0).unwrap();
    // Claiming with a different amount than the leaf was built with changes
    // the recomputed leaf hash, so the root no longer matches.
    let result = client.try_claim_batch_payment(&distribution_id, &0u32, &r0, &999i128, &proof0);
    assert_eq!(result, Err(Ok(Error::InvalidMerkleProof)));
}

#[test]
fn test_empty_proof_against_multi_leaf_root_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_addr, client) = setup(&env);

    let creator = Address::generate(&env);
    let r0 = Address::generate(&env);
    let r1 = Address::generate(&env);
    let amounts: [i128; 2] = [100, 200];
    let (_, token_addr) = setup_token(&env, &creator, 300);
    let recipients: [Address; 2] = [r0.clone(), r1.clone()];
    let (root, _proofs) = build_tree(&env, &token_addr, &recipients, &amounts);

    let now = env.ledger().timestamp();
    let distribution_id =
        client.create_batch(&creator, &token_addr, &root, &300i128, &2u32, &(now + 1000));

    let empty_proof: Vec<BytesN<32>> = Vec::new(&env);
    let result =
        client.try_claim_batch_payment(&distribution_id, &0u32, &r0, &100i128, &empty_proof);
    assert_eq!(result, Err(Ok(Error::InvalidMerkleProof)));
}

#[test]
fn test_claim_after_expiry_rejected_then_defund_recovers_unclaimed() {
    let env = Env::default();
    env.mock_all_auths();
    let (_addr, client) = setup(&env);

    let creator = Address::generate(&env);
    let r0 = Address::generate(&env);
    let r1 = Address::generate(&env);
    let amounts: [i128; 2] = [100, 200];
    let (token_client, token_addr) = setup_token(&env, &creator, 300);
    let recipients: [Address; 2] = [r0.clone(), r1.clone()];
    let (root, proofs) = build_tree(&env, &token_addr, &recipients, &amounts);

    let now = env.ledger().timestamp();
    let expires_at = now + 100;
    let distribution_id =
        client.create_batch(&creator, &token_addr, &root, &300i128, &2u32, &expires_at);

    // One recipient claims before expiry.
    let proof0 = proofs.get(0).unwrap();
    client.claim_batch_payment(&distribution_id, &0u32, &r0, &100i128, &proof0);

    let ledger_info = env.ledger().get();
    env.ledger().set(LedgerInfo {
        timestamp: expires_at + 1,
        protocol_version: ledger_info.protocol_version,
        sequence_number: ledger_info.sequence_number,
        network_id: ledger_info.network_id,
        base_reserve: ledger_info.base_reserve,
    });

    // Second recipient is now too late to claim.
    let proof1 = proofs.get(1).unwrap();
    let late_claim = client.try_claim_batch_payment(&distribution_id, &1u32, &r1, &200i128, &proof1);
    assert_eq!(late_claim, Err(Ok(Error::MerkleDistributionExpired)));

    // Creator reclaims the unclaimed 200.
    let refunded = client.defund_batch(&creator, &distribution_id);
    assert_eq!(refunded, 200i128);
    assert_eq!(token_client.balance(&creator), 200i128);

    // Defunding twice is rejected.
    let second_defund = client.try_defund_batch(&creator, &distribution_id);
    assert_eq!(
        second_defund,
        Err(Ok(Error::MerkleDistributionAlreadyDefunded))
    );
}

#[test]
fn test_defund_before_expiry_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_addr, client) = setup(&env);

    let creator = Address::generate(&env);
    let r0 = Address::generate(&env);
    let amounts: [i128; 1] = [100];
    let (_, token_addr) = setup_token(&env, &creator, 100);
    let recipients: [Address; 1] = [r0];
    let (root, _proofs) = build_tree(&env, &token_addr, &recipients, &amounts);

    let now = env.ledger().timestamp();
    let distribution_id =
        client.create_batch(&creator, &token_addr, &root, &100i128, &1u32, &(now + 1000));

    let result = client.try_defund_batch(&creator, &distribution_id);
    assert_eq!(result, Err(Ok(Error::MerkleDistributionNotExpired)));
}

#[test]
fn test_invalid_leaf_count_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_addr, client) = setup(&env);

    let creator = Address::generate(&env);
    let (_, token_addr) = setup_token(&env, &creator, 100);
    let root = BytesN::from_array(&env, &[7u8; 32]);
    let now = env.ledger().timestamp();

    let result =
        client.try_create_batch(&creator, &token_addr, &root, &100i128, &0u32, &(now + 1000));
    assert_eq!(result, Err(Ok(Error::InvalidMerkleLeafCount)));
}

#[test]
fn test_compute_merkle_root_matches_build_tree_root() {
    // Direct unit check on the hashing primitives, independent of the
    // contract wiring: folding a built proof over `compute_merkle_root`
    // must reproduce the root `build_tree` computed.
    let env = Env::default();
    let token = Address::generate(&env);
    let recipients: [Address; 3] = [
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    let amounts: [i128; 3] = [10, 20, 30];
    let (root, proofs) = build_tree(&env, &token, &recipients, &amounts);

    for i in 0..3usize {
        let proof = proofs.get(i as u32).unwrap();
        let computed = compute_merkle_root(&env, i as u32, &recipients[i], amounts[i], &token, &proof);
        assert_eq!(computed, root);
    }
}
