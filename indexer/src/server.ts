/**
 * Issue #672: Indexer REST API with SEP-10 authentication.
 *
 * Exposes read endpoints backed by the indexed Postgres tables (see
 * `database.ts`). All endpoints except `/health` require a SEP-10 JWT
 * (see `auth/sep10-jwt.ts`) issued via the SEP-10 challenge/response flow
 * against `STELLAR_WEB_AUTH_ENDPOINT`:
 *
 *   Authorization: Bearer <jwt>
 *
 * - `GET /merchants/:merchantId/*` endpoints only return data for the
 *   merchant the token was issued to (`sub` claim), or for admin accounts.
 * - `GET /admin/*` endpoints require an admin account (`INDEXER_ADMIN_ACCOUNTS`).
 *
 * Run standalone with `npm run dev:api` / `npm run start:api` (see
 * package.json); this is separate from the event-subscriber process
 * (`index.ts`) so the two can be scaled/deployed independently.
 */
import express, { type Request, type Response } from "express";
import * as dotenv from "dotenv";
import { Database } from "./database";
import { loadSEP10AuthConfig } from "./auth/config";
import { requireAdmin, requireMerchantScope, requireSEP10Auth } from "./auth/middleware";

dotenv.config();

export function createServer(database: Database) {
  const app = express();
  const authConfig = loadSEP10AuthConfig();
  const auth = requireSEP10Auth(authConfig);

  app.get("/health", (_req: Request, res: Response) => {
    res.json({ status: "ok" });
  });

  app.get(
    "/merchants/:merchantId/payments",
    auth,
    requireMerchantScope("merchantId", authConfig),
    async (req: Request, res: Response) => {
      const rows = await database.getPaymentsByMerchant(req.params.merchantId);
      res.json({ payments: rows });
    },
  );

  app.get(
    "/merchants/:merchantId/refunds",
    auth,
    requireMerchantScope("merchantId", authConfig),
    async (req: Request, res: Response) => {
      const rows = await database.getRefundsByMerchant(req.params.merchantId);
      res.json({ refunds: rows });
    },
  );

  app.get(
    "/merchants/:merchantId/disputes",
    auth,
    requireMerchantScope("merchantId", authConfig),
    async (req: Request, res: Response) => {
      const rows = await database.getDisputesByMerchant(req.params.merchantId);
      res.json({ disputes: rows });
    },
  );

  app.get(
    "/admin/stats",
    auth,
    requireAdmin(authConfig),
    async (_req: Request, res: Response) => {
      const stats = await database.getAdminStats();
      res.json({ stats });
    },
  );

  return app;
}

async function main(): Promise<void> {
  const dbConnectionString =
    process.env.DATABASE_URL ||
    "postgres://postgres:password@localhost:5432/fluxapay";
  const port = parseInt(process.env.INDEXER_API_PORT || "3001");

  const database = new Database(dbConnectionString);
  await database.initialize();

  const app = createServer(database);
  app.listen(port, () => {
    console.log(`Indexer REST API listening on port ${port}`);
  });
}

if (require.main === module) {
  main().catch((error) => {
    console.error("Fatal error starting indexer API:", error);
    process.exit(1);
  });
}
