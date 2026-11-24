/**
 * Issue #814: unit tests for `getPaymentStatuses`.
 *
 * Uses Node's built-in test runner, matching `error-mapping.test.ts` — no extra
 * test-framework dependency.
 *
 * The client is exercised through a stubbed `getPayment` rather than a live RPC.
 * That is the seam that matters: the method's whole job is fanning out reads and
 * folding not-found into `null`, and an RPC in the loop would make this a
 * network-availability check.
 */
import { test, describe } from "node:test";
import assert from "node:assert/strict";

import {
  BatchTooLargeError,
  FluxapayClient,
  FluxapayError,
  MAX_BATCH_STATUS_IDS,
} from "../src/index.js";

interface StubClient {
  calls: string[];
  getPaymentStatuses(ids: string[]): Promise<Map<string, unknown>>;
}

/**
 * Exercises the real `getPaymentStatuses` over a scripted `getPayment`.
 *
 * The method is borrowed off `FluxapayClient.prototype` and bound to a stub, so
 * the code under test is the shipped implementation rather than a copy of it —
 * and no contract client, RPC, or network is needed to construct one.
 */
function makeClient(responses: Record<string, unknown>): StubClient {
  const calls: string[] = [];

  const stub = {
    calls,
    async getPayment(id: string) {
      calls.push(id);
      const entry = responses[id];
      if (typeof entry === "function") {
        return (entry as () => never)();
      }
      if (entry === undefined) {
        throw new FluxapayError(404, "PaymentNotFound", "no such payment");
      }
      return entry;
    },
  } as unknown as StubClient & { getPayment(id: string): Promise<unknown> };

  stub.getPaymentStatuses = (
    FluxapayClient.prototype as unknown as StubClient
  ).getPaymentStatuses.bind(stub);

  return stub;
}

const payment = (status: string) => ({ payment_id: "x", status });

describe("getPaymentStatuses", () => {
  test("returns a Map keyed by payment ID", async () => {
    const client = makeClient({
      pay_001: payment("Confirmed"),
      pay_002: payment("Pending"),
    });

    const result = await client.getPaymentStatuses(["pay_001", "pay_002"]);

    assert.equal(result.size, 2);
    assert.equal(result.get("pay_001"), "Confirmed");
    assert.equal(result.get("pay_002"), "Pending");
  });

  test("maps a not-found payment to null rather than failing the batch", async () => {
    // A merchant checking 50 orders should not lose the other 49 because one ID
    // was mistyped.
    const client = makeClient({
      pay_001: payment("Confirmed"),
      // pay_404 absent → the stub throws PaymentNotFound
      pay_003: payment("Expired"),
    });

    const result = await client.getPaymentStatuses(["pay_001", "pay_404", "pay_003"]);

    assert.equal(result.size, 3);
    assert.equal(result.get("pay_001"), "Confirmed");
    assert.equal(result.get("pay_404"), null);
    assert.equal(result.get("pay_003"), "Expired");
  });

  test("parses a mixed found / not-found response set", async () => {
    const client = makeClient({
      a: payment("Pending"),
      c: payment("Confirmed"),
    });

    const result = await client.getPaymentStatuses(["a", "b", "c", "d"]);

    assert.deepEqual(
      [...result.entries()].sort(),
      [
        ["a", "Pending"],
        ["b", null],
        ["c", "Confirmed"],
        ["d", null],
      ].sort(),
    );
  });

  test("throws BatchTooLargeError above the limit, before any request", async () => {
    const client = makeClient({});
    const ids = Array.from({ length: MAX_BATCH_STATUS_IDS + 1 }, (_, i) => `p${i}`);

    await assert.rejects(() => client.getPaymentStatuses(ids), BatchTooLargeError);
    // The cap is enforced client-side; nothing should have gone out.
    assert.equal(client.calls.length, 0);
  });

  test("accepts exactly the limit", async () => {
    const ids = Array.from({ length: MAX_BATCH_STATUS_IDS }, (_, i) => `p${i}`);
    const client = makeClient(
      Object.fromEntries(ids.map((id) => [id, payment("Pending")])),
    );

    const result = await client.getPaymentStatuses(ids);
    assert.equal(result.size, MAX_BATCH_STATUS_IDS);
  });

  test("returns an empty Map for an empty input without any request", async () => {
    const client = makeClient({});
    const result = await client.getPaymentStatuses([]);

    assert.equal(result.size, 0);
    assert.equal(client.calls.length, 0);
  });

  test("collapses duplicate IDs into one read", async () => {
    const client = makeClient({ pay_001: payment("Confirmed") });

    const result = await client.getPaymentStatuses(["pay_001", "pay_001", "pay_001"]);

    assert.equal(result.size, 1);
    assert.equal(client.calls.length, 1);
  });

  test("rethrows a transport failure rather than reporting every ID missing", async () => {
    // Mapping an RPC outage to "these orders do not exist" would be far worse
    // than surfacing the error.
    const client = makeClient({
      pay_001: () => {
        throw new Error("ECONNREFUSED: rpc unreachable");
      },
    });

    await assert.rejects(
      () => client.getPaymentStatuses(["pay_001"]),
      /ECONNREFUSED/,
    );
  });

  test("treats a contract PaymentNotFound code as not-found", async () => {
    const client = makeClient({
      pay_001: () => {
        throw new FluxapayError(404, "PaymentNotFound");
      },
    });

    const result = await client.getPaymentStatuses(["pay_001"]);
    assert.equal(result.get("pay_001"), null);
  });

  test("unwraps a simulation-style { result } envelope", async () => {
    const client = makeClient({
      pay_001: { result: { payment_id: "pay_001", status: "Confirmed" } },
    });

    const result = await client.getPaymentStatuses(["pay_001"]);
    assert.equal(result.get("pay_001"), "Confirmed");
  });

  test("handles an enum-shaped status", async () => {
    const client = makeClient({
      pay_001: { status: { tag: "PendingApproval", values: undefined } },
    });

    const result = await client.getPaymentStatuses(["pay_001"]);
    assert.deepEqual(result.get("pay_001"), { tag: "PendingApproval", values: undefined });
  });

  test("reports null when the payload carries no status field", async () => {
    const client = makeClient({ pay_001: { payment_id: "pay_001" } });

    const result = await client.getPaymentStatuses(["pay_001"]);
    assert.equal(result.get("pay_001"), null);
  });
});
