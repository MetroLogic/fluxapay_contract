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

  it("marks a settlement failed after exhausting retries on a stuck pending_anchor status", async () => {
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
    expect(record?.status).toBe("failed");
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

  it("falls back to the SEP-24 endpoint when sep6Endpoint is not set", async () => {
    nock("https://anchor.example.com")
      .post("/sep24/transactions/withdrawal")
      .reply(200, { id: "anchor_txn_4" });

    nock("https://anchor.example.com")
      .get("/sep24/transaction")
      .query({ id: "anchor_txn_4" })
      .reply(200, { transaction: { status: "completed" } });

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
    expect(record?.status).toBe("fiat_sent");
  });
});
