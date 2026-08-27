import { NetworkProfileSwitcher, NetworkEnvironment } from "../network-profiles.js";
import { withMappedContractError } from "../index.js";

export interface RefundManagerConfig {
  network: NetworkEnvironment;
  rpcUrl?: string;
  contractId: string;
}

/**
 * RefundManagerClient provides a high-level interface for interacting with the RefundManager contract.
 * Handles refund creation, processing, rejection, and cancellation operations.
 */
export class RefundManagerClient {
  private contract: any;
  public networkSwitcher: NetworkProfileSwitcher;
  private contractId: string;
  private rpcUrl: string;
  private networkPassphrase: string;

  constructor(config: RefundManagerConfig) {
    this.networkSwitcher = new NetworkProfileSwitcher(config.network);
    const profile = this.networkSwitcher.getProfile();
    this.rpcUrl = config.rpcUrl || profile.rpcUrl;
    this.networkPassphrase = profile.networkPassphrase;
    this.contractId = config.contractId;
    this.initializeContract();
  }

  private async initializeContract(): Promise<void> {
    // Contract will be lazily initialized on first use
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
   * @param environment - The target network environment (e.g., 'testnet', 'mainnet')
   * @param contractId - Optional contract ID for the new network
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
   * Create a new refund request for a payment.
   * @param paymentId - The ID of the payment to refund
   * @param refundAmount - The amount to refund in stroops
   * @param reason - The reason for the refund
   * @param requester - The address requesting the refund
   * @returns A promise resolving to the refund ID
   * @throws Error if the refund creation fails
   */
  async createRefund(
    paymentId: string,
    refundAmount: bigint,
    reason: string,
    requester: string,
  ): Promise<string> {
    return withMappedContractError(() =>
      this.getContract().create_refund({
        payment_id: paymentId,
        amount: refundAmount,
        reason: reason,
        requester: requester,
      }),
    );
  }

  /**
   * Process a pending refund, transferring funds to the requester.
   * @param operator - The address authorized to process refunds
   * @param refundId - The ID of the refund to process
   * @returns A promise that resolves when the refund is processed
   * @throws Error if processing fails
   */
  async processRefund(operator: string, refundId: string): Promise<void> {
    return withMappedContractError(() =>
      this.getContract().process_refund({
        operator: operator,
        refund_id: refundId,
      }),
    );
  }

  /**
   * Reject a pending refund request.
   * @param operator - The address authorized to reject refunds
   * @param refundId - The ID of the refund to reject
   * @returns A promise that resolves when the refund is rejected
   * @throws Error if rejection fails
   */
  async rejectRefund(operator: string, refundId: string): Promise<void> {
    return withMappedContractError(() =>
      this.getContract().reject_refund({
        operator: operator,
        refund_id: refundId,
      }),
    );
  }

  /**
   * Cancel a pending refund request.
   * @param requester - The address that created the refund
   * @param refundId - The ID of the refund to cancel
   * @returns A promise that resolves when the refund is cancelled
   * @throws Error if cancellation fails
   */
  async cancelRefund(requester: string, refundId: string): Promise<void> {
    return withMappedContractError(() =>
      this.getContract().cancel_refund({
        requester: requester,
        refund_id: refundId,
      }),
    );
  }

  /**
   * Retrieve details of a specific refund.
   * @param refundId - The ID of the refund to retrieve
   * @returns A promise resolving to the refund details
   * @throws Error if the refund is not found
   */
  async getRefund(refundId: string): Promise<any> {
    return withMappedContractError(() =>
      this.getContract().get_refund({
        refund_id: refundId,
      }),
    );
  }

  /**
   * Retrieve all refunds for a specific payment.
   * @param paymentId - The ID of the payment
   * @returns A promise resolving to an array of refund IDs for the payment
   * @throws Error if the payment is not found
   */
  async getPaymentRefunds(paymentId: string): Promise<string[]> {
    return withMappedContractError(() =>
      this.getContract().get_payment_refunds({
        payment_id: paymentId,
      }),
    );
  }

  /**
   * Issue #676: Read the consolidated refund policy — `require_receipt_hash`,
   * `refund_expiry_secs`, `refund_fee_bps`, and `cooldown_secs` — in one call.
   * @returns A promise resolving to the current `RefundPolicy`.
   */
  async getRefundPolicy(): Promise<RefundPolicy> {
    return withMappedContractError(() => this.getContract().get_refund_policy());
  }
}

/**
 * Issue #676: consolidated refund configuration, mirroring the on-chain
 * `RefundPolicy` struct returned by `get_refund_policy`.
 */
export interface RefundPolicy {
  requireReceiptHash: boolean;
  refundExpirySecs: bigint;
  refundFeeBps: bigint;
  cooldownSecs: bigint;
}
