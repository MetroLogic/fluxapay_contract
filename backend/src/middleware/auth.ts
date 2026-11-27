/**
 * requireAuth middleware — verifies the merchant bearer token (HS256 JWT)
 * issued by the SEP-10 flow and attaches the decoded merchant to the request.
 *
 * Usage:
 *   router.get('/protected', requireAuth, handler)
 *
 * The JWT secret is read from the SEP10_JWT_SECRET environment variable at
 * module load time. All requests missing a valid token are rejected with 401.
 */

import { Request, Response, NextFunction } from "express";
import { createHmac } from "crypto";

declare global {
  // eslint-disable-next-line @typescript-eslint/no-namespace
  namespace Express {
    interface Request {
      merchant?: { id: string; account: string };
    }
  }
}

const JWT_SECRET = process.env.SEP10_JWT_SECRET ?? "";

function verifyToken(
  token: string,
  secret: string,
): Record<string, unknown> | null {
  const parts = token.split(".");
  if (parts.length !== 3) return null;
  const [header, body, sig] = parts;
  const expected = createHmac("sha256", secret)
    .update(`${header}.${body}`)
    .digest("base64url");
  if (sig !== expected) return null;
  try {
    const payload = JSON.parse(
      Buffer.from(body, "base64url").toString("utf8"),
    ) as Record<string, unknown>;
    const now = Math.floor(Date.now() / 1000);
    if (typeof payload.exp === "number" && payload.exp < now) return null;
    return payload;
  } catch {
    return null;
  }
}

export function requireAuth(
  req: Request,
  res: Response,
  next: NextFunction,
): void {
  const authHeader = req.header("authorization") ?? "";
  const [scheme, token] = authHeader.split(" ");

  if (scheme !== "Bearer" || !token) {
    res.status(401).json({ error: "merchant authentication required" });
    return;
  }

  const payload = verifyToken(token, JWT_SECRET);
  if (!payload) {
    res.status(401).json({ error: "invalid or expired token" });
    return;
  }

  const merchantId =
    typeof payload.merchant_id === "string"
      ? payload.merchant_id
      : typeof payload.sub === "string"
        ? payload.sub
        : token;

  const account =
    typeof payload.sub === "string" ? payload.sub : merchantId;

  req.merchant = { id: merchantId, account };
  next();
}
