import { Networks } from "@stellar/stellar-sdk";

export type NetworkEnvironment = "mainnet" | "testnet" | "sandbox" | "standalone";

export interface NetworkProfile {
  environment: NetworkEnvironment;
  networkPassphrase: string;
  rpcUrl: string;
  defaultContractId?: string;
}

/** Placeholder used for any contract not yet deployed to a given environment. */
export const UNSET_CONTRACT_ID = "CONTRACT_ID_NOT_SET";

/** The five Fluxapay Soroban contracts that ship a deployed address per environment. */
export interface FluxapayContractIds {
  paymentProcessor: string;
  refundManager: string;
  merchantRegistry: string;
  fxOracle: string;
  paymentLinkManager: string;
}

/**
 * Canonical, per-environment contract addresses for all Fluxapay contracts.
 *
 * `FluxapayClient` (and the individual contract clients) fall back to these
 * values whenever a `*ContractId` is not explicitly supplied in config, so
 * integrators don't need to hard-code addresses themselves.
 *
 * `mainnet` entries are placeholders (`UNSET_CONTRACT_ID`) until the mainnet
 * deployment lands — see `scripts/check-error-map-sync.ts`'s sibling CI check
 * in `.github/workflows/ci.yml` ("Check mainnet contract IDs"), which fails
 * the moment a real address should have replaced these but hasn't.
 */
export const FLUXAPAY_CONTRACT_IDS: Record<NetworkEnvironment, FluxapayContractIds> = {
  mainnet: {
    paymentProcessor: UNSET_CONTRACT_ID,
    refundManager: UNSET_CONTRACT_ID,
    merchantRegistry: UNSET_CONTRACT_ID,
    fxOracle: UNSET_CONTRACT_ID,
    paymentLinkManager: UNSET_CONTRACT_ID,
  },
  testnet: {
    paymentProcessor: UNSET_CONTRACT_ID,
    refundManager: UNSET_CONTRACT_ID,
    merchantRegistry: UNSET_CONTRACT_ID,
    fxOracle: UNSET_CONTRACT_ID,
    paymentLinkManager: UNSET_CONTRACT_ID,
  },
  sandbox: {
    paymentProcessor: UNSET_CONTRACT_ID,
    refundManager: UNSET_CONTRACT_ID,
    merchantRegistry: UNSET_CONTRACT_ID,
    fxOracle: UNSET_CONTRACT_ID,
    paymentLinkManager: UNSET_CONTRACT_ID,
  },
  standalone: {
    paymentProcessor: UNSET_CONTRACT_ID,
    refundManager: UNSET_CONTRACT_ID,
    merchantRegistry: UNSET_CONTRACT_ID,
    fxOracle: UNSET_CONTRACT_ID,
    paymentLinkManager: UNSET_CONTRACT_ID,
  },
};

export const NetworkProfiles: Record<NetworkEnvironment, NetworkProfile> = {
  mainnet: {
    environment: "mainnet",
    networkPassphrase: Networks.PUBLIC,
    rpcUrl: "https://soroban-rpc.stellar.org",
    defaultContractId: FLUXAPAY_CONTRACT_IDS.mainnet.paymentProcessor,
  },
  testnet: {
    environment: "testnet",
    networkPassphrase: Networks.TESTNET,
    rpcUrl: "https://soroban-testnet.stellar.org",
    defaultContractId: FLUXAPAY_CONTRACT_IDS.testnet.paymentProcessor,
  },
  sandbox: {
    environment: "sandbox",
    networkPassphrase: Networks.STANDALONE,
    rpcUrl: "http://localhost:8000/soroban/rpc",
    defaultContractId: FLUXAPAY_CONTRACT_IDS.sandbox.paymentProcessor,
  },
  standalone: {
    environment: "standalone",
    networkPassphrase: Networks.STANDALONE,
    rpcUrl: "http://localhost:8000/soroban/rpc",
    defaultContractId: FLUXAPAY_CONTRACT_IDS.standalone.paymentProcessor,
  },
};

export class NetworkProfileSwitcher {
  private currentProfile: NetworkProfile;

  constructor(initialEnvironment: NetworkEnvironment = "testnet") {
    const profile = NetworkProfiles[initialEnvironment];
    if (!Object.prototype.hasOwnProperty.call(NetworkProfiles, initialEnvironment) || !profile) {
      throw new Error(`Unsupported network environment: ${String(initialEnvironment)}`);
    }
    this.currentProfile = profile;
  }

  /**
   * Switch the current network environment.
   */
  public switchEnvironment(environment: NetworkEnvironment): void {
    if (!Object.prototype.hasOwnProperty.call(NetworkProfiles, environment)) {
      throw new Error(`Unsupported network environment: ${environment}`);
    }
    this.currentProfile = NetworkProfiles[environment];
  }

  /**
   * Get the active network profile.
   */
  public getProfile(): NetworkProfile {
    return this.currentProfile;
  }

  /**
   * Update the default contract ID for a specific environment.
   */
  public setContractId(environment: NetworkEnvironment, contractId: string): void {
    const profile = NetworkProfiles[environment];
    if (profile) {
      profile.defaultContractId = contractId;
    }
  }
}
