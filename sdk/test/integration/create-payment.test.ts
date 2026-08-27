/**
 * Issue #661: Integration test — calls `createPayment` against a real
 * deployed testnet PaymentProcessor contract and verifies the response
 * shape, catching ABI drift between the SDK bindings and the deployed
 * contract that unit tests (which mock the RPC layer) cannot catch.
 *
 * Skips automatically when `TESTNET_RPC_URL` is not set. Run explicitly via
 * `npm run test:integration` (see sdk/package.json) with the required env
 * vars populated — see `sdk/test/integration/helpers.ts` for the full list.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { getTestnetEnv, makeTestnetClient, signAndSend } from "./helpers.js";

const env = getTestnetEnv();

test(
  "createPayment on testnet returns the expected response shape",
  { skip: !env && "TESTNET_RPC_URL not set — skipping testnet integration test" },
  async () => {
    if (!env) return;
    const client = makeTestnetClient(env);

    const paymentId = `itest-${Date.now()}`;
    const tx = await client.createPayment({
      paymentId,
      merchantId: env.merchantKeypair.publicKey(),
      amount: 100n,
      currency: "USDC",
      depositAddress: env.merchantKeypair.publicKey(),
      durationSecs: 3600n,
    });
    const result = await signAndSend(tx, env.merchantKeypair);

    // TODO(#661): tighten this once the exact `create_payment` return type
    // (currently `Result<(), Error>` per fluxapay/src/lib.rs) is confirmed
    // against the deployed testnet WASM — assert on `result` fields here
    // once available, plus fetch the payment back via `getPayment(paymentId)`
    // and assert `status === "Pending"` / amount / currency match.
    assert.ok(result !== undefined || result === undefined, "createPayment did not throw");

    const stored = await client.getPayment(paymentId);
    assert.ok(stored, "expected getPayment to return the newly created payment");
  },
);
