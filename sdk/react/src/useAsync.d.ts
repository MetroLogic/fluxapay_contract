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
export declare function useAsync<T, E extends Error = Error>(fetcher: () => Promise<T>, deps: React.DependencyList, enabled?: boolean): AsyncState<T, E>;
