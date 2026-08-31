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
import { Keypair } from "@stellar/stellar-sdk";
import type { AssembledTransaction } from "@stellar/stellar-sdk/contract";
import { FluxapayClient } from "../../src/index.js";
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
export declare function getTestnetEnv(): TestnetEnv | null;
/**
 * Build a `FluxapayClient` pointed at the testnet contracts described by
 * `env` (as returned by `getTestnetEnv()`).
 */
export declare function makeTestnetClient(env: TestnetEnv): FluxapayClient;
/**
 * Sign and submit an `AssembledTransaction` returned by a generated contract
 * method (e.g. `client.contract.create_payment(...)`), using a testnet
 * keypair loaded from env vars via `getTestnetEnv()`.
 */
export declare function signAndSend<T>(tx: AssembledTransaction<T>, signer: Keypair): Promise<T>;
