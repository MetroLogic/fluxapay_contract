/**
 * FluxaPay Indexer — Drizzle ORM schema (Issue #842).
 *
 * Mirrors the tables created by indexer/migrations/*.sql so TypeScript
 * catches column renames / type mismatches at build time.
 */

import {
  boolean,
  bigint,
  index,
  integer,
  jsonb,
  pgTable,
  serial,
  text,
  timestamp,
  varchar,
  uuid,
} from "drizzle-orm/pg-core";

export const contractEvents = pgTable(
  "contract_events",
  {
    id: serial("id").primaryKey(),
    eventId: varchar("event_id", { length: 255 }).notNull().unique(),
    eventType: varchar("event_type", { length: 50 }).notNull(),
    ledger: integer("ledger").notNull(),
    txHash: varchar("tx_hash", { length: 255 }),
    timestamp: timestamp("timestamp").notNull(),
    data: jsonb("data"),
    contractId: varchar("contract_id", { length: 255 }),
    createdAt: timestamp("created_at").defaultNow(),
  },
  (t) => ({
    eventIdIdx: index("idx_event_id").on(t.eventId),
    ledgerIdx: index("idx_ledger").on(t.ledger),
    eventTypeIdx: index("idx_event_type").on(t.eventType),
  }),
);

export const payments = pgTable(
  "payments",
  {
    id: serial("id").primaryKey(),
    paymentId: varchar("payment_id", { length: 255 }).notNull().unique(),
    merchantId: varchar("merchant_id", { length: 255 }).notNull(),
    amount: bigint("amount", { mode: "bigint" }).notNull(),
    currency: varchar("currency", { length: 50 }),
    status: varchar("status", { length: 50 }),
    createdAt: timestamp("created_at"),
    updatedAt: timestamp("updated_at").defaultNow(),
    // Issue #840: reconciliation export columns
    fiatAmount: text("fiat_amount"),
    fiatCurrency: varchar("fiat_currency", { length: 16 }),
    settledAt: timestamp("settled_at"),
    customerRef: varchar("customer_ref", { length: 255 }),
    tags: text("tags"),
  },
  (t) => ({
    paymentIdIdx: index("idx_payment_id").on(t.paymentId),
    merchantIdIdx: index("idx_merchant_id").on(t.merchantId),
    statusIdx: index("idx_status").on(t.status),
  }),
);

export const refunds = pgTable(
  "refunds",
  {
    id: serial("id").primaryKey(),
    refundId: varchar("refund_id", { length: 255 }).notNull().unique(),
    paymentId: varchar("payment_id", { length: 255 }).notNull(),
    amount: bigint("amount", { mode: "bigint" }).notNull(),
    status: varchar("status", { length: 50 }),
    createdAt: timestamp("created_at"),
    updatedAt: timestamp("updated_at").defaultNow(),
  },
  (t) => ({
    refundIdIdx: index("idx_refund_id").on(t.refundId),
    paymentIdIdx: index("idx_refund_payment_id").on(t.paymentId),
  }),
);

export const disputes = pgTable(
  "disputes",
  {
    id: serial("id").primaryKey(),
    disputeId: varchar("dispute_id", { length: 255 }).notNull().unique(),
    paymentId: varchar("payment_id", { length: 255 }).notNull(),
    amount: bigint("amount", { mode: "bigint" }).notNull(),
    status: varchar("status", { length: 50 }),
    createdAt: timestamp("created_at"),
    updatedAt: timestamp("updated_at").defaultNow(),
    escalated: boolean("escalated").default(false),
    resolvedAt: timestamp("resolved_at"),
  },
);

export const merchants = pgTable("merchants", {
  id: serial("id").primaryKey(),
  merchantId: varchar("merchant_id", { length: 255 }).notNull().unique(),
  status: varchar("status", { length: 50 }),
  lastUpdate: timestamp("last_update"),
  createdAt: timestamp("created_at").defaultNow(),
});

export const streams = pgTable("streams", {
  id: serial("id").primaryKey(),
  streamId: varchar("stream_id", { length: 255 }).notNull().unique(),
  sender: varchar("sender", { length: 255 }),
  receiver: varchar("receiver", { length: 255 }),
  amount: bigint("amount", { mode: "bigint" }),
  status: varchar("status", { length: 50 }),
  createdAt: timestamp("created_at"),
  updatedAt: timestamp("updated_at").defaultNow(),
});

export const streamWithdrawals = pgTable("stream_withdrawals", {
  id: serial("id").primaryKey(),
  streamId: varchar("stream_id", { length: 255 }),
  recipient: varchar("recipient", { length: 255 }),
  amount: bigint("amount", { mode: "bigint" }),
  remainingDeposit: bigint("remaining_deposit", { mode: "bigint" }),
  createdAt: timestamp("created_at"),
});

export const subscriptions = pgTable("subscriptions", {
  id: serial("id").primaryKey(),
  subscriptionId: varchar("subscription_id", { length: 255 }).notNull().unique(),
  payer: varchar("payer", { length: 255 }),
  status: varchar("status", { length: 50 }),
  createdAt: timestamp("created_at"),
  updatedAt: timestamp("updated_at").defaultNow(),
});

export const invoices = pgTable("invoices", {
  id: serial("id").primaryKey(),
  invoiceId: varchar("invoice_id", { length: 255 }).notNull().unique(),
  merchantId: varchar("merchant_id", { length: 255 }),
  totalAmount: bigint("total_amount", { mode: "bigint" }),
  status: varchar("status", { length: 50 }),
  createdAt: timestamp("created_at"),
  updatedAt: timestamp("updated_at").defaultNow(),
});

export const disputeBonds = pgTable("dispute_bonds", {
  id: serial("id").primaryKey(),
  disputeId: varchar("dispute_id", { length: 255 }),
  recipient: varchar("recipient", { length: 255 }),
  amount: bigint("amount", { mode: "bigint" }),
  status: varchar("status", { length: 50 }),
  createdAt: timestamp("created_at"),
});

export const deadLetterEvents = pgTable("dead_letter_events", {
  id: serial("id").primaryKey(),
  eventId: varchar("event_id", { length: 255 }).notNull().unique(),
  rawData: jsonb("raw_data").notNull(),
  error: text("error").notNull(),
  createdAt: timestamp("created_at").defaultNow(),
  retryCount: integer("retry_count").default(0),
});

/**
 * Webhook Dead-Letter Queue (Issue #961).
 *
 * One row per failed webhook delivery. The retry daemon picks up rows where
 * status = 'pending' AND next_retry_at <= NOW() and re-delivers them with
 * exponential backoff. After 7 attempts, status transitions to
 * 'permanently_failed' and the merchant is notified.
 */
export const webhookDlq = pgTable(
  "webhook_dlq",
  {
    id: uuid("id").primaryKey().defaultRandom(),
    endpointId: text("endpoint_id").notNull(),
    merchantId: text("merchant_id").notNull(),
    eventType: text("event_type").notNull(),
    paymentId: text("payment_id"),
    /** The full signed envelope — stored so retries send an identical payload. */
    payload: jsonb("payload").notNull(),
    retryCount: integer("retry_count").notNull().default(0),
    nextRetryAt: timestamp("next_retry_at", { withTimezone: true }),
    /** 'pending' | 'permanently_failed' */
    status: text("status").notNull().default("pending"),
    lastError: text("last_error"),
    createdAt: timestamp("created_at", { withTimezone: true }).defaultNow().notNull(),
    updatedAt: timestamp("updated_at", { withTimezone: true }).defaultNow().notNull(),
  },
  (t) => ({
    dueIdx: index("idx_webhook_dlq_due").on(t.nextRetryAt, t.status),
    merchantIdx: index("idx_webhook_dlq_merchant").on(t.merchantId, t.createdAt),
    endpointIdx: index("idx_webhook_dlq_endpoint").on(t.endpointId, t.createdAt),
  }),
);

export const indexerCheckpoint = pgTable("indexer_checkpoint", {
  id: serial("id").primaryKey(),
  contractId: varchar("contract_id", { length: 255 }).notNull().unique(),
  lastProcessedLedger: integer("last_processed_ledger").notNull().default(0),
  lastUpdate: timestamp("last_update").defaultNow(),
});

/** Settlements table for reconciliation (Issue #840). */
export const settlements = pgTable("settlements", {
  id: serial("id").primaryKey(),
  paymentId: varchar("payment_id", { length: 255 }).notNull(),
  merchantId: varchar("merchant_id", { length: 255 }).notNull(),
  amount: bigint("amount", { mode: "bigint" }).notNull(),
  status: varchar("status", { length: 50 }),
  createdAt: timestamp("created_at"),
  settledAt: timestamp("settled_at"),
  fiatAmount: text("fiat_amount"),
  fiatCurrency: varchar("fiat_currency", { length: 16 }),
  customerRef: varchar("customer_ref", { length: 255 }),
  tags: text("tags"),
});
