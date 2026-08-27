import {
  Client as ContractClient,
  Merchant,
  PaymentCharge,
  Refund,
  Dispute,
  PaymentStatus,
  RefundStatus,
  DisputeStatus,
  FeeConfig,
  MaybeFeeConfig,
  CreatePaymentArgs,
} from "./contracts/fluxapay/src/index.js";
import { Networks } from "@stellar/stellar-sdk";
import {
  FluxapayOfflineSigner,
  OfflineTransactionPayload,
  SubscriptionBillingClient,
  buildOfflinePayload,
  buildCreatePaymentPayload,
  buildVerifyPaymentPayload,
  buildCreateRefundPayload,
  buildSubscriptionTickPayload,
  buildPullAuthorizationPayload,
  prepareForOfflineSigning,
  restoreFromOfflinePayload,
} from "./offline-signer.js";
import {
  NetworkProfileSwitcher,
  NetworkEnvironment,
  NetworkProfiles,
  NetworkProfile,
  FLUXAPAY_CONTRACT_IDS,
  UNSET_CONTRACT_ID,
} from "./network-profiles.js";

export { FLUXAPAY_CONTRACT_IDS, UNSET_CONTRACT_ID } from "./network-profiles.js";
export type { FluxapayContractIds } from "./network-profiles.js";
import { FxOracleClient } from "./contracts/fx-oracle.js";
import { MerchantRegistryClient } from "./contracts/merchant-registry.js";
import {
  PaymentLinkManagerClient,
  type PaymentLinkManagerConfig,
  type PaymentLink,
  type LinkAnalytics,
  type CreateLinkParams,
  type CreatePaymentLinkResult,
} from "./contracts/payment-link-manager.js";
import { SEP10Authenticator, type SEP10ChallengeResponse, type SEP10AuthenticatedResponse } from "./sep10.js";



export interface FluxapayConfig {
  network: NetworkEnvironment;
  rpcUrl?: string;
  /**
   * PaymentProcessor contract ID. Optional — falls back to
   * `FLUXAPAY_CONTRACT_IDS[network].paymentProcessor` when omitted.
   */
  contractId?: string;
  /** FX Oracle contract ID for multi-currency rate queries. */
  oracleContractId?: string;
  /** MerchantRegistry contract ID for merchant management operations. */
  merchantRegistryContractId?: string;
  /** PaymentLinkManager contract ID for payment link operations. */
  paymentLinkContractId?: string;
  /**
   * Issue #680: Base URL of the FluxaPay backend API, used for off-chain
   * invoice management (`getInvoice`, `createInvoice`, etc). Required only
   * when invoice methods are used.
   */
  apiUrl?: string;
}

/**
 * Issue #680: A single line item on an invoice.
 */
export interface LineItem {
  description: string;
  quantity: number;
  unitAmount: bigint;
}

/**
 * Issue #680: Lifecycle status of an invoice.
 */
export type InvoiceStatus = "draft" | "sent" | "paid" | "void" | "expired";

/**
 * Issue #680: An invoice issued by a merchant, optionally linked to an
 * on-chain payment once paid.
 */
export interface Invoice {
  invoiceId: string;
  merchantId: string;
  customerId?: string;
  lineItems: LineItem[];
  currency: string;
  status: InvoiceStatus;
  paymentId?: string;
  createdAt: string;
  dueAt?: string;
}

export interface CreateInvoiceParams {
  merchantId: string;
  customerId?: string;
  lineItems: LineItem[];
  currency: string;
  dueAt?: string;
}

export interface CreatePaymentParams {
  paymentId: string;
  merchantId: string;
  amount: bigint;
  currency: string;
  depositAddress: string;
  expiresAt?: bigint;
  durationSecs?: bigint;
  memo?: string;
  memoType?: string;
  tokenAddress?: string;
  clientToken?: string;
  /**
   * Optional payment metadata map.
   *
   * Limits (enforced on-chain):
   * - At most 20 keys
   * - Each key ≤ 64 characters
   * - Each value ≤ 256 characters
   *
   * Violations return `MetadataTooLarge` (#49) or `MetadataValueTooLong` (#47).
   */
  metadata?: Record<string, string>;
  /**
   * Optional per-payment fee-waiver code. If the code is valid at
   * settlement time (exists, not expired, still has remaining uses), the
   * platform fee is waived. See `addFeeWaiverCode` for how admin registers
   * codes.
   */
  feeWaiverCode?: string;
}

/** Mirrors the on-chain `StreamStatus` enum in `stream.rs`. */
export type StreamStatus = "Active" | "Cancelled" | "Exhausted" | "Paused";

/** Mirrors the on-chain `StreamError` enum in `stream.rs`. */
export enum StreamError {
  StreamNotFound = 1,
  Unauthorized = 2,
  RateNotDecreased = 3,
  InvalidRate = 4,
  StreamAlreadyExists = 5,
  InvalidDeposit = 6,
  StreamNotActive = 7,
  DestinationNotSet = 8,
  ContractPaused = 9,
  MilestoneNotApproved = 10,
  WithdrawalInProgress = 11,
  RateBelowMinimum = 12,
  StreamNotPaused = 13,
  InvalidReceiver = 14,
}

/** Mirrors the on-chain `PaymentStream` struct in `stream.rs`. */
export interface PaymentStream {
  streamId: string;
  sender: string;
  receiver: string;
  destination: string | null;
  token: string;
  ratePerSecond: bigint;
  minRatePerSecond: bigint;
  remainingDeposit: bigint;
  lastCheckpointAt: bigint;
  accruedAtCheckpoint: bigint;
  status: StreamStatus;
  milestonesApproved: boolean;
}

export interface CreateStreamParams {
  sender: string;
  receiver: string;
  token: string;
  ratePerSecond: bigint;
  deposit: bigint;
  streamId: string;
}

function fromContractStream(raw: {
  stream_id: string;
  sender: string;
  receiver: string;
  destination?: string | null;
  token: string;
  rate_per_second: bigint;
  min_rate_per_second: bigint;
  remaining_deposit: bigint;
  last_checkpoint_at: bigint;
  accrued_at_checkpoint: bigint;
  status: StreamStatus;
  milestones_approved: boolean;
}): PaymentStream {
  return {
    streamId: raw.stream_id,
    sender: raw.sender,
    receiver: raw.receiver,
    destination: raw.destination ?? null,
    token: raw.token,
    ratePerSecond: raw.rate_per_second,
    minRatePerSecond: raw.min_rate_per_second,
    remainingDeposit: raw.remaining_deposit,
    lastCheckpointAt: raw.last_checkpoint_at,
    accruedAtCheckpoint: raw.accrued_at_checkpoint,
    status: raw.status,
    milestonesApproved: raw.milestones_approved,
  };
}

/** Max i128 value, used as a sentinel "withdraw everything accrued" amount. */
const I128_MAX = (1n << 127n) - 1n;

export interface RegisterMerchantParams {
  merchantId: string;
  businessName: string;
  settlementCurrency: string;
  payoutAddress?: string;
  bankAccount?: string;
  feeConfig?: FeeConfig;
}

/**
 * A customer's pre-authorization for a merchant to pull recurring payments,
 * mirroring `MerchantAuthorization` in `fluxapay/src/merchant_auth.rs`.
 */
export interface MerchantAuthorization {
  customer: string;
  merchant: string;
  token: string;
  limit_per_period: bigint;
  period_secs: bigint;
  period_start: bigint;
  pulled_this_period: bigint;
  active: boolean;
  created_at: bigint;
}

/** Error codes from `fluxapay/src/merchant_auth.rs::MerchantAuthError`. */
export const MerchantAuthError = {
  1: { message: "AuthorizationNotFound" },
  2: { message: "AuthorizationInactive" },
  3: { message: "LimitExceeded" },
  4: { message: "InvalidAmount" },
  5: { message: "Unauthorized" },
  6: { message: "AuthorizationAlreadyExists" },
} as const;

/**
 * Issue #185 / #665: Record of a dispute settled off-chain by mutual
 * agreement between buyer and merchant, mirroring `CollaborativeSettlement`
 * in `fluxapay/src/lib.rs`.
 */
export interface CollaborativeSettlement {
  dispute_id: string;
  settlement_amount: bigint;
  buyer_pubkey: Buffer;
  merchant_pubkey: Buffer;
  settled_at: bigint;
}

/**
 * Issue #664: A single usage-metering record for a subscription, mirroring
 * `UsageMetrics` in `fluxapay/src/lib.rs`.
 */
export interface UsageMetrics {
  subscription_id: string;
  units_used: bigint;
  unit_price: bigint;
  amount: bigint;
  recorded_at: bigint;
}

export interface UpdateMerchantParams {
  merchantId: string;
  businessName?: string;
  settlementCurrency?: string;
  active?: boolean;
  payoutAddress?: string;
  bankAccount?: string;
  feeConfig?: FeeConfig;
}

/**
 * Issue #666: Aggregated platform fee report for a queried time period,
 * returned by `PaymentProcessor.get_platform_fee_report`.
 */
export interface PlatformFeeReport {
  totalFeesCollected: bigint;
  treasuryShare: bigint;
  developerShare: bigint;
  paymentCount: bigint;
}

/**
 * Maps numeric contract error codes to their name, for the main `Error` enum
 * shared by the `PaymentProcessor` and `RefundManager` contracts
 * (`fluxapay/src/lib.rs`).
 *
 * Other contracts (`AccessControlError`, `StreamError`, `FXOracleError`,
 * `MerchantError`, `MerchantAuthError`, `DexRouterError`,
 * `AccountAbstractionError`) each define their own independent, overlapping
 * code space, so a single flat `code -> name` map cannot disambiguate them —
 * see `docs/error-codes.md` for the full per-contract reference.
 *
 * Codes `46` and `54` are ambiguous even within this enum: multiple variants
 * share the same discriminant in the Rust source (a known issue tracked in
 * `docs/error-codes.md`). The lower/first-declared variant name is used
 * here; `scripts/check-error-map-sync.ts` flags this file if it drifts from
 * `fluxapay/src/lib.rs` again.
 */
export const FLUXAPAY_CONTRACT_ERROR_MAP: Record<number, string> = {
  1: "Unauthorized",
  2: "PaymentAlreadyExists",
  3: "PaymentExpired",
  4: "InvalidPaymentId",
  8: "RefundAlreadyProcessed",
  9: "DisputeNotFound",
  12: "DisputeAlreadyResolved",
  14: "PaymentAlreadyProcessed",
  15: "AccessControlError",
  16: "RefundExceedsPayment",
  17: "ContractPaused",
  18: "RateLimitExceeded",
  19: "RefundCancelled",
  20: "UnsupportedToken",
  21: "AmountBelowMin",
  22: "AmountAboveMax",
  23: "InvalidExpiry",
  24: "InvalidSettlement",
  25: "DuplicateIdempotencyKey",
  26: "InvalidAddress",
  27: "ArbitrageDetected",
  28: "SwapPathInvalid",
  29: "OraclePriceDeviation",
  30: "SubscriptionInGracePeriod",
  31: "SubscriptionRetryExhausted",
  32: "InvalidResumeTimestamp",
  33: "MerchantAuthError",
  34: "TierVolumeLimitExceeded",
  35: "BatchTooLarge",
  36: "RefundExpired",
  37: "InsufficientArbitrators",
  38: "ArbitrationVotingThresholdNotMet",
  39: "FeeProposalNotReady",
  40: "InvalidEvidenceFormat",
  41: "InvalidSettlementSignature",
  42: "RefundCooldownNotElapsed",
  43: "Reentrancy",
  44: "NoFeeProposal",
  45: "StaleOracleRate",
  46: "LinkExpired", // ambiguous: also InsufficientTreasuryBalance = 46
  47: "MetadataValueTooLong",
  48: "UpgradeFailed",
  49: "MetadataTooLarge",
  50: "InvalidMemoType",
  51: "MemoTooLong",
  52: "InvalidMemoId",
  53: "PayerNotWhitelisted",
  54: "DisputeRateLimitExceeded", // ambiguous: also LinkMaxUsesReached, DirectTransferNotDisputable, MaxRetriesExceeded, InvalidStatusTransition = 54
  55: "RateDeviationExceeded",
  404: "PaymentNotFound",
  405: "RefundNotFound",
  406: "InvalidAmount",
};

export class FluxapayError extends Error {
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

function toFluxapayError(error: unknown): FluxapayError {
  const code = parseContractErrorCode(error);
  if (code === null) {
    if (error instanceof Error) {
      throw error;
    }
    throw new Error("Unknown Fluxapay SDK error");
  }

  const contractErrorName = FLUXAPAY_CONTRACT_ERROR_MAP[code] ?? "UnknownContractError";
  return new FluxapayError(
    code,
    contractErrorName,
    `${contractErrorName} (contract error #${code})`,
    error,
  );
}

async function withMappedContractError<T>(operation: () => Promise<T>): Promise<T> {
  try {
    return await operation();
  } catch (error) {
    throw toFluxapayError(error);
  }
}

function toCreatePaymentArgs(params: CreatePaymentParams): CreatePaymentArgs {
  return {
    payment_id: params.paymentId,
    merchant_id: params.merchantId,
    amount: params.amount,
    currency: params.currency,
    deposit_address: params.depositAddress,
    expires_at: params.expiresAt,
    duration_secs: params.durationSecs,
    memo: params.memo,
    memo_type: params.memoType,
    token_address: params.tokenAddress,
    client_token: params.clientToken,
    metadata_hash: undefined,
    metadata: params.metadata,
    fee_waiver_code: params.feeWaiverCode,
  };
}

/**
 * Resolve a contract ID: prefer the explicit override, otherwise fall back
 * to the per-environment default. Throws if neither is set to a real
 * (non-placeholder) address, so misconfiguration fails fast with a clear
 * message instead of an opaque RPC error at call time.
 */
function resolveContractId(explicit: string | undefined, fallback: string, label: string): string {
  const contractId = explicit || fallback;
  if (!contractId || contractId === UNSET_CONTRACT_ID) {
    throw new Error(
      `${label} is required: pass it explicitly in FluxapayConfig, or deploy the contract and populate FLUXAPAY_CONTRACT_IDS.`,
    );
  }
  return contractId;
}

export class FluxapayClient {
  public contract: ContractClient;
  public networkSwitcher: NetworkProfileSwitcher;
  private fxOracleClient?: FxOracleClient;
  private merchantRegistryClient?: MerchantRegistryClient;
  private paymentLinkManagerClient?: PaymentLinkManagerClient;
  private sep10Authenticator?: SEP10Authenticator;
  private readonly config: FluxapayConfig;

  constructor(config: FluxapayConfig) {
    this.config = config;
    this.networkSwitcher = new NetworkProfileSwitcher(config.network);

    const rpcUrl = config.rpcUrl || this.networkSwitcher.getProfile().rpcUrl;
    const contractId = resolveContractId(
      config.contractId,
      FLUXAPAY_CONTRACT_IDS[config.network].paymentProcessor,
      "contractId (PaymentProcessor)",
    );

    this.contract = new ContractClient({
      networkPassphrase: this.networkSwitcher.getProfile().networkPassphrase,
      rpcUrl: rpcUrl,
      contractId,
    });
  }

  private getMerchantRegistry(): MerchantRegistryClient {
    const contractId = resolveContractId(
      this.config.merchantRegistryContractId,
      FLUXAPAY_CONTRACT_IDS[this.config.network].merchantRegistry,
      "merchantRegistryContractId",
    );

    if (!this.merchantRegistryClient) {
      const profile = this.networkSwitcher.getProfile();
      this.merchantRegistryClient = new MerchantRegistryClient({
        network: profile.environment,
        rpcUrl: this.config.rpcUrl || profile.rpcUrl,
        contractId,
      });
    }

    return this.merchantRegistryClient;
  }

  /**
   * Get an FX Oracle client, using `oracleContractId` from config when
   * provided, falling back to `FLUXAPAY_CONTRACT_IDS[network].fxOracle`.
   */
  fxOracle(): FxOracleClient {
    const oracleContractId = resolveContractId(
      this.config.oracleContractId,
      FLUXAPAY_CONTRACT_IDS[this.config.network].fxOracle,
      "oracleContractId",
    );

    if (!this.fxOracleClient) {
      const profile = this.networkSwitcher.getProfile();
      this.fxOracleClient = new FxOracleClient({
        network: profile.environment,
        rpcUrl: this.config.rpcUrl || profile.rpcUrl,
        oracleContractId,
      });
    }

    return this.fxOracleClient;
  }

  /**
   * Switch the client to a different network environment.
   * This re-initializes the contract client seamlessly.
   */
  public switchNetwork(environment: NetworkEnvironment, contractId?: string): void {
    this.networkSwitcher.switchEnvironment(environment);
    const profile = this.networkSwitcher.getProfile();
    const newContractId = contractId || profile.defaultContractId || this.contract.options.contractId;

    this.contract = new ContractClient({
      networkPassphrase: profile.networkPassphrase,
      rpcUrl: profile.rpcUrl,
      contractId: newContractId,
    });
    this.fxOracleClient = undefined;
    this.merchantRegistryClient = undefined;
    this.paymentLinkManagerClient = undefined;
    this.sep10Authenticator = undefined;
  }

  /**
   * Issue #490: Initialize SEP-10 authenticator for merchant authentication.
   * Must be called before using SEP-10 authentication methods.
   */
  public initSEP10(serverPublicKey: string, homeDomain?: string): void {
    const profile = this.networkSwitcher.getProfile();
    this.sep10Authenticator = new SEP10Authenticator(
      serverPublicKey,
      profile.networkPassphrase,
      homeDomain,
    );
  }

  /**
   * Issue #490: Generate a SEP-10 challenge for a merchant keypair.
   */
  public generateSEP10Challenge(merchantPublicKey: string): SEP10ChallengeResponse {
    if (!this.sep10Authenticator) {
      throw new Error("SEP10 authenticator not initialized. Call initSEP10() first.");
    }
    return this.sep10Authenticator.generateChallenge(merchantPublicKey);
  }

  /**
   * Issue #490: Verify a signed SEP-10 challenge and return JWT for API access.
   */
  public authorizeSEP10(
    challengeXdr: string,
    signedXdr: string,
    merchantPublicKey: string,
  ): SEP10AuthenticatedResponse {
    if (!this.sep10Authenticator) {
      throw new Error("SEP10 authenticator not initialized. Call initSEP10() first.");
    }
    return this.sep10Authenticator.authenticate(challengeXdr, signedXdr, merchantPublicKey);
  }

  /**
   * Create a new payment charge
   */
  async createPayment(params: CreatePaymentParams) {
    return withMappedContractError(() =>
      this.contract.create_payment(toCreatePaymentArgs(params)),
    );
  }

  /**
   * Verify a payment via oracle
   */
  async verifyPayment(params: {
    oracle: string;
    paymentId: string;
    transactionHash: Buffer;
    payerAddress: string;
    amountReceived: bigint;
  }) {
    return withMappedContractError(() =>
      this.contract.verify_payment({
        oracle: params.oracle,
        payment_id: params.paymentId,
        transaction_hash: params.transactionHash,
        payer_address: params.payerAddress,
        amount_received: params.amountReceived,
      }),
    );
  }

  /**
   * Register a new merchant in the MerchantRegistry contract
   */
  async registerMerchant(params: RegisterMerchantParams) {
    if (this.config.merchantRegistryContractId) {
      return this.getMerchantRegistry().registerMerchant(params);
    }

    return withMappedContractError(() =>
      this.contract.register_merchant({
        merchant_id: params.merchantId,
        business_name: params.businessName,
        settlement_currency: params.settlementCurrency,
        payout_address: params.payoutAddress,
        bank_account: params.bankAccount,
        fee_config: params.feeConfig,
      }),
    );
  }

  /**
   * Update merchant settings in the MerchantRegistry contract
   */
  async updateMerchant(params: UpdateMerchantParams) {
    if (this.config.merchantRegistryContractId) {
      return this.getMerchantRegistry().updateMerchant(params);
    }

    return withMappedContractError(() =>
      this.contract.update_merchant({
        merchant_id: params.merchantId,
        business_name: params.businessName,
        settlement_currency: params.settlementCurrency,
        active: params.active,
        payout_address: params.payoutAddress,
        bank_account: params.bankAccount,
        fee_config: params.feeConfig,
      }),
    );
  }

  /**
   * Get merchant details
   */
  async getMerchant(merchantId: string) {
    if (this.config.merchantRegistryContractId) {
      return this.getMerchantRegistry().getMerchant(merchantId);
    }

    return withMappedContractError(() =>
      this.contract.get_merchant({
        merchant_id: merchantId,
      }),
    );
  }

  /**
   * Verify a merchant (admin only)
   */
  async verifyMerchant(admin: string, merchantId: string) {
    if (this.config.merchantRegistryContractId) {
      return this.getMerchantRegistry().verifyMerchant(admin, merchantId);
    }

    return withMappedContractError(() =>
      this.contract.verify_merchant({
        admin,
        merchant_id: merchantId,
      }),
    );
  }

  /**
   * Apply or clear a time-based fee waiver for a merchant.
   *
   * Requires the MerchantRegistry admin signer. Delegates to
   * `MerchantRegistryClient.setMerchantFeeWaiver` when the merchant registry
   * contract ID is configured; falls back to calling the underlying
   * `set_merchant_fee_waiver` on the main contract (MerchantRegistry
   * embedded path).
   */
  async setMerchantFeeWaiver(params: {
    admin: string;
    merchantId: string;
    expiresAt?: bigint | null;
  }) {
    if (this.config.merchantRegistryContractId) {
      return this.getMerchantRegistry().setMerchantFeeWaiver(params);
    }
    return withMappedContractError(() =>
      this.contract.set_merchant_fee_waiver({
        admin: params.admin,
        merchant_id: params.merchantId,
        expires_at:
          params.expiresAt === null || params.expiresAt === undefined
            ? null
            : params.expiresAt,
      }),
    );
  }

  /**
   * Admin-only: register a reusable fee-waiver code for per-payment zero-fee
   * promotions on the PaymentProcessor.
   *
   * Merchants pass the returned `code` via `CreatePaymentParams.feeWaiverCode`
   * at `createPayment` time; at settlement, a valid code waives the platform
   * fee and atomically decrements the code's `remainingUses` counter.
   *
   * Requires the PaymentProcessor ADMIN role.
   *
   * @param admin            – ADMIN signer for the PaymentProcessor contract
   * @param code             – case-sensitive promo code string (e.g. "LAUNCH2026")
   * @param expiresAt        – ledger timestamp (seconds) after which the code is rejected
   * @param maxUses          – maximum total payments that can consume this code (>=1)
   */
  async addFeeWaiverCode(params: {
    admin: string;
    code: string;
    expiresAt: bigint;
    maxUses: number;
  }) {
    return withMappedContractError(() =>
      this.contract.add_fee_waiver_code({
        admin: params.admin,
        code: params.code,
        expires_at: params.expiresAt,
        max_uses: params.maxUses,
      }),
    );
  }

  /**
   * Issue #666: Aggregate platform fee collection over `[fromTs, toTs]`
   * (inclusive, ledger timestamps in seconds), for treasury reporting.
   *
   * Read-only — no authorization required.
   */
  async getPlatformFeeReport(fromTs: bigint, toTs: bigint): Promise<PlatformFeeReport> {
    const result = await withMappedContractError(() =>
      this.contract.get_platform_fee_report({
        from_ts: fromTs,
        to_ts: toTs,
      }),
    );
    return {
      totalFeesCollected: result.total_fees_collected,
      treasuryShare: result.treasury_share,
      developerShare: result.developer_share,
      paymentCount: result.payment_count,
    };
  }

  /**
   * Issue #660: Add an address to the global compliance blacklist.
   * Blacklisted addresses are rejected as payer, merchant, or requester on
   * subsequent payment/refund/dispute operations.
   *
   * Requires the PaymentProcessor ADMIN role.
   */
  async addToBlacklist(admin: string, address: string): Promise<void> {
    return withMappedContractError(() =>
      this.contract.add_to_blacklist({
        admin,
        address,
      }),
    );
  }

  /**
   * Issue #660: Remove an address from the global compliance blacklist.
   *
   * Requires the PaymentProcessor ADMIN role.
   */
  async removeFromBlacklist(admin: string, address: string): Promise<void> {
    return withMappedContractError(() =>
      this.contract.remove_from_blacklist({
        admin,
        address,
      }),
    );
  }

  /**
   * Issue #660: Check whether an address is currently blacklisted.
   *
   * Read-only — no authorization required.
   */
  async isBlacklisted(address: string): Promise<boolean> {
    return withMappedContractError(() =>
      this.contract.is_blacklisted({
        address,
      }),
    );
  }

  /**
   * Create a refund request
   */
  async createRefund(params: {
    paymentId: string;
    amount: bigint;
    reason: string;
    requester: string;
  }) {
    return withMappedContractError(() =>
      this.contract.create_refund({
        payment_id: params.paymentId,
        refund_amount: params.amount,
        reason: params.reason,
        requester: params.requester,
      }),
    );
  }

  /**
   * Process a pending refund
   */
  async processRefund(operator: string, refundId: string) {
    return withMappedContractError(() =>
      this.contract.process_refund({
        operator,
        refund_id: refundId,
      }),
    );
  }

  /**
   * Issue #676: Read the consolidated refund policy — `require_receipt_hash`,
   * `refund_expiry_secs`, `refund_fee_bps`, and `cooldown_secs` — in one call.
   */
  async getRefundPolicy() {
    return withMappedContractError(() => this.contract.get_refund_policy());
  }

  /**
   * Get refund details by ID
   */
  async getRefund(refundId: string) {
    return withMappedContractError(() =>
      this.contract.get_refund({
        refund_id: refundId,
      }),
    );
  }

  // ── Merchant pre-authorization (pull billing, #454) ─────────────────────────
  //
  // These delegate to `pre_authorize_merchant` / `pull_payment` /
  // `revoke_merchant_authorization` / `get_merchant_authorization`, entry
  // points already exposed on `PaymentProcessor` (see
  // `fluxapay/src/lib.rs`). They're invoked via a loose cast because the
  // checked-in `contracts/fluxapay` bindings predate these entry points;
  // regenerating bindings with `npm run generate` (see `scripts/generate-sdk.sh`)
  // against a freshly built contract will pick up proper typings, at which
  // point the `as any` casts below can be removed.

  /**
   * Customer grants a merchant permission to pull up to `limitPerPeriod`
   * tokens per `periodSecs`-second billing window.
   */
  async preAuthorizeMerchant(params: {
    customer: string;
    merchant: string;
    token: string;
    limitPerPeriod: bigint;
    periodSecs: bigint;
  }): Promise<MerchantAuthorization> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).pre_authorize_merchant({
        customer: params.customer,
        merchant: params.merchant,
        token: params.token,
        limit_per_period: params.limitPerPeriod,
        period_secs: params.periodSecs,
      });
      return tx.result;
    });
  }

  /**
   * Merchant pulls `amount` tokens from `customer` against an existing
   * pre-authorization. Returns the cumulative amount pulled this period.
   */
  async pullFromAuthorization(
    merchant: string,
    customer: string,
    amount: bigint,
  ): Promise<bigint> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).pull_payment({
        merchant,
        customer,
        amount,
      });
      return tx.result;
    });
  }

  /**
   * Customer revokes a previously granted merchant authorization.
   */
  async revokeAuthorization(customer: string, merchant: string): Promise<void> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).revoke_merchant_authorization({
        customer,
        merchant,
      });
      return tx.result;
    });
  }

  /**
   * Fetch the stored authorization for a (customer, merchant) pair, or
   * `null` if none exists.
   */
  async getAuthorization(
    customer: string,
    merchant: string,
  ): Promise<MerchantAuthorization | null> {
    try {
      return await withMappedContractError(async () => {
        const tx = await (this.contract as any).get_merchant_authorization({
          customer,
          merchant,
        });
        return tx.result;
      });
    } catch (error) {
      // Note: `FLUXAPAY_CONTRACT_ERROR_MAP` only covers the main `Error`
      // enum, whose code space overlaps with `MerchantAuthError`'s — code 1
      // means `AuthorizationNotFound` here, not the mapped "Unauthorized"
      // name (see docs/error-codes.md). Check the raw code, not the name.
      if (error instanceof FluxapayError && error.code === 1) {
        return null;
      }
      throw error;
    }
  }

  /**
   * Get all refunds for a payment
   */
  async getPaymentRefunds(paymentId: string) {
    return withMappedContractError(() =>
      this.contract.get_payment_refunds({
        payment_id: paymentId,
      }),
    );
  }

  /**
   * Create a dispute for a payment
   */
  async createDispute(params: {
    paymentId: string;
    amount: bigint;
    reason: string;
    evidence: string;
    disputer: string;
  }) {
    return withMappedContractError(() =>
      this.contract.create_dispute({
        payment_id: params.paymentId,
        amount: params.amount,
        reason: params.reason,
        evidence: params.evidence,
        disputer: params.disputer,
      }),
    );
  }

  /**
   * Move a dispute to under-review status
   */
  async reviewDispute(operator: string, disputeId: string) {
    return withMappedContractError(() =>
      this.contract.review_dispute({
        operator,
        dispute_id: disputeId,
      }),
    );
  }

  /**
   * Resolve a dispute by issuing a refund
   */
  async resolveDisputeWithRefund(
    operator: string,
    disputeId: string,
    notes: string,
  ) {
    return withMappedContractError(() =>
      this.contract.resolve_dispute_with_refund({
        operator,
        dispute_id: disputeId,
        resolution_notes: notes,
      }),
    );
  }

  /**
   * Reject a dispute
   */
  async rejectDispute(operator: string, disputeId: string, notes: string) {
    return withMappedContractError(() =>
      this.contract.reject_dispute({
        operator,
        dispute_id: disputeId,
        resolution_notes: notes,
      }),
    );
  }

  /**
   * Get dispute details by ID
   */
  async getDispute(disputeId: string) {
    return withMappedContractError(() =>
      this.contract.get_dispute({
        dispute_id: disputeId,
      }),
    );
  }

  /**
   * Get all disputes for a payment
   */
  async getPaymentDisputes(paymentId: string) {
    return withMappedContractError(() =>
      this.contract.get_payment_disputes({
        payment_id: paymentId,
      }),
    );
  }

  /**
   * Issue #659: Merchant accepts a `PartiallyPaid` payment at the amount
   * actually received, moving it to `Confirmed` without issuing a refund
   * for the shortfall.
   * Maps to `PaymentProcessor.accept_partial_payment` on-chain.
   *
   * @param authority - The merchant's Stellar address (must sign; must match
   * `payment.merchant_id`).
   * @param paymentId - The `PartiallyPaid` payment to accept.
   */
  async acceptPartialPayment(authority: string, paymentId: string): Promise<void> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).accept_partial_payment({
        merchant_id: authority,
        payment_id: paymentId,
      });
      return tx.result;
    });
  }

  /**
   * Issue #659: Customer tops up a `PartiallyPaid` payment with additional
   * funds, moving it back to `Pending` so a subsequent `verifyPayment` call
   * can confirm it with the combined amount.
   * Maps to `PaymentProcessor.complete_partial_payment` on-chain.
   *
   * @param operator - The payer's Stellar address (must sign).
   * @param paymentId - The `PartiallyPaid` payment to top up.
   * @param topUpAmount - Additional amount (in stroops) being sent, must be > 0.
   */
  async completePartialPayment(
    operator: string,
    paymentId: string,
    topUpAmount: bigint,
  ): Promise<void> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).complete_partial_payment({
        payer: operator,
        payment_id: paymentId,
        top_up_amount: topUpAmount,
      });
      return tx.result;
    });
  }

  /**
   * Get payment details
   */
  async getPayment(paymentId: string) {
    return withMappedContractError(() =>
      this.contract.get_payment({ payment_id: paymentId }),
    );
  }

  /**
   * Issue #489: Get payment by metadata_hash for order reconciliation.
   * Performs reverse lookup using the merchant-supplied metadata hash.
   */
  async getPaymentByMetadataHash(metadataHash: Buffer) {
    return withMappedContractError(() =>
      this.contract.get_payment_by_metadata_hash({ metadata_hash: metadataHash }),
    );
  }

  /**
   * Issue #492: Get customer profile for a merchant.
   */
  async getCustomer(merchantId: string, customerId: string) {
    return withMappedContractError(() =>
      this.contract.get_customer({ merchant_id: merchantId, customer_id: customerId }),
    );
  }

  /**
   * Issue #492: Get top customers for a merchant sorted by total spending.
   */
  async getTopCustomers(merchantId: string, limit: number) {
    return withMappedContractError(() =>
      this.contract.get_top_customers({ merchant_id: merchantId, limit }),
    );
  }

  /**
   * Issue #488: Public TTL bump for a single payment (permissionless).
   */
  async bumpPaymentTTL(paymentId: string) {
    return withMappedContractError(() =>
      this.contract.bump_payment_ttl_public({ payment_id: paymentId }),
    );
  }

  /**
   * Issue #488: Bulk bump TTLs for payment maintenance (max 50 per call).
   */
  async bulkBumpPaymentTTLs(paymentIds: string[]) {
    return withMappedContractError(() =>
      this.contract.bulk_bump_payment_ttls({ payment_ids: paymentIds }),
    );
  }

  /**
   * Issue #665: Close a dispute instantly when both the buyer and merchant
   * have agreed on a settlement amount off-chain and submit their Ed25519
   * signatures over `SHA-256(dispute_id || settlement_amount_le16)`.
   *
   * Wraps `RefundManager::settle_dispute_collaboratively`. Note: bindings
   * for this entry point haven't been regenerated yet (TODO: `npm run
   * generate`), so this calls through `this.contract` untyped.
   *
   * @returns The refund ID created for the settlement.
   */
  async settleDisputeCollaboratively(params: {
    disputeId: string;
    settlementAmount: bigint;
    buyerPubkey: Buffer;
    signatureBuyer: Buffer;
    merchantPubkey: Buffer;
    signatureMerchant: Buffer;
  }): Promise<string> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).settle_dispute_collaboratively({
        dispute_id: params.disputeId,
        settlement_amount: params.settlementAmount,
        buyer_pubkey: params.buyerPubkey,
        signature_buyer: params.signatureBuyer,
        merchant_pubkey: params.merchantPubkey,
        signature_merchant: params.signatureMerchant,
      });
      return tx.result;
    });
  }

  /**
   * Issue #665: Retrieve the collaborative settlement record for a dispute,
   * or `null` if the dispute has no such record (or doesn't exist —
   * `DisputeNotFound`).
   */
  async getCollaborativeSettlement(disputeId: string): Promise<CollaborativeSettlement | null> {
    try {
      return await withMappedContractError(async () => {
        const tx = await (this.contract as any).get_collaborative_settlement({
          dispute_id: disputeId,
        });
        return tx.result;
      });
    } catch (error) {
      if (error instanceof FluxapayError && error.contractErrorName === "DisputeNotFound") {
        return null;
      }
      throw error;
    }
  }

  /**
   * Issue #664: Submit usage metrics for a metered subscription. The
   * subscription's charge amount is overridden to `units * unitPrice` and
   * charged immediately. Throws a mapped `InvalidStatusTransition` error if
   * the subscription is Cancelled/Expired (not currently billable).
   *
   * Wraps `RefundManager::submit_usage_metrics`. Note: bindings for this
   * entry point haven't been regenerated yet (TODO: `npm run generate`),
   * so this calls through `this.contract` untyped.
   */
  async submitUsageMetrics(params: {
    subscriptionId: string;
    units: bigint;
    unitPrice: bigint;
    token: string;
    caller: string;
  }): Promise<void> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).submit_usage_metrics({
        operator: params.caller,
        subscription_id: params.subscriptionId,
        units_used: params.units,
        unit_price: params.unitPrice,
        token: params.token,
      });
      return tx.result;
    });
  }

  /**
   * Issue #664: Retrieve usage-metric records for a subscription recorded
   * within `[fromTimestamp, toTimestamp]` (inclusive), oldest first.
   */
  async getUsageMetrics(
    subscriptionId: string,
    fromTimestamp: number,
    toTimestamp: number,
  ): Promise<UsageMetrics[]> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).get_usage_metrics({
        subscription_id: subscriptionId,
        from_timestamp: BigInt(fromTimestamp),
        to_timestamp: BigInt(toTimestamp),
      });
      return tx.result;
    });
   * Issue #680: Resolve the configured backend API URL, throwing a clear
   * error if invoice methods are used without one.
   */
  private getApiUrl(): string {
    if (!this.config.apiUrl) {
      throw new Error(
        "apiUrl is required in FluxapayConfig to use invoice methods.",
      );
    }
    return this.config.apiUrl.replace(/\/$/, "");
  }

  /**
   * Issue #680: Fetch a single invoice by id from the FluxaPay backend.
   */
  async getInvoice(invoiceId: string): Promise<Invoice> {
    const res = await fetch(`${this.getApiUrl()}/invoices/${invoiceId}`);
    if (!res.ok) {
      throw new Error(`Failed to fetch invoice ${invoiceId}: ${res.status}`);
    }
    return res.json();
  }

  /**
   * Issue #680: List invoice ids for a merchant from the FluxaPay backend.
   */
  async getMerchantInvoices(merchantId: string): Promise<string[]> {
    const res = await fetch(`${this.getApiUrl()}/merchants/${merchantId}/invoices`);
    if (!res.ok) {
      throw new Error(`Failed to fetch invoices for merchant ${merchantId}: ${res.status}`);
    }
    return res.json();
  }

  /**
   * Issue #680: Create a new invoice via the FluxaPay backend.
   */
  async createInvoice(params: CreateInvoiceParams): Promise<Invoice> {
    const res = await fetch(`${this.getApiUrl()}/invoices`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(params),
    });
    if (!res.ok) {
      throw new Error(`Failed to create invoice: ${res.status}`);
    }
    return res.json();
  }

  /**
   * Issue #680: Mark an invoice as paid via the FluxaPay backend.
   */
  async markInvoicePaid(invoiceId: string): Promise<void> {
    const res = await fetch(`${this.getApiUrl()}/invoices/${invoiceId}/mark-paid`, {
      method: "POST",
    });
    if (!res.ok) {
      throw new Error(`Failed to mark invoice ${invoiceId} as paid: ${res.status}`);
    }
  }

  private getPaymentLinkManager(): PaymentLinkManagerClient {
    const contractId = resolveContractId(
      this.config.paymentLinkContractId,
      FLUXAPAY_CONTRACT_IDS[this.config.network].paymentLinkManager,
      "paymentLinkContractId",
    );

    if (!this.paymentLinkManagerClient) {
      const profile = this.networkSwitcher.getProfile();
      this.paymentLinkManagerClient = new PaymentLinkManagerClient({
        network: profile.environment,
        rpcUrl: this.config.rpcUrl || profile.rpcUrl,
        contractId,
      });
    }

    return this.paymentLinkManagerClient;
  }

  /**
   * Create a new payment link.
   * Maps to `PaymentLinkManager.create_link` on-chain.
   * @param params.merchant - The merchant's Stellar address
   * @param params.amount - Optional fixed amount in stroops
   * @param params.usdcToken - USDC token contract address
   * @param params.metadata - Optional key/value metadata (≤20 keys, key≤64, value≤256)
   * @param params.baseUrl - Optional checkout base URL for shareable_url
   * @returns A promise resolving to the new link ID
   */
  async createLink(params: CreateLinkParams): Promise<string> {
    return this.getPaymentLinkManager().createLink(params);
  }

  /**
   * Create a payment link and return shareable URL + QR code payload.
   *
   * @returns `{ linkId, shareableUrl, qrCodeData }` where `qrCodeData` is the
   * shareable URL (or link ID fallback) suitable for QR generation.
   */
  async createPaymentLink(params: CreateLinkParams): Promise<CreatePaymentLinkResult> {
    return this.getPaymentLinkManager().createPaymentLink(params);
  }

  /**
   * Query the on-chain shareable URL for a payment link.
   */
  async getLinkUrl(linkId: string): Promise<string | null> {
    return this.getPaymentLinkManager().getLinkUrl(linkId);
  }

  /**
   * Use a payment link to initiate a payment.
   * Maps to `PaymentLinkManager.use_link` on-chain.
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
    return this.getPaymentLinkManager().useLink(payer, linkId, amount, usdcToken);
  }

  /**
   * Deactivate a payment link (merchant only).
   * Maps to `PaymentLinkManager.deactivate_link` on-chain.
   * @param merchant - The merchant's Stellar address
   * @param linkId - The payment link ID to deactivate
   */
  async deactivateLink(merchant: string, linkId: string): Promise<void> {
    return this.getPaymentLinkManager().deactivateLink(merchant, linkId);
  }

  /**
   * Retrieve details of a specific payment link.
   * Maps to `PaymentLinkManager.get_link` on-chain.
   * @param linkId - The payment link ID
   * @returns A promise resolving to the PaymentLink details
   */
  async getLink(linkId: string): Promise<PaymentLink> {
    return this.getPaymentLinkManager().getLink(linkId);
  }

  /**
   * Verify a batch of payment links, returning only active ones.
   * Maps to `PaymentLinkManager.verify_batch` on-chain.
   * @param linkIds - Array of link IDs to verify
   * @returns A promise resolving to an array of active link IDs
   */
  async verifyBatch(linkIds: string[]): Promise<string[]> {
    return this.getPaymentLinkManager().verifyBatch(linkIds);
  }

  /**
   * Record a view of a payment link (permissionless).
   * Maps to `PaymentLinkManager.record_link_view` on-chain.
   * @param linkId - The payment link ID
   */
  async recordLinkView(linkId: string): Promise<void> {
    return this.getPaymentLinkManager().recordLinkView(linkId);
  }

  /**
   * Retrieve analytics for a payment link.
   * Maps to `PaymentLinkManager.get_link_analytics` on-chain.
   * @param linkId - The payment link ID
   * @returns A promise resolving to the LinkAnalytics
   */
  async getLinkAnalytics(linkId: string): Promise<LinkAnalytics> {
    return this.getPaymentLinkManager().getLinkAnalytics(linkId);
  }

  /**
   * Create a new payment stream. Tokens are pulled from `params.sender` into
   * the contract and streamed to `params.receiver` at `ratePerSecond`.
   * Maps to `PaymentProcessor.create_stream` on-chain.
   */
  async createStream(params: CreateStreamParams): Promise<PaymentStream> {
    const raw = await withMappedContractError(() =>
      (this.contract as any).create_stream({
        sender: params.sender,
        receiver: params.receiver,
        token: params.token,
        rate_per_second: params.ratePerSecond,
        deposit: params.deposit,
        stream_id: params.streamId,
      }),
    );
    return fromContractStream(raw);
  }

  /**
   * Withdraw accrued funds from a stream to `recipient`.
   * Maps to `PaymentProcessor.batch_withdraw_to` on-chain with a single entry.
   * @param recipient - Must be the stream's receiver; must sign.
   * @param streamId - The stream to withdraw from.
   * @param amount - Optional amount cap; defaults to withdrawing everything accrued.
   */
  async withdrawStream(recipient: string, streamId: string, amount?: bigint): Promise<void> {
    return withMappedContractError(() =>
      (this.contract as any).batch_withdraw_to({
        recipient,
        withdrawals: [
          { stream_id: streamId, destination: recipient, amount: amount ?? I128_MAX },
        ],
      }),
    );
  }

  /**
   * Cancel an active stream and refund any un-accrued deposit to the sender.
   * Maps to `PaymentProcessor.cancel_stream` on-chain.
   */
  async cancelStream(sender: string, streamId: string): Promise<void> {
    return withMappedContractError(() =>
      (this.contract as any).cancel_stream({ sender, stream_id: streamId }),
    );
  }

  /**
   * Pause an active stream, freezing accrual until resumed.
   * Maps to `PaymentProcessor.pause_stream` on-chain.
   */
  async pauseStream(sender: string, streamId: string): Promise<void> {
    return withMappedContractError(() =>
      (this.contract as any).pause_stream({ sender, stream_id: streamId }),
    );
  }

  /**
   * Resume a paused stream, restarting accrual from the current timestamp.
   * Maps to `PaymentProcessor.resume_stream` on-chain.
   */
  async resumeStream(sender: string, streamId: string): Promise<void> {
    return withMappedContractError(() =>
      (this.contract as any).resume_stream({ sender, stream_id: streamId }),
    );
  }

  /**
   * Top up an existing stream's deposit.
   * Maps to `PaymentProcessor.top_up_stream` on-chain.
   */
  async topUpStream(sender: string, streamId: string, amount: bigint): Promise<void> {
    return withMappedContractError(() =>
      (this.contract as any).top_up_stream({ caller: sender, stream_id: streamId, amount }),
    );
  }

  /**
   * Get stream details by ID.
   * Maps to `PaymentProcessor.get_stream` on-chain.
   */
  async getStream(streamId: string): Promise<PaymentStream> {
    const raw = await withMappedContractError(() =>
      (this.contract as any).get_stream({ stream_id: streamId }),
    );
    return fromContractStream(raw);
  }

  /**
   * Query streams created by `sender`, paginated (max 100 per page).
   * Maps to `PaymentProcessor.get_sender_streams` on-chain.
   */
  async getSenderStreams(
    sender: string,
    page = 0,
    pageSize = 100,
  ): Promise<PaymentStream[]> {
    const raw: unknown[] = await withMappedContractError(() =>
      (this.contract as any).get_sender_streams({ sender, page, page_size: pageSize }),
    );
    return raw.map((s) => fromContractStream(s as Parameters<typeof fromContractStream>[0]));
  }

  /** Offline/hardware wallet payload builder utilities. */


  offlineSigner(): FluxapayOfflineSigner {
    return new FluxapayOfflineSigner(
      this.contract as import("./offline-signer.js").OfflineCapableClient,
      this.contract.options.contractId,
      this.contract.options.networkPassphrase,
    );
  }
}

export { toFluxapayError, withMappedContractError };

export {
  Merchant,
  PaymentCharge,
  Refund,
  Dispute,
  PaymentStatus,
  RefundStatus,
  DisputeStatus,
  FeeConfig,
  MaybeFeeConfig,
  CreatePaymentArgs,
  FluxapayOfflineSigner,
  OfflineTransactionPayload,
  SubscriptionBillingClient,
  buildOfflinePayload,
  buildCreatePaymentPayload,
  buildVerifyPaymentPayload,
  buildCreateRefundPayload,
  buildSubscriptionTickPayload,
  buildPullAuthorizationPayload,
  prepareForOfflineSigning,
  restoreFromOfflinePayload,
  NetworkProfileSwitcher,
  NetworkEnvironment,
  NetworkProfiles,
  NetworkProfile,
  PaymentStream,
  StreamStatus,
  StreamError,
  CreateStreamParams,
};

export { RefundManagerClient, type RefundManagerConfig } from "./contracts/refund-manager.js";
export { MerchantRegistryClient, type MerchantRegistryConfig } from "./contracts/merchant-registry.js";
export {
  FxOracleClient,
  FxOracleError,
  type FxOracleConfig,
  type RateData,
  FX_ORACLE_ERROR_MAP,
} from "./contracts/fx-oracle.js";
export {
  PaymentLinkManagerClient,
  type PaymentLinkManagerConfig,
  type PaymentLink,
  type LinkAnalytics,
  type CreateLinkParams,
  type CreatePaymentLinkResult,
} from "./contracts/payment-link-manager.js";
export { SEP10Authenticator, type SEP10ChallengeResponse, type SEP10AuthenticatedResponse } from "./sep10.js";
export {
  GasEstimatorClient,
  type GasEstimatorConfig,
  type GasEstimate,
  type GasOperation,
} from "./contracts/gas-estimator.js";



