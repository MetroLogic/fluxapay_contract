import { GasEstimator } from './contracts/gas-estimator';

export interface FeeEstimate {
  baseFee: bigint;
  platformFee: bigint;
  totalFee: bigint;
  currency: 'USDC';
}

export interface EstimateFeeParams {
  operation: string;
  amount: bigint;
  merchantId: string;
}

export class FluxapayClient {
  private gasEstimator: GasEstimator;

  constructor(gasEstimator: GasEstimator) {
    this.gasEstimator = gasEstimator;
  }

  /**
   * Estimate the fees for an operation before submission.
   *
   * Calls the on-chain `GasEstimator` contract to simulate the operation and
   * returns a strongly-typed fee preview suitable for checkout UIs.
   *
   * @example
   * ```ts
   * const estimate = await client.estimateFee({
   *   operation: 'create_payment',
   *   amount: 1_000_000_000n, // stroops
   *   merchantId: 'merchant_abc',
   * });
   * // estimate: { baseFee, platformFee, totalFee, currency: 'USDC' }
   * ```
   */
  async estimateFee(params: EstimateFeeParams): Promise<FeeEstimate> {
    const { operation, amount, merchantId } = params;

    const result = await this.gasEstimator.estimate({
      operation,
      amount,
      merchantId,
    });

    const baseFee = BigInt(result.baseFee);
    const platformFee = BigInt(result.platformFee);

    return {
      baseFee,
      platformFee,
      totalFee: baseFee + platformFee,
      currency: 'USDC',
    };
  }
}
