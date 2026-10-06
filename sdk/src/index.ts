import {
  Client as ContractClient,
  type Merchant,
  type PaymentCharge,
  type Refund,
  type Dispute,
  type PaymentStatus,
  type RefundStatus,
  type DisputeStatus,
  type FeeConfig,
  type MaybeFeeConfig,
  type CreatePaymentArgs,
} from "./contracts/fluxapay/src/index.js";
import { Networks, Keypair } from "@stellar/stellar-sdk";
import {
  FluxapayOfflineSigner,
  type OfflineTransactionPayload,
  type SubscriptionBillingClient,
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
  type NetworkEnvironment,
  NetworkProfiles,
  type NetworkProfile,
  FLUXAPAY_CONTRACT_IDS,
  UNSET_CONTRACT_ID,
} from "./network-profiles.js";

export { FLUXAPAY_CONTRACT_IDS, UNSET_CONTRACT_ID } from "./network-profiles.js";
export type { FluxapayContractIds } from "./network-profiles.js";
export {
  verifyWebhookSignature,
  parseWebhookSignatureHeader,
  type ParsedWebhookSignature,
} from "./webhooks.js";
import { FxOracleClient } from "./contracts/fx-oracle.js";
import {
  MerchantRegistryClient,
  type MerchantRegistryConfig,
  type AddCurrencyPayoutParams,
  type CurrencyPayout,
  type BankAccount,
  type MerchantPage,
} from "./contracts/merchant-registry.js";
export type { MerchantPage };
import {
  PaymentLinkManagerClient,
  type PaymentLinkManagerConfig,
  type PaymentLink,
  type LinkAnalytics,
  type CreateLinkParams,
  type CreatePaymentLinkResult,
  type BatchLinkItem,
} from "./contracts/payment-link-manager.js";
import {
  DexRouterClient,
  type DexRouterConfig,
  type ExecuteSwapParams,
  DexRouterError,
  DEX_ROUTER_ERROR_MAP,
} from "./contracts/dex-router.js";
import { SEP10Authenticator, type SEP10ChallengeResponse, type SEP10AuthenticatedResponse } from "./sep10.js";
import {
  getLocalizedErrorMessage,
  MESSAGES,
  SUPPORTED_LOCALES,
  type SupportedLocale,
} from "./locales/index.js";
import {
  buildPaymentReceipt,
  verifyReceipt as verifyReceiptProof,
  DEFAULT_RECEIPT_BASE_URL,
  type PaymentReceipt,
} from "./receipt.js";
import { FluxapayError, ERROR_CONSTRUCTOR_MAP } from "./errors.js";


export {
  DexRouterClient,
  type DexRouterConfig,
  type ExecuteSwapParams,
  DexRouterError,
  DEX_ROUTER_ERROR_MAP,
};


export { getLocalizedErrorMessage, MESSAGES, SUPPORTED_LOCALES, type SupportedLocale };

export interface FluxapayConfig {
  network: NetworkEnvironment;
  rpcUrl?: string;
  /** Locale for error messages (e.g. 'en', 'fr', 'pt', 'es'). Defaults to 'en'. */
  locale?: string;
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
  /**
   * Issue #840, #839: Base URL of the FluxaPay indexer API used for reconciliation
   * CSV downloads and currency conversions. Falls back to `apiUrl` when omitted.
   */
  indexerUrl?: string;
  /**
   * Issue #816: Stellar secret key (S...) for the FluxaPay platform receipt
   * signing key. Required for `generateReceipt`.
   */
  platformSigningKey?: string;
  /**
   * Issue #816: Platform Ed25519 public key (G...) used by `verifyReceipt`.
   * Defaults to the public key derived from `platformSigningKey` when omitted.
   */
  platformPublicKey?: string;
  /**
   * Issue #816: Base URL for hosted receipt pages.
   * Receipt URLs are `{receiptBaseUrl}/r/{payment_id}`.
   * Default: `https://receipts.fluxapay.io`
   */
  receiptBaseUrl?: string;
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
  /** Optional 24-hour idempotency key for safely retrying payment creation. */
  idempotencyKey?: string;
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
  /** Issue #767: When true, allows customers to fund this payment across multiple transactions. */
  allowPartial?: boolean;
  /**
   * Issue #841: Optional muxed payer as a Stellar M-address string.
   * Decoded to the underlying G-address + 64-bit sub-account ID. When set,
   * `verify_payment` only confirms if the incoming muxed ID matches.
   */
  muxedPayer?: string;
  /**
   * Issue #841: Optional muxed sub-account ID directly (alternative to `muxedPayer`).
   */
  payerMuxedId?: bigint;
  /**
   * Issue #844: When true, checkout may collect an optional tip via
   * `confirmPayment({ tipAmount })`.
   */
  tipEnabled?: boolean;
}

/**
 * Issue #771: A single payment request item within a batch creation transaction.
 */
export interface PaymentRequest {
  paymentId: string;
  amount: bigint;
  currency: string;
  depositAddress: string;
  expiresAt?: bigint;
  durationSecs?: bigint;
  memo?: string;
  memoType?: string;
  tokenAddress?: string;
  clientToken?: string;
  metadata?: Record<string, string>;
  feeWaiverCode?: string;
  payer?: string;
  payerMuxedId?: bigint;
}

/**
 * Issue #771: Parameters for creating a batch of up to 10 payments.
 */
export interface CreatePaymentBatchParams {
  merchantId: string;
  payments: PaymentRequest[];
}

/**
 * Issue #577: Parameters for atomic swap-and-pay through a DEX router.
 */
export interface SwapAndPayParams {
  payer: string;
  merchantId: string;
  paymentId: string;
  dexRouter: string;
  path: string[];
  amountIn: bigint;
  amountOutMin: bigint;
  deadline?: number;
  amount?: bigint;
  currency?: string;
  depositAddress?: string;
  tokenIn?: string;
  expiresAt?: number | bigint;
  fxOracle?: string;
  oraclePair?: string;
  maxDeviationBps?: number;
}

/**
 * Issue #577: Single route specification for multi-route swap routing / aggregation.
 */
export interface SwapRoute {
  router: string;
  path: string[];
  amountIn: bigint;
}

/**
 * Issue #577: Parameters for multi-route swap payments.
 */
export interface SwapAndPayMultiRouteParams extends SwapAndPayParams {
  routes: SwapRoute[];
  minOutputAmount?: bigint;
}

/**
 * Issue #576: Stream top-up entry for batch top-up operations.
 */
export interface StreamTopUp {
  streamId: string;
  amount: bigint;
}

/**
 * Issue #576: Stream withdrawal entry for batch withdrawal operations.
 */
export interface StreamWithdrawal {
  streamId: string;
  destination?: string;
  amount?: bigint;
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
  /** Issue #627: a bulk operation exceeded the per-call cap (50). */
  BatchTooLarge = 15,
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

/** Mirrors the on-chain `SubscriptionPlan` struct in `fluxapay/src/lib.rs`. */
export interface SubscriptionPlan {
  planId: string;
  merchantId: string;
  name: string;
  description: string;
  amount: bigint;
  currency: string;
  intervalSecs: bigint;
  billingInterval: "Daily" | "Weekly" | "Monthly" | "Annually";
  active: boolean;
  /** Issue #836: optional free-trial length in days (max 90). */
  trialDays?: number | null;
}

/** Mirrors the on-chain `Subscription` struct in `fluxapay/src/types.rs`. */
export interface Subscription {
  subscriptionId: string;
  merchantId: string;
  payerAddress: string;
  planId: string;
  amount: bigint;
  currency: string;
  intervalSecs: bigint;
  nextPaymentAt: bigint;
  status: "Active" | "Paused" | "Cancelled" | "Expired";
  createdAt: bigint;
  lastPaymentAt: bigint | null;
  totalPayments: number;
  maxPayments: number | null;
  retryCount: number;
  nextRetryAt: bigint | null;
  resumeAt: bigint | null;
  affiliate: string | null;
  affiliateFeeBps: number | null;
  /** Issue #836: ledger timestamp when free trial ends, if any. */
  trialEndsAt?: bigint | null;
}

export interface CreatePlanParams {
  merchant: string;
  planId: string;
  name: string;
  description: string;
  amount: bigint;
  currency: string;
  billingInterval: SubscriptionPlan["billingInterval"];
  /** Issue #836: optional free-trial length in days (max 90). */
  trialDays?: number;
}

export interface SubscribeParams {
  payer: string;
  planId: string;
  maxPayments?: number;
  affiliate?: string;
  affiliateFeeBps?: number;
}

function fromContractSubscription(raw: {
  subscription_id: string;
  merchant_id: string;
  payer_address: string;
  plan_id: string;
  amount: bigint;
  currency: string;
  interval_secs: bigint;
  next_payment_at: bigint;
  status: Subscription["status"];
  created_at: bigint;
  last_payment_at?: bigint | null;
  total_payments: number;
  max_payments?: number | null;
  retry_count: number;
  next_retry_at?: bigint | null;
  resume_at?: bigint | null;
  affiliate?: string | null;
  affiliate_fee_bps?: number | null;
  trial_ends_at?: bigint | null;
}): Subscription {
  return {
    subscriptionId: raw.subscription_id,
    merchantId: raw.merchant_id,
    payerAddress: raw.payer_address,
    planId: raw.plan_id,
    amount: raw.amount,
    currency: raw.currency,
    intervalSecs: raw.interval_secs,
    nextPaymentAt: raw.next_payment_at,
    status: raw.status,
    createdAt: raw.created_at,
    lastPaymentAt: raw.last_payment_at ?? null,
    totalPayments: raw.total_payments,
    maxPayments: raw.max_payments ?? null,
    retryCount: raw.retry_count,
    nextRetryAt: raw.next_retry_at ?? null,
    resumeAt: raw.resume_at ?? null,
    affiliate: raw.affiliate ?? null,
    affiliateFeeBps: raw.affiliate_fee_bps ?? null,
    trialEndsAt: raw.trial_ends_at ?? null,
  };
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
  7: { message: "ApiKeyNotFound" },
  8: { message: "ApiKeyRevoked" },
} as const;

/**
 * Issue #854: Scoped API key record for a merchant.
 */
export interface ApiKeyRecord {
  key_hash: string;
  merchant: string;
  scopes: string[];
  created_at: bigint;
  revoked: boolean;
}

export interface CreateApiKeyParams {
  merchant: string;
  keyHash: string;
  scopes: string[];
}


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
 * Issue #628: One merchant's entry in `PaymentProcessor.get_top_merchants`,
 * mirroring the `MerchantRanking` contracttype in `fluxapay/src/lib.rs`.
 * `getTopMerchants` returns these already ordered by `totalVolume` descending.
 */
export interface MerchantRanking {
  merchantId: string;
  /** Sum of `amount` over every payment created for this merchant. */
  totalVolume: bigint;
  /** Number of payments created for this merchant. */
  paymentCount: bigint;
}

/**
 * Issue #629: Aggregate analytics for a single merchant over a time window,
 * mirroring the `MerchantAnalytics` contracttype in `fluxapay/src/lib.rs`
 * (`PaymentProcessor.get_merchant_analytics`).
 */
export interface MerchantAnalytics {
  totalPayments: number;
  confirmedPayments: number;
  failedPayments: number;
  totalVolume: bigint;
  averageAmount: bigint;
  disputeCount: number;
  refundCount: number;
  netSettledVolume: bigint;
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
  34: "InvalidSplitSum",
  35: "MissingReceiptHash",
  36: "RefundExpired",
  37: "AlreadyVoted",
  38: "TierVolumeLimitExceeded",
  39: "BatchTooLarge",
  40: "InsufficientArbitrators",
  41: "ArbitrationVotingThresholdNotMet",
  42: "RefundCooldownNotElapsed",
  43: "FeeProposalNotReady",
  44: "NoFeeProposal",
  45: "InvalidEvidenceFormat",
  46: "DisputeRateLimitExceeded",
  47: "InvalidSettlementSignature",
  48: "StaleOracleRate",
  49: "LinkExpired",
  50: "Reentrancy",
  51: "UpgradeFailed",
  52: "InsufficientTreasuryBalance",
  53: "MetadataTooLarge",
  54: "MetadataValueTooLong",
  55: "InvalidMemoType",
  56: "MemoTooLong",
  57: "InvalidMemoId",
  58: "PayerNotWhitelisted",
  59: "LinkMaxUsesReached",
  60: "DirectTransferNotDisputable",
  61: "MaxRetriesExceeded",
  347: "RetryChainTooDeep",
  62: "InvalidStatusTransition",
  63: "RefundNotApproved",
  64: "RouterNotAllowed",
  65: "RouteOutputInsufficient",
  66: "BatchContainsDuplicates",
  67: "InputTooLong",
  68: "TimelockNotExpired",
  69: "InvalidEvidenceCid",
  70: "MuxedAccountMismatch",
  71: "TreasuryMultisigNotConfigured",
  72: "NotAuthorizedTreasurySigner",
  73: "TreasuryProposalNotFound",
  74: "TreasuryProposalAlreadyExecuted",
  75: "TreasuryProposalCancelled",
  76: "TreasuryTimelockNotExpired",
  77: "TreasuryProposalExpired",
  78: "TreasuryAlreadyApproved",
  79: "TreasuryInsufficientApprovals",
  80: "InvalidTreasuryThreshold",
  81: "InsufficientTokenTreasuryBalance",
  82: "TrialActive",
  83: "TrialTooLong",
  84: "InvalidPaymentLink",
  85: "KycLimitExceeded",
  404: "PaymentNotFound",
  405: "RefundNotFound",
  406: "InvalidAmount",
};

export {
  FluxapayError,
  UnauthorizedError,
  PaymentAlreadyExistsError,
  PaymentExpiredError,
  InvalidPaymentIdError,
  RefundAlreadyProcessedError,
  DisputeNotFoundError,
  DisputeAlreadyResolvedError,
  PaymentAlreadyProcessedError,
  AccessControlContractError,
  RefundExceedsPaymentError,
  ContractPausedError,
  RateLimitExceededError,
  RefundCancelledError,
  UnsupportedTokenError,
  AmountBelowMinError,
  AmountAboveMaxError,
  InvalidExpiryError,
  InvalidSettlementError,
  DuplicateIdempotencyKeyError,
  InvalidAddressError,
  ArbitrageDetectedError,
  SwapPathInvalidError,
  OraclePriceDeviationError,
  SubscriptionInGracePeriodError,
  SubscriptionRetryExhaustedError,
  InvalidResumeTimestampError,
  MerchantAuthContractError,
  InvalidSplitSumError,
  MissingReceiptHashError,
  RefundExpiredError,
  AlreadyVotedError,
  TierVolumeLimitExceededError,
  BatchTooLargeContractError,
  InsufficientArbitratorsError,
  ArbitrationVotingThresholdNotMetError,
  RefundCooldownNotElapsedError,
  FeeProposalNotReadyError,
  NoFeeProposalError,
  InvalidEvidenceFormatError,
  DisputeRateLimitExceededError,
  InvalidSettlementSignatureError,
  StaleOracleRateError,
  LinkExpiredError,
  ReentrancyError,
  UpgradeFailedError,
  InsufficientTreasuryBalanceError,
  MetadataTooLargeError,
  MetadataValueTooLongError,
  InvalidMemoTypeError,
  MemoTooLongError,
  InvalidMemoIdError,
  PayerNotWhitelistedError,
  LinkMaxUsesReachedError,
  DirectTransferNotDisputableError,
  MaxRetriesExceededError,
  RetryChainTooDeepError,
  InvalidStatusTransitionError,
  RefundNotApprovedError,
  RouterNotAllowedError,
  RouteOutputInsufficientError,
  BatchContainsDuplicatesError,
  InputTooLongError,
  TimelockNotExpiredError,
  InvalidEvidenceCidError,
  MuxedAccountMismatchError,
  TreasuryMultisigNotConfiguredError,
  NotAuthorizedTreasurySignerError,
  TreasuryProposalNotFoundError,
  TreasuryProposalAlreadyExecutedError,
  TreasuryProposalCancelledError,
  TreasuryTimelockNotExpiredError,
  TreasuryProposalExpiredError,
  TreasuryAlreadyApprovedError,
  TreasuryInsufficientApprovalsError,
  InvalidTreasuryThresholdError,
  InsufficientTokenTreasuryBalanceError,
  TrialActiveError,
  TrialTooLongError,
  InvalidPaymentLinkError,
  KycLimitExceededError,
  PaymentNotFoundError,
  RefundNotFoundError,
  InvalidAmountError,
  ERROR_CONSTRUCTOR_MAP,
} from "./errors.js";

/**
 * Issue #814: raised client-side when a batch exceeds `MAX_BATCH_STATUS_IDS`.
 *
 * Thrown before any network call rather than letting the RPC reject it, so the
 * caller gets an actionable message instead of a simulation failure — and so
 * the cap is enforced even against an RPC that would have accepted more.
 */
export class BatchTooLargeError extends Error {
  constructor(
    readonly requested: number,
    readonly limit: number,
  ) {
    super(`Batch of ${requested} exceeds the maximum of ${limit} payment IDs`);
    this.name = "BatchTooLargeError";
  }
}

/** Issue #814: maximum payment IDs accepted by `getPaymentStatuses`. */
export const MAX_BATCH_STATUS_IDS = 50;

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

export function toFluxapayError(error: unknown, locale = "en"): FluxapayError {
  const code = parseContractErrorCode(error);
  if (code === null) {
    if (error instanceof Error) {
      throw error;
    }
    throw new Error("Unknown Fluxapay SDK error");
  }

  const Constructor = ERROR_CONSTRUCTOR_MAP[code];
  const contractErrorName = FLUXAPAY_CONTRACT_ERROR_MAP[code] ?? "UnknownContractError";
  const message = `${contractErrorName} (contract error #${code})`;
  if (Constructor) {
    return new Constructor(message, error, locale);
  }
  return new FluxapayError(
    code,
    contractErrorName,
    message,
    error,
    locale,
  );
}

/**
 * Issue #814: a payment's status as the contract reports it.
 *
 * Deliberately loose. The contract's status enum is generated per-binding and
 * the shape differs between a `Result`-wrapped simulation and a direct read, so
 * pinning it here would break on the next binding regeneration.
 */
export type PaymentStatusValue = string | { tag: string; values?: unknown };

/** Pulls the status field out of whatever shape `get_payment` returned. */
function extractPaymentStatus(payment: unknown): PaymentStatusValue | null {
  if (payment === null || payment === undefined) return null;

  // Simulation results arrive wrapped; unwrap one level if present.
  const unwrapped =
    typeof payment === "object" && payment !== null && "result" in payment
      ? (payment as { result: unknown }).result
      : payment;

  if (typeof unwrapped !== "object" || unwrapped === null) return null;

  const status = (unwrapped as { status?: unknown }).status;
  if (status === undefined || status === null) return null;

  return status as PaymentStatusValue;
}

/** True when an error means "no such payment" rather than a transport failure. */
function isPaymentNotFound(error: unknown): boolean {
  if (error instanceof FluxapayError) {
    return error.contractErrorName === "PaymentNotFound";
  }
  // Some bindings surface a missing entry as a plain message before the code
  // mapper sees it.
  const message = error instanceof Error ? error.message : String(error);
  return /PaymentNotFound|payment not found|#404\b/i.test(message);
}

let defaultClientLocale = "en";

export function setDefaultLocale(locale: string): void {
  defaultClientLocale = locale;
}

export async function withMappedContractError<T>(
  operation: () => Promise<T>,
  locale?: string,
): Promise<T> {
  try {
    return await operation();
  } catch (error) {
    throw toFluxapayError(error, locale ?? defaultClientLocale);
  }
}

function toCreatePaymentArgs(params: CreatePaymentParams): CreatePaymentArgs {
  let payerMuxedId: bigint | undefined = params.payerMuxedId;
  if (params.muxedPayer) {
    const decoded = decodeMuxedAddress(params.muxedPayer);
    payerMuxedId = decoded.id;
  }
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
    idempotency_key: params.idempotencyKey,
    metadata_hash: undefined,
    metadata: params.metadata,
    fee_waiver_code: params.feeWaiverCode,
    allow_partial: params.allowPartial,
    payer_muxed_id: payerMuxedId,
    tip_enabled: params.tipEnabled ?? false,
  };
}

function toSwapAndPayArgs(params: SwapAndPayParams) {
  const tokenIn = params.tokenIn ?? (params.path && params.path.length > 0 ? params.path[0] : "");
  const amount = params.amount ?? params.amountOutMin;
  const depositAddress = params.depositAddress ?? params.merchantId;
  const expiresAt =
    params.expiresAt !== undefined
      ? BigInt(params.expiresAt)
      : params.deadline !== undefined
        ? BigInt(params.deadline)
        : undefined;
  return {
    payer: params.payer,
    payment_id: params.paymentId,
    merchant_id: params.merchantId,
    amount,
    currency: params.currency ?? "USDC",
    deposit_address: depositAddress,
    token_in: tokenIn,
    amount_in: params.amountIn,
    amount_out_min: params.amountOutMin,
    path: params.path,
    expires_at: expiresAt,
    dex_router: params.dexRouter,
    fx_oracle: params.fxOracle,
    oracle_pair: params.oraclePair,
    max_deviation_bps: params.maxDeviationBps ?? 0,
  };
}

/**
 * Issue #841: Encode a G-address + sub-account ID into a Stellar M-address.
 */
export function encodeMuxedAddress(gAddress: string, id: bigint | number | string): string {
  const { StrKey } = require("@stellar/stellar-sdk") as typeof import("@stellar/stellar-sdk");
  if (!gAddress.startsWith("G")) {
    throw new Error(`encodeMuxedAddress expects a G-address, got: ${gAddress}`);
  }
  const pubkey: Buffer = StrKey.decodeEd25519PublicKey(gAddress);
  const idBuf = Buffer.alloc(8);
  idBuf.writeBigUInt64BE(BigInt(id));
  // Muxed account binary: 0x00 discriminant? Stellar uses 0x60 for muxed med25519 in StrKey.
  // Prefer SDK encodeMuxedAccount when available.
  if (typeof (StrKey as any).encodeMuxedAccount === "function") {
    const payload = Buffer.alloc(40);
    idBuf.copy(payload, 0);
    pubkey.copy(payload, 8);
    return (StrKey as any).encodeMuxedAccount(payload);
  }
  // Fallback via MuxedAccount class
  const { MuxedAccount, Keypair } = require("@stellar/stellar-sdk");
  const kp = Keypair.fromPublicKey(gAddress);
  const account = { accountId: () => kp.publicKey() };
  const muxed = new MuxedAccount(account, String(id));
  return muxed.accountId();
}

/**
 * Issue #841: Decode a Stellar M-address into its G-address and 64-bit sub-account ID.
 */
export function decodeMuxedAddress(mAddress: string): { gAddress: string; id: bigint } {
  if (!mAddress) {
    throw new Error("Invalid muxed address: empty");
  }
  if (mAddress.startsWith("G")) {
    return { gAddress: mAddress, id: 0n };
  }
  if (!mAddress.startsWith("M")) {
    throw new Error(`Invalid muxed address: ${mAddress}`);
  }
  const { StrKey } = require("@stellar/stellar-sdk") as typeof import("@stellar/stellar-sdk");
  if (typeof (StrKey as any).decodeMuxedAccount === "function") {
    const decoded: Buffer = (StrKey as any).decodeMuxedAccount(mAddress);
    // decoded is 40 bytes: 8-byte id + 32-byte ed25519 pubkey
    const id = decoded.readBigUInt64BE(0);
    const gAddress = StrKey.encodeEd25519PublicKey(decoded.subarray(8, 40));
    return { gAddress, id };
  }
  const { MuxedAccount } = require("@stellar/stellar-sdk");
  const muxed = MuxedAccount.fromAddress(mAddress, "0");
  return {
    gAddress: muxed.baseAccount().accountId(),
    id: BigInt(muxed.id),
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
  public readonly locale: string;
  private fxOracleClient?: FxOracleClient;
  private merchantRegistryClient?: MerchantRegistryClient;
  private paymentLinkManagerClient?: PaymentLinkManagerClient;
  private sep10Authenticator?: SEP10Authenticator;
  private readonly config: FluxapayConfig;

  constructor(config: FluxapayConfig) {
    this.config = config;
    this.locale = config.locale ?? "en";
    setDefaultLocale(this.locale);
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async createPayment(params: CreatePaymentParams) {
    return withMappedContractError(() =>
      this.contract.create_payment(toCreatePaymentArgs(params)),
    );
  }

  /**
   * Issue #771: Create up to 10 payment charges atomically in a single transaction.
   * Wraps the `create_payment_batch` contract entry point.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async createPaymentBatch(params: CreatePaymentBatchParams): Promise<string[]> {
    if (params.payments.length > 10) {
      throw new FluxapayError(39, "BatchTooLarge", "createPaymentBatch accepts a maximum of 10 items per batch");
    }
    return withMappedContractError(async () => {
      const result = await (this.contract as any).create_payment_batch({
        merchant_id: params.merchantId,
        payments: params.payments.map((p) => ({
          payment_id: p.paymentId,
          amount: p.amount,
          currency: p.currency,
          deposit_address: p.depositAddress,
          expires_at: p.expiresAt,
          duration_secs: p.durationSecs,
          memo: p.memo,
          memo_type: p.memoType,
          token_address: p.tokenAddress,
          client_token: p.clientToken,
          metadata_hash: undefined,
          metadata: p.metadata,
          fee_waiver_code: p.feeWaiverCode,
          payer: p.payer,
          payer_muxed_id: p.payerMuxedId,
        })),
      });
      return (result as any)?.result ?? result;
    });
  }

  /**
   * Issue #577: Swap tokens on a DEX router and pay a merchant in one atomic transaction.
   * Maps to `PaymentProcessor.swap_and_pay` on-chain.
   *
   * @throws {ArbitrageDetectedError} If DEX output is worse than the quoted path return.
   * @throws {SwapPathInvalidError} If swap path is empty or output token does not match.
   * @throws {OraclePriceDeviationError} If swap rate deviates from oracle beyond tolerance.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async swapAndPay(params: SwapAndPayParams): Promise<PaymentCharge> {
    return withMappedContractError(async () => {
      const args = toSwapAndPayArgs(params);
      const fn = (this.contract as any).swap_and_pay;
      if (typeof fn === "function") {
        try {
          const res = await fn.call(this.contract, { args });
          return (res as any)?.result ?? res;
        } catch (e) {
          const res = await fn.call(this.contract, args);
          return (res as any)?.result ?? res;
        }
      }
      throw new FluxapayError(28, "SwapPathInvalid", "swap_and_pay is not supported on this contract instance");
    });
  }

  /**
   * Issue #577: Split a token swap across multiple DEX routes and pay a merchant atomically.
   * Maps to `PaymentProcessor.swap_and_pay_multi_route` on-chain.
   *
   * @throws {ArbitrageDetectedError} If DEX output is worse than quoted path returns.
   * @throws {SwapPathInvalidError} If any route swap path is invalid.
   * @throws {OraclePriceDeviationError} If swap rate deviates from oracle beyond tolerance.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async swapAndPayMultiRoute(params: SwapAndPayMultiRouteParams): Promise<PaymentCharge> {
    return withMappedContractError(async () => {
      const args = toSwapAndPayArgs(params);
      const routes = params.routes.map((r) => ({
        router: r.router,
        path: r.path,
        amount_in: r.amountIn,
      }));
      const minOutputAmount = params.minOutputAmount ?? params.amountOutMin ?? params.amount ?? 0n;
      const fn = (this.contract as any).swap_and_pay_multi_route;
      if (typeof fn === "function") {
        try {
          const res = await fn.call(this.contract, {
            args,
            routes,
            min_output_amount: minOutputAmount,
          });
          return (res as any)?.result ?? res;
        } catch (e) {
          const res = await fn.call(this.contract, args, routes, minOutputAmount);
          return (res as any)?.result ?? res;
        }
      }
      throw new FluxapayError(28, "SwapPathInvalid", "swap_and_pay_multi_route is not supported on this contract instance");
    });
  }

  /**
   * Issue #763: Permissionlessly expire a pending payment whose TTL has elapsed.
   * Returns PaymentExpired error if the payment has not yet expired.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async expirePayment(paymentId: string): Promise<void> {
    return withMappedContractError(async () => {
      await (this.contract as any).expire_payment({ payment_id: paymentId });
    });
  }

  /**
   * Issue #856: Executes a token swap via DexRouter with slippage tolerance and max_slippage_bps guards.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async executeSwap(
    params: {
      caller: string;
      tokenIn: string;
      tokenOut: string;
      amountIn: bigint;
      minAmountOut: bigint;
      maxSlippageBps: number;
      dexRouterContractId?: string;
    },
    signerKeypair?: any,
  ): Promise<bigint> {
    if (params.maxSlippageBps > 5000) {
      throw new FluxapayError(4, "SlippageExceeded", "maxSlippageBps cannot exceed 5000 (50%)");
    }
    const routerContractId = params.dexRouterContractId || this.config.contractId || UNSET_CONTRACT_ID;
    const routerClient = new DexRouterClient({
      network: this.config.network,
      rpcUrl: this.config.rpcUrl,
      contractId: routerContractId,
    });
    return routerClient.executeSwap(
      {
        caller: params.caller,
        tokenIn: params.tokenIn,
        tokenOut: params.tokenOut,
        amountIn: params.amountIn,
        minAmountOut: params.minAmountOut,
        maxSlippageBps: params.maxSlippageBps,
      },
      signerKeypair,
    );
  }


  /**
   * Verify a payment via oracle
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * verifyPaymentBatch
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async verifyPaymentBatch(params: {
    operator: string;
    verifications: Array<{
      paymentId: string;
      transactionHash: Buffer;
      payerAddress: string;
      amountReceived: bigint;
      payerMuxedId?: bigint;
    }>;
  }) {
    return withMappedContractError(() =>
      (this.contract as any).verify_payment_batch({
        operator: params.operator,
        verifications: params.verifications.map((verification) => ({
          payment_id: verification.paymentId,
          transaction_hash: verification.transactionHash,
          payer_address: verification.payerAddress,
          amount_received: verification.amountReceived,
          payer_muxed_id: verification.payerMuxedId,
        })),
      }),
    );
  }

  /**
   * Issue #844: Confirm a payment (checkout flow), optionally with a tip.
   * `tipAmount` is only accepted when the payment was created with
   * `tipEnabled: true`. Tip is stored separately from base `amount`.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async confirmPayment(params: {
    oracle: string;
    paymentId: string;
    transactionHash: Buffer;
    payerAddress: string;
    amountReceived: bigint;
    tipAmount?: bigint;
    payerMuxedId?: bigint;
  }) {
    return withMappedContractError(() =>
      (this.contract as any).confirm_payment({
        oracle: params.oracle,
        args: {
          payment_id: params.paymentId,
          transaction_hash: params.transactionHash,
          payer_address: params.payerAddress,
          amount_received: params.amountReceived,
          tip_amount: params.tipAmount,
          payer_muxed_id: params.payerMuxedId,
        },
      }),
    );
  }

  /**
   * Register a new merchant in the MerchantRegistry contract
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * Issue #630 / #529: Set a merchant-specific payment tolerance (smallest
   * currency unit). Pass `null` to clear the override and use the global
   * default. Signed by the **merchant** (`merchant_id.require_auth()` on-chain);
   * the contract caps the effective tolerance at 1% of each payment amount.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async setMerchantPaymentTolerance(
    merchantId: string,
    tolerance: bigint | null,
  ): Promise<void> {
    return this.getMerchantRegistry().setMerchantPaymentTolerance(merchantId, tolerance);
  }

  /**
   * Issue #630 / #529: Read a merchant's effective payment tolerance — the
   * merchant-specific override when set, otherwise the global default.
   * Read-only.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getMerchantPaymentTolerance(merchantId: string): Promise<bigint> {
    return this.getMerchantRegistry().getMerchantPaymentTolerance(merchantId);
  }

  /**
   * Issue #630 / #529: Set the global default payment tolerance. Signed by the
   * MerchantRegistry **admin**; an unauthorized caller surfaces as a mapped
   * `Unauthorized` `FluxapayError`, and negative values are rejected.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async setGlobalPaymentTolerance(admin: string, tolerance: bigint): Promise<void> {
    return this.getMerchantRegistry().setGlobalPaymentTolerance(admin, tolerance);
  }

  /**
   * Issue #630 / #529: Read the global default payment tolerance. Read-only.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getGlobalPaymentTolerance(): Promise<bigint> {
    return this.getMerchantRegistry().getGlobalPaymentTolerance();
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getRefundPolicy() {
    return withMappedContractError(() => this.contract.get_refund_policy());
  }

  /**
   * Get refund details by ID
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * Issue #854: Merchant creates a scoped API key.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async createApiKey(params: CreateApiKeyParams): Promise<ApiKeyRecord> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).create_api_key({
        merchant: params.merchant,
        key_hash: params.keyHash,
        scopes: params.scopes,
      });
      return tx?.result ?? tx;
    });
  }

  /**
   * Issue #854: Retrieve an API key record by its key hash.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getApiKey(keyHash: string): Promise<ApiKeyRecord> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).get_api_key({
        key_hash: keyHash,
      });
      return tx?.result ?? tx;
    });
  }

  /**
   * Issue #854: Merchant revokes an active API key.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async revokeApiKey(merchant: string, keyHash: string): Promise<void> {
    return withMappedContractError(async () => {
      const tx = await (this.contract as any).revoke_api_key({
        merchant,
        key_hash: keyHash,
      });
      return tx?.result ?? tx;
    });
  }

  /**
   * Get all refunds for a payment

   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * Open a dispute with on-chain SHA-256 evidence hash verification (Issue #773).
   * Accepts an optional evidenceHash (hex string, auto-decoded to 32 bytes).
   */
  async openDispute(params: {
    paymentId: string | number;
    amount: bigint | number;
    reason?: string;
    evidence?: string;
    disputer?: string;
    opener?: string;
    evidenceHash?: string | Buffer | Uint8Array;
    bondAmount?: bigint | number;
  }) {
    let hashBuf: Buffer;
    if (params.evidenceHash) {
      if (typeof params.evidenceHash === "string") {
        hashBuf = Buffer.from(params.evidenceHash.replace(/^0x/, ""), "hex");
      } else {
        hashBuf = Buffer.from(params.evidenceHash);
      }
    } else {
      const { createHash } = await import("node:crypto");
      hashBuf = createHash("sha256").update(params.evidence ?? "").digest();
    }

    if (hashBuf.length !== 32) {
      throw new Error(`evidenceHash must be 32 bytes (got ${hashBuf.length})`);
    }

    const caller = params.disputer || params.opener || "";
    return withMappedContractError(async () => {
      if (typeof (this.contract as any).open_dispute === "function") {
        return (this.contract as any).open_dispute({
          opener: caller,
          payment_id: typeof params.paymentId === "number" ? BigInt(params.paymentId) : params.paymentId,
          disputed_amount: BigInt(params.amount),
          bond_amount: BigInt(params.bondAmount ?? 0),
          evidence_hash: hashBuf,
        });
      }
      return this.contract.create_dispute({
        payment_id: String(params.paymentId),
        amount: BigInt(params.amount),
        reason: params.reason ?? "",
        evidence: params.evidence ?? "",
        disputer: caller,
        evidence_hash: hashBuf,
      });
    });
  }

  /**
   * Read-only view function to retrieve the stored SHA-256 evidence hash for a dispute (Issue #773).
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async verifyEvidence(disputeId: string | number): Promise<string> {
    return withMappedContractError(async () => {
      const res = await (this.contract as any).verify_evidence({
        dispute_id: typeof disputeId === "number" ? BigInt(disputeId) : disputeId,
      });
      const val = (res as { result?: any }).result ?? res;
      return Buffer.from(val).toString("hex");
    });
  }

  /**
   * Move a dispute to under-review status
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getDispute(disputeId: string) {
    return withMappedContractError(() =>
      this.contract.get_dispute({
        dispute_id: disputeId,
      }),
    );
  }

  /**
   * Issue #575: Get all disputes for a payment, optionally filtered by status.
   * @param paymentId - The payment ID to query disputes for.
   * @param status - Optional dispute status filter.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getPaymentDisputes(paymentId: string, status?: DisputeStatus) {
    if (status !== undefined) {
      return this.getPaymentDisputesByStatus(paymentId, status);
    }
    return withMappedContractError(() =>
      this.contract.get_payment_disputes({
        payment_id: paymentId,
      }),
    );
  }

  /**
   * Issue #575: Get disputes for a payment filtered by dispute status.
   * Maps to `PaymentProcessor.get_payment_disputes_by_status` on-chain.
   *
   * @param paymentId - The payment ID to query disputes for.
   * @param status - The dispute status to filter by.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getPaymentDisputesByStatus(paymentId: string, status: DisputeStatus) {
    return withMappedContractError(async () => {
      const fn = (this.contract as any).get_payment_disputes_by_status;
      if (typeof fn === "function") {
        const res = await fn.call(this.contract, {
          payment_id: paymentId,
          status,
        });
        return (res as any)?.result ?? res;
      }
      const all = await this.contract.get_payment_disputes({ payment_id: paymentId });
      const list = (all as any)?.result ?? all;
      return Array.isArray(list) ? list.filter((d: any) => d.status === status) : list;
    });
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getPayment(paymentId: string) {
    return withMappedContractError(() =>
      this.contract.get_payment({ payment_id: paymentId }),
    );
  }

  /**
   * Issue #816: Produce a signed, shareable payment receipt for a confirmed
   * (or settled) payment. The `proof` field is an Ed25519 signature over the
   * canonical receipt fields, verifiable offline with {@link verifyReceipt}.
   *
   * Receipt URL format: `{receiptBaseUrl}/r/{payment_id}`
   * (default base `https://receipts.fluxapay.io`).
   *
   * Requires `platformSigningKey` in {@link FluxapayConfig}.
   * @throws {Error} If platformSigningKey is not configured.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async generateReceipt(paymentId: string): Promise<PaymentReceipt> {
    if (!this.config.platformSigningKey) {
      throw new Error(
        "platformSigningKey is required in FluxapayConfig to generate receipts.",
      );
    }

    const raw = await this.getPayment(paymentId);
    const payment = ((raw as { result?: unknown }).result ?? raw) as {
      payment_id?: string;
      amount?: bigint | number | string;
      currency?: string;
      merchant_id?: string;
      confirmed_at?: bigint | number | string | null;
      status?: { tag?: string } | string;
      transaction_hash?: Buffer | Uint8Array | string | null;
    };

    const statusTag =
      typeof payment.status === "string"
        ? payment.status
        : payment.status?.tag;
    if (statusTag && statusTag !== "Confirmed" && statusTag !== "Settled") {
      throw new Error(
        `Payment ${paymentId} is not confirmed (status=${statusTag ?? "unknown"})`,
      );
    }

    let merchantName = payment.merchant_id ?? "Unknown merchant";
    try {
      if (payment.merchant_id) {
        const merchantRaw = await this.getMerchant(payment.merchant_id);
        const merchant = ((merchantRaw as { result?: unknown }).result ?? merchantRaw) as {
          business_name?: string;
          businessName?: string;
        };
        merchantName =
          merchant.business_name ?? merchant.businessName ?? merchantName;
      }
    } catch {
      // Fall back to merchant_id when registry lookup is unavailable.
    }

    const confirmedRaw = payment.confirmed_at;
    const confirmedAt =
      confirmedRaw === null || confirmedRaw === undefined
        ? new Date()
        : typeof confirmedRaw === "bigint"
          ? Number(confirmedRaw)
          : confirmedRaw;

    let txHash = "";
    const th = payment.transaction_hash;
    if (th == null) {
      throw new Error(`Payment ${paymentId} has no transaction_hash`);
    } else if (typeof th === "string") {
      txHash = th.startsWith("0x") ? th.slice(2) : th;
    } else {
      txHash = Buffer.from(th).toString("hex");
    }

    return buildPaymentReceipt({
      paymentId: payment.payment_id ?? paymentId,
      amountStroops: payment.amount ?? 0n,
      currency: payment.currency ?? "USDC",
      merchantName,
      confirmedAt,
      txHash,
      platformSecretKey: this.config.platformSigningKey,
      receiptBaseUrl: this.config.receiptBaseUrl ?? DEFAULT_RECEIPT_BASE_URL,
    });
  }

  /**
   * Issue #816: Pure offline verification of a receipt `proof` against the
   * FluxaPay platform public key. No network calls.
   *
   * Uses `platformPublicKey` from config, or derives it from
   * `platformSigningKey` when only the secret is configured.
   * @throws {Error} If platformPublicKey or platformSigningKey is not configured.
   */
  verifyReceipt(receipt: PaymentReceipt): boolean {
    const publicKey =
      this.config.platformPublicKey ??
      (this.config.platformSigningKey
        ? Keypair.fromSecret(this.config.platformSigningKey).publicKey()
        : undefined);
    if (!publicKey) {
      throw new Error(
        "platformPublicKey (or platformSigningKey) is required to verify receipts.",
      );
    }
    return verifyReceiptProof(receipt, publicKey);
  }

  /**
   * Fetch payment status transition history for a given payment.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getPaymentStatusHistory(paymentId: string) {
    return withMappedContractError(() =>
      (this.contract as any).get_payment_status_history({ payment_id: paymentId }),
    );
  }

  /**
   * Issue #814: read the status of many payments in one round trip.
   *
   * Merchants reconciling orders were calling `getPayment` in a loop, which is
   * N sequential RPC calls — latency scales linearly with the order book.
   *
   * @param paymentIds up to {@link MAX_BATCH_STATUS_IDS} IDs
   * @returns a Map from payment ID to status, with `null` for IDs that do not
   *          exist. A missing payment is an ordinary result here, not an error:
   *          a merchant checking 50 orders should not lose the other 49 because
   *          one ID was mistyped.
   * @throws {BatchTooLargeError} if more than the limit is requested
   *
   * # Why the reads are issued concurrently rather than as one contract call
   *
   * A true single-invocation batch needs an on-chain view that takes a vector
   * of IDs and returns a vector of statuses. `get_payment` takes one ID, so
   * batching on-chain would mean a contract change and a redeploy. Issuing the
   * reads concurrently against the same RPC gets the latency win — one round
   * trip's worth of wall time instead of N — without touching the contract.
   *
   * If a `get_payment_summary`-style vector view lands later, this method's
   * signature does not change; only its body does.
   */
  async getPaymentStatuses(
    paymentIds: string[],
  ): Promise<Map<string, PaymentStatusValue | null>> {
    if (paymentIds.length > MAX_BATCH_STATUS_IDS) {
      throw new BatchTooLargeError(paymentIds.length, MAX_BATCH_STATUS_IDS);
    }

    const results = new Map<string, PaymentStatusValue | null>();
    if (paymentIds.length === 0) {
      return results;
    }

    // Duplicates are collapsed so a caller passing the same ID twice does not
    // pay for it twice; the returned Map is keyed by ID either way.
    const unique = [...new Set(paymentIds)];

    const settled = await Promise.all(
      unique.map(async (id) => {
        try {
          const payment = await this.getPayment(id);
          return { id, status: extractPaymentStatus(payment) };
        } catch (error) {
          // A not-found payment is reported as null. Anything else is a real
          // failure and is rethrown, because silently mapping an RPC outage to
          // "these 50 orders do not exist" would be far worse than an error.
          if (isPaymentNotFound(error)) {
            return { id, status: null };
          }
          throw error;
        }
      }),
    );

    for (const { id, status } of settled) {
      results.set(id, status);
    }

    return results;
  }

  /**
   * generateReconciliationReportPaginated
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async generateReconciliationReportPaginated(params: {
    merchantId: string;
    fromTs: bigint;
    toTs: bigint;
    offset: number;
    limit: number;
  }) {
    return withMappedContractError(() =>
      (this.contract as any).generate_reconciliation_page({
        merchant_id: params.merchantId,
        from_ts: params.fromTs,
        to_ts: params.toTs,
        offset: params.offset,
        limit: params.limit,
      }),
    );
  }

  /**
   * getAllReconciliationPages
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getAllReconciliationPages(params: {
    merchantId: string;
    fromTs: bigint;
    toTs: bigint;
    pageSize?: number;
  }) {
    const items: unknown[] = [];
    let offset = 0;
    const limit = params.pageSize ?? 100;
    for (;;) {
      const page = await this.generateReconciliationReportPaginated({
        ...params,
        offset,
        limit,
      }) as any;
      items.push(...page.items);
      if (!page.has_more) return { ...page, items };
      offset += limit;
    }
  }

  /**
   * Issue #489: Get payment by metadata_hash for order reconciliation.
   * Performs reverse lookup using the merchant-supplied metadata hash.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getPaymentByMetadataHash(metadataHash: Buffer) {
    return withMappedContractError(() =>
      this.contract.get_payment_by_metadata_hash({ metadata_hash: metadataHash }),
    );
  }

  /**
   * Issue #492: Get customer profile for a merchant.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getCustomer(merchantId: string, customerId: string) {
    return withMappedContractError(() =>
      this.contract.get_customer({ merchant_id: merchantId, customer_id: customerId }),
    );
  }

  /**
   * Issue #492: Get top customers for a merchant sorted by total spending.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getTopCustomers(merchantId: string, limit: number) {
    return withMappedContractError(() =>
      this.contract.get_top_customers({ merchant_id: merchantId, limit }),
    );
  }

  /**
   * Issue #628: Rank merchants by cumulative gross payment volume across the
   * whole platform, for operator-level analytics. Reads the on-chain
   * volume/count indexes only — it never scans individual payments.
   *
   * `limit` is capped at 100 by the contract; a `limit` of 0 returns up to the
   * cap. Results are already ordered by `totalVolume` descending.
   *
   * Read-only — no authorization required.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getTopMerchants(limit: number): Promise<MerchantRanking[]> {
    const result = await withMappedContractError(() =>
      this.contract.get_top_merchants({ limit }),
    );
    const rows = (result as { result?: unknown }).result ?? result;
    return (rows as Array<{ merchant_id: string; total_volume: bigint; payment_count: bigint }>).map(
      (row) => ({
        merchantId: row.merchant_id,
        totalVolume: row.total_volume,
        paymentCount: row.payment_count,
      }),
    );
  }

  /**
   * Issue #629: O(1) count of payments created for a merchant, backed by the
   * `MerchantPaymentCount` index. Pairs with `getMerchantPaymentsFull` for
   * dashboard pagination.
   *
   * Read-only — no authorization required.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getMerchantPaymentCount(merchantId: string): Promise<number> {
    const result = await withMappedContractError(() =>
      this.contract.get_merchant_payment_count({ merchant_id: merchantId }),
    );
    const value = (result as { result?: unknown }).result ?? result;
    return Number(value as number | bigint);
  }

  /**
   * Issue #629: Aggregate analytics for a merchant over `[fromTimestamp,
   * toTimestamp]` (inclusive, ledger seconds): volume, payment counts,
   * average amount, dispute and refund counts, and net settled volume.
   *
   * Pass `fromTimestamp = 0` and omit `toTimestamp` (or pass `0` /
   * `Number.MAX_SAFE_INTEGER`) for all-time analytics — the client sends the
   * contract's `u64::MAX` sentinel, which makes it scan the merchant index
   * directly instead of walking an unbounded range of day buckets.
   *
   * Read-only — no authorization required.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getMerchantAnalytics(
    merchantId: string,
    fromTimestamp: number,
    toTimestamp?: number,
  ): Promise<MerchantAnalytics> {
    const U64_MAX = 18446744073709551615n;
    // A missing / zero / not-finite / above-safe-integer upper bound means
    // "all time": send the exact u64::MAX the contract special-cases, never a
    // large finite value (that would make it iterate ~to_ts/86400 day buckets).
    const toTs =
      toTimestamp === undefined ||
      toTimestamp <= 0 ||
      !Number.isFinite(toTimestamp) ||
      toTimestamp >= Number.MAX_SAFE_INTEGER
        ? U64_MAX
        : BigInt(Math.floor(toTimestamp));
    const result = await withMappedContractError(() =>
      this.contract.get_merchant_analytics({
        merchant_id: merchantId,
        from_ts: BigInt(Math.max(0, Math.floor(fromTimestamp))),
        to_ts: toTs,
      }),
    );
    const a = ((result as { result?: unknown }).result ?? result) as {
      total_payments: number;
      confirmed_payments: number;
      failed_payments: number;
      total_volume: bigint;
      avg_payment_amount: bigint;
      dispute_count: number;
      refund_count: number;
      net_settled_volume: bigint;
    };
    return {
      totalPayments: Number(a.total_payments),
      confirmedPayments: Number(a.confirmed_payments),
      failedPayments: Number(a.failed_payments),
      totalVolume: a.total_volume,
      averageAmount: a.avg_payment_amount,
      disputeCount: Number(a.dispute_count),
      refundCount: Number(a.refund_count),
      netSettledVolume: a.net_settled_volume,
    };
  }

  /**
   * Issue #488: Public TTL bump for a single payment (permissionless).
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async bumpPaymentTTL(paymentId: string) {
    return withMappedContractError(() =>
      this.contract.bump_payment_ttl_public({ payment_id: paymentId }),
    );
  }

  /**
   * Issue #488: Bulk bump TTLs for payment maintenance (max 50 per call).
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
  }

  /**
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

  private getIndexerUrl(): string {
    const url = this.config.indexerUrl || this.config.apiUrl;
    if (!url) {
      throw new Error(
        "indexerUrl (or apiUrl) is required in FluxapayConfig to download reports.",
      );
    }
    return url.replace(/\/$/, "");
  }

  /**
   * Issue #840: Download a merchant reconciliation CSV for `[from, to]` and
   * write it to a Node.js writable stream (e.g. `fs.createWriteStream`).
   *
   * Requires a SEP-10 JWT whose `sub` is the merchant account.
   */
  async downloadReconciliationReport(params: {
    from: string;
    to: string;
    token: string;
    /** Node.js Writable stream (or any object with `write`/`end`). */
    dest: { write: (chunk: string | Buffer) => boolean; end: () => void; once?: (event: string, cb: () => void) => void };
  }): Promise<void> {
    const url =
      `${this.getIndexerUrl()}/v1/reports/reconciliation` +
      `?from=${encodeURIComponent(params.from)}&to=${encodeURIComponent(params.to)}`;

    const res = await fetch(url, {
      headers: {
        Accept: "text/csv",
        Authorization: `Bearer ${params.token}`,
      },
    });

    if (!res.ok) {
      const body = await res.text().catch(() => "");
      throw new Error(
        `Failed to download reconciliation report: ${res.status} ${body}`,
      );
    }

    // Prefer streaming the body when available (Node undici / modern fetch).
    const body = res.body as any;
    if (body && typeof body.getReader === "function") {
      const reader = body.getReader();
      const decoder = new TextDecoder();
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        const chunk = typeof value === "string" ? value : decoder.decode(value, { stream: true });
        if (!params.dest.write(chunk) && params.dest.once) {
          await new Promise<void>((resolve) => params.dest.once!("drain", resolve));
        }
      }
      params.dest.end();
      return;
    }

    const text = await res.text();
    params.dest.write(text);
    params.dest.end();
  }

  /**
   * Issue #839: Preview a USDC→fiat conversion at the indexer's cached
   * oracle rate without creating a payment.
   *
   * @param params.from - Source currency (typically "USDC")
   * @param params.to - Destination fiat currency (e.g. "NGN")
   * @param params.amount - Amount in stroops (7 decimal places)
   */
  async convertCurrency(params: {
    from: string;
    to: string;
    amount: bigint | string | number;
  }): Promise<{
    from: string;
    to: string;
    amount_usdc: string;
    amount_fiat: string;
    rate: string;
    rate_age_secs: number;
    stale: boolean;
  }> {
    const qs = new URLSearchParams({
      from: params.from,
      to: params.to,
      amount: params.amount.toString(),
    });
    const res = await fetch(`${this.getIndexerUrl()}/v1/fx/convert?${qs}`);
    if (!res.ok) {
      const body = await res.text().catch(() => "");
      throw new Error(`convertCurrency failed (${res.status}): ${body}`);
    }
    return res.json();
  }

  /**
   * Issue #680: Fetch a single invoice by id from the FluxaPay backend.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async createLink(params: CreateLinkParams): Promise<string> {
    return this.getPaymentLinkManager().createLink(params);
  }

  /**
   * Issue #637: Bulk-create up to 50 payment links for one merchant in a single
   * atomic call. Maps to `PaymentLinkManager.batch_create_links` on-chain.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async batchCreateLinks(merchant: string, links: BatchLinkItem[]): Promise<string[]> {
    return this.getPaymentLinkManager().batchCreateLinks(merchant, links);
  }

  /**
   * Issue #632: Atomically create an on-chain invoice together with a payment
   * link and wire them up (`invoice.payment_link_id` is set to the new link ID).
   * Maps to `PaymentProcessor.create_payment_link_invoice`.
   *
   * If link creation fails the invoice is never persisted; if any later step
   * fails the whole transaction reverts.
   *
   * @returns `[invoice, paymentLink]` as returned by the contract
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async createInvoiceWithLink(params: {
    merchantId: string;
    /** PaymentLinkManager contract address (defaults to the configured one) */
    linkManager?: string;
    customerEmail: string;
    lineItems: Array<{ description: string; amount: bigint; quantity: number }>;
    totalAmount: bigint;
    currency: string;
    dueDate: bigint;
    link: {
      linkId: string;
      amount?: bigint;
      currency?: string;
      description?: string;
      expiresAt?: bigint;
      maxUses?: number;
      directTransfer?: boolean;
      metadata?: Record<string, string>;
      baseUrl?: string;
    };
  }): Promise<unknown> {
    const linkManager =
      params.linkManager ??
      resolveContractId(
        this.config.paymentLinkContractId,
        FLUXAPAY_CONTRACT_IDS[this.config.network].paymentLinkManager,
        "paymentLinkContractId",
      );
    return withMappedContractError(() =>
      (this.contract as any).create_payment_link_invoice({
        merchant_id: params.merchantId,
        link_manager: linkManager,
        customer_email: params.customerEmail,
        line_items: params.lineItems,
        total_amount: params.totalAmount,
        currency: params.currency,
        due_date: params.dueDate,
        link_args: {
          link_id: params.link.linkId,
          amount: params.link.amount,
          currency: params.link.currency ?? params.currency,
          description: params.link.description ?? "",
          expires_at: params.link.expiresAt,
          max_uses: params.link.maxUses,
          direct_transfer: params.link.directTransfer ?? false,
          metadata: params.link.metadata,
          fiat: { tag: "None", values: undefined },
          base_url: params.link.baseUrl,
        },
      }),
    );
  }

  /**
   * Create a payment link and return shareable URL + QR code payload.
   *
   * @returns `{ linkId, shareableUrl, qrCodeData }` where `qrCodeData` is the
   * shareable URL (or link ID fallback) suitable for QR generation.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async createPaymentLink(params: CreateLinkParams): Promise<CreatePaymentLinkResult> {
    return this.getPaymentLinkManager().createPaymentLink(params);
  }

  /**
   * Query the on-chain shareable URL for a payment link.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async deactivateLink(merchant: string, linkId: string): Promise<void> {
    return this.getPaymentLinkManager().deactivateLink(merchant, linkId);
  }

  /**
   * Retrieve details of a specific payment link.
   * Maps to `PaymentLinkManager.get_link` on-chain.
   * @param linkId - The payment link ID
   * @returns A promise resolving to the PaymentLink details
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getLink(linkId: string): Promise<PaymentLink> {
    return this.getPaymentLinkManager().getLink(linkId);
  }

  /**
   * Issue #634: List a merchant's payment links, paginated.
   * Maps to `PaymentLinkManager.get_merchant_links` on-chain.
   *
   * @param merchantId - The merchant's Stellar address
   * @param opts.offset - Index into the merchant's link list (default 0)
   * @param opts.limit - Max links to return, 1..=100 (default 100)
   * @param opts.activeOnly - Exclude deactivated/expired links (default false)
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getMerchantLinks(
    merchantId: string,
    opts: { offset?: number; limit?: number; activeOnly?: boolean } = {},
  ): Promise<PaymentLink[]> {
    return this.getPaymentLinkManager().getMerchantLinks(merchantId, opts);
  }

  /**
   * Verify a batch of payment links, returning only active ones.
   * Maps to `PaymentLinkManager.verify_batch` on-chain.
   * @param linkIds - Array of link IDs to verify
   * @returns A promise resolving to an array of active link IDs
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async verifyBatch(linkIds: string[]): Promise<string[]> {
    return this.getPaymentLinkManager().verifyBatch(linkIds);
  }

  /**
   * Record a view of a payment link.
   * Maps to `PaymentLinkManager.record_link_view` on-chain.
   * The viewer must authorize (sign) the transaction; calls are rate-limited
   * per viewer by the contract.
   * @param viewer - The viewer's Stellar address
   * @param linkId - The payment link ID
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async recordLinkView(viewer: string, linkId: string): Promise<void> {
    return this.getPaymentLinkManager().recordLinkView(viewer, linkId);
  }

  /**
   * Retrieve analytics for a payment link.
   * Maps to `PaymentLinkManager.get_link_analytics` on-chain.
   * @param linkId - The payment link ID
   * @returns A promise resolving to the LinkAnalytics
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getLinkAnalytics(linkId: string): Promise<LinkAnalytics> {
    return this.getPaymentLinkManager().getLinkAnalytics(linkId);
  }

  /**
   * Issue #683: Fetch a health summary of the PaymentProcessor contract.
   * No authentication required — this is a public read endpoint.
   * @returns ContractHealth with version, pause state, treasury balance, and config flags.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getContractHealth(): Promise<{
    version: string;
    is_paused: boolean;
    is_creation_paused: boolean;
    treasury_balance: bigint;
    active_payment_count: number;
    fx_oracle_configured: boolean;
    merchant_registry_configured: boolean;
  }> {
    const raw = await (this.contract as any).get_contract_health({});
    return raw.result;
  }

  /**
   * Issue #679: Create a subscription plan (merchant only).
   * Maps to `PaymentProcessor.create_subscription_plan` on-chain.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async createSubscriptionPlan(params: CreatePlanParams): Promise<string> {
    const billingIntervalMap: Record<string, number> = {
      Daily: 0,
      Weekly: 1,
      Monthly: 2,
      Annually: 3,
    };
    await withMappedContractError(() =>
      (this.contract as any).create_subscription_plan({
        merchant: params.merchant,
        plan_id: params.planId,
        name: params.name,
        description: params.description,
        amount: params.amount,
        currency: params.currency,
        billing_interval: billingIntervalMap[params.billingInterval] ?? 2,
        trial_days: params.trialDays ?? null,
      }),
    );
    return params.planId;
  }

  /**
   * Issue #679: Fetch a subscription plan by ID.
   * Maps to `PaymentProcessor.get_subscription_plan` on-chain.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getSubscriptionPlan(planId: string): Promise<SubscriptionPlan> {
    const raw: any = await withMappedContractError(() =>
      (this.contract as any).get_subscription_plan({ plan_id: planId }),
    );
    const p = raw.result;
    const billingIntervalLabels: Record<number, SubscriptionPlan["billingInterval"]> = {
      0: "Daily",
      1: "Weekly",
      2: "Monthly",
      3: "Annually",
    };
    return {
      planId: p.plan_id,
      merchantId: p.merchant_id,
      name: p.name,
      description: p.description,
      amount: p.amount,
      currency: p.currency,
      intervalSecs: p.interval_secs,
      billingInterval: billingIntervalLabels[p.billing_interval] ?? "Monthly",
      active: p.active,
      trialDays: p.trial_days ?? null,
    };
  }

  /**
   * Issue #679: Subscribe a payer to a subscription plan.
   * Maps to `PaymentProcessor.subscribe_to_plan` on-chain.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async subscribeToPlan(params: {
    payer: string;
    planId: string;
    paymentId: string;
  }): Promise<void> {
    return withMappedContractError(() =>
      (this.contract as any).subscribe_to_plan({
        payer: params.payer,
        plan_id: params.planId,
        payment_id: params.paymentId,
      }),
    );
  }

  /**
   * Create a subscription and return its contract-generated ID.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async subscribe(params: SubscribeParams): Promise<string> {
    const raw: any = await withMappedContractError(() =>
      (this.contract as any).subscribe({
        payer: params.payer,
        plan_id: params.planId,
        max_payments: params.maxPayments ?? null,
        affiliate: params.affiliate ?? null,
        affiliate_fee_bps: params.affiliateFeeBps ?? null,
      }),
    );
    return raw.result as string;
  }

  /**
   * Charge a due subscription. This uses the contract's `process_subscription`
   * entry point, which resolves the configured billing token internally.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async chargeSubscription(operator: string, subscriptionId: string): Promise<void> {
    await withMappedContractError(() =>
      (this.contract as any).process_subscription({
        operator,
        subscription_id: subscriptionId,
      }),
    );
  }

  /**
   * cancelSubscription
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async cancelSubscription(caller: string, subscriptionId: string): Promise<void> {
    await withMappedContractError(() =>
      (this.contract as any).cancel_subscription({
        payer_or_merchant: caller,
        subscription_id: subscriptionId,
        refund_remaining: false,
      }),
    );
  }

  /**
   * pauseSubscription
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async pauseSubscription(caller: string, subscriptionId: string): Promise<void> {
    await withMappedContractError(() =>
      (this.contract as any).pause_subscription({ payer: caller, subscription_id: subscriptionId }),
    );
  }

  /**
   * resumeSubscription
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async resumeSubscription(caller: string, subscriptionId: string): Promise<void> {
    await withMappedContractError(() =>
      (this.contract as any).resume_subscription({ payer: caller, subscription_id: subscriptionId }),
    );
  }

  /**
   * getSubscription
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getSubscription(subscriptionId: string): Promise<Subscription> {
    const raw: any = await withMappedContractError(() =>
      (this.contract as any).get_subscription({ subscription_id: subscriptionId }),
    );
    return fromContractSubscription(raw.result);
  }

  /**
   * getPayerSubscriptions
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getPayerSubscriptions(payer: string): Promise<Subscription[]> {
    const raw: any = await withMappedContractError(() =>
      (this.contract as any).get_payer_subscriptions({ payer }),
    );
    return (raw.result as Parameters<typeof fromContractSubscription>[0][]).map(fromContractSubscription);
  }

  /**
   * Issue #633: List the subscribers to a plan, paginated, for plan-level
   * analytics and bulk notifications.
   *
   * Maps to `RefundManager.get_plan_subscribers` on-chain. When
   * `includeCancelled` is `false` (the default), subscriptions with status
   * `Cancelled` are filtered out before pagination. `limit` is hard-capped at
   * 100 per call; pass `0` for the maximum page.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getPlanSubscribers(params: {
    planId: string;
    offset?: number;
    limit?: number;
    includeCancelled?: boolean;
  }): Promise<unknown[]> {
    const raw = await withMappedContractError(() =>
      (this.contract as any).get_plan_subscribers({
        plan_id: params.planId,
        offset: params.offset ?? 0,
        limit: params.limit ?? 100,
        include_cancelled: params.includeCancelled ?? false,
      }),
    );
    return raw.result as unknown[];
  }

  /**
   * Create a new payment stream. Tokens are pulled from `params.sender` into
   * the contract and streamed to `params.receiver` at `ratePerSecond`.
   * Maps to `PaymentProcessor.create_stream` on-chain.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * Issue #576: Batch withdraw accrued tokens from multiple streams to a recipient.
   * Maps to `PaymentProcessor.batch_withdraw_to` on-chain.
   *
   * @param recipient - Must be the streams' receiver; must sign.
   * @param streamIds - List of stream IDs (or withdrawal specs) to withdraw from.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async batchWithdrawTo(
    recipient: string,
    streamIds: string[] | StreamWithdrawal[],
  ): Promise<void> {
    return withMappedContractError(async () => {
      const withdrawals = streamIds.map((item) => {
        if (typeof item === "string") {
          return {
            stream_id: item,
            destination: recipient,
            amount: I128_MAX,
          };
        }
        return {
          stream_id: item.streamId,
          destination: item.destination ?? recipient,
          amount: item.amount ?? I128_MAX,
        };
      });
      await (this.contract as any).batch_withdraw_to({
        recipient,
        withdrawals,
      });
    });
  }

  /**
   * setStreamFeeRecipient
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async setStreamFeeRecipient(admin: string, recipient: string): Promise<void> {
    return withMappedContractError(() =>
      (this.contract as any).set_stream_fee_recipient({ admin, recipient }),
    );
  }

  /**
   * getStreamFeeRecipient
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async getStreamFeeRecipient(): Promise<string | null> {
    return withMappedContractError(() =>
      (this.contract as any).get_stream_fee_recipient({}),
    );
  }

  /**
   * Cancel an active stream and refund any un-accrued deposit to the sender.
   * Maps to `PaymentProcessor.cancel_stream` on-chain.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async cancelStream(sender: string, streamId: string): Promise<void> {
    return withMappedContractError(() =>
      (this.contract as any).cancel_stream({ sender, stream_id: streamId }),
    );
  }

  /**
   * Issue #576: Cancel multiple active streams in a single transaction.
   * Maps to `PaymentProcessor.cancel_multiple_streams` on-chain.
   *
   * @param sender - The creator of the streams; must sign.
   * @param streamIds - List of stream IDs to cancel.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async cancelMultipleStreams(sender: string, streamIds: string[]): Promise<void> {
    return withMappedContractError(async () => {
      await (this.contract as any).cancel_multiple_streams({
        sender,
        stream_ids: streamIds,
      });
    });
  }

  /**
   * Pause an active stream, freezing accrual until resumed.
   * Maps to `PaymentProcessor.pause_stream` on-chain.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async pauseStream(sender: string, streamId: string): Promise<void> {
    return withMappedContractError(() =>
      (this.contract as any).pause_stream({ sender, stream_id: streamId }),
    );
  }

  /**
   * Resume a paused stream, restarting accrual from the current timestamp.
   * Maps to `PaymentProcessor.resume_stream` on-chain.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async resumeStream(sender: string, streamId: string): Promise<void> {
    return withMappedContractError(() =>
      (this.contract as any).resume_stream({ sender, stream_id: streamId }),
    );
  }

  /**
   * Top up an existing stream's deposit.
   * Maps to `PaymentProcessor.top_up_stream` on-chain.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async topUpStream(sender: string, streamId: string, amount: bigint): Promise<void> {
    return withMappedContractError(() =>
      (this.contract as any).top_up_stream({ caller: sender, stream_id: streamId, amount }),
    );
  }

  /**
   * Issue #576: Top up multiple streams in a single transaction.
   * Maps to `PaymentProcessor.top_up_multiple_streams` on-chain.
   *
   * @param sender - The sender funding the top-ups; must sign.
   * @param topUps - List of stream IDs and top-up amounts.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async topUpMultipleStreams(
    sender: string,
    topUps: StreamTopUp[] | Array<{ streamId: string; amount: bigint }>,
  ): Promise<void> {
    return withMappedContractError(async () => {
      const formatted = topUps.map((t) => [t.streamId, t.amount]);
      await (this.contract as any).top_up_multiple_streams({
        sender,
        top_ups: formatted,
      });
    });
  }

  /**
   * Issue #627: Bulk-extend the persistent-storage TTL of many streams in one
   * call, instead of paying a TTL write per stream interaction. Mirrors
   * `bulkBumpPaymentTTLs`.
   *
   * Permissionless (TTL bumps only extend lifetime). Stream IDs that don't
   * resolve to a stored stream are silently skipped; batches larger than 50 are
   * rejected with a mapped `BatchTooLarge` error. Returns the number of streams
   * whose TTL was bumped.
   *
   * Maps to `PaymentProcessor.bulk_bump_stream_ttls` on-chain.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  async bulkBumpStreamTTLs(streamIds: string[]): Promise<number> {
    const result = await withMappedContractError(() =>
      (this.contract as any).bulk_bump_stream_ttls({ stream_ids: streamIds }),
    );
    const value = (result as { result?: unknown }).result ?? result;
    return Number(value as number | bigint);
  }

  /**
   * Get stream details by ID.
   * Maps to `PaymentProcessor.get_stream` on-chain.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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
   * @throws {FluxapayError} If the contract operation fails or returns an error.
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

  /**
   * Offline/hardware wallet payload builder utilities.
   * @throws {FluxapayError} If the contract operation fails or returns an error.
   */
  offlineSigner(): FluxapayOfflineSigner {
    return new FluxapayOfflineSigner(
      this.contract as import("./offline-signer.js").OfflineCapableClient,
      this.contract.options.contractId,
      this.contract.options.networkPassphrase,
    );
  }
}

export {
  type Merchant,
  type PaymentCharge,
  type Refund,
  type Dispute,
  type PaymentStatus,
  type RefundStatus,
  type DisputeStatus,
  type FeeConfig,
  type MaybeFeeConfig,
  type CreatePaymentArgs,
  FluxapayOfflineSigner,
  type OfflineTransactionPayload,
  type SubscriptionBillingClient,
  buildOfflinePayload,
  buildCreatePaymentPayload,
  buildVerifyPaymentPayload,
  buildCreateRefundPayload,
  buildSubscriptionTickPayload,
  buildPullAuthorizationPayload,
  prepareForOfflineSigning,
  restoreFromOfflinePayload,
  NetworkProfileSwitcher,
  type NetworkEnvironment,
  NetworkProfiles,
  type NetworkProfile,
};
export {
  buildMetaTransactionPayload,
  domainSeparator,
  signingPreimage,
  signingDigest,
  signMetaTransaction,
  META_TX_FUNCTIONS,
  type MetaTransactionPayload,
  type MetaTransactionTarget,
} from "./metaTx.js";

export { RefundManagerClient, type RefundManagerConfig } from "./contracts/refund-manager.js";
export {
  MerchantRegistryClient,
  type MerchantRegistryConfig,
  type AddCurrencyPayoutParams,
  type CurrencyPayout,
  type BankAccount,
} from "./contracts/merchant-registry.js";
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
export {
  AdminOpsClient,
  type AdminOpsConfig,
  type AdminAction,
  type FeeSplitConfig,
} from "./contracts/admin-ops.js";
export {
  verifyReceipt,
  buildPaymentReceipt,
  buildReceiptMessage,
  buildReceiptUrl,
  formatReceiptAmount,
  signReceiptProof,
  DEFAULT_RECEIPT_BASE_URL,
  type PaymentReceipt,
  type ReceiptSignedFields,
  type BuildPaymentReceiptParams,
} from "./receipt.js";

// Issue #765: Export typed event payload interfaces, events namespace, and parseFluxapayEvent helper
export * as events from "./events.js";
export {
  parseFluxapayEvent,
  type FluxapayEvent,
  type BaseFluxapayEvent,
  type RawEventInput,
  type PaymentCreatedEvent,
  type PaymentCreatedPayload,
  type PaymentConfirmedEvent,
  type PaymentConfirmedPayload,
  type PaymentVerifiedEvent,
  type PaymentVerifiedPayload,
  type PaymentSettledEvent,
  type PaymentSettledPayload,
  type PaymentCancelledEvent,
  type PaymentCancelledPayload,
  type PaymentExpiredEvent,
  type PaymentExpiredPayload,
  type PaymentPartiallyPaidEvent,
  type PaymentPartiallyPaidPayload,
  type PaymentOverpaidEvent,
  type PaymentOverpaidPayload,
  type PaymentFailedEvent,
  type PaymentFailedPayload,
  type RefundRequestedEvent,
  type RefundRequestedPayload,
  type RefundCreatedEvent,
  type RefundCreatedPayload,
  type RefundProcessedEvent,
  type RefundProcessedPayload,
  type RefundCompletedEvent,
  type RefundCompletedPayload,
  type RefundRejectedEvent,
  type RefundRejectedPayload,
  type DisputeCreatedEvent,
  type DisputeCreatedPayload,
  type DisputeReviewedEvent,
  type DisputeReviewedPayload,
  type DisputeResolvedEvent,
  type DisputeResolvedPayload,
  type DisputeRejectedEvent,
  type DisputeRejectedPayload,
  type DisputeEscalatedEvent,
  type DisputeEscalatedPayload,
  type DisputeBondReturnedEvent,
  type DisputeBondReturnedPayload,
  type DisputeBondForfeitedEvent,
  type DisputeBondForfeitedPayload,
  type MerchantRegisteredEvent,
  type MerchantRegisteredPayload,
  type MerchantUpdatedEvent,
  type MerchantUpdatedPayload,
  type MerchantVerifiedEvent,
  type MerchantVerifiedPayload,
  type MerchantSuspendedEvent,
  type MerchantSuspendedPayload,
  type MerchantReinstatedEvent,
  type MerchantReinstatedPayload,
  type KycTierUpgradedEvent,
  type KycTierUpgradedPayload,
  type LinkCreatedEvent,
  type LinkCreatedPayload,
  type LinkUsedEvent,
  type LinkUsedPayload,
  type LinkDeactivatedEvent,
  type LinkDeactivatedPayload,
  type LinkExpiredEvent,
  type LinkExpiredPayload,
  type LinkViewedEvent,
  type LinkViewedPayload,
  type SubscriptionCreatedEvent,
  type SubscriptionCreatedPayload,
  type SubscriptionChargedEvent,
  type SubscriptionChargedPayload,
  type SubscriptionCancelledEvent,
  type SubscriptionCancelledPayload,
  type SubscriptionExpiredEvent,
  type SubscriptionExpiredPayload,
  type StreamCreatedEvent,
  type StreamCreatedPayload,
  type StreamToppedUpEvent,
  type StreamToppedUpPayload,
  type StreamWithdrawnEvent,
  type StreamWithdrawnPayload,
  type StreamCancelledEvent,
  type StreamCancelledPayload,
  type StreamPausedEvent,
  type StreamPausedPayload,
  type StreamResumedEvent,
  type StreamResumedPayload,
  type StreamRateUpdatedEvent,
  type StreamRateUpdatedPayload,
  type StreamRateDecreasedEvent,
  type StreamRateDecreasedPayload,
  type StreamMilestoneApprovedEvent,
  type StreamMilestoneApprovedPayload,
  type StreamDestinationSetEvent,
  type StreamDestinationSetPayload,
  type StreamClosedEvent,
  type StreamClosedPayload,
  type RateUpdatedEvent,
  type RateUpdatedPayload,
  type RoleGrantedEvent,
  type RoleGrantedPayload,
  type RoleRevokedEvent,
  type RoleRevokedPayload,
  type AdminTransferProposedEvent,
  type AdminTransferProposedPayload,
  type AdminTransferCompletedEvent,
  type AdminTransferCompletedPayload,
  type AdminTransferCancelledEvent,
  type AdminTransferCancelledPayload,
  type FeeSplitUpdatedEvent,
  type FeeSplitUpdatedPayload,
  type TreasuryWithdrawnEvent,
  type TreasuryWithdrawnPayload,
  type ContractUpgradedEvent,
  type ContractUpgradedPayload,
  type InvoiceCreatedEvent,
  type InvoiceCreatedPayload,
  type InvoicePaidEvent,
  type InvoicePaidPayload,
  type InvoiceOverdueEvent,
  type InvoiceOverduePayload,
  type SwapExecutedEvent,
  type SwapExecutedPayload,
  type UnknownFluxapayEvent,
} from "./events.js";



