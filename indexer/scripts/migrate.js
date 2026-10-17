#!/usr/bin/env node
'use strict';

/**
 * Migration runner for indexer/migrations.
 *
 * Usage:
 *   node indexer/scripts/migrate.js up          # apply all pending migrations
 *   node indexer/scripts/migrate.js down        # roll back the most recent migration
 *   node indexer/scripts/migrate.js down:all    # roll back all applied migrations
 *
 * Each migration is a `.sql` file in indexer/migrations. A migration's rollback
 * is either an inline `-- down` section in the same file, or a sibling
 * `<name>.down.sql` file. Migrations without a rollback cause the runner to
 * exit non-zero with a clear error so CI fails loudly.
 */

const fs = require('fs');
const path = require('path');
const { Client } = require('pg');

const MIGRATIONS_DIR = path.resolve(__dirname, '..', 'migrations');
const MIGRATIONS_TABLE = 'schema_migrations';
const DOWN_MARKER = /^\s*--\s*down\b.*$/im;

function listMigrationFiles() {
  if (!fs.existsSync(MIGRATIONS_DIR)) {
    throw new Error(`Migrations directory not found: ${MIGRATIONS_DIR}`);
  }
  return fs
    .readdirSync(MIGRATIONS_DIR)
    .filter((f) => f.endsWith('.sql') && !f.endsWith('.down.sql'))
    .sort();
}

function splitUpDown(fileName) {
  const fullPath = path.join(MIGRATIONS_DIR, fileName);
  const sql = fs.readFileSync(fullPath, 'utf8');
  const match = sql.match(DOWN_MARKER);

  let up = sql;
  let down = null;

  if (match) {
    const idx = match.index;
    up = sql.slice(0, idx);
    down = sql.slice(idx + match[0].length);
  }

  const downFile = path.join(MIGRATIONS_DIR, fileName.replace(/\.sql$/, '.down.sql'));
  if (fs.existsSync(downFile)) {
    down = fs.readFileSync(downFile, 'utf8');
  }

  return { up: up.trim(), down: down ? down.trim() : null };
}

function assertHasDown(fileName, down) {
  if (!down) {
    throw new Error(
      `Migration "${fileName}" has no rollback (down) section. ` +
        'Add an inline "-- down" block or a sibling "*.down.sql" file.'
    );
  }
}

async function ensureMigrationsTable(client) {
  await client.query(
    `CREATE TABLE IF NOT EXISTS ${MIGRATIONS_TABLE} (
       name text PRIMARY KEY,
       applied_at timestamptz NOT NULL DEFAULT now()
     )`
  );
}

async function appliedMigrations(client) {
  const res = await client.query(`SELECT name FROM ${MIGRATIONS_TABLE} ORDER BY name ASC`);
  return res.rows.map((r) => r.name);
}

async function up(client) {
  const files = listMigrationFiles();
  const applied = new Set(await appliedMigrations(client));
  const pending = files.filter((f) => !applied.has(f));

  if (pending.length === 0) {
    console.log('No pending migrations.');
    return;
  }

  for (const file of pending) {
    const { up: upSql } = splitUpDown(file);
    console.log(`Applying ${file}...`);
    await client.query('BEGIN');
    try {
      await client.query(upSql);
      await client.query(`INSERT INTO ${MIGRATIONS_TABLE} (name) VALUES ($1)`, [file]);
      await client.query('COMMIT');
    } catch (err) {
      await client.query('ROLLBACK');
      throw new Error(`Failed to apply migration "${file}": ${err.message}`);
    }
  }
}

async function downOne(client) {
  const applied = await appliedMigrations(client);
  if (applied.length === 0) {
    console.log('No applied migrations to roll back.');
    return false;
  }

  const file = applied[applied.length - 1];
  const { down: downSql } = splitUpDown(file);
  assertHasDown(file, downSql);

  console.log(`Rolling back ${file}...`);
  await client.query('BEGIN');
  try {
    await client.query(downSql);
    await client.query(`DELETE FROM ${MIGRATIONS_TABLE} WHERE name = $1`, [file]);
    await client.query('COMMIT');
  } catch (err) {
    await client.query('ROLLBACK');
    throw new Error(`Failed to roll back migration "${file}": ${err.message}`);
  }
  return true;
}

async function downAll(client) {
  let rolledBack = 0;
  while (await downOne(client)) {
    rolledBack += 1;
  }
  console.log(`Rolled back ${rolledBack} migration(s).`);
}

async function main() {
  const command = process.argv[2];
  const valid = ['up', 'down', 'down:all'];
  if (!valid.includes(command)) {
    console.error(`Usage: migrate.js <${valid.join('|')}>`);
    process.exit(1);
  }

  const client = new Client({
    connectionString: process.env.DATABASE_URL,
  });

  await client.connect();
  try {
    await ensureMigrationsTable(client);
    if (command === 'up') {
      await up(client);
    } else if (command === 'down') {
      await downOne(client);
    } else {
      await downAll(client);
    }
  } finally {
    await client.end();
  }
}

main().catch((err) => {
  console.error(`Migration error: ${err.message}`);
  process.exit(1);
});
