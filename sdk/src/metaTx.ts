/**
 * Sponsored meta-transactions (Issue #893).
 *
 * Builds the same domain separator and signing preimage as
 * `fluxapay/src/meta_transaction.rs`. The merchant signs the SHA-256 digest
 * with the Ed25519 key of their G-address. A relayer submits the payload.
 */
import { hash, Keypair, StrKey } from "@stellar/stellar-sdk";

export const META_TX_FUNCTIONS = {
  createCharge: "create_charge",
  authorizeRefund: "authorize_refund",
  rotateWebhookKey: "rotate_webhook_key",
  setFeeRecipient: "set_fee_recipient",
} as const;

export type MetaTransactionTarget =
  (typeof META_TX_FUNCTIONS)[keyof typeof META_TX_FUNCTIONS];

export interface MetaTransactionPayload {
  /** Merchant account id (G...). This key signs the payload. */
  merchantId: string;
  /** One of `create_charge`, `authorize_refund`, `rotate_webhook_key`, `set_fee_recipient`. */
  targetFunction: string;
  /** XDR-encoded arguments for `targetFunction`. */
  callData: Buffer;
  nonce: bigint;
  /** Ledger timestamp deadline. The contract accepts `timestamp <= deadline`. */
  deadline: bigint;
  /** 32-byte domain separator from {@link domainSeparator}. */
  domainSeparator: Buffer;
}

export interface BuildMetaTransactionInput {
  merchantId: string;
  targetFunction: MetaTransactionTarget | string;
  callData: Buffer;
  nonce: bigint;
  deadline: bigint;
  /** Contract id (C...) the signature is bound to. */
  contractId: string;
  /** Network passphrase. Hashed to the 32-byte network id. */
  networkPassphrase: string;
}

const DOMAIN_NAME = Buffer.from("FluxaPay", "utf8");

function u32be(value: number): Buffer {
  const buf = Buffer.alloc(4);
  buf.writeUInt32BE(value);
  return buf;
}

function u64be(value: bigint): Buffer {
  const buf = Buffer.alloc(8);
  buf.writeBigUInt64BE(value);
  return buf;
}

/**
 * `SHA-256(utf8 "FluxaPay" || contract hash || network id)`.
 * Binding the contract id and network id blocks cross-contract and
 * cross-network replay.
 */
export function domainSeparator(
  contractId: string,
  networkPassphrase: string,
): Buffer {
  const contractHash = Buffer.from(StrKey.decodeContract(contractId));
  const networkId = hash(Buffer.from(networkPassphrase, "utf8"));
  return hash(Buffer.concat([DOMAIN_NAME, contractHash, networkId]));
}

export function buildMetaTransactionPayload(
  input: BuildMetaTransactionInput,
): MetaTransactionPayload {
  return {
    merchantId: input.merchantId,
    targetFunction: input.targetFunction,
    callData: input.callData,
    nonce: input.nonce,
    deadline: input.deadline,
    domainSeparator: domainSeparator(input.contractId, input.networkPassphrase),
  };
}

/**
 * Canonical preimage. The Ed25519 message is `SHA-256` of these bytes,
 * not a second hash of that digest.
 *
 * `domain_separator (32) || merchant_pubkey (32) || u32be(name_len) ||
 *  utf8 function name || u32be(call_data_len) || call_data ||
 *  u64be(nonce) || u64be(deadline)`
 */
export function signingPreimage(payload: MetaTransactionPayload): Buffer {
  const pubkey = Buffer.from(StrKey.decodeEd25519PublicKey(payload.merchantId));
  const name = Buffer.from(payload.targetFunction, "utf8");
  return Buffer.concat([
    payload.domainSeparator,
    pubkey,
    u32be(name.length),
    name,
    u32be(payload.callData.length),
    payload.callData,
    u64be(payload.nonce),
    u64be(payload.deadline),
  ]);
}

export function signingDigest(payload: MetaTransactionPayload): Buffer {
  return hash(signingPreimage(payload));
}

/** Sign the 32-byte digest with the merchant's Stellar keypair. */
export function signMetaTransaction(
  payload: MetaTransactionPayload,
  keypair: Keypair,
): Buffer {
  if (keypair.publicKey() !== payload.merchantId) {
    throw new Error("Keypair does not match payload.merchantId");
  }
  return keypair.sign(signingDigest(payload));
}
