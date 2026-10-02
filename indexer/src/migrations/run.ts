/**
 * FluxaPay Indexer Migration Runner
 * Applies SQL migrations to the database
 */

import { Pool } from "pg";
import * as fs from "fs";
import * as path from "path";
import * as dotenv from "dotenv";

dotenv.config();

async function runMigrations(): Promise<void> {
  const dbConnectionString =
    process.env.DATABASE_URL ||
    "postgres://postgres:password@localhost:5432/fluxapay";

  const pool = new Pool({ connectionString: dbConnectionString });
  const client = await pool.connect();

  try {
    console.log("Running migrations...");

    // Get all migration files
    const migrationsDir = path.join(__dirname, "../../migrations");
    const migrationFiles = fs
      .readdirSync(migrationsDir)
      .filter((f) => f.endsWith(".sql"))
      .sort();

    for (const file of migrationFiles) {
      const filePath = path.join(migrationsDir, file);
      const sql = fs.readFileSync(filePath, "utf-8");

      try {
        await client.query(sql);
        console.log(`✓ Applied migration: ${file}`);
      } catch (error) {
        console.error(`✗ Failed to apply migration ${file}:`, error);
        throw error;
      }
    }

    console.log("All migrations completed successfully");
  } finally {
    client.release();
    await pool.end();
  }
}

runMigrations().catch((error) => {
  console.error("Migration failed:", error);
  process.exit(1);
});
