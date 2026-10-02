import { getLocalizedErrorMessage } from "./locales/index.js";

export class FluxapayError extends Error {
  readonly code: number;
  readonly contractErrorName: string;
  readonly cause?: unknown;
  readonly locale: string;

  constructor(
    code: number,
    contractErrorName: string,
    message?: string,
    cause?: unknown,
    locale = "en",
  ) {
    super(message ?? contractErrorName);
    this.name = `${contractErrorName}Error`;
    this.code = code;
    this.contractErrorName = contractErrorName;
    this.cause = cause;
    this.locale = locale;
  }

  get localizedMessage(): string {
    return getLocalizedErrorMessage(this.code, this.locale, this.message);
  }
}

export class UnauthorizedError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(1, "Unauthorized", message, cause, locale);
    this.name = "UnauthorizedError";
  }
}

export class PaymentAlreadyExistsError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(2, "PaymentAlreadyExists", message, cause, locale);
    this.name = "PaymentAlreadyExistsError";
  }
}

export class PaymentExpiredError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(3, "PaymentExpired", message, cause, locale);
    this.name = "PaymentExpiredError";
  }
}

export class InvalidPaymentIdError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(4, "InvalidPaymentId", message, cause, locale);
    this.name = "InvalidPaymentIdError";
  }
}

export class RefundAlreadyProcessedError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(8, "RefundAlreadyProcessed", message, cause, locale);
    this.name = "RefundAlreadyProcessedError";
  }
}

export class DisputeNotFoundError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(9, "DisputeNotFound", message, cause, locale);
    this.name = "DisputeNotFoundError";
  }
}

export class DisputeAlreadyResolvedError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(12, "DisputeAlreadyResolved", message, cause, locale);
    this.name = "DisputeAlreadyResolvedError";
  }
}

export class PaymentAlreadyProcessedError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(14, "PaymentAlreadyProcessed", message, cause, locale);
    this.name = "PaymentAlreadyProcessedError";
  }
}

export class AccessControlContractError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(15, "AccessControlError", message, cause, locale);
    this.name = "AccessControlContractError";
  }
}

export class RefundExceedsPaymentError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(16, "RefundExceedsPayment", message, cause, locale);
    this.name = "RefundExceedsPaymentError";
  }
}

export class ContractPausedError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(17, "ContractPaused", message, cause, locale);
    this.name = "ContractPausedError";
  }
}

export class RateLimitExceededError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(18, "RateLimitExceeded", message, cause, locale);
    this.name = "RateLimitExceededError";
  }
}

export class RefundCancelledError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(19, "RefundCancelled", message, cause, locale);
    this.name = "RefundCancelledError";
  }
}

export class UnsupportedTokenError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(20, "UnsupportedToken", message, cause, locale);
    this.name = "UnsupportedTokenError";
  }
}

export class AmountBelowMinError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(21, "AmountBelowMin", message, cause, locale);
    this.name = "AmountBelowMinError";
  }
}

export class AmountAboveMaxError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(22, "AmountAboveMax", message, cause, locale);
    this.name = "AmountAboveMaxError";
  }
}

export class InvalidExpiryError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(23, "InvalidExpiry", message, cause, locale);
    this.name = "InvalidExpiryError";
  }
}

export class InvalidSettlementError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(24, "InvalidSettlement", message, cause, locale);
    this.name = "InvalidSettlementError";
  }
}

export class DuplicateIdempotencyKeyError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(25, "DuplicateIdempotencyKey", message, cause, locale);
    this.name = "DuplicateIdempotencyKeyError";
  }
}

export class InvalidAddressError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(26, "InvalidAddress", message, cause, locale);
    this.name = "InvalidAddressError";
  }
}

export class ArbitrageDetectedError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(27, "ArbitrageDetected", message, cause, locale);
    this.name = "ArbitrageDetectedError";
  }
}

export class SwapPathInvalidError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(28, "SwapPathInvalid", message, cause, locale);
    this.name = "SwapPathInvalidError";
  }
}

export class OraclePriceDeviationError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(29, "OraclePriceDeviation", message, cause, locale);
    this.name = "OraclePriceDeviationError";
  }
}

export class SubscriptionInGracePeriodError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(30, "SubscriptionInGracePeriod", message, cause, locale);
    this.name = "SubscriptionInGracePeriodError";
  }
}

export class SubscriptionRetryExhaustedError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(31, "SubscriptionRetryExhausted", message, cause, locale);
    this.name = "SubscriptionRetryExhaustedError";
  }
}

export class InvalidResumeTimestampError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(32, "InvalidResumeTimestamp", message, cause, locale);
    this.name = "InvalidResumeTimestampError";
  }
}

export class MerchantAuthContractError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(33, "MerchantAuthError", message, cause, locale);
    this.name = "MerchantAuthContractError";
  }
}

export class InvalidSplitSumError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(34, "InvalidSplitSum", message, cause, locale);
    this.name = "InvalidSplitSumError";
  }
}

export class MissingReceiptHashError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(35, "MissingReceiptHash", message, cause, locale);
    this.name = "MissingReceiptHashError";
  }
}

export class RefundExpiredError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(36, "RefundExpired", message, cause, locale);
    this.name = "RefundExpiredError";
  }
}

export class AlreadyVotedError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(37, "AlreadyVoted", message, cause, locale);
    this.name = "AlreadyVotedError";
  }
}

export class TierVolumeLimitExceededError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(38, "TierVolumeLimitExceeded", message, cause, locale);
    this.name = "TierVolumeLimitExceededError";
  }
}

export class BatchTooLargeContractError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(39, "BatchTooLarge", message, cause, locale);
    this.name = "BatchTooLargeContractError";
  }
}

export class InsufficientArbitratorsError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(40, "InsufficientArbitrators", message, cause, locale);
    this.name = "InsufficientArbitratorsError";
  }
}

export class ArbitrationVotingThresholdNotMetError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(41, "ArbitrationVotingThresholdNotMet", message, cause, locale);
    this.name = "ArbitrationVotingThresholdNotMetError";
  }
}

export class RefundCooldownNotElapsedError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(42, "RefundCooldownNotElapsed", message, cause, locale);
    this.name = "RefundCooldownNotElapsedError";
  }
}

export class FeeProposalNotReadyError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(43, "FeeProposalNotReady", message, cause, locale);
    this.name = "FeeProposalNotReadyError";
  }
}

export class NoFeeProposalError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(44, "NoFeeProposal", message, cause, locale);
    this.name = "NoFeeProposalError";
  }
}

export class InvalidEvidenceFormatError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(45, "InvalidEvidenceFormat", message, cause, locale);
    this.name = "InvalidEvidenceFormatError";
  }
}

export class DisputeRateLimitExceededError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(46, "DisputeRateLimitExceeded", message, cause, locale);
    this.name = "DisputeRateLimitExceededError";
  }
}

export class InvalidSettlementSignatureError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(47, "InvalidSettlementSignature", message, cause, locale);
    this.name = "InvalidSettlementSignatureError";
  }
}

export class StaleOracleRateError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(48, "StaleOracleRate", message, cause, locale);
    this.name = "StaleOracleRateError";
  }
}

export class LinkExpiredError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(49, "LinkExpired", message, cause, locale);
    this.name = "LinkExpiredError";
  }
}

export class ReentrancyError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(50, "Reentrancy", message, cause, locale);
    this.name = "ReentrancyError";
  }
}

export class UpgradeFailedError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(51, "UpgradeFailed", message, cause, locale);
    this.name = "UpgradeFailedError";
  }
}

export class InsufficientTreasuryBalanceError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(52, "InsufficientTreasuryBalance", message, cause, locale);
    this.name = "InsufficientTreasuryBalanceError";
  }
}

export class MetadataTooLargeError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(53, "MetadataTooLarge", message, cause, locale);
    this.name = "MetadataTooLargeError";
  }
}

export class MetadataValueTooLongError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(54, "MetadataValueTooLong", message, cause, locale);
    this.name = "MetadataValueTooLongError";
  }
}

export class InvalidMemoTypeError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(55, "InvalidMemoType", message, cause, locale);
    this.name = "InvalidMemoTypeError";
  }
}

export class MemoTooLongError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(56, "MemoTooLong", message, cause, locale);
    this.name = "MemoTooLongError";
  }
}

export class InvalidMemoIdError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(57, "InvalidMemoId", message, cause, locale);
    this.name = "InvalidMemoIdError";
  }
}

export class PayerNotWhitelistedError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(58, "PayerNotWhitelisted", message, cause, locale);
    this.name = "PayerNotWhitelistedError";
  }
}

export class LinkMaxUsesReachedError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(59, "LinkMaxUsesReached", message, cause, locale);
    this.name = "LinkMaxUsesReachedError";
  }
}

export class DirectTransferNotDisputableError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(60, "DirectTransferNotDisputable", message, cause, locale);
    this.name = "DirectTransferNotDisputableError";
  }
}

export class MaxRetriesExceededError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(61, "MaxRetriesExceeded", message, cause, locale);
    this.name = "MaxRetriesExceededError";
  }
}

export class RetryChainTooDeepError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(347, "RetryChainTooDeep", message, cause, locale);
    this.name = "RetryChainTooDeepError";
  }
}

export class InvalidStatusTransitionError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(62, "InvalidStatusTransition", message, cause, locale);
    this.name = "InvalidStatusTransitionError";
  }
}

export class RefundNotApprovedError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(63, "RefundNotApproved", message, cause, locale);
    this.name = "RefundNotApprovedError";
  }
}

export class RouterNotAllowedError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(64, "RouterNotAllowed", message, cause, locale);
    this.name = "RouterNotAllowedError";
  }
}

export class RouteOutputInsufficientError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(65, "RouteOutputInsufficient", message, cause, locale);
    this.name = "RouteOutputInsufficientError";
  }
}

export class BatchContainsDuplicatesError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(66, "BatchContainsDuplicates", message, cause, locale);
    this.name = "BatchContainsDuplicatesError";
  }
}

export class InputTooLongError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(67, "InputTooLong", message, cause, locale);
    this.name = "InputTooLongError";
  }
}

export class TimelockNotExpiredError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(68, "TimelockNotExpired", message, cause, locale);
    this.name = "TimelockNotExpiredError";
  }
}

export class InvalidEvidenceCidError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(69, "InvalidEvidenceCid", message, cause, locale);
    this.name = "InvalidEvidenceCidError";
  }
}

export class MuxedAccountMismatchError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(70, "MuxedAccountMismatch", message, cause, locale);
    this.name = "MuxedAccountMismatchError";
  }
}

export class TreasuryMultisigNotConfiguredError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(71, "TreasuryMultisigNotConfigured", message, cause, locale);
    this.name = "TreasuryMultisigNotConfiguredError";
  }
}

export class NotAuthorizedTreasurySignerError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(72, "NotAuthorizedTreasurySigner", message, cause, locale);
    this.name = "NotAuthorizedTreasurySignerError";
  }
}

export class TreasuryProposalNotFoundError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(73, "TreasuryProposalNotFound", message, cause, locale);
    this.name = "TreasuryProposalNotFoundError";
  }
}

export class TreasuryProposalAlreadyExecutedError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(74, "TreasuryProposalAlreadyExecuted", message, cause, locale);
    this.name = "TreasuryProposalAlreadyExecutedError";
  }
}

export class TreasuryProposalCancelledError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(75, "TreasuryProposalCancelled", message, cause, locale);
    this.name = "TreasuryProposalCancelledError";
  }
}

export class TreasuryTimelockNotExpiredError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(76, "TreasuryTimelockNotExpired", message, cause, locale);
    this.name = "TreasuryTimelockNotExpiredError";
  }
}

export class TreasuryProposalExpiredError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(77, "TreasuryProposalExpired", message, cause, locale);
    this.name = "TreasuryProposalExpiredError";
  }
}

export class TreasuryAlreadyApprovedError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(78, "TreasuryAlreadyApproved", message, cause, locale);
    this.name = "TreasuryAlreadyApprovedError";
  }
}

export class TreasuryInsufficientApprovalsError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(79, "TreasuryInsufficientApprovals", message, cause, locale);
    this.name = "TreasuryInsufficientApprovalsError";
  }
}

export class InvalidTreasuryThresholdError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(80, "InvalidTreasuryThreshold", message, cause, locale);
    this.name = "InvalidTreasuryThresholdError";
  }
}

export class InsufficientTokenTreasuryBalanceError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(81, "InsufficientTokenTreasuryBalance", message, cause, locale);
    this.name = "InsufficientTokenTreasuryBalanceError";
  }
}

export class TrialActiveError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(82, "TrialActive", message, cause, locale);
    this.name = "TrialActiveError";
  }
}

export class TrialTooLongError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(83, "TrialTooLong", message, cause, locale);
    this.name = "TrialTooLongError";
  }
}

export class InvalidPaymentLinkError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(84, "InvalidPaymentLink", message, cause, locale);
    this.name = "InvalidPaymentLinkError";
  }
}

export class KycLimitExceededError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(85, "KycLimitExceeded", message, cause, locale);
    this.name = "KycLimitExceededError";
  }
}

export class PaymentNotFoundError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(404, "PaymentNotFound", message, cause, locale);
    this.name = "PaymentNotFoundError";
  }
}

export class RefundNotFoundError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(405, "RefundNotFound", message, cause, locale);
    this.name = "RefundNotFoundError";
  }
}

export class InvalidAmountError extends FluxapayError {
  constructor(message?: string, cause?: unknown, locale = "en") {
    super(406, "InvalidAmount", message, cause, locale);
    this.name = "InvalidAmountError";
  }
}

export const ERROR_CONSTRUCTOR_MAP: Record<
  number,
  new (message?: string, cause?: unknown, locale?: string) => FluxapayError
> = {
  1: UnauthorizedError,
  2: PaymentAlreadyExistsError,
  3: PaymentExpiredError,
  4: InvalidPaymentIdError,
  8: RefundAlreadyProcessedError,
  9: DisputeNotFoundError,
  12: DisputeAlreadyResolvedError,
  14: PaymentAlreadyProcessedError,
  15: AccessControlContractError,
  16: RefundExceedsPaymentError,
  17: ContractPausedError,
  18: RateLimitExceededError,
  19: RefundCancelledError,
  20: UnsupportedTokenError,
  21: AmountBelowMinError,
  22: AmountAboveMaxError,
  23: InvalidExpiryError,
  24: InvalidSettlementError,
  25: DuplicateIdempotencyKeyError,
  26: InvalidAddressError,
  27: ArbitrageDetectedError,
  28: SwapPathInvalidError,
  29: OraclePriceDeviationError,
  30: SubscriptionInGracePeriodError,
  31: SubscriptionRetryExhaustedError,
  32: InvalidResumeTimestampError,
  33: MerchantAuthContractError,
  34: InvalidSplitSumError,
  35: MissingReceiptHashError,
  36: RefundExpiredError,
  37: AlreadyVotedError,
  38: TierVolumeLimitExceededError,
  39: BatchTooLargeContractError,
  40: InsufficientArbitratorsError,
  41: ArbitrationVotingThresholdNotMetError,
  42: RefundCooldownNotElapsedError,
  43: FeeProposalNotReadyError,
  44: NoFeeProposalError,
  45: InvalidEvidenceFormatError,
  46: DisputeRateLimitExceededError,
  47: InvalidSettlementSignatureError,
  48: StaleOracleRateError,
  49: LinkExpiredError,
  50: ReentrancyError,
  51: UpgradeFailedError,
  52: InsufficientTreasuryBalanceError,
  53: MetadataTooLargeError,
  54: MetadataValueTooLongError,
  55: InvalidMemoTypeError,
  56: MemoTooLongError,
  57: InvalidMemoIdError,
  58: PayerNotWhitelistedError,
  59: LinkMaxUsesReachedError,
  60: DirectTransferNotDisputableError,
  61: MaxRetriesExceededError,
  347: RetryChainTooDeepError,
  62: InvalidStatusTransitionError,
  63: RefundNotApprovedError,
  64: RouterNotAllowedError,
  65: RouteOutputInsufficientError,
  66: BatchContainsDuplicatesError,
  67: InputTooLongError,
  68: TimelockNotExpiredError,
  69: InvalidEvidenceCidError,
  70: MuxedAccountMismatchError,
  71: TreasuryMultisigNotConfiguredError,
  72: NotAuthorizedTreasurySignerError,
  73: TreasuryProposalNotFoundError,
  74: TreasuryProposalAlreadyExecutedError,
  75: TreasuryProposalCancelledError,
  76: TreasuryTimelockNotExpiredError,
  77: TreasuryProposalExpiredError,
  78: TreasuryAlreadyApprovedError,
  79: TreasuryInsufficientApprovalsError,
  80: InvalidTreasuryThresholdError,
  81: InsufficientTokenTreasuryBalanceError,
  82: TrialActiveError,
  83: TrialTooLongError,
  84: InvalidPaymentLinkError,
  85: KycLimitExceededError,
  404: PaymentNotFoundError,
  405: RefundNotFoundError,
  406: InvalidAmountError,
};
