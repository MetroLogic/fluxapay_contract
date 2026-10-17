import { AssembledTransaction } from "@stellar/stellar-sdk/contract";
import { Client as ContractClient } from "./contracts/fluxapay/src/index.js";

/** Serializable payload for hardware/offline signing workflows (Issue #232). */
export interface OfflineTransactionPayload {
  /** Soroban contract method name. */
  method: string;
  /** Target contract ID (C...). */
  contractId: string;
  /** Network passphrase used to build the transaction. */
  networkPassphrase: string;
  /** Base64 XDR of the unsigned transaction envelope. */
  unsignedXdr: string;
  /** Hex-encoded transaction hash for wallet display/verification. */
  hash: string;
  /** JSON snapshot compatible with `Client.fromJSON.<method>()`. */
  json: string;
  /** Addresses that must sign auth entries before submission. */
  requiredAuthSigners: string[];
}

export type OfflineCapableClient = ContractClient & {
  fromJSON: Record<string, (json: string) => AssembledTransaction<unknown>>;
};

/** Extended client surface covering subscription/pre-auth billing operations
 * not yet present in the generated `ContractClient` bindings. */
export type SubscriptionBillingClient = OfflineCapableClient & {
  charge_subscription(args: {
    operator: string;
    subscription_id: string;
    token: string;
  }): Promise<AssembledTransaction<unknown>>;
  pull_payment(args: {
    merchant: string;
    customer: string;
    amount: bigint;
  }): Promise<AssembledTransaction<unknown>>;
};

/**
 * Ensure simulation completed so payload fields are populated.
 *
 * Operation-agnostic: works for any `AssembledTransaction`, including the
 * `charge_subscription` and `pull_payment` operations used by
 * `buildSubscriptionTickPayload` / `buildPullAuthorizationPayload`.
 */
export async function prepareForOfflineSigning<T>(
  tx: AssembledTransaction<T>,
): Promise<AssembledTransaction<T>> {
  if (!tx.simulation) {
    await tx.simulate();
  }
  return tx;
}

/** Build a raw offline payload from a simulated assembled transaction. */
export async function buildOfflinePayload<T>(
  method: string,
  contractId: string,
  networkPassphrase: string,
  tx: AssembledTransaction<T>,
): Promise<OfflineTransactionPayload> {
  const prepared = await prepareForOfflineSigning(tx);

  return {
    method,
    contractId,
    networkPassphrase,
    unsignedXdr: prepared.toXDR(),
    hash: prepared.built?.hash().toString("hex") ?? "",
    json: prepared.toJSON(),
    requiredAuthSigners: prepared.needsNonInvokerSigningBy(),
  };
}

/** Restore an assembled transaction from a previously exported JSON payload. */
export function restoreFromOfflinePayload(
  client: OfflineCapableClient,
  payload: Pick<OfflineTransactionPayload, "method" | "json">,
): AssembledTransaction<unknown> {
  const restore = client.fromJSON[payload.method];
  if (!restore) {
    throw new Error(`Unknown contract method for offline restore: ${payload.method}`);
  }
  return restore(payload.json);
}

/** Raw payload builder for `create_payment` invocations. */
export async function buildCreatePaymentPayload(
  client: OfflineCapableClient,
  contractId: string,
  networkPassphrase: string,
  args: Parameters<ContractClient["create_payment"]>[0],
): Promise<OfflineTransactionPayload> {
  const tx = await client.create_payment(args);
  return buildOfflinePayload("create_payment", contractId, networkPassphrase, tx);
}

/** Raw payload builder for `verify_payment` invocations. */
export async function buildVerifyPaymentPayload(
  client: OfflineCapableClient,
  contractId: string,
  networkPassphrase: string,
  args: Parameters<ContractClient["verify_payment"]>[0],
): Promise<OfflineTransactionPayload> {
  const tx = await client.verify_payment(args);
  return buildOfflinePayload("verify_payment", contractId, networkPassphrase, tx);
}

/** Raw payload builder for `create_refund` invocations. */
export async function buildCreateRefundPayload(
  client: OfflineCapableClient,
  contractId: string,
  networkPassphrase: string,
  args: Parameters<ContractClient["create_refund"]>[0],
): Promise<OfflineTransactionPayload> {
  const tx = await client.create_refund(args);
  return buildOfflinePayload("create_refund", contractId, networkPassphrase, tx);
}

/** Raw payload builder for `charge_subscription` (subscription tick) invocations. */
export async function buildSubscriptionTickPayload(
  client: SubscriptionBillingClient,
  contractId: string,
  networkPassphrase: string,
  params: { operator: string; subscriptionId: string; token: string },
): Promise<OfflineTransactionPayload> {
  const tx = await client.charge_subscription({
    operator: params.operator,
    subscription_id: params.subscriptionId,
    token: params.token,
  });
  return buildOfflinePayload("charge_subscription", contractId, networkPassphrase, tx);
}

/** Raw payload builder for `pull_payment` (pre-authorized pull) invocations. */
export async function buildPullAuthorizationPayload(
  client: SubscriptionBillingClient,
  contractId: string,
  networkPassphrase: string,
  params: { merchant: string; customer: string; amount: bigint },
): Promise<OfflineTransactionPayload> {
  const tx = await client.pull_payment({
    merchant: params.merchant,
    customer: params.customer,
    amount: params.amount,
  });
  return buildOfflinePayload("pull_payment", contractId, networkPassphrase, tx);
}

/**
 * High-level helper exposing common raw payload builders for hardware wallets.
 */
export class FluxapayOfflineSigner {
  constructor(
    private readonly client: OfflineCapableClient,
    private readonly contractId: string,
    private readonly networkPassphrase: string,
  ) {}

  /**
   * Build an unsigned transaction for a contract method call (Issue #1059).
   * 
   * Returns the unsigned XDR and transaction hash that can be transferred to
   * an air-gapped signer for signature.
   * 
   * @param method - Contract method name (e.g., 'create_payment')
   * @param args - Method arguments matching the contract method signature
   * @returns Object with `xdr` (base64 unsigned transaction) and `txHash` (hex transaction hash)
   */
  async buildUnsignedTransaction(
    method: string,
    args: Record<string, unknown>,
  ): Promise<{ xdr: string; txHash: string }> {
    // Dynamically invoke the method on the client
    const clientMethod = (this.client as any)[method];
    if (!clientMethod) {
      throw new Error(`Unknown contract method: ${method}`);
    }

    const tx = await clientMethod.call(this.client, args);
    const payload = await buildOfflinePayload(
      method,
      this.contractId,
      this.networkPassphrase,
      tx,
    );

    return {
      xdr: payload.unsignedXdr,
      txHash: payload.hash,
    };
  }

  /**
   * Sign an unsigned transaction XDR with a Stellar keypair (Issue #1059).
   * 
   * This is a static method that can be used in air-gapped environments with
   * zero network dependencies.
   * 
   * @param xdr - Base64-encoded unsigned transaction envelope
   * @param signerKeypair - Stellar keypair to sign with
   * @returns Base64-encoded signed transaction envelope
   */
  static async sign(params: {
    xdr: string;
    signerKeypair: { publicKey(): string; sign(data: Buffer): Buffer };
  }): Promise<string> {
    const { Transaction, Keypair } = await import("@stellar/stellar-sdk");
    
    // Parse the unsigned transaction
    const tx = new Transaction(params.xdr, undefined as any);
    
    // Sign with the provided keypair
    tx.sign(params.signerKeypair as any);
    
    return tx.toEnvelope().toXDR("base64");
  }

  /**
   * Submit a pre-signed transaction to the Stellar network (Issue #1059).
   * 
   * @param signedXdr - Base64-encoded signed transaction envelope
   * @returns Transaction result from the contract
   */
  async submitSignedTransaction(signedXdr: string): Promise<unknown> {
    const { SorobanRpc } = await import("@stellar/stellar-sdk");
    
    // Get the RPC server from the client's options
    const rpcUrl = (this.client as any).options?.rpcUrl;
    if (!rpcUrl) {
      throw new Error("No RPC URL configured in client");
    }
    
    const server = new SorobanRpc.Server(rpcUrl);
    const response = await server.sendTransaction(signedXdr as any);
    
    // Wait for confirmation and return result
    if (response.status === "PENDING") {
      let attempt = 0;
      while (attempt < 10) {
        await new Promise((resolve) => setTimeout(resolve, 1000));
        const txResponse = await server.getTransaction(response.hash);
        if (txResponse.status !== SorobanRpc.Api.GetTransactionStatus.NOT_FOUND) {
          return txResponse;
        }
        attempt++;
      }
    }
    
    return response;
  }

  buildCreatePayment(
    args: Parameters<ContractClient["create_payment"]>[0],
  ): Promise<OfflineTransactionPayload> {
    return buildCreatePaymentPayload(
      this.client,
      this.contractId,
      this.networkPassphrase,
      args,
    );
  }

  buildVerifyPayment(
    args: Parameters<ContractClient["verify_payment"]>[0],
  ): Promise<OfflineTransactionPayload> {
    return buildVerifyPaymentPayload(
      this.client,
      this.contractId,
      this.networkPassphrase,
      args,
    );
  }

  buildCreateRefund(
    args: Parameters<ContractClient["create_refund"]>[0],
  ): Promise<OfflineTransactionPayload> {
    return buildCreateRefundPayload(
      this.client,
      this.contractId,
      this.networkPassphrase,
      args,
    );
  }

  buildSubscriptionTick(params: {
    operator: string;
    subscriptionId: string;
    token: string;
  }): Promise<OfflineTransactionPayload> {
    return buildSubscriptionTickPayload(
      this.client as SubscriptionBillingClient,
      this.contractId,
      this.networkPassphrase,
      params,
    );
  }

  buildPullAuthorization(params: {
    merchant: string;
    customer: string;
    amount: bigint;
  }): Promise<OfflineTransactionPayload> {
    return buildPullAuthorizationPayload(
      this.client as SubscriptionBillingClient,
      this.contractId,
      this.networkPassphrase,
      params,
    );
  }

  restore(payload: OfflineTransactionPayload): AssembledTransaction<unknown> {
    return restoreFromOfflinePayload(this.client, payload);
  }
}

/**
 * Configuration for the network-free offline transaction builder (Issue #827).
 *
 * No RPC connection is required: the builder assembles unsigned Soroban
 * transaction XDR that can be signed by any Stellar keypair (HSM, air-gapped
 * signer) and later submitted via a standard Horizon/RPC client.
 */
export interface OfflineBuilderConfig {
  /** Network passphrase (e.g. `Networks.TESTNET`). */
  networkPassphrase: string;
  /** Map of logical contract names to their C... contract IDs. */
  contractIds: Record<string, string>;
  /** Source account used to build the transaction envelope. */
  sourceAccount: { publicKey: string; sequence: bigint };
}

/**
 * Network-free builder that constructs unsigned Soroban transaction XDR for
 * the major FluxaPay entry points without connecting to RPC.
 *
 * The produced XDR is a base64-encoded unsigned transaction envelope that can
 * be signed by any Stellar keypair and submitted via standard Horizon.
 */
export class OfflineTransactionBuilder {
  constructor(private readonly config: OfflineBuilderConfig) {}

  /** Resolve a contract ID by logical name, failing loudly when unknown. */
  private contractId(name: string): string {
    const id = this.config.contractIds[name];
    if (!id) {
      throw new Error(`Unknown contract name for offline builder: ${name}`);
    }
    return id;
  }

  /**
   * Build an unsigned `create_payment` transaction XDR.
   *
   * Returns the base64-encoded unsigned Soroban transaction envelope. Sign it
   * with any Stellar keypair, then submit via `stellarClient.submitTransaction`.
   */
  async createPayment(args: {
    merchantId: string;
    amount: bigint;
    currency: string;
  }): Promise<string> {
    return this.buildInvocation("paymentProcessor", "create_payment", {
      merchant_id: args.merchantId,
      amount: args.amount,
      currency: args.currency,
    });
  }

  /**
   * Assemble an unsigned Soroban invocation envelope for the given contract
   * method. Kept operation-agnostic so additional entry points can reuse it.
   */
  private async buildInvocation(
    contractName: string,
    method: string,
    args: Record<string, unknown>,
  ): Promise<string> {
    const contractId = this.contractId(contractName);
    const { networkPassphrase, sourceAccount } = this.config;

    const { Contract, TransactionBuilder, Account, nativeToScVal, xdr } =
      await import("@stellar/stellar-sdk");

    const contract = new Contract(contractId);
    const operation = contract.call(
      method,
      ...Object.values(args).map((value) => nativeToScVal(value)),
    );

    const account = new Account(sourceAccount.publicKey, sourceAccount.sequence.toString());
    const tx = new TransactionBuilder(account, {
      fee: "100",
      networkPassphrase,
    })
      .addOperation(operation)
      .setTimeout(0)
      .build();

    return tx.toEnvelope().toXDR("base64");
  }
}

/**
 * `OfflineSigner` — lightweight alias for `FluxapayOfflineSigner` that
 * exposes the air-gapped signing workflow proposed in Issue #1059.
 *
 * Usage:
 * ```ts
 * // Online: build unsigned XDR
 * const signer = new OfflineSigner(client, contractId, networkPassphrase);
 * const { xdr, txHash } = await signer.buildUnsignedTransaction('create_payment', args);
 *
 * // Air-gapped: sign with no network calls
 * const signedXdr = await OfflineSigner.sign({ xdr, signerKeypair });
 *
 * // Online: broadcast
 * const result = await signer.submitSignedTransaction(signedXdr);
 * ```
 */
export const OfflineSigner = FluxapayOfflineSigner;
export type OfflineSigner = FluxapayOfflineSigner;
