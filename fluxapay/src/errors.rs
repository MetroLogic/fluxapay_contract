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
        }
    }
}

impl std::error::Error for PaymentLinkError {}

/// Errors that can occur during stream and vesting operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamError {
    /// The stream could not be found.
    StreamNotFound,
    /// The stream has already been cancelled/revoked.
    StreamCancelled,
    /// The caller is not authorized to perform the operation.
    Unauthorized,
    /// The cliff timestamp has not yet been reached.
    CliffNotReached,
    /// The supplied time range is invalid (e.g. end before start).
    InvalidTimeRange,
    /// The cliff timestamp falls outside the [start, end] window.
    InvalidCliff,
    /// The cliff amount exceeds the total vesting amount.
    InvalidCliffAmount,
    /// The stream is not revocable and cannot be cancelled.
    NotRevocable,
    /// There are no vested funds available to withdraw.
    NothingToWithdraw,
    /// A generic storage or persistence failure.
    StorageError(String),
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<_>) -> fmt::Result {
        match self {
            StreamError::StreamNotFound => write!(f, "stream not found"),
            StreamError::StreamCancelled => write!(f, "stream has been cancelled"),
            StreamError::Unauthorized => write!(f, "unauthorized operation on stream"),
            StreamError::CliffNotReached => write!(f, "cliff time not reached"),
            StreamError::InvalidTimeRange => write!(f, "invalid time range"),
            StreamError::InvalidCliff => write!(f, "invalid cliff timestamp"),
            StreamError::InvalidCliffAmount => write!(f, "invalid cliff amount"),
            StreamError::NotRevocable => write!(f, "stream is not revocable"),
            StreamError::NothingToWithdraw => write!(f, "nothing to withdraw"),
            StreamError::StorageError(msg) => write!(f, "storage error: {msg}"),
        }
    }
}

impl std::error::Error for StreamError {}
