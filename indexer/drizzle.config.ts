import { defineConfig } from "drizzle-kit";
import * as dotenv from "dotenv";

dotenv.config();

/**
 * Issue #842: Drizzle Kit config. SQL migration history in `migrations/` is
 * preserved and remains the source of truth applied by `npm run migrate`.
 * Use `drizzle:generate` only when iterating on `src/schema.ts` locally.
 */
export default defineConfig({
  schema: "./src/schema.ts",
  out: "./drizzle",
  dialect: "postgresql",
  dbCredentials: {
    url:
      process.env.DATABASE_URL ||
      "postgres://postgres:password@localhost:5432/fluxapay",
  },
});
