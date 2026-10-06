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

/// Errors that can occur during merchant rolling reserve operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReserveError {
    /// The merchant was not found in the registry.
    MerchantNotFound,
    /// The caller is not authorized to perform the reserve operation.
    Unauthorized,
    /// The requested reserve amount exceeds the merchant's locked reserve balance.
    InsufficientReserve,
    /// The provided reserve policy parameters are invalid (e.g. bps out of range).
    InvalidPolicy,
    /// The provided amount is invalid (e.g. negative or zero where not allowed).
    InvalidAmount,
    /// A generic storage or persistence failure.
    StorageError(String),
}

impl fmt::Display for ReserveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReserveError::MerchantNotFound => write!(f, "merchant not found"),
            ReserveError::Unauthorized => write!(f, "unauthorized reserve operation"),
            ReserveError::InsufficientReserve => write!(f, "insufficient locked reserve balance"),
            ReserveError::InvalidPolicy => write!(f, "invalid rolling reserve policy"),
            ReserveError::InvalidAmount => write!(f, "invalid reserve amount"),
            ReserveError::StorageError(msg) => write!(f, "storage error: {msg}"),
        }
    }
}

impl std::error::Error for ReserveError {}

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

impl From<ReserveError> for PaymentLinkError {
    fn from(err: ReserveError) -> Self {
        PaymentLinkError::StorageError(err.to_string())
    }
}
