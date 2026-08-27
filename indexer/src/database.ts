/**
 * FluxaPay Indexer Database Module
 * Handles PostgreSQL connections and event persistence with idempotency
 */

import { Pool } from "pg";
import { ContractEvent, AnyEvent } from "./types";

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
      // Check if event already exists (idempotency)
      const existingEvent = await client.query(
        "SELECT id FROM contract_events WHERE event_id = $1",
        [event.id]
      );

      if (existingEvent.rows.length > 0) {
        console.log(`Event ${event.id} already processed, skipping...`);
        return false;
      }

      // Determine table based on event topic
      const [eventType, eventSubtype] = event.topic;
      const table = this.getTableName(eventType, eventSubtype);

      // Store in contract_events (dedup table)
      await client.query(
        `INSERT INTO contract_events (event_id, event_type, ledger, tx_hash, timestamp, data)
         VALUES ($1, $2, $3, $4, to_timestamp($5), $6)
         ON CONFLICT (event_id) DO NOTHING`,
        [
          event.id,
          eventType,
          event.ledger,
          event.txHash,
          event.timestamp,
          JSON.stringify(event.value),
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

      case "disputes":
        await client.query(
          `INSERT INTO disputes (dispute_id, payment_id, amount, status, created_at)
           VALUES ($1, $2, $3, $4, to_timestamp($5))
           ON CONFLICT (dispute_id) DO UPDATE SET status = $4`,
          [
            value.dispute_id,
            value.payment_id,
            value.amount,
            event.topic[1],
            event.timestamp,
          ]
        );
        break;

      case "merchants":
        await client.query(
          `INSERT INTO merchants (merchant_id, status, last_update)
           VALUES ($1, $2, to_timestamp($3))
           ON CONFLICT (merchant_id) DO UPDATE SET status = $2, last_update = to_timestamp($3)`,
          [value.merchant_id, value.status, event.timestamp]
        );
        break;

      case "streams":
        await client.query(
          `INSERT INTO streams (stream_id, sender, receiver, amount, status, created_at)
           VALUES ($1, $2, $3, $4, $5, to_timestamp($6))
           ON CONFLICT (stream_id) DO UPDATE SET status = $5`,
          [
            value.stream_id,
            value.sender,
            value.receiver,
            value.amount,
            event.topic[1],
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
        // Issue #677: bond lifecycle events (BOND_RETURNED / BOND_FORFEITED)
        // carry a recipient + amount, not a payment_id/status update, so
        // they're tracked separately from the `disputes` table.
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
    // Issue #677: dispute bond events route to a dedicated table since
    // their shape (recipient + amount) doesn't fit the `disputes` row.
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

  // ── Read queries (Issue #672: REST API backing queries) ─────────────────

  async getPaymentsByMerchant(merchantId: string, limit = 100): Promise<unknown[]> {
    const { rows } = await this.pool.query(
      `SELECT payment_id, merchant_id, amount, currency, status, created_at
       FROM payments WHERE merchant_id = $1 ORDER BY created_at DESC LIMIT $2`,
      [merchantId, limit]
    );
    return rows;
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

  async getDisputesByMerchant(merchantId: string, limit = 100): Promise<unknown[]> {
    const { rows } = await this.pool.query(
      `SELECT d.dispute_id, d.payment_id, d.amount, d.status, d.created_at
       FROM disputes d
       JOIN payments p ON p.payment_id = d.payment_id
       WHERE p.merchant_id = $1
       ORDER BY d.created_at DESC LIMIT $2`,
      [merchantId, limit]
    );
    return rows;
  }

  /** Admin-only: event counts by type, across all merchants. */
  async getAdminStats(): Promise<Record<string, number>> {
    const { rows } = await this.pool.query(
      `SELECT event_type, COUNT(*)::int AS count FROM contract_events GROUP BY event_type`
    );
    return Object.fromEntries(rows.map((r: { event_type: string; count: number }) => [r.event_type, r.count]));
  }

  async close(): Promise<void> {
    await this.pool.end();
  }
}
