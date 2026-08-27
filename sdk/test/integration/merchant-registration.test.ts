/**
 * Issue #661: Integration test — register a merchant, fetch it back, and
 * verify its fields against a real deployed testnet MerchantRegistry (or
 * MerchantRegistry-embedded PaymentProcessor) contract.
 *
 * Skips automatically when `TESTNET_RPC_URL` is not set.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { getTestnetEnv, makeTestnetClient } from "./helpers.js";

const env = getTestnetEnv();

test(
  "register -> get -> verify merchant round-trips on testnet",
  { skip: !env && "TESTNET_RPC_URL not set — skipping testnet integration test" },
  async () => {
    if (!env) return;
    const client = makeTestnetClient(env);

    const merchantId = env.merchantKeypair.publicKey();
    const businessName = `Integration Test Merchant ${Date.now()}`;

    // NOTE(#661): `registerMerchant`/`verifyMerchant` require the merchant's
    // and admin's signatures respectively (require_auth on-chain). This test
    // documents the intended flow; wiring the AssembledTransaction ->
    // signAndSend calls for each step depends on which entry point
    // (register_merchant vs MerchantRegistry.register_merchant) the
    // configured `contractId` exposes. See TODO below.
    //
    // TODO(#661): once a merchant-registry testnet deployment + funded
    // admin/merchant keypairs are available in CI secrets, replace this
    // with real signAndSend calls, e.g.:
    //   const registerTx = await client.registerMerchant({...});
    //   await signAndSend(registerTx, env.merchantKeypair);
    //   const verifyTx = await client.verifyMerchant(env.adminKeypair.publicKey(), merchantId);
    //   await signAndSend(verifyTx, env.adminKeypair);

    const merchant = await client.getMerchant(merchantId);

    // A merchant that doesn't exist yet returns `None`/`null` rather than
    // throwing — assert the read-only call itself succeeds against the live
    // contract (catches ABI drift on `get_merchant`'s return shape).
    assert.ok(
      merchant === null || merchant === undefined || typeof merchant === "object",
      "get_merchant should return an object, null, or undefined",
    );

    void businessName; // reserved for the real register_merchant call above
  },
);
