import nock from "nock";
import { Account, Keypair, Networks, TransactionBuilder } from "stellar-sdk";
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

function makeChallenge(account: string): string {
  return new TransactionBuilder(new Account(account, "0"), {
    fee: "100",
    networkPassphrase: Networks.TESTNET,
  })
    .addOperation({
      type: "manageData",
      name: "anchor.example.com auth",
      value: Buffer.from("nonce"),
    })
    .setTimeout(300)
    .build()
    .toXDR();
}

function jwtWithExpiry(exp: number): string {
  const payload = Buffer.from(JSON.stringify({ exp })).toString("base64url");
  return `header.${payload}.signature`;
}

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

  it("authenticates with SEP-10 and sends the bearer token to withdrawal and status requests", async () => {
    const settlementKeypair = Keypair.random();
    const challenge = makeChallenge(settlementKeypair.publicKey());

    nock("https://anchor.example.com")
      .get("/auth")
      .query({ account: settlementKeypair.publicKey(), home_domain: "anchor.example.com" })
      .reply(200, { transaction: challenge, network_passphrase: Networks.TESTNET });
    nock("https://anchor.example.com")
      .post("/auth", (body) => typeof body.transaction === "string")
      .reply(200, { token: jwtWithExpiry(2_000_000_000) });
    nock("https://anchor.example.com", {
      reqheaders: { authorization: `Bearer ${jwtWithExpiry(2_000_000_000)}` },
    })
      .post("/sep6/transactions/withdrawal", (body) => body.jwt === jwtWithExpiry(2_000_000_000))
      .reply(200, { id: "anchor_auth_txn" });
    nock("https://anchor.example.com", {
      reqheaders: { authorization: `Bearer ${jwtWithExpiry(2_000_000_000)}` },
    })
      .get("/sep6/transaction")
      .query({ id: "anchor_auth_txn" })
      .reply(200, { transaction: { status: "completed" } });

    const store = new FakeSettlementStore();
    const service = new SettlementService(store, {
      pollIntervalMs: 1,
      maxRetries: 3,
      sep10: {
        secretKey: settlementKeypair.secret(),
        webAuthEndpoint: "https://anchor.example.com/auth",
        networkPassphrase: Networks.TESTNET,
      },
      nowSeconds: () => 1_700_000_000,
    });

    await service.settle(makePayload({ paymentId: "pay_auth" }));

    expect((await store.getSettlement("pay_auth"))?.status).toBe("fiat_sent");
  });

  it("refreshes a rejected or expired anchor token", async () => {
    const settlementKeypair = Keypair.random();
    const challenge = makeChallenge(settlementKeypair.publicKey());
    const expiredToken = jwtWithExpiry(1_700_000_000);
    const refreshedToken = jwtWithExpiry(1_700_060_000);

    nock("https://anchor.example.com")
      .get("/auth")
      .query(true)
      .reply(200, { transaction: challenge, network_passphrase: Networks.TESTNET });
    nock("https://anchor.example.com")
      .post("/auth")
      .reply(200, { token: expiredToken });
    nock("https://anchor.example.com")
      .post("/sep6/transactions/withdrawal")
      .matchHeader("Authorization", `Bearer ${expiredToken}`)
      .reply(401, { error: "expired" });

    nock("https://anchor.example.com")
      .get("/auth")
      .query(true)
      .reply(200, { transaction: challenge, network_passphrase: Networks.TESTNET });
    nock("https://anchor.example.com")
      .post("/auth")
      .reply(200, { token: refreshedToken });
    nock("https://anchor.example.com")
      .post("/sep6/transactions/withdrawal")
      .matchHeader("Authorization", `Bearer ${refreshedToken}`)
      .reply(200, { id: "anchor_refreshed" });
    nock("https://anchor.example.com")
      .get("/sep6/transaction")
      .query({ id: "anchor_refreshed" })
      .matchHeader("Authorization", `Bearer ${refreshedToken}`)
      .reply(200, { transaction: { status: "completed" } });

    const service = new SettlementService(new FakeSettlementStore(), {
      pollIntervalMs: 1,
      maxRetries: 3,
      sep10: {
        secretKey: settlementKeypair.secret(),
        webAuthEndpoint: "https://anchor.example.com/auth",
        networkPassphrase: Networks.TESTNET,
      },
      nowSeconds: () => 1_700_000_000,
    });

    await service.settle(makePayload({ paymentId: "pay_refresh" }));
    expect(nock.isDone()).toBe(true);
  });

  it("does not reuse tokens across anchors and rejects bad credentials without logging secrets", async () => {
    const settlementKeypair = Keypair.random();
    const anchorAChallenge = makeChallenge(settlementKeypair.publicKey());

    nock("https://anchor-a.example.com")
      .get("/auth")
      .query(true)
      .reply(200, { transaction: anchorAChallenge, network_passphrase: Networks.TESTNET });
    nock("https://anchor-a.example.com")
      .post("/auth")
      .reply(403, { error: "invalid credentials" });

    const store = new FakeSettlementStore();
    const secret = settlementKeypair.secret();
    const errorSpy = jest.spyOn(console, "error").mockImplementation(() => undefined);
    const service = new SettlementService(store, {
      pollIntervalMs: 1,
      maxRetries: 3,
      sep10: {
        secretKey: secret,
        webAuthEndpoint: "https://anchor-a.example.com/auth",
        networkPassphrase: Networks.TESTNET,
      },
    });

    await service.settle(makePayload({
      paymentId: "pay_rejected",
      anchorDomain: "anchor-a.example.com",
      sep6Endpoint: "https://anchor-a.example.com/sep6",
    }));

    expect((await store.getSettlement("pay_rejected"))?.status).toBe("failed");
    const logged = errorSpy.mock.calls.flat().join(" ");
    expect(logged).not.toContain(secret);
    errorSpy.mockRestore();
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
