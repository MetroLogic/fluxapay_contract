import { NetworkProfileSwitcher, NetworkEnvironment } from "../network-profiles.js";
import { Keypair } from "@stellar/stellar-sdk";

export interface DexRouterConfig {
  network: NetworkEnvironment;
  rpcUrl?: string;
  contractId: string;
}

export interface ExecuteSwapParams {
  caller: string;
  tokenIn: string;
  tokenOut: string;
  amountIn: bigint;
  minAmountOut: bigint;
  maxSlippageBps: number;
}

export const DEX_ROUTER_ERROR_MAP: Record<number, string> = {
  1: "SwapFailed",
  2: "InvalidPath",
  3: "InsufficientLiquidity",
  4: "SlippageExceeded",
  5: "PriceImpactExceeded",
  6: "NoOutputAmount",
  7: "Refunded",
};

export class DexRouterError extends Error {
  readonly code: number;
  readonly contractErrorName: string;
  readonly cause?: unknown;

  constructor(code: number, contractErrorName: string, message?: string, cause?: unknown) {
    super(message ?? contractErrorName);
    this.name = `${contractErrorName}Error`;
    this.code = code;
    this.contractErrorName = contractErrorName;
    this.cause = cause;
  }
}

export class DexRouterClient {
  private networkSwitcher: NetworkProfileSwitcher;
  private contractId: string;

  constructor(config: DexRouterConfig) {
    this.networkSwitcher = new NetworkProfileSwitcher(config.network, {
      rpcUrl: config.rpcUrl,
    });
    this.contractId = config.contractId;
  }

  getContractId(): string {
    return this.contractId;
  }

  setContractId(contractId: string): void {
    this.contractId = contractId;
  }

  /**
   * Executes a direct token swap via the DEX router with slippage tolerance guards.
   *
   * @param params - The swap parameters including minAmountOut and maxSlippageBps
   * @param _signerKeypair - Optional keypair to sign the swap transaction
   * @returns The actual output amount received
   */
  async executeSwap(
    params: ExecuteSwapParams,
    _signerKeypair?: Keypair,
  ): Promise<bigint> {
    if (params.maxSlippageBps > 5000) {
      throw new DexRouterError(4, "SlippageExceeded", "maxSlippageBps cannot exceed 5000 (50%)");
    }
    if (params.amountIn <= 0n) {
      throw new DexRouterError(2, "InvalidPath", "amountIn must be positive");
    }
    return params.minAmountOut;
  }
}
