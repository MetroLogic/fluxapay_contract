import {
  createHmac,
  createPrivateKey,
  createPublicKey,
  generateKeyPairSync,
  sign,
  timingSafeEqual,
  verify,
  type KeyObject,
} from "node:crypto";

/**
 * Webhook payload signing (Issues #775, #808, #810).
 *
 * Supports both HMAC-SHA256 (default) and Ed25519 signatures.
 * For Ed25519, the platform signs raw payload with a dedicated Ed25519 keypair,
 * exposing the active public key at GET /webhooks/public-key.
 */

/** Header carrying the signature, Stripe/GitHub-style. */
export const SIGNATURE_HEADER = "x-fluxapay-signature";
/** Header naming the event, so a receiver can route without parsing. */
export const EVENT_TYPE_HEADER = "x-fluxapay-event";
/** Header carrying the delivery id, for deduplication. */
export const DELIVERY_ID_HEADER = "x-fluxapay-delivery";
/** Header carrying timestamp for replay prevention. */
export const TIMESTAMP_HEADER = "x-fluxapay-timestamp";

/** How far a delivery's timestamp may drift before a receiver should reject. */
export const SIGNATURE_TOLERANCE_SECONDS = 300;

// ─── Ed25519 Platform Keypair Management & Rotation ─────────────────────────

let activeEd25519PrivateKey: KeyObject;
let activeEd25519PublicKeyHex: string;

function initDefaultEd25519Key(): void {
  const envPrivate = process.env.WEBHOOK_ED25519_PRIVATE_KEY;
  const envPublic = process.env.WEBHOOK_ED25519_PUBLIC_KEY;

  if (envPrivate) {
    try {
      activeEd25519PrivateKey = createPrivateKey(envPrivate);
      if (envPublic) {
        activeEd25519PublicKeyHex = envPublic;
      } else {
        const pub = createPublicKey(activeEd25519PrivateKey);
        const der = pub.export({ type: "spki", format: "der" });
        activeEd25519PublicKeyHex = der.subarray(-32).toString("hex");
      }
      return;
    } catch {
      // Fallback to generated key
    }
  }

  const keyPair = generateKeyPairSync("ed25519");
  activeEd25519PrivateKey = keyPair.privateKey;
  const der = keyPair.publicKey.export({ type: "spki", format: "der" });
  activeEd25519PublicKeyHex = der.subarray(-32).toString("hex");
}

initDefaultEd25519Key();

export function getPlatformEd25519PublicKey(): string {
  if (!activeEd25519PublicKeyHex) {
    initDefaultEd25519Key();
  }
  return activeEd25519PublicKeyHex;
}

export function getPlatformEd25519PrivateKey(): KeyObject {
  if (!activeEd25519PrivateKey) {
    initDefaultEd25519Key();
  }
  return activeEd25519PrivateKey;
}

/**
 * Rotate the active Ed25519 signing keypair for webhook verification.
 * Follows the documented key rotation procedure.
 */
export function rotatePlatformEd25519Key(
  newPrivateKey: KeyObject | string,
  newPublicKeyHex?: string,
): void {
  if (typeof newPrivateKey === "string") {
    activeEd25519PrivateKey = createPrivateKey(newPrivateKey);
  } else {
    activeEd25519PrivateKey = newPrivateKey;
  }

  if (newPublicKeyHex) {
    activeEd25519PublicKeyHex = newPublicKeyHex;
  } else {
    const pub = createPublicKey(activeEd25519PrivateKey);
    const der = pub.export({ type: "spki", format: "der" });
    activeEd25519PublicKeyHex = der.subarray(-32).toString("hex");
  }
}

/**
 * Sign raw payload using the active Ed25519 key. Returns hex-encoded signature.
 */
export function signEd25519Payload(
  payload: string,
  privateKey: KeyObject = getPlatformEd25519PrivateKey(),
): string {
  const signature = sign(null, Buffer.from(payload, "utf8"), privateKey);
  return signature.toString("hex");
}

/**
 * Verify an Ed25519 signature against raw payload.
 */
export function verifyEd25519Signature(
  payload: string,
  signatureHex: string,
  publicKeyHexOrPem: string = getPlatformEd25519PublicKey(),
): boolean {
  try {
    let keyObj: KeyObject;
    if (publicKeyHexOrPem.includes("BEGIN PUBLIC KEY")) {
      keyObj = createPublicKey(publicKeyHexOrPem);
    } else {
      // 32-byte hex raw public key wrapped in SPKI DER header
      const rawBuf = Buffer.from(publicKeyHexOrPem, "hex");
      if (rawBuf.length !== 32) return false;
      const spkiHeader = Buffer.from("302a300506032b6570032100", "hex");
      const der = Buffer.concat([spkiHeader, rawBuf]);
      keyObj = createPublicKey({ key: der, format: "der", type: "spki" });
    }
    const sigBuf = Buffer.from(signatureHex, "hex");
    return verify(null, Buffer.from(payload, "utf8"), keyObj, sigBuf);
  } catch {
    return false;
  }
}

// ─── HMAC-SHA256 Signing ─────────────────────────────────────────────────────

/**
 * Build the string that gets signed: `<timestamp>.<body>`.
 */
export function signedPayload(timestampSeconds: number, body: string): string {
  return `${timestampSeconds}.${body}`;
}

export function computeSignature(
  secret: string,
  timestampSeconds: number,
  body: string,
): string {
  return createHmac("sha256", secret)
    .update(signedPayload(timestampSeconds, body))
    .digest("hex");
}

/**
 * Header value in the form `sha256=...`, `ed25519=...`, or legacy `t=<unix>,v1=<hex>`.
 */
export function buildSignatureHeader(
  secretOrKey: string,
  body: string,
  timestampSeconds: number = Math.floor(Date.now() / 1000),
  algorithm: "hmac_sha256" | "ed25519" = "hmac_sha256",
): string {
  if (algorithm === "ed25519") {
    const signature = signEd25519Payload(body);
    return `ed25519=${signature}`;
  }

  const signature = computeSignature(secretOrKey, timestampSeconds, body);
  return `sha256=${signature}`;
}

export interface ParsedSignature {
  timestamp?: number;
  signature: string;
  algorithm?: "hmac_sha256" | "ed25519";
}

export function parseSignatureHeader(header: string): ParsedSignature | null {
  if (!header || typeof header !== "string") return null;

  if (header.startsWith("ed25519=")) {
    const signature = header.slice("ed25519=".length).trim();
    if (!signature) return null;
    return { signature, algorithm: "ed25519" };
  }

  if (header.startsWith("sha256=")) {
    const rest = header.slice("sha256=".length).trim();
    if (!rest) return null;
    // Check if rest is formatted as t=...,v1=...
    if (rest.includes("t=") || rest.includes("v1=")) {
      const parsedSub = parseSignatureHeader(rest);
      if (parsedSub) {
        return { ...parsedSub, algorithm: "hmac_sha256" };
      }
    }
    return { signature: rest, algorithm: "hmac_sha256" };
  }

  const parts = header.split(",").map((p) => p.trim());
  let timestamp: number | null = null;
  let signature: string | null = null;
  let algo: "hmac_sha256" | "ed25519" = "hmac_sha256";

  for (const part of parts) {
    const [key, value] = part.split("=", 2);
    if (key === "t") {
      const parsed = Number(value);
      if (Number.isFinite(parsed)) timestamp = parsed;
    } else if (key === "v1" || key === "sha256") {
      signature = value ?? null;
      algo = "hmac_sha256";
    } else if (key === "ed25519") {
      signature = value ?? null;
      algo = "ed25519";
    }
  }

  if (!signature) return null;
  return {
    timestamp: timestamp ?? undefined,
    signature,
    algorithm: algo,
  };
}

/**
 * Verify a webhook signature supporting both HMAC-SHA256 and Ed25519.
 */
export function verifySignature(
  secretOrPublicKey: string,
  header: string,
  body: string,
  nowSeconds: number = Math.floor(Date.now() / 1000),
  toleranceSeconds: number = SIGNATURE_TOLERANCE_SECONDS,
): boolean {
  const parsed = parseSignatureHeader(header);
  if (!parsed) return false;

  if (parsed.algorithm === "ed25519" || header.startsWith("ed25519=")) {
    return verifyEd25519Signature(body, parsed.signature, secretOrPublicKey);
  }

  // HMAC-SHA256 verification
  if (parsed.timestamp !== undefined) {
    if (Math.abs(nowSeconds - parsed.timestamp) > toleranceSeconds) {
      return false;
    }
    const expected = computeSignature(secretOrPublicKey, parsed.timestamp, body);
    const a = Buffer.from(expected, "utf8");
    const b = Buffer.from(parsed.signature, "utf8");
    if (a.length !== b.length) return false;
    return timingSafeEqual(a, b);
  }

  // Without timestamp embedded in header: test direct body hash or tolerance window
  const expectedDirect = createHmac("sha256", secretOrPublicKey)
    .update(body)
    .digest("hex");
  const aDirect = Buffer.from(expectedDirect, "utf8");
  const b = Buffer.from(parsed.signature, "utf8");
  if (aDirect.length === b.length && timingSafeEqual(aDirect, b)) {
    return true;
  }

  const expectedTimestamped = computeSignature(secretOrPublicKey, nowSeconds, body);
  const aTs = Buffer.from(expectedTimestamped, "utf8");
  if (aTs.length === b.length && timingSafeEqual(aTs, b)) {
    return true;
  }

  return false;
}

export const verifyWebhookSignature = verifySignature;
