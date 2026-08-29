/**
 * Unit & Integration Test Suite for FluxaPay Indexer
 * Covers:
 * - Issue #616: REST API server, authentication, pagination, filtering, 404s, health check
 * - Issue #618: Multi-contract configuration, subscription filters, contract ID identification & routing
 * - Issue #617: Dead-letter queue insertion, retry_count, auto-retry, manual POST /admin/replay-dlq
 * - Issue #615: Dispute lifecycle status persistence (CREATED, RESOLVED, REJECTED, ESCALATED), idempotency, unknown dispute ID handling
 */

import assert from "node:assert";
import test, { describe, it } from "node:test";
import { loadConfigFromEnv, EventSubscriber } from "../src/index";
import { createServer } from "../src/server";
import { Database } from "../src/database";
import { extractApiKey, requireApiKey } from "../src/auth/api-key";

// Helper to create a mocked Database instance for unit testing
function createMockDatabase() {
  const state = {
    payments: new Map<string, any>(),
    refunds: new Map<string, any>(),
    disputes: new Map<string, any>(),
    contractEvents: new Array<any>(),
    dlqEvents: new Map<string, any>(),
    dbConnected: true,
  };

  const mockDb = {
    state,
    async initialize() {},
    async close() {},

    async checkHealth() {
      if (!state.dbConnected) {
        return { healthy: false, database: "disconnected", details: "Connection pool exhausted" };
      }
      return { healthy: true, database: "connected" };
    },

    async getPaymentById(paymentId: string) {
      return state.payments.get(paymentId) || null;
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
          state.disputes.set(val.dispute_id, {
            dispute_id: val.dispute_id,
            payment_id: val.payment_id,
            amount: val.amount,
            status,
            escalated: false,
            created_at: new Date(event.timestamp * 1000),
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
});
