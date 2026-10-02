import express, { type Request, type Response, type NextFunction } from "express";
import * as dotenv from "dotenv";
import { z } from "zod";
import { Database } from "./database";
import { requireApiKey, requireAdminApiKey, requireScope } from "./auth/api-key";
import { requireSEP10Auth } from "./auth/middleware";
import { loadSEP10AuthConfig } from "./auth/config";
import { sseManager } from "./sse";
import {
  getPlatformEd25519PublicKey,
  registerWebhookRoutes,
  WebhookStore,
} from "./webhooks";
import { getCachedRate } from "./fx-rate-cache";
import { createRateLimitMiddleware } from "./rate-limit";

dotenv.config();

export type ReplayDLQHandler = () => Promise<{ attempted: number; succeeded: number; failed: number }>;

export interface ReplayProgressUpdate {
  processed: number;
  total: number;
  stored?: number;
  currentLedger?: number;
}

export type EventReplayHandler = (
  fromLedger: number,
  toLedger: number,
  onProgress?: (progress: ReplayProgressUpdate) => void,
) => Promise<{ processed: number; stored: number; total: number }>;

export const MAX_REPLAY_LEDGER_RANGE = 10000;

/** Issue #839: stroops use 7 decimal places for USDC amounts. */
const USDC_DECIMALS = 7;
/** Issue #839: default staleness threshold (seconds) for convert preview. */
const DEFAULT_FX_STALENESS_SECS = Number(process.env.FX_STALENESS_SECS || 300);

/** Issue #839: simple per-IP sliding window rate limiter (60 req/min). */
const FX_CONVERT_LIMIT = 60;
const FX_CONVERT_WINDOW_MS = 60_000;
const fxConvertHits = new Map<string, number[]>();

function fxConvertRateLimit(req: Request, res: Response, next: NextFunction): void {
  const ip = (req.ip || req.socket.remoteAddress || "unknown").toString();
  const now = Date.now();
  const windowStart = now - FX_CONVERT_WINDOW_MS;
  const recent = (fxConvertHits.get(ip) || []).filter((t) => t > windowStart);
  if (recent.length >= FX_CONVERT_LIMIT) {
    res.status(429).json({ error: "Rate limit exceeded: 60 requests per minute" });
    return;
  }
  recent.push(now);
  fxConvertHits.set(ip, recent);
  next();
}

function formatFixed(n: number, decimals: number): string {
  return n.toFixed(decimals);
}

// Issue #785: query schema for the filtered payments endpoint.
const isoDateString = z
  .string()
  .refine((value) => !Number.isNaN(Date.parse(value)), { message: "Invalid ISO-8601 date" });

const paymentsQuerySchema = z.object({
  merchant_id: z.string().min(1).max(128),
  status: z
    .string()
    .optional()
    .transform((value) =>
      value
        ? value
            .split(",")
            .map((s) => s.trim())
            .filter((s) => s.length > 0)
        : undefined,
    ),
  from: isoDateString.optional(),
  to: isoDateString.optional(),
  limit: z.coerce.number().int().min(1).max(100).optional().default(50),
  cursor: z.string().max(512).optional(),
});

/**
 * Decode an opaque pagination cursor of the form base64url(created_at|id).
 * Returns null when the cursor is malformed so the caller can reject it.
 */
function decodePaymentsCursor(cursor: string): { createdAt: string; id: string } | null {
  try {
    const decoded = Buffer.from(cursor, "base64url").toString("utf8");
    const separator = decoded.lastIndexOf("|");
    if (separator <= 0) return null;
    const createdAt = decoded.slice(0, separator);
    const id = decoded.slice(separator + 1);
    if (!createdAt || !id) return null;
    return { createdAt, id };
  } catch {
    return null;
  }
}

function encodePaymentsCursor(createdAt: string, id: string): string {
  return Buffer.from(`${createdAt}|${id}`, "utf8").toString("base64url");
}

export function createServer(
  database: Database,
  replayDlqHandler?: ReplayDLQHandler,
  eventReplayHandler?: EventReplayHandler,
) {
  const app = express();
  app.use(express.json());

  // Issue #817: rate limit ALL routes (auth gets a stricter per-IP budget).
  app.use(createRateLimitMiddleware());

  // GET /health - Public endpoint checking database connection
  app.get("/health", async (_req: Request, res: Response) => {
    try {
      const health = await database.checkHealth();
      if (health.healthy) {
        res.status(200).json({ status: "healthy", database: "connected" });
      } else {
        res.status(503).json({ status: "unhealthy", database: "disconnected", error: health.details });
      }
    } catch (error: any) {
      res.status(503).json({ status: "unhealthy", database: "disconnected", error: error.message || String(error) });
    }
  });

  // Issue #839: public currency conversion preview (cached oracle rate, no auth).
  // Rate-limited to 60 req/min per IP. Must stay before the API-key gate.
  app.get("/v1/fx/convert", fxConvertRateLimit, (req: Request, res: Response) => {
    const from = typeof req.query.from === "string" ? req.query.from.toUpperCase() : "";
    const to = typeof req.query.to === "string" ? req.query.to.toUpperCase() : "";
    const amountRaw = typeof req.query.amount === "string" ? req.query.amount : "";

    if (!from || !to || !amountRaw) {
      res.status(400).json({ error: "Query params from, to, and amount are required" });
      return;
    }

    let amountStroops: bigint;
    try {
      amountStroops = BigInt(amountRaw);
    } catch {
      res.status(400).json({ error: "amount must be an integer stroop count" });
      return;
    }
    if (amountStroops < 0n) {
      res.status(400).json({ error: "amount must be non-negative" });
      return;
    }

    const cached = getCachedRate(from, to);
    if (!cached) {
      res.status(404).json({ error: `No cached rate for ${from}/${to}` });
      return;
    }

    const nowSecs = Math.floor(Date.now() / 1000);
    const rateAgeSecs = Math.max(0, nowSecs - cached.updatedAt);
    const stale = rateAgeSecs > DEFAULT_FX_STALENESS_SECS;

    const amountUsdc = Number(amountStroops) / 10 ** USDC_DECIMALS;
    const amountFiat = amountUsdc * cached.rate;

    res.status(200).json({
      from,
      to,
      amount_usdc: formatFixed(amountUsdc, 2),
      amount_fiat: formatFixed(amountFiat, 2),
      rate: formatFixed(cached.rate, 2),
      rate_age_secs: rateAgeSecs,
      stale,
  // Issue #817: auth endpoints (stricter 10 req/min per-IP via rate limiter above)
  app.post("/v1/auth/login", (req: Request, res: Response) => {
    const apiKey =
      (typeof req.body?.api_key === "string" && req.body.api_key) ||
      (typeof req.headers["x-api-key"] === "string" && req.headers["x-api-key"]);
    if (!apiKey) {
      res.status(400).json({ error: "api_key is required" });
      return;
    }
    res.status(200).json({
      token: apiKey,
      token_type: "api_key",
      expires_in: 3600,
    });
  });

  app.post("/v1/auth/refresh", (req: Request, res: Response) => {
    const refreshToken =
      (typeof req.body?.refresh_token === "string" && req.body.refresh_token) ||
      (typeof req.body?.token === "string" && req.body.token);
    if (!refreshToken) {
      res.status(400).json({ error: "refresh_token is required" });
      return;
    }
    res.status(200).json({
      token: refreshToken,
      token_type: "api_key",
      expires_in: 3600,
    });
  });

  // Issue #855: Real-time event streaming via Server-Sent Events (SSE)
  const sep10Config = loadSEP10AuthConfig();
  const sseHandler = async (req: Request, res: Response): Promise<void> => {
    const merchantId = (req.query.merchant_id as string) || req.auth?.sub;
    if (!merchantId) {
      res.status(400).json({ error: "Missing required query parameter: merchant_id" });
      return;
    }

    if (req.auth && req.auth.sub !== merchantId && !sep10Config.adminAccounts.has(req.auth.sub)) {
      res.status(403).json({ error: "Token is not authorized for this merchant" });
      return;
    }

    if (!sseManager.canConnect(merchantId)) {
      res.status(429).json({
        error: "Rate limit exceeded: maximum 5 concurrent SSE connections per merchant",
        limit: 5,
      });
      return;
    }

    res.writeHead(200, {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache",
      "Connection": "keep-alive",
    });
    res.flushHeaders?.();

    const eventTypes =
      typeof req.query.event_types === "string"
        ? req.query.event_types.split(",").map((s) => s.trim())
        : undefined;

    const clientId = `${merchantId}-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`;
    sseManager.registerClient(clientId, merchantId, res, eventTypes);

    req.on("close", () => {
      sseManager.removeClient(clientId);
    });
  };

  app.get("/v1/events/stream", requireSEP10Auth(sep10Config), sseHandler);
  app.get("/events/stream", requireSEP10Auth(sep10Config), sseHandler);

  // GET /webhooks/public-key (Issue #775): Unauthenticated public key endpoint for webhook verification
  const publicKeyHandler = (_req: Request, res: Response) => {
    const key = getPlatformEd25519PublicKey();
    res.status(200).json({
      algorithm: "ed25519",
      public_key: key,
      publicKey: key,
    });
  };
  app.get("/webhooks/public-key", publicKeyHandler);
  app.get("/v1/webhooks/public-key", publicKeyHandler);

  // All subsequent routes require API-key authentication
  app.use(requireApiKey);

  // Webhook test delivery and delivery history (Issues #808, #810, #854).
  app.use("/webhooks", requireScope("manage:webhooks"));
  registerWebhookRoutes(app, {
    store: new WebhookStore(database.getPool()),
    merchantIdFromRequest: (req) =>
      typeof req.header("x-merchant-id") === "string"
        ? (req.header("x-merchant-id") as string)
        : null,
  });

  // Issue #785: GET /v1/payments
  app.get("/v1/payments", requireScope("read:payments"), async (req: Request, res: Response, next: NextFunction) => {
    try {
      const parsed = paymentsQuerySchema.safeParse(req.query);
      if (!parsed.success) {
        res.status(400).json({ error: "Invalid query parameters", details: parsed.error.flatten() });
        return;
      }

      const { merchant_id, status, from, to, limit, cursor } = parsed.data;

      const subject = req.auth?.sub;
      if (!subject) {
        res.status(401).json({ error: "Missing authenticated subject" });
        return;
      }
      if (subject !== merchant_id && !sep10Config.adminAccounts.has(subject)) {
        res.status(403).json({ error: "Token is not authorized for this merchant" });
        return;
      }

      let decodedCursor: { createdAt: string; id: string } | null = null;
      if (cursor) {
        decodedCursor = decodePaymentsCursor(cursor);
        if (!decodedCursor) {
          res.status(400).json({ error: "Invalid cursor" });
          return;
        }
      }

      const result = await database.getPaymentsFiltered({
        merchantId: merchant_id,
        statuses: status,
        from,
        to,
        limit,
        cursor: decodedCursor ?? undefined,
      });

      const nextCursor =
        result.payments.length === limit && result.payments.length > 0
          ? encodePaymentsCursor(
              String(result.payments[result.payments.length - 1].created_at),
              String(result.payments[result.payments.length - 1].id),
            )
          : null;

      res.status(200).json({
        data: result.payments,
        next_cursor: nextCursor,
        total: result.total,
      });
    } catch (error) {
      next(error);
    }
  });

  // Issue #840: GET /v1/reports/reconciliation?from=&to=
  // Merchant-authenticated CSV download of payments, refunds, and settlements.
  // Large reports are streamed via a PostgreSQL cursor (not buffered in memory).
  const reconciliationQuerySchema = z.object({
    from: isoDateString,
    to: isoDateString,
  });

  const reconciliationHandler = async (req: Request, res: Response, next: NextFunction) => {
    try {
      const parsed = reconciliationQuerySchema.safeParse(req.query);
      if (!parsed.success) {
        res.status(400).json({ error: "Invalid query parameters", details: parsed.error.flatten() });
        return;
      }

      const subject = req.auth?.sub;
      if (!subject) {
        res.status(401).json({ error: "Missing authenticated subject" });
        return;
      }

      const merchantId = subject;
      const from = new Date(parsed.data.from);
      const to = new Date(parsed.data.to);
      if (from > to) {
        res.status(400).json({ error: "`from` must be on or before `to`" });
        return;
      }

      const monthLabel = `${from.getUTCFullYear()}-${String(from.getUTCMonth() + 1).padStart(2, "0")}`;
      const filename = `reconciliation_${monthLabel}.csv`;

      res.status(200);
      res.setHeader("Content-Type", "text/csv");
      res.setHeader("Content-Disposition", `attachment; filename=${filename}`);

      const header =
        "payment_id,type,amount_usdc,fiat_amount,fiat_currency,status,created_at,settled_at,customer_ref,tags\n";
      res.write(header);

      const escapeCsv = (value: string): string => {
        if (value == null) return "";
        const s = String(value);
        if (/[",\n\r]/.test(s)) return `"${s.replace(/"/g, '""')}"`;
        return s;
      };

      for await (const row of database.streamReconciliationReport(merchantId, from, to)) {
        const line = [
          row.payment_id,
          row.type,
          row.amount_usdc,
          row.fiat_amount,
          row.fiat_currency,
          row.status,
          row.created_at,
          row.settled_at,
          row.customer_ref,
          row.tags,
        ]
          .map(escapeCsv)
          .join(",");
        if (!res.write(line + "\n")) {
          await new Promise<void>((resolve) => res.once("drain", resolve));
        }
      }

      res.end();
    } catch (error) {
      next(error);
    }
  };

  // SEP-10 JWT authenticated (merchant subject = merchant_id).
  app.get(
    "/v1/reports/reconciliation",
    requireSEP10Auth(sep10Config),
    reconciliationHandler,
  );

  // GET /payments/:paymentId
  app.get("/payments/:paymentId", requireScope("read:payments"), async (req: Request, res: Response, next: NextFunction) => {
    try {
      const { paymentId } = req.params;
      const payment = await database.getPaymentById(paymentId);
      if (!payment) {
        res.status(404).json({ error: "Payment not found" });
        return;
      }
      res.status(200).json(payment);
    } catch (error) {
      next(error);
    }
  });

  app.get("/merchants/:merchantId/payments", requireScope("read:payments"), async (req: Request, res: Response, next: NextFunction) => {
    try {
      const { merchantId } = req.params;
      const page = req.query.page ? parseInt(req.query.page as string, 10) : 1;
      const limit = req.query.limit ? parseInt(req.query.limit as string, 10) : 20;
      const status = req.query.status ? (req.query.status as string) : undefined;

      if (isNaN(page) || page < 1 || isNaN(limit) || limit < 1) {
        res.status(400).json({ error: "Invalid pagination parameters" });
        return;
      }

      const result = await database.getPaymentsByMerchantPaginated(merchantId, page, limit, status);
      res.status(200).json(result);
    } catch (error) {
      next(error);
    }
  });

  app.get("/merchants/:merchantId/disputes", requireScope("read:payments"), async (req: Request, res: Response, next: NextFunction) => {
    try {
      const { merchantId } = req.params;
      const status = req.query.status ? (req.query.status as string) : undefined;
      const disputes = await database.getDisputesByMerchant(merchantId, status);
      res.status(200).json({ disputes });
    } catch (error) {
      next(error);
    }
  });

  app.get("/refunds/:refundId", requireScope("read:payments"), async (req: Request, res: Response, next: NextFunction) => {
    try {
      const { refundId } = req.params;
      const refund = await database.getRefundById(refundId);
      if (!refund) {
        res.status(404).json({ error: "Refund not found" });
        return;
      }
      res.status(200).json(refund);
    } catch (error) {
      next(error);
    }
  });

  app.get("/events", requireScope("read:analytics"), async (req: Request, res: Response, next: NextFunction) => {
    try {
      const type = req.query.type ? (req.query.type as string) : undefined;
      const fromLedger = req.query.from ? parseInt(req.query.from as string, 10) : undefined;
      const toLedger = req.query.to ? parseInt(req.query.to as string, 10) : undefined;

      if (fromLedger !== undefined && isNaN(fromLedger)) {
        res.status(400).json({ error: "Invalid 'from' ledger parameter" });
        return;
      }
      if (toLedger !== undefined && isNaN(toLedger)) {
        res.status(400).json({ error: "Invalid 'to' ledger parameter" });
        return;
      }

      const events = await database.getEventsFiltered(type, fromLedger, toLedger);
      res.status(200).json({ events });
    } catch (error) {
      next(error);
    }
  });

  app.post("/admin/replay-dlq", requireScope("admin"), async (_req: Request, res: Response, next: NextFunction) => {
    try {
      if (!replayDlqHandler) {
        res.status(501).json({ error: "DLQ replay handler not configured on server" });
        return;
      }
      const result = await replayDlqHandler();
      res.status(200).json(result);
    } catch (error) {
      next(error);
    }
  });

  app.post("/admin/replay", requireAdminApiKey, async (req: Request, res: Response, next: NextFunction) => {
    try {
      const fromParam = req.query.from_ledger ?? req.query.from ?? req.body?.from_ledger ?? req.body?.from;
      const toParam = req.query.to_ledger ?? req.query.to ?? req.body?.to_ledger ?? req.body?.to;

      const fromLedger = parseInt(fromParam as string, 10);
      const toLedger = parseInt(toParam as string, 10);

      if (isNaN(fromLedger) || isNaN(toLedger) || fromLedger < 1 || toLedger < fromLedger) {
        res.status(400).json({
          error: "Invalid ledger parameters: 'from_ledger' and 'to_ledger' must be positive integers with from_ledger <= to_ledger",
        });
        return;
      }

      if (toLedger - fromLedger > MAX_REPLAY_LEDGER_RANGE) {
        res.status(400).json({
          error: `Requested ledger range (${toLedger - fromLedger + 1}) exceeds maximum allowed limit of ${MAX_REPLAY_LEDGER_RANGE} ledgers`,
        });
        return;
      }

      if (!eventReplayHandler) {
        res.status(501).json({ error: "Event replay handler not configured on server" });
        return;
      }

      res.setHeader("Content-Type", "text/event-stream");
      res.setHeader("Cache-Control", "no-cache");
      res.setHeader("Connection", "keep-alive");
      res.flushHeaders?.();

      let isClientConnected = true;
      req.on("close", () => {
        isClientConnected = false;
      });

      const onProgress = (progress: ReplayProgressUpdate) => {
        if (!isClientConnected) return;
        res.write(`data: ${JSON.stringify({ processed: progress.processed, total: progress.total, stored: progress.stored })}\n\n`);
      };

      const result = await eventReplayHandler(fromLedger, toLedger, onProgress);
      if (isClientConnected) {
        res.write(`data: ${JSON.stringify({ type: "complete", processed: result.processed, total: result.total, stored: result.stored })}\n\n`);
        res.end();
      }
    } catch (error: any) {
      if (res.headersSent) {
        res.write(`data: ${JSON.stringify({ type: "error", error: error.message || String(error) })}\n\n`);
        res.end();
      } else {
        next(error);
      }
    }
  });

  app.use((err: any, _req: Request, res: Response, _next: NextFunction) => {
    console.error("API Request Error:", err);
    res.status(500).json({ error: "Internal Server Error" });
  });

  return app;
}

export async function startServer(
  database: Database,
  port = parseInt(process.env.PORT || process.env.INDEXER_API_PORT || "3001", 10),
  replayDlqHandler?: ReplayDLQHandler,
  eventReplayHandler?: EventReplayHandler,
) {
  const app = createServer(database, replayDlqHandler, eventReplayHandler);
  const server = app.listen(port, () => {
    console.log(`Indexer REST API listening on port ${port}`);
  });
  return server;
}

async function main(): Promise<void> {
  const dbConnectionString =
    process.env.DATABASE_URL ||
    "postgres://postgres:password@localhost:5432/fluxapay";
  const port = parseInt(process.env.PORT || process.env.INDEXER_API_PORT || "3001", 10);

  const database = new Database(dbConnectionString);
  await database.initialize();

  await startServer(database, port);
}

if (require.main === module) {
  main().catch((error) => {
    console.error("Fatal error starting indexer API:", error);
    process.exit(1);
  });
}
