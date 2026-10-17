//! On-chain Merkle batch payment proof verifier for bulk disbursements.
//!
//! Resolves issues #1026 and #1027. Both issues describe the exact same
//! feature — same proposed `claim_batch_payment` signature, same acceptance
//! criteria, same suggested branch/commit name — and are genuine duplicates
//! rather than split contract/indexer scopes. The contract entry points
//! (`create_batch`, `claim_batch_payment`, `defund_batch`, `get_batch`,
//! `is_batch_claimed`) live in `lib.rs`'s `impl PaymentProcessor` block —
//! alongside `create_payment` / `verify_payment` — so they become part of
//! the single generated `PaymentProcessorClient` rather than a second,
//! separate client type. This module holds the pure hashing/bitmap helpers
//! those methods call into.
//!
//! A payer ("creator") commits a single 32-byte Merkle root over N
//! `(index, recipient, amount)` allocations and funds the total amount in
//! one transaction (`create_batch`). Each recipient later proves inclusion
//! of their own leaf with an O(log N) sibling-hash proof and withdraws their
//! share (`claim_batch_payment`). This is dramatically cheaper than paying
//! every recipient in its own transaction for payroll/airdrop-style bulk
//! disbursements.
//!
//! ## Leaf and node hashing
//!
//! Leaves are domain-separated SHA-256 hashes over a `(index, recipient,
//! amount, token)` preimage; internal nodes are domain-separated SHA-256
//! hashes over a lexicographically-sorted sibling pair (so the SDK does not
//! need to track left/right position while building a proof):
//!
//! ```text
//! leaf(i)       = SHA256(0x00 || be32(index) || xdr(recipient) || be16(amount) || xdr(token))
//! node(a, b)    = SHA256(0x01 || min(a, b) || max(a, b))
//! ```
//!
//! `sdk/src/merkle.ts` implements the identical scheme so that proofs
//! generated off-chain verify here without modification.
//!
//! ## Replay protection
//!
//! Each distribution stores a `claimed_bitmap: Bytes` — one bit per leaf
//! index — inside the single `MerkleDistribution` storage record, so a
//! `(distribution_id, index)` pair can only be claimed once without paying
//! for a separate storage entry per recipient.

use soroban_sdk::{xdr::ToXdr, Address, Bytes, BytesN, Env, Vec};

/// Hard cap on leaves per distribution, bounding the maximum proof length
/// (`ceil(log2(MAX_MERKLE_BATCH_LEAVES))`) and the bitmap storage size.
pub const MAX_MERKLE_BATCH_LEAVES: u32 = 10_000;
/// Defensive cap on proof length accepted by `claim_batch_payment`, well
/// above what `MAX_MERKLE_BATCH_LEAVES` can ever require. Rejects malformed
/// or adversarially oversized proofs before any hashing work is done.
pub const MAX_PROOF_LEN: u32 = 32;

const LEAF_DOMAIN: u8 = 0x00;
const NODE_DOMAIN: u8 = 0x01;

/// Build an all-zero claim bitmap sized for `leaf_count` leaves (one bit per leaf).
pub fn new_claim_bitmap(env: &Env, leaf_count: u32) -> Bytes {
    let byte_len = (leaf_count + 7) / 8;
    let mut bitmap = Bytes::new(env);
    for _ in 0..byte_len {
        bitmap.push_back(0u8);
    }
    bitmap
}

pub fn is_claimed_bit(bitmap: &Bytes, index: u32) -> bool {
    let byte_idx = index / 8;
    let bit_idx = index % 8;
    match bitmap.get(byte_idx) {
        Some(byte) => (byte >> bit_idx) & 1 == 1,
        None => false,
    }
}

pub fn set_claimed_bit(bitmap: &mut Bytes, index: u32) {
    let byte_idx = index / 8;
    let bit_idx = index % 8;
    let byte = bitmap.get(byte_idx).unwrap_or(0);
    bitmap.set(byte_idx, byte | (1u8 << bit_idx));
}

/// Domain-separated leaf hash. `pub` (not just within the contract impl) so
/// the SDK-compatibility tests in `merkle_distributor_test.rs` can build
/// real trees against the exact bytes the contract verifies, without
/// duplicating the algorithm.
pub fn leaf_hash(env: &Env, index: u32, recipient: &Address, amount: i128, token: &Address) -> BytesN<32> {
    let mut preimage = Bytes::from_array(env, &[LEAF_DOMAIN]);
    preimage.append(&Bytes::from_array(env, &index.to_be_bytes()));
    preimage.append(&recipient.clone().to_xdr(env));
    preimage.append(&Bytes::from_array(env, &amount.to_be_bytes()));
    preimage.append(&token.clone().to_xdr(env));
    env.crypto().sha256(&preimage).to_bytes()
}

/// Domain-separated hash over a lexicographically-sorted sibling pair.
/// Sorting means the caller never needs to track left/right position.
pub fn node_hash(env: &Env, a: &BytesN<32>, b: &BytesN<32>) -> BytesN<32> {
    let (left, right) = if a.to_array() <= b.to_array() {
        (a, b)
    } else {
        (b, a)
    };
    let mut preimage = Bytes::from_array(env, &[NODE_DOMAIN]);
    preimage.append(&Bytes::from_array(env, &left.to_array()));
    preimage.append(&Bytes::from_array(env, &right.to_array()));
    env.crypto().sha256(&preimage).to_bytes()
}

/// Fold `proof` up from the leaf at `index` to a candidate Merkle root.
pub fn compute_merkle_root(
    env: &Env,
    index: u32,
    recipient: &Address,
    amount: i128,
    token: &Address,
    proof: &Vec<BytesN<32>>,
) -> BytesN<32> {
    let mut computed = leaf_hash(env, index, recipient, amount, token);
    for sibling in proof.iter() {
        computed = node_hash(env, &computed, &sibling);
    }
    computed
}
