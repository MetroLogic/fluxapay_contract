import { NetworkProfileSwitcher, NetworkEnvironment } from "../network-profiles.js";

export interface GasEstimatorConfig {
  network: NetworkEnvironment;
  rpcUrl?: string;
  gasEstimatorContractId: string;
}

/** Mirrors the on-chain `Operation` enum in `gas_estimator.rs`. */
export type GasOperation =
  | "CreatePayment"
  | "VerifyPayment"
  | "CancelPayment"
  | "ExpirePayment"
  | "SettlePayment"
  | "CreateRefund"
  | "ProcessRefund"
  | "RejectRefund"
  | "CancelRefund"
  | "CreateDispute"
  | "ResolveDispute"
  | "RejectDispute"
  | "SwapAndPay"
  | "CreateStream"
  | "WithdrawStream"
  | "CancelStream";

/** Mirrors the on-chain `CostEstimate` struct returned by `GasEstimator`. */
export interface GasEstimate {
  operation: GasOperation;
  instructions: bigint;
  ledgerReads: number;
  ledgerWrites: number;
  events: number;
  resourceFeeStroops: bigint;
}

/**
 * Friendly, snake_case operation identifiers accepted by the SDK's
 * `estimateFee` helper. These map onto the on-chain `GasOperation` enum.
 */
export type FeeOperation =
  | "create_payment"
  | "verify_payment"
  | "cancel_payment"
  | "expire_payment"
  | "settle_payment"
  | "create_refund"
  | "process_refund"
  | "reject_refund"
  | "cancel_refund"
  | "create_dispute"
  | "resolve_dispute"
  | "reject_dispute"
  | "swap_and_pay"
  | "create_stream"
  | "withdraw_stream"
  | "cancel_stream";

/** Parameters accepted by `GasEstimatorClient.estimateFee`. */
export interface FeeEstimateParams {
  operation: FeeOperation;
  /** Amount in stroops. */
  amount: bigint;
  merchantId: string;
}

/** Strongly-typed fee preview returned by `estimateFee`. */
export interface FeeEstimate {
  baseFee: bigint;
  platformFee: bigint;
  totalFee: bigint;
  currency: "USDC";
}

const FEE_OPERATION_MAP: Record<FeeOperation, GasOperation> = {
  create_payment: "CreatePayment",
  verify_payment: "VerifyPayment",
  cancel_payment: "CancelPayment",
  expire_payment: "ExpirePayment",
  settle_payment: "SettlePayment",
  create_refund: "CreateRefund",
  process_refund: "ProcessRefund",
  reject_refund: "RejectRefund",
  cancel_refund: "CancelRefund",
  create_dispute: "CreateDispute",
  resolve_dispute: "ResolveDispute",
  reject_dispute: "RejectDispute",
  swap_and_pay: "SwapAndPay",
  create_stream: "CreateStream",
  withdraw_stream: "WithdrawStream",
  cancel_stream: "CancelStream",
};

/** Basis-point denominator used for platform fee math. */
const BPS_DENOMINATOR = 10_000n;

function fromContractEstimate(raw: {
  operation: GasOperation;
  instructions: bigint;
  ledger_reads: number;
  ledger_writes: number;
  events: number;
  resource_fee_stroops: bigint;
}): GasEstimate {
  return {
    operation: raw.operation,
    instructions: raw.instructions,
    ledgerReads: raw.ledger_reads,
    ledgerWrites: raw.ledger_writes,
    events: raw.events,
    resourceFeeStroops: raw.resource_fee_stroops,
  };
}

/**
 * GasEstimatorClient provides a high-level interface for querying on-chain
 * Soroban resource cost estimates from the `GasEstimator` contract before
 * submitting a transaction.
 */
export class GasEstimatorClient {
  private contract: any;
  public networkSwitcher: NetworkProfileSwitcher;
  private gasEstimatorContractId: string;
  private rpcUrl: string;
  private networkPassphrase: string;

  constructor(config: GasEstimatorConfig) {
    this.networkSwitcher = new NetworkProfileSwitcher(config.network);
    const profile = this.networkSwitcher.getProfile();
    this.rpcUrl = config.rpcUrl || profile.rpcUrl;
    this.networkPassphrase = profile.networkPassphrase;
    this.gasEstimatorContractId = config.gasEstimatorContractId;
  }

  private getContract(): any {
    if (!this.contract) {
      const { Client } = require("@stellar/stellar-sdk/contract");
      this.contract = new Client({
        networkPassphrase: this.networkPassphrase,
        rpcUrl: this.rpcUrl,
        contractId: this.gasEstimatorContractId,
      });
    }
    return this.contract;
  }

  /**
   * Switch the client to a different network environment.
   * @param environment - The target network environment (e.g., 'testnet', 'mainnet')
   * @param gasEstimatorContractId - Optional GasEstimator contract ID for the new network
   */
  public switchNetwork(environment: NetworkEnvironment, gasEstimatorContractId?: string): void {
    this.networkSwitcher.switchEnvironment(environment);
    const profile = this.networkSwitcher.getProfile();
    this.rpcUrl = profile.rpcUrl;
    this.networkPassphrase = profile.networkPassphrase;
    if (gasEstimatorContractId) {
      this.gasEstimatorContractId = gasEstimatorContractId;
    }
    this.contract = undefined;
  }

  /** Estimate the resource cost of a single operation. */
  async estimate(operation: GasOperation): Promise<GasEstimate> {
    const raw = await this.getContract().estimate({ op: operation });
    return fromContractEstimate(raw);
  }

  /** Estimate the resource cost of every supported operation. */
  async estimateAll(): Promise<GasEstimate[]> {
    const raw: Array<Parameters<typeof fromContractEstimate>[0]> =
      await this.getContract().estimate_all();
    return raw.map(fromContractEstimate);
  }

  /** Fetch the on-chain Symbol name for an operation (useful for display). */
  async operationName(operation: GasOperation): Promise<string> {
    return this.getContract().operation_name({ op: operation });
  }

  /**
   * Produce a friendly fee preview for a checkout UI before submission.
   *
   * Resolves the on-chain resource fee for the given operation and combines
   * it with the platform fee (in basis points) to yield a total fee in USDC.
   */
  async estimateFee(params: FeeEstimateParams): Promise<FeeEstimate> {
    const operation = FEE_OPERATION_MAP[params.operation];
    if (!operation) {
      throw new Error(`Unsupported fee operation: ${params.operation}`);
    }

    const raw = await this.getContract().estimate_fee({
      op: operation,
      amount: params.amount,
      merchant_id: params.merchantId,
    });

    const baseFee = BigInt(raw.base_fee_stroops ?? raw.baseFee ?? 0n);
    const platformFee = BigInt(
      raw.platform_fee_stroops ??
        raw.platformFee ??
        (params.amount * BigInt(raw.platform_fee_bps ?? 0)) / BPS_DENOMINATOR,
    );

    return {
      baseFee,
      platformFee,
      totalFee: baseFee + platformFee,
      currency: "USDC",
    };
  }
}
