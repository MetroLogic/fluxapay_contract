/**
 * Database-backed concurrency tests for webhook DLQ claims (Issue #1258).
 *
 * These tests need a real PostgreSQL database: `FOR UPDATE SKIP LOCKED` and
 * lease expiry are database semantics, so an in-memory fake cannot prove the
 * guarantee. Point `DATABASE_URL` at a disposable database (CI provides
 * `indexer_test`); the suite applies `migrations/*.sql` and truncates the
 * webhook tables, so do not run it against shared data. Without
 * `DATABASE_URL` the suite is skipped rather than silently passing.
 */

import { randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { Pool } from "pg";
import { startWebhookRetryDaemon } from "./retry";
import { WebhookStore } from "./store";
import type { FetchLike } from "./delivery";

const DATABASE_URL = process.env.DATABASE_URL;
const describeWithDatabase = DATABASE_URL ? describe : describe.skip;

const MIGRATIONS_DIR = join(__dirname, "..", "..", "migrations");

/**
 * Bring up only the tables this suite needs.
 *
 * 001_initial_schema.sql is not valid PostgreSQL (it uses MySQL-style inline
 * `INDEX` clauses inside CREATE TABLE), and the other migrations touch tables
 * irrelevant to webhook claims. Start from the 003 definition of the DLQ, then
 * apply the shipped webhook, retry-column and claim migrations for real.
 */
async function applyMigrations(pool: Pool): Promise<void> {
  await pool.query(`
    CREATE TABLE IF NOT EXISTS dead_letter_events (
      id SERIAL PRIMARY KEY,
      event_id VARCHAR(255) UNIQUE NOT NULL,
      raw_data JSONB NOT NULL,
      error TEXT NOT NULL,
      created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
      retry_count INTEGER DEFAULT 0
    )
  `);

  // Mirrors scripts/migrate.js: run the up section only, never the rollback.
  const DOWN_MARKER = /^\s*--\s*down\b.*$/im;
  for (const file of [
    "005_webhook_endpoints_and_delivery_log.sql",
    "013_webhook_retry_columns.sql",
    "015_webhook_dlq_claims.sql",
  ]) {
    const sql = readFileSync(join(MIGRATIONS_DIR, file), "utf8");
    await pool.query(sql.split(DOWN_MARKER)[0]);
  }
}

describeWithDatabase("webhook DLQ atomic claims (#1258)", () => {
  let setupPool: Pool;
  let workerAPool: Pool;
  let workerBPool: Pool;

  beforeAll(async () => {
    setupPool = new Pool({ connectionString: DATABASE_URL, max: 4 });
    workerAPool = new Pool({ connectionString: DATABASE_URL, max: 2 });
    workerBPool = new Pool({ connectionString: DATABASE_URL, max: 2 });
    await applyMigrations(setupPool);
  });

  afterAll(async () => {
    await Promise.all([setupPool.end(), workerAPool.end(), workerBPool.end()]);
  });

  beforeEach(async () => {
    await setupPool.query(
      "TRUNCATE webhook_delivery_log, dead_letter_events, webhook_endpoints RESTART IDENTITY CASCADE",
    );
  });

  async function insertEndpoint(pool: Pool = setupPool): Promise<string> {
    const endpointId = `whe_${randomUUID()}`;
    await pool.query(
      `INSERT INTO webhook_endpoints (id, merchant_id, url, signing_secret, event_types, enabled)
       VALUES ($1, 'merchant_1', 'https://merchant.example/hooks', 'whsec_test', '{}', TRUE)`,
      [endpointId],
    );
    return endpointId;
  }

  async function insertDueWebhookRow(endpointId: string): Promise<number> {
    const envelope = {
      id: `whd_${randomUUID()}`,
      event_id: `evt_${randomUUID()}`,
      type: "payment.confirmed",
      createdAt: new Date().toISOString(),
      livemode: true,
      data: { payment_id: "pay_1258" },
      endpoint_id: endpointId,
    };
    const { rows } = await setupPool.query(
      `INSERT INTO dead_letter_events (event_id, raw_data, error, retry_count, next_retry_at)
       VALUES ($1, $2::jsonb, 'connection refused', 0, NOW() - INTERVAL '1 second')
       RETURNING id`,
      [envelope.event_id, JSON.stringify(envelope)],
    );
    return rows[0].id;
  }

  function daemonOptions(fetchImpl: FetchLike, workerId: string) {
    return {
      fetch: fetchImpl,
      workerId,
      leaseMs: 30_000,
      logger: { info: () => {}, error: () => {} },
    };
  }

  it("delivers one due event exactly once when two workers sweep concurrently", async () => {
    const endpointId = await insertEndpoint();
    await insertDueWebhookRow(endpointId);

    const deliveredUrls: string[] = [];
    const fetchImpl: FetchLike = async (url) => {
      deliveredUrls.push(url);
      // Hold the attempt open so both sweeps overlap while the winner owns it.
      await new Promise((resolve) => setTimeout(resolve, 100));
      return { status: 200, text: async () => "ok" };
    };

    const workerA = startWebhookRetryDaemon(
      new WebhookStore(workerAPool),
      daemonOptions(fetchImpl, "worker-a"),
    );
    const workerB = startWebhookRetryDaemon(
      new WebhookStore(workerBPool),
      daemonOptions(fetchImpl, "worker-b"),
    );

    try {
      await Promise.all([workerA.runOnce(), workerB.runOnce()]);
    } finally {
      workerA.stop();
      workerB.stop();
    }

    expect(deliveredUrls).toEqual(["https://merchant.example/hooks"]);

    const { rows: dlqRows } = await setupPool.query(
      "SELECT id FROM dead_letter_events WHERE id IS NOT NULL",
    );
    expect(dlqRows).toHaveLength(0);

    const { rows: logRows } = await setupPool.query(
      "SELECT endpoint_id, attempt_number, success FROM webhook_delivery_log",
    );
    expect(logRows).toEqual([
      { endpoint_id: endpointId, attempt_number: 1, success: true },
    ]);
  });

  it("does not steal an active claim and recovers it once the lease expires", async () => {
    const endpointId = await insertEndpoint();
    const rowId = await insertDueWebhookRow(endpointId);

    const storeA = new WebhookStore(workerAPool);
    const storeB = new WebhookStore(workerBPool);

    const claimedByA = await storeA.claimDueWebhookRetries("worker-a", { leaseMs: 60_000 });
    expect(claimedByA).toHaveLength(1);
    expect(claimedByA[0].id).toBe(rowId);

    // Worker B cannot take a row whose lease is still live.
    expect(await storeB.claimDueWebhookRetries("worker-b", { leaseMs: 60_000 })).toHaveLength(0);

    // A stale token can never complete the row it no longer owns.
    expect(
      await storeA.markWebhookRetryDelivered(rowId, randomUUID()),
    ).toBe(false);
    expect(
      await storeA.scheduleWebhookRetry(rowId, randomUUID(), {
        retryCount: 1,
        error: "stale",
        delayMs: 60_000,
      }),
    ).toBe(false);

    // Simulate worker A exiting mid-attempt: its lease lapses.
    await setupPool.query(
      "UPDATE dead_letter_events SET claim_expires_at = NOW() - INTERVAL '1 second' WHERE id = $1",
      [rowId],
    );

    const claimedByB = await storeB.claimDueWebhookRetries("worker-b", { leaseMs: 60_000 });
    expect(claimedByB).toHaveLength(1);
    expect(claimedByB[0].claimToken).not.toBe(claimedByA[0].claimToken);

    // The recoverer owns completion; A's old token is still fenced out.
    expect(
      await storeA.markWebhookRetryDelivered(rowId, claimedByA[0].claimToken),
    ).toBe(false);
    expect(
      await storeB.markWebhookRetryDelivered(rowId, claimedByB[0].claimToken),
    ).toBe(true);
  });

  it("keeps success, retry scheduling and permanent failure consistent with the claim", async () => {
    const endpointId = await insertEndpoint();
    const rowId = await insertDueWebhookRow(endpointId);
    const store = new WebhookStore(workerAPool);

    const [claimed] = await store.claimDueWebhookRetries("worker-a", { leaseMs: 60_000 });
    expect(claimed.retryCount).toBe(0);

    // Failure schedules the next attempt and releases the claim.
    expect(
      await store.scheduleWebhookRetry(rowId, claimed.claimToken, {
        retryCount: 1,
        error: "HTTP 500",
        delayMs: 60_000,
      }),
    ).toBe(true);

    const { rows: scheduled } = await setupPool.query(
      `SELECT retry_count, error, status, claim_token, claim_expires_at,
              next_retry_at > NOW() AS not_due_yet
         FROM dead_letter_events WHERE id = $1`,
      [rowId],
    );
    expect(scheduled[0]).toMatchObject({
      retry_count: 1,
      error: "HTTP 500",
      status: "pending",
      claim_token: null,
      claim_expires_at: null,
      not_due_yet: true,
    });

    // Not claimable until the backoff elapses.
    expect(await store.claimDueWebhookRetries("worker-a", { leaseMs: 60_000 })).toHaveLength(0);

    await setupPool.query(
      "UPDATE dead_letter_events SET next_retry_at = NOW() - INTERVAL '1 second' WHERE id = $1",
      [rowId],
    );

    const [reclaimed] = await store.claimDueWebhookRetries("worker-a", { leaseMs: 60_000 });
    expect(reclaimed.retryCount).toBe(1);

    // A stale token cannot escalate the row to permanent failure.
    expect(
      await store.failWebhookRetryPermanently(rowId, randomUUID(), {
        retryCount: 7,
        error: "wrong worker",
      }),
    ).toBe(false);
    expect(
      await store.failWebhookRetryPermanently(rowId, reclaimed.claimToken, {
        retryCount: 7,
        error: "HTTP 500",
      }),
    ).toBe(true);

    const { rows: failed } = await setupPool.query(
      `SELECT status, retry_count, claim_token FROM dead_letter_events WHERE id = $1`,
      [rowId],
    );
    expect(failed[0]).toMatchObject({
      status: "permanently_failed",
      retry_count: 7,
      claim_token: null,
    });

    // Permanently failed rows are never claimed again.
    await setupPool.query(
      "UPDATE dead_letter_events SET next_retry_at = NOW() - INTERVAL '1 second' WHERE id = $1",
      [rowId],
    );
    expect(await store.claimDueWebhookRetries("worker-a", { leaseMs: 60_000 })).toHaveLength(0);
  });
});
