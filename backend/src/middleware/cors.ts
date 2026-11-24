import { Request, Response, NextFunction } from "express";

/**
 * Issue #1156: Configurable CORS middleware for the merchant dashboard SPA.
 *
 * The dashboard is a browser SPA that needs to make credentialed (JWT-bearing)
 * cross-origin requests to the backend. Without CORS headers the browser
 * blocks every request.
 *
 * Allowed origins are env-configurable via `CORS_ALLOWED_ORIGINS` (comma-
 * separated). Safe defaults cover the common local development ports for the
 * Next.js dashboard. Preflight (`OPTIONS`) is handled explicitly. Credentials
 * are enabled so `Authorization` headers and cookies flow cross-origin.
 */

const DEFAULT_ALLOWED_ORIGINS = [
  "http://localhost:3000",
  "http://localhost:3001",
];

function parseAllowedOrigins(): string[] {
  const raw = process.env.CORS_ALLOWED_ORIGINS;
  if (!raw || raw.trim() === "") {
    return DEFAULT_ALLOWED_ORIGINS;
  }
  return raw
    .split(",")
    .map((o) => o.trim())
    .filter((o) => o.length > 0);
}

const ALLOWED_ORIGINS = parseAllowedOrigins();

const ALLOWED_METHODS = [
  "GET",
  "POST",
  "PUT",
  "PATCH",
  "DELETE",
  "OPTIONS",
].join(", ");

const ALLOWED_HEADERS = [
  "Content-Type",
  "Authorization",
  "X-Requested-With",
  "Accept",
].join(", ");

const MAX_AGE_SECONDS = 86400;

function isOriginAllowed(origin: string | undefined): boolean {
  if (!origin) return false;
  return ALLOWED_ORIGINS.includes(origin);
}

/**
 * CORS middleware. Must be mounted before route handlers and before
 * `requireJsonContentType` so that preflight `OPTIONS` requests are answered
 * with the correct CORS headers and never reach the content-type guard.
 */
export function corsMiddleware(
  req: Request,
  res: Response,
  next: NextFunction,
): void {
  const origin = req.header("origin");

  if (isOriginAllowed(origin)) {
    res.setHeader("Access-Control-Allow-Origin", origin as string);
    res.setHeader("Vary", "Origin");
    res.setHeader("Access-Control-Allow-Credentials", "true");
    res.setHeader(
      "Access-Control-Expose-Headers",
      "X-RateLimit-Limit, X-RateLimit-Remaining, X-RateLimit-Reset",
    );
  }

  if (req.method === "OPTIONS") {
    res.setHeader("Access-Control-Allow-Methods", ALLOWED_METHODS);
    res.setHeader("Access-Control-Allow-Headers", ALLOWED_HEADERS);
    res.setHeader("Access-Control-Max-Age", String(MAX_AGE_SECONDS));
    res.status(204).end();
    return;
  }

  next();
}
