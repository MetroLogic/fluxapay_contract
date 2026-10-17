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

    // Get all migration files, sorted by filename so numeric prefixes
    // determine apply order deterministically.
    const migrationsDir = path.join(__dirname, "../../migrations");
    const migrationFiles = fs
      .readdirSync(migrationsDir)
      .filter((f) => f.endsWith(".sql"))
      .sort();

    // Guard against duplicate numeric prefixes (e.g. two files both named
    // `005_*.sql`), which previously caused non-deterministic migration
    // order: whichever file won the .sort() tie-break would run first,
    // silently applying its schema change ahead of the other. See #1022.
    const seenPrefixes = new Map<string, string>();
    for (const file of migrationFiles) {
      const prefixMatch = file.match(/^(\d+)_/);
      if (!prefixMatch) {
        throw new Error(
          `Migration file "${file}" does not start with a numeric prefix (NNN_name.sql)`,
        );
      }
      const prefix = prefixMatch[1];
      const existing = seenPrefixes.get(prefix);
      if (existing) {
        throw new Error(
          `Duplicate migration prefix "${prefix}": "${existing}" and "${file}" both claim it. ` +
            `Rename one to the next available sequential number.`,
        );
      }
      seenPrefixes.set(prefix, file);
    }

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
