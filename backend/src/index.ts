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
 * Issue #974: starts the Stellar Fee-Bump daemon that re-wraps stuck
 * pending payments with a higher fee (CAP-0015).
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

/**
 * Issue #974: Fee-Bump daemon configuration.
 *
 * `FEE_BUMP_THRESHOLD_SECS` — how long a payment may sit in `Pending` before
 * it is considered stuck (default 30s).
 * `FEE_BUMP_MAX_STROOPS` — the maximum fee (in stroops) FluxaPay is willing
 * to pay when re-wrapping a stuck transaction. Charged to FluxaPay's
 * operational fee account, never the merchant.
 */
const FEE_BUMP_THRESHOLD_SECS = Number(process.env.FEE_BUMP_THRESHOLD_SECS || 30);
const FEE_BUMP_MAX_STROOPS = Number(process.env.FEE_BUMP_MAX_STROOPS || 10_000);
const FEE_BUMP_INTERVAL_MS = 30_000;

/**
 * Issue #974: scan for payments stuck in `Pending` past the threshold and
 * bump their fee via a CAP-0015 Fee-Bump transaction.
 *
 * The actual Horizon interaction lives in the indexer's fee-bump module; this
 * daemon only schedules the sweep and logs the outcome so the backend process
 * owns the lifecycle.
 */
async function runFeeBumpSweep(): Promise<void> {
  try {
    const { bumpStuckPayments } = await import("./services/feeBump");
    const bumped = await bumpStuckPayments({
      thresholdSecs: FEE_BUMP_THRESHOLD_SECS,
      maxFeeStroops: FEE_BUMP_MAX_STROOPS,
      networkPassphrase: NETWORK_PASSPHRASE,
    });
    if (bumped > 0) {
      console.log(`[fee-bump] bumped ${bumped} stuck pending payment(s)`);
    }
  } catch (err) {
    console.error("[fee-bump] sweep failed", err);
  }
}

function startFeeBumpDaemon(): NodeJS.Timeout {
  const timer = setInterval(runFeeBumpSweep, FEE_BUMP_INTERVAL_MS);
  // Do not keep the event loop alive solely for the daemon.
  timer.unref?.();
  return timer;
}

if (require.main === module) {
  app.listen(PORT, () => {
    console.log(`FluxaPay backend listening on :${PORT}`);
  });
  startFeeBumpDaemon();
}

export { app, startFeeBumpDaemon, runFeeBumpSweep };
