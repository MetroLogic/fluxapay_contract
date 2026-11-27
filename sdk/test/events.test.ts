/**
 * Unit tests for SDK Event Type Exports & parseFluxapayEvent (Issue #765)
 */

import assert from "node:assert";
import test, { describe, it } from "node:test";
import { parseFluxapayEvent, events } from "../src/index.js";

describe("Issue #765: SDK Event Type Exports & parseFluxapayEvent", () => {
  it("parses and discriminates a raw PAYMENT/CREATED event", () => {
    const rawEvent = {
      contractId: "CAAA_PAYMENT_PROCESSOR",
      ledger: 12345,
      txHash: "tx_hash_payment_created",
      timestamp: 1700000000,
      topic: ["PAYMENT", "CREATED", "pay_001"],
      value: {
        payment_id: "pay_001",
        merchant_id: "G_MERCHANT_1",
        amount: "5000000",
        metadata: { order_id: "ORD-999" },
      },
    };

    const parsed = parseFluxapayEvent(rawEvent);

    assert.strictEqual(parsed.type, "PAYMENT/CREATED");
    assert.strictEqual(parsed.namespace, "PAYMENT");
    assert.strictEqual(parsed.action, "CREATED");
    assert.strictEqual(parsed.contractId, "CAAA_PAYMENT_PROCESSOR");
    assert.strictEqual(parsed.ledger, 12345);
    assert.strictEqual(parsed.txHash, "tx_hash_payment_created");

    if (parsed.type === "PAYMENT/CREATED") {
      assert.strictEqual(parsed.payload.payment_id, "pay_001");
      assert.strictEqual(parsed.payload.merchant_id, "G_MERCHANT_1");
      assert.strictEqual(parsed.payload.amount, 5000000n);
      assert.deepStrictEqual(parsed.payload.metadata, { order_id: "ORD-999" });
    } else {
      assert.fail("Failed to narrow to PaymentCreatedEvent");
    }
  });

  it("parses and discriminates a raw REFUND/COMPLETED event", () => {
    const rawEvent = {
      contractId: "CBBB_REFUND_MANAGER",
      ledger: 12346,
      txHash: "tx_hash_refund_completed",
      timestamp: 1700000100,
      topic: ["REFUND", "COMPLETED"],
      value: {
        payment_id: "pay_001",
        refund_id: "ref_001",
        refund_amount: 2500000,
      },
    };

    const parsed = parseFluxapayEvent(rawEvent);

    assert.strictEqual(parsed.type, "REFUND/COMPLETED");
    if (parsed.type === "REFUND/COMPLETED") {
      assert.strictEqual(parsed.payload.payment_id, "pay_001");
      assert.strictEqual(parsed.payload.refund_id, "ref_001");
      assert.strictEqual(parsed.payload.refund_amount, 2500000n);
    } else {
      assert.fail("Failed to narrow to RefundCompletedEvent");
    }
  });

  it("parses and discriminates a raw STREAM/WITHDRAWN event with memo", () => {
    const rawEvent = {
      contractId: "CAAA_PAYMENT_PROCESSOR",
      ledger: 12347,
      txHash: "tx_hash_stream_withdrawn",
      timestamp: 1700000200,
      topic: ["STREAM", "WITHDRAWN", "stream_42"],
      value: {
        stream_id: "stream_42",
        receiver: "G_RECEIVER",
        destination: "G_DEST",
        amount: "1500000",
        remaining_deposit: "8500000",
        memo: "payroll-october",
      },
    };

    const parsed = parseFluxapayEvent(rawEvent);

    assert.strictEqual(parsed.type, "STREAM/WITHDRAWN");
    if (parsed.type === "STREAM/WITHDRAWN") {
      assert.strictEqual(parsed.payload.stream_id, "stream_42");
      assert.strictEqual(parsed.payload.receiver, "G_RECEIVER");
      assert.strictEqual(parsed.payload.destination, "G_DEST");
      assert.strictEqual(parsed.payload.amount, 1500000n);
      assert.strictEqual(parsed.payload.remaining_deposit, 8500000n);
      assert.strictEqual(parsed.payload.memo, "payroll-october");
    } else {
      assert.fail("Failed to narrow to StreamWithdrawnEvent");
    }
  });

  it("parses and discriminates an ACCESS_CONTROL/ADMIN_TRANSFER_PROPOSED event", () => {
    const rawEvent = {
      contractId: "CBBB_REFUND_MANAGER",
      ledger: 12348,
      topic: ["ACCESS_CONTROL", "ADMIN_TRANSFER_PROPOSED"],
      value: {
        new_admin: "G_NEW_ADMIN",
        earliest_acceptance_ledger: 29628,
      },
    };

    const parsed = parseFluxapayEvent(rawEvent);

    assert.strictEqual(parsed.type, "ACCESS_CONTROL/ADMIN_TRANSFER_PROPOSED");
    if (parsed.type === "ACCESS_CONTROL/ADMIN_TRANSFER_PROPOSED") {
      assert.strictEqual(parsed.payload.new_admin, "G_NEW_ADMIN");
      assert.strictEqual(parsed.payload.earliest_acceptance_ledger, 29628);
    } else {
      assert.fail("Failed to narrow to AdminTransferProposedEvent");
    }
  });

  it("exports parseFluxapayEvent directly under events namespace", () => {
    assert.strictEqual(typeof events.parseFluxapayEvent, "function");
  });
});
