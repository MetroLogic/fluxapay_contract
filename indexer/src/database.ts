/**
 * FluxaPay Indexer Database Module
 * Handles PostgreSQL connections, event persistence, dispute status updates,
 * dead-letter queue operations, and REST API queries.
 *
 * Issue #842: all queries use Drizzle ORM for compile-time schema safety.
 */

import { Pool } from "pg";
import { SettlementRecord, SettlementStatus, SettlementStore } from "./settlement";
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
  feeCollectionHistory,
  merchantInvoiceSequences,
  paymentLinks,
  subscriptionPlans,
  subscriptionMetricEvents,
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

export interface MerchantOnboardingState {
  registeredAt: Date | string;
  kycSubmittedAt: Date | string | null;
}

export interface ReconciliationData {
  payments: Record<string, unknown>[];
  settlements: Record<string, unknown>[];
  refunds: Record<string, unknown>[];
  disputeAdjustments: Record<string, unknown>[];
  fxRates: Record<string, unknown>[];
}

export interface PaymentLinkAnalytics {
  link_id: string;
  views: number;
  attempts: number;
  conversions: number;
  total_revenue_stroops: string;
  last_used_at: Date | null;
}

export interface SubscriptionMetrics {
  mrr_stroops: string;
  active_subscribers: number;
  new_subscribers: number;
  churned_subscribers: number;
  churn_rate: number | null;
  trial_conversions: number;
  trial_conversion_rate: number | null;
  past_due_subscribers: number;
  dunning_recoveries: number;
  dunning_recovery_rate: number | null;
}

export class Database implements SettlementStore {
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
      const [eventType, eventSubtype] = event.topic;
      const table = this.getTableName(eventType, eventSubtype);
      const inserted = await tx
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
        .onConflictDoNothing()
        .returning({ id: contractEvents.id });

      if (inserted.length === 0) {
        console.log(`Event ${event.id} already processed, skipping...`);
        return false;
      }

      await this.storeTypedEventDrizzle(table, event, tx);

      // Issue #839: keep an in-memory FX rate cache for /v1/fx/convert.
      this.updateFxRateCache(event);

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
        // Issue #1037: PAYMENT/RETRY_CREATED is emitted by the contract and
        // carries (new_payment_id, original_payment_id), but the Drizzle
        // `payments` schema has no `retryOfPaymentId` column yet to persist
        // it into — the on-chain link (PaymentCharge.retry_of_payment_id,
        // queryable via get_payment) is this issue's actual requirement and
        // is unaffected; indexer-side reconciliation would need its own
        // follow-up migration + schema column, left as a known gap rather
        // than rushed in here.
        await tx
          .insert(payments)
          .values({
            paymentId: String(value.payment_id),
            merchantId: String(value.merchant_id),
            amount: BigInt(String(value.amount ?? 0)),
            currency: value.currency != null ? String(value.currency) : null,
            status: event.topic[1],
            paymentLinkId:
              value.payment_link_id != null ? String(value.payment_link_id) : null,
            createdAt: ts,
            fiatAmount: value.fiat_amount != null ? String(value.fiat_amount) : null,
            fiatCurrency: value.fiat_currency != null ? String(value.fiat_currency) : null,
            customerRef: value.customer_ref != null ? String(value.customer_ref) : null,
            tags: value.tags != null ? String(value.tags) : null,
            settledAt:
              event.topic[1] === "SETTLED" ? ts : null,
            // Issue #1035: sub-account ID from an M-address payer.
            payerMuxedId:
              value.payer_muxed_id != null ? BigInt(String(value.payer_muxed_id)) : null,
          })
          .onConflictDoUpdate({
            target: payments.paymentId,
            set: {
              status: event.topic[1],
              paymentLinkId:
                value.payment_link_id != null
                  ? String(value.payment_link_id)
                  : undefined,
              updatedAt: new Date(),
              settledAt:
                event.topic[1] === "SETTLED" ? ts : undefined,
              payerMuxedId:
                value.payer_muxed_id != null
                  ? BigInt(String(value.payer_muxed_id))
                  : undefined,
            },
          });
        break;
      }

      case "payment_links": {
        const [eventType, eventSubtype] = event.topic;
        if (eventType === "LINK" && eventSubtype === "CREATED") {
          await tx
            .insert(paymentLinks)
            .values({
              linkId: String(value.link_id),
              merchantId: String(value.merchant_id),
              createdAt: ts,
            })
            .onConflictDoUpdate({
              target: paymentLinks.linkId,
              set: {
                merchantId: String(value.merchant_id),
              },
            });
          break;
        }

        if (eventType === "LINK" && eventSubtype === "USED") {
          const linkId = String(value.link_id);
          const paymentId = String(value.payment_id);
          const [link] = await tx
            .select({ merchantId: paymentLinks.merchantId })
            .from(paymentLinks)
            .where(eq(paymentLinks.linkId, linkId))
            .limit(1);
          if (!link) {
            throw new Error(`Payment link ${linkId} has no indexed owner`);
          }

          await tx
            .insert(payments)
            .values({
              paymentId,
              merchantId: link.merchantId,
              amount: BigInt(String(value.amount ?? 0)),
              currency: "USDC",
              status: "PENDING",
              paymentLinkId: linkId,
              createdAt: ts,
            })
            .onConflictDoUpdate({
              target: payments.paymentId,
              set: { paymentLinkId: linkId },
            });
        }
        break;
      }

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
        if (subtype === "ADJUSTMENT") {
          await client.query(
            `INSERT INTO dispute_adjustments (dispute_id, payment_id, refund_id, amount, created_at)
             VALUES ($1, $2, $3, $4, to_timestamp($5))
             ON CONFLICT (dispute_id) DO UPDATE SET
               payment_id = EXCLUDED.payment_id,
               refund_id = EXCLUDED.refund_id,
               amount = EXCLUDED.amount,
               created_at = EXCLUDED.created_at`,
            [value.dispute_id, value.payment_id, value.refund_id, value.amount, event.timestamp]
          );
        } else if (subtype === "RESOLVED") {
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
              disputer: value.disputer != null ? String(value.disputer) : null,
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

      case "fx_rate":
        await client.query(
          `INSERT INTO fx_rate_cache (pair, rate, decimals, updated_at)
           VALUES ($1, $2, $3, to_timestamp($4))
           ON CONFLICT (pair) DO UPDATE SET
             rate = EXCLUDED.rate,
             decimals = EXCLUDED.decimals,
             updated_at = EXCLUDED.updated_at`,
          [value.pair, value.rate, value.decimals, event.timestamp]
        );
        break;

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
          memo: value.memo != null ? String(value.memo) : null,
        });
        break;

      case "subscriptions":
        if (event.topic[1] === "CHARGED") {
          const subscriptionId = String(value.subscription_id);
          const merchantId = String(value.merchant_id);
          const cycleNumber = Number(value.total_payments ?? 1);
          const existingInvoice = await tx
            .select({ invoiceId: invoices.invoiceId })
            .from(invoices)
            .where(
              and(
                eq(invoices.subscriptionId, subscriptionId),
                eq(invoices.cycleNumber, cycleNumber),
              ),
            )
            .limit(1);

          if (existingInvoice.length === 0) {
            const [sequence] = await tx
              .insert(merchantInvoiceSequences)
              .values({ merchantId, nextNumber: 1 })
              .onConflictDoUpdate({
                target: merchantInvoiceSequences.merchantId,
                set: { nextNumber: sql`${merchantInvoiceSequences.nextNumber} + 1` },
              })
              .returning({ number: merchantInvoiceSequences.nextNumber });
            const invoiceNumber = `INV-${String(sequence.number).padStart(5, "0")}`;
            await tx.insert(invoices).values({
              invoiceId: `${subscriptionId}-${cycleNumber}`,
              invoiceNumber,
              merchantId,
              subscriptionId,
              cycleNumber,
              subscriber: value.payer != null ? String(value.payer) : null,
              subscriberEmail: value.subscriber_email != null
                ? String(value.subscriber_email)
                : null,
              totalAmount: BigInt(String(value.amount ?? 0)),
              status: "Paid",
              createdAt: ts,
              paidAt: ts,
              billingPeriodStart: ts,
              billingPeriodEnd: ts,
            });
          }
        }
        await tx
          .insert(subscriptions)
          .values({
            subscriptionId: String(value.subscription_id),
            payer: value.payer != null ? String(value.payer) : null,
            status: value.status != null
              ? String(value.status)
              : event.topic[1] === "CHARGED"
                ? "Active"
                : null,
            createdAt: ts,
          })
          .onConflictDoUpdate({
            target: subscriptions.subscriptionId,
            set: {
              status: value.status != null
                ? String(value.status)
                : event.topic[1] === "CHARGED"
                  ? "Active"
                  : null,
            },
          });
        if (event.topic[1] === "PLAN_CREATED") {
          await tx
            .insert(subscriptionPlans)
            .values({
              planId: String(value.plan_id),
              merchantId: String(value.merchant_id),
              amount: BigInt(String(value.amount)),
              intervalSecs: BigInt(String(value.interval_secs)),
              createdAt: ts,
            })
            .onConflictDoUpdate({
              target: subscriptionPlans.planId,
              set: {
                merchantId: String(value.merchant_id),
                amount: BigInt(String(value.amount)),
                intervalSecs: BigInt(String(value.interval_secs)),
              },
            });
          break;
        }

        if (event.topic[1] === "PLAN_DEACTIVATED") break;

        const subscriptionId = String(value.subscription_id);
        const subscriptionEvent = event.topic[1];
        const statusByEvent: Record<string, string> = {
          CREATED: "ACTIVE",
          ACTIVE: "ACTIVE",
          CHARGED: "ACTIVE",
          RESUMED: "ACTIVE",
          CANCELLED: "CANCELLED",
          EXPIRED: "EXPIRED",
          COMPLETED: "COMPLETED",
          CHARGE_FAILED: "PAST_DUE",
          DUNNING_TRIGGERED: "PAST_DUE",
          PAUSED: "PAUSED",
        };
        const trackedEvents = new Set([
          "CREATED",
          ...Object.keys(statusByEvent),
          "TRIAL_STARTED",
          "TRIAL_ENDED",
        ]);
        if (!trackedEvents.has(subscriptionEvent)) break;

        let merchantId: string | null | undefined;
        let planId: string | null | undefined;
        if (subscriptionEvent === "CREATED") {
          planId = String(value.plan_id);
          const [plan] = await tx
            .select({
              merchantId: subscriptionPlans.merchantId,
            })
            .from(subscriptionPlans)
            .where(eq(subscriptionPlans.planId, planId))
            .limit(1);
          if (!plan) {
            throw new Error(`Subscription plan ${planId} has no indexed owner`);
          }
          merchantId = plan.merchantId;

          await tx
            .insert(subscriptions)
            .values({
              subscriptionId,
              merchantId,
              planId,
              payer: value.payer != null ? String(value.payer) : null,
              status: "ACTIVE",
              createdAt: ts,
            })
            .onConflictDoUpdate({
              target: subscriptions.subscriptionId,
              set: {
                merchantId,
                planId,
                status: "ACTIVE",
              },
            });
        } else {
          const [subscription] = await tx
            .select({ merchantId: subscriptions.merchantId, planId: subscriptions.planId })
            .from(subscriptions)
            .where(eq(subscriptions.subscriptionId, subscriptionId))
            .limit(1);

          if (!subscription?.merchantId) {
            throw new Error(`Subscription ${subscriptionId} has no indexed merchant`);
          }
          merchantId = subscription.merchantId;
          planId = subscription.planId;

          const state = statusByEvent[subscriptionEvent];
          if (state) {
            await tx
              .update(subscriptions)
              .set({ status: state, updatedAt: ts })
              .where(eq(subscriptions.subscriptionId, subscriptionId));
          }
        }

        if (!merchantId) {
          throw new Error(`Subscription ${subscriptionId} has no indexed merchant`);
        }
        await tx.insert(subscriptionMetricEvents).values({
          eventId: event.id,
          subscriptionId,
          merchantId,
          planId: planId ?? null,
          eventType: subscriptionEvent,
          occurredAt: ts,
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
    if (eventType === "PAYMENT" && eventSubtype === "FEE_TOTAL") {
      return "payment_fee";
    }
    if (eventType === "FX_RATE" && eventSubtype === "UPDATED") {
      return "fx_rate";
    }
    if (eventType === "DISPUTE" && (eventSubtype === "BOND_RETURNED" || eventSubtype === "BOND_FORFEITED")) {
      return "dispute_bonds";
    }
    if (eventType === "SETTLEMENT" || eventSubtype === "SETTLED") {
      return eventType === "SETTLEMENT" ? "settlements" : "payments";
    }

    const tableMap: Record<string, string> = {
      PAYMENT: "payments",
      LINK: "payment_links",
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
   * Fetch the current settlement record for a payment, if the SEP-6/SEP-24
   * offramp has been attempted for it. Used by `SettlementService` to make
   * `settle()` idempotent against re-delivered `PAYMENT/ANCHOR_WITHDRAW`
   * events.
   */
  async getSettlement(paymentId: string): Promise<SettlementRecord | null> {
    const client = await this.pool.connect();
    try {
      const result = await client.query(
        `SELECT payment_id, merchant_id, anchor_domain, anchor_txn_id, amount, currency, status
         FROM settlements WHERE payment_id = $1`,
        [paymentId]
      );
      if (result.rows.length === 0) return null;
      const row = result.rows[0];
      return {
        paymentId: row.payment_id,
        merchantId: row.merchant_id,
        anchorDomain: row.anchor_domain,
        anchorTxnId: row.anchor_txn_id,
        amount: Number(row.amount),
        currency: row.currency,
        status: row.status as SettlementStatus,
      };
    } finally {
      client.release();
    }
  }

  /**
   * Insert or update the settlement row for a payment, keyed by
   * `payment_id`. Called at each step of the SEP-6 withdrawal flow
   * (initiated -> pending -> fiat_sent/failed).
   */
  async upsertSettlement(record: SettlementRecord): Promise<void> {
    const client = await this.pool.connect();
    try {
      await client.query(
        `INSERT INTO settlements
           (payment_id, merchant_id, anchor_domain, anchor_txn_id, amount, currency, status, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, CURRENT_TIMESTAMP)
         ON CONFLICT (payment_id) DO UPDATE SET
           anchor_domain = $3,
           anchor_txn_id = COALESCE($4, settlements.anchor_txn_id),
           amount = $5,
           currency = $6,
           status = $7,
           updated_at = CURRENT_TIMESTAMP`,
        [
          record.paymentId,
          record.merchantId,
          record.anchorDomain,
          record.anchorTxnId,
          record.amount,
          record.currency,
          record.status,
        ]
      );
    } finally {
      client.release();
    }
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
        // Issue #1035: sub-account ID from an M-address payer, for reconciliation.
        payer_muxed_id: payments.payerMuxedId,
      })
      .from(payments)
      .where(eq(payments.paymentId, paymentId))
      .limit(1);
    return rows.length > 0 ? rows[0] : null;
  }

  // Issue #1039: resolve a dispute's payment/merchant so live dispute events
  // that only carry `dispute_id` (e.g. `DISPUTE/VOTE_CAST`,
  // `DISPUTE/AUTO_RESOLVED`) can still be routed to the right merchant's SSE
  // stream and webhook endpoints.
  async getDisputeById(disputeId: string): Promise<unknown | null> {
    const rows = await this.db
      .select({
        dispute_id: disputes.disputeId,
        payment_id: disputes.paymentId,
        status: disputes.status,
      })
      .from(disputes)
      .where(eq(disputes.disputeId, disputeId))
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

  async recordPaymentLinkView(linkId: string): Promise<boolean> {
    const result = await this.pool.query(
      `INSERT INTO payment_link_views (link_id)
       SELECT link_id FROM payment_links WHERE link_id = $1
       RETURNING id`,
      [linkId],
    );
    return (result.rowCount ?? 0) > 0;
  }

  async getPaymentLinkAnalytics(
    linkId: string,
    merchantId: string,
  ): Promise<PaymentLinkAnalytics | null> {
    const { rows } = await this.pool.query(
      `SELECT l.link_id,
              (SELECT COUNT(*) FROM payment_link_views v
                WHERE v.link_id = l.link_id)::int AS views,
              COUNT(p.payment_id)::int AS attempts,
              COUNT(p.payment_id) FILTER (
                WHERE UPPER(p.status) IN ('CONFIRMED', 'SETTLED')
              )::int AS conversions,
              COALESCE(SUM(p.amount) FILTER (
                WHERE UPPER(p.status) IN ('CONFIRMED', 'SETTLED')
              ), 0)::text AS total_revenue_stroops,
              MAX(p.created_at) AT TIME ZONE 'UTC' AS last_used_at
         FROM payment_links l
         LEFT JOIN payments p ON p.payment_link_id = l.link_id
        WHERE l.link_id = $1 AND l.merchant_id = $2
        GROUP BY l.link_id`,
      [linkId, merchantId],
    );
    return (rows[0] as PaymentLinkAnalytics | undefined) ?? null;
  }

  async getPaymentLinksForMerchant(merchantId: string): Promise<
    Array<PaymentLinkAnalytics & { daily_conversions: number[] }>
  > {
    const { rows } = await this.pool.query(
      `SELECT l.link_id,
              (SELECT COUNT(*) FROM payment_link_views v
                WHERE v.link_id = l.link_id)::int AS views,
              COUNT(p.payment_id)::int AS attempts,
              COUNT(p.payment_id) FILTER (
                WHERE UPPER(p.status) IN ('CONFIRMED', 'SETTLED')
              )::int AS conversions,
              COALESCE(SUM(p.amount) FILTER (
                WHERE UPPER(p.status) IN ('CONFIRMED', 'SETTLED')
              ), 0)::text AS total_revenue_stroops,
              MAX(p.created_at) AT TIME ZONE 'UTC' AS last_used_at
         FROM payment_links l
         LEFT JOIN payments p ON p.payment_link_id = l.link_id
        WHERE l.merchant_id = $1
        GROUP BY l.link_id
        ORDER BY l.created_at DESC`,
      [merchantId],
    );
    if (rows.length === 0) return [];

    const { rows: dailyRows } = await this.pool.query(
      `SELECT p.payment_link_id AS link_id,
              p.created_at::date::text AS day,
              COUNT(*)::int AS conversions
         FROM payments p
         JOIN payment_links l ON l.link_id = p.payment_link_id
        WHERE l.merchant_id = $1
          AND p.created_at >= ((NOW() AT TIME ZONE 'UTC')::date - 6)
          AND UPPER(p.status) IN ('CONFIRMED', 'SETTLED')
        GROUP BY p.payment_link_id, p.created_at::date`,
      [merchantId],
    );

    const dailyByLink = new Map<string, Map<string, number>>();
    for (const row of dailyRows as Array<{ link_id: string; day: string; conversions: number }>) {
      const byDay = dailyByLink.get(row.link_id) ?? new Map<string, number>();
      byDay.set(row.day, Number(row.conversions));
      dailyByLink.set(row.link_id, byDay);
    }
    const days = Array.from({ length: 7 }, (_, index) => {
      const day = new Date();
      day.setUTCHours(0, 0, 0, 0);
      day.setUTCDate(day.getUTCDate() - (6 - index));
      return day.toISOString().slice(0, 10);
    });

    return (rows as PaymentLinkAnalytics[]).map((row) => ({
      ...row,
      views: Number(row.views),
      attempts: Number(row.attempts),
      conversions: Number(row.conversions),
      total_revenue_stroops: String(row.total_revenue_stroops),
      daily_conversions: days.map((day) => dailyByLink.get(row.link_id)?.get(day) ?? 0),
    }));
  }

  async getSubscriptionMetrics(
    merchantId: string,
    periodStart: Date,
    periodEnd: Date,
  ): Promise<SubscriptionMetrics> {
    const { rows } = await this.pool.query(
      `WITH period_events AS (
         SELECT subscription_id, event_type, occurred_at
           FROM subscription_metric_events
          WHERE merchant_id = $1
            AND occurred_at >= $2
            AND occurred_at < $3
       ),
       latest_state_at_start AS (
         SELECT DISTINCT ON (subscription_id) subscription_id, event_type
           FROM subscription_metric_events
          WHERE merchant_id = $1
            AND occurred_at < $2
            AND event_type IN (
              'CREATED', 'CHARGED', 'RESUMED', 'CANCELLED', 'EXPIRED',
              'COMPLETED', 'CHARGE_FAILED', 'DUNNING_TRIGGERED', 'PAUSED'
            )
          ORDER BY subscription_id, occurred_at DESC, event_id DESC
       ),
       active_at_start AS (
         SELECT COUNT(*)::int AS count
           FROM latest_state_at_start
          WHERE event_type IN ('CREATED', 'CHARGED', 'RESUMED')
       ),
       current_metrics AS (
         SELECT
           COALESCE(SUM(p.amount) FILTER (WHERE UPPER(s.status) = 'ACTIVE'), 0)::text AS mrr_stroops,
           COUNT(*) FILTER (WHERE UPPER(s.status) = 'ACTIVE')::int AS active_subscribers,
           COUNT(*) FILTER (WHERE UPPER(s.status) IN ('PAST_DUE', 'PASTDUE'))::int AS past_due_subscribers
           FROM subscriptions s
           JOIN subscription_plans p ON p.plan_id = s.plan_id
          WHERE s.merchant_id = $1
       ),
       period_counts AS (
         SELECT
           COUNT(DISTINCT subscription_id) FILTER (WHERE event_type = 'CREATED')::int AS new_subscribers,
           COUNT(DISTINCT subscription_id) FILTER (
             WHERE event_type IN ('CANCELLED', 'EXPIRED', 'COMPLETED')
           )::int AS churned_subscribers,
           COUNT(DISTINCT subscription_id) FILTER (WHERE event_type = 'TRIAL_ENDED')::int AS trial_ends,
           COUNT(DISTINCT subscription_id) FILTER (WHERE event_type = 'DUNNING_TRIGGERED')::int AS dunning_triggers
           FROM period_events
       ),
       trial_conversions AS (
         SELECT COUNT(DISTINCT t.subscription_id)::int AS count
           FROM period_events t
          WHERE t.event_type = 'TRIAL_ENDED'
            AND EXISTS (
              SELECT 1 FROM period_events c
               WHERE c.subscription_id = t.subscription_id
                 AND c.event_type = 'CHARGED'
                 AND c.occurred_at >= t.occurred_at
            )
       ),
       dunning_recoveries AS (
         SELECT COUNT(DISTINCT d.subscription_id)::int AS count
           FROM period_events d
          WHERE d.event_type = 'DUNNING_TRIGGERED'
            AND EXISTS (
              SELECT 1 FROM period_events c
               WHERE c.subscription_id = d.subscription_id
                 AND c.event_type = 'CHARGED'
                 AND c.occurred_at >= d.occurred_at
            )
       )
       SELECT current_metrics.mrr_stroops,
              current_metrics.active_subscribers,
              current_metrics.past_due_subscribers,
              period_counts.new_subscribers,
              period_counts.churned_subscribers,
              CASE WHEN active_at_start.count = 0 THEN NULL
                   ELSE period_counts.churned_subscribers::decimal / active_at_start.count
              END AS churn_rate,
              trial_conversions.count AS trial_conversions,
              CASE WHEN period_counts.trial_ends = 0 THEN NULL
                   ELSE trial_conversions.count::decimal / period_counts.trial_ends
              END AS trial_conversion_rate,
              dunning_recoveries.count AS dunning_recoveries,
              CASE WHEN period_counts.dunning_triggers = 0 THEN NULL
                   ELSE dunning_recoveries.count::decimal / period_counts.dunning_triggers
              END AS dunning_recovery_rate
         FROM current_metrics
         CROSS JOIN active_at_start
         CROSS JOIN period_counts
         CROSS JOIN trial_conversions
         CROSS JOIN dunning_recoveries`,
      [merchantId, periodStart, periodEnd],
    );
    const row = rows[0];
    if (!row) {
      throw new Error("Subscription metrics query returned no result");
    }
    return {
      mrr_stroops: String(row.mrr_stroops),
      active_subscribers: Number(row.active_subscribers),
      new_subscribers: Number(row.new_subscribers),
      churned_subscribers: Number(row.churned_subscribers),
      churn_rate: row.churn_rate == null ? null : Number(row.churn_rate),
      trial_conversions: Number(row.trial_conversions),
      trial_conversion_rate:
        row.trial_conversion_rate == null ? null : Number(row.trial_conversion_rate),
      past_due_subscribers: Number(row.past_due_subscribers),
      dunning_recoveries: Number(row.dunning_recoveries),
      dunning_recovery_rate:
        row.dunning_recovery_rate == null ? null : Number(row.dunning_recovery_rate),
    };
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
    // Issue #1035: filter to payments routed from a specific muxed sub-account.
    payerMuxedId?: bigint;
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
    if (opts.payerMuxedId !== undefined) {
      conditions.push(eq(payments.payerMuxedId, opts.payerMuxedId));
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
        payer_muxed_id: payments.payerMuxedId,
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
        disputer: disputes.disputer,
        merchant_id: payments.merchantId,
        amount: disputes.amount,
        status: disputes.status,
        evidence: disputes.evidence,
        escalated: disputes.escalated,
        resolved_at: disputes.resolvedAt,
        created_at: disputes.createdAt,
      })
      .from(disputes)
      .innerJoin(payments, eq(disputes.paymentId, payments.paymentId))
      .where(and(...conditions))
      .orderBy(desc(disputes.createdAt));
  }

  async appendDisputeEvidence(disputeId: string, item: Record<string, unknown>) {
    const client = await this.pool.connect();
    try {
      await client.query("BEGIN");
      const current = await client.query(
        "SELECT evidence FROM disputes WHERE dispute_id = $1 FOR UPDATE",
        [disputeId],
      );
      if (current.rowCount !== 1) {
        await client.query("ROLLBACK");
        return null;
      }
      const updated = await client.query(
        `UPDATE disputes
         SET evidence = COALESCE(evidence, '[]'::jsonb) || $2::jsonb, updated_at = NOW()
         WHERE dispute_id = $1
         RETURNING evidence`,
        [disputeId, JSON.stringify([item])],
      );
      await client.query("COMMIT");
      return updated.rows[0].evidence as Array<Record<string, unknown>>;
    } catch (error) {
      await client.query("ROLLBACK");
      throw error;
    } finally {
      client.release();
    }
  }

  async getDisputeEvidence(disputeId: string) {
    const rows = await this.db
      .select({ evidence: disputes.evidence })
      .from(disputes)
      .where(eq(disputes.disputeId, disputeId))
      .limit(1);
    return rows[0]?.evidence ?? null;
  }

  async getDisputeEvidenceContext(disputeId: string) {
    const rows = await this.db
      .select({
        disputer: disputes.disputer,
        merchantId: payments.merchantId,
        evidence: disputes.evidence,
      })
      .from(disputes)
      .innerJoin(payments, eq(disputes.paymentId, payments.paymentId))
      .where(eq(disputes.disputeId, disputeId))
      .limit(1);
    return rows[0] ?? null;
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

  // ------------------------------------------------------------------ Issue #1067: Invoice helpers

  async getInvoices(params: {
    merchantId?: string;
    status?: string;
    limit: number;
    offset: number;
  }) {
    const conditions = [];
    if (params.merchantId) {
      conditions.push(eq(invoices.merchantId, params.merchantId));
    }
    if (params.status) {
      conditions.push(eq(invoices.status, params.status));
    }

    const rows = await this.db
      .select()
      .from(invoices)
      .where(conditions.length > 0 ? and(...conditions) : undefined)
      .orderBy(desc(invoices.createdAt))
      .limit(params.limit)
      .offset(params.offset);

    return { invoices: rows, limit: params.limit, offset: params.offset };
  }

  async getInvoiceById(invoiceId: string) {
    const rows = await this.db
      .select()
      .from(invoices)
      .where(eq(invoices.invoiceId, invoiceId))
      .limit(1);
    return rows[0] ?? null;
  }

  async getInvoicesBySubscription(subscriptionId: string) {
    return this.db
      .select()
      .from(invoices)
      .where(eq(invoices.subscriptionId, subscriptionId))
      .orderBy(asc(invoices.cycleNumber));
  }

  /**
   * Soft-delete: transitions a Pending or Overdue invoice to Voided.
   * Returns the updated row, or null if not found / already Paid / already Voided.
   */
  async voidInvoice(invoiceId: string, merchantId: string) {
    const existing = await this.getInvoiceById(invoiceId);
    if (!existing) return null;
    if (existing.merchantId !== merchantId) return null;
    if (existing.status === "Voided" || existing.status === "Paid") return null;

    const now = new Date();
    const updated = await this.db
      .update(invoices)
      .set({ status: "Voided", voidedAt: now, updatedAt: now })
      .where(eq(invoices.invoiceId, invoiceId))
      .returning();
    return updated[0] ?? null;
  }

  // ------------------------------------------------------------------ Issue #1066: Fee collection history

  async getFeeCollectionHistory(params: {
    offset: number;
    limit: number;
    token?: string;
  }) {
    const conditions = [];
    if (params.token) {
      conditions.push(eq(feeCollectionHistory.token, params.token));
    }

    const rows = await this.db
      .select()
      .from(feeCollectionHistory)
      .where(conditions.length > 0 ? and(...conditions) : undefined)
      .orderBy(desc(feeCollectionHistory.collectedAt))
      .limit(params.limit)
      .offset(params.offset);

    return { fees: rows, limit: params.limit, offset: params.offset };
  }

  // ------------------------------------------------------------------ Issue #1064: Merchant leaderboard

  async getTopMerchants(limit: number) {
    const rows = await this.db
      .select({
        merchantId: merchants.merchantId,
        status: merchants.status,
        lastUpdate: merchants.lastUpdate,
      })
      .from(merchants)
      .where(eq(merchants.status, "active"))
      .limit(limit);

    // Enrich with payment volume from the payments table.
    const enriched = await Promise.all(
      rows.map(async (m) => {
        const volRows = await this.db
          .select({ total: sql<string>`COALESCE(SUM(amount::numeric), 0)`, cnt: sql<string>`COUNT(*)` })
          .from(payments)
          .where(eq(payments.merchantId, m.merchantId));
        const vol = volRows[0];
        return {
          merchant_id: m.merchantId,
          score: vol?.total ?? "0",
          confirmed_payment_count: Number(vol?.cnt ?? 0),
          cumulative_volume: vol?.total ?? "0",
        };
      }),
    );

    // Sort by score descending.
    enriched.sort((a, b) => {
      const diff = BigInt(b.score) - BigInt(a.score);
      return diff > 0n ? 1 : diff < 0n ? -1 : 0;
    });

    return enriched.slice(0, limit);
  }

  async close(): Promise<void> {
    await this.pool.end();
  }
}
