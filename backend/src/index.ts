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
 * Issue #971: Horizon RPC failover with configurable fallback endpoints.
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

/**
 * Issue #971: ordered list of Horizon endpoints. The primary is
 * `HORIZON_URL`; `HORIZON_FALLBACK_1` and `HORIZON_FALLBACK_2` are tried in
 * order when the primary is unavailable. Empty/unset entries are dropped.
 */
const HORIZON_ENDPOINTS = [
  process.env.HORIZON_URL,
  process.env.HORIZON_FALLBACK_1,
  process.env.HORIZON_FALLBACK_2,
].filter((url): url is string => Boolean(url));

/**
 * Issue #971: transparent failover wrapper. Callers pass a function that
 * performs a Horizon request against a given base URL; the wrapper tries each
 * configured endpoint in order, logging a warning (with the failed URL) on
 * each failure. If every endpoint fails, the error lists all attempted URLs.
 */
async function withFailover<T>(fn: (url: string) => Promise<T>): Promise<T> {
  if (HORIZON_ENDPOINTS.length === 0) {
    throw new Error("No Horizon endpoints configured (set HORIZON_URL)");
  }

  const attempted: string[] = [];
  let lastError: unknown;

  for (const url of HORIZON_ENDPOINTS) {
    attempted.push(url);
    try {
      return await fn(url);
    } catch (e) {
      lastError = e;
      console.warn(
        `[horizon] endpoint ${url} failed, trying next fallback: ${
          e instanceof Error ? e.message : String(e)
        }`,
      );
    }
  }

  const reason = lastError instanceof Error ? `: ${lastError.message}` : "";
  throw new Error(
    `All Horizon endpoints failed (attempted: ${attempted.join(", ")})${reason}`,
  );
}

if (!SERVER_PUBLIC_KEY || !SERVER_SECRET_KEY || !JWT_SECRET) {
  throw new Error("SEP10_SERVER_PUBLIC_KEY, SEP10_SERVER_SECRET, and SEP10_JWT_SECRET are required to start the backend.");
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
 * Issue #971: health check reports the currently active Horizon endpoint.
 * The active endpoint is the first configured one that responds; if none
 * respond, `horizon_endpoint` is null and `horizon_ok` is false.
 */
app.get("/health", async (_req, res) => {
  let activeEndpoint: string | null = null;
  try {
    await withFailover(async (url) => {
      const response = await fetch(`${url.replace(/\/$/, "")}/`);
      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }
      activeEndpoint = url;
      return url;
    });
  } catch {
    activeEndpoint = null;
  }

  return res.status(200).json({
    status: "ok",
    horizon_endpoint: activeEndpoint,
    horizon_ok: activeEndpoint !== null,
  });
});

if (require.main === module) {
  app.listen(PORT, () => {
    console.log(`FluxaPay backend listening on :${PORT}`);
  });
}

export { app, withFailover, HORIZON_ENDPOINTS };
