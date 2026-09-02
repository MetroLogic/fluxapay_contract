import * as React from "react";
/**
 * Runs `fetcher` whenever `deps` change and exposes a `{ data, loading, error }`
 * shape compatible with React Query / SWR consumers. Skips fetching entirely
 * when `enabled` is false (e.g. a required id is not yet available).
 */
export function useAsync(fetcher, deps, enabled = true) {
    const [data, setData] = React.useState(undefined);
    const [loading, setLoading] = React.useState(enabled);
    const [error, setError] = React.useState(undefined);
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
                setError((err instanceof Error ? err : new Error(String(err))));
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
//# sourceMappingURL=useAsync.js.map