import * as React from "react";
import { FluxapayClient, type FluxapayConfig } from "@fluxapay/sdk";
export interface FluxapayProviderProps {
    config: FluxapayConfig;
    children: React.ReactNode;
}
/**
 * Provides a shared `FluxapayClient` instance to every `@fluxapay/react` hook
 * in the component tree below it.
 */
export declare function FluxapayProvider({ config, children }: FluxapayProviderProps): any;
/** Access the `FluxapayClient` supplied by the nearest `FluxapayProvider`. */
export declare function useFluxapayClient(): FluxapayClient;
