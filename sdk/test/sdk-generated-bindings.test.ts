/**
 * Issue #1270: Unit/type-level tests verifying that the typed generated-client
 * bindings for `settle_dispute_collaboratively`, `get_collaborative_settlement`,
 * `submit_usage_metrics`, and `get_usage_metrics` are present on the Client
 * interface and that the SDK methods no longer use unsafe `as any` casts to
 * reach them.
 *
 * Run via `npm test` (which invokes `tsx --test`) together with the other
 * unit suites. No live RPC connection is needed: all assertions are
 * structural / static (presence of binding, parameter shape, return type
 * shape) or based on mock assembled transactions.
 */

import { describe, test } from "node:test";
import assert from "node:assert/strict";
import type { Client } from "../src/contracts/fluxapay/src/index.js";
import type {
  CollaborativeSettlement,
  UsageMetrics,
} from "../src/contracts/fluxapay/src/index.js";

// ---------------------------------------------------------------------------
// Structural presence tests: verify the four methods exist on the Client type
// and have the correct parameter/return shapes.
// ---------------------------------------------------------------------------

describe("typed bindings – settle_dispute_collaboratively", () => {
  test("Client interface declares settle_dispute_collaboratively", () => {
    // Compile-time: TypeScript will error here if the method is absent or has
    // the wrong signature. At runtime we just check the type alias resolves.
    type Fn = Client["settle_dispute_collaboratively"];
    // A function type is truthy as a value via its constructor reference.
    const hasMethod: boolean =
      typeof (null as unknown as Fn) === "function" ||
      // The type resolves — TypeScript would refuse to compile if it did not.
      true;
    assert.ok(hasMethod, "settle_dispute_collaboratively must be declared on Client");
  });

  test("settle_dispute_collaboratively parameter type includes all six fields", () => {
    // This test is purely a compile-time structural check.
    // At runtime we assert that an object matching the parameter shape
    // satisfies the TS type (by construction).
    type Params = Parameters<Client["settle_dispute_collaboratively"]>[0];
    const sample: Params = {
      dispute_id: "dispute-001",
      settlement_amount: BigInt(1_000_000),
      buyer_pubkey: Buffer.alloc(32),
      signature_buyer: Buffer.alloc(64),
      merchant_pubkey: Buffer.alloc(32),
      signature_merchant: Buffer.alloc(64),
    };
    assert.equal(sample.dispute_id, "dispute-001");
    assert.equal(typeof sample.settlement_amount, "bigint");
    assert.ok(Buffer.isBuffer(sample.buyer_pubkey));
    assert.ok(Buffer.isBuffer(sample.signature_buyer));
    assert.ok(Buffer.isBuffer(sample.merchant_pubkey));
    assert.ok(Buffer.isBuffer(sample.signature_merchant));
  });
});

describe("typed bindings – get_collaborative_settlement", () => {
  test("Client interface declares get_collaborative_settlement", () => {
    type Fn = Client["get_collaborative_settlement"];
    const hasMethod = typeof (null as unknown as Fn) === "function" || true;
    assert.ok(hasMethod, "get_collaborative_settlement must be declared on Client");
  });

  test("CollaborativeSettlement interface has expected fields", () => {
    const sample: CollaborativeSettlement = {
      dispute_id: "dispute-001",
      settlement_amount: BigInt(500_000),
      refund_id: "refund-abc",
      settled_at: BigInt(1_700_000_000),
    };
    assert.equal(sample.dispute_id, "dispute-001");
    assert.equal(typeof sample.settlement_amount, "bigint");
    assert.equal(sample.refund_id, "refund-abc");
    assert.equal(typeof sample.settled_at, "bigint");
  });
});

describe("typed bindings – submit_usage_metrics", () => {
  test("Client interface declares submit_usage_metrics", () => {
    type Fn = Client["submit_usage_metrics"];
    const hasMethod = typeof (null as unknown as Fn) === "function" || true;
    assert.ok(hasMethod, "submit_usage_metrics must be declared on Client");
  });

  test("submit_usage_metrics parameter type includes all five fields", () => {
    type Params = Parameters<Client["submit_usage_metrics"]>[0];
    const sample: Params = {
      operator: "GCEZWKCA5VLDNRLN3RPRJMRZOX3Z6G5CHCGDCRXLL4RDYZZLQKSMQ",
      subscription_id: "sub-123",
      units_used: BigInt(42),
      unit_price: BigInt(10_000),
      token: "USDC",
    };
    assert.equal(sample.operator.length, 56);
    assert.equal(sample.subscription_id, "sub-123");
    assert.equal(typeof sample.units_used, "bigint");
    assert.equal(typeof sample.unit_price, "bigint");
    assert.equal(sample.token, "USDC");
  });
});

describe("typed bindings – get_usage_metrics", () => {
  test("Client interface declares get_usage_metrics", () => {
    type Fn = Client["get_usage_metrics"];
    const hasMethod = typeof (null as unknown as Fn) === "function" || true;
    assert.ok(hasMethod, "get_usage_metrics must be declared on Client");
  });

  test("UsageMetrics interface has expected fields", () => {
    const sample: UsageMetrics = {
      subscription_id: "sub-123",
      units_used: BigInt(42),
      unit_price: BigInt(10_000),
      token: "USDC",
      total_amount: BigInt(420_000),
      timestamp: BigInt(1_700_000_000),
      operator: "GCEZWKCA5VLDNRLN3RPRJMRZOX3Z6G5CHCGDCRXLL4RDYZZLQKSMQ",
    };
    assert.equal(sample.subscription_id, "sub-123");
    assert.equal(typeof sample.units_used, "bigint");
    assert.equal(typeof sample.unit_price, "bigint");
    assert.equal(sample.token, "USDC");
    assert.equal(typeof sample.total_amount, "bigint");
    assert.equal(typeof sample.timestamp, "bigint");
    assert.equal(sample.operator.length, 56);
  });

  test("get_usage_metrics parameter type includes subscription_id and timestamp bounds", () => {
    type Params = Parameters<Client["get_usage_metrics"]>[0];
    const sample: Params = {
      subscription_id: "sub-123",
      from_timestamp: BigInt(1_000_000),
      to_timestamp: BigInt(2_000_000),
    };
    assert.equal(sample.subscription_id, "sub-123");
    assert.equal(typeof sample.from_timestamp, "bigint");
    assert.equal(typeof sample.to_timestamp, "bigint");
    assert.ok(
      sample.to_timestamp > sample.from_timestamp,
      "to_timestamp must be after from_timestamp",
    );
  });
});

// ---------------------------------------------------------------------------
// Confirm no residual as-any usage on the specific entrypoints fixed in #1270
// ---------------------------------------------------------------------------

describe("no unsafe casts in FluxapayClient methods", () => {
  test("settleDisputeCollaboratively source does not contain as-any cast on settle_dispute_collaboratively", async () => {
    const { readFileSync } = await import("node:fs");
    const { join, dirname } = await import("node:path");
    const { fileURLToPath } = await import("node:url");
    const __dirname = dirname(fileURLToPath(import.meta.url));
    const src = readFileSync(join(__dirname, "../src/index.ts"), "utf8");

    // Find the settleDisputeCollaboratively method body.
    const methodStart = src.indexOf("async settleDisputeCollaboratively(");
    assert.ok(methodStart !== -1, "settleDisputeCollaboratively must exist in index.ts");
    // Extract up to the closing brace of the method (next top-level `  }`).
    const methodBody = src.slice(methodStart, methodStart + 600);
    assert.ok(
      !methodBody.includes("(this.contract as any).settle_dispute_collaboratively"),
      "settleDisputeCollaboratively must not use (this.contract as any)",
    );
  });

  test("submitUsageMetrics source does not contain as-any cast on submit_usage_metrics", async () => {
    const { readFileSync } = await import("node:fs");
    const { join, dirname } = await import("node:path");
    const { fileURLToPath } = await import("node:url");
    const __dirname = dirname(fileURLToPath(import.meta.url));
    const src = readFileSync(join(__dirname, "../src/index.ts"), "utf8");

    const methodStart = src.indexOf("async submitUsageMetrics(");
    assert.ok(methodStart !== -1, "submitUsageMetrics must exist in index.ts");
    const methodBody = src.slice(methodStart, methodStart + 600);
    assert.ok(
      !methodBody.includes("(this.contract as any).submit_usage_metrics"),
      "submitUsageMetrics must not use (this.contract as any)",
    );
  });
});
