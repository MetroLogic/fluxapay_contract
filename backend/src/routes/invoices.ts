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
 * POST   /v1/invoices/preview         – render invoice PDF without persisting
 *
 * Auth: the DELETE and preview endpoints require a `Authorization: Bearer <jwt>`
 * header issued by the SEP-10 flow. The JWT subject becomes the merchant_id and
 * is forwarded to the indexer.
 */

export interface InvoicesRouterConfig {
  /** Base URL of the indexer API, e.g. http://localhost:3001 */
  indexerUrl: string;
}

/** Maximum size (in bytes) of a preview PDF before it is rejected with 413. */
const MAX_PREVIEW_PDF_BYTES = 5 * 1024 * 1024;

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

  // ------------------------------------------------------------------ POST /v1/invoices/preview

  router.post("/preview", async (req: Request, res: Response, next: NextFunction) => {
    try {
      const authHeader = req.header("authorization") ?? "";
      const [scheme, token] = authHeader.split(" ");
      if (scheme !== "Bearer" || !token) {
        return res.status(401).json({ error: "merchant authentication required" });
      }

      // Delegate to the indexer's preview renderer, which shares the same PDF
      // generation code path as the persisted invoice. The indexer must not
      // create a draft invoice record for this request.
      const upstream = await fetch(`${indexerUrl}/v1/invoices/preview`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Accept: "application/pdf",
          Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify(req.body ?? {}),
      });

      if (!upstream.ok) {
        const body = await upstream.json().catch(() => ({ error: "preview generation failed" }));
        return res.status(upstream.status).json(body);
      }

      const pdf = Buffer.from(await upstream.arrayBuffer());
      if (pdf.byteLength > MAX_PREVIEW_PDF_BYTES) {
        return res.status(413).json({ error: "preview PDF exceeds 5 MB limit" });
      }

      res.setHeader("Content-Type", "application/pdf");
      res.setHeader("Content-Length", String(pdf.byteLength));
      res.setHeader("Content-Disposition", "inline; filename=\"invoice-preview.pdf\"");
      return res.status(200).send(pdf);
    } catch (err) {
      return next(err);
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
