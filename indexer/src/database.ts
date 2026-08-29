/**
 * FluxaPay Indexer Database Module
 * Handles PostgreSQL connections, event persistence, dispute status updates,
 * dead-letter queue operations, and REST API queries.
 */

import { Pool } from "pg";
import { AnyEvent } from "./types";

export interface DLQRecord {
  id: number;
  event_id: string;
  raw_data: any;
  error: string;
  retry_count: number;
  created_at: Date;
}

export class Database {
  private pool: Pool;
  private initialized = false;

  constructor(connectionString: string) {
    this.pool = new Pool({ connectionString });
  }

  async initialize(): Promise<void> {
    try {
      const client = await this.pool.connect();
      await client.query("SELECT 1");
      client.release();
      this.initialized = true;
      console.log("Database connected successfully");
    } catch (error) {
      console.error("Failed to connect to database:", error);
      throw error;
    }
  }

  async storeEvent(event: AnyEvent): Promise<boolean> {
    if (!this.initialized) throw new Error("Database not initialized");

    const client = await this.pool.connect();
    try {
      await client.query("BEGIN");

      // Check if event already exists (idempotency)
      const existingEvent = await client.query(
        "SELECT id FROM contract_events WHERE event_id = $1",
        [event.id]
      );

      if (existingEvent.rows.length > 0) {
        console.log(`Event ${event.id} already processed, skipping...`);
        await client.query("ROLLBACK");
        return false;
      }

      // Determine table based on event topic
      const [eventType, eventSubtype] = event.topic;
      const table = this.getTableName(eventType, eventSubtype);

      // Store in contract_events (dedup table)
      await client.query(
        `INSERT INTO contract_events (event_id, event_type, ledger, tx_hash, timestamp, data, contract_id)
         VALUES ($1, $2, $3, $4, to_timestamp($5), $6, $7)
         ON CONFLICT (event_id) DO NOTHING`,
        [
          event.id,
          eventType,
          event.ledger,
          event.txHash,
          event.timestamp,
          JSON.stringify(event.value),
          event.contractId || null,
        ]
      );

      // Store in type-specific table
      await this.storeTypedEvent(table, event, client);

      await client.query("COMMIT");
      return true;
    } catch (error) {
      await client.query("ROLLBACK");
      console.error("Error storing event:", error);
      throw error;
    } finally {
      client.release();
    }
  }

  private async storeTypedEvent(
    table: string,
    event: AnyEvent,
    client: any
  ): Promise<void> {
    const value = event.value as Record<string, unknown>;

    switch (table) {
      case "payments":
        await client.query(
          `INSERT INTO payments (payment_id, merchant_id, amount, currency, status, created_at)
           VALUES ($1, $2, $3, $4, $5, to_timestamp($6))
           ON CONFLICT (payment_id) DO UPDATE SET status = $5`,
          [
            value.payment_id,
            value.merchant_id,
            value.amount,
            value.currency,
            event.topic[1],
            event.timestamp,
          ]
        );
        break;

      case "refunds":
        await client.query(
          `INSERT INTO refunds (refund_id, payment_id, amount, status, created_at)
           VALUES ($1, $2, $3, $4, to_timestamp($5))
           ON CONFLICT (refund_id) DO UPDATE SET status = $4`,
          [
            value.refund_id,
            value.payment_id,
            value.amount,
            event.topic[1],
            event.timestamp,
          ]
        );
        break;

      case "disputes": {
        // Issue #615: Persist dispute status updates for RESOLVED, REJECTED, and ESCALATED events
        const subtype = event.topic[1];
        if (subtype === "RESOLVED") {
          const res = await client.query(
            `UPDATE disputes
             SET status = 'Resolved', resolved_at = to_timestamp($2), updated_at = CURRENT_TIMESTAMP
             WHERE dispute_id = $1`,
            [value.dispute_id, event.timestamp]
          );
          if (res.rowCount === 0) {
            console.warn(`[DisputeUpdate] Dispute ID ${value.dispute_id} not found for RESOLVED event`);
          }
        } else if (subtype === "REJECTED") {
          const res = await client.query(
            `UPDATE disputes
             SET status = 'Rejected', updated_at = CURRENT_TIMESTAMP
             WHERE dispute_id = $1`,
            [value.dispute_id]
          );
          if (res.rowCount === 0) {
            console.warn(`[DisputeUpdate] Dispute ID ${value.dispute_id} not found for REJECTED event`);
          }
        } else if (subtype === "ESCALATED") {
          const res = await client.query(
            `UPDATE disputes
             SET escalated = true, status = 'Escalated', updated_at = CURRENT_TIMESTAMP
             WHERE dispute_id = $1`,
            [value.dispute_id]
          );
          if (res.rowCount === 0) {
            console.warn(`[DisputeUpdate] Dispute ID ${value.dispute_id} not found for ESCALATED event`);
          }
        } else {
          // CREATED or default status insertion
          const status = subtype === "CREATED" ? "Open" : subtype;
          await client.query(
            `INSERT INTO disputes (dispute_id, payment_id, amount, status, created_at, escalated)
             VALUES ($1, $2, $3, $4, to_timestamp($5), false)
             ON CONFLICT (dispute_id) DO UPDATE SET status = EXCLUDED.status`,
            [
              value.dispute_id,
              value.payment_id,
              value.amount,
              status,
              event.timestamp,
            ]
          );
        }
        break;
      }

      case "merchants":
        await client.query(
          `INSERT INTO merchants (merchant_id, status, last_update)
           VALUES ($1, $2, to_timestamp($3))
           ON CONFLICT (merchant_id) DO UPDATE SET status = $2, last_update = to_timestamp($3)`,
          [value.merchant_id, value.status, event.timestamp]
        );
        break;

      case "streams": {
        const subtype = event.topic[1];
        if (subtype === "CANCELLED") {
          await client.query(
            `UPDATE streams SET status = 'Cancelled', updated_at = to_timestamp($2) WHERE stream_id = $1`,
            [value.stream_id, event.timestamp]
          );
        } else if (subtype === "PAUSED") {
          await client.query(
            `UPDATE streams SET status = 'Paused', updated_at = to_timestamp($2) WHERE stream_id = $1`,
            [value.stream_id, event.timestamp]
          );
        } else if (subtype === "RESUMED") {
          await client.query(
            `UPDATE streams SET status = 'Active', updated_at = to_timestamp($2) WHERE stream_id = $1`,
            [value.stream_id, event.timestamp]
          );
        } else {
          await client.query(
            `INSERT INTO streams (stream_id, sender, receiver, amount, status, created_at)
             VALUES ($1, $2, $3, $4, $5, to_timestamp($6))
             ON CONFLICT (stream_id) DO UPDATE SET status = $5`,
            [
              value.stream_id,
              value.sender,
              value.receiver,
              value.amount,
              subtype === "CREATED" ? "Active" : subtype,
              event.timestamp,
            ]
          );
        }
        break;
      }

      case "stream_withdrawals":
        await client.query(
          `INSERT INTO stream_withdrawals (stream_id, recipient, amount, remaining_deposit, created_at)
           VALUES ($1, $2, $3, $4, to_timestamp($5))`,
          [
            value.stream_id || (Array.isArray(value) ? value[0] : null),
            value.recipient || value.destination || value.receiver || (Array.isArray(value) ? value[2] || value[1] : null),
            value.amount || value.withdrawable || (Array.isArray(value) ? value[3] : null),
            value.remaining_deposit || (Array.isArray(value) ? value[4] : null),
            event.timestamp,
          ]
        );
        break;

      case "subscriptions":
        await client.query(
          `INSERT INTO subscriptions (subscription_id, payer, status, created_at)
           VALUES ($1, $2, $3, to_timestamp($4))
           ON CONFLICT (subscription_id) DO UPDATE SET status = $3`,
          [
            value.subscription_id,
            value.payer,
            value.status,
            event.timestamp,
          ]
        );
        break;

      case "dispute_bonds":
        await client.query(
          `INSERT INTO dispute_bonds (dispute_id, recipient, amount, status, created_at)
           VALUES ($1, $2, $3, $4, to_timestamp($5))`,
          [
            value.dispute_id,
            value.recipient,
            value.amount,
            event.topic[1],
            event.timestamp,
          ]
        );
        break;

      case "invoices":
        await client.query(
          `INSERT INTO invoices (invoice_id, merchant_id, total_amount, status, created_at)
           VALUES ($1, $2, $3, $4, to_timestamp($5))
           ON CONFLICT (invoice_id) DO UPDATE SET status = $4`,
          [
            value.invoice_id,
            value.merchant_id,
            value.total_amount,
            event.topic[1],
            event.timestamp,
          ]
        );
        break;
    }
  }

  private getTableName(eventType: string, eventSubtype?: string): string {
    if (eventType === "DISPUTE" && (eventSubtype === "BOND_RETURNED" || eventSubtype === "BOND_FORFEITED")) {
      return "dispute_bonds";
    }

    const tableMap: Record<string, string> = {
      PAYMENT: "payments",
      REFUND: "refunds",
      DISPUTE: "disputes",
      MERCHANT: "merchants",
      STREAM: "streams",
      SUBSCRIPTION: "subscriptions",
      INVOICE: "invoices",
    };
    return tableMap[eventType] || "contract_events";
  }

  // ── Read queries (Issue #616 REST API Backing Queries) ─────────────────

  async getPaymentById(paymentId: string): Promise<unknown | null> {
    const { rows } = await this.pool.query(
      `SELECT payment_id, merchant_id, amount, currency, status, created_at, updated_at
       FROM payments WHERE payment_id = $1`,
      [paymentId]
    );
    return rows.length > 0 ? rows[0] : null;
  }

  async getPaymentsByMerchantPaginated(
    merchantId: string,
    page = 1,
    limit = 20,
    status?: string
  ): Promise<{
    payments: unknown[];
    pagination: { page: number; limit: number; total: number; totalPages: number };
  }> {
    const pageNum = Math.max(1, page);
    const limitNum = Math.max(1, limit);
    const offset = (pageNum - 1) * limitNum;

    let countSql = `SELECT COUNT(*)::int AS total FROM payments WHERE merchant_id = $1`;
    let dataSql = `SELECT payment_id, merchant_id, amount, currency, status, created_at, updated_at
                   FROM payments WHERE merchant_id = $1`;
    const params: unknown[] = [merchantId];

    if (status) {
      countSql += ` AND status = $2`;
      dataSql += ` AND status = $2`;
      params.push(status);
    }

    dataSql += ` ORDER BY created_at DESC LIMIT $${params.length + 1} OFFSET $${params.length + 2}`;

    const countResult = await this.pool.query(countSql, params);
    const total = countResult.rows[0]?.total || 0;

    const dataParams = [...params, limitNum, offset];
    const dataResult = await this.pool.query(dataSql, dataParams);
    const totalPages = Math.ceil(total / limitNum) || (total === 0 ? 0 : 1);

    return {
      payments: dataResult.rows,
      pagination: {
        page: pageNum,
        limit: limitNum,
        total,
        totalPages,
      },
    };
  }

  async getPaymentsByMerchant(merchantId: string, limit = 100): Promise<unknown[]> {
    const { rows } = await this.pool.query(
      `SELECT payment_id, merchant_id, amount, currency, status, created_at
       FROM payments WHERE merchant_id = $1 ORDER BY created_at DESC LIMIT $2`,
      [merchantId, limit]
    );
    return rows;
  }

  async getDisputesByMerchant(merchantId: string, status?: string): Promise<unknown[]> {
    let sql = `SELECT d.dispute_id, d.payment_id, d.amount, d.status, d.escalated, d.resolved_at, d.created_at
               FROM disputes d
               JOIN payments p ON p.payment_id = d.payment_id
               WHERE p.merchant_id = $1`;
    const params: unknown[] = [merchantId];

    if (status) {
      sql += ` AND d.status = $2`;
      params.push(status);
    }

    sql += ` ORDER BY d.created_at DESC`;
    const { rows } = await this.pool.query(sql, params);
    return rows;
  }

  async getRefundById(refundId: string): Promise<unknown | null> {
    const { rows } = await this.pool.query(
      `SELECT refund_id, payment_id, amount, status, created_at, updated_at
       FROM refunds WHERE refund_id = $1`,
      [refundId]
    );
    return rows.length > 0 ? rows[0] : null;
  }

  async getRefundsByMerchant(merchantId: string, limit = 100): Promise<unknown[]> {
    const { rows } = await this.pool.query(
      `SELECT r.refund_id, r.payment_id, r.amount, r.status, r.created_at
       FROM refunds r
       JOIN payments p ON p.payment_id = r.payment_id
       WHERE p.merchant_id = $1
       ORDER BY r.created_at DESC LIMIT $2`,
      [merchantId, limit]
    );
    return rows;
  }

  async getEventsFiltered(
    type?: string,
    fromLedger?: number,
    toLedger?: number
  ): Promise<unknown[]> {
    let sql = `SELECT id, event_id, event_type, ledger, tx_hash, timestamp, data, contract_id, created_at
               FROM contract_events WHERE 1=1`;
    const params: unknown[] = [];

    if (type) {
      params.push(type);
      sql += ` AND (event_type = $${params.length} OR event_type LIKE $${params.length} || '/%' OR data->>'topic' LIKE '%' || $${params.length} || '%')`;
    }

    if (fromLedger !== undefined && !isNaN(fromLedger)) {
      params.push(fromLedger);
      sql += ` AND ledger >= $${params.length}`;
    }

    if (toLedger !== undefined && !isNaN(toLedger)) {
      params.push(toLedger);
      sql += ` AND ledger <= $${params.length}`;
    }

    sql += ` ORDER BY ledger DESC, id DESC LIMIT 100`;
    const { rows } = await this.pool.query(sql, params);
    return rows;
  }

  async checkHealth(): Promise<{ healthy: boolean; database: string; details?: string }> {
    try {
      await this.pool.query("SELECT 1");
      return { healthy: true, database: "connected" };
    } catch (error: any) {
      return { healthy: false, database: "disconnected", details: error.message || String(error) };
    }
  }

  async getAdminStats(): Promise<Record<string, number>> {
    const { rows } = await this.pool.query(
      `SELECT event_type, COUNT(*)::int AS count FROM contract_events GROUP BY event_type`
    );
    return Object.fromEntries(rows.map((r: { event_type: string; count: number }) => [r.event_type, r.count]));
  }

  // ── Dead-Letter Queue Operations (Issue #617) ──────────────────────────

  async storeDeadLetterEvent(eventId: string, rawData: unknown, error: string): Promise<void> {
    await this.pool.query(
      `INSERT INTO dead_letter_events (event_id, raw_data, error, created_at, retry_count)
       VALUES ($1, $2, $3, CURRENT_TIMESTAMP, 0)
       ON CONFLICT (event_id) DO UPDATE SET
         error = EXCLUDED.error,
         created_at = CURRENT_TIMESTAMP`,
      [eventId, JSON.stringify(rawData), error]
    );
  }

  async getEligibleDLQEvents(minAgeSeconds = 60, limit = 100): Promise<DLQRecord[]> {
    const { rows } = await this.pool.query(
      `SELECT id, event_id, raw_data, error, retry_count, created_at
       FROM dead_letter_events
       WHERE created_at <= NOW() - ($1 || ' seconds')::INTERVAL
       ORDER BY created_at ASC
       LIMIT $2`,
      [minAgeSeconds, limit]
    );
    return rows;
  }

  async getAllDLQEvents(limit = 100): Promise<DLQRecord[]> {
    const { rows } = await this.pool.query(
      `SELECT id, event_id, raw_data, error, retry_count, created_at
       FROM dead_letter_events
       ORDER BY created_at ASC
       LIMIT $1`,
      [limit]
    );
    return rows;
  }

  async incrementDLQRetryCount(eventId: string, error: string): Promise<void> {
    await this.pool.query(
      `UPDATE dead_letter_events
       SET retry_count = retry_count + 1, error = $2
       WHERE event_id = $1`,
      [eventId, error]
    );
  }

  async removeDeadLetterEvent(eventId: string): Promise<void> {
    await this.pool.query(`DELETE FROM dead_letter_events WHERE event_id = $1`, [eventId]);
  }

  async close(): Promise<void> {
    await this.pool.end();
  }
}
