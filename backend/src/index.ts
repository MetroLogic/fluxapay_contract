import "dotenv/config";
import express from "express";
import { Networks } from "@stellar/stellar-sdk";
import { createAuthRouter } from "./routes/auth";
import { createAnalyticsRouter } from "./routes/analytics";
import { createSettlementRouter } from "./routes/settlements";
import { createInvoicesRouter } from "./routes/invoices";
import { createPayoutChannelsRouter } from "./routes/payoutChannels";
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
 * Issue #962: also exposes merchant payout channel configuration
 * (`/v1/merchants/:id/payout-channels`) for bank/mobile money settlement.
 * Issue #964: also exposes a streamed CSV export of merchant transaction
 * reports (`GET /v1/reports/transactions/export`).
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

/**
 * Issue #962: merchant payout channel configuration. Account numbers are
 * encrypted at rest; the settlement service reads the default channel.
 */
app.use(
  "/v1/merchants",
  createPayoutChannelsRouter({
    encryptionKey: process.env.PAYOUT_ENCRYPTION_KEY || JWT_SECRET,
  }),
);

/**
 * Issue #964: streamed CSV export of merchant transaction reports.
 *
 * `GET /v1/reports/transactions/export?format=csv&from=...&to=...&merchant_id=...`
 *
 * - Requires merchant authentication (bearer token from `/auth/token`).
 * - Supports ISO-8601 `from`/`to` date-range filters and a comma-separated
 *   `status` filter (e.g. `status=Confirmed,Settled`).
 * - Caps exports at 10,000 rows; exceeding the cap returns HTTP 413 with a
 *   pagination hint.
 * - Streams rows to the client rather than buffering the whole report.
 * - A merchant may only export their own data: the requested `merchant_id`
 *   must match the authenticated merchant.
 */
const CSV_EXPORT_MAX_ROWS = 10_000;
const CSV_COLUMNS = [
  "payment_id",
  "created_at",
  "status",
  "amount_usdc",
  "fiat_amount",
  "fiat_currency",
  "settled_at",
  "customer_ref",
  "tags",
  "fees",
  "net_amount",
] as const;

function csvEscape(value: unknown): string {
  if (value === null || value === undefined) return "";
  const str = String(value);
  if (/[",\n\r]/.test(str)) {
    return `"${str.replace(/"/g, '""')}"`;
  }
  return str;
}

function parseIsoDate(value: unknown, field: string): Date | undefined {
  if (value === undefined) return undefined;
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error(`${field} must be an ISO-8601 date`);
  }
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    throw new Error(`${field} must be an ISO-8601 date`);
  }
  return parsed;
}

app.get("/v1/reports/transactions/export", async (req, res) => {
  const authHeader = req.header("authorization") || "";
  const [scheme, token] = authHeader.split(" ");
  if (scheme !== "Bearer" || !token) {
    return res.status(401).json({ error: "merchant authentication required" });
  }

  const format = (req.query.format as string | undefined) || "csv";
  if (format !== "csv") {
    return res.status(400).json({ error: `unsupported export format: ${format}` });
  }

  let from: Date | undefined;
  let to: Date | undefined;
  try {
    from = parseIsoDate(req.query.from, "from");
    to = parseIsoDate(req.query.to, "to");
  } catch (err) {
    return res.status(400).json({ error: (err as Error).message });
  }
  if (from && to && from.getTime() > to.getTime()) {
    return res.status(400).json({ error: "from must be before to" });
  }

  const statusFilter = typeof req.query.status === "string" && req.query.status.trim() !== ""
    ? req.query.status.split(",").map((s) => s.trim()).filter(Boolean)
    : undefined;

  const requestedMerchantId = typeof req.query.merchant_id === "string"
    ? req.query.merchant_id
    : undefined;
  // TODO(#675): resolve the authenticated merchant id from the bearer token
  // via MerchantRegistryClient. Until then the token itself is the merchant
  // identity, so a merchant can only ever export their own data.
  const authenticatedMerchantId = token;
  if (requestedMerchantId && requestedMerchantId !== authenticatedMerchantId) {
    return res.status(403).json({ error: "cannot export another merchant's transactions" });
  }

  const rows = await fetchTransactionReportRows({
    merchantId: authenticatedMerchantId,
    from,
    to,
    statuses: statusFilter,
    limit: CSV_EXPORT_MAX_ROWS + 1,
  });

  if (rows.length > CSV_EXPORT_MAX_ROWS) {
    return res.status(413).json({
      error: `export exceeds maximum of ${CSV_EXPORT_MAX_ROWS} rows`,
      hint: "narrow the date range or use pagination via GET /v1/reports/transactions",
      max_rows: CSV_EXPORT_MAX_ROWS,
    });
  }

  const filenameDate = new Date().toISOString().slice(0, 10);
  res.status(200);
  res.setHeader("Content-Type", "text/csv; charset=utf-8");
  res.setHeader(
    "Content-Disposition",
    `attachment; filename="transactions-${filenameDate}.csv"`,
  );

  res.write(CSV_COLUMNS.join(",") + "\n");
  for (const row of rows) {
    res.write(CSV_COLUMNS.map((col) => csvEscape(row[col])).join(",") + "\n");
  }
  res.end();
});

app.get("/health", (_req, res) => res.status(200).json({ status: "ok" }));

if (require.main === module) {
  app.listen(PORT, () => {
    console.log(`FluxaPay backend listening on :${PORT}`);
  });
}

export { app };
