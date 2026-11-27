import { useCallback, useEffect, useRef, useState } from 'react';
import type { FluxapayClient } from '../src/client';
import type { FeeEstimate, FeeEstimateParams } from '../src/contracts/gas-estimator';

export interface UseFeeEstimateOptions extends FeeEstimateParams {
  /**
   * The FluxapayClient instance used to reach the on-chain GasEstimator.
   */
  client: FluxapayClient;
  /**
   * When false, the estimate is not fetched automatically. Defaults to true.
   */
  enabled?: boolean;
}

export interface UseFeeEstimateResult {
  estimate: FeeEstimate | null;
  loading: boolean;
  error: Error | null;
  /**
   * Manually re-run the fee simulation, e.g. after the amount changes.
   */
  refetch: () => Promise<FeeEstimate | null>;
}

/**
 * React hook that binds `FluxapayClient.estimateFee` to component state so
 * checkout UIs can preview fees before submission.
 */
export function useFeeEstimate({
  client,
  operation,
  amount,
  merchantId,
  enabled = true,
}: UseFeeEstimateOptions): UseFeeEstimateResult {
  const [estimate, setEstimate] = useState<FeeEstimate | null>(null);
  const [loading, setLoading] = useState<boolean>(enabled);
  const [error, setError] = useState<Error | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const refetch = useCallback(async (): Promise<FeeEstimate | null> => {
    setLoading(true);
    setError(null);
    try {
      const result = await client.estimateFee({ operation, amount, merchantId });
      if (mountedRef.current) {
        setEstimate(result);
      }
      return result;
    } catch (err) {
      const normalized = err instanceof Error ? err : new Error(String(err));
      if (mountedRef.current) {
        setError(normalized);
      }
      return null;
    } finally {
      if (mountedRef.current) {
        setLoading(false);
      }
    }
  }, [client, operation, amount, merchantId]);

  useEffect(() => {
    if (!enabled) {
      setLoading(false);
      return;
    }
    void refetch();
  }, [enabled, refetch]);

  return { estimate, loading, error, refetch };
}

export default useFeeEstimate;
