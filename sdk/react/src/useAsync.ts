import * as React from "react";

export interface AsyncState<T, E extends Error = Error> {
  data: T | undefined;
  loading: boolean;
  error: E | undefined;
  refetch: () => void;
}

/**
 * Runs `fetcher` whenever `deps` change and exposes a `{ data, loading, error }`
 * shape compatible with React Query / SWR consumers. Skips fetching entirely
 * when `enabled` is false (e.g. a required id is not yet available).
 */
export function useAsync<T, E extends Error = Error>(
  fetcher: () => Promise<T>,
  deps: React.DependencyList,
  enabled: boolean = true,
): AsyncState<T, E> {
  const [data, setData] = React.useState<T | undefined>(undefined);
  const [loading, setLoading] = React.useState<boolean>(enabled);
  const [error, setError] = React.useState<E | undefined>(undefined);
  const [tick, setTick] = React.useState(0);

  React.useEffect(() => {
    if (!enabled) {
      setLoading(false);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError(undefined);

    fetcher()
      .then((result) => {
        if (!cancelled) {
          setData(result);
          setLoading(false);
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setError((err instanceof Error ? err : new Error(String(err))) as E);
          setLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, enabled, tick]);

  const refetch = React.useCallback(() => setTick((t) => t + 1), []);

  return { data, loading, error, refetch };
}
