/**
 * Issue #661: Shared setup for SDK integration tests that run against real
 * deployed Soroban testnet contracts (as opposed to the unit tests, which
 * mock the RPC layer entirely).
 *
 * Every integration test file MUST call `requireTestnetEnv()` at the top of
 * each `test()` body (or use `skipIfNoTestnet()`) so the suite degrades to a
 * no-op skip when `TESTNET_RPC_URL` is not set — e.g. on a contributor's
 * machine or in a PR-triggered CI run that intentionally does not exercise
 * live network calls (see `.github/workflows/sdk-integration-tests.yml`,
 * which only runs this suite on push to `main`).
 *
 * Keypairs are read from environment variables (populated from GitHub
 * Secrets in CI) — never hard-code testnet secret keys in this repo.
 */
import { Keypair, Networks } from "@stellar/stellar-sdk";
import { basicNodeSigner } from "@stellar/stellar-sdk/contract";
import type { AssembledTransaction } from "@stellar/stellar-sdk/contract";
import { FluxapayClient, type FluxapayConfig } from "../../src/index.js";

export interface TestnetEnv {
  rpcUrl: string;
  contractId: string;
  merchantRegistryContractId?: string;
  adminKeypair: Keypair;
  merchantKeypair: Keypair;
  payerKeypair: Keypair;
}

/**
 * Returns the testnet environment config, or `null` if integration tests
 * should be skipped (TESTNET_RPC_URL not set).
 */
export function getTestnetEnv(): TestnetEnv | null {
  const rpcUrl = process.env.TESTNET_RPC_URL;
  if (!rpcUrl) {
    return null;
  }

  const contractId = process.env.TESTNET_PAYMENT_PROCESSOR_CONTRACT_ID;
  if (!contractId) {
    throw new Error(
      "TESTNET_RPC_URL is set but TESTNET_PAYMENT_PROCESSOR_CONTRACT_ID is missing — " +
        "both must be configured together for integration tests to run.",
    );
  }

  const adminSecret = process.env.TESTNET_ADMIN_SECRET_KEY;
  const merchantSecret = process.env.TESTNET_MERCHANT_SECRET_KEY;
  const payerSecret = process.env.TESTNET_PAYER_SECRET_KEY;
  if (!adminSecret || !merchantSecret || !payerSecret) {
    throw new Error(
      "TESTNET_RPC_URL is set but one or more of TESTNET_ADMIN_SECRET_KEY / " +
        "TESTNET_MERCHANT_SECRET_KEY / TESTNET_PAYER_SECRET_KEY is missing.",
    );
  }

  return {
    rpcUrl,
    contractId,
    merchantRegistryContractId: process.env.TESTNET_MERCHANT_REGISTRY_CONTRACT_ID,
    adminKeypair: Keypair.fromSecret(adminSecret),
    merchantKeypair: Keypair.fromSecret(merchantSecret),
    payerKeypair: Keypair.fromSecret(payerSecret),
  };
}

/**
 * Build a `FluxapayClient` pointed at the testnet contracts described by
 * `env` (as returned by `getTestnetEnv()`).
 */
export function makeTestnetClient(env: TestnetEnv): FluxapayClient {
  const config: FluxapayConfig = {
    network: "testnet",
    rpcUrl: env.rpcUrl,
    contractId: env.contractId,
    merchantRegistryContractId: env.merchantRegistryContractId,
  };
  return new FluxapayClient(config);
}

/**
 * Sign and submit an `AssembledTransaction` returned by a generated contract
 * method (e.g. `client.contract.create_payment(...)`), using a testnet
 * keypair loaded from env vars via `getTestnetEnv()`.
 */
export async function signAndSend<T>(
  tx: AssembledTransaction<T>,
  signer: Keypair,
): Promise<T> {
  const { signTransaction } = basicNodeSigner(signer, Networks.TESTNET);
  const sent = await tx.signAndSend({ signTransaction });
  return sent.result;
}
