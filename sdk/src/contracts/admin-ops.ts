/**
 * Issues #579 / #580 / #581 / #582: Admin & treasury operations wrappers.
 *
 * One client covering four admin-side operation families on the FluxaPay
 * contracts. Bindings for these entry points have not been regenerated yet,
 * so calls go through the underlying contract client untyped (same convention
 * as `settleDisputeCollaboratively` in index.ts) and results are returned raw.
 *
 * - #579 On-chain invoices: createInvoice / markInvoicePaid / getInvoice
 * - #580 Admin multi-sig:  createProposal / voteProposal / executeProposal
 * - #581 Session keys:     registerSessionKey / revokeSessionKey / executeWithSession
 * - #582 Treasury:         withdrawTreasury / getTreasuryBalance / setFeeSplitConfig / getFeeSplitConfig
 */
import { withMappedContractError } from "../index.js";
import { NetworkProfileSwitcher, NetworkEnvironment, FLUXAPAY_CONTRACT_IDS, UNSET_CONTRACT_ID } from "../network-profiles.js";

export interface AdminOpsConfig {
  network: NetworkEnvironment;
  rpcUrl?: string;
  /** PaymentProcessor / RefundManager contract ID (invoices, treasury, fee split). */
  contractId?: string;
  /** AccountAbstraction contract ID (session keys). Optional until used. */
  accountAbstractionContractId?: string;
}

/** Mirrors the on-chain `FeeSplitConfig` contracttype (`fluxapay/src/types.rs`). */
export interface FeeSplitConfig {
  /** Treasury allocation in basis points (e.g. 7000 = 70%). */
  treasuryBps: number;
  /** Developer rewards allocation in basis points (e.g. 3000 = 30%). Sum with treasuryBps must be <= 10000. */
  developerBps: number;
  /** Treasury destination address. */
  treasuryAddress: string;
  /** Developer rewards destination address. */
  developerAddress: string;
}

/**
 * #580: AdminAction variants accepted by `create_proposal`. Pass exactly one
 * field set; the tag is derived from whichever field is present.
 */
export type AdminAction =
  | { tag: "SetGlobalPause"; values: [boolean, string] }
  | { tag: "AllowToken"; values: [string] }
  | { tag: "GrantRole"; values: [string, string] }
  | { tag: "RevokeRole"; values: [string, string] }
  | { tag: "TransferAdmin"; values: [string] }
  | { tag: "EmergencyRevokeRole"; values: [string, string] }
  | { tag: "SetDisputeBond"; values: [bigint] }
  | { tag: "SetVolumeCap"; values: [string, bigint] }
  | { tag: "SetRefundFeeBps"; values: [bigint] }
  | { tag: "SetRateLimit"; values: [number, bigint] };

function resolveContractId(explicit: string | undefined, fallback: string, label: string): string {
  const contractId = explicit || fallback;
  if (!contractId || contractId === "CONTRACT_ID_NOT_SET") {
    throw new Error(
      `${label} is required: pass it explicitly in FluxapayConfig, or deploy the contract and populate FLUXAPAY_CONTRACT_IDS.`,
    );
  }
  return contractId;
}

export class AdminOpsClient {
  private contract: any;
  public networkSwitcher: NetworkProfileSwitcher;
  private readonly config: AdminOpsConfig;

  constructor(config: AdminOpsConfig) {
    this.config = config;
    this.networkSwitcher = new NetworkProfileSwitcher(config.network);
    const profile = this.networkSwitcher.getProfile();
    const contractId = resolveContractId(
      config.contractId,
      FLUXAPAY_CONTRACT_IDS[config.network].paymentProcessor,
      "contractId (PaymentProcessor/RefundManager)",
    );
    // Lazy import keeps the SDK loadable in restricted runtimes.
    const { Client } = require("@stellar/stellar-sdk/contract");
    this.contract = new Client({
      networkPassphrase: profile.networkPassphrase,
      rpcUrl: config.rpcUrl || profile.rpcUrl,
      contractId,
    });
  }

  private accountAbstractionContract(): any {
    const contractId = resolveContractId(
      this.config.accountAbstractionContractId,
      (FLUXAPAY_CONTRACT_IDS as any)[this.config.network].accountAbstraction ?? "CONTRACT_ID_NOT_SET",
      "accountAbstractionContractId",
    );
    const { Client } = require("@stellar/stellar-sdk/contract");
    return new Client({
      networkPassphrase: this.networkSwitcher.getProfile().networkPassphrase,
      rpcUrl: this.config.rpcUrl || this.networkSwitcher.getProfile().rpcUrl,
      contractId,
    });
  }

  // ------------------------------------------------------------------ #579

  /** #579: Create an on-chain invoice. Returns the new invoice ID. */
  async createInvoice(params: {
    merchantId: string;
    customerEmail: string;
    lineItems: Array<{ description: string; amount: bigint; quantity: number }>;
    totalAmount: bigint;
    currency: string;
    dueDate: bigint;
  }): Promise<string> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).create_invoice({
        merchant_id: params.merchantId,
        customer_email: params.customerEmail,
        line_items: params.lineItems,
        total_amount: params.totalAmount,
        currency: params.currency,
        due_date: params.dueDate,
      });
      return tx.result as string;
    });
  }

  /** #579: Mark an invoice as paid. Only valid while status is `Created`. */
  async markInvoicePaid(invoiceId: string): Promise<void> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).mark_invoice_paid({ invoice_id: invoiceId });
      return tx.result;
    });
  }

  /** #579: Fetch an on-chain invoice. Throws mapped `PaymentNotFound` if absent. */
  async getInvoice(invoiceId: string): Promise<unknown> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).get_invoice({ invoice_id: invoiceId });
      return tx.result;
    });
  }

  /** #579: List invoice IDs for a merchant (on-chain). */
  async getMerchantInvoices(merchantId: string): Promise<string[]> {
    const raw = await withMappedContractError(() =>
      (this.contract as any).get_merchant_invoices({ merchant_id: merchantId }),
    );
    return (raw.result ?? raw) as string[];
  }

  // ------------------------------------------------------------------ #580

  /** #580: Create an admin multi-sig proposal. Returns the proposal nonce. */
  async createProposal(signer: string, action: unknown): Promise<number> {
    const tx = await withMappedContractError(() =>
      (this.contract as any).create_proposal({ signer, action }),
    );
    const value = (tx as { result?: unknown }).result ?? tx;
    return Number(value as number | bigint);
  }

  /** #580: Vote on a proposal (multi-sig signer only, one vote per signer). */
  async voteProposal(signer: string, nonce: number | bigint): Promise<void> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).vote_proposal({ signer, nonce });
      return tx.result;
    });
  }

  /** #580: Execute a proposal once the multi-sig threshold is met (48h expiry). */
  async executeProposal(executor: string, nonce: number | bigint): Promise<void> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).execute_proposal({ executor, nonce });
      return tx.result;
    });
  }

  // ------------------------------------------------------------------ #581

  /** #581: Register a delegated session key for `account` (owner must sign). */
  async registerSessionKey(params: {
    account: string;
    sessionKey: string;
    /** Expiry timestamp (ledger seconds). */
    expiresAt: bigint;
  }): Promise<void> {
    const aa = this.accountAbstractionContract();
    return withMappedContractError(async () => {
      const tx = await aa.register_session_key({
        account: params.account,
        session_key: params.sessionKey,
        expires_at: params.expiresAt,
      });
      return tx.result;
    });
  }

  /** #581: Revoke a session key (owner must sign). */
  async revokeSessionKey(account: string, sessionKey: string): Promise<void> {
    const aa = this.accountAbstractionContract();
    return withMappedContractError(async () => {
      const tx = await aa.revoke_session_key({ account, session_key: sessionKey });
      return tx.result;
    });
  }

  /** #581: Execute a payload on behalf of `account` via a valid session key. */
  async executeWithSession(params: {
    account: string;
    sessionKey: string;
    /** Raw transaction payload bytes. */
    payload: Buffer;
  }): Promise<Buffer> {
    const aa = this.accountAbstractionContract();
    return withMappedContractError(async () => {
      const tx = await aa.execute_with_session({
        account: params.account,
        session_key: params.sessionKey,
        payload: params.payload,
      });
      return tx.result as Buffer;
    });
  }

  // ------------------------------------------------------------------ #582

  /** #582: Admin withdrawal of accumulated treasury fees to `destination`. */
  async withdrawTreasury(params: {
    admin: string;
    /** Amount in stroops; must be > 0 and <= treasury balance. */
    amount: bigint;
    destination: string;
  }): Promise<void> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).withdraw_treasury({
        admin: params.admin,
        amount: params.amount,
        destination: params.destination,
      });
      return tx.result;
    });
  }

  /** #582: Current accumulated treasury balance (read-only). */
  async getTreasuryBalance(): Promise<bigint> {
    const raw = await withMappedContractError(() =>
      (this.contract as any).get_treasury_balance({}),
    );
    const value = (raw as { result?: unknown }).result ?? raw;
    return BigInt(value as bigint | string | number);
  }

  /** #582: Set the platform fee split (admin only). treasuryBps + developerBps must be <= 10000. */
  async setFeeSplitConfig(admin: string, config: FeeSplitConfig): Promise<void> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).set_fee_split_config({
        admin,
        config: {
          treasury_bps: config.treasuryBps,
          developer_bps: config.developerBps,
          treasury_address: config.treasuryAddress,
          developer_address: config.developerAddress,
        },
      });
      return tx.result;
    });
  }

  /** #582: Read the current fee split config, or `null` when never configured. */
  async getFeeSplitConfig(): Promise<FeeSplitConfig | null> {
    const raw = await withMappedContractError(() =>
      (this.contract as any).get_fee_split_config({}),
    );
    const value = (raw as { result?: unknown }).result ?? raw;
    if (!value) return null;
    const v = value as { treasury_bps: number; developer_bps: number; treasury_address: string; developer_address: string };
    return {
      treasuryBps: Number(v.treasury_bps),
      developerBps: Number(v.developer_bps),
      treasuryAddress: v.treasury_address,
      developerAddress: v.developer_address,
    };
  }
}

export { resolveContractId };
