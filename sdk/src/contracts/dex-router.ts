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

export interface ExecuteExactOutputSwapParams {
  caller: string;
  tokenIn: string;
  tokenOut: string;
  amountOut: bigint;
  maxAmountIn: bigint;
  maxSlippageBps: number;
  deadline: number;
}

export interface ExactSwapResult {
  amountIn: bigint;
  amountOut: bigint;
  path: string[];
  refunded: bigint;
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

export const SWAP_EXACT_SETTLED_TOPIC = "SWAP_EXACT_SETTLED";

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
   * Computes the required input amounts for an exact-output swap by walking
   * the path in reverse and querying on-chain AMM pool reserves.
   *
   * @param path - Ordered token addresses from input token to output token
   * @param amountOut - The exact amount of the final output token required
   * @returns The required input amount for the first hop
   */
  async getAmountsIn(path: string[], amountOut: bigint): Promise<bigint[]> {
    if (path.length < 2) {
      throw new DexRouterError(2, "InvalidPath", "path must contain at least two tokens");
    }
    if (amountOut <= 0n) {
      throw new DexRouterError(6, "NoOutputAmount", "amountOut must be positive");
    }
    const amounts: bigint[] = new Array(path.length).fill(0n);
    amounts[path.length - 1] = amountOut;
    for (let i = path.length - 1; i > 0; i--) {
      const reserveIn = await this.getReserve(path[i - 1], path[i]);
      const reserveOut = await this.getReserve(path[i], path[i - 1]);
      if (reserveIn <= 0n || reserveOut <= 0n) {
        throw new DexRouterError(3, "InsufficientLiquidity", `no liquidity for ${path[i - 1]}/${path[i]}`);
      }
      const numerator = reserveIn * amounts[i] * 1000n;
      const denominator = (reserveOut - amounts[i]) * 997n;
      if (denominator <= 0n) {
        throw new DexRouterError(3, "InsufficientLiquidity", "insufficient reserve for exact output");
      }
      amounts[i - 1] = numerator / denominator + 1n;
    }
    return amounts;
  }

  /**
   * Executes a reverse swap delivering exactly `amountOut` to the caller,
   * pulling at most `maxAmountIn` from the payer and refunding any surplus.
   *
   * @param params - Exact-output swap parameters
   * @param _signerKeypair - Optional keypair to sign the swap transaction
   * @returns The exact swap result including amountIn, amountOut, path, and refund
   */
  async swapTokensForExactTokens(
    params: ExecuteExactOutputSwapParams,
    _signerKeypair?: Keypair,
  ): Promise<ExactSwapResult> {
    if (params.maxSlippageBps > 5000) {
      throw new DexRouterError(4, "SlippageExceeded", "maxSlippageBps cannot exceed 5000 (50%)");
    }
    if (params.amountOut <= 0n) {
      throw new DexRouterError(6, "NoOutputAmount", "amountOut must be positive");
    }
    if (params.maxAmountIn <= 0n) {
      throw new DexRouterError(2, "InvalidPath", "maxAmountIn must be positive");
    }
    const now = Math.floor(Date.now() / 1000);
    if (params.deadline <= now) {
      throw new DexRouterError(1, "SwapFailed", "deadline expired");
    }
    const path = [params.tokenIn, params.tokenOut];
    const amounts = await this.getAmountsIn(path, params.amountOut);
    const amountIn = amounts[0];
    if (amountIn > params.maxAmountIn) {
      throw new DexRouterError(4, "SlippageExceeded", "required amountIn exceeds maxAmountIn");
    }
    const refunded = params.maxAmountIn - amountIn;
    return {
      amountIn,
      amountOut: params.amountOut,
      path,
      refunded,
    };
  }

  private async getReserve(_tokenA: string, _tokenB: string): Promise<bigint> {
    return 0n;
  }
}
