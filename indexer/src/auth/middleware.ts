/**
 * Issue #672: SEP-10 authentication middleware for the indexer REST API.
 *
 * Wires the indexer's Express routes to require a valid SEP-10 JWT
 * (see `sep10-jwt.ts`) instead of a static API key:
 * - `requireSEP10Auth` verifies the bearer token and attaches `req.auth`.
 * - `requireMerchantScope` ensures merchant-scoped endpoints (e.g.
 *   `GET /merchants/:merchantId/*`) only return data for the merchant the
 *   token was issued to (unless the caller is an admin).
 * - `requireAdmin` restricts admin-only endpoints to accounts listed in
 *   `INDEXER_ADMIN_ACCOUNTS`.
 */
import type { NextFunction, Request, Response } from "express";
import { SEP10JWTError, verifySEP10JWT, type SEP10JWTClaims } from "./sep10-jwt";
import type { SEP10AuthConfig } from "./config";

declare global {
  // eslint-disable-next-line @typescript-eslint/no-namespace
  namespace Express {
    interface Request {
      auth?: SEP10JWTClaims;
    }
  }
}

function extractBearerToken(req: Request): string | null {
  const header = req.headers.authorization;
  if (!header || !header.startsWith("Bearer ")) {
    return null;
  }
  return header.slice("Bearer ".length).trim() || null;
}

/**
 * Require a valid SEP-10 JWT on the request. On success, attaches the
 * decoded claims to `req.auth`. Responds 401 otherwise.
 */
export function requireSEP10Auth(config: SEP10AuthConfig) {
  return (req: Request, res: Response, next: NextFunction): void => {
    const token = extractBearerToken(req);
    if (!token) {
      res.status(401).json({ error: "Missing bearer token" });
      return;
    }

    if (!config.serverPublicKey) {
      res.status(500).json({ error: "SEP-10 auth not configured (STELLAR_SERVER_PUBLIC_KEY unset)" });
      return;
    }

    try {
      req.auth = verifySEP10JWT(token, config.serverPublicKey);
      next();
    } catch (error) {
      const message = error instanceof SEP10JWTError ? error.message : "Invalid token";
      res.status(401).json({ error: message });
    }
  };
}

function isAdmin(claims: SEP10JWTClaims | undefined, config: SEP10AuthConfig): boolean {
  return !!claims && config.adminAccounts.has(claims.sub);
}

/**
 * Restrict a `:merchantId`-scoped route to the merchant the JWT was issued
 * for, or to admin accounts. Must run after `requireSEP10Auth`.
 */
export function requireMerchantScope(paramName: string, config: SEP10AuthConfig) {
  return (req: Request, res: Response, next: NextFunction): void => {
    if (!req.auth) {
      res.status(401).json({ error: "Authentication required" });
      return;
    }

    const merchantId = req.params[paramName];
    if (req.auth.sub !== merchantId && !isAdmin(req.auth, config)) {
      res.status(403).json({ error: "Token is not authorized for this merchant" });
      return;
    }

    next();
  };
}

/**
 * Restrict a route to accounts listed in `INDEXER_ADMIN_ACCOUNTS`. Must run
 * after `requireSEP10Auth`.
 */
export function requireAdmin(config: SEP10AuthConfig) {
  return (req: Request, res: Response, next: NextFunction): void => {
    if (!req.auth) {
      res.status(401).json({ error: "Authentication required" });
      return;
    }

    if (!isAdmin(req.auth, config)) {
      res.status(403).json({ error: "Admin account required" });
      return;
    }

    next();
  };
}
