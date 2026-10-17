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
