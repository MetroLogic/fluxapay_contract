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

export interface FluxapayClientOptions {
  /**
   * Maximum number of retries for transient errors (429 / 5xx).
   * Defaults to 3, yielding backoff delays of 1s, 2s, 4s.
   */
  maxRetries?: number;
}

const DEFAULT_MAX_RETRIES = 3;

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function isRetriableStatus(status: number): boolean {
  return status === 429 || status >= 500;
}

/**
 * Perform a fetch with automatic retry and exponential backoff for
 * transient errors (429 Too Many Requests and 5xx server errors).
 *
 * - Respects the `Retry-After` header (in seconds) on 429 responses.
 * - Otherwise backs off exponentially: 1s, 2s, 4s, ...
 * - Non-retriable statuses (400, 401, 403, 404, 422, ...) return immediately.
 */
export async function fetchWithRetry(
  url: string,
  options: RequestInit,
  maxRetries: number = DEFAULT_MAX_RETRIES,
): Promise<Response> {
  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    const res = await fetch(url, options);

    if (!isRetriableStatus(res.status)) {
      return res;
    }

    if (attempt === maxRetries) {
      return res;
    }

    const retryAfter = res.headers.get('Retry-After');
    const delay = retryAfter
      ? parseInt(retryAfter, 10) * 1000
      : Math.pow(2, attempt) * 1000;

    console.debug(
      `[FluxaPay] Retry attempt ${attempt + 1}/${maxRetries} after ${delay}ms ` +
        `(status ${res.status}) for ${url}`,
    );

    await sleep(delay);
  }

  // Unreachable: the loop always returns on the final attempt.
  return fetch(url, options);
}

export class FluxapayClient {
  private gasEstimator: GasEstimator;
  private maxRetries: number;

  constructor(gasEstimator: GasEstimator, options: FluxapayClientOptions = {}) {
    this.gasEstimator = gasEstimator;
    this.maxRetries = options.maxRetries ?? DEFAULT_MAX_RETRIES;
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
