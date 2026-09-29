use std::fmt;

/// Errors that can occur during payment link operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaymentLinkError {
    /// The provided `base_url` failed validation (scheme, length, or format).
    InvalidBaseUrl,
    /// The payment link could not be found.
    NotFound,
    /// The payment link has expired.
    Expired,
    /// A generic storage or persistence failure.
    StorageError(String),
    /// The requested swap path is invalid (empty, too long, or contains duplicate hops).
    InvalidSwapPath,
    /// A required AMM pool for the swap path could not be found.
    PoolNotFound,
    /// The computed input amount exceeds the caller-supplied `max_amount_in`.
    SlippageExceeded,
    /// The swap deadline has already passed.
    DeadlineExpired,
    /// The AMM pool returned an invalid or zero reserve.
    InvalidPoolReserve,
    /// The reverse quote produced a non-positive or overflowing amount.
    InvalidAmount,
    /// The customer did not supply enough input tokens to cover the required amount.
    InsufficientInputAmount,
}

impl fmt::Display for PaymentLinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PaymentLinkError::InvalidBaseUrl => {
                write!(f, "invalid base_url: must be a valid https:// URL of at most 256 characters")
            }
            PaymentLinkError::NotFound => write!(f, "payment link not found"),
            PaymentLinkError::Expired => write!(f, "payment link has expired"),
            PaymentLinkError::StorageError(msg) => write!(f, "storage error: {msg}"),
            PaymentLinkError::InvalidSwapPath => {
                write!(f, "invalid swap path: must be non-empty, within hop limit, and free of duplicates")
            }
            PaymentLinkError::PoolNotFound => write!(f, "required AMM pool not found for swap path"),
            PaymentLinkError::SlippageExceeded => {
                write!(f, "slippage exceeded: required input is greater than max_amount_in")
            }
            PaymentLinkError::DeadlineExpired => write!(f, "swap deadline has expired"),
            PaymentLinkError::InvalidPoolReserve => write!(f, "AMM pool returned an invalid reserve"),
            PaymentLinkError::InvalidAmount => write!(f, "swap amount is invalid or overflowed"),
            PaymentLinkError::InsufficientInputAmount => {
                write!(f, "insufficient input amount provided to settle the exact output")
            }
        }
    }
}

impl std::error::Error for PaymentLinkError {}
