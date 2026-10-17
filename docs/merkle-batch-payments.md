# Merkle Tree Batch Payment Distributor

The Merkle Tree Batch Payment Distributor lets a payer commit a single 32-byte Merkle root and fund the total amount in one transaction. Each recipient then independently claims their allocation by submitting an O(log N) cryptographic inclusion proof. This avoids the operation-count and transaction-size limits of a naive batch transfer, making it practical for payroll- and airdrop-style bulk disbursements. Resolves issues #1026 and #1027 (duplicates of the same feature request).

## Overview

A distribution is created with `create_batch`, over a Merkle root computed off-chain (via `sdk/src/merkle.ts`) from leaves `(index, recipient, amount, token)`. The creator deposits the full `total_amount` into the contract at creation time. Each recipient later calls `claim_batch_payment` with their `(index, recipient, amount)` and the sibling-hash proof from leaf to root; a valid proof releases their share immediately.

## Module

```rust
fluxapay/src/merkle_distributor.rs
```

### Key types

```rust
// fluxapay/src/types.rs
pub struct MerkleDistribution {
    pub distribution_id: u64,
    pub creator: Address,
    pub token: Address,
    pub merkle_root: BytesN<32>,
    pub total_amount: i128,
    pub claimed_amount: i128,
    pub leaf_count: u32,
    pub expires_at: u64,
    pub defunded: bool,
    pub claimed_bitmap: Bytes,
}
```

### Entry points (`PaymentProcessor`)

```rust
pub fn create_batch(
    env: Env,
    creator: Address,
    token: Address,
    merkle_root: BytesN<32>,
    total_amount: i128,
    leaf_count: u32,
    expires_at: u64,
) -> Result<u64, Error>; // returns distribution_id

pub fn claim_batch_payment(
    env: Env,
    distribution_id: u64,
    index: u32,
    recipient: Address,
    amount: i128,
    proof: Vec<BytesN<32>>,
) -> Result<(), Error>;

pub fn defund_batch(env: Env, creator: Address, distribution_id: u64) -> Result<i128, Error>;

pub fn get_batch(env: Env, distribution_id: u64) -> Result<MerkleDistribution, Error>;
pub fn is_batch_claimed(env: Env, distribution_id: u64, index: u32) -> Result<bool, Error>;
```

`distribution_id` (a `u64` counter) is this contract's batch identifier — it is what the original issues called `batch_id`. A `u64` counter is cheaper on-chain than hashing/storing a `String` id and matches the identifier type already used by the `MERKLE/*` events below.

### Verification

Leaves and internal nodes are domain-separated SHA-256 hashes:

```text
leaf(i)    = SHA256(0x00 || be32(index) || xdr(recipient) || be16(amount) || xdr(token))
node(a, b) = SHA256(0x01 || min(a, b) || max(a, b))   // lexicographically-sorted pair
```

Sorting the pair before hashing means the proof generator never needs to track left/right position — it just needs the list of sibling hashes from leaf to root, in order. `claim_batch_payment` folds the submitted `proof` onto the recomputed leaf hash and compares the result against the distribution's stored `merkle_root`. Any tampered sibling, wrong `index`, or wrong `amount` changes the computed root and is rejected with `Error::InvalidMerkleProof`.

### Bitmap claim tracking

Claimed leaves are tracked with a bitmap (`claimed_bitmap: Bytes`, one bit per leaf index) stored as a field on the single `MerkleDistribution` record, rather than one storage entry per recipient. Double claims revert with `Error::MerkleAlreadyClaimed`.

## Expiry and defund

A distribution carries an `expires_at` Unix timestamp. Claims after `expires_at` revert with `Error::MerkleDistributionExpired`. After `expires_at`, the creator may call `defund_batch` to recover `total_amount - claimed_amount`; calling it before expiry reverts with `Error::MerkleDistributionNotExpired`, and calling it (or claiming) again after defunding reverts with `Error::MerkleDistributionAlreadyDefunded`.

## Events

- `MERKLE/DISTRIBUTION_CREATED` — `distribution_id`, creator, token, merkle_root, total_amount, leaf_count, expires_at
- `MERKLE/CLAIMED` — `distribution_id`, index, recipient, amount
- `MERKLE/DEFUNDED` — `distribution_id`, creator, refunded_amount

## TypeScript SDK

`sdk/src/merkle.ts` implements the identical leaf/node hashing scheme so proofs generated off-chain verify on-chain without modification.

```ts
import { buildMerkleTree, buildClaimArgs } from "./merkle";

const leaves = [
  { index: 0, recipient: "G...", amount: 100n, token: "C..." },
  { index: 1, recipient: "G...", amount: 200n, token: "C..." },
];

const tree = buildMerkleTree(leaves);
// tree.root -> pass as merkle_root to create_batch

const args = buildClaimArgs(tree, leaves, 0); // { index, recipient, amount, proof }
// pass args.proof (Buffer[]) as the `proof: Vec<BytesN<32>>` argument to claim_batch_payment
```

## Testing

Unit tests live in `fluxapay/src/merkle_distributor_test.rs` and cover:

- Creating a 5-leaf batch and all five recipients successfully claiming.
- Rejecting a double claim (`Error::MerkleAlreadyClaimed`).
- Rejecting a tampered sibling hash and a wrong claimed amount (`Error::InvalidMerkleProof`).
- Rejecting an empty proof against a multi-leaf root.
- Claims after `expires_at` (`Error::MerkleDistributionExpired`), followed by `defund_batch` recovering the unclaimed balance, and rejecting a second defund.
- Rejecting `defund_batch` before expiry (`Error::MerkleDistributionNotExpired`).
- Rejecting an invalid `leaf_count` (`Error::InvalidMerkleLeafCount`).
- A direct check that folding a generated proof through `compute_merkle_root` reproduces the tree's root for every leaf.
