/**
 * Issue #672: SEP-10 JWT verification for the indexer REST API.
 *
 * Verifies JWTs issued by `SEP10Authenticator.generateJWT` (see
 * `sdk/src/sep10.ts`) after a client completes the SEP-10 Web
 * Authentication challenge/response flow against `STELLAR_WEB_AUTH_ENDPOINT`.
 *
 * The token format and signing scheme here intentionally mirror
 * `SEP10Authenticator.generateJWT` byte-for-byte so tokens minted by that
 * class validate here without a shared library: a compact
 * `header.payload.signature` string (base64url segments), HMAC-SHA256 signed
 * with the server's Stellar account key used as HMAC key material.
 */
import * as crypto from "crypto";

export interface SEP10JWTClaims {
  /** Issuer — the auth server's Stellar public key. */
  iss: string;
  /** Subject — the authenticated Stellar account (merchant or admin). */
  sub: string;
  iat: number;
  exp: number;
  /** Optional scope claim, e.g. "merchant" or "admin". */
  scope?: string;
}

export class SEP10JWTError extends Error {}

function base64urlDecode(segment: string): Buffer {
  return Buffer.from(segment, "base64url");
}

/**
 * Verify a SEP-10 JWT's signature and expiry.
 *
 * @param token - The compact JWT string from the `Authorization: Bearer <token>` header.
 * @param serverPublicKey - The Stellar account whose key material signed the token
 *   (must match `STELLAR_SERVER_PUBLIC_KEY`).
 * @throws {SEP10JWTError} if the token is malformed, the signature is invalid, or it has expired.
 */
export function verifySEP10JWT(token: string, serverPublicKey: string): SEP10JWTClaims {
  const parts = token.split(".");
  if (parts.length !== 3) {
    throw new SEP10JWTError("Malformed JWT: expected 3 segments");
  }
  const [headerB64, payloadB64, signatureB64] = parts;

  const message = `${headerB64}.${payloadB64}`;
  const expectedSignature = crypto
    .createHmac("sha256", Buffer.from(serverPublicKey, "base64"))
    .update(message)
    .digest("base64url");

  const provided = base64urlDecode(signatureB64);
  const expected = base64urlDecode(expectedSignature);
  if (
    provided.length !== expected.length ||
    !crypto.timingSafeEqual(provided, expected)
  ) {
    throw new SEP10JWTError("Invalid JWT signature");
  }

  let claims: SEP10JWTClaims;
  try {
    claims = JSON.parse(base64urlDecode(payloadB64).toString("utf8"));
  } catch {
    throw new SEP10JWTError("Malformed JWT payload");
  }

  if (!claims.sub || typeof claims.sub !== "string") {
    throw new SEP10JWTError("JWT missing 'sub' claim");
  }

  const now = Math.floor(Date.now() / 1000);
  if (typeof claims.exp !== "number" || claims.exp < now) {
    throw new SEP10JWTError("JWT expired");
  }

  return claims;
}
