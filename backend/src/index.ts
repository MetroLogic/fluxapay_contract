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
 * Issue #981: adds an unauthenticated `/health` endpoint reporting database
 * and Horizon connectivity for load balancers and readiness probes.
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
 * Issue #981: health check configuration.
 */
const HORIZON_URL = process.env.HORIZON_URL || "https://horizon-testnet.stellar.org";
const DATABASE_URL = process.env.DATABASE_URL;
const HEALTH_HORIZON_TIMEOUT_MS = Number(process.env.HEALTH_HORIZON_TIMEOUT_MS || 3_000);
const SERVICE_VERSION = process.env.npm_package_version || "0.0.0";
const STARTED_AT = Date.now();

/**
 * Issue #981: ping the database. Returns null when healthy, or an error
 * message describing the failure. When no DATABASE_URL is configured the
 * database is treated as not configured and reported as connected.
 */
async function checkDatabase(): Promise<string | null> {
  if (!DATABASE_URL) {
    return null;
  }
  try {
    const { Pool } = await import("pg");
    const pool = new Pool({ connectionString: DATABASE_URL, connectionTimeoutMillis: 2_000 });
    try {
      await pool.query("SELECT 1");
      return null;
    } finally {
      await pool.end();
    }
  } catch (err) {
    return err instanceof Error ? err.message : "database ping failed";
  }
}

/**
 * Issue #981: ping Horizon. Returns null when healthy, or an error message
 * describing the failure (including timeouts).
 */
async function checkHorizon(): Promise<string | null> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), HEALTH_HORIZON_TIMEOUT_MS);
  try {
    const response = await fetch(HORIZON_URL, { signal: controller.signal });
    if (!response.ok) {
      return `Horizon responded with status ${response.status}`;
    }
    return null;
  } catch (err) {
    if (err instanceof Error && err.name === "AbortError") {
      return `Horizon request timed out after ${HEALTH_HORIZON_TIMEOUT_MS}ms`;
    }
    return err instanceof Error ? err.message : "Horizon request failed";
  } finally {
    clearTimeout(timer);
  }
}

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

/**
 * Issue #981: unauthenticated health check endpoint. Reports database and
 * Horizon connectivity so load balancers and readiness probes can detect a
 * degraded service. Returns 200 when healthy, 503 when degraded.
 */
app.get("/health", async (_req, res) => {
  const [dbError, horizonError] = await Promise.all([checkDatabase(), checkHorizon()]);
  const healthy = !dbError && !horizonError;

  const body: Record<string, unknown> = {
    status: healthy ? "ok" : "degraded",
    version: SERVICE_VERSION,
    database: dbError ? "disconnected" : "connected",
    horizon: horizonError ? "disconnected" : "connected",
    horizon_url: HORIZON_URL,
    uptime_secs: Math.floor((Date.now() - STARTED_AT) / 1000),
  };

  if (!healthy) {
    body.error = dbError || horizonError;
  }

  return res.status(healthy ? 200 : 503).json(body);
});

if (require.main === module) {
  app.listen(PORT, () => {
    console.log(`FluxaPay backend listening on :${PORT}`);
  });
}

export { app };
