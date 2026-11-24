/**
 * Issue #816: signed, shareable payment receipts.
 *
 * A receipt binds payment fields to a FluxaPay platform Ed25519 signature so
 * customers and merchants can verify authenticity offline via `verifyReceipt`.
 *
 * Receipt URL format (documented):
 *   `{receiptBaseUrl}/r/{payment_id}`
 * Default base: `https://receipts.fluxapay.io`
 */

import { Keypair, StrKey } from "@stellar/stellar-sdk";
import { createHash } from "node:crypto";

export interface PaymentReceipt {
  payment_id: string;
  /** Formatted amount, e.g. `'10.00 USDC'`. */
  amount: string;
  merchant_name: string;
  /** ISO-8601 timestamp of confirmation. */
  confirmed_at: string;
  /** Stellar transaction hash linking to Horizon. */
  tx_hash: string;
  /** Base64-encoded Ed25519 signature over the canonical receipt payload. */
  proof: string;
  /** Hosted verifiable receipt page URL. */
  receipt_url: string;
}

/** Fields that are covered by `proof` (everything except proof/receipt_url). */
export interface ReceiptSignedFields {
  payment_id: string;
  amount: string;
  merchant_name: string;
  confirmed_at: string;
  tx_hash: string;
}

export const DEFAULT_RECEIPT_BASE_URL = "https://receipts.fluxapay.io";

/**
 * Canonical message bytes signed by the platform key.
 * Format: `fluxapay-receipt-v1\n{payment_id}\n{amount}\n{merchant_name}\n{confirmed_at}\n{tx_hash}`
 */
export function buildReceiptMessage(fields: ReceiptSignedFields): Buffer {
  const lines = [
    "fluxapay-receipt-v1",
    fields.payment_id,
    fields.amount,
    fields.merchant_name,
    fields.confirmed_at,
    fields.tx_hash,
  ];
  return Buffer.from(lines.join("\n"), "utf8");
}

/** SHA-256 digest of the canonical message (what Ed25519 signs). */
export function receiptMessageDigest(fields: ReceiptSignedFields): Buffer {
  return createHash("sha256").update(buildReceiptMessage(fields)).digest();
}

/**
 * Format a stroop amount (7 decimal places for USDC) as a human-readable string.
 */
export function formatReceiptAmount(amountStroops: bigint | number | string, currency: string): string {
  const raw = typeof amountStroops === "bigint" ? amountStroops : BigInt(amountStroops);
  const negative = raw < 0n;
  const abs = negative ? -raw : raw;
  const whole = abs / 10_000_000n;
  const frac = abs % 10_000_000n;
  const fracStr = frac.toString().padStart(7, "0").replace(/0+$/, "") || "0";
  const formatted = fracStr === "0" ? whole.toString() : `${whole}.${fracStr}`;
  return `${negative ? "-" : ""}${formatted} ${currency}`;
}

export function buildReceiptUrl(paymentId: string, baseUrl = DEFAULT_RECEIPT_BASE_URL): string {
  const base = baseUrl.replace(/\/$/, "");
  return `${base}/r/${encodeURIComponent(paymentId)}`;
}

/**
 * Sign receipt fields with a Stellar secret key (S...) used as the FluxaPay
 * platform signing key. Returns a base64 Ed25519 signature.
 */
export function signReceiptProof(
  fields: ReceiptSignedFields,
  platformSecretKey: string,
): string {
  const keypair = Keypair.fromSecret(platformSecretKey);
  const digest = receiptMessageDigest(fields);
  const sig = keypair.sign(digest);
  return Buffer.from(sig).toString("base64");
}

/**
 * Pure offline verification of a receipt proof against a platform public key
 * (G... Stellar account / Ed25519 pubkey). No network calls.
 */
export function verifyReceipt(
  receipt: Pick<PaymentReceipt, keyof ReceiptSignedFields | "proof">,
  platformPublicKey: string,
): boolean {
  if (!StrKey.isValidEd25519PublicKey(platformPublicKey)) {
    return false;
  }
  let signature: Buffer;
  try {
    signature = Buffer.from(receipt.proof, "base64");
  } catch {
    return false;
  }
  if (signature.length !== 64) {
    return false;
  }

  const fields: ReceiptSignedFields = {
    payment_id: receipt.payment_id,
    amount: receipt.amount,
    merchant_name: receipt.merchant_name,
    confirmed_at: receipt.confirmed_at,
    tx_hash: receipt.tx_hash,
  };
  const digest = receiptMessageDigest(fields);
  try {
    const keypair = Keypair.fromPublicKey(platformPublicKey);
    return keypair.verify(digest, signature);
  } catch {
    return false;
  }
}

export interface BuildPaymentReceiptParams {
  paymentId: string;
  amountStroops: bigint | number | string;
  currency: string;
  merchantName: string;
  confirmedAt: Date | string | number;
  txHash: string;
  platformSecretKey: string;
  receiptBaseUrl?: string;
}

/** Assemble a fully signed {@link PaymentReceipt} from payment details. */
export function buildPaymentReceipt(params: BuildPaymentReceiptParams): PaymentReceipt {
  const confirmedAt =
    params.confirmedAt instanceof Date
      ? params.confirmedAt.toISOString()
      : typeof params.confirmedAt === "number"
        ? new Date(params.confirmedAt * (params.confirmedAt < 1e12 ? 1000 : 1)).toISOString()
        : new Date(params.confirmedAt).toISOString();

  const fields: ReceiptSignedFields = {
    payment_id: params.paymentId,
    amount: formatReceiptAmount(params.amountStroops, params.currency),
    merchant_name: params.merchantName,
    confirmed_at: confirmedAt,
    tx_hash: params.txHash,
  };

  return {
    ...fields,
    proof: signReceiptProof(fields, params.platformSecretKey),
    receipt_url: buildReceiptUrl(params.paymentId, params.receiptBaseUrl),
  };
}
