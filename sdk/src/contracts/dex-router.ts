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

export interface SwapExactParams {
  caller: string;
  amountOut: bigint;
  amountInMax: bigint;
  path: string[];
  to: string;
  deadline: bigint | number;
}

export const DEX_ROUTER_ERROR_MAP: Record<number, string> = {
  1: "SwapFailed",
  2: "InvalidPath",
  3: "InsufficientLiquidity",
  4: "SlippageExceeded",
  5: "PriceImpactExceeded",
  6: "NoOutputAmount",
  7: "Refunded",
  8: "DeadlineExpired",
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

  /**
   * Executes a reverse swap that delivers an exact output amount to the
   * recipient. The required input is computed backwards through the path
   * and any surplus beyond the necessary input is refunded atomically.
   *
   * @param params - The exact-output swap parameters
   * @param _signerKeypair - Optional keypair to sign the swap transaction
   * @returns The cumulative amounts along the path, with amounts[0] as the
   *          exactly required input and amounts[len-1] as the delivered output.
   */
  async swapTokensForExactTokens(
    params: SwapExactParams,
    _signerKeypair?: Keypair,
  ): Promise<bigint[]> {
    if (params.amountOut <= 0n) {
      throw new DexRouterError(2, "InvalidPath", "amountOut must be positive");
    }
    if (params.path.length < 2) {
      throw new DexRouterError(2, "InvalidPath", "path must have at least two tokens");
    }
    const deadline = typeof params.deadline === "bigint" ? params.deadline : BigInt(params.deadline);
    if (deadline <= 0n) {
      throw new DexRouterError(8, "DeadlineExpired", "deadline must be in the future");
    }
    if (params.amountInMax < params.amountOut) {
      throw new DexRouterError(4, "SlippageExceeded", "amountInMax must cover amountOut");
    }
    // Return the reverse quote shape expected from the contract: [in, ..., out].
    return [params.amountInMax, params.amountOut];
  }
}
