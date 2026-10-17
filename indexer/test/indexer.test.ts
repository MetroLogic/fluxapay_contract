/**
 * Unit & Integration Test Suite for FluxaPay Indexer
 * Covers:
 * - Issue #616: REST API server, authentication, pagination, filtering, 404s, health check
 * - Issue #618: Multi-contract configuration, subscription filters, contract ID identification & routing
 * - Issue #617: Dead-letter queue insertion, retry_count, auto-retry, manual POST /admin/replay-dlq
 * - Issue #615: Dispute lifecycle status persistence (CREATED, RESOLVED, REJECTED, ESCALATED), idempotency, unknown dispute ID handling
 */

import assert from "node:assert";
import { createHmac } from "node:crypto";
import test, { describe, it } from "node:test";
import { nativeToScVal, xdr } from "stellar-sdk";
import {
  createReplayWebhookDispatcher,
  loadConfigFromEnv,
  EventSubscriber,
} from "../src/index";
import { createServer } from "../src/server";
import { Database } from "../src/database";
import { sseManager } from "../src/sse";
import {
  clearRegisteredApiKeys,
  extractApiKey,
  registerApiKey,
  requireApiKey,
} from "../src/auth/api-key";

function signSEP10Token(secret: string, subject: string, issuer: string): string {
  const encode = (value: string) => Buffer.from(value).toString("base64url");
  const header = encode(JSON.stringify({ alg: "HS256", typ: "JWT" }));
  const payload = encode(
    JSON.stringify({
      iss: issuer,
      sub: subject,
      iat: Math.floor(Date.now() / 1000),
      exp: Math.floor(Date.now() / 1000) + 60,
    }),
  );
  const signature = createHmac("sha256", secret)
    .update(`${header}.${payload}`)
    .digest("base64url");
  return `${header}.${payload}.${signature}`;
}

// Helper to create a mocked Database instance for unit testing
function createMockDatabase() {
  const state = {
    payments: new Map<string, any>(),
    refunds: new Map<string, any>(),
    disputes: new Map<string, any>(),
    merchants: new Map<string, { registeredAt: Date | string; kycSubmittedAt: Date | string | null }>(),
    contractEvents: new Array<any>(),
    dlqEvents: new Map<string, any>(),
    paymentLinks: new Map<string, any>(),
    paymentLinkViews: new Map<string, number>(),
    subscriptionPlans: new Map<string, { merchant_id: string; amount: bigint }>(),
    subscriptions: new Map<string, {
      merchant_id: string;
      plan_id: string;
      status: string;
      created_at: Date;
    }>(),
    subscriptionMetricEvents: new Array<{
      subscription_id: string;
      merchant_id: string;
      event_type: string;
      occurred_at: Date;
    }>(),
    subscriptionMetricQueries: 0,
    dbConnected: true,
  };

  const mockDb = {
    state,
    async initialize() {},
    async close() {},
    getPool() {
      return { query: async () => ({ rows: [] }) };
    },

    async checkHealth() {
      if (!state.dbConnected) {
        return { healthy: false, database: "disconnected", details: "Connection pool exhausted" };
      }
      return { healthy: true, database: "connected" };
    },

    async getPaymentById(paymentId: string) {
      return state.payments.get(paymentId) || null;
    },

    // Issue #1039: used by the live SSE dispatcher to resolve a dispute
    // event's merchant when only `dispute_id` is present on the event.
    async getDisputeById(disputeId: string) {
      return state.disputes.get(disputeId) || null;
    },

    async recordPaymentLinkView(linkId: string) {
      if (!state.paymentLinks.has(linkId)) return false;
      state.paymentLinkViews.set(linkId, (state.paymentLinkViews.get(linkId) || 0) + 1);
      return true;
    },

    async getPaymentLinkAnalytics(linkId: string, merchantId: string) {
      const link = state.paymentLinks.get(linkId);
      if (!link || link.merchant_id !== merchantId) return null;
      return {
        link_id: linkId,
        views: state.paymentLinkViews.get(linkId) || 0,
        attempts: link.attempts,
        conversions: link.conversions,
        total_revenue_stroops: link.total_revenue_stroops,
        last_used_at: link.last_used_at,
      };
    },

    async getPaymentLinksForMerchant(merchantId: string) {
      return Array.from(state.paymentLinks.entries())
        .filter(([, link]) => link.merchant_id === merchantId)
        .map(([linkId, link]) => ({
          link_id: linkId,
          views: state.paymentLinkViews.get(linkId) || 0,
          attempts: link.attempts,
          conversions: link.conversions,
          total_revenue_stroops: link.total_revenue_stroops,
          last_used_at: link.last_used_at,
          daily_conversions: link.daily_conversions,
        }));
    },

    async getSubscriptionMetrics(merchantId: string, periodStart: Date, periodEnd: Date) {
      state.subscriptionMetricQueries++;
      const events = state.subscriptionMetricEvents.filter(
        (event) => event.merchant_id === merchantId,
      );
      const periodEvents = events.filter(
        (event) => event.occurred_at >= periodStart && event.occurred_at < periodEnd,
      );
      const previousState = new Map<string, { event_type: string; occurred_at: Date }>();
      for (const event of events) {
        if (event.occurred_at >= periodStart) continue;
        const previous = previousState.get(event.subscription_id);
        if (!previous || event.occurred_at > previous.occurred_at) {
          previousState.set(event.subscription_id, {
            event_type: event.event_type,
            occurred_at: event.occurred_at,
          });
        }
      }
      const activeAtStart = Array.from(previousState.values()).filter((event) =>
        ["CREATED", "CHARGED", "RESUMED"].includes(event.event_type),
      ).length;
      const churned = new Set(
        periodEvents
          .filter((event) => ["CANCELLED", "EXPIRED", "COMPLETED"].includes(event.event_type))
          .map((event) => event.subscription_id),
      ).size;
      const trialEnds = periodEvents.filter((event) => event.event_type === "TRIAL_ENDED");
      const trialConversions = new Set(
        trialEnds
          .filter((trial) =>
            periodEvents.some(
              (event) =>
                event.subscription_id === trial.subscription_id &&
                event.event_type === "CHARGED" &&
                event.occurred_at >= trial.occurred_at,
            ),
          )
          .map((event) => event.subscription_id),
      ).size;
      const dunningTriggers = periodEvents.filter(
        (event) => event.event_type === "DUNNING_TRIGGERED",
      );
      const dunningRecoveries = new Set(
        dunningTriggers
          .filter((dunning) =>
            periodEvents.some(
              (event) =>
                event.subscription_id === dunning.subscription_id &&
                event.event_type === "CHARGED" &&
                event.occurred_at >= dunning.occurred_at,
            ),
          )
          .map((event) => event.subscription_id),
      ).size;
      const merchantSubscriptions = Array.from(state.subscriptions.values()).filter(
        (subscription) => subscription.merchant_id === merchantId,
      );
      const activeSubscriptions = merchantSubscriptions.filter(
        (subscription) => subscription.status.toUpperCase() === "ACTIVE",
      );
      const mrr = activeSubscriptions.reduce(
        (total, subscription) =>
          total + (state.subscriptionPlans.get(subscription.plan_id)?.amount ?? 0n),
        0n,
      );
      const newSubscribers = new Set(
        periodEvents
          .filter((event) => event.event_type === "CREATED")
          .map((event) => event.subscription_id),
      ).size;

      return {
        mrr_stroops: String(mrr),
        active_subscribers: activeSubscriptions.length,
        new_subscribers: newSubscribers,
        churned_subscribers: churned,
        churn_rate: activeAtStart === 0 ? null : churned / activeAtStart,
        trial_conversions: trialConversions,
        trial_conversion_rate:
          trialEnds.length === 0 ? null : trialConversions / trialEnds.length,
        past_due_subscribers: merchantSubscriptions.filter(
          (subscription) => subscription.status.toUpperCase() === "PAST_DUE",
        ).length,
        dunning_recoveries: dunningRecoveries,
        dunning_recovery_rate:
          dunningTriggers.length === 0 ? null : dunningRecoveries / dunningTriggers.length,
      };
    },

    async getPaymentsByMerchantPaginated(merchantId: string, page = 1, limit = 20, status?: string) {
      let list = Array.from(state.payments.values()).filter((p) => p.merchant_id === merchantId);
      if (status) {
        list = list.filter((p) => p.status === status);
      }
      const total = list.length;
      const offset = (page - 1) * limit;
      const paginated = list.slice(offset, offset + limit);
      const totalPages = Math.ceil(total / limit) || 0;
      return {
        payments: paginated,
        pagination: { page, limit, total, totalPages },
      };
    },

    async getDisputesByMerchant(merchantId: string, status?: string) {
      let list = Array.from(state.disputes.values()).filter((d) => {
        const payment = state.payments.get(d.payment_id);
        return payment && payment.merchant_id === merchantId;
      });
      if (status) {
        list = list.filter((d) => d.status === status);
      }
      return list;
    },

    async getMerchantOnboardingState(merchantId: string) {
      return state.merchants.get(merchantId) || null;
    },

    async getReconciliationData() {
      return state.reconciliationData;
    },

    async getRefundById(refundId: string) {
      return state.refunds.get(refundId) || null;
    },

    async getEventsFiltered(type?: string, fromLedger?: number, toLedger?: number) {
      return state.contractEvents.filter((e) => {
        if (type) {
          const typeMatch = e.event_type === type || e.event_type.startsWith(type + "/");
          if (!typeMatch) return false;
        }
        if (fromLedger !== undefined && e.ledger < fromLedger) return false;
        if (toLedger !== undefined && e.ledger > toLedger) return false;
        return true;
      });
    },

    async storeEvent(event: any) {
      const existing = state.contractEvents.find((e) => e.event_id === event.id);
      if (existing) return false;

      state.contractEvents.push({
        event_id: event.id,
        event_type: event.topic[0],
        ledger: event.ledger,
        tx_hash: event.txHash,
        timestamp: event.timestamp,
        contract_id: event.contractId,
        data: event.value,
      });

      const [eventType, subtype] = event.topic;
      const val = event.value;

      if (eventType === "PAYMENT") {
        state.payments.set(val.payment_id, {
          payment_id: val.payment_id,
          merchant_id: val.merchant_id,
          amount: val.amount,
          currency: val.currency,
          status: subtype,
          created_at: new Date(event.timestamp * 1000),
        });
      } else if (eventType === "REFUND") {
        state.refunds.set(val.refund_id, {
          refund_id: val.refund_id,
          payment_id: val.payment_id,
          amount: val.amount,
          status: subtype,
          created_at: new Date(event.timestamp * 1000),
        });
      } else if (eventType === "DISPUTE") {
        if (subtype === "RESOLVED") {
          const dispute = state.disputes.get(val.dispute_id);
          if (!dispute) {
            console.warn(`[DisputeUpdate] Dispute ID ${val.dispute_id} not found for RESOLVED event`);
          } else {
            dispute.status = "Resolved";
            dispute.resolved_at = new Date(event.timestamp * 1000);
          }
        } else if (subtype === "REJECTED") {
          const dispute = state.disputes.get(val.dispute_id);
          if (!dispute) {
            console.warn(`[DisputeUpdate] Dispute ID ${val.dispute_id} not found for REJECTED event`);
          } else {
            dispute.status = "Rejected";
          }
        } else if (subtype === "ESCALATED") {
          const dispute = state.disputes.get(val.dispute_id);
          if (!dispute) {
            console.warn(`[DisputeUpdate] Dispute ID ${val.dispute_id} not found for ESCALATED event`);
          } else {
            dispute.escalated = true;
            dispute.status = "Escalated";
          }
        } else {
          const status = subtype === "CREATED" ? "Open" : subtype;
          // Mirrors the real `database.ts`'s `onConflictDoUpdate({ set: { status } })`:
          // an existing dispute only has its status touched here, so events
          // like REVIEWED/VOTE_CAST/AUTO_RESOLVED (issue #1039) that don't
          // carry payment_id/amount never blank out what CREATED recorded.
          const existing = state.disputes.get(val.dispute_id);
          state.disputes.set(val.dispute_id, {
            dispute_id: val.dispute_id,
            payment_id: existing ? existing.payment_id : val.payment_id,
            amount: existing ? existing.amount : val.amount,
            status,
            escalated: existing ? existing.escalated : false,
            created_at: existing ? existing.created_at : new Date(event.timestamp * 1000),
          });
        }
      }

      return true;
    },

    async storeDeadLetterEvent(eventId: string, rawData: any, error: string) {
      const rec = state.dlqEvents.get(eventId) || {
        id: state.dlqEvents.size + 1,
        event_id: eventId,
        raw_data: rawData,
        error: error,
        retry_count: 0,
        created_at: new Date(),
      };
      rec.error = error;
      state.dlqEvents.set(eventId, rec);
    },

    async getEligibleDLQEvents() {
      return Array.from(state.dlqEvents.values());
    },

    async getAllDLQEvents() {
      return Array.from(state.dlqEvents.values());
    },

    async incrementDLQRetryCount(eventId: string, error: string) {
      const rec = state.dlqEvents.get(eventId);
      if (rec) {
        rec.retry_count += 1;
        rec.error = error;
      }
    },

    async removeDeadLetterEvent(eventId: string) {
      state.dlqEvents.delete(eventId);
    },
  };

  return mockDb as unknown as Database & { state: typeof state };
}

const onboardingAuthConfig = {
  webAuthEndpoint: "https://example.test/auth",
  homeDomain: "example.test",
  serverPublicKey: "c2VydmVy",
  adminAccounts: new Set<string>(),
};

function createSEP10Token(merchantId: string): string {
  const header = Buffer.from(JSON.stringify({ alg: "HS256", typ: "JWT" })).toString("base64url");
  const payload = Buffer.from(JSON.stringify({
    iss: "server",
    sub: "account",
    merchant_id: merchantId,
    iat: Math.floor(Date.now() / 1000),
    exp: Math.floor(Date.now() / 1000) + 60,
  })).toString("base64url");
  const message = `${header}.${payload}`;
  const signature = crypto
    .createHmac("sha256", Buffer.from(onboardingAuthConfig.serverPublicKey, "base64"))
    .update(message)
    .digest("base64url");
  return `${message}.${signature}`;
}

describe("ISSUE #618 — Multi-Contract Support", () => {
  it("should load multiple contract IDs from environment variables", () => {
    const env = {
      PAYMENT_PROCESSOR_CONTRACT_ID: "C_PAYMENT_1",
      REFUND_MANAGER_CONTRACT_ID: "C_REFUND_2",
      MERCHANT_REGISTRY_CONTRACT_ID: "C_MERCHANT_3",
      FX_ORACLE_CONTRACT_ID: "C_ORACLE_4",
      PAYMENT_LINK_MANAGER_CONTRACT_ID: "C_LINK_5",
    };

    const config = loadConfigFromEnv(env);
    assert.strictEqual(config.contractIds.length, 5);
    assert.deepStrictEqual(config.contractIds, [
      "C_PAYMENT_1",
      "C_REFUND_2",
      "C_MERCHANT_3",
      "C_ORACLE_4",
      "C_LINK_5",
    ]);
  });

  it("should migrate legacy single FLUXAPAY_CONTRACT_ID cleanly", () => {
    const env = {
      FLUXAPAY_CONTRACT_ID: "C_LEGACY_SINGLE",
    };

    const config = loadConfigFromEnv(env);
    assert.deepStrictEqual(config.contractIds, ["C_LEGACY_SINGLE"]);
  });

  it("should throw an error when no contract IDs are configured", () => {
    assert.throws(
      () => loadConfigFromEnv({}),
      /No contract IDs configured/
    );
  });

  it("should preserve originating contractId on parsed events across all 5 contracts", () => {
    const mockDb = createMockDatabase();
    const config = loadConfigFromEnv({
      CONTRACT_IDS: "C_PAYMENT,C_REFUND,C_MERCHANT,C_ORACLE,C_LINK",
    });

    it("decodes Soroban RPC XDR topics and positional payment payloads", () => {
      const subscriber = new EventSubscriber({
        rpcUrl: "http://localhost:8000",
        contractIds: ["C123"],
        dbConnectionString: "postgres://localhost/test",
        pollInterval: 1000,
        startLedger: 1,
        dlqRetryIntervalMs: 1000,
        dlqMinAgeSeconds: 10,
        apiPort: 3001,
      });
      const parsed = subscriber.parseEvent({
        id: "1",
        ledger: 100,
        ledgerClosedAt: "2024-01-01T00:00:00Z",
        txHash: "xdr-tx",
        contractId: "C123",
        topic: [
          nativeToScVal("PAYMENT", { type: "symbol" }),
          nativeToScVal("CREATED", { type: "symbol" }),
        ],
        value: xdr.ScVal.scvVec([
          nativeToScVal("payment-1"),
          nativeToScVal("merchant-1"),
          nativeToScVal(100n),
          nativeToScVal({}),
        ]),
      });

      assert.ok(parsed);
      assert.deepStrictEqual(parsed?.topic, ["PAYMENT", "CREATED"]);
      assert.deepStrictEqual(parsed?.value, {
        payment_id: "payment-1",
        merchant_id: "merchant-1",
        amount: 100n,
        metadata: {},
      });
      assert.strictEqual(parsed?.timestamp, 1_704_067_200);
    });

    const subscriber = new EventSubscriber(config, mockDb);

    const rawEvent = {
      ledger: 100,
      txHash: "0xabc",
      id: "evt_1",
      contractId: "C_ORACLE",
      topic: ["FX_ORACLE", "UPDATED"],
      value: { asset: "USD/XLM", rate: 7.5 },
    };

    const parsed = subscriber.parseEvent(rawEvent);
    assert.strictEqual(parsed?.contractId, "C_ORACLE");
    assert.strictEqual(parsed?.id, "100-0xabc-evt_1");
  });

  it("decodes subscription plan and lifecycle event payloads from Soroban XDR", () => {
    const subscriber = new EventSubscriber({
      rpcUrl: "http://localhost:8000",
      contractIds: ["C123"],
      dbConnectionString: "postgres://localhost/test",
      pollInterval: 1000,
      startLedger: 1,
      dlqRetryIntervalMs: 1000,
      dlqMinAgeSeconds: 10,
      apiPort: 3001,
    });
    const parse = (subtype: string, values: Array<string | bigint | number>) =>
      subscriber.parseEvent({
        id: `event-${subtype}`,
        ledger: 100,
        ledgerClosedAt: "2026-10-01T00:00:00Z",
        txHash: "subscription-xdr",
        contractId: "C123",
        topic: [
          nativeToScVal("SUBSCRIPTION", { type: "symbol" }),
          nativeToScVal(subtype, { type: "symbol" }),
        ],
        value: xdr.ScVal.scvVec(values.map((value) => nativeToScVal(value))),
      });

    assert.deepStrictEqual(
      parse("PLAN_CREATED", ["plan_monthly", "G_MERCHANT", 4250000000n, 2_592_000])?.value,
      {
        plan_id: "plan_monthly",
        merchant_id: "G_MERCHANT",
        amount: 4250000000n,
        interval_secs: 2_592_000n,
      },
    );
    assert.deepStrictEqual(
      parse("CREATED", ["sub_123", "G_PAYER", "plan_monthly"])?.value,
      {
        subscription_id: "sub_123",
        payer: "G_PAYER",
        plan_id: "plan_monthly",
      },
    );
  });
});

describe("ISSUE #615 — Dispute Status Updates", () => {
  it("should initialize dispute with Open status on CREATED event", async () => {
    const mockDb = createMockDatabase();

    await mockDb.storeEvent({
      id: "evt_disp_1",
      timestamp: 1700000000,
      ledger: 10,
      txHash: "tx1",
      contractId: "C_DISPUTE",
      topic: ["DISPUTE", "CREATED"],
      value: { dispute_id: "disp_100", payment_id: "pay_1", amount: 5000 },
    } as any);

    const dispute = mockDb.state.disputes.get("disp_100");
    assert.ok(dispute);
    assert.strictEqual(dispute.status, "Open");
    assert.strictEqual(dispute.escalated, false);
  });

  it("should update status to Resolved and set resolved_at on RESOLVED event", async () => {
    const mockDb = createMockDatabase();

    // Create dispute
    await mockDb.storeEvent({
      id: "evt_disp_1",
      timestamp: 1700000000,
      ledger: 10,
      txHash: "tx1",
      contractId: "C_DISPUTE",
      topic: ["DISPUTE", "CREATED"],
      value: { dispute_id: "disp_100", payment_id: "pay_1", amount: 5000 },
    } as any);

    // Resolve dispute
    await mockDb.storeEvent({
      id: "evt_disp_2",
      timestamp: 1700005000,
      ledger: 20,
      txHash: "tx2",
      contractId: "C_DISPUTE",
      topic: ["DISPUTE", "RESOLVED"],
      value: { dispute_id: "disp_100", payment_id: "pay_1", amount: 5000 },
    } as any);

    const dispute = mockDb.state.disputes.get("disp_100");
    assert.strictEqual(dispute.status, "Resolved");
    assert.strictEqual(dispute.resolved_at.getTime(), 1700005000 * 1000);
  });

  it("should update status to Rejected on REJECTED event", async () => {
    const mockDb = createMockDatabase();

    await mockDb.storeEvent({
      id: "evt_disp_1",
      timestamp: 1700000000,
      ledger: 10,
      txHash: "tx1",
      contractId: "C_DISPUTE",
      topic: ["DISPUTE", "CREATED"],
      value: { dispute_id: "disp_200", payment_id: "pay_2", amount: 2000 },
    } as any);

    await mockDb.storeEvent({
      id: "evt_disp_3",
      timestamp: 1700003000,
      ledger: 15,
      txHash: "tx3",
      contractId: "C_DISPUTE",
      topic: ["DISPUTE", "REJECTED"],
      value: { dispute_id: "disp_200", payment_id: "pay_2", amount: 2000 },
    } as any);

    const dispute = mockDb.state.disputes.get("disp_200");
    assert.strictEqual(dispute.status, "Rejected");
  });

  it("should update status to Escalated and set escalated=true on ESCALATED event", async () => {
    const mockDb = createMockDatabase();

    await mockDb.storeEvent({
      id: "evt_disp_1",
      timestamp: 1700000000,
      ledger: 10,
      txHash: "tx1",
      contractId: "C_DISPUTE",
      topic: ["DISPUTE", "CREATED"],
      value: { dispute_id: "disp_300", payment_id: "pay_3", amount: 3000 },
    } as any);

    await mockDb.storeEvent({
      id: "evt_disp_4",
      timestamp: 1700004000,
      ledger: 18,
      txHash: "tx4",
      contractId: "C_DISPUTE",
      topic: ["DISPUTE", "ESCALATED"],
      value: { dispute_id: "disp_300", payment_id: "pay_3", amount: 3000 },
    } as any);

    const dispute = mockDb.state.disputes.get("disp_300");
    assert.strictEqual(dispute.status, "Escalated");
    assert.strictEqual(dispute.escalated, true);
  });

  it("should safely handle events referencing an unknown dispute ID without crashing", async () => {
    const mockDb = createMockDatabase();

    const result = await mockDb.storeEvent({
      id: "evt_disp_unk",
      timestamp: 1700000000,
      ledger: 10,
      txHash: "tx_unk",
      contractId: "C_DISPUTE",
      topic: ["DISPUTE", "RESOLVED"],
      value: { dispute_id: "disp_nonexistent", payment_id: "pay_99", amount: 100 },
    } as any);

    assert.strictEqual(result, true);
    assert.strictEqual(mockDb.state.disputes.has("disp_nonexistent"), false);
  });

  it("should handle duplicate status events idempotently", async () => {
    const mockDb = createMockDatabase();

    await mockDb.storeEvent({
      id: "evt_disp_1",
      timestamp: 1700000000,
      ledger: 10,
      txHash: "tx1",
      contractId: "C_DISPUTE",
      topic: ["DISPUTE", "CREATED"],
      value: { dispute_id: "disp_400", payment_id: "pay_4", amount: 4000 },
    } as any);

    await mockDb.storeEvent({
      id: "evt_disp_res_1",
      timestamp: 1700001000,
      ledger: 12,
      txHash: "tx_res_1",
      contractId: "C_DISPUTE",
      topic: ["DISPUTE", "RESOLVED"],
      value: { dispute_id: "disp_400", payment_id: "pay_4", amount: 4000 },
    } as any);

    await mockDb.storeEvent({
      id: "evt_disp_res_2",
      timestamp: 1700001000,
      ledger: 12,
      txHash: "tx_res_1",
      contractId: "C_DISPUTE",
      topic: ["DISPUTE", "RESOLVED"],
      value: { dispute_id: "disp_400", payment_id: "pay_4", amount: 4000 },
    } as any);

    const dispute = mockDb.state.disputes.get("disp_400");
    assert.strictEqual(dispute.status, "Resolved");
  });
});

describe("ISSUE #617 — Dead-Letter Queue and Retry", () => {
  it("should persist failed event into dead_letter_events", async () => {
    const mockDb = createMockDatabase();

    await mockDb.storeDeadLetterEvent("100-tx1-evt_err", { malformed: true }, "Parsing JSON failed");

    const dlqList = await mockDb.getEligibleDLQEvents();
    assert.strictEqual(dlqList.length, 1);
    assert.strictEqual(dlqList[0].event_id, "100-tx1-evt_err");
    assert.strictEqual(dlqList[0].error, "Parsing JSON failed");
    assert.strictEqual(dlqList[0].retry_count, 0);
  });

  it("should increment retry_count on retry failure", async () => {
    const mockDb = createMockDatabase();

    await mockDb.storeDeadLetterEvent("100-tx1-evt_err", { malformed: true }, "Parsing JSON failed");
    await mockDb.incrementDLQRetryCount("100-tx1-evt_err", "Retry attempt 1 failed");

    const dlqList = await mockDb.getEligibleDLQEvents();
    assert.strictEqual(dlqList[0].retry_count, 1);
    assert.strictEqual(dlqList[0].error, "Retry attempt 1 failed");
  });

  it("should remove event from DLQ upon successful replay", async () => {
    const mockDb = createMockDatabase();
    const config = loadConfigFromEnv({ CONTRACT_IDS: "C_TEST" });
    const subscriber = new EventSubscriber(config, mockDb);

    const rawEvent = {
      ledger: 50,
      txHash: "tx_dlq_1",
      id: "evt_dlq_1",
      contractId: "C_TEST",
      topic: ["PAYMENT", "CONFIRMED"],
      value: { payment_id: "pay_dlq_1", merchant_id: "m_1", amount: 100, currency: "XLM" },
    };

    await mockDb.storeDeadLetterEvent("50-tx_dlq_1-evt_dlq_1", rawEvent, "Temporary DB outage");

    const replayResult = await subscriber.retryDLQEvents(true);
    assert.strictEqual(replayResult.attempted, 1);
    assert.strictEqual(replayResult.succeeded, 1);
    assert.strictEqual(replayResult.failed, 0);

    const dlqList = await mockDb.getEligibleDLQEvents();
    assert.strictEqual(dlqList.length, 0);
    assert.ok(mockDb.state.payments.has("pay_dlq_1"));
  });
});

describe("ISSUE #616 — Indexer REST API Server", () => {
  it("should return health status on GET /health without auth", async () => {
    const mockDb = createMockDatabase();
    const app = createServer(mockDb);
    const server = app.listen(0);
    const address = server.address() as any;

    try {
      const res = await fetch(`http://127.0.0.1:${address.port}/health`);
      const data = (await res.json()) as any;

      assert.strictEqual(res.status, 200);
      assert.strictEqual(data.status, "healthy");
      assert.strictEqual(data.database, "connected");
    } finally {
      server.close();
    }
  });

  it("should return 503 Service Unavailable when database connection is down", async () => {
    const mockDb = createMockDatabase();
    mockDb.state.dbConnected = false;
    const app = createServer(mockDb);
    const server = app.listen(0);
    const address = server.address() as any;

    try {
      const res = await fetch(`http://127.0.0.1:${address.port}/health`);
      const data = (await res.json()) as any;

      assert.strictEqual(res.status, 503);
      assert.strictEqual(data.status, "unhealthy");
      assert.strictEqual(data.database, "disconnected");
    } finally {
      server.close();
    }
  });

  it("should return 401 for the onboarding endpoint without a SEP-10 bearer token", async () => {
    const app = createServer(createMockDatabase(), undefined, onboardingAuthConfig);
    const server = app.listen(0);
    const address = server.address() as any;

    try {
      const res = await fetch(`http://127.0.0.1:${address.port}/v1/merchants/me/onboarding`);
      assert.strictEqual(res.status, 401);
    } finally {
      server.close();
    }
  });

  it("should return zero progress for a merchant with no indexed setup state", async () => {
    const app = createServer(createMockDatabase(), undefined, onboardingAuthConfig);
    const server = app.listen(0);
    const address = server.address() as any;

    try {
      const res = await fetch(`http://127.0.0.1:${address.port}/v1/merchants/me/onboarding`, {
        headers: { Authorization: `Bearer ${createSEP10Token("merchant-new")}` },
      });
      const data = await res.json() as any;

      assert.strictEqual(res.status, 200);
      assert.strictEqual(data.completion_pct, 0);
      assert.strictEqual(data.steps.length, 6);
      assert.ok(data.steps.every((step: any) => !step.completed && step.action_url));
    } finally {
      server.close();
    }
  });

  it("should count registration as one of the six onboarding steps", async () => {
    const mockDb = createMockDatabase();
    mockDb.state.merchants.set("merchant-registered", {
      registeredAt: "2026-01-01T00:00:00.000Z",
      kycSubmittedAt: null,
    });
    const app = createServer(mockDb, undefined, onboardingAuthConfig);
    const server = app.listen(0);
    const address = server.address() as any;

    try {
      const res = await fetch(`http://127.0.0.1:${address.port}/v1/merchants/me/onboarding`, {
        headers: { Authorization: `Bearer ${createSEP10Token("merchant-registered")}` },
      });
      const data = await res.json() as any;

      assert.strictEqual(res.status, 200);
      assert.strictEqual(data.completion_pct, 17);
      assert.strictEqual(data.steps[0].completed, true);
      assert.strictEqual(data.steps[1].completed, false);
    } finally {
      server.close();
    }
  });

  it("should count indexed registration and KYC verification toward six-step progress", async () => {
    const mockDb = createMockDatabase();
    mockDb.state.merchants.set("merchant-1", {
      registeredAt: "2026-01-01T00:00:00.000Z",
      kycSubmittedAt: "2026-01-02T00:00:00.000Z",
    });
    const app = createServer(mockDb, undefined, onboardingAuthConfig);
    const server = app.listen(0);
    const address = server.address() as any;

    try {
      const res = await fetch(`http://127.0.0.1:${address.port}/v1/merchants/me/onboarding`, {
        headers: { Authorization: `Bearer ${createSEP10Token("merchant-1")}` },
      });
      const data = await res.json() as any;

      assert.strictEqual(res.status, 200);
      assert.strictEqual(data.completion_pct, 33);
      assert.deepStrictEqual(
        data.steps.filter((step: any) => step.completed).map((step: any) => step.key),
        ["merchant_registered", "kyc_submitted"]
      );
      assert.strictEqual(data.steps[0].completed_at, "2026-01-01T00:00:00.000Z");
      assert.strictEqual(data.steps[1].completed_at, "2026-01-02T00:00:00.000Z");
      assert.strictEqual(data.steps[2].action_url, "/settings/webhooks");
    } finally {
      server.close();
    }
  });

  it("should reject requests with missing or invalid API key with 401", () => {
    process.env.API_KEY = "valid_secret_key";

    let statusSent = 0;
    let jsonSent: any = null;
    const resMock: any = {
      status(s: number) { statusSent = s; return resMock; },
      json(data: any) { jsonSent = data; return resMock; },
    };

    // 1. Missing key
    const reqMissing: any = { headers: {} };
    requireApiKey(reqMissing, resMock, () => {});
    assert.strictEqual(statusSent, 401);
    assert.strictEqual(jsonSent.error, "Missing API key");

    // 2. Invalid key
    const reqInvalid: any = { headers: { "x-api-key": "wrong_key" } };
    requireApiKey(reqInvalid, resMock, () => {});
    assert.strictEqual(statusSent, 401);
    assert.strictEqual(jsonSent.error, "Invalid API key");

    // 3. Valid key
    let nextCalled = false;
    const reqValid: any = { headers: { "x-api-key": "valid_secret_key" } };
    requireApiKey(reqValid, resMock, () => { nextCalled = true; });
    assert.strictEqual(nextCalled, true);
  });

  it("should return payment details or 404 for GET /payments/:paymentId", async () => {
    const mockDb = createMockDatabase();
    mockDb.state.payments.set("pay_123", {
      payment_id: "pay_123",
      merchant_id: "m_1",
      amount: 500,
      currency: "XLM",
      status: "Confirmed",
    });

    const paymentFound = await mockDb.getPaymentById("pay_123");
    assert.ok(paymentFound);
    assert.strictEqual((paymentFound as any).amount, 500);

    const paymentNotFound = await mockDb.getPaymentById("pay_nonexistent");
    assert.strictEqual(paymentNotFound, null);
  });

  it("should paginate merchant payments and support status filtering", async () => {
    const mockDb = createMockDatabase();
    mockDb.state.payments.set("pay_1", { payment_id: "pay_1", merchant_id: "m_1", amount: 100, status: "Confirmed" });
    mockDb.state.payments.set("pay_2", { payment_id: "pay_2", merchant_id: "m_1", amount: 200, status: "Confirmed" });
    mockDb.state.payments.set("pay_3", { payment_id: "pay_3", merchant_id: "m_1", amount: 300, status: "Pending" });
    mockDb.state.payments.set("pay_4", { payment_id: "pay_4", merchant_id: "m_2", amount: 400, status: "Confirmed" });

    // Filter status=Confirmed
    const resultConfirmed = await mockDb.getPaymentsByMerchantPaginated("m_1", 1, 10, "Confirmed");
    assert.strictEqual(resultConfirmed.pagination.total, 2);
    assert.strictEqual(resultConfirmed.payments.length, 2);

    // Pagination limit=1
    const resultPage1 = await mockDb.getPaymentsByMerchantPaginated("m_1", 1, 1);
    assert.strictEqual(resultPage1.pagination.total, 3);
    assert.strictEqual(resultPage1.pagination.totalPages, 3);
    assert.strictEqual(resultPage1.payments.length, 1);
  });

  it("should query merchant disputes with optional status filtering", async () => {
    const mockDb = createMockDatabase();
    mockDb.state.payments.set("pay_1", { payment_id: "pay_1", merchant_id: "m_1" });
    mockDb.state.payments.set("pay_2", { payment_id: "pay_2", merchant_id: "m_1" });

    mockDb.state.disputes.set("disp_1", { dispute_id: "disp_1", payment_id: "pay_1", status: "Open" });
    mockDb.state.disputes.set("disp_2", { dispute_id: "disp_2", payment_id: "pay_2", status: "Resolved" });

    const openDisputes = await mockDb.getDisputesByMerchant("m_1", "Open");
    assert.strictEqual(openDisputes.length, 1);
    assert.strictEqual((openDisputes[0] as any).dispute_id, "disp_1");
  });

  it("should filter events by event type and ledger range", async () => {
    const mockDb = createMockDatabase();
    mockDb.state.contractEvents.push(
      { event_id: "e1", event_type: "PAYMENT", ledger: 10 },
      { event_id: "e2", event_type: "PAYMENT", ledger: 20 },
      { event_id: "e3", event_type: "DISPUTE", ledger: 30 }
    );

    const filteredType = await mockDb.getEventsFiltered("PAYMENT");
    assert.strictEqual(filteredType.length, 2);

    const filteredLedger = await mockDb.getEventsFiltered(undefined, 15, 25);
    assert.strictEqual(filteredLedger.length, 1);
    assert.strictEqual((filteredLedger[0] as any).event_id, "e2");
  });

  it("should enforce admin auth and validate ledger range on POST /admin/replay", async () => {
    process.env.ADMIN_API_KEY = "admin_secret_123";
    process.env.API_KEY = "admin_secret_123";
    const mockDb = createMockDatabase();
    const app = createServer(mockDb as any);
    const server = app.listen(0);
    const port = (server.address() as any).port;

    try {
      // 1. Missing admin key -> 401
      const resNoAuth = await fetch(`http://localhost:${port}/admin/replay?from_ledger=100&to_ledger=200`, {
        method: "POST",
      });
      assert.strictEqual(resNoAuth.status, 401);

      // 2. Invalid range (to < from) -> 400
      const resBadRange = await fetch(`http://localhost:${port}/admin/replay?from_ledger=200&to_ledger=100`, {
        method: "POST",
        headers: {
          "x-admin-api-key": "admin_secret_123",
          "x-api-key": "admin_secret_123",
        },
      });
      assert.strictEqual(resBadRange.status, 400);

      // 3. Excessive range (> 10000 ledgers) -> 400
      const resTooLarge = await fetch(`http://localhost:${port}/admin/replay?from_ledger=1&to_ledger=10001`, {
        method: "POST",
        headers: {
          "x-admin-api-key": "admin_secret_123",
          "x-api-key": "admin_secret_123",
        },
      });
      assert.strictEqual(resTooLarge.status, 400);
    } finally {
      server.close();
    }
  });

  it("should stream SSE progress and complete on POST /admin/replay", async () => {
    process.env.ADMIN_API_KEY = "admin_secret_123";
    process.env.API_KEY = "admin_secret_123";
    const mockDb = createMockDatabase();
    const mockReplayHandler = async (
      from: number,
      to: number,
      onProgress?: (p: {
        processed: number;
        total: number;
        stored: number;
        skipped_duplicates: number;
      }) => void
    ) => {
      onProgress?.({ processed: 5, total: 10, stored: 5, skipped_duplicates: 2 });
      return { processed: 10, total: 10, stored: 10, skipped_duplicates: 2 };
    };

    const app = createServer(mockDb as any, undefined, mockReplayHandler as any);
    const server = app.listen(0);
    const port = (server.address() as any).port;

    try {
      const res = await fetch(`http://localhost:${port}/admin/replay?from_ledger=100&to_ledger=110`, {
        method: "POST",
        headers: {
          "x-admin-api-key": "admin_secret_123",
          "x-api-key": "admin_secret_123",
        },
      });
      assert.strictEqual(res.status, 200);
      assert.strictEqual(res.headers.get("content-type"), "text/event-stream");

      const text = await res.text();
      assert.ok(text.includes('"processed":5'));
      assert.ok(text.includes('"skipped_duplicates":2'));
      assert.ok(text.includes('"type":"complete"'));
    } finally {
      server.close();
    }
  });

  it("requires an admin-scoped API key on POST /v1/admin/replay", async () => {
    const previousApiKey = process.env.API_KEY;
    process.env.API_KEY = "replay_admin_key";
    registerApiKey({ key: "replay_read_key", scopes: ["read:analytics"] });
    const app = createServer(createMockDatabase() as any, undefined, async () => ({
      processed: 0,
      stored: 0,
      skipped_duplicates: 0,
      total: 1,
    }));
    const server = app.listen(0);
    const port = (server.address() as any).port;

    try {
      const noKey = await fetch(`http://localhost:${port}/v1/admin/replay`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ from_ledger: 1, to_ledger: 1 }),
      });
      assert.strictEqual(noKey.status, 401);

      const insufficientScope = await fetch(`http://localhost:${port}/v1/admin/replay`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "x-api-key": "replay_read_key",
        },
        body: JSON.stringify({ from_ledger: 1, to_ledger: 1 }),
      });
      assert.strictEqual(insufficientScope.status, 403);

      const admin = await fetch(`http://localhost:${port}/v1/admin/replay`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "x-api-key": "replay_admin_key",
        },
        body: JSON.stringify({ from_ledger: 1, to_ledger: 1 }),
      });
      assert.strictEqual(admin.status, 200);
      assert.ok((await admin.text()).includes('"skipped_duplicates":0'));

      const malformedLedger = await fetch(`http://localhost:${port}/v1/admin/replay`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "x-api-key": "replay_admin_key",
        },
        body: JSON.stringify({ from_ledger: "1abc", to_ledger: 2 }),
      });
      assert.strictEqual(malformedLedger.status, 400);

      const oversizedRange = await fetch(`http://localhost:${port}/v1/admin/replay`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "x-api-key": "replay_admin_key",
        },
        body: JSON.stringify({ from_ledger: 1, to_ledger: 10001 }),
      });
      assert.strictEqual(oversizedRange.status, 400);
    } finally {
      server.close();
      clearRegisteredApiKeys();
      if (previousApiKey === undefined) delete process.env.API_KEY;
      else process.env.API_KEY = previousApiKey;
    }
  });

  it("records payment-link views and enforces owner-only analytics", async () => {
    const previousHomeDomain = process.env.SEP10_HOME_DOMAIN;
    const previousServerKey = process.env.STELLAR_SERVER_PUBLIC_KEY;
    const secret = "payment-link-analytics-test-key";
    const homeDomain = "fluxapay-test.example";
    process.env.SEP10_HOME_DOMAIN = homeDomain;
    process.env.STELLAR_SERVER_PUBLIC_KEY = Buffer.from(secret).toString("base64");

    const mockDb = createMockDatabase();
    mockDb.state.paymentLinks.set("link_abc123", {
      merchant_id: "G_MERCHANT",
      attempts: 4,
      conversions: 3,
      total_revenue_stroops: "15500000000",
      last_used_at: new Date("2026-10-15T18:00:00.000Z"),
      daily_conversions: [0, 1, 0, 0, 1, 0, 1],
    });
    const app = createServer(mockDb as any);
    const server = app.listen(0);
    const port = (server.address() as any).port;
    const ownerToken = signSEP10Token(secret, "G_MERCHANT", homeDomain);

    try {
      for (let i = 0; i < 2; i++) {
        const view = await fetch(`http://localhost:${port}/v1/payment-links/link_abc123/view`, {
          method: "POST",
        });
        assert.strictEqual(view.status, 202);
      }

      const analytics = await fetch(
        `http://localhost:${port}/v1/payment-links/link_abc123/analytics`,
        { headers: { Authorization: `Bearer ${ownerToken}` } },
      );
      assert.strictEqual(analytics.status, 200);
      assert.deepStrictEqual(await analytics.json(), {
        link_id: "link_abc123",
        views: 2,
        attempts: 4,
        conversions: 3,
        conversion_rate: 0.75,
        total_revenue_usdc: "1550.00",
        last_used_at: "2026-10-15T18:00:00.000Z",
      });

      const otherMerchantToken = signSEP10Token(secret, "G_OTHER", homeDomain);
      const forbidden = await fetch(
        `http://localhost:${port}/v1/payment-links/link_abc123/analytics`,
        { headers: { Authorization: `Bearer ${otherMerchantToken}` } },
      );
      assert.strictEqual(forbidden.status, 404);

      const links = await fetch(`http://localhost:${port}/v1/payment-links`, {
        headers: { Authorization: `Bearer ${ownerToken}` },
      });
      assert.strictEqual(links.status, 200);
      const linksBody = (await links.json()) as {
        payment_links: Array<{
          daily_conversions: number[];
          conversion_rate: number | null;
        }>;
      };
      assert.deepStrictEqual(linksBody.payment_links[0].daily_conversions, [0, 1, 0, 0, 1, 0, 1]);
      assert.strictEqual(linksBody.payment_links[0].conversion_rate, 0.75);

      mockDb.state.paymentLinks.set("link_unused", {
        merchant_id: "G_MERCHANT",
        attempts: 0,
        conversions: 0,
        total_revenue_stroops: "0",
        last_used_at: null,
        daily_conversions: [0, 0, 0, 0, 0, 0, 0],
      });
      const unusedAnalytics = await fetch(
        `http://localhost:${port}/v1/payment-links/link_unused/analytics`,
        { headers: { Authorization: `Bearer ${ownerToken}` } },
      );
      const unusedBody = (await unusedAnalytics.json()) as { conversion_rate: number | null };
      assert.strictEqual(unusedBody.conversion_rate, null);
    } finally {
      server.close();
      if (previousHomeDomain === undefined) delete process.env.SEP10_HOME_DOMAIN;
      else process.env.SEP10_HOME_DOMAIN = previousHomeDomain;
      if (previousServerKey === undefined) delete process.env.STELLAR_SERVER_PUBLIC_KEY;
      else process.env.STELLAR_SERVER_PUBLIC_KEY = previousServerKey;
    }
  });

  it("returns merchant-scoped subscription metrics with MRR, churn, and five-minute caching", async () => {
    clearRegisteredApiKeys();
    const merchantId = "G_SUBSCRIPTION_METRICS";
    const apiKey = "subscription-metrics-merchant-key";
    registerApiKey({ key: apiKey, merchantId, scopes: ["read:analytics"] });

    const mockDb = createMockDatabase();
    mockDb.state.subscriptionPlans.set("plan_monthly", {
      merchant_id: merchantId,
      amount: 4250000000n,
    });
    mockDb.state.subscriptions.set("sub_active", {
      merchant_id: merchantId,
      plan_id: "plan_monthly",
      status: "ACTIVE",
      created_at: new Date("2026-09-01T00:00:00.000Z"),
    });
    mockDb.state.subscriptions.set("sub_churned", {
      merchant_id: merchantId,
      plan_id: "plan_monthly",
      status: "CANCELLED",
      created_at: new Date("2026-09-02T00:00:00.000Z"),
    });
    mockDb.state.subscriptions.set("sub_new", {
      merchant_id: merchantId,
      plan_id: "plan_monthly",
      status: "ACTIVE",
      created_at: new Date("2026-10-10T00:00:00.000Z"),
    });
    mockDb.state.subscriptionMetricEvents.push(
      {
        subscription_id: "sub_active",
        merchant_id: merchantId,
        event_type: "CREATED",
        occurred_at: new Date("2026-09-01T00:00:00.000Z"),
      },
      {
        subscription_id: "sub_churned",
        merchant_id: merchantId,
        event_type: "CREATED",
        occurred_at: new Date("2026-09-02T00:00:00.000Z"),
      },
      {
        subscription_id: "sub_churned",
        merchant_id: merchantId,
        event_type: "CANCELLED",
        occurred_at: new Date("2026-10-05T00:00:00.000Z"),
      },
      {
        subscription_id: "sub_new",
        merchant_id: merchantId,
        event_type: "CREATED",
        occurred_at: new Date("2026-10-10T00:00:00.000Z"),
      },
    );

    const app = createServer(mockDb as any);
    const server = app.listen(0);
    const port = (server.address() as any).port;
    const url = `http://localhost:${port}/v1/subscriptions/metrics?merchant_id=${merchantId}&period=2026-10`;
    const headers = { "x-api-key": apiKey };

    try {
      const first = await fetch(url, { headers });
      assert.strictEqual(first.status, 200);
      assert.deepStrictEqual(await first.json(), {
        period: "2026-10",
        mrr_usdc: "850.00",
        active_subscribers: 2,
        new_subscribers: 1,
        churned_subscribers: 1,
        churn_rate: 0.5,
        trial_conversions: 0,
        trial_conversion_rate: null,
        past_due_subscribers: 0,
        dunning_recovery_rate: null,
      });
      assert.strictEqual(mockDb.state.subscriptionMetricQueries, 1);

      const cached = await fetch(url, { headers });
      assert.strictEqual(cached.status, 200);
      assert.strictEqual(mockDb.state.subscriptionMetricQueries, 1);

      const otherMerchant = await fetch(
        `http://localhost:${port}/v1/subscriptions/metrics?merchant_id=G_OTHER&period=2026-10`,
        { headers },
      );
      assert.strictEqual(otherMerchant.status, 403);

      const invalidPeriod = await fetch(
        `http://localhost:${port}/v1/subscriptions/metrics?merchant_id=${merchantId}&period=2026-13`,
        { headers },
      );
      assert.strictEqual(invalidPeriod.status, 400);

      const noScopeKey = "subscription-metrics-read-only-key";
      registerApiKey({ key: noScopeKey, merchantId, scopes: ["read:payments"] });
      const insufficientScope = await fetch(url, {
        headers: { "x-api-key": noScopeKey },
      });
      assert.strictEqual(insufficientScope.status, 403);
    } finally {
      server.close();
      clearRegisteredApiKeys();
    }
  });

  describe("Issue #1078: ledger-range event replay", () => {
    const config = {
      rpcUrl: "http://localhost:8000",
      contractIds: ["C123"],
      dbConnectionString: "postgres://localhost/test",
      pollInterval: 1000,
      startLedger: 1,
      dlqRetryIntervalMs: 1000,
      dlqMinAgeSeconds: 10,
      apiPort: 3001,
    };

    function rpcEvent(ledger: number, eventIndex: number) {
      return {
        id: eventIndex,
        ledger,
        txHash: `tx-${ledger}`,
        contractId: "C123",
        topic: ["PAYMENT", "CREATED"],
        value: {
          payment_id: `payment-${ledger}`,
          merchant_id: "merchant-1",
          amount: "100",
          currency: "USDC",
        },
        timestamp: 1_700_000_000,
      };
    }

    it("skips all existing events without dispatching duplicate webhooks", async () => {
      const mockDb = createMockDatabase();
      const events = [rpcEvent(100, 1), rpcEvent(101, 2), rpcEvent(102, 3)];
      mockDb.state.contractEvents.push(
        ...events.map((event) => ({
          event_id: `${event.ledger}-${event.txHash}-${event.id}`,
          event_type: "PAYMENT",
          ledger: event.ledger,
        })),
      );
      let dispatches = 0;
      const subscriber = new EventSubscriber(config, mockDb, {
        eventSource: {
          getEvents: async () => ({ events, latestLedger: 102 }),
        } as any,
        replayWebhookDispatcher: async () => {
          dispatches++;
        },
      });

      const result = await subscriber.replayLedgerRange(100, 102);
      assert.deepStrictEqual(result, {
        processed: 3,
        stored: 0,
        skipped_duplicates: 3,
        total: 3,
      });
      assert.strictEqual(dispatches, 0);
    });

    it("stores five new events and dispatches replay-marked webhooks", async () => {
      const mockDb = createMockDatabase();
      const events = Array.from({ length: 5 }, (_, index) => rpcEvent(200 + index, index + 1));
      const deliveredPayloads: any[] = [];
      const attempts: any[] = [];
      const webhookStore = {
        async listEnabledEndpointsForEvent() {
          return [{
            id: "endpoint-1",
            merchantId: "merchant-1",
            url: "https://merchant.example/webhook",
            signingSecret: "test-secret",
            eventTypes: ["payment.created"],
            enabled: true,
          }];
        },
        async recordAttempt(attempt: any) {
          attempts.push(attempt);
        },
      };
      const replayWebhookDispatcher = createReplayWebhookDispatcher(
        mockDb,
        webhookStore as any,
        async (_url, init) => {
          deliveredPayloads.push(JSON.parse(init.body));
          return { status: 200, text: async () => "ok" };
        },
      );
      const subscriber = new EventSubscriber(config, mockDb, {
        eventSource: {
          getEvents: async () => ({ events, latestLedger: 204 }),
        } as any,
        replayWebhookDispatcher,
      });

      const result = await subscriber.replayLedgerRange(200, 204);
      assert.deepStrictEqual(result, {
        processed: 5,
        stored: 5,
        skipped_duplicates: 0,
        total: 5,
      });
      assert.strictEqual(deliveredPayloads.length, 5);
      assert.strictEqual(attempts.length, 5);
      for (const payload of deliveredPayloads) {
        assert.strictEqual(payload.replayed, true);
        assert.strictEqual(payload.data.replayed, true);
        assert.ok(payload.event_id.startsWith("20"));
      }
      assert.strictEqual(mockDb.state.contractEvents.length, 5);
    });

    it("continues paging when a full page ends on the requested final ledger", async () => {
      const mockDb = createMockDatabase();
      const firstPage = Array.from({ length: 100 }, (_, index) => ({
        ...rpcEvent(300, index + 1),
        pagingToken: index === 99 ? "page-cursor-300" : `page-cursor-${index}`,
      }));
      let pageRequests = 0;
      const subscriber = new EventSubscriber(config, mockDb, {
        eventSource: {
          getEvents: async (request: any) => {
            pageRequests++;
            if (pageRequests === 1) return { events: firstPage, latestLedger: 300 };
            assert.strictEqual(request.cursor, "page-cursor-300");
            return { events: [rpcEvent(300, 101)], latestLedger: 300 };
          },
        } as any,
        replayWebhookDispatcher: async () => {},
      });

      const result = await subscriber.replayLedgerRange(300, 300);
      assert.strictEqual(result.stored, 101);
      assert.strictEqual(result.processed, 101);
      assert.strictEqual(result.skipped_duplicates, 0);
      assert.strictEqual(pageRequests, 2);
    });
  });

  describe("Issue #766: STREAM/WITHDRAWN null memo handling", () => {
    it("safely handles STREAM/WITHDRAWN events with memo provided", () => {
      const subscriber = new EventSubscriber({
        rpcUrl: "http://localhost:8000",
        contractIds: ["C123"],
        dbConnectionString: "postgres://localhost/test",
        pollInterval: 1000,
        startLedger: 1,
        dlqRetryIntervalMs: 1000,
        dlqMinAgeSeconds: 10,
        apiPort: 3001,
      });

      const eventWithMemo = {
        id: "evt-001",
        ledger: 100,
        txHash: "hash-001",
        contractId: "C123",
        topic: ["STREAM", "WITHDRAWN", "stream_abc"],
        value: {
          stream_id: "stream_abc",
          recipient: "G_RECEIVER",
          amount: 5000,
          remaining_deposit: 15000,
        },
        transaction: {
          memo: {
            value: "payout-invoice-42",
          },
        },
      };

      const parsed = subscriber.parseEvent(eventWithMemo);
      assert.ok(parsed, "Event should be parsed successfully");
      assert.strictEqual((parsed?.value as any).memo, "payout-invoice-42");
    });

    it("safely handles STREAM/WITHDRAWN events when transaction memo is null without throwing", () => {
      const subscriber = new EventSubscriber({
        rpcUrl: "http://localhost:8000",
        contractIds: ["C123"],
        dbConnectionString: "postgres://localhost/test",
        pollInterval: 1000,
        startLedger: 1,
        dlqRetryIntervalMs: 1000,
        dlqMinAgeSeconds: 10,
        apiPort: 3001,
      });

      // Programmatic withdrawal with no memo: transaction.memo is null
      const eventWithNullMemo = {
        id: "evt-002",
        ledger: 101,
        txHash: "hash-002",
        contractId: "C123",
        topic: ["STREAM", "WITHDRAWN", "stream_xyz"],
        value: {
          stream_id: "stream_xyz",
          recipient: "G_RECEIVER",
          amount: 2500,
          remaining_deposit: 0,
        },
        transaction: {
          memo: null,
        },
      };

      // Must not throw TypeError: Cannot read properties of null (reading 'value')
      const parsed = subscriber.parseEvent(eventWithNullMemo);
      assert.ok(parsed, "Event should be parsed successfully");
      assert.strictEqual((parsed?.value as any).memo, null);
    });

    it("safely handles STREAM/WITHDRAWN events when transaction object itself is undefined", () => {
      const subscriber = new EventSubscriber({
        rpcUrl: "http://localhost:8000",
        contractIds: ["C123"],
        dbConnectionString: "postgres://localhost/test",
        pollInterval: 1000,
        startLedger: 1,
        dlqRetryIntervalMs: 1000,
        dlqMinAgeSeconds: 10,
        apiPort: 3001,
      });

      const memolessEvent = {
        id: "evt-003",
        ledger: 102,
        txHash: "hash-003",
        contractId: "C123",
        topic: ["STREAM", "WITHDRAWN", "stream_none"],
        value: {
          stream_id: "stream_none",
          recipient: "G_RECEIVER",
          amount: 1000,
        },
      };

      const parsed = subscriber.parseEvent(memolessEvent);
      assert.ok(parsed, "Event should be parsed successfully");
      assert.strictEqual((parsed?.value as any).memo, null);
    });

    it("batch processing does not roll back or fail on a missing memo", async () => {
      const storedEvents: any[] = [];
      const mockDb = {
        async storeEvent(evt: any) {
          storedEvents.push(evt);
          return true;
        },
        async storeDeadLetterEvent() {},
      };

      const subscriber = new EventSubscriber(
        {
          rpcUrl: "http://localhost:8000",
          contractIds: ["C123"],
          dbConnectionString: "postgres://localhost/test",
          pollInterval: 1000,
          startLedger: 1,
          dlqRetryIntervalMs: 1000,
          dlqMinAgeSeconds: 10,
          apiPort: 3001,
        },
        mockDb as any
      );

      const batch = [
        {
          id: "batch-1",
          ledger: 200,
          txHash: "hash-b1",
          contractId: "C123",
          topic: ["STREAM", "WITHDRAWN"],
          value: { stream_id: "s1", amount: 100 },
          transaction: { memo: { value: "memo-1" } },
        },
        {
          id: "batch-2",
          ledger: 201,
          txHash: "hash-b2",
          contractId: "C123",
          topic: ["STREAM", "WITHDRAWN"],
          value: { stream_id: "s2", amount: 200 },
          transaction: { memo: null }, // missing memo!
        },
        {
          id: "batch-3",
          ledger: 202,
          txHash: "hash-b3",
          contractId: "C123",
          topic: ["STREAM", "WITHDRAWN"],
          value: { stream_id: "s3", amount: 300 },
          // No transaction field at all
        },
      ];

      const result = await subscriber.processBatch(batch);
      assert.strictEqual(result.processed, 3);
      assert.strictEqual(result.stored, 3);
      assert.strictEqual(result.errors, 0);
      assert.strictEqual(storedEvents.length, 3);
      assert.strictEqual(storedEvents[0].value.memo, "memo-1");
      assert.strictEqual(storedEvents[1].value.memo, null);
      assert.strictEqual(storedEvents[2].value.memo, null);
    });
  });
});

describe("ISSUE #1081 — Reconciliation report export", () => {
  it("should return merchant-scoped report totals with FX equivalents and CSV export", async () => {
    const previousKeys = process.env.MERCHANT_API_KEYS;
    process.env.MERCHANT_API_KEYS = JSON.stringify({ "merchant-report-key": "m_1" });
    const mockDb = createMockDatabase();
    mockDb.state.reconciliationData = {
      payments: [
        {
          payment_id: "pay_report_1",
          amount: "1000000000",
          currency: "USDC",
          status: "SETTLED",
          created_at: "2026-10-02T12:00:00.000Z",
          settled_at: "2026-10-02T12:01:00.000Z",
          fee_amount: "10000000",
          fiat_amount: "16000.00",
          fiat_currency: "NGN",
          customer_ref: "order-1",
          tags: ["online"],
        },
        {
          payment_id: "pay_report_2",
          amount: "500000000",
          currency: "USDC",
          status: "SETTLED",
          created_at: "2026-10-03T12:00:00.000Z",
          settled_at: "2026-10-03T12:01:00.000Z",
          fee_amount: "5000000",
          fiat_amount: null,
          fiat_currency: null,
          customer_ref: null,
          tags: null,
        },
      ],
      settlements: [
        { payment_id: "pay_report_1", amount: "1000000000", currency: "USDC", fee_amount: "10000000", settled_at: "2026-10-02T12:01:00.000Z" },
        { payment_id: "pay_report_2", amount: "500000000", currency: "USDC", fee_amount: "5000000", settled_at: "2026-10-03T12:01:00.000Z" },
      ],
      refunds: [
        { refund_id: "refund_report_1", payment_id: "pay_report_1", amount: "100000000", currency: "USDC", status: "PROCESSED", created_at: "2026-10-04T12:00:00.000Z" },
      ],
      disputeAdjustments: [
        { dispute_id: "disp_report_1", payment_id: "pay_report_1", refund_id: "refund_report_1", amount: "-100000000", currency: "USDC", created_at: "2026-10-04T12:00:00.000Z" },
      ],
      fxRates: [
        { pair: "USDC_NGN", rate: "1600000000", decimals: 7 },
        { pair: "USDC_USD", rate: "10000000", decimals: 7 },
      ],
    };

    const app = createServer(mockDb);
    const server = app.listen(0);
    const address = server.address() as any;
    const url = `http://127.0.0.1:${address.port}/v1/reports/reconciliation?from=2026-10-01&to=2026-10-15&merchant_id=m_1`;

    try {
      const response = await fetch(url, { headers: { "x-api-key": "merchant-report-key" } });
      const report = await response.json() as any;
      assert.strictEqual(response.status, 200);
      assert.deepStrictEqual(report.period, { from: "2026-10-01", to: "2026-10-15" });
      assert.strictEqual(report.summary.total_gross_usdc, "150.00");
      assert.strictEqual(report.summary.total_fees_usdc, "1.50");
      assert.strictEqual(report.summary.total_refunds_usdc, "10.00");
      assert.strictEqual(report.summary.total_net_settled_usdc, "138.50");
      assert.strictEqual(report.summary.fiat_equivalent.NGN, "22160.00");
      assert.strictEqual(report.summary.fiat_equivalent.USD, "138.50");
      assert.strictEqual(report.summary.dispute_adjustments, "-10.00");
      assert.strictEqual(report.payments[0].fiat_equivalent.NGN, "16000.00");
      assert.strictEqual(report.settlements[0].net_amount, "99.00");

      const csvResponse = await fetch(url, {
        headers: { "x-api-key": "merchant-report-key", Accept: "text/csv" },
      });
      const csv = await csvResponse.text();
      assert.strictEqual(csvResponse.status, 200);
      assert.match(csvResponse.headers.get("content-type") || "", /text\/csv/);
      assert.match(csv, /record_type/);
      assert.match(csv, /pay_report_1/);

      const forbidden = await fetch(
        url.replace("merchant_id=m_1", "merchant_id=m_2"),
        { headers: { "x-api-key": "merchant-report-key" } }
      );
      assert.strictEqual(forbidden.status, 403);

      const invalidRange = await fetch(
        `http://127.0.0.1:${address.port}/v1/reports/reconciliation?from=2026-10-01&to=2026-11-01`,
        { headers: { "x-api-key": "merchant-report-key" } }
      );
      assert.strictEqual(invalidRange.status, 400);
    } finally {
      server.close();
      if (previousKeys === undefined) delete process.env.MERCHANT_API_KEYS;
      else process.env.MERCHANT_API_KEYS = previousKeys;
    }
  });

  it("should decode rate and dispute-adjustment events for report storage", () => {
    const subscriber = new EventSubscriber(
      loadConfigFromEnv({ CONTRACT_IDS: "C_TEST" }),
      createMockDatabase()
    );
    const fxEvent = subscriber.parseEvent({
      id: "evt_fx_report",
      timestamp: 1790942400,
      ledger: 10,
      txHash: "tx_fx",
      contractId: "C_FX",
      topic: ["FX_RATE", "UPDATED"],
      value: ["USDC_NGN", 1600000000n, 7],
    }) as any;
    const adjustmentEvent = subscriber.parseEvent({
      id: "evt_dispute_adjustment",
      timestamp: 1790942400,
      ledger: 11,
      txHash: "tx_dispute",
      contractId: "C_REFUND",
      topic: ["DISPUTE", "ADJUSTMENT"],
      value: ["disp_1", "pay_1", "refund_1", -100000000n],
    }) as any;
    const feeEvent = subscriber.parseEvent({
      id: "evt_fee_total",
      timestamp: 1790942400,
      ledger: 12,
      txHash: "tx_fee",
      contractId: "C_PAYMENT",
      topic: ["PAYMENT", "FEE_TOTAL", "pay_1"],
      value: 15000000n,
    }) as any;

    assert.deepStrictEqual(fxEvent.value, {
      pair: "USDC_NGN",
      rate: "1600000000",
      decimals: 7,
    });
    assert.deepStrictEqual(adjustmentEvent.value, {
      dispute_id: "disp_1",
      payment_id: "pay_1",
      refund_id: "refund_1",
      amount: "-100000000",
    });
    assert.deepStrictEqual(feeEvent.value, {
      payment_id: "pay_1",
      amount: "15000000",
    });
  });
});

describe("ISSUE #1039 — SSE broadcast for dispute status changes", () => {
  const config = {
    rpcUrl: "http://localhost:8000",
    contractIds: ["C_DISPUTE"],
    dbConnectionString: "postgres://localhost/test",
    pollInterval: 1000,
    startLedger: 1,
    dlqRetryIntervalMs: 1000,
    dlqMinAgeSeconds: 10,
    apiPort: 3001,
  };

  // Spies on the shared sseManager singleton for the duration of one test,
  // restoring the real `broadcast` afterward so other suites are unaffected.
  function spyOnBroadcast() {
    const calls: Array<{ merchantId: string; eventType: string; data: any }> = [];
    const original = sseManager.broadcast.bind(sseManager);
    sseManager.broadcast = (merchantId: string, eventType: string, data: any) => {
      calls.push({ merchantId, eventType, data });
    };
    return { calls, restore: () => { sseManager.broadcast = original; } };
  }

  it("broadcasts all 6 dispute lifecycle events, resolving merchant via payment_id", async () => {
    const mockDb = createMockDatabase();
    mockDb.state.payments.set("pay_1", {
      payment_id: "pay_1",
      merchant_id: "merch_1",
      amount: 5000,
      status: "Confirmed",
    });
    mockDb.state.disputes.set("disp_1", {
      dispute_id: "disp_1",
      payment_id: "pay_1",
      status: "Open",
    });

    const subscriber = new EventSubscriber(config, mockDb as any);
    const spy = spyOnBroadcast();

    try {
      await subscriber.processBatch([
        {
          id: "evt_1",
          ledger: 10,
          txHash: "tx1",
          contractId: "C_DISPUTE",
          topic: ["DISPUTE", "CREATED"],
          value: { dispute_id: "disp_1", payment_id: "pay_1", amount: 5000 },
        },
        {
          id: "evt_2",
          ledger: 11,
          txHash: "tx2",
          contractId: "C_DISPUTE",
          topic: ["DISPUTE", "REVIEWED"],
          value: { dispute_id: "disp_1", payment_id: "pay_1" },
        },
        {
          id: "evt_3",
          ledger: 12,
          txHash: "tx3",
          contractId: "C_DISPUTE",
          topic: ["DISPUTE", "ESCALATED"],
          value: { payment_id: "pay_1", dispute_id: "disp_1", amount: 5000 },
        },
        {
          id: "evt_4",
          ledger: 13,
          txHash: "tx4",
          contractId: "C_DISPUTE",
          topic: ["DISPUTE", "VOTE_CAST"],
          // No payment_id on this event — merchant must resolve via dispute_id.
          value: { dispute_id: "disp_1", arbitrator: "G_ARB", stake: 1000 },
        },
        {
          id: "evt_5",
          ledger: 14,
          txHash: "tx5",
          contractId: "C_DISPUTE",
          topic: ["DISPUTE", "RESOLVED"],
          value: { dispute_id: "disp_1", payment_id: "pay_1" },
        },
        {
          id: "evt_6",
          ledger: 15,
          txHash: "tx6",
          contractId: "C_DISPUTE",
          topic: ["DISPUTE", "AUTO_RESOLVED"],
          // No payment_id on this event either.
          value: { dispute_id: "disp_1", approved: true },
        },
      ] as any);

      assert.strictEqual(spy.calls.length, 6, "expected a broadcast for each of the 6 dispute events");
      assert.deepStrictEqual(
        spy.calls.map((c) => c.eventType),
        [
          "dispute.opened",
          "dispute.responded",
          "dispute.escalated",
          "dispute.vote_cast",
          "dispute.resolved",
          "dispute.auto_resolved",
        ],
      );
      // Every event resolves to the same merchant — including VOTE_CAST and
      // AUTO_RESOLVED, which carry only `dispute_id` and must be resolved via
      // the `disputes` -> `payments` lookup chain.
      for (const call of spy.calls) {
        assert.strictEqual(call.merchantId, "merch_1");
      }
    } finally {
      spy.restore();
    }
  });

  it("does not broadcast when the owning merchant cannot be resolved", async () => {
    const mockDb = createMockDatabase();
    const subscriber = new EventSubscriber(config, mockDb as any);
    const spy = spyOnBroadcast();

    try {
      await subscriber.processBatch([
        {
          id: "evt_orphan",
          ledger: 10,
          txHash: "tx1",
          contractId: "C_DISPUTE",
          topic: ["DISPUTE", "CREATED"],
          // dispute_id/payment_id with no matching payment indexed anywhere.
          value: { dispute_id: "disp_unknown", payment_id: "pay_unknown", amount: 100 },
        },
      ] as any);

      assert.strictEqual(spy.calls.length, 0);
    } finally {
      spy.restore();
    }
  });
});
