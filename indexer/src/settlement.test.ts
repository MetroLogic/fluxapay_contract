import nock from "nock";
import {
  AnchorWithdrawPayload,
  SettlementRecord,
  SettlementService,
  SettlementStore,
} from "./settlement";

/** In-memory stand-in for `Database` so tests don't need a real Postgres. */
class FakeSettlementStore implements SettlementStore {
  public records = new Map<string, SettlementRecord>();

  async getSettlement(paymentId: string): Promise<SettlementRecord | null> {
    return this.records.get(paymentId) ?? null;
  }

  async upsertSettlement(record: SettlementRecord): Promise<void> {
    this.records.set(record.paymentId, record);
  }
}

const ANCHOR_BASE = "https://anchor.example.com/sep6";

function makePayload(overrides: Partial<AnchorWithdrawPayload> = {}): AnchorWithdrawPayload {
  return {
    paymentId: "pay_abc123",
    merchantId: "GMERCHANT",
    amount: 15000,
    currency: "USD",
    payoutAddress: "GPAYOUTADDRESS",
    anchorDomain: "anchor.example.com",
    sep6Endpoint: ANCHOR_BASE,
    sep24Endpoint: "",
    supportedCurrencies: ["USD"],
    ledgerTimestamp: 1_700_000_000,
    ...overrides,
  };
}

describe("SettlementService", () => {
  afterEach(() => {
    nock.cleanAll();
  });

  it("marks a settlement fiat_sent when the anchor reports completed", async () => {
    nock("https://anchor.example.com")
      .post("/sep6/transactions/withdrawal")
      .reply(200, { id: "anchor_txn_1" });

    nock("https://anchor.example.com")
      .get("/sep6/transaction")
      .query({ id: "anchor_txn_1" })
      .reply(200, { transaction: { status: "completed" } });

    const store = new FakeSettlementStore();
    const service = new SettlementService(store, { pollIntervalMs: 1, maxRetries: 3 });
    const events: string[] = [];
    service.onSettlementEvent((e) => events.push(e.type));

    await service.settle(makePayload());

    const record = await store.getSettlement("pay_abc123");
    expect(record?.status).toBe("fiat_sent");
    expect(record?.anchorTxnId).toBe("anchor_txn_1");
    expect(events).toContain("SETTLEMENT/FIAT_SENT");
  });

  it("marks a settlement failed when the anchor reports a terminal error status", async () => {
    nock("https://anchor.example.com")
      .post("/sep6/transactions/withdrawal")
      .reply(200, { id: "anchor_txn_2" });

    nock("https://anchor.example.com")
      .get("/sep6/transaction")
      .query({ id: "anchor_txn_2" })
      .reply(200, { transaction: { status: "error" } });

    const store = new FakeSettlementStore();
    const service = new SettlementService(store, { pollIntervalMs: 1, maxRetries: 3 });
    const events: string[] = [];
    service.onSettlementEvent((e) => events.push(e.type));

    await service.settle(makePayload({ paymentId: "pay_failcase" }));

    const record = await store.getSettlement("pay_failcase");
    expect(record?.status).toBe("failed");
    expect(events).toContain("SETTLEMENT/FIAT_FAILED");
  });

  it("keeps a settlement pending after polling times out without an authoritative terminal status", async () => {
    nock("https://anchor.example.com")
      .post("/sep6/transactions/withdrawal")
      .reply(200, { id: "anchor_txn_3" });

    nock("https://anchor.example.com")
      .get("/sep6/transaction")
      .query({ id: "anchor_txn_3" })
      .times(3)
      .reply(200, { transaction: { status: "pending_anchor" } });

    const store = new FakeSettlementStore();
    const service = new SettlementService(store, { pollIntervalMs: 1, maxRetries: 3 });

    await service.settle(makePayload({ paymentId: "pay_stuck" }));

    const record = await store.getSettlement("pay_stuck");
    expect(record?.status).toBe("pending");
    expect(record?.anchorTxnId).toBe("anchor_txn_3");
  });

  it("is idempotent: skips a payment whose settlement already reached a terminal state", async () => {
    const store = new FakeSettlementStore();
    await store.upsertSettlement({
      paymentId: "pay_done",
      merchantId: "GMERCHANT",
      anchorDomain: "anchor.example.com",
      anchorTxnId: "anchor_txn_old",
      amount: 1000,
      currency: "USD",
      status: "fiat_sent",
    });

    const service = new SettlementService(store, { pollIntervalMs: 1, maxRetries: 3 });

    // No nock interceptors registered — a network call here would throw.
    await service.settle(makePayload({ paymentId: "pay_done" }));

    const record = await store.getSettlement("pay_done");
    expect(record?.anchorTxnId).toBe("anchor_txn_old");
  });

  it("returns a persisted SEP-24 handoff when no SEP-6 endpoint is configured", async () => {
    nock("https://anchor.example.com")
      .post("/sep24/transactions/withdraw/interactive")
      .reply(200, {
        id: "anchor_txn_4",
        url: "https://anchor.example.com/interactive?token=session-token",
      });

    const store = new FakeSettlementStore();
    const service = new SettlementService(store, { pollIntervalMs: 1, maxRetries: 3 });

    await service.settle(
      makePayload({
        paymentId: "pay_sep24",
        sep6Endpoint: "",
        sep24Endpoint: "https://anchor.example.com/sep24",
      })
    );

    const record = await store.getSettlement("pay_sep24");
    expect(record?.status).toBe("awaiting_user");
    expect(record?.anchorTxnId).toBe("anchor_txn_4");
    expect(record?.interactiveUrl).toBe(
      "https://anchor.example.com/interactive?token=session-token"
    );
    expect(record?.sep24Endpoint).toBe("https://anchor.example.com/sep24");
  });

  it("does not persist an unsafe SEP-24 interactive URL", async () => {
    nock("https://anchor.example.com")
      .post("/sep24/transactions/withdraw/interactive")
      .reply(200, {
        id: "anchor_txn_unsafe",
        url: "http://anchor.example.com/withdraw/session",
      });

    const store = new FakeSettlementStore();
    const service = new SettlementService(store, { pollIntervalMs: 1, maxRetries: 1 });
    await service.settle(
      makePayload({
        paymentId: "pay_sep24_unsafe",
        sep6Endpoint: "",
        sep24Endpoint: "https://anchor.example.com/sep24",
      })
    );

    const record = await store.getSettlement("pay_sep24_unsafe");
    expect(record?.status).toBe("pending");
    expect(record?.interactiveUrl).toBeUndefined();
  });

  it("resumes SEP-24 polling and marks fiat_sent only after anchor completion", async () => {
    nock("https://anchor.example.com")
      .post("/sep24/transactions/withdraw/interactive")
      .reply(200, {
        id: "anchor_txn_complete",
        url: "https://anchor.example.com/interactive/complete",
      });
    nock("https://anchor.example.com")
      .get("/sep24/transaction")
      .query({ id: "anchor_txn_complete" })
      .reply(200, { transaction: { status: "completed" } });

    const store = new FakeSettlementStore();
    const service = new SettlementService(store, { pollIntervalMs: 1, maxRetries: 1 });
    const events: string[] = [];
    service.onSettlementEvent((event) => events.push(event.type));
    const payload = makePayload({
      paymentId: "pay_sep24_complete",
      sep6Endpoint: "",
      sep24Endpoint: "https://anchor.example.com/sep24",
    });

    await service.settle(payload);
    const completed = await service.resume(payload.paymentId);

    expect(completed?.status).toBe("fiat_sent");
    expect(events).toContain("SETTLEMENT/FIAT_SENT");
  });

  it("marks a SEP-24 settlement failed when the anchor reports cancellation", async () => {
    nock("https://anchor.example.com")
      .post("/sep24/transactions/withdraw/interactive")
      .reply(200, {
        id: "anchor_txn_cancelled",
        url: "https://anchor.example.com/interactive/cancelled",
      });
    nock("https://anchor.example.com")
      .get("/sep24/transaction")
      .query({ id: "anchor_txn_cancelled" })
      .reply(200, { transaction: { status: "cancelled" } });

    const store = new FakeSettlementStore();
    const service = new SettlementService(store, { pollIntervalMs: 1, maxRetries: 1 });
    const payload = makePayload({
      paymentId: "pay_sep24_cancelled",
      sep6Endpoint: "",
      sep24Endpoint: "https://anchor.example.com/sep24",
    });

    await service.settle(payload);
    const cancelled = await service.resume(payload.paymentId);

    expect(cancelled?.status).toBe("failed");
  });

  it("marks a SEP-24 settlement failed when the anchor reports a terminal error", async () => {
    nock("https://anchor.example.com")
      .post("/sep24/transactions/withdraw/interactive")
      .reply(200, {
        id: "anchor_txn_error",
        url: "https://anchor.example.com/interactive/error",
      });
    nock("https://anchor.example.com")
      .get("/sep24/transaction")
      .query({ id: "anchor_txn_error" })
      .reply(200, { transaction: { status: "error" } });

    const store = new FakeSettlementStore();
    const service = new SettlementService(store, { pollIntervalMs: 1, maxRetries: 1 });
    const payload = makePayload({
      paymentId: "pay_sep24_error",
      sep6Endpoint: "",
      sep24Endpoint: "https://anchor.example.com/sep24",
    });

    await service.settle(payload);
    const failed = await service.resume(payload.paymentId);

    expect(failed?.status).toBe("failed");
  });

  it("falls back to SEP-24 when the SEP-6 transaction requires customer information", async () => {
    nock("https://anchor.example.com")
      .post("/sep6/transactions/withdrawal")
      .reply(200, { id: "anchor_txn_incomplete" });
    nock("https://anchor.example.com")
      .get("/sep6/transaction")
      .query({ id: "anchor_txn_incomplete" })
      .reply(200, { transaction: { status: "incomplete" } });
    nock("https://anchor.example.com")
      .post("/sep24/transactions/withdraw/interactive")
      .reply(200, {
        id: "anchor_txn_interactive",
        url: "https://anchor.example.com/interactive",
      });

    const store = new FakeSettlementStore();
    const service = new SettlementService(store, { pollIntervalMs: 1, maxRetries: 1 });
    await service.settle(
      makePayload({
        paymentId: "pay_sep6_incomplete",
        sep24Endpoint: "https://anchor.example.com/sep24",
      })
    );

    const record = await store.getSettlement("pay_sep6_incomplete");
    expect(record?.status).toBe("awaiting_user");
    expect(record?.anchorTxnId).toBe("anchor_txn_interactive");
  });
});
