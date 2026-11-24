/**
 * Issue #834: Node.js subpath export smoke test.
 *
 * Imports `@fluxapay/sdk` via the relative dist/node.js entry (same as the
 * package.json `exports["./node"]` target) and asserts the public client
 * constructors are available without caller-side fetch/TLS setup.
 */
import { describe, it } from "node:test";
import assert from "node:assert/strict";

describe("@fluxapay/sdk/node entry point", () => {
  it("exports FluxapayClient and related clients", async () => {
    const mod = await import("../dist/node.js");
    assert.equal(typeof mod.FluxapayClient, "function");
    assert.equal(typeof mod.RefundManagerClient, "function");
    assert.equal(typeof mod.MerchantRegistryClient, "function");
  });

  it("runs on Node.js 18+ with global fetch available", () => {
    assert.equal(typeof globalThis.fetch, "function");
    const major = Number(process.versions.node.split(".")[0]);
    assert.ok(major >= 18, `expected Node >= 18, got ${process.versions.node}`);
  });
});
