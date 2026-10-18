/**
 * Webhook Dead-Letter Queue persistence (Issue #961).
 *
 * Separated from WebhookStore so the retry daemon has a focused API without
 * pulling in endpoint or delivery-log concerns.
 */

import type { Pool } from "pg";
import type {
  EnqueueDLQInput,
  WebhookDLQItem,
  WebhookDLQStatus,
  WebhookEnvelope,
} from "./types";

/**
 * Retry schedule (Issue #961): delays in milliseconds for each attempt index.
 *
 * attempt 0 → enqueue with next_retry_at = NULL (retry immediately on first pick-up)
 * attempt 1 → 1 minute
 * attempt 2 → 5 minutes
 * attempt 3 → 30 minutes
 * attempt 4 → 2 hours
 * attempt 5 → 8 hours
 * attempt 6 → 24 hours
 * attempt 7+ → permanently_failed
 *
 * The schedule is exported so tests can assert against the exact values.
 */
export const RETRY_DELAYS_MS: readonly number[] = [
  0,               // attempt 0 — immediate
  1 * 60_000,      // attempt 1 — 1 min
  5 * 60_000,      // attempt 2 — 5 min
  30 * 60_000,     // attempt 3 — 30 min
  2 * 60 * 60_000, // attempt 4 — 2 hr
  8 * 60 * 60_000, // attempt 5 — 8 hr
  24 * 60 * 60_000,// attempt 6 — 24 hr
];

/** Maximum number of delivery attempts before a DLQ item is permanently failed. */
export const MAX_RETRY_ATTEMPTS = 7;

/**
 * Compute the delay (ms) before attempt number `attempt`.
 *
 * The issue specifies: delay = BASE_DELAY * 2^attempt, but also lists the
 * explicit schedule above. We use the explicit schedule when the attempt index
 * is in range and fall back to the formula for any out-of-range value so the
 * function never throws.
 */
export function retryDelayMs(attempt: number): number {
  if (attempt >= 0 && attempt < RETRY_DELAYS_MS.length) {
    return RETRY_DELAYS_MS[attempt];
  }
  // Formula fallback: BASE_DELAY (1 min) * 2^attempt, capped at 24 hr.
  const BASE_DELAY = 60_000;
  return Math.min(BASE_DELAY * Math.pow(2, attempt), 24 * 60 * 60_000);
}

/** Next retry timestamp given the current attempt count and optional base time. */
export function nextRetryAt(currentRetryCount: number, now = Date.now()): Date | null {
  const delay = retryDelayMs(currentRetryCount);
  if (delay === 0) return null; // immediate
  return new Date(now + delay);
}

export class WebhookDLQStore {
  constructor(private readonly pool: Pool) {}

  /**
   * Enqueue a failed webhook delivery for retry.
   *
   * Called by the delivery layer after a deliverOnce failure. The first call
   * for a given endpoint+envelope creates the DLQ row; the daemon then handles
   * subsequent attempts via `recordAttemptResult`.
   */
  async enqueue(input: EnqueueDLQInput): Promise<string> {
    const retryAt = nextRetryAt(0);
    const { rows } = await this.pool.query<{ id: string }>(
      `INSERT INTO webhook_dlq
         (endpoint_id, merchant_id, event_type, payment_id, payload,
          retry_count, next_retry_at, status, last_error)
       VALUES ($1, $2, $3, $4, $5, 0, $6, 'pending', $7)
       RETURNING id`,
      [
        input.endpointId,
        input.merchantId,
        input.eventType,
        input.paymentId ?? null,
        JSON.stringify(input.payload),
        retryAt,
        input.error,
      ],
    );
    return rows[0].id;
  }

  /**
   * Return all DLQ items that are due for retry right now.
   *
   * "Due" means: status = 'pending' AND (next_retry_at IS NULL OR next_retry_at <= NOW()).
   * Ordered oldest-first so longer-waiting items are processed first.
   */
  async getDueDLQItems(limit = 100): Promise<WebhookDLQItem[]> {
    const { rows } = await this.pool.query<WebhookDLQItem>(
      `SELECT id, endpoint_id, merchant_id, event_type, payment_id,
              payload, retry_count, next_retry_at, status, last_error,
              created_at, updated_at
         FROM webhook_dlq
        WHERE status = 'pending'
          AND (next_retry_at IS NULL OR next_retry_at <= NOW())
        ORDER BY created_at ASC
        LIMIT $1`,
      [limit],
    );
    return rows;
  }

  /**
   * Record the result of a retry attempt.
   *
   * - On success: deletes the DLQ row (delivery confirmed).
   * - On failure with retries remaining: advances retry_count and schedules
   *   the next next_retry_at.
   * - On failure with no retries left: transitions to permanently_failed.
   *
   * Returns the new status after the update, or null if the row was deleted
   * (success case).
   */
  async recordAttemptResult(
    id: string,
    succeeded: boolean,
    error: string | null,
  ): Promise<WebhookDLQStatus | null> {
    if (succeeded) {
      await this.pool.query(`DELETE FROM webhook_dlq WHERE id = $1`, [id]);
      return null;
    }

    // Fetch current count to decide the next schedule.
    const { rows } = await this.pool.query<{ retry_count: number }>(
      `SELECT retry_count FROM webhook_dlq WHERE id = $1`,
      [id],
    );
    if (rows.length === 0) return null;

    const newCount = rows[0].retry_count + 1;

    if (newCount >= MAX_RETRY_ATTEMPTS) {
      await this.pool.query(
        `UPDATE webhook_dlq
            SET retry_count = $1,
                status      = 'permanently_failed',
                last_error  = $2,
                updated_at  = NOW()
          WHERE id = $3`,
        [newCount, error, id],
      );
      return "permanently_failed";
    }

    const retryAt = nextRetryAt(newCount);
    await this.pool.query(
      `UPDATE webhook_dlq
          SET retry_count   = $1,
              next_retry_at = $2,
              last_error    = $3,
              updated_at    = NOW()
        WHERE id = $4`,
      [newCount, retryAt, error, id],
    );
    return "pending";
  }

  /**
   * List DLQ entries for a merchant.
   *
   * Used by the merchant dashboard to display retry history. Newest first.
   */
  async listForMerchant(
    merchantId: string,
    options: {
      status?: WebhookDLQStatus;
      limit?: number;
      before?: string; // ISO timestamp for cursor pagination
    } = {},
  ): Promise<WebhookDLQItem[]> {
    const limit = Math.min(options.limit ?? 20, 100);
    const params: unknown[] = [merchantId];
    let sql = `
      SELECT id, endpoint_id, merchant_id, event_type, payment_id,
             payload, retry_count, next_retry_at, status, last_error,
             created_at, updated_at
        FROM webhook_dlq
       WHERE merchant_id = $1`;

    if (options.status) {
      params.push(options.status);
      sql += ` AND status = $${params.length}`;
    }
    if (options.before) {
      params.push(options.before);
      sql += ` AND created_at < $${params.length}`;
    }

    params.push(limit);
    sql += ` ORDER BY created_at DESC LIMIT $${params.length}`;

    const { rows } = await this.pool.query<WebhookDLQItem>(sql, params);
    return rows;
  }

  /**
   * Get a single DLQ item by ID.
   *
   * Returns null when the item doesn't exist or has been delivered and
   * removed (success case deletes the row).
   */
  async getById(id: string): Promise<WebhookDLQItem | null> {
    const { rows } = await this.pool.query<WebhookDLQItem>(
      `SELECT id, endpoint_id, merchant_id, event_type, payment_id,
              payload, retry_count, next_retry_at, status, last_error,
              created_at, updated_at
         FROM webhook_dlq
        WHERE id = $1`,
      [id],
    );
    return rows[0] ?? null;
  }

  /**
   * Manually reset a permanently_failed item back to pending so it will be
   * retried on the next daemon tick.
   *
   * Called by POST /v1/webhooks/dlq/replay to let merchants force-retry a
   * failed delivery without waiting for the scheduled window.
   */
  async requeueItem(id: string): Promise<boolean> {
    const { rowCount } = await this.pool.query(
      `UPDATE webhook_dlq
          SET status        = 'pending',
              next_retry_at = NULL,
              updated_at    = NOW()
        WHERE id = $1`,
      [id],
    );
    return (rowCount ?? 0) > 0;
  }

  /**
   * Manually replay ALL pending and permanently_failed items for a merchant.
   *
   * Resets next_retry_at to NULL so the daemon picks them up on its very next
   * tick. Useful for bulk recovery after an outage on the merchant's side.
   */
  async requeueAllForMerchant(merchantId: string): Promise<number> {
    const { rowCount } = await this.pool.query(
      `UPDATE webhook_dlq
          SET status        = 'pending',
              next_retry_at = NULL,
              updated_at    = NOW()
        WHERE merchant_id = $1`,
      [merchantId],
    );
    return rowCount ?? 0;
  }
}
