import { randomBytes, randomUUID } from "node:crypto";
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

  async listEndpoints(merchantId: string): Promise<WebhookEndpoint[]> {
    const { rows } = await this.pool.query(
      `SELECT id, merchant_id, url, signing_secret, event_types, enabled,
              COALESCE(signing_algorithm, 'hmac_sha256') AS signing_algorithm
         FROM webhook_endpoints
        WHERE merchant_id = $1
        ORDER BY created_at ASC, id ASC`,
      [merchantId],
    ).catch(async () => this.pool.query(
      `SELECT id, merchant_id, url, signing_secret, event_types, enabled
         FROM webhook_endpoints
        WHERE merchant_id = $1
        ORDER BY created_at ASC, id ASC`,
      [merchantId],
    ));

    return rows.map((row) => this.mapEndpoint(row));
  }

  async createEndpoint(input: {
    merchantId: string;
    url: string;
    eventTypes: string[];
    signingSecret: string;
    signingAlgorithm?: "hmac_sha256" | "ed25519";
  }): Promise<WebhookEndpoint> {
    const id = `wh_${randomUUID()}`;
    const algorithm = input.signingAlgorithm ?? "hmac_sha256";
    const { rows } = await this.pool.query(
      `INSERT INTO webhook_endpoints
         (id, merchant_id, url, signing_secret, event_types, enabled, signing_algorithm)
       VALUES ($1, $2, $3, $4, $5, TRUE, $6)
       RETURNING id, merchant_id, url, signing_secret, event_types, enabled,
                 COALESCE(signing_algorithm, 'hmac_sha256') AS signing_algorithm`,
      [id, input.merchantId, input.url, input.signingSecret, input.eventTypes, algorithm],
    ).catch(async () => {
      const result = await this.pool.query(
        `INSERT INTO webhook_endpoints
           (id, merchant_id, url, signing_secret, event_types, enabled)
         VALUES ($1, $2, $3, $4, $5, TRUE)
         RETURNING id, merchant_id, url, signing_secret, event_types, enabled`,
        [id, input.merchantId, input.url, input.signingSecret, input.eventTypes],
      );
      return result;
    });
    return this.mapEndpoint(rows[0]);
  }

  async updateEndpoint(
    endpointId: string,
    merchantId: string,
    input: {
      url?: string;
      eventTypes?: string[];
      enabled?: boolean;
      signingAlgorithm?: "hmac_sha256" | "ed25519";
      signingSecret?: string;
    },
  ): Promise<WebhookEndpoint | null> {
    const sets: string[] = [];
    const params: unknown[] = [];
    const add = (value: unknown) => {
      params.push(value);
      return `$${params.length}`;
    };

    if (input.url !== undefined) sets.push(`url = ${add(input.url)}`);
    if (input.eventTypes !== undefined) sets.push(`event_types = ${add(input.eventTypes)}`);
    if (input.enabled !== undefined) sets.push(`enabled = ${add(input.enabled)}`);
    if (input.signingAlgorithm !== undefined) sets.push(`signing_algorithm = ${add(input.signingAlgorithm)}`);
    if (input.signingSecret !== undefined) {
      sets.push(`signing_secret = ${add(input.signingSecret)}`);
    }
    if (sets.length === 0) return this.getEndpointForMerchant(endpointId, merchantId);

    sets.push("updated_at = NOW()");
    params.push(endpointId, merchantId);
    const idParam = `$${params.length - 1}`;
    const merchantParam = `$${params.length}`;
    const { rows } = await this.pool.query(
      `UPDATE webhook_endpoints
          SET ${sets.join(", ")}
        WHERE id = ${idParam} AND merchant_id = ${merchantParam}
        RETURNING id, merchant_id, url, signing_secret, event_types, enabled,
                  COALESCE(signing_algorithm, 'hmac_sha256') AS signing_algorithm`,
      params,
    ).catch(async () => {
      const fallback: string[] = [];
      const fallbackValues: unknown[] = [];
      const fallbackAdd = (value: unknown) => {
        fallbackValues.push(value);
        return `$${fallbackValues.length}`;
      };
      if (input.url !== undefined) fallback.push(`url = ${fallbackAdd(input.url)}`);
      if (input.eventTypes !== undefined) fallback.push(`event_types = ${fallbackAdd(input.eventTypes)}`);
      if (input.enabled !== undefined) fallback.push(`enabled = ${fallbackAdd(input.enabled)}`);
      if (input.signingSecret !== undefined) fallback.push(`signing_secret = ${fallbackAdd(input.signingSecret)}`);
      fallback.push("updated_at = NOW()");
      fallbackValues.push(endpointId, merchantId);
      const result = await this.pool.query(
        `UPDATE webhook_endpoints
            SET ${fallback.join(", ")}
          WHERE id = $${fallbackValues.length - 1} AND merchant_id = $${fallbackValues.length}
          RETURNING id, merchant_id, url, signing_secret, event_types, enabled`,
        fallbackValues,
      );
      return result;
    });
    return rows.length > 0 ? this.mapEndpoint(rows[0]) : null;
  }

  async deleteEndpoint(endpointId: string, merchantId: string): Promise<boolean> {
    const { rowCount } = await this.pool.query(
      `DELETE FROM webhook_endpoints WHERE id = $1 AND merchant_id = $2`,
      [endpointId, merchantId],
    );
    return (rowCount ?? 0) > 0;
  }

  async rotateSigningSecret(
    endpointId: string,
    merchantId: string,
  ): Promise<{ endpoint: WebhookEndpoint; signingSecret: string } | null> {
    const signingSecret = randomBytes(32).toString("base64url");
    const { rows } = await this.pool.query(
      `UPDATE webhook_endpoints
          SET signing_secret = $1, updated_at = NOW()
        WHERE id = $2 AND merchant_id = $3
        RETURNING id, merchant_id, url, signing_secret, event_types, enabled,
                  COALESCE(signing_algorithm, 'hmac_sha256') AS signing_algorithm`,
      [signingSecret, endpointId, merchantId],
    ).catch(async () => {
      const result = await this.pool.query(
        `UPDATE webhook_endpoints
            SET signing_secret = $1, updated_at = NOW()
          WHERE id = $2 AND merchant_id = $3
          RETURNING id, merchant_id, url, signing_secret, event_types, enabled`,
        [signingSecret, endpointId, merchantId],
      );
      return result;
    });
    if (rows.length === 0) return null;
    return { endpoint: this.mapEndpoint(rows[0]), signingSecret };
  }

  async getEndpointForMerchant(
    endpointId: string,
    merchantId: string,
  ): Promise<WebhookEndpoint | null> {
    const endpoint = await this.getEndpoint(endpointId);
    return endpoint?.merchantId === merchantId ? endpoint : null;
  }

  private mapEndpoint(row: Record<string, any>): WebhookEndpoint {
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

  async listEnabledEndpointsForEvent(
    merchantId: string,
    eventType: string,
  ): Promise<WebhookEndpoint[]> {
    const { rows } = await this.pool.query(
      `SELECT id, merchant_id, url, signing_secret, event_types, enabled,
              COALESCE(signing_algorithm, 'hmac_sha256') AS signing_algorithm
         FROM webhook_endpoints
        WHERE merchant_id = $1
          AND enabled = TRUE
          AND (cardinality(event_types) = 0 OR $2 = ANY(event_types))`,
      [merchantId, eventType],
    );

    return rows.map((row) => ({
      id: row.id,
      merchantId: row.merchant_id,
      url: row.url,
      signingSecret: row.signing_secret,
      eventTypes: row.event_types ?? [],
      enabled: row.enabled,
      signingAlgorithm: (row.signing_algorithm as "hmac_sha256" | "ed25519") ?? "hmac_sha256",
    }));
  }

  async recordAttempt(attempt: DeliveryAttempt): Promise<string> {
    const { rows } = await this.pool.query(
      `INSERT INTO webhook_delivery_log
         (endpoint_id, event_type, payment_id, attempt_number,
          http_status, response_body, duration_ms, success, livemode)
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
