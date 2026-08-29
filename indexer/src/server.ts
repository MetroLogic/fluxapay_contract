/**
 * FluxaPay Indexer REST API Server
 * Exposes read-only endpoints for persisted payments, disputes, refunds, and events,
 * as well as health check and manual DLQ replay endpoints.
 */

import express, { type Request, type Response, type NextFunction } from "express";
import * as dotenv from "dotenv";
import { Database } from "./database";
import { requireApiKey } from "./auth/api-key";

dotenv.config();

export type ReplayDLQHandler = () => Promise<{ attempted: number; succeeded: number; failed: number }>;

export function createServer(database: Database, replayDlqHandler?: ReplayDLQHandler) {
  const app = express();
  app.use(express.json());

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

  // All subsequent routes require API-key authentication
  app.use(requireApiKey);

  // GET /payments/:paymentId
  app.get("/payments/:paymentId", async (req: Request, res: Response, next: NextFunction) => {
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

  // GET /merchants/:merchantId/payments?page=1&limit=20&status=Confirmed
  app.get("/merchants/:merchantId/payments", async (req: Request, res: Response, next: NextFunction) => {
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

  // GET /merchants/:merchantId/disputes?status=Open
  app.get("/merchants/:merchantId/disputes", async (req: Request, res: Response, next: NextFunction) => {
    try {
      const { merchantId } = req.params;
      const status = req.query.status ? (req.query.status as string) : undefined;

      const disputes = await database.getDisputesByMerchant(merchantId, status);
      res.status(200).json({ disputes });
    } catch (error) {
      next(error);
    }
  });

  // GET /refunds/:refundId
  app.get("/refunds/:refundId", async (req: Request, res: Response, next: NextFunction) => {
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

  // GET /events?type=PAYMENT/CONFIRMED&from=<ledger>&to=<ledger>
  app.get("/events", async (req: Request, res: Response, next: NextFunction) => {
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

  // POST /admin/replay-dlq - Trigger manual replay of dead-letter queue events
  app.post("/admin/replay-dlq", async (_req: Request, res: Response, next: NextFunction) => {
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

  // Global Error Handler
  app.use((err: any, _req: Request, res: Response, _next: NextFunction) => {
    console.error("API Request Error:", err);
    res.status(500).json({ error: "Internal Server Error" });
  });

  return app;
}

export async function startServer(
  database: Database,
  port = parseInt(process.env.PORT || process.env.INDEXER_API_PORT || "3001", 10),
  replayDlqHandler?: ReplayDLQHandler
) {
  const app = createServer(database, replayDlqHandler);
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
