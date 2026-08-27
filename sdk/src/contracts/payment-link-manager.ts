import { NetworkProfileSwitcher, NetworkEnvironment } from "../network-profiles.js";
import { withMappedContractError } from "../index.js";

export interface PaymentLinkManagerConfig {
  network: NetworkEnvironment;
  rpcUrl?: string;
  contractId: string;
}

/**
 * Represents a payment link stored on-chain in the PaymentLinkManager contract.
 */
export interface PaymentLink {
  /** Unique link identifier */
  link_id: string;
  /** Stellar address of the merchant that created the link */
  merchant: string;
  /** Optional fixed amount in stroops; undefined means the payer supplies the amount */
  amount?: bigint;
  /** Whether this link is currently active */
  active: boolean;
  /** USDC token contract address */
  usdc_token: string;
  /**
   * Arbitrary key/value metadata attached to the link.
   *
   * Limits (enforced on-chain): ≤20 keys, key ≤64 chars, value ≤256 chars.
   */
  metadata?: Record<string, string>;
  /** Canonical shareable checkout URL (`{base_url}/pay/{link_id}`), if configured */
  shareable_url?: string;
  /**
   * Per-link fee override in basis points (0-10000), set via
   * `setPaymentLinkFeeBps`. When unset, `useLink` falls back to the
   * contract-wide default fee (issue #663).
   */
  fee_bps?: bigint;
}

/**
 * Analytics summary for a payment link.
 *
 * Returned by `getLinkAnalytics`. `conversion_rate` is expressed in
 * basis points (bps): `(use_count * 10_000) / view_count`, or `0`
 * when the link has not been viewed yet.
 */
export interface LinkAnalytics {
  /** Number of times the link has been viewed */
  view_count: number;
  /** Number of times the link has been successfully used */
  use_count: number;
  /** Total revenue accumulated from successful uses (in USDC stroops) */
  total_revenue: string;
  /** Conversion rate in basis points (bps). 100 bps = 1%. */
  conversion_rate: number;
}

/**
 * Parameters for creating a new payment link.
 */
export interface CreateLinkParams {
  /** The merchant's Stellar address */
  merchant: string;
  /** Optional fixed amount in stroops */
  amount?: bigint;
  /** USDC token contract address */
  usdcToken: string;
  /**
   * Arbitrary metadata (e.g. product info, order reference).
   *
   * Limits (enforced on-chain): ≤20 keys, key ≤64 chars, value ≤256 chars.
   */
  metadata?: Record<string, string>;
  /**
   * Optional checkout base URL. When provided (or when admin has set a default
   * via `set_payment_base_url`), the link stores `{baseUrl}/pay/{linkId}` as
   * `shareable_url`.
   */
  baseUrl?: string;
  /** Optional link ID; when omitted the contract/caller supplies one */
  linkId?: string;
  currency?: string;
  description?: string;
}

/** Result of `createPaymentLink`, including QR-ready payload. */
export interface CreatePaymentLinkResult {
  linkId: string;
  /** Canonical shareable URL when a base URL was available; otherwise null */
  shareableUrl: string | null;
  /**
   * String suitable for QR code generation (the shareable URL when present,
   * otherwise the raw link ID).
   */
  qrCodeData: string;
}

/**
 * PaymentLinkManagerClient provides a high-level interface for interacting
 * with the PaymentLinkManager Soroban contract.
 */
export class PaymentLinkManagerClient {
  private contract: any;
  public networkSwitcher: NetworkProfileSwitcher;
  private contractId: string;
  private rpcUrl: string;
  private networkPassphrase: string;

  constructor(config: PaymentLinkManagerConfig) {
    this.networkSwitcher = new NetworkProfileSwitcher(config.network);
    const profile = this.networkSwitcher.getProfile();
    this.rpcUrl = config.rpcUrl || profile.rpcUrl;
    this.networkPassphrase = profile.networkPassphrase;
    this.contractId = config.contractId;
  }

  private getContract(): any {
    if (!this.contract) {
      const { Client } = require("@stellar/stellar-sdk/contract");
      this.contract = new Client({
        networkPassphrase: this.networkPassphrase,
        rpcUrl: this.rpcUrl,
        contractId: this.contractId,
      });
    }
    return this.contract;
  }

  /**
   * Switch the client to a different network environment.
   * @param environment - Target network environment
   * @param contractId - Optional new contract ID
   */
  public switchNetwork(environment: NetworkEnvironment, contractId?: string): void {
    this.networkSwitcher.switchEnvironment(environment);
    const profile = this.networkSwitcher.getProfile();
    this.rpcUrl = profile.rpcUrl;
    this.networkPassphrase = profile.networkPassphrase;
    if (contractId) {
      this.contractId = contractId;
    }
    this.contract = undefined;
  }

  /**
   * Create a new payment link.
   * @param params - Link creation parameters
   * @returns A promise resolving to the new link ID
   */
  async createLink(params: CreateLinkParams): Promise<string> {
    return withMappedContractError(() =>
      this.getContract().create_link({
        merchant: params.merchant,
        link_id: params.linkId,
        amount: params.amount,
        currency: params.currency ?? "USDC",
        description: params.description ?? "",
        expires_at: undefined,
        max_uses: undefined,
        direct_transfer: false,
        usdc_token: params.usdcToken,
        metadata: params.metadata,
        base_url: params.baseUrl,
      }),
    );
  }

  /**
   * Create a payment link and return shareable URL + QR payload.
   *
   * @param params - Link creation parameters (include `baseUrl` for a shareable URL)
   * @returns linkId, shareableUrl, and qrCodeData for invoice/QR embedding
   */
  async createPaymentLink(params: CreateLinkParams): Promise<CreatePaymentLinkResult> {
    const linkId = await this.createLink(params);
    let shareableUrl: string | null = null;
    try {
      shareableUrl = (await this.getLinkUrl(linkId)) ?? null;
    } catch {
      shareableUrl = params.baseUrl ? `${params.baseUrl.replace(/\/$/, "")}/pay/${linkId}` : null;
    }
    return {
      linkId,
      shareableUrl,
      qrCodeData: shareableUrl ?? linkId,
    };
  }

  /**
   * Query the on-chain shareable URL for a link.
   */
  async getLinkUrl(linkId: string): Promise<string | null> {
    return withMappedContractError(async () => {
      const url = await this.getContract().get_link_url({ link_id: linkId });
      return url ?? null;
    });
  }

  /**
   * Admin: set the default payment base URL used when create_link omits base_url.
   */
  async setPaymentBaseUrl(admin: string, url: string): Promise<void> {
    return withMappedContractError(() =>
      this.getContract().set_payment_base_url({ admin, url }),
    );
  }

  /**
   * Admin: set a per-link or contract-wide default fee override (basis
   * points, 0-10000) for payment links (issue #663).
   *
   * @param admin - The admin address (must match the address passed to `initialize`)
   * @param linkId - Set the override on this specific link, or `null` to set the
   *                 contract-wide default applied to links without their own override
   * @param feeBps - Fee in basis points, or `null` to clear the override
   */
  async setPaymentLinkFeeBps(
    admin: string,
    linkId: string | null,
    feeBps: bigint | null,
  ): Promise<void> {
    return withMappedContractError(() =>
      this.getContract().set_payment_link_fee_bps({
        admin,
        link_id: linkId ?? undefined,
        fee_bps: feeBps ?? undefined,
      }),
    );
  }

  /**
   * Get the fee (basis points) that `useLink` would currently apply to a
   * link: its own override if set, otherwise the contract-wide default,
   * otherwise `null` (no fee).
   */
  async getEffectiveFeeBps(linkId: string): Promise<bigint | null> {
    return withMappedContractError(async () => {
      const bps = await this.getContract().get_effective_fee_bps({ link_id: linkId });
      return bps ?? null;
    });
  }

  /**
   * Use a payment link to initiate a payment.
   * @param payer - The payer's Stellar address
   * @param linkId - The payment link ID
   * @param amount - The amount to pay in stroops
   * @param usdcToken - The USDC token contract address
   */
  async useLink(
    payer: string,
    linkId: string,
    amount: bigint,
    usdcToken: string,
  ): Promise<void> {
    return withMappedContractError(() =>
      this.getContract().use_link({
        payer,
        link_id: linkId,
        amount,
        usdc_token: usdcToken,
      }),
    );
  }

  /**
   * Deactivate a payment link (merchant only).
   * @param merchant - The merchant's Stellar address
   * @param linkId - The payment link ID to deactivate
   */
  async deactivateLink(merchant: string, linkId: string): Promise<void> {
    return withMappedContractError(() =>
      this.getContract().deactivate_link({
        merchant,
        link_id: linkId,
      }),
    );
  }

  /**
   * Retrieve details of a specific payment link.
   * @param linkId - The payment link ID
   * @returns A promise resolving to the PaymentLink details
   */
  async getLink(linkId: string): Promise<PaymentLink> {
    return withMappedContractError(() =>
      this.getContract().get_link({
        link_id: linkId,
      }),
    );
  }

  /**
   * Verify a batch of payment links, returning only the still-active ones.
   * @param linkIds - Array of link IDs to verify
   * @returns A promise resolving to an array of active link IDs
   */
  async verifyBatch(linkIds: string[]): Promise<string[]> {
    return withMappedContractError(() =>
      this.getContract().verify_batch({
        link_ids: linkIds,
      }),
    );
  }

  /**
   * Record a view of a payment link (permissionless).
   *
   * Increments the link's `view_count` so merchants can track
   * how many people viewed the link versus how many actually paid.
   * @param linkId - The payment link ID
   */
  async recordLinkView(linkId: string): Promise<void> {
    return withMappedContractError(() =>
      this.getContract().record_link_view({
        link_id: linkId,
      }),
    );
  }

  /**
   * Retrieve analytics for a payment link.
   *
   * Returns view_count, use_count, total_revenue, and conversion_rate
   * (in basis points: `use_count * 10_000 / view_count`, or `0` if
   * the link has not been viewed).
   * @param linkId - The payment link ID
   * @returns A promise resolving to the LinkAnalytics
   */
  async getLinkAnalytics(linkId: string): Promise<LinkAnalytics> {
    return withMappedContractError(() =>
      this.getContract().get_link_analytics({
        link_id: linkId,
      }),
    );
  }
}
