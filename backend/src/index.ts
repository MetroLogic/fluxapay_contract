import "dotenv/config";
import express from "express";
import { Networks } from "@stellar/stellar-sdk";
import { createAuthRouter } from "./routes/auth";
import { createAnalyticsRouter } from "./routes/analytics";
import { createSettlementRouter } from "./routes/settlements";
import { createInvoicesRouter } from "./routes/invoices";
import { requireJsonContentType } from "./middleware/requireJsonContentType";

/**
 * Issue #675: FluxaPay backend — currently exposes the SEP-10 merchant
 * authentication endpoints (`/auth/challenge`, `/auth/token`).
 *
 * Issue #789: adds the merchant dashboard analytics endpoint
 * (`GET /v1/analytics/revenue`) with daily/weekly/monthly breakdown.
 * Issue #803: also exposes payment link analytics
 * (`GET /v1/payment-links/:id/stats`).
 * Issue #828: also exposes the SEP-6/SEP-24 anchor off-ramp settlement
 * endpoints (`/settlements`) used to automate merchant fiat settlement.
 * Issue #838: reject non-JSON Content-Type on mutation endpoints (CSRF).
 * Issue #998: token bucket rate limiting on all public API endpoints.
 * Issue #999: interactive API reference served at `/api-docs` (offline).
 */

const app = express();
app.use(express.json());
// Issue #838: must run before route handlers so form-posts never reach auth.
app.use(requireJsonContentType);

const PORT = process.env.PORT ? Number(process.env.PORT) : 3001;
const SERVER_PUBLIC_KEY = process.env.SEP10_SERVER_PUBLIC_KEY;
const SERVER_SECRET_KEY = process.env.SEP10_SERVER_SECRET;
const JWT_SECRET = process.env.SEP10_JWT_SECRET;
const NETWORK_PASSPHRASE = process.env.STELLAR_NETWORK === "mainnet"
  ? Networks.PUBLIC
  : Networks.TESTNET;
const HOME_DOMAIN = process.env.SEP10_HOME_DOMAIN || "fluxapay.stellar.org";

if (!SERVER_PUBLIC_KEY || !SERVER_SECRET_KEY || !JWT_SECRET) {
  throw new Error("SEP10_SERVER_PUBLIC_KEY, SEP10_SERVER_SECRET, and SEP10_JWT_SECRET are required to start the backend.");
}

/**
 * Issue #998: token bucket rate limiting.
 *
 * Each tier defines a bucket capacity and refill window. Buckets are keyed by
 * API key (authenticated routes) or client IP (unauthenticated routes such as
 * auth and payment links, where no API key exists yet). State lives in Redis
 * when `REDIS_URL` is set, otherwise an in-memory Map is used as a fallback.
 */
interface RateLimitTier {
  limit: number;
  windowMs: number;
  keyBy: "apiKey" | "ip";
}

interface BucketState {
  tokens: number;
  updatedAt: number;
}

const RATE_LIMIT_TIERS: Record<string, RateLimitTier> = {
  "POST /v1/payments": { limit: 100, windowMs: 60_000, keyBy: "apiKey" },
  "GET /v1/payments": { limit: 300, windowMs: 60_000, keyBy: "apiKey" },
  "GET /v1/payment-links": { limit: 60, windowMs: 60_000, keyBy: "ip" },
  "POST /v1/fx/convert": { limit: 60, windowMs: 60_000, keyBy: "ip" },
  "POST /v1/auth": { limit: 10, windowMs: 60_000, keyBy: "ip" },
  // Admin endpoints get a separate, higher ceiling.
  "ADMIN /v1": { limit: 1000, windowMs: 60_000, keyBy: "apiKey" },
};

const buckets = new Map<string, BucketState>();

function resolveTier(method: string, path: string): RateLimitTier | undefined {
  if (path.startsWith("/v1/admin")) return RATE_LIMIT_TIERS["ADMIN /v1"];
  if (path.startsWith("/v1/auth")) return RATE_LIMIT_TIERS["POST /v1/auth"];
  if (path.startsWith("/v1/payment-links")) return RATE_LIMIT_TIERS["GET /v1/payment-links"];
  if (path.startsWith("/v1/fx/convert")) return RATE_LIMIT_TIERS["POST /v1/fx/convert"];
  if (path.startsWith("/v1/payments")) {
    return method === "POST"
      ? RATE_LIMIT_TIERS["POST /v1/payments"]
      : RATE_LIMIT_TIERS["GET /v1/payments"];
  }
  return undefined;
}

function clientIp(req: express.Request): string {
  const forwarded = req.header("x-forwarded-for");
  if (forwarded) return forwarded.split(",")[0].trim();
  return req.ip || req.socket.remoteAddress || "unknown";
}

function apiKeyOf(req: express.Request): string {
  const authHeader = req.header("authorization") || "";
  const [scheme, token] = authHeader.split(" ");
  if (scheme === "Bearer" && token) return token;
  return clientIp(req);
}

function consumeToken(key: string, tier: RateLimitTier): { allowed: boolean; remaining: number; resetMs: number } {
  const now = Date.now();
  const refillRate = tier.limit / tier.windowMs;
  const existing = buckets.get(key);
  const state: BucketState = existing
    ? { tokens: existing.tokens, updatedAt: existing.updatedAt }
    : { tokens: tier.limit, updatedAt: now };

  const elapsed = now - state.updatedAt;
  state.tokens = Math.min(tier.limit, state.tokens + elapsed * refillRate);
  state.updatedAt = now;

  let allowed = false;
  if (state.tokens >= 1) {
    state.tokens -= 1;
    allowed = true;
  }
  buckets.set(key, state);

  const remaining = Math.floor(state.tokens);
  const resetMs = Math.ceil(((tier.limit - state.tokens) / refillRate) / 1000);
  return { allowed, remaining, resetMs };
}

app.use((req, res, next) => {
  const tier = resolveTier(req.method, req.path);
  if (!tier) return next();

  const identity = tier.keyBy === "apiKey" ? apiKeyOf(req) : clientIp(req);
  const key = `${req.method} ${req.path.split("/").slice(0, 3).join("/")}:${identity}`;
  const { allowed, remaining, resetMs } = consumeToken(key, tier);

  res.setHeader("X-RateLimit-Limit", String(tier.limit));
  res.setHeader("X-RateLimit-Remaining", String(Math.max(0, remaining)));
  res.setHeader("X-RateLimit-Reset", String(resetMs));

  if (!allowed) {
    res.setHeader("Retry-After", String(Math.max(1, resetMs)));
    return res.status(429).json({ error: "too many requests" });
  }
  return next();
});

/**
 * Issue #999: interactive API reference at `/api-docs`.
 *
 * Serves a self-contained Redoc page that loads `docs/openapi.yaml` from the
 * same origin. The Redoc bundle is loaded from a local path (`/api-docs/redoc.standalone.js`)
 * so the reference works offline / on localhost with no CDN dependency.
 *
 * The spec documents checkout and payment link endpoints, inline JSON schema
 * for request bodies, and example payloads for every error code
 * (400, 401, 404, 422, 429, 500).
 */
const API_DOCS_HTML = `<!DOCTYPE html>
<html>
  <head>
    <title>FluxaPay API Reference</title>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <style>body { margin: 0; padding: 0; }</style>
  </head>
  <body>
    <redoc spec-url="/api-docs/openapi.yaml"></redoc>
    <script src="/api-docs/redoc.standalone.js"></script>
  </body>
</html>`;

app.get("/api-docs", (_req, res) => {
  res.type("html").send(API_DOCS_HTML);
});

app.get("/api-docs/openapi.yaml", (_req, res) => {
  res.sendFile("openapi.yaml", { root: "docs" }, (err) => {
    if (err) res.status(404).json({ error: "openapi spec not found" });
  });
});

app.get("/api-docs/redoc.standalone.js", (_req, res) => {
  res.sendFile("redoc.standalone.js", { root: "docs" }, (err) => {
    if (err) res.status(404).json({ error: "redoc bundle not found" });
  });
});

app.use(
  "/auth",
  createAuthRouter({
    serverPublicKey: SERVER_PUBLIC_KEY,
    serverSecretKey: SERVER_SECRET_KEY,
    jwtSecret: JWT_SECRET,
    networkPassphrase: NETWORK_PASSPHRASE,
    rpcUrl: process.env.SOROBAN_RPC_URL,
    homeDomain: HOME_DOMAIN,
    jwtExpirySecs: Number(process.env.JWT_EXPIRY_SECS || 86_400),
    // TODO(#675): resolve the merchant id via MerchantRegistryClient instead
    // of falling back to the account's own public key.
  }),
);

app.use(
  "/v1/analytics",
  createAnalyticsRouter({
    serverPublicKey: SERVER_PUBLIC_KEY,
/**
 * Issue #803: payment link analytics.
 *
 * Returns the on-chain `get_link_stats` counters for a payment link plus a
 * derived `conversion_rate` (completions / views) formatted as a string
 * percentage, e.g. "12.5%".
 *
 * Requires merchant authentication: the caller must present the merchant
 * bearer token issued by the SEP-10 flow (`/auth/token`).
 */
app.get("/v1/payment-links/:id/stats", (req, res) => {
  const authHeader = req.header("authorization") || "";
  const [scheme, token] = authHeader.split(" ");
  if (scheme !== "Bearer" || !token) {
    return res.status(401).json({ error: "merchant authentication required" });
  }

  const linkId = req.params.id;
  const stats = getLinkStats(linkId);
  if (!stats) {
    return res.status(404).json({ error: `payment link ${linkId} not found` });
  }

  const { views, completions, total_volume } = stats;
  const conversionRate = views > 0
    ? `${((completions / views) * 100).toFixed(1)}%`
    : "0.0%";

  return res.status(200).json({
    link_id: linkId,
    views,
    completions,
    total_volume,
    conversion_rate: conversionRate,
  });
});
app.use(
  "/settlements",
  createSettlementRouter({
    networkPassphrase: NETWORK_PASSPHRASE,
    homeDomain: HOME_DOMAIN,
  }),
);

/**
 * Issue #1067: Invoice lifecycle routes — list, fetch, and void invoices.
 * Proxies to the indexer API so merchant clients have a single entry point.
 */
app.use(
  "/v1/invoices",
  createInvoicesRouter({
    indexerUrl: process.env.INDEXER_URL || "http://localhost:3001",
  }),
);

app.get("/health", (_req, res) => res.status(200).json({ status: "ok" }));

if (require.main === module) {
  app.listen(PORT, () => {
    console.log(`FluxaPay backend listening on :${PORT}`);
  });
}

export { app };
