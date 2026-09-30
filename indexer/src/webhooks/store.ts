import type { Pool } from "pg";
import type { DeliveryAttempt, DeliveryLogRow, WebhookEndpoint } from "./types";

/**
 * Persistence for webhook endpoints and the delivery log (Issues #808, #810).
 */

/**
 * Idempotency key included in every webhook payload so merchants can
 * deduplicate retried deliveries (Issue: duplicate webhook delivery).
 */
export function buildEventId(paymentId: string, eventType: string): string {
  return `${paymentId}:${eventType}`;
}

/** Test deliveries allowed per endpoint per hour (Issue #808). */
export const TEST_DELIVERY_LIMIT_PER_HOUR = 5;

/** Delivery log retention (Issue #810). */
export const DELIVERY_LOG_RETENTION_DAYS = 30;

/** Upper bound on a delivery listing page. */
export const MAX_DELIVERY_PAGE_SIZE = 100;
export const DEFAULT_DELIVERY_PAGE_SIZE = 20;

export class WebhookStore {
  constructor(private readonly pool: Pool) {}

  async getEndpoint(endpointId: string): Promise<WebhookEndpoint | null> {
    const { rows } = await this.pool.query(
      `SELECT id, merchant_id, url, signing_secret, event_types, enabled,
              COALESCE(signing_algorithm, 'hmac_sha256') AS signing_algorithm
         FROM webhook_endpoints
        WHERE id = $1`,
      [endpointId],
    ).catch(async () => {
      // Fallback for tables prior to signing_algorithm column
      return this.pool.query(
        `SELECT id, merchant_id, url, signing_secret, event_types, enabled
           FROM webhook_endpoints
          WHERE id = $1`,
        [endpointId],
      );
    });
    if (rows.length === 0) return null;

    const row = rows[0];
    return {
      id: row.id,
      merchantId: row.merchant_id,
      url: row.url,
      signingSecret: row.signing_secret,
      eventTypes: row.event_types ?? [],
      enabled: row.enabled,
      signingAlgorithm: (row.signing_algorithm as "hmac_sha256" | "ed25519") ?? "hmac_sha256",
    };
  }

  async recordAttempt(attempt: DeliveryAttempt): Promise<string> {
    const { rows } = await this.pool.query(
      `INSERT INTO webhook_delivery_log
         (endpoint_id, event_type, payment_id, attempt_number,
          http_status, response_body, duration_ms, success, livemode, event_id)
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
       RETURNING id`,
      [
        attempt.endpointId,
        attempt.eventType,
        attempt.paymentId,
        attempt.attemptNumber,
        attempt.httpStatus,
        attempt.responseBody,
        attempt.durationMs,
        attempt.success,
        attempt.livemode,
        buildEventId(attempt.paymentId, attempt.eventType),
      ],
    );
    return rows[0].id;
  }

  /**
   * Deliveries for one endpoint, newest first.
   *
   * Always scoped to an endpoint the caller has been authorised for; there is
   * deliberately no "all deliveries" query, because one would need its own
   * authorisation story and nothing needs it.
   */
  async listDeliveries(options: {
    endpointId: string;
    paymentId?: string;
    limit?: number;
    before?: string;
  }): Promise<DeliveryLogRow[]> {
    const limit = Math.min(
      Math.max(options.limit ?? DEFAULT_DELIVERY_PAGE_SIZE, 1),
      MAX_DELIVERY_PAGE_SIZE,
    );

    const params: unknown[] = [options.endpointId];
    let sql = `SELECT id, endpoint_id, event_type, payment_id, attempt_number,
                      delivered_at, http_status, response_body, duration_ms,
                      success, livemode
                 FROM webhook_delivery_log
                WHERE endpoint_id = $1`;

    if (options.paymentId) {
      params.push(options.paymentId);
      sql += ` AND payment_id = $${params.length}`;
    }

    // Keyset pagination on the indexed ordering column. Offset pagination
    // would drift as new deliveries arrive mid-page, which on a debugging
    // surface means a merchant can miss the attempt they are looking for.
    if (options.before) {
      params.push(options.before);
      sql += ` AND delivered_at < $${params.length}`;
    }

    params.push(limit);
    sql += ` ORDER BY delivered_at DESC LIMIT $${params.length}`;

    const { rows } = await this.pool.query(sql, params);
    return rows as DeliveryLogRow[];
  }

  /**
   * Test deliveries made for an endpoint in the trailing hour.
   *
   * Counted from the log rather than a separate counter, so there is one
   * source of truth. A counter that drifts from the log is worse than a
   * slightly more expensive query on a path capped at five calls an hour.
   */
  async countRecentTestDeliveries(endpointId: string): Promise<number> {
    const { rows } = await this.pool.query(
      `SELECT COUNT(*)::int AS count
         FROM webhook_delivery_log
        WHERE endpoint_id = $1
          AND livemode = FALSE
          AND delivered_at > NOW() - INTERVAL '1 hour'`,
      [endpointId],
    );
    return rows[0]?.count ?? 0;
  }

  /** Delete log rows past the retention window. Returns rows removed. */
  async purgeExpiredDeliveries(
    retentionDays: number = DELIVERY_LOG_RETENTION_DAYS,
  ): Promise<number> {
    const { rowCount } = await this.pool.query(
      `DELETE FROM webhook_delivery_log
        WHERE delivered_at < NOW() - ($1 || ' days')::interval`,
      [String(retentionDays)],
    );
    return rowCount ?? 0;
  }
}
