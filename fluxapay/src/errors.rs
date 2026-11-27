//! Issue #1000: Centralized contract error registry.
//!
//! Every contract module previously defined its own `Error` enum with
//! overlapping numeric codes (e.g. `Unauthorized = 1` in both
//! `payment_processor` and `refund_manager`). When a cross-contract call
//! failed, the SDK could not tell which module produced a given code.
//!
//! `GlobalError` assigns a unique, non-overlapping code to every error
//! across all modules. Code ranges are reserved per module:
//!
//! | Range   | Module            |
//! |---------|-------------------|
//! | 1xx     | payment           |
//! | 2xx     | refund            |
//! | 3xx     | merchant          |
//! | 4xx     | FX                |
//! | 5xx     | dispute           |
//! | 6xx     | stream            |
//!
//! This is a pure renumbering: no behaviour changes.

use soroban_sdk::contracterror;

/// Centralized error enum with globally unique codes.
///
/// Ranges: 1xx=payment, 2xx=refund, 3xx=merchant, 4xx=FX, 5xx=dispute, 6xx=stream.
#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum GlobalError {
    // ---- Payment errors: 1xx ----
    PaymentUnauthorized = 101,
    PaymentNotFound = 102,
    PaymentAlreadyExists = 103,
    PaymentExpired = 104,
    PaymentAlreadyConfirmed = 105,
    PaymentAmountMismatch = 106,
    PaymentInvalidStatus = 107,
    PaymentInvalidAmount = 108,
    PaymentInvalidCurrency = 109,
    PaymentRateLimited = 110,
    PaymentIdempotencyConflict = 111,
    PaymentTipExceedsMax = 112,
    PaymentInvalidMetadata = 113,
    PaymentInvalidMemo = 114,
    PaymentTierCapExceeded = 115,
    PaymentSwapSlippageExceeded = 116,
    PaymentSwapVolumeExceeded = 117,

    // ---- Refund errors: 2xx ----
    RefundUnauthorized = 201,
    RefundNotFound = 202,
    RefundAlreadyExists = 203,
    RefundAlreadyProcessed = 204,
    RefundExpired = 205,
    RefundCooldownActive = 206,
    RefundInvalidAmount = 207,
    RefundInvalidStatus = 208,
    RefundIdempotencyConflict = 209,
    RefundExceedsPaymentAmount = 210,

    // ---- Merchant errors: 3xx ----
    MerchantUnauthorized = 301,
    MerchantNotFound = 302,
    MerchantAlreadyRegistered = 303,
    MerchantNotVerified = 304,
    MerchantInvalidKycTier = 305,
    MerchantInvalidFeeConfig = 306,
    MerchantSuspended = 307,
    MerchantInvalidMetadata = 308,

    // ---- FX errors: 4xx ----
    FxUnauthorized = 401,
    FxRateNotFound = 402,
    FxRateStale = 403,
    FxInvalidRate = 404,
    FxOracleUnavailable = 405,
    FxPriceImpactExceeded = 406,
    FxInvalidPair = 407,

    // ---- Dispute errors: 5xx ----
    DisputeUnauthorized = 501,
    DisputeNotFound = 502,
    DisputeAlreadyExists = 503,
    DisputeAlreadyResolved = 504,
    DisputeDeadlinePassed = 505,
    DisputeInvalidBond = 506,
    DisputeInvalidStatus = 507,
    DisputeEvidenceTooLarge = 508,

    // ---- Stream errors: 6xx ----
    StreamUnauthorized = 601,
    StreamNotFound = 602,
    StreamAlreadyCancelled = 603,
    StreamAlreadyCompleted = 604,
    StreamInvalidAmount = 605,
    StreamInvalidDuration = 606,
    StreamInvalidPayees = 607,
    StreamInsufficientBalance = 608,
}

impl GlobalError {
    /// Returns the module range prefix for this error (1=payment, 2=refund,
    /// 3=merchant, 4=FX, 5=dispute, 6=stream).
    pub fn module_range(self) -> u32 {
        (self as u32) / 100
    }
}
