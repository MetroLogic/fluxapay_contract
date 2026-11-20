import "dotenv/config";
import express from "express";
import { Networks, Horizon } from "@stellar/stellar-sdk";
import { createAuthRouter } from "./routes/auth";
import { createAnalyticsRouter } from "./routes/analytics";
import { createSettlementRouter } from "./routes/settlements";
import { createInvoicesRouter } from "./routes/invoices";
import { createPayoutChannelsRouter } from "./routes/payoutChannels";
import { requireJsonContentType } from "./middleware/requireJsonContentType";
import { requestLogger } from "./middleware/requestLogger";
import {
  sanitizeStrings,
  payloadSizeGuard,
  validateBody,
  verifyPaymentSchema,
  createPaymentSchema,
} from "./middleware/validateBody";
import { buildMerchantIdResolver } from "./lib/merchantRegistry";
import { corsMiddleware } from "./middleware/cors";

/**
 * FluxaPay backend API.
 *
 * Issue #675:  SEP-10 merchant authentication endpoints.
 * Issue #789:  GET /v1/analytics/revenue — daily/weekly/monthly breakdown.
 * Issue #803:  GET /v1/payment-links/:id/stats — payment link analytics.
 * Issue #828:  POST /settlements — SEP-6/SEP-24 anchor off-ramp settlement.
 * Issue #838:  Reject non-JSON Content-Type on mutation endpoints (CSRF).
 * Issue #962:  GET/POST /v1/merchants/:id/payout-channels.
 * Issue #964:  GET /v1/reports/transactions/export — streamed CSV export.
 * Issue #971:  Horizon RPC failover with configurable fallback endpoints.
 * Issue #972:  Cross-validate payment tx hash against Horizon.
 * Issue #974:  Stellar Fee-Bump daemon (CAP-0015).
 * Issue #976:  Deposit address validation before create_payment.
 * Issue #981:  Unauthenticated /health endpoint.
 * Issue #984:  POST /v1/invoices/preview — PDF preview without persistence.
 * Issue #998:  Token bucket rate limiting on all public API endpoints.
 * Issue #999:  Interactive API reference at /api-docs (offline Redoc).
 * Issue #1067: Invoice lifecycle routes — list, fetch, void.
 * Issue #1151: createAnalyticsRouter — revenue + payment-link stats.
 * Issue #1153: Resolve merchant id via MerchantRegistryClient.
 * Issue #1154: Structured request logging middleware.
 * Issue #1155: Input validation and sanitization middleware.
 */

// ---------------------------------------------------------------------------
// App setup
// ---------------------------------------------------------------------------

const app = express();

// Issue #1155: reject oversized payloads before parsing.
app.use(payloadSizeGuard());
app.use(express.json());
// Issue #1156: CORS must be mounted before requireJsonContentType so
// preflight OPTIONS requests get CORS headers and never hit the content-type
// guard (which would reject them with 415).
app.use(corsMiddleware);
// Issue #838: must run before route handlers so form-posts never reach auth.
app.use(requireJsonContentType);
// Issue #1155: sanitize all string inputs after JSON parsing.
app.use(sanitizeStrings());
// Issue #1154: structured request logging.
app.use(requestLogger({ jwtSecret: process.env.SEP10_JWT_SECRET }));

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

const PORT = process.env.PORT ? Number(process.env.PORT) : 3001;
const SERVER_PUBLIC_KEY = process.env.SEP10_SERVER_PUBLIC_KEY;
const SERVER_SECRET_KEY = process.env.SEP10_SERVER_SECRET;
const JWT_SECRET = process.env.SEP10_JWT_SECRET;
const NETWORK_PASSPHRASE =
  process.env.STELLAR_NETWORK === "mainnet" ? Networks.PUBLIC : Networks.TESTNET;
const HOME_DOMAIN = process.env.SEP10_HOME_DOMAIN || "fluxapay.stellar.org";

if (!SERVER_PUBLIC_KEY || !SERVER_SECRET_KEY || !JWT_SECRET) {
  throw new Error(
    "SEP10_SERVER_PUBLIC_KEY, SEP10_SERVER_SECRET, and SEP10_JWT_SECRET are required to start the backend.",
  );
}

// ---------------------------------------------------------------------------
// Issue #971: Horizon endpoint failover
// ---------------------------------------------------------------------------

const HORIZON_ENDPOINTS = [
  process.env.HORIZON_URL,
  process.env.HORIZON_FALLBACK_1,
  process.env.HORIZON_FALLBACK_2,
].filter((url): url is string => Boolean(url));

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

// ---------------------------------------------------------------------------
// Issue #972: payment charge lookup (stub — replace with DB/indexer query)
// ---------------------------------------------------------------------------

interface PaymentCharge {
  payment_id: string;
  deposit_address: string;
  amount: string;
  asset?: string;
  memo?: string;
}

interface TxValidationResult {
  ok: boolean;
  error?: string;
}

/** Stub: returns undefined (no charge found). Replace with indexer query. */
function getPaymentCharge(_paymentId: string): PaymentCharge | undefined {
  return undefined;
}

async function validateTransaction(
  txHash: string,
  payment: PaymentCharge,
): Promise<TxValidationResult> {
  const tx = await withFailover(async (url) => {
    const response = await fetch(
      `${url.replace(/\/$/, "")}/transactions/${encodeURIComponent(txHash)}`,
    );
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
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

  const operations = await withFailover(async (url) => {
    const response = await fetch(
      `${url.replace(/\/$/, "")}/transactions/${encodeURIComponent(txHash)}/operations`,
    );
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
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
    if (payment.asset && op.asset_code && op.asset_code !== payment.asset) return false;
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

// ---------------------------------------------------------------------------
// Issue #998: token bucket rate limiting
// ---------------------------------------------------------------------------

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

function consumeToken(
  key: string,
  tier: RateLimitTier,
): { allowed: boolean; remaining: number; resetMs: number } {
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
  const resetMs = Math.ceil((tier.limit - state.tokens) / refillRate / 1000);
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

// ---------------------------------------------------------------------------
// Issue #999: interactive API reference at /api-docs
// ---------------------------------------------------------------------------

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

app.get("/api-docs", (_req, res) => res.type("html").send(API_DOCS_HTML));

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

// ---------------------------------------------------------------------------
// Issue #981: health check configuration
// ---------------------------------------------------------------------------

const HORIZON_URL_HEALTH =
  process.env.HORIZON_URL || "https://horizon-testnet.stellar.org";
const DATABASE_URL = process.env.DATABASE_URL;
const HEALTH_HORIZON_TIMEOUT_MS = Number(process.env.HEALTH_HORIZON_TIMEOUT_MS || 3_000);
const SERVICE_VERSION = process.env.npm_package_version || "0.0.0";
const STARTED_AT = Date.now();

async function checkDatabase(): Promise<string | null> {
  if (!DATABASE_URL) return null;
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

async function checkHorizon(): Promise<string | null> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), HEALTH_HORIZON_TIMEOUT_MS);
  try {
    const response = await fetch(HORIZON_URL_HEALTH, { signal: controller.signal });
    if (!response.ok) return `Horizon responded with status ${response.status}`;
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

// ---------------------------------------------------------------------------
// Issue #1153: MerchantRegistryClient wiring
// ---------------------------------------------------------------------------

const { resolveMerchantId: resolveMerchantIdFromRegistry } =
  buildMerchantIdResolver(process.env);

// ---------------------------------------------------------------------------
// Routes: auth
// ---------------------------------------------------------------------------

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
    // Issue #1153: resolved via MerchantRegistryClient; falls back to the
    // account public key when the registry is unavailable or not configured.
    resolveMerchantId: resolveMerchantIdFromRegistry,
  }),
);

// ---------------------------------------------------------------------------
// Routes: analytics (Issue #1151)
// ---------------------------------------------------------------------------

app.use(
  "/v1/analytics",
  createAnalyticsRouter({
    serverPublicKey: SERVER_PUBLIC_KEY,
    jwtSecret: JWT_SECRET,
    indexerUrl: process.env.INDEXER_URL,
    networkPassphrase: NETWORK_PASSPHRASE,
    homeDomain: HOME_DOMAIN,
    jwtSecret: JWT_SECRET,
  }),
);

// ---------------------------------------------------------------------------
// Routes: settlements (Issue #828)
// ---------------------------------------------------------------------------

app.use(
  "/settlements",
  createSettlementRouter({
    networkPassphrase: NETWORK_PASSPHRASE,
    homeDomain: HOME_DOMAIN,
    anchorUrl: process.env.ANCHOR_URL,
  }),
);

// ---------------------------------------------------------------------------
// Routes: invoices (Issue #1067)
// ---------------------------------------------------------------------------

app.use(
  "/v1/invoices",
  createInvoicesRouter({
    indexerUrl: process.env.INDEXER_URL || "http://localhost:3001",
  }),
);

// ---------------------------------------------------------------------------
// Routes: payout channels (Issue #962)
// ---------------------------------------------------------------------------

app.use(
  "/v1/merchants",
  createPayoutChannelsRouter({
    encryptionKey: process.env.PAYOUT_ENCRYPTION_KEY || JWT_SECRET,
  }),
);

// ---------------------------------------------------------------------------
// Routes: transaction CSV export (Issue #964)
// ---------------------------------------------------------------------------

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

type CsvRow = Partial<Record<(typeof CSV_COLUMNS)[number], unknown>>;

function csvEscape(value: unknown): string {
  if (value === null || value === undefined) return "";
  const str = String(value);
  if (/[",\n\r]/.test(str)) return `"${str.replace(/"/g, '""')}"`;
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

/**
 * Stub: replace with an actual indexer/DB query.
 * Returns an empty list so the endpoint compiles and runs without a database.
 */
async function fetchTransactionReportRows(_opts: {
  merchantId: string;
  from?: Date;
  to?: Date;
  statuses?: string[];
  limit: number;
}): Promise<CsvRow[]> {
  return [];
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

  const statusFilter =
    typeof req.query.status === "string" && req.query.status.trim() !== ""
      ? req.query.status
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean)
      : undefined;

  const requestedMerchantId =
    typeof req.query.merchant_id === "string" ? req.query.merchant_id : undefined;

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
  return res.end();
});

// ---------------------------------------------------------------------------
// Routes: invoice PDF preview (Issue #984)
// ---------------------------------------------------------------------------

const MAX_PREVIEW_PDF_BYTES = 5 * 1024 * 1024;

/**
 * Stub: replace with actual PDF rendering library.
 * Returns a minimal valid PDF so the endpoint runs without a renderer.
 */
async function renderInvoicePdf(_body: Record<string, unknown>): Promise<Buffer> {
  // 1-byte stub — real implementation would call a PDF library.
  return Buffer.from("%PDF-1.4 stub");
}

app.post("/v1/invoices/preview", async (req, res) => {
  const authHeader = req.header("authorization") || "";
  const [scheme] = authHeader.split(" ");
  if (scheme !== "Bearer") {
    return res.status(401).json({ error: "merchant authentication required" });
  }

  try {
    const pdf = await renderInvoicePdf(req.body);
    if (pdf.length > MAX_PREVIEW_PDF_BYTES) {
      return res.status(413).json({ error: "invoice PDF exceeds 5 MB limit" });
    }
    res.setHeader("Content-Type", "application/pdf");
    res.setHeader("Content-Length", String(pdf.length));
    return res.status(200).send(pdf);
  } catch (err) {
    return res.status(400).json({
      error: err instanceof Error ? err.message : "failed to render invoice preview",
    });
  }
});

// ---------------------------------------------------------------------------
// Issue #976: deposit address validation
// ---------------------------------------------------------------------------

const USDC_ASSET_CODE = process.env.USDC_ASSET_CODE || "USDC";
const USDC_ASSET_ISSUER = process.env.USDC_ASSET_ISSUER || "";
const BASE_RESERVE_XLM = 1;
const SUBENTRY_RESERVE_XLM = 0.5;
const FEE_BUFFER_XLM = 0.01;
const DEPOSIT_CHECK_TTL_MS = 60_000;

const horizonUrl =
  process.env.STELLAR_NETWORK === "mainnet"
    ? "https://horizon.stellar.org"
    : "https://horizon-testnet.stellar.org";
const horizonServer = new Horizon.Server(horizonUrl);

type DepositCheckError =
  | "DEPOSIT_ADDRESS_NOT_FUNDED"
  | "USDC_TRUSTLINE_MISSING"
  | "INSUFFICIENT_RESERVE";

interface DepositCheckResult {
  ok: boolean;
  code?: DepositCheckError;
  message?: string;
}

const depositCheckCache = new Map<
  string,
  { result: DepositCheckResult; expiresAt: number }
>();

async function validateDepositAddress(address: string): Promise<DepositCheckResult> {
  const now = Date.now();
  const cached = depositCheckCache.get(address);
  if (cached && cached.expiresAt > now) return cached.result;

  let result: DepositCheckResult;
  try {
    const account = await horizonServer.loadAccount(address);

    const hasUsdcTrustline = account.balances.some(
      (b) =>
        "asset_code" in b &&
        b.asset_code === USDC_ASSET_CODE &&
        (!USDC_ASSET_ISSUER ||
          ("asset_issuer" in b && b.asset_issuer === USDC_ASSET_ISSUER)),
    );
    if (!hasUsdcTrustline) {
      result = {
        ok: false,
        code: "USDC_TRUSTLINE_MISSING",
        message: `deposit address ${address} has no ${USDC_ASSET_CODE} trustline`,
      };
    } else {
      const native = account.balances.find((b) => b.asset_type === "native");
      const xlmBalance = native ? Number(native.balance) : 0;
      const subentries = account.subentry_count || 0;
      const minReserve =
        BASE_RESERVE_XLM + SUBENTRY_RESERVE_XLM * subentries + FEE_BUFFER_XLM;
      if (xlmBalance < minReserve) {
        result = {
          ok: false,
          code: "INSUFFICIENT_RESERVE",
          message: `deposit address ${address} has ${xlmBalance} XLM, needs at least ${minReserve} XLM`,
        };
      } else {
        result = { ok: true };
      }
    }
  } catch (err: any) {
    if (err?.response?.status === 404) {
      result = {
        ok: false,
        code: "DEPOSIT_ADDRESS_NOT_FUNDED",
        message: `deposit address ${address} does not exist on Stellar`,
      };
    } else {
      // Horizon unavailable — do not block payment creation on transient errors.
      return { ok: true };
    }
  }

  depositCheckCache.set(address, { result, expiresAt: now + DEPOSIT_CHECK_TTL_MS });
  return result;
}

// ---------------------------------------------------------------------------
// Routes: payments
// ---------------------------------------------------------------------------

/**
 * POST /v1/payments
 * Issue #976: validates deposit address before returning it.
 * Issue #1155: request body validated against createPaymentSchema.
 */
app.post("/v1/payments", validateBody(createPaymentSchema), async (req, res) => {
  const depositAddress = req.body?.deposit_address;
  if (!depositAddress || typeof depositAddress !== "string") {
    return res.status(400).json({ error: "deposit_address is required" });
  }

  const check = await validateDepositAddress(depositAddress);
  if (!check.ok) {
    return res.status(422).json({ error: check.message, code: check.code });
  }

  return res.status(201).json({ deposit_address: depositAddress });
});

/**
 * POST /v1/payments/:paymentId/verify
 * Issue #972: cross-validates tx hash against Horizon before verify_payment.
 * Issue #1155: request body validated against verifyPaymentSchema.
 */
app.post(
  "/v1/payments/:paymentId/verify",
  validateBody(verifyPaymentSchema),
  async (req, res) => {
    const { tx_hash: txHash } = req.body ?? {};
    if (typeof txHash !== "string" || txHash.length === 0) {
      return res.status(400).json({ error: "tx_hash is required" });
    }

    const payment = getPaymentCharge(req.params.paymentId);
    if (!payment) {
      return res.status(404).json({ error: `payment ${req.params.paymentId} not found` });
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

    return res.status(200).json({
      payment_id: payment.payment_id,
      tx_hash: txHash,
      verified: true,
    });
  },
);

// ---------------------------------------------------------------------------
// Routes: health (Issue #981)
// ---------------------------------------------------------------------------

app.get("/health", async (_req, res) => {
  const [dbError, horizonError] = await Promise.all([checkDatabase(), checkHorizon()]);
  const healthy = !dbError && !horizonError;

  const body: Record<string, unknown> = {
    status: healthy ? "ok" : "degraded",
    version: SERVICE_VERSION,
    database: dbError ? "disconnected" : "connected",
    horizon: horizonError ? "disconnected" : "connected",
    horizon_url: HORIZON_URL_HEALTH,
    uptime_secs: Math.floor((Date.now() - STARTED_AT) / 1000),
  };

  if (!healthy) body.error = dbError || horizonError;

  return res.status(healthy ? 200 : 503).json(body);
});

// ---------------------------------------------------------------------------
// Issue #974: Fee-Bump daemon
// ---------------------------------------------------------------------------

const FEE_BUMP_THRESHOLD_SECS = Number(process.env.FEE_BUMP_THRESHOLD_SECS || 30);
const FEE_BUMP_MAX_STROOPS = Number(process.env.FEE_BUMP_MAX_STROOPS || 10_000);
const FEE_BUMP_INTERVAL_MS = 30_000;

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
  timer.unref?.();
  return timer;
}

// ---------------------------------------------------------------------------
// Entrypoint
// ---------------------------------------------------------------------------

if (require.main === module) {
  app.listen(PORT, () => {
    console.log(`FluxaPay backend listening on :${PORT}`);
  });
  startFeeBumpDaemon();
}

export {
  app,
  startFeeBumpDaemon,
  runFeeBumpSweep,
  validateDepositAddress,
  withFailover,
  HORIZON_ENDPOINTS,
  validateTransaction,
};
