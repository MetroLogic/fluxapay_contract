import * as React from "react";
import { FluxapayClient, type FluxapayConfig } from "@fluxapay/sdk";

const FluxapayContext = React.createContext<FluxapayClient | undefined>(
  undefined,
);

export interface FluxapayProviderProps {
  config: FluxapayConfig;
  children: React.ReactNode;
}

/**
 * Provides a shared `FluxapayClient` instance to every `@fluxapay/react` hook
 * in the component tree below it.
 */
export function FluxapayProvider({ config, children }: FluxapayProviderProps) {
  const client = React.useMemo(() => new FluxapayClient(config), [
    config.network,
    config.rpcUrl,
    config.contractId,
    config.oracleContractId,
    config.merchantRegistryContractId,
    config.paymentLinkContractId,
  ]);

  return (
    <FluxapayContext.Provider value={client}>
      {children}
    </FluxapayContext.Provider>
  );
}

/** Access the `FluxapayClient` supplied by the nearest `FluxapayProvider`. */
export function useFluxapayClient(): FluxapayClient {
  const client = React.useContext(FluxapayContext);
  if (!client) {
    throw new Error(
      "useFluxapayClient must be used within a <FluxapayProvider>",
    );
  }
  return client;
}
