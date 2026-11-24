import { Address } from "@stellar/stellar-sdk";
import { createHash } from "node:crypto";

/**
 * Merkle Tree Batch Payment Distributor SDK helpers.
 *
 * Builds the exact leaf/node hashing scheme implemented on-chain by
 * `fluxapay/src/merkle_distributor.rs` (issues #1026 / #1027), so proofs
 * generated here verify against `claim_batch_payment` without modification.
 *
 * Hashing scheme (domain-separated SHA-256):
 *
 * ```text
 * leaf(i)    = SHA256(0x00 || be32(index) || xdr(recipient) || be16(amount) || xdr(token))
 * node(a, b) = SHA256(0x01 || min(a, b) || max(a, b))   // lexicographically-sorted pair
 * ```
 *
 * `xdr(address)` is the XDR encoding of the Soroban `Address` (an
 * `ScVal::Address`), i.e. `new Address(address).toScVal().toXDR()` — the
 * same bytes `soroban_sdk::Address::to_xdr(&env)` produces on-chain.
 */

const LEAF_DOMAIN = 0x00;
const NODE_DOMAIN = 0x01;

export interface MerkleLeaf {
  /** Index of this recipient within the distribution (0-based). */
  index: number;
  /** Stellar G-address or C-address of the recipient. */
  recipient: string;
  /** Allocation amount, in the token's smallest unit. */
  amount: bigint;
  /** Token contract address (C-address). */
  token: string;
}

export interface MerkleProof {
  /** Leaf index this proof is for. */
  index: number;
  /** Sibling hashes from leaf to root, in order. */
  siblings: Buffer[];
}

export interface MerkleTree {
  root: Buffer;
  leaves: Buffer[];
  layers: Buffer[][];
  proofs: MerkleProof[];
}

/** XDR-encode a Stellar address exactly as `Address::to_xdr(&env)` does on-chain. */
export function addressToXdrBytes(address: string): Buffer {
  return new Address(address).toScVal().toXDR();
}

function uint32BE(value: number): Buffer {
  const out = Buffer.alloc(4);
  out.writeUInt32BE(value >>> 0, 0);
  return out;
}

/** Encode an `i128` as 16-byte big-endian two's complement, matching `i128::to_be_bytes()`. */
export function i128ToBytesBE(value: bigint): Buffer {
  const unsigned = BigInt.asUintN(128, value);
  const out = Buffer.alloc(16);
  let remaining = unsigned;
  for (let i = 15; i >= 0; i--) {
    out[i] = Number(remaining & 0xffn);
    remaining >>= 8n;
  }
  return out;
}

/** Compute the canonical leaf hash. Matches the contract's `leaf_hash`. */
export function leafHash(leaf: MerkleLeaf): Buffer {
  const preimage = Buffer.concat([
    Buffer.from([LEAF_DOMAIN]),
    uint32BE(leaf.index),
    addressToXdrBytes(leaf.recipient),
    i128ToBytesBE(leaf.amount),
    addressToXdrBytes(leaf.token),
  ]);
  return createHash("sha256").update(preimage).digest();
}

/** Compute an internal node hash over a lexicographically-sorted pair. Matches the contract's `node_hash`. */
export function nodeHash(a: Buffer, b: Buffer): Buffer {
  const [left, right] = Buffer.compare(a, b) <= 0 ? [a, b] : [b, a];
  const preimage = Buffer.concat([Buffer.from([NODE_DOMAIN]), left, right]);
  return createHash("sha256").update(preimage).digest();
}

/**
 * Build a Merkle tree from a set of leaves. Leaves are sorted by `index`
 * first. An odd trailing node at any level is promoted to the next level
 * unchanged (no pairing hash applied), matching
 * `fluxapay/src/merkle_distributor_test.rs::build_tree` and the contract's
 * tolerance for non-power-of-two leaf counts — such a node needs no sibling
 * entry in its proof at that level.
 */
export function buildMerkleTree(leaves: MerkleLeaf[]): MerkleTree {
  if (leaves.length === 0) {
    throw new Error("cannot build a Merkle tree from an empty leaf set");
  }
  const sorted = [...leaves].sort((a, b) => a.index - b.index);
  const leafHashes = sorted.map(leafHash);

  const layers: Buffer[][] = [leafHashes];
  let current = leafHashes;
  while (current.length > 1) {
    const next: Buffer[] = [];
    for (let i = 0; i < current.length; i += 2) {
      if (i + 1 < current.length) {
        next.push(nodeHash(current[i], current[i + 1]));
      } else {
        next.push(current[i]);
      }
    }
    layers.push(next);
    current = next;
  }

  const root = current[0];
  const proofs = sorted.map((_, i) => buildProof(layers, i));
  return { root, leaves: leafHashes, layers, proofs };
}

function buildProof(layers: Buffer[][], index: number): MerkleProof {
  const siblings: Buffer[] = [];
  let idx = index;
  for (let level = 0; level < layers.length - 1; level++) {
    const layer = layers[level];
    const siblingIdx = idx % 2 === 0 ? idx + 1 : idx - 1;
    // An odd trailing node promoted unchanged to the next level (no sibling
    // in range) contributes no proof entry at this level.
    if (siblingIdx >= 0 && siblingIdx < layer.length) {
      siblings.push(layer[siblingIdx]);
    }
    idx = Math.floor(idx / 2);
  }
  return { index, siblings };
}

/** Verify a Merkle proof against an expected root, independent of the contract (useful for client-side sanity checks). */
export function verifyProof(leaf: MerkleLeaf, proof: MerkleProof, root: Buffer): boolean {
  let computed = leafHash(leaf);
  for (const sibling of proof.siblings) {
    computed = nodeHash(computed, sibling);
  }
  return Buffer.compare(computed, root) === 0;
}

/**
 * Build the `claim_batch_payment` argument set (minus `distribution_id`,
 * which the caller already has from `create_batch`'s return value) for a
 * given leaf. The `proof` array maps directly onto the contract's
 * `proof: Vec<BytesN<32>>` parameter.
 */
export function buildClaimArgs(
  tree: MerkleTree,
  leaves: MerkleLeaf[],
  leafIndex: number,
): { index: number; recipient: string; amount: bigint; proof: Buffer[] } {
  const leaf = [...leaves].sort((a, b) => a.index - b.index).find((l) => l.index === leafIndex);
  const proof = tree.proofs.find((p) => p.index === leafIndex);
  if (!leaf || !proof) {
    throw new Error(`no leaf/proof found for index ${leafIndex}`);
  }
  return {
    index: leaf.index,
    recipient: leaf.recipient,
    amount: leaf.amount,
    proof: proof.siblings,
  };
}
