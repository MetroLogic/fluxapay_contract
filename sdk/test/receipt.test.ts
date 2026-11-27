/**
 * Issue #816: unit tests for payment receipt generation and verification.
 */
import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { Keypair } from "@stellar/stellar-sdk";

import {
  buildPaymentReceipt,
  verifyReceipt,
  formatReceiptAmount,
  buildReceiptUrl,
  DEFAULT_RECEIPT_BASE_URL,
} from "../src/receipt.js";

describe("payment receipt (Issue #816)", () => {
  const platform = Keypair.random();

  test("formatReceiptAmount formats stroops as decimal currency", () => {
    assert.equal(formatReceiptAmount(100_000_000n, "USDC"), "10 USDC");
    assert.equal(formatReceiptAmount(10_500_000n, "USDC"), "1.05 USDC");
    assert.equal(formatReceiptAmount(1n, "USDC"), "0.0000001 USDC");
  });

  test("receipt URL uses documented /r/{payment_id} format", () => {
    assert.equal(
      buildReceiptUrl("pay_abc"),
      `${DEFAULT_RECEIPT_BASE_URL}/r/pay_abc`,
    );
    assert.equal(
      buildReceiptUrl("pay_abc", "https://example.com/receipts/"),
      "https://example.com/receipts/r/pay_abc",
    );
  });

  test("generate + verify round-trip for a mock receipt", () => {
    const receipt = buildPaymentReceipt({
      paymentId: "pay_test_001",
      amountStroops: 100_000_000n,
      currency: "USDC",
      merchantName: "Acme Coffee",
      confirmedAt: "2026-03-15T12:00:00.000Z",
      txHash: "a".repeat(64),
      platformSecretKey: platform.secret(),
    });

    assert.equal(receipt.payment_id, "pay_test_001");
    assert.equal(receipt.amount, "10 USDC");
    assert.equal(receipt.merchant_name, "Acme Coffee");
    assert.equal(receipt.confirmed_at, "2026-03-15T12:00:00.000Z");
    assert.equal(receipt.tx_hash, "a".repeat(64));
    assert.ok(receipt.proof.length > 0);
    assert.equal(
      receipt.receipt_url,
      `${DEFAULT_RECEIPT_BASE_URL}/r/pay_test_001`,
    );

    assert.equal(verifyReceipt(receipt, platform.publicKey()), true);
  });

  test("verifyReceipt rejects tampered amount", () => {
    const receipt = buildPaymentReceipt({
      paymentId: "pay_tamper",
      amountStroops: 50_000_000n,
      currency: "USDC",
      merchantName: "Shop",
      confirmedAt: "2026-01-01T00:00:00.000Z",
      txHash: "b".repeat(64),
      platformSecretKey: platform.secret(),
    });

    const tampered = { ...receipt, amount: "99.00 USDC" };
    assert.equal(verifyReceipt(tampered, platform.publicKey()), false);
  });

  test("verifyReceipt rejects wrong platform public key", () => {
    const receipt = buildPaymentReceipt({
      paymentId: "pay_wrong_key",
      amountStroops: 1_000_000n,
      currency: "USDC",
      merchantName: "Shop",
      confirmedAt: "2026-01-01T00:00:00.000Z",
      txHash: "c".repeat(64),
      platformSecretKey: platform.secret(),
    });

    const other = Keypair.random();
    assert.equal(verifyReceipt(receipt, other.publicKey()), false);
  });
});
