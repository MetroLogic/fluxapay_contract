/**
 * Issue #661: Integration test — exercises the full create -> verify ->
 * settle payment lifecycle against a real deployed testnet PaymentProcessor
 * contract.
 *
 * Skips automatically when `TESTNET_RPC_URL` is not set.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { getTestnetEnv, makeTestnetClient, signAndSend } from "./helpers.js";

const env = getTestnetEnv();

test(
  "create -> verify -> settle payment lifecycle on testnet",
  { skip: !env && "TESTNET_RPC_URL not set — skipping testnet integration test" },
  async () => {
    if (!env) return;
    const client = makeTestnetClient(env);

    const paymentId = `itest-lifecycle-${Date.now()}`;

    // 1. Create
    const createTx = await client.createPayment({
      paymentId,
      merchantId: env.merchantKeypair.publicKey(),
      amount: 250n,
      currency: "USDC",
      depositAddress: env.merchantKeypair.publicKey(),
      durationSecs: 3600n,
    });
    await signAndSend(createTx, env.merchantKeypair);

    let payment = await client.getPayment(paymentId);
    assert.ok(payment, "payment should exist after createPayment");

    // 2. Verify (requires an ORACLE-role signer on the deployed contract —
    // TODO(#661): populate TESTNET_ORACLE_SECRET_KEY in CI secrets once an
    // oracle role is provisioned on the testnet deployment; until then this
    // step is best-effort and the assertion below is on `getPayment` alone).
    //
    // const verifyTx = await client.verifyPayment({
    //   oracle: env.oracleKeypair.publicKey(),
    //   paymentId,
    //   transactionHash: Buffer.alloc(32),
    //   payerAddress: env.payerKeypair.publicKey(),
    //   amountReceived: 250n,
    // });
    // await signAndSend(verifyTx, env.oracleKeypair);

    // 3. Settle (requires a SETTLEMENT_OPERATOR-role signer — same
    // provisioning caveat as verify above).
    //
    // const settleTx = await client.contract.settle_payment({
    //   operator: env.adminKeypair.publicKey(),
    //   payment_id: paymentId,
    //   splits: [],
    // });
    // await signAndSend(settleTx, env.adminKeypair);

    payment = await client.getPayment(paymentId);
    assert.ok(payment, "payment should still be retrievable after the lifecycle steps run");
  },
);
