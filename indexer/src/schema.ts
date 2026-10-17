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
  uniqueIndex,
  varchar,
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
    paymentLinkId: varchar("payment_link_id", { length: 255 }),
    createdAt: timestamp("created_at"),
    updatedAt: timestamp("updated_at").defaultNow(),
    // Issue #840: reconciliation export columns
    fiatAmount: text("fiat_amount"),
    fiatCurrency: varchar("fiat_currency", { length: 16 }),
    settledAt: timestamp("settled_at"),
    customerRef: varchar("customer_ref", { length: 255 }),
    tags: text("tags"),
    // Issue #1035: sub-account ID extracted from the payer's M-address, for
    // reconciliation and `GET /v1/transactions?payer_muxed_id=` filtering.
    payerMuxedId: bigint("payer_muxed_id", { mode: "bigint" }),
  },
  (t) => ({
    paymentIdIdx: index("idx_payment_id").on(t.paymentId),
    merchantIdIdx: index("idx_merchant_id").on(t.merchantId),
    statusIdx: index("idx_status").on(t.status),
    paymentLinkIdIdx: index("idx_payments_payment_link_id").on(t.paymentLinkId),
    payerMuxedIdIdx: index("idx_payments_payer_muxed_id").on(t.payerMuxedId),
  }),
);

export const paymentLinks = pgTable(
  "payment_links",
  {
    linkId: varchar("link_id", { length: 255 }).primaryKey(),
    merchantId: varchar("merchant_id", { length: 255 }).notNull(),
    createdAt: timestamp("created_at", { withTimezone: true }).notNull(),
  },
  (t) => ({
    merchantCreatedIdx: index("idx_payment_links_merchant").on(t.merchantId, t.createdAt),
  }),
);

export const paymentLinkViews = pgTable(
  "payment_link_views",
  {
    id: serial("id").primaryKey(),
    linkId: varchar("link_id", { length: 255 }).notNull(),
    viewedAt: timestamp("viewed_at", { withTimezone: true }).notNull().defaultNow(),
  },
  (t) => ({
    linkViewedAtIdx: index("idx_payment_link_views_link_time").on(t.linkId, t.viewedAt),
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
    disputer: varchar("disputer", { length: 255 }),
    status: varchar("status", { length: 50 }),
    createdAt: timestamp("created_at"),
    updatedAt: timestamp("updated_at").defaultNow(),
    escalated: boolean("escalated").default(false),
    resolvedAt: timestamp("resolved_at"),
    evidence: jsonb("evidence").notNull().default([]),
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
  memo: varchar("memo", { length: 255 }),
  createdAt: timestamp("created_at"),
});

export const subscriptions = pgTable("subscriptions", {
  id: serial("id").primaryKey(),
  subscriptionId: varchar("subscription_id", { length: 255 }).notNull().unique(),
  merchantId: varchar("merchant_id", { length: 255 }),
  planId: varchar("plan_id", { length: 255 }),
  payer: varchar("payer", { length: 255 }),
  status: varchar("status", { length: 50 }),
  createdAt: timestamp("created_at"),
  updatedAt: timestamp("updated_at").defaultNow(),
});

export const subscriptionPlans = pgTable(
  "subscription_plans",
  {
    planId: varchar("plan_id", { length: 255 }).primaryKey(),
    merchantId: varchar("merchant_id", { length: 255 }).notNull(),
    amount: bigint("amount", { mode: "bigint" }).notNull(),
    intervalSecs: bigint("interval_secs", { mode: "bigint" }).notNull(),
    createdAt: timestamp("created_at", { withTimezone: true }).notNull(),
  },
  (t) => ({
    merchantIdIdx: index("idx_subscription_plans_merchant").on(t.merchantId),
  }),
);

export const subscriptionMetricEvents = pgTable(
  "subscription_metric_events",
  {
    eventId: varchar("event_id", { length: 255 }).primaryKey(),
    subscriptionId: varchar("subscription_id", { length: 255 }).notNull(),
    merchantId: varchar("merchant_id", { length: 255 }).notNull(),
    planId: varchar("plan_id", { length: 255 }),
    eventType: varchar("event_type", { length: 50 }).notNull(),
    occurredAt: timestamp("occurred_at", { withTimezone: true }).notNull(),
  },
  (t) => ({
    merchantTimeIdx: index("idx_subscription_metric_events_merchant_time").on(
      t.merchantId,
      t.occurredAt,
    ),
    subscriptionTimeIdx: index("idx_subscription_metric_events_subscription_time").on(
      t.subscriptionId,
      t.occurredAt,
    ),
  }),
);

export const invoices = pgTable("invoices", {
  id: serial("id").primaryKey(),
  invoiceId: varchar("invoice_id", { length: 255 }).notNull().unique(),
  merchantId: varchar("merchant_id", { length: 255 }),
  totalAmount: bigint("total_amount", { mode: "bigint" }),
  status: varchar("status", { length: 50 }),
  dueDate: timestamp("due_date"),
  paidAt: timestamp("paid_at"),
  voidedAt: timestamp("voided_at"),
  linkedPaymentId: varchar("linked_payment_id", { length: 255 }),
  subscriptionId: varchar("subscription_id", { length: 255 }),
  cycleNumber: integer("cycle_number"),
  invoiceNumber: varchar("invoice_number", { length: 64 }),
  subscriber: varchar("subscriber", { length: 255 }),
  subscriberEmail: varchar("subscriber_email", { length: 320 }),
  billingPeriodStart: timestamp("billing_period_start"),
  billingPeriodEnd: timestamp("billing_period_end"),
  createdAt: timestamp("created_at"),
  updatedAt: timestamp("updated_at").defaultNow(),
},
(t) => ({
  merchantIdIdx: index("idx_invoice_merchant_id").on(t.merchantId),
  statusIdx: index("idx_invoice_status").on(t.status),
  merchantNumberIdx: uniqueIndex("idx_invoice_merchant_number").on(t.merchantId, t.invoiceNumber),
}),
);

export const disputeBonds = pgTable("dispute_bonds", {
  id: serial("id").primaryKey(),
  disputeId: varchar("dispute_id", { length: 255 }),
  recipient: varchar("recipient", { length: 255 }),
  amount: bigint("amount", { mode: "bigint" }),
  status: varchar("status", { length: 50 }),
  createdAt: timestamp("created_at"),
});

export const merchantInvoiceSequences = pgTable("merchant_invoice_sequences", {
  merchantId: varchar("merchant_id", { length: 255 }).primaryKey(),
  nextNumber: integer("next_number").notNull().default(1),
});

export const deadLetterEvents = pgTable("dead_letter_events", {
  id: serial("id").primaryKey(),
  eventId: varchar("event_id", { length: 255 }).notNull().unique(),
  rawData: jsonb("raw_data").notNull(),
  error: text("error").notNull(),
  createdAt: timestamp("created_at").defaultNow(),
  retryCount: integer("retry_count").default(0),
});

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

/** Issue #1066: Platform fee collection records mirroring FeeRecord on-chain. */
export const feeCollectionHistory = pgTable(
  "fee_collection_history",
  {
    id: serial("id").primaryKey(),
    paymentId: varchar("payment_id", { length: 255 }).notNull(),
    feeAmount: bigint("fee_amount", { mode: "bigint" }).notNull(),
    token: varchar("token", { length: 255 }).notNull(),
    collectedAt: timestamp("collected_at").notNull(),
    createdAt: timestamp("created_at").defaultNow(),
  },
  (t) => ({
    paymentIdIdx: index("idx_fee_payment_id").on(t.paymentId),
    collectedAtIdx: index("idx_fee_collected_at").on(t.collectedAt),
  }),
);
