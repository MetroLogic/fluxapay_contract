import { NetworkProfileSwitcher, NetworkEnvironment } from "../network-profiles.js";
import type { RateData } from "./fluxapay/src/index.js";

export type { RateData };

export interface FxOracleConfig {
  network: NetworkEnvironment;
  rpcUrl?: string;
  oracleContractId: string;
}

export const FX_ORACLE_ERROR_MAP: Record<number, string> = {
  1: "RateNotFound",
  2: "RateStale",
  3: "Unauthorized",
  4: "BatchTooLarge",
};

export class FxOracleError extends Error {
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

const HOST_ERROR_CODE_REGEX = /Error\(Contract,\s*#(\d+)\)/;

function parseContractErrorCode(error: unknown): number | null {
  if (typeof error !== "object" || error === null) {
    return null;
  }

  const maybeCode = (error as { code?: unknown }).code;
  if (typeof maybeCode === "number") {
    return maybeCode;
  }

  const maybeMessage = (error as { message?: unknown }).message;
  if (typeof maybeMessage === "string") {
    const match = maybeMessage.match(HOST_ERROR_CODE_REGEX);
    if (match && match[1]) {
      return Number(match[1]);
    }
  }

  const maybeResult = (error as { result?: unknown }).result;
  if (typeof maybeResult === "string") {
    const match = maybeResult.match(HOST_ERROR_CODE_REGEX);
    if (match && match[1]) {
      return Number(match[1]);
    }
  }

  return null;
}

function toFxOracleError(error: unknown): FxOracleError {
  const code = parseContractErrorCode(error);
  if (code === null) {
    if (error instanceof Error) {
      throw error;
    }
    throw new Error("Unknown FX Oracle SDK error");
  }

  const contractErrorName = FX_ORACLE_ERROR_MAP[code] ?? "UnknownContractError";
  return new FxOracleError(
    code,
    contractErrorName,
    `${contractErrorName} (contract error #${code})`,
    error,
  );
}

async function withFxOracleContractError<T>(operation: () => Promise<T>): Promise<T> {
  try {
    return await operation();
  } catch (error) {
    throw toFxOracleError(error);
  }
}

/**
 * FxOracleClient provides a high-level interface for interacting with the FX Oracle contract.
 * Supports rate publishing, settlement amount queries, and staleness threshold management.
 */
export class FxOracleClient {
  private contract: any;
  public networkSwitcher: NetworkProfileSwitcher;
  private oracleContractId: string;
  private rpcUrl: string;
  private networkPassphrase: string;

  constructor(config: FxOracleConfig) {
    this.networkSwitcher = new NetworkProfileSwitcher(config.network);
    const profile = this.networkSwitcher.getProfile();
    this.rpcUrl = config.rpcUrl || profile.rpcUrl;
    this.networkPassphrase = profile.networkPassphrase;
    this.oracleContractId = config.oracleContractId;
  }

  private getContract(): any {
    if (!this.contract) {
      const { Client } = require("@stellar/stellar-sdk/contract");
      this.contract = new Client({
        networkPassphrase: this.networkPassphrase,
        rpcUrl: this.rpcUrl,
        contractId: this.oracleContractId,
      });
    }
    return this.contract;
  }

  /**
   * Switch the client to a different network environment.
   * @param environment - The target network environment (e.g., 'testnet', 'mainnet')
   * @param oracleContractId - Optional FX Oracle contract ID for the new network
   */
  public switchNetwork(environment: NetworkEnvironment, oracleContractId?: string): void {
    this.networkSwitcher.switchEnvironment(environment);
    const profile = this.networkSwitcher.getProfile();
    this.rpcUrl = profile.rpcUrl;
    this.networkPassphrase = profile.networkPassphrase;
    if (oracleContractId) {
      this.oracleContractId = oracleContractId;
    }
    this.contract = undefined;
  }

  /**
   * Publish an exchange rate for a currency pair.
   * @param operator - Address with the ORACLE role
   * @param pair - Currency pair symbol (e.g., "USDCNGN")
   * @param rate - Exchange rate as i128
   * @param decimals - Decimal precision of the rate
   */
  async setRate(
    operator: string,
    pair: string,
    rate: bigint,
    decimals: number,
  ) {
    return withFxOracleContractError(() =>
      this.getContract().set_rate({
        operator,
        pair,
        rate,
        decimals,
      }),
    );
  }

  async setOracleQuorum(admin: string, quorum: number) {
    return withFxOracleContractError(() =>
      (this.getContract() as any).set_oracle_quorum({ admin, quorum }),
    );
  }

  async getOracleSubmissions(pair: string) {
    return withFxOracleContractError(() =>
      (this.getContract() as any).get_oracle_submissions({ pair }),
    );
  }

  /**
   * Retrieve the current exchange rate for a currency pair.
   * Rejects stale rates based on the configured staleness threshold.
   * @param pair - Currency pair symbol (e.g., "USDCNGN")
   */
  async getRate(pair: string) {
    return withFxOracleContractError(() =>
      this.getContract().get_rate({ pair }),
    );
  }

  /**
   * Convert a USDC amount to the target settlement currency.
   * @param usdcAmount - Amount in USDC stroops
   * @param targetCurrency - Target currency symbol (e.g., "NGN")
   */
  async getSettlementAmount(usdcAmount: bigint, targetCurrency: string) {
    return withFxOracleContractError(() =>
      this.getContract().get_settlement_amount({
        usdc_amount: usdcAmount,
        target_currency: targetCurrency,
      }),
    );
  }

  /**
   * Retrieve the current staleness threshold in seconds.
   */
  async getStalenessThreshold() {
    return withFxOracleContractError(() =>
      this.getContract().get_staleness_threshold(),
    );
  }

  /**
   * Update the staleness threshold for rate data.
   * @param admin - Address with the ADMIN role
   * @param threshold - New threshold in seconds
   */
  async setStalenessThreshold(admin: string, threshold: bigint) {
    return withFxOracleContractError(() =>
      this.getContract().set_staleness_threshold({
        admin,
        threshold,
      }),
    );
  }

  /**
   * Permissionless probe: returns true when the rate is stale (emits RATE/STALE_ALERT).
   * @param pair - Currency pair symbol
   */
  async checkRateStaleness(pair: string): Promise<boolean> {
    return withFxOracleContractError(() =>
      this.getContract().check_rate_staleness({ pair }),
    );
  }

  /**
   * Atomically update up to 20 currency pairs. Requires the ORACLE role.
   * @param operator - Address with the ORACLE role
   * @param rates - Array of `[pair, rate, decimals]` tuples
   */
  async setRatesBatch(
    operator: string,
    rates: Array<[string, bigint, number]>,
  ): Promise<number> {
    return withFxOracleContractError(() =>
      this.getContract().set_rates_batch({
        operator,
        rates,
      }),
    );
  }
}
