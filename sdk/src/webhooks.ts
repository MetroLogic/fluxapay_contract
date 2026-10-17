import { createHmac, createPublicKey, timingSafeEqual, verify, type KeyObject } from "node:crypto";
import { Keypair, StrKey } from "@stellar/stellar-sdk";

export const SIGNATURE_TOLERANCE_SECONDS = 300;

export interface ParsedWebhookSignature {
  algorithm: "hmac_sha256" | "ed25519";
  signature: string;
  timestamp?: number;
}

/**
 * Parse an X-FluxaPay-Signature header.
 * Supports prefixes: `ed25519=`, `sha256=`, or Stripe-style `t=<unix>,v1=<hex>`.
 */
export function parseWebhookSignatureHeader(header: string): ParsedWebhookSignature | null {
  if (!header || typeof header !== "string") return null;

  const trimmed = header.trim();
  if (trimmed.startsWith("ed25519=")) {
    const signature = trimmed.slice("ed25519=".length).trim();
    return signature ? { algorithm: "ed25519", signature } : null;
  }

  if (trimmed.startsWith("sha256=")) {
    const rest = trimmed.slice("sha256=".length).trim();
    if (!rest) return null;
    if (rest.includes("t=") || rest.includes("v1=")) {
      const parsed = parseWebhookSignatureHeader(rest);
      if (parsed) {
        return { ...parsed, algorithm: "hmac_sha256" };
      }
    }
    return { algorithm: "hmac_sha256", signature: rest };
  }

  const parts = trimmed.split(",").map((p) => p.trim());
  let timestamp: number | undefined;
  let signature: string | undefined;
  let algorithm: "hmac_sha256" | "ed25519" = "hmac_sha256";

  for (const part of parts) {
    const [key, value] = part.split("=", 2);
    if (key === "t") {
      const parsed = Number(value);
      if (Number.isFinite(parsed)) timestamp = parsed;
    } else if (key === "v1" || key === "sha256") {
      signature = value ?? undefined;
      algorithm = "hmac_sha256";
    } else if (key === "ed25519") {
      signature = value ?? undefined;
      algorithm = "ed25519";
    }
  }

  if (!signature) return null;
  return { algorithm, signature, timestamp };
}

/**
 * Verify an incoming FluxaPay webhook signature.
 *
 * Supports both HMAC-SHA256 (`secret`) and Ed25519 (`publicKey`).
 * For Ed25519, `secretOrPublicKey` can be a Stellar public address ('G...'),
 * a 32-byte hex public key, or a PEM public key.
 *
 * @param payload The raw webhook request body string
 * @param header The X-FluxaPay-Signature header value
 * @param secretOrPublicKey Merchant webhook secret or platform Ed25519 public key
 * @param toleranceSeconds Replay protection tolerance in seconds (default 300)
 * @param nowSeconds Current Unix epoch in seconds (optional)
 */
export function verifyWebhookSignature(
  payload: string,
  header: string,
  secretOrPublicKey: string,
  toleranceSeconds: number = SIGNATURE_TOLERANCE_SECONDS,
  nowSeconds: number = Math.floor(Date.now() / 1000),
): boolean {
  try {
    const parsed = parseWebhookSignatureHeader(header);
    if (!parsed) return false;

    if (parsed.algorithm === "ed25519") {
      return verifyEd25519(payload, parsed.signature, secretOrPublicKey);
    }

    return verifyHmac(payload, parsed.signature, parsed.timestamp, secretOrPublicKey, toleranceSeconds, nowSeconds);
  } catch {
    return false;
  }
}

function verifyEd25519(payload: string, signatureHex: string, publicKey: string): boolean {
  try {
    const payloadBuf = Buffer.from(payload, "utf8");
    const sigBuf = Buffer.from(signatureHex, "hex");

    // Stellar G... address
    if (StrKey.isValidEd25519PublicKey(publicKey)) {
      const kp = Keypair.fromPublicKey(publicKey);
      return kp.verify(payloadBuf, sigBuf);
    }

    if (publicKey.includes("BEGIN PUBLIC KEY")) {
      const keyObj = createPublicKey(publicKey);
      return verify(null, payloadBuf, keyObj, sigBuf);
    }

    // 32-byte hex string
    const rawBuf = Buffer.from(publicKey, "hex");
    if (rawBuf.length === 32) {
      const spkiHeader = Buffer.from("302a300506032b6570032100", "hex");
      const der = Buffer.concat([spkiHeader, rawBuf]);
      const keyObj = createPublicKey({ key: der, format: "der", type: "spki" });
      return verify(null, payloadBuf, keyObj, sigBuf);
    }

    return false;
  } catch {
    return false;
  }
}

function verifyHmac(
  payload: string,
  signature: string,
  timestamp: number | undefined,
  secret: string,
  toleranceSeconds: number,
  nowSeconds: number,
): boolean {
  try {
    if (timestamp !== undefined) {
      if (Math.abs(nowSeconds - timestamp) > toleranceSeconds) {
        return false;
      }
      const expected = createHmac("sha256", secret)
        .update(`${timestamp}.${payload}`)
        .digest("hex");
      const a = Buffer.from(expected, "utf8");
      const b = Buffer.from(signature, "utf8");
      return a.length === b.length && timingSafeEqual(a, b);
    }

    const expectedDirect = createHmac("sha256", secret).update(payload).digest("hex");
    const aDirect = Buffer.from(expectedDirect, "utf8");
    const b = Buffer.from(signature, "utf8");
    if (aDirect.length === b.length && timingSafeEqual(aDirect, b)) {
      return true;
    }

    const expectedTimestamped = createHmac("sha256", secret)
      .update(`${nowSeconds}.${payload}`)
      .digest("hex");
    const aTs = Buffer.from(expectedTimestamped, "utf8");
    return aTs.length === b.length && timingSafeEqual(aTs, b);
  } catch {
    return false;
  }
}
