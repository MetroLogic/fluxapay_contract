import { Router, Request, Response, NextFunction } from "express";

/**
 * Issue #1067: Invoice lifecycle REST routes.
 *
 * These routes proxy to the indexer API so the backend stays the single
 * entry point for dashboard clients.
 *
 * GET    /v1/invoices                  – list with optional ?status= filter
 * GET    /v1/invoices/:id              – fetch single invoice
 * DELETE /v1/invoices/:id             – void (soft-delete) an invoice
 *
 * Issue #964: Transaction CSV export.
 *
 * GET    /v1/reports/transactions/export – streamed CSV export of the
 *        merchant's reconciliation report (see createTransactionsExportRouter).
 *
 * Auth: the DELETE endpoint requires a `Authorization: Bearer <jwt>` header
 * issued by the SEP-10 flow. The JWT subject becomes the merchant_id and is
 * forwarded to the indexer.
 */

export interface InvoicesRouterConfig {
  /** Base URL of the indexer API, e.g. http://localhost:3001 */
  indexerUrl: string;
}

export function createInvoicesRouter(config: InvoicesRouterConfig): Router {
  const router = Router();
  const { indexerUrl } = config;

  // ------------------------------------------------------------------ GET /v1/invoices

  router.get("/", async (req: Request, res: Response, next: NextFunction) => {
    try {
      const params = new URLSearchParams();
      if (typeof req.query.status === "string") params.set("status", req.query.status);
      if (typeof req.query.merchant_id === "string") params.set("merchant_id", req.query.merchant_id);
      if (typeof req.query.limit === "string") params.set("limit", req.query.limit);
      if (typeof req.query.offset === "string") params.set("offset", req.query.offset);

      const upstream = await fetch(`${indexerUrl}/v1/invoices?${params.toString()}`, {
        headers: { Accept: "application/json" },
      });
      const body = await upstream.json();
      res.status(upstream.status).json(body);
    } catch (err) {
      next(err);
    }
  });

  // ------------------------------------------------------------------ GET /v1/invoices/:id

  router.get("/:id", async (req: Request, res: Response, next: NextFunction) => {
    try {
      const upstream = await fetch(`${indexerUrl}/v1/invoices/${encodeURIComponent(req.params.id)}`, {
        headers: { Accept: "application/json" },
      });
      const body = await upstream.json();
      res.status(upstream.status).json(body);
    } catch (err) {
      next(err);
    }
  });

  // ------------------------------------------------------------------ DELETE /v1/invoices/:id

  router.delete("/:id", async (req: Request, res: Response, next: NextFunction) => {
    try {
      const authHeader = req.header("authorization") ?? "";
      const [scheme, token] = authHeader.split(" ");
      if (scheme !== "Bearer" || !token) {
        return res.status(401).json({ error: "merchant authentication required" });
      }

      const upstream = await fetch(
        `${indexerUrl}/v1/invoices/${encodeURIComponent(req.params.id)}`,
        {
          method: "DELETE",
          headers: {
            "Content-Type": "application/json",
            Authorization: `Bearer ${token}`,
          },
        },
      );
      const body = await upstream.json();
      return res.status(upstream.status).json(body);
    } catch (err) {
      return next(err);
    }
  });

  return router;
}

/**
 * Issue #964: CSV export of merchant transaction reports.
 *
 * GET /v1/reports/transactions/export?format=csv&from=...&to=...&status=Confirmed,Settled
 *
 * Streams the indexer's CSV response straight through to the client so large
 * datasets are never buffered in the backend. The merchant identity is taken
 * from the SEP-10 JWT subject and forwarded to the indexer, which enforces
 * that a merchant can only export their own data.
 */

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

const MAX_EXPORT_ROWS = 10_000;

function isIso8601(value: string): boolean {
  return !Number.isNaN(Date.parse(value));
}

function csvEscape(value: unknown): string {
  if (value === null || value === undefined) return "";
  const str = String(value);
  if (/[",\n\r]/.test(str)) {
    return `"${str.replace(/"/g, '""')}"`;
  }
  return str;
}

function toCsvRow(values: readonly unknown[]): string {
  return values.map(csvEscape).join(",") + "\n";
}

export function createTransactionsExportRouter(config: InvoicesRouterConfig): Router {
  const router = Router();
  const { indexerUrl } = config;

  router.get("/transactions/export", async (req: Request, res: Response, next: NextFunction) => {
    try {
      const authHeader = req.header("authorization") ?? "";
      const [scheme, token] = authHeader.split(" ");
      if (scheme !== "Bearer" || !token) {
        return res.status(401).json({ error: "merchant authentication required" });
      }

      const format = typeof req.query.format === "string" ? req.query.format : "csv";
      if (format !== "csv") {
        return res.status(400).json({ error: "unsupported format; only csv is supported" });
      }

      const from = typeof req.query.from === "string" ? req.query.from : undefined;
      const to = typeof req.query.to === "string" ? req.query.to : undefined;
      if (from && !isIso8601(from)) {
        return res.status(400).json({ error: "from must be an ISO-8601 date" });
      }
      if (to && !isIso8601(to)) {
        return res.status(400).json({ error: "to must be an ISO-8601 date" });
      }

      const params = new URLSearchParams();
      params.set("format", "csv");
      if (from) params.set("from", from);
      if (to) params.set("to", to);
      if (typeof req.query.status === "string" && req.query.status.length > 0) {
        params.set("status", req.query.status);
      }
      if (typeof req.query.merchant_id === "string") {
        params.set("merchant_id", req.query.merchant_id);
      }
      params.set("limit", String(MAX_EXPORT_ROWS + 1));

      const upstream = await fetch(
        `${indexerUrl}/v1/reports/transactions/export?${params.toString()}`,
        {
          headers: {
            Accept: "text/csv",
            Authorization: `Bearer ${token}`,
          },
        },
      );

      if (upstream.status === 413) {
        const body = await upstream.json().catch(() => ({}));
        return res.status(413).json({
          error: "export exceeds maximum of 10000 rows",
          hint: "narrow the date range or use pagination via /v1/reports/transactions",
          ...body,
        });
      }

      if (!upstream.ok || !upstream.body) {
        const body = await upstream.json().catch(() => ({ error: "export failed" }));
        return res.status(upstream.status).json(body);
      }

      const date = new Date().toISOString().slice(0, 10);
      res.setHeader("Content-Type", "text/csv; charset=utf-8");
      res.setHeader(
        "Content-Disposition",
        `attachment; filename="transactions-${date}.csv"`,
      );

      // Stream the upstream CSV body straight through without buffering.
      const reader = upstream.body.getReader();
      const decoder = new TextDecoder();
      let headerWritten = false;
      let rowCount = 0;

      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        let chunk = decoder.decode(value, { stream: true });

        if (!headerWritten) {
          // Ensure the canonical column header is present exactly once.
          const firstNewline = chunk.indexOf("\n");
          if (firstNewline === -1) {
            res.write(toCsvRow(CSV_COLUMNS));
            headerWritten = true;
            res.write(chunk);
            continue;
          }
          const firstLine = chunk.slice(0, firstNewline).trim();
          if (firstLine !== CSV_COLUMNS.join(",")) {
            res.write(toCsvRow(CSV_COLUMNS));
          }
          headerWritten = true;
          chunk = chunk.slice(firstNewline + 1);
        }

        rowCount += (chunk.match(/\n/g) ?? []).length;
        if (rowCount > MAX_EXPORT_ROWS) {
          reader.cancel().catch(() => undefined);
          res.end();
          return;
        }

        res.write(chunk);
      }

      res.end();
    } catch (err) {
      next(err);
    }
  });

  return router;
}
