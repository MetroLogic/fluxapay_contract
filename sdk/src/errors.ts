import { getLocalizedErrorMessage } from "./locales/index.js";

export interface FluxapayErrorMeta {
  code: string;
  contractCode: number;
  message: string;
  hint: string;
  docsUrl: string;
}

const DOCS_BASE = "https://docs.fluxapay.com/errors";

function buildDocsUrl(code: string): string {
  return `${DOCS_BASE}#${code}`;
}

/**
 * Canonical mapping of contract error codes to human-readable metadata.
 * Every entry must include a specific, actionable `hint` and a `docsUrl`.
 * Keep in sync with docs/error-codes.md (enforced by scripts/check-error-map-sync.ts).
 */
export const ERROR_MAP: Record<number, FluxapayErrorMeta> = {
  1: {
    code: "UNAUTHORIZED",
    contractCode: 1,
    message: "The caller is not authorized to perform this action.",
    hint: "Ensure the transaction is signed by the merchant or admin address that owns this resource.",
    docsUrl: buildDocsUrl("UNAUTHORIZED"),
  },
  2: {
    code: "PAYMENT_ALREADY_EXISTS",
    contractCode: 2,
    message: "A payment with this ID already exists.",
    hint: "Generate a new unique payment ID or reuse the existing payment instead of creating a duplicate.",
    docsUrl: buildDocsUrl("PAYMENT_ALREADY_EXISTS"),
  },
  3: {
    code: "PAYMENT_EXPIRED",
    contractCode: 3,
    message: "The payment has expired and can no longer be processed.",
    hint: "Create a new payment with a future expiry timestamp before attempting settlement.",
    docsUrl: buildDocsUrl("PAYMENT_EXPIRED"),
  },
  4: {
    code: "INVALID_PAYMENT_ID",
    contractCode: 4,
    message: "The provided payment ID is invalid.",
    hint: "Verify the payment ID is a 32-byte hex string returned by create_payment.",
    docsUrl: buildDocsUrl("INVALID_PAYMENT_ID"),
  },
  8: {
    code: "REFUND_ALREADY_PROCESSED",
    contractCode: 8,
    message: "This refund has already been processed.",
    hint: "Query the refund status before retrying; do not resubmit a refund that already completed.",
    docsUrl: buildDocsUrl("REFUND_ALREADY_PROCESSED"),
  },
  9: {
    code: "DISPUTE_NOT_FOUND",
    contractCode: 9,
    message: "The referenced dispute does not exist.",
    hint: "Confirm the dispute ID was returned by open_dispute before resolving it.",
    docsUrl: buildDocsUrl("DISPUTE_NOT_FOUND"),
  },
  12: {
    code: "DISPUTE_ALREADY_RESOLVED",
    contractCode: 12,
    message: "The dispute has already been resolved.",
    hint: "Fetch the dispute to read its resolution before attempting to resolve it again.",
    docsUrl: buildDocsUrl("DISPUTE_ALREADY_RESOLVED"),
  },
  14: {
    code: "PAYMENT_ALREADY_PROCESSED",
    contractCode: 14,
    message: "The payment has already been processed.",
    hint: "Check the payment status; only pending payments can be settled or refunded.",
    docsUrl: buildDocsUrl("PAYMENT_ALREADY_PROCESSED"),
  },
  15: {
    code: "ACCESS_CONTROL_ERROR",
    contractCode: 15,
    message: "The access control check failed.",
    hint: "Verify the caller holds the required role (admin/merchant) via the access control contract.",
    docsUrl: buildDocsUrl("ACCESS_CONTROL_ERROR"),
  },
  16: {
    code: "REFUND_EXCEEDS_PAYMENT",
    contractCode: 16,
    message: "The total refund amount exceeds the original payment amount.",
    hint: "Check the sum of all pending and completed refunds for this payment before creating a new one.",
    docsUrl: buildDocsUrl("REFUND_EXCEEDS_PAYMENT"),
  },
  17: {
    code: "CONTRACT_PAUSED",
    contractCode: 17,
    message: "The contract is currently paused.",
    hint: "Wait for the admin to unpause the contract before retrying the transaction.",
    docsUrl: buildDocsUrl("CONTRACT_PAUSED"),
  },
  18: {
    code: "RATE_LIMIT_EXCEEDED",
    contractCode: 18,
    message: "The rate limit for this operation has been exceeded.",
    hint: "Back off and retry after the rate-limit window resets; batch requests where possible.",
    docsUrl: buildDocsUrl("RATE_LIMIT_EXCEEDED"),
  },
  19: {
    code: "REFUND_CANCELLED",
    contractCode: 19,
    message: "The refund has been cancelled.",
    hint: "Create a new refund request instead of retrying a cancelled refund.",
    docsUrl: buildDocsUrl("REFUND_CANCELLED"),
  },
  20: {
    code: "UNSUPPORTED_TOKEN",
    contractCode: 20,
    message: "The token is not supported by this contract.",
    hint: "Use a token address from the supported-token allowlist configured on the contract.",
    docsUrl: buildDocsUrl("UNSUPPORTED_TOKEN"),
  },
  21: {
    code: "AMOUNT_BELOW_MIN",
    contractCode: 21,
    message: "The amount is below the minimum allowed.",
    hint: "Increase the amount to at least the contract's configured minimum before retrying.",
    docsUrl: buildDocsUrl("AMOUNT_BELOW_MIN"),
  },
  22: {
    code: "AMOUNT_ABOVE_MAX",
    contractCode: 22,
    message: "The amount is above the maximum allowed.",
    hint: "Reduce the amount to at most the contract's configured maximum before retrying.",
    docsUrl: buildDocsUrl("AMOUNT_ABOVE_MAX"),
  },
  23: {
    code: "INVALID_EXPIRY",
    contractCode: 23,
    message: "The expiry timestamp is invalid.",
    hint: "Provide an expiry timestamp strictly greater than the current ledger time.",
    docsUrl: buildDocsUrl("INVALID_EXPIRY"),
  },
  24: {
    code: "INVALID_SETTLEMENT",
    contractCode: 24,
    message: "The settlement parameters are invalid.",
    hint: "Verify the settlement asset and destination address match the payment's configuration.",
    docsUrl: buildDocsUrl("INVALID_SETTLEMENT"),
  },
  25: {
    code: "DUPLICATE_IDEMPOTENCY_KEY",
    contractCode: 25,
    message: "The idempotency key has already been used.",
    hint: "Reuse the original response for this key or generate a fresh idempotency key.",
    docsUrl: buildDocsUrl("DUPLICATE_IDEMPOTENCY_KEY"),
  },
  26: {
    code: "INVALID_ADDRESS",
    contractCode: 26,
    message: "The provided address is invalid.",
    hint: "Ensure the address is a valid Stellar account (G...) or contract (C...) address.",
    docsUrl: buildDocsUrl("INVALID_ADDRESS"),
  },
  27: {
    code: "ARBITRAGE_DETECTED",
    contractCode: 27,
    message: "Arbitrage was detected in the swap.",
    hint: "Adjust the swap path or slippage tolerance so the quoted rate matches the oracle price.",
    docsUrl: buildDocsUrl("ARBITRAGE_DETECTED"),
  },
  28: {
    code: "SWAP_PATH_INVALID",
    contractCode: 28,
    message: "The swap path is invalid.",
    hint: "Verify each hop in the swap path references a supported liquidity pool.",
    docsUrl: buildDocsUrl("SWAP_PATH_INVALID"),
  },
  29: {
    code: "ORACLE_PRICE_DEVIATION",
    contractCode: 29,
    message: "The oracle price deviates beyond the allowed threshold.",
    hint: "Wait for the oracle to refresh or widen the deviation threshold before retrying.",
    docsUrl: buildDocsUrl("ORACLE_PRICE_DEVIATION"),
  },
  30: {
    code: "SUBSCRIPTION_IN_GRACE_PERIOD",
    contractCode: 30,
    message: "The subscription is in its grace period.",
    hint: "Top up the subscription balance to exit the grace period before charging again.",
    docsUrl: buildDocsUrl("SUBSCRIPTION_IN_GRACE_PERIOD"),
  },
  31: {
    code: "SUBSCRIPTION_RETRY_EXHAUSTED",
    contractCode: 31,
    message: "The subscription retry limit has been exhausted.",
    hint: "Re-enable the subscription with a valid payment method before the next billing cycle.",
    docsUrl: buildDocsUrl("SUBSCRIPTION_RETRY_EXHAUSTED"),
  },
  32: {
    code: "INVALID_RESUME_TIMESTAMP",
    contractCode: 32,
    message: "The resume timestamp is invalid.",
    hint: "Provide a resume timestamp in the future relative to the current ledger time.",
    docsUrl: buildDocsUrl("INVALID_RESUME_TIMESTAMP"),
  },
  33: {
    code: "MERCHANT_AUTH_ERROR",
    contractCode: 33,
    message: "Merchant authentication failed.",
    hint: "Confirm the merchant is registered and the signing key matches the registered merchant key.",
    docsUrl: buildDocsUrl("MERCHANT_AUTH_ERROR"),
  },
  34: {
    code: "INVALID_SPLIT_SUM",
    contractCode: 34,
    message: "The split amounts do not sum to the payment total.",
    hint: "Ensure all split amounts sum exactly to the payment amount before submitting.",
    docsUrl: buildDocsUrl("INVALID_SPLIT_SUM"),
  },
  35: {
    code: "MISSING_RECEIPT_HASH",
    contractCode: 35,
    message: "The receipt hash is missing.",
    hint: "Attach a valid receipt hash to the payment before attempting settlement.",
    docsUrl: buildDocsUrl("MISSING_RECEIPT_HASH"),
  },
  36: {
    code: "REFUND_EXPIRED",
    contractCode: 36,
    message: "The refund window has expired.",
    hint: "Create a new refund request within the allowed refund window.",
    docsUrl: buildDocsUrl("REFUND_EXPIRED"),
  },
  37: {
    code: "ALREADY_VOTED",
    contractCode: 37,
    message: "This address has already voted on the dispute.",
    hint: "Use a different authorized voter address or wait for the dispute to be resolved.",
    docsUrl: buildDocsUrl("ALREADY_VOTED"),
  },
};

export class FluxapayError extends Error {
  readonly code: number;
  readonly contractErrorName: string;
  readonly cause?: unknown;
  readonly locale: string;
  readonly errorCode: string;
  readonly contractCode: number;
  readonly hint: string;
  readonly docsUrl: string;

  constructor(
    code: number,
    contractErrorName: string,
    message?: string,
    cause?: unknown,
    locale = "en",
  ) {
    const meta = ERROR_MAP[code];
    const resolvedMessage = message ?? meta?.message ?? contractErrorName;
    super(resolvedMessage);
    this.name = `${contractErrorName}Error`;
    this.code = code;
    this.contractErrorName = contractErrorName;
    this.cause = cause;
    this.locale = locale;
    this.errorCode = meta?.code ?? contractErrorName.toUpperCase();
    this.contractCode = code;
    this.hint = meta?.hint ?? "Inspect the transaction result and retry after correcting the inputs.";
    this.docsUrl = meta?.docsUrl ?? buildDocsUrl(this.errorCode);
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
