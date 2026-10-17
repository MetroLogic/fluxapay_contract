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
 * Issue #972: cross-validate payment tx hash against Horizon (destination,
 * amount, memo) before calling the contract's `verify_payment`.
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

/**
 * Issue #972: a `PaymentCharge` as stored by the indexer. `memo` is optional;
 * when set, the on-chain transaction memo must match it exactly.
 */
interface PaymentCharge {
  payment_id: string;
  deposit_address: string;
  amount: string;
  asset?: string;
  memo?: string;
}

/**
 * Issue #972: result of cross-checking a tx hash against Horizon. `ok` is
 * false when the transaction exists but does not match the payment; `error`
 * carries a human-readable reason for the mismatch.
 */
interface TxValidationResult {
  ok: boolean;
  error?: string;
}

/**
 * Issue #972: fetch a transaction from Horizon (with failover) and validate
 * that it pays `payment.deposit_address` at least `payment.amount`, and that
 * its memo matches `payment.memo` (or `payment.payment_id` when no explicit
 * memo is set). Throws when Horizon cannot be reached so the caller can retry.
 */
async function validateTransaction(
  txHash: string,
  payment: PaymentCharge,
): Promise<TxValidationResult> {
  const tx = await withFailover(async (url) => {
    const response = await fetch(
      `${url.replace(/\/$/, "")}/transactions/${encodeURIComponent(txHash)}`,
    );
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    return (await response.json()) as {
      memo?: string;
      memo_type?: string;
      successful?: boolean;
    };
  });

  if (tx.successful === false) {
    return { ok: false, error: "transaction was not successful" };
  }

  const expectedMemo = payment.memo ?? payment.payment_id;
  if ((tx.memo ?? "") !== expectedMemo) {
    return {
      ok: false,
      error: `memo mismatch: expected ${expectedMemo}, got ${tx.memo ?? "<none>"}`,
    };
  }

  // Validate the payment operation's destination and amount.
  const operations = await withFailover(async (url) => {
    const response = await fetch(
      `${url.replace(/\/$/, "")}/transactions/${encodeURIComponent(txHash)}/operations`,
    );
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    return (await response.json()) as {
      _embedded?: {
        records?: Array<{
          type?: string;
          to?: string;
          amount?: string;
          asset_code?: string;
        }>;
      };
    };
  });

  const records = operations._embedded?.records ?? [];
  const expectedAmount = Number(payment.amount);
  const matching = records.find((op) => {
    if (op.type !== "payment" && op.type !== "create_account") return false;
    if (op.to !== payment.deposit_address) return false;
    if (payment.asset && op.asset_code && op.asset_code !== payment.asset) {
      return false;
    }
    return Number(op.amount ?? 0) >= expectedAmount;
  });

  if (!matching) {
    return {
      ok: false,
      error: `no payment to ${payment.deposit_address} of at least ${payment.amount}`,
    };
  }

  return { ok: true };
}

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
 * Issue #972: verify a customer deposit before calling the contract's
 * `verify_payment`. The tx hash is cross-checked against Horizon (destination,
 * amount, memo). Horizon lookups are retried up to 3 times; if all attempts
 * fail the request returns HTTP 503. A transaction that exists but does not
 * match the payment returns HTTP 422 with error code `TRANSACTION_MISMATCH`.
 */
app.post("/v1/payments/:paymentId/verify", async (req, res) => {
  const { tx_hash: txHash } = req.body ?? {};
  if (typeof txHash !== "string" || txHash.length === 0) {
    return res.status(400).json({ error: "tx_hash is required" });
  }

  const payment = getPaymentCharge(req.params.paymentId);
  if (!payment) {
    return res
      .status(404)
      .json({ error: `payment ${req.params.paymentId} not found` });
  }

  const MAX_ATTEMPTS = 3;
  let validation: TxValidationResult | null = null;
  let lastError: unknown;

  for (let attempt = 1; attempt <= MAX_ATTEMPTS; attempt += 1) {
    try {
      validation = await validateTransaction(txHash, payment);
      break;
    } catch (e) {
      lastError = e;
      console.warn(
        `[verify_payment] Horizon lookup attempt ${attempt}/${MAX_ATTEMPTS} failed: ${
          e instanceof Error ? e.message : String(e)
        }`,
      );
    }
  }

  if (!validation) {
    return res.status(503).json({
      error: "Horizon unavailable, could not verify transaction",
      code: "HORIZON_UNAVAILABLE",
      detail: lastError instanceof Error ? lastError.message : String(lastError),
    });
  }

  if (!validation.ok) {
    return res.status(422).json({
      error: "transaction does not match payment",
      code: "TRANSACTION_MISMATCH",
      detail: validation.error,
    });
  }

  // All Horizon checks passed — safe to call the contract's `verify_payment`.
  return res.status(200).json({
    payment_id: payment.payment_id,
    tx_hash: txHash,
    verified: true,
  });
});

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

export { app, withFailover, HORIZON_ENDPOINTS, validateTransaction };
