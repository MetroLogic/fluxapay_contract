/**
 * FluxaPay Indexer Database Module
 * Handles PostgreSQL connections, event persistence, dispute status updates,
 * dead-letter queue operations, and REST API queries.
 *
 * Issue #842: all queries use Drizzle ORM for compile-time schema safety.
 */

import { Pool } from "pg";
import { drizzle, type NodePgDatabase } from "drizzle-orm/node-postgres";
import {
  and,
  count,
  desc,
  eq,
  gte,
  lte,
  sql,
  asc,
  inArray,
  lt,
  or,
} from "drizzle-orm";
import { AnyEvent } from "./types";
import * as schema from "./schema";
import {
  contractEvents,
  payments,
  refunds,
  disputes,
  merchants,
  streams,
  streamWithdrawals,
  subscriptions,
  invoices,
  disputeBonds,
  deadLetterEvents,
  settlements,
} from "./schema";
import { setCachedRate } from "./fx-rate-cache";

export interface DLQRecord {
  id: number;
  event_id: string;
  raw_data: any;
  error: string;
  retry_count: number;
  created_at: Date;
}

export interface ReconciliationRow {
  payment_id: string;
  type: "payment" | "refund" | "settlement";
  amount_usdc: string;
  fiat_amount: string;
  fiat_currency: string;
  status: string;
  created_at: string;
  settled_at: string;
  customer_ref: string;
  tags: string;
}

export class Database {
  private pool: Pool;
  private db: NodePgDatabase<typeof schema>;
  private initialized = false;

  constructor(connectionString: string) {
    this.pool = new Pool({ connectionString });
    this.db = drizzle(this.pool, { schema });
  }

  /**
   * The underlying connection pool.
   *
   * Exposed so modules that own their own tables — the webhook store
   * (Issues #808, #810) — can share this pool rather than opening a second
   * one against the same database.
   */
  getPool(): Pool {
    return this.pool;
  }

  /** Drizzle client — preferred for new typed queries. */
  getDb(): NodePgDatabase<typeof schema> {
    return this.db;
  }

  async initialize(): Promise<void> {
    try {
      await this.db.execute(sql`SELECT 1`);
      this.initialized = true;
      console.log("Database connected successfully");
    } catch (error) {
      console.error("Failed to connect to database:", error);
      throw error;
    }
  }

  async storeEvent(event: AnyEvent): Promise<boolean> {
    if (!this.initialized) throw new Error("Database not initialized");

    return this.db.transaction(async (tx) => {
      const existing = await tx
        .select({ id: contractEvents.id })
        .from(contractEvents)
        .where(eq(contractEvents.eventId, event.id))
        .limit(1);

      if (existing.length > 0) {
        console.log(`Event ${event.id} already processed, skipping...`);
        return false;
      }

      const [eventType, eventSubtype] = event.topic;
      const table = this.getTableName(eventType, eventSubtype);

      await tx
        .insert(contractEvents)
        .values({
          eventId: event.id,
          eventType,
          ledger: event.ledger,
          txHash: event.txHash,
          timestamp: new Date(event.timestamp * 1000),
          data: event.value as any,
          contractId: event.contractId || null,
        })
        .onConflictDoNothing();

      await this.storeTypedEventDrizzle(table, event, tx);
          event.ledger,
          event.txHash,
          event.timestamp,
          JSON.stringify(event.value),
          event.contractId || null,
        ]
      );

      // Store in type-specific table
      await this.storeTypedEvent(table, event, client);

      // Issue #839: keep an in-memory FX rate cache for /v1/fx/convert.
      this.updateFxRateCache(event);

      await client.query("COMMIT");
      return true;
    });
  }

  private async storeTypedEventDrizzle(
    table: string,
    event: AnyEvent,
    tx: NodePgDatabase<typeof schema>,
  ): Promise<void> {
    const value = event.value as Record<string, unknown>;
    const ts = new Date(event.timestamp * 1000);

    switch (table) {
      case "payments":
        await tx
          .insert(payments)
          .values({
            paymentId: String(value.payment_id),
            merchantId: String(value.merchant_id),
            amount: BigInt(String(value.amount ?? 0)),
            currency: value.currency != null ? String(value.currency) : null,
            status: event.topic[1],
            createdAt: ts,
            fiatAmount: value.fiat_amount != null ? String(value.fiat_amount) : null,
            fiatCurrency: value.fiat_currency != null ? String(value.fiat_currency) : null,
            customerRef: value.customer_ref != null ? String(value.customer_ref) : null,
            tags: value.tags != null ? String(value.tags) : null,
            settledAt:
              event.topic[1] === "SETTLED" || event.topic[1] === "Settled"
                ? ts
                : null,
          })
          .onConflictDoUpdate({
            target: payments.paymentId,
            set: {
              status: event.topic[1],
              updatedAt: new Date(),
              settledAt:
                event.topic[1] === "SETTLED" || event.topic[1] === "Settled"
                  ? ts
                  : undefined,
            },
          });
        break;

      case "refunds":
        await tx
          .insert(refunds)
          .values({
            refundId: String(value.refund_id),
            paymentId: String(value.payment_id),
            amount: BigInt(String(value.amount ?? 0)),
            status: event.topic[1],
            createdAt: ts,
          })
          .onConflictDoUpdate({
            target: refunds.refundId,
            set: { status: event.topic[1], updatedAt: new Date() },
          });
        break;

      case "disputes": {
        const subtype = event.topic[1];
        if (subtype === "RESOLVED") {
          const res = await tx
            .update(disputes)
            .set({
              status: "Resolved",
              resolvedAt: ts,
              updatedAt: new Date(),
            })
            .where(eq(disputes.disputeId, String(value.dispute_id)));
          if ((res as any).rowCount === 0) {
            console.warn(
              `[DisputeUpdate] Dispute ID ${value.dispute_id} not found for RESOLVED event`,
            );
          }
        } else if (subtype === "REJECTED") {
          const res = await tx
            .update(disputes)
            .set({ status: "Rejected", updatedAt: new Date() })
            .where(eq(disputes.disputeId, String(value.dispute_id)));
          if ((res as any).rowCount === 0) {
            console.warn(
              `[DisputeUpdate] Dispute ID ${value.dispute_id} not found for REJECTED event`,
            );
          }
        } else if (subtype === "ESCALATED") {
          const res = await tx
            .update(disputes)
            .set({
              escalated: true,
              status: "Escalated",
              updatedAt: new Date(),
            })
            .where(eq(disputes.disputeId, String(value.dispute_id)));
          if ((res as any).rowCount === 0) {
            console.warn(
              `[DisputeUpdate] Dispute ID ${value.dispute_id} not found for ESCALATED event`,
            );
          }
        } else {
          const status = subtype === "CREATED" ? "Open" : subtype;
          await tx
            .insert(disputes)
            .values({
              disputeId: String(value.dispute_id),
              paymentId: String(value.payment_id),
              amount: BigInt(String(value.amount ?? 0)),
              status,
              createdAt: ts,
              escalated: false,
            })
            .onConflictDoUpdate({
              target: disputes.disputeId,
              set: { status },
            });
        }
        break;
      }

      case "merchants":
        await tx
          .insert(merchants)
          .values({
            merchantId: String(value.merchant_id),
            status: value.status != null ? String(value.status) : null,
            lastUpdate: ts,
          })
          .onConflictDoUpdate({
            target: merchants.merchantId,
            set: {
              status: value.status != null ? String(value.status) : null,
              lastUpdate: ts,
            },
          });
        break;

      case "streams": {
        const subtype = event.topic[1];
        if (subtype === "CANCELLED") {
          await tx
            .update(streams)
            .set({ status: "Cancelled", updatedAt: ts })
            .where(eq(streams.streamId, String(value.stream_id)));
        } else if (subtype === "PAUSED") {
          await tx
            .update(streams)
            .set({ status: "Paused", updatedAt: ts })
            .where(eq(streams.streamId, String(value.stream_id)));
        } else if (subtype === "RESUMED") {
          await tx
            .update(streams)
            .set({ status: "Active", updatedAt: ts })
            .where(eq(streams.streamId, String(value.stream_id)));
        } else {
          await tx
            .insert(streams)
            .values({
              streamId: String(value.stream_id),
              sender: value.sender != null ? String(value.sender) : null,
              receiver: value.receiver != null ? String(value.receiver) : null,
              amount: value.amount != null ? BigInt(String(value.amount)) : null,
              status: subtype === "CREATED" ? "Active" : subtype,
              createdAt: ts,
            })
            .onConflictDoUpdate({
              target: streams.streamId,
              set: { status: subtype === "CREATED" ? "Active" : subtype },
            });
        }
        break;
      }

      case "stream_withdrawals":
        await tx.insert(streamWithdrawals).values({
          streamId: String(
            value.stream_id || (Array.isArray(value) ? value[0] : null) || "",
          ),
          recipient: String(
            value.recipient ||
              value.destination ||
              value.receiver ||
              (Array.isArray(value) ? value[2] || value[1] : null) ||
              "",
          ),
          amount:
            value.amount != null || value.withdrawable != null
              ? BigInt(
                  String(
                    value.amount ||
                      value.withdrawable ||
                      (Array.isArray(value) ? value[3] : 0) ||
                      0,
                  ),
                )
              : null,
          remainingDeposit:
            value.remaining_deposit != null
              ? BigInt(String(value.remaining_deposit))
              : Array.isArray(value) && value[4] != null
                ? BigInt(String(value[4]))
                : null,
          createdAt: ts,
        });
        await client.query(
          `INSERT INTO stream_withdrawals (stream_id, recipient, amount, remaining_deposit, memo, created_at)
           VALUES ($1, $2, $3, $4, $5, to_timestamp($6))`,
          [
            value.stream_id || (Array.isArray(value) ? value[0] : null),
            value.recipient || value.destination || value.receiver || (Array.isArray(value) ? value[2] || value[1] : null),
            value.amount || value.withdrawable || (Array.isArray(value) ? value[3] : null),
            value.remaining_deposit || (Array.isArray(value) ? value[4] : null),
            value.memo !== undefined ? value.memo : null,
            event.timestamp,
          ]
        );
        break;

      case "subscriptions":
        await tx
          .insert(subscriptions)
          .values({
            subscriptionId: String(value.subscription_id),
            payer: value.payer != null ? String(value.payer) : null,
            status: value.status != null ? String(value.status) : null,
            createdAt: ts,
          })
          .onConflictDoUpdate({
            target: subscriptions.subscriptionId,
            set: {
              status: value.status != null ? String(value.status) : null,
            },
          });
        break;

      case "dispute_bonds":
        await tx.insert(disputeBonds).values({
          disputeId: String(value.dispute_id),
          recipient: value.recipient != null ? String(value.recipient) : null,
          amount: BigInt(String(value.amount ?? 0)),
          status: event.topic[1],
          createdAt: ts,
        });
        break;

      case "invoices":
        await tx
          .insert(invoices)
          .values({
            invoiceId: String(value.invoice_id),
            merchantId: value.merchant_id != null ? String(value.merchant_id) : null,
            totalAmount:
              value.total_amount != null
                ? BigInt(String(value.total_amount))
                : null,
            status: event.topic[1],
            createdAt: ts,
          })
          .onConflictDoUpdate({
            target: invoices.invoiceId,
            set: { status: event.topic[1] },
          });
        break;

      case "settlements":
        await tx.insert(settlements).values({
          paymentId: String(value.payment_id),
          merchantId: String(value.merchant_id),
          amount: BigInt(String(value.amount ?? 0)),
          status: event.topic[1],
          createdAt: ts,
          settledAt: ts,
          fiatAmount: value.fiat_amount != null ? String(value.fiat_amount) : null,
          fiatCurrency:
            value.fiat_currency != null ? String(value.fiat_currency) : null,
          customerRef:
            value.customer_ref != null ? String(value.customer_ref) : null,
          tags: value.tags != null ? String(value.tags) : null,
        });
        break;
    }
  }

  private getTableName(eventType: string, eventSubtype?: string): string {
    if (
      eventType === "DISPUTE" &&
      (eventSubtype === "BOND_RETURNED" || eventSubtype === "BOND_FORFEITED")
    ) {
      return "dispute_bonds";
    }
    if (eventType === "SETTLEMENT" || eventSubtype === "SETTLED") {
      return eventType === "SETTLEMENT" ? "settlements" : "payments";
    }

    const tableMap: Record<string, string> = {
      PAYMENT: "payments",
      REFUND: "refunds",
      DISPUTE: "disputes",
      MERCHANT: "merchants",
      STREAM: "streams",
      SUBSCRIPTION: "subscriptions",
      INVOICE: "invoices",
      SETTLEMENT: "settlements",
    };
    return tableMap[eventType] || "contract_events";
  }

  /**
   * Issue #839: Mirror oracle rate events into the in-memory convert cache.
   * Accepts both `FX_ORACLE` and `ORACLE` topic prefixes and pair formats
   * like `USDC/NGN` or `USDC_NGN`.
   */
  private updateFxRateCache(event: AnyEvent): void {
    const [eventType] = event.topic;
    if (eventType !== "FX_ORACLE" && eventType !== "ORACLE") return;
    const value = event.value as { asset?: string; pair?: string; rate?: number };
    const rawPair = (value.asset || value.pair || "").toString();
    if (!rawPair || typeof value.rate !== "number" || !Number.isFinite(value.rate)) return;
    const pair = rawPair.replace("/", "_").toUpperCase();
    setCachedRate(pair, value.rate, event.timestamp || Math.floor(Date.now() / 1000));
  }

  // ── Read queries (Issue #616 REST API Backing Queries) ─────────────────

  async getPaymentById(paymentId: string): Promise<unknown | null> {
    const rows = await this.db
      .select({
        payment_id: payments.paymentId,
        merchant_id: payments.merchantId,
        amount: payments.amount,
        currency: payments.currency,
        status: payments.status,
        created_at: payments.createdAt,
        updated_at: payments.updatedAt,
      })
      .from(payments)
      .where(eq(payments.paymentId, paymentId))
      .limit(1);
    return rows.length > 0 ? rows[0] : null;
  }

  async getPaymentsByMerchantPaginated(
    merchantId: string,
    page = 1,
    limit = 20,
    status?: string,
  ): Promise<{
    payments: unknown[];
    pagination: { page: number; limit: number; total: number; totalPages: number };
  }> {
    const pageNum = Math.max(1, page);
    const limitNum = Math.max(1, limit);
    const offset = (pageNum - 1) * limitNum;

    const conditions = [eq(payments.merchantId, merchantId)];
    if (status) conditions.push(eq(payments.status, status));

    const [{ total }] = await this.db
      .select({ total: count() })
      .from(payments)
      .where(and(...conditions));

    const rows = await this.db
      .select({
        payment_id: payments.paymentId,
        merchant_id: payments.merchantId,
        amount: payments.amount,
        currency: payments.currency,
        status: payments.status,
        created_at: payments.createdAt,
        updated_at: payments.updatedAt,
      })
      .from(payments)
      .where(and(...conditions))
      .orderBy(desc(payments.createdAt))
      .limit(limitNum)
      .offset(offset);

    const totalNum = Number(total) || 0;
    const totalPages = Math.ceil(totalNum / limitNum) || (totalNum === 0 ? 0 : 1);

    return {
      payments: rows,
      pagination: { page: pageNum, limit: limitNum, total: totalNum, totalPages },
    };
  }

  async getPaymentsByMerchant(merchantId: string, limit = 100): Promise<unknown[]> {
    return this.db
      .select({
        payment_id: payments.paymentId,
        merchant_id: payments.merchantId,
        amount: payments.amount,
        currency: payments.currency,
        status: payments.status,
        created_at: payments.createdAt,
      })
      .from(payments)
      .where(eq(payments.merchantId, merchantId))
      .orderBy(desc(payments.createdAt))
      .limit(limit);
  }

  /**
   * Issue #785: cursor-paginated filtered payments query.
   */
  async getPaymentsFiltered(opts: {
    merchantId: string;
    statuses?: string[];
    from?: string;
    to?: string;
    limit: number;
    cursor?: { createdAt: string; id: string };
  }): Promise<{ payments: Array<Record<string, unknown>>; total: number }> {
    const conditions = [eq(payments.merchantId, opts.merchantId)];
    if (opts.statuses && opts.statuses.length > 0) {
      conditions.push(inArray(payments.status, opts.statuses));
    }
    if (opts.from) {
      conditions.push(gte(payments.createdAt, new Date(opts.from)));
    }
    if (opts.to) {
      conditions.push(lte(payments.createdAt, new Date(opts.to)));
    }
    if (opts.cursor) {
      const cursorDate = new Date(opts.cursor.createdAt);
      conditions.push(
        or(
          lt(payments.createdAt, cursorDate),
          and(
            eq(payments.createdAt, cursorDate),
            lt(payments.id, Number(opts.cursor.id)),
          ),
        )!,
      );
    }

    const [{ total }] = await this.db
      .select({ total: count() })
      .from(payments)
      .where(and(...conditions));

    const rows = await this.db
      .select({
        id: payments.id,
        payment_id: payments.paymentId,
        merchant_id: payments.merchantId,
        amount: payments.amount,
        currency: payments.currency,
        status: payments.status,
        created_at: payments.createdAt,
        updated_at: payments.updatedAt,
      })
      .from(payments)
      .where(and(...conditions))
      .orderBy(desc(payments.createdAt), desc(payments.id))
      .limit(opts.limit);

    return { payments: rows as Array<Record<string, unknown>>, total: Number(total) || 0 };
  }

  async getDisputesByMerchant(merchantId: string, status?: string): Promise<unknown[]> {
    const conditions = [eq(payments.merchantId, merchantId)];
    if (status) conditions.push(eq(disputes.status, status));

    return this.db
      .select({
        dispute_id: disputes.disputeId,
        payment_id: disputes.paymentId,
        amount: disputes.amount,
        status: disputes.status,
        escalated: disputes.escalated,
        resolved_at: disputes.resolvedAt,
        created_at: disputes.createdAt,
      })
      .from(disputes)
      .innerJoin(payments, eq(disputes.paymentId, payments.paymentId))
      .where(and(...conditions))
      .orderBy(desc(disputes.createdAt));
  }

  async getRefundById(refundId: string): Promise<unknown | null> {
    const rows = await this.db
      .select({
        refund_id: refunds.refundId,
        payment_id: refunds.paymentId,
        amount: refunds.amount,
        status: refunds.status,
        created_at: refunds.createdAt,
        updated_at: refunds.updatedAt,
      })
      .from(refunds)
      .where(eq(refunds.refundId, refundId))
      .limit(1);
    return rows.length > 0 ? rows[0] : null;
  }

  async getRefundsByMerchant(merchantId: string, limit = 100): Promise<unknown[]> {
    return this.db
      .select({
        refund_id: refunds.refundId,
        payment_id: refunds.paymentId,
        amount: refunds.amount,
        status: refunds.status,
        created_at: refunds.createdAt,
      })
      .from(refunds)
      .innerJoin(payments, eq(refunds.paymentId, payments.paymentId))
      .where(eq(payments.merchantId, merchantId))
      .orderBy(desc(refunds.createdAt))
      .limit(limit);
  }

  async getEventsFiltered(
    type?: string,
    fromLedger?: number,
    toLedger?: number,
  ): Promise<unknown[]> {
    const conditions = [];
    if (type) {
      conditions.push(
        or(
          eq(contractEvents.eventType, type),
          sql`${contractEvents.eventType} LIKE ${type + "/%"}`,
        )!,
      );
    }
    if (fromLedger !== undefined && !isNaN(fromLedger)) {
      conditions.push(gte(contractEvents.ledger, fromLedger));
    }
    if (toLedger !== undefined && !isNaN(toLedger)) {
      conditions.push(lte(contractEvents.ledger, toLedger));
    }

    return this.db
      .select({
        id: contractEvents.id,
        event_id: contractEvents.eventId,
        event_type: contractEvents.eventType,
        ledger: contractEvents.ledger,
        tx_hash: contractEvents.txHash,
        timestamp: contractEvents.timestamp,
        data: contractEvents.data,
        contract_id: contractEvents.contractId,
        created_at: contractEvents.createdAt,
      })
      .from(contractEvents)
      .where(conditions.length ? and(...conditions) : undefined)
      .orderBy(desc(contractEvents.ledger), desc(contractEvents.id))
      .limit(100);
  }

  async checkHealth(): Promise<{ healthy: boolean; database: string; details?: string }> {
    try {
      await this.db.execute(sql`SELECT 1`);
      return { healthy: true, database: "connected" };
    } catch (error: any) {
      return {
        healthy: false,
        database: "disconnected",
        details: error.message || String(error),
      };
    }
  }

  async getAdminStats(): Promise<Record<string, number>> {
    const rows = await this.db
      .select({
        event_type: contractEvents.eventType,
        count: count(),
      })
      .from(contractEvents)
      .groupBy(contractEvents.eventType);
    return Object.fromEntries(rows.map((r) => [r.event_type, Number(r.count)]));
  }

  // ── Dead-Letter Queue Operations (Issue #617) ──────────────────────────

  async storeDeadLetterEvent(eventId: string, rawData: unknown, error: string): Promise<void> {
    await this.db
      .insert(deadLetterEvents)
      .values({
        eventId,
        rawData: rawData as any,
        error,
        retryCount: 0,
      })
      .onConflictDoUpdate({
        target: deadLetterEvents.eventId,
        set: { error, createdAt: new Date() },
      });
  }

  async getEligibleDLQEvents(minAgeSeconds = 60, limit = 100): Promise<DLQRecord[]> {
    const rows = await this.db.execute(sql`
      SELECT id, event_id, raw_data, error, retry_count, created_at
      FROM dead_letter_events
      WHERE created_at <= NOW() - (${String(minAgeSeconds)} || ' seconds')::INTERVAL
      ORDER BY created_at ASC
      LIMIT ${limit}
    `);
    return (rows as any).rows ?? (rows as unknown as DLQRecord[]);
  }

  async getAllDLQEvents(limit = 100): Promise<DLQRecord[]> {
    const rows = await this.db
      .select({
        id: deadLetterEvents.id,
        event_id: deadLetterEvents.eventId,
        raw_data: deadLetterEvents.rawData,
        error: deadLetterEvents.error,
        retry_count: deadLetterEvents.retryCount,
        created_at: deadLetterEvents.createdAt,
      })
      .from(deadLetterEvents)
      .orderBy(asc(deadLetterEvents.createdAt))
      .limit(limit);
    return rows as unknown as DLQRecord[];
  }

  async incrementDLQRetryCount(eventId: string, error: string): Promise<void> {
    await this.db
      .update(deadLetterEvents)
      .set({
        retryCount: sql`${deadLetterEvents.retryCount} + 1`,
        error,
      })
      .where(eq(deadLetterEvents.eventId, eventId));
  }

  async removeDeadLetterEvent(eventId: string): Promise<void> {
    await this.db
      .delete(deadLetterEvents)
      .where(eq(deadLetterEvents.eventId, eventId));
  }

  /**
   * Issue #840: Stream reconciliation rows for a merchant over `[from, to]`.
   * Uses a server-side cursor so large reports (>10k rows) are not buffered.
   */
  async *streamReconciliationReport(
    merchantId: string,
    from: Date,
    to: Date,
  ): AsyncGenerator<ReconciliationRow> {
    const client = await this.pool.connect();
    const cursorName = `recon_${Date.now()}`;
    try {
      await client.query("BEGIN");
      await client.query(
        `DECLARE ${cursorName} CURSOR FOR
         (
           SELECT payment_id::text AS payment_id,
                  'payment'::text AS type,
                  amount::text AS amount_usdc,
                  COALESCE(fiat_amount, '') AS fiat_amount,
                  COALESCE(fiat_currency, '') AS fiat_currency,
                  COALESCE(status, '') AS status,
                  COALESCE(created_at::text, '') AS created_at,
                  COALESCE(settled_at::text, '') AS settled_at,
                  COALESCE(customer_ref, '') AS customer_ref,
                  COALESCE(tags, '') AS tags
           FROM payments
           WHERE merchant_id = $1
             AND created_at >= $2 AND created_at <= $3
         )
         UNION ALL
         (
           SELECT r.refund_id::text,
                  'refund'::text,
                  r.amount::text,
                  ''::text,
                  ''::text,
                  COALESCE(r.status, ''),
                  COALESCE(r.created_at::text, ''),
                  ''::text,
                  ''::text,
                  ''::text
           FROM refunds r
           JOIN payments p ON p.payment_id = r.payment_id
           WHERE p.merchant_id = $1
             AND r.created_at >= $2 AND r.created_at <= $3
         )
         UNION ALL
         (
           SELECT payment_id::text,
                  'settlement'::text,
                  amount::text,
                  COALESCE(fiat_amount, ''),
                  COALESCE(fiat_currency, ''),
                  COALESCE(status, ''),
                  COALESCE(created_at::text, ''),
                  COALESCE(settled_at::text, ''),
                  COALESCE(customer_ref, ''),
                  COALESCE(tags, '')
           FROM settlements
           WHERE merchant_id = $1
             AND COALESCE(settled_at, created_at) >= $2
             AND COALESCE(settled_at, created_at) <= $3
         )
         ORDER BY created_at ASC`,
        [merchantId, from, to],
      );

      const batchSize = 500;
      while (true) {
        const batch = await client.query<ReconciliationRow>(
          `FETCH ${batchSize} FROM ${cursorName}`,
        );
        if (batch.rows.length === 0) break;
        for (const row of batch.rows) {
          yield row;
        }
      }
      await client.query("COMMIT");
    } catch (err) {
      await client.query("ROLLBACK");
      throw err;
    } finally {
      client.release();
    }
  }

  async close(): Promise<void> {
    await this.pool.end();
  }
}
