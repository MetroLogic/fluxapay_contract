# Merkle Tree Batch Payment Distributor

The Merkle Tree Batch Payment Distributor lets a payer commit a single 32-byte Merkle root and fund the total amount in one transaction. Each recipient then independently claims their allocation by submitting an O(log N) cryptographic inclusion proof. This avoids the Operation limits and transaction-size constraints of a naive batch transfer.

## Overview

A distribution is created with a Merkle root over the leaves `HASH(recipient || amount)`. The creator deposits the total of all allocations into the distributor contract. Recipients claim by providing their leaf index and the sibling hashes along the path to the root.

## Module

```rust
fluxapay/src/merkle_distributor.rs
```

### Key types

```rust
pub struct Distribution {
    pub creator: Address,
    pub token: Address,
    pub root: [a8],
    pub total: i128,
    pub claimed: i128,
    pub expiry: u64,
    pub claimed_bitmap: Bytes,
}

pub enum MerkleError {
    InvalidProof,
    AlreadyClaimed,
    Expired,
    NotExpired,
    InsufficientFunds,
    Unauthorized,
    InvalidIndex,
}
```

### Verification

Leaves are derived as `SHA256(recipient || amount)`. Proof verification uses `env.crypto().sha256()` and combines sibling hashes in the correct order based on the index bits. Any tampered sibling or altered leaf results in a root mismatch and reverts with `MerkleError::InvalidProof`.

### Bitmap claim tracking

Claimed leaves are tracked with a bitmap stored in a single persistent ledger entry. Bit `index` is set on claim. This minimizes writes: one entry per distribution rather than one entry per recipient. Double claims revert with `MerkleError::AlreadyClaimed`.

## Expiry and defund

A distribution carries an `expiry` ledge. After expiry, the creator may call defund to recover the unclaimed balance. Once defunded, all subsequent claims revert with `MerkleError::Expired`.

## Events

- `MERKLE/DISTRIBUTION_CREATED` — root, total, token, expiry
- `MERKLE/CLAIMED` — index, recipient, amount
- `MERKLE/DEFUNDED` — remaining amount returned to creator

## TypeScript SDK

`sdk/src/merkle.ts` provides helpers to build the tree, generate inclusion proofs, and submit claims.

```ts
import { buildMerkleTree, getProof, submitClaim } from "./merkle";

const leaves = [
  { recipient: "G", amount: 100n },
  { recipient: "G", amount: 200n },
];

const tree = buildMerkleTree(leaves);
const proof = getProof(tree, 0);
await submitClaim(distributorId, 0, leaves[0], proof);
```

## Testing

Unit and fuzz tests live in `merkle_distributor_test.rs` and cover:

- 10-leaf and 100-leaf Merkle trees.
- Correct claims with valid proofs.
- Sibling tampering / invalid proofs.
-  Out-of-order claims.
-  Expiry defund and post-defund claim rejection.
