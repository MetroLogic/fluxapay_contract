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
 * with the configured SEP10_JWT_SECRET.
 *
 * Issue #802: the `iss` claim is validated against the expected SEP-10 home
 * domain (`SEP10_HOME_DOMAIN`) so tokens minted by any other issuer are
 * rejected even when their signature is valid.
 */
import * as crypto from "crypto";

export interface SEP10JWTClaims {
  /** Issuer — the auth server's Stellar public key. */
  iss: string;
  /** Subject — the authenticated Stellar account (merchant or admin). */
  sub: string;
  /** Merchant identifier resolved during SEP-10 authentication, when available. */
  merchant_id?: string;
  iat: number;
  exp: number;
  /** Optional scope claim, e.g. "merchant" or "admin". */
  scope?: string;
}

export class SEP10JWTError extends Error {}

/**
 * Resolve the expected SEP-10 home domain.
 *
 * `SEP10_HOME_DOMAIN` is required: without it the server cannot distinguish
 * tokens minted by the legitimate SEP-10 server from tokens minted by an
 * attacker-controlled one, so startup must fail fast.
 *
 * @throws {SEP10JWTError} if `SEP10_HOME_DOMAIN` is unset or empty.
 */
export function getExpectedHomeDomain(): string {
  const domain = process.env.SEP10_HOME_DOMAIN;
  if (!domain || domain.trim() === "") {
    throw new SEP10JWTError(
      "SEP10_HOME_DOMAIN is required but not set; refusing to verify SEP-10 JWTs",
    );
  }
  return domain;
}

function base64urlDecode(segment: string): Buffer {
  return Buffer.from(segment, "base64url");
}

/**
 * Verify a SEP-10 JWT's signature, expiry, and issuer.
 *
 * @param token - The compact JWT string from the `Authorization: Bearer <token>` header.
 * @param jwtSecret - Shared secret configured on both auth service and indexer.
 * @param expectedIssuer - The expected `iss` home domain.
 * @throws {SEP10JWTError} if the token is malformed, the signature is invalid, it has
 *   expired, or it was issued by an unexpected issuer.
 */
export function verifySEP10JWT(
  token: string,
  jwtSecret: string,
  expectedIssuer: string = getExpectedHomeDomain(),
): SEP10JWTClaims {
  const parts = token.split(".");
  if (parts.length !== 3) {
    throw new SEP10JWTError("Malformed JWT: expected 3 segments");
  }
  const [headerB64, payloadB64, signatureB64] = parts;

  const message = `${headerB64}.${payloadB64}`;
  const expectedSignature = crypto
    .createHmac("sha256", jwtSecret)
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

  if (typeof claims.iss !== "string" || claims.iss !== expectedIssuer) {
    throw new SEP10JWTError("Invalid issuer");
  }

  const now = Math.floor(Date.now() / 1000);
  if (typeof claims.exp !== "number" || claims.exp < now) {
    throw new SEP10JWTError("JWT expired");
  }

  return claims;
}
