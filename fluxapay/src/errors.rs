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

/// Errors that can occur during Merkle batch distribution operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MerkleError {
    /// The distribution could not be found.
    NotFound,
    /// The provided inclusion proof is invalid or does not match the committed root.
    InvalidProof,
    /// The recipient has already claimed their allocation.
    AlreadyClaimed,
    /// The distribution has expired and cannot be claimed.
    Expired,
    /// The distribution has been defunded and cannot be claimed.
    Defunded,
    /// The distribution has not yet expired and cannot be defunded.
    NotExpired,
    /// The provided longitude or index is out of bounds for the distribution.
    InvalidIndex,
    /// The provided amount is invalid (e.g. zero or negative).
    InvalidAmount,
    /// The distribution has already been created for this identifier.
    AlreadyExists,
    /// A generic storage or persistence failure.
    StorageError(String),
}

impl fmt::Display for MerkleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MerkleError::NotFound => write!(f, "merkle distribution not found"),
            MerkleError::InvalidProof => write!(f, "invalid merkle inclusion proof"),
            MerkleError::AlreadyClaimed => write!(f, "allocation already claimed"),
            MerkleError::Expired => write!(f, "merkle distribution has expired"),
            MerkleError::Defunded => write!(f, "merkle distribution has been defunded"),
            MerkleError::NotExpired => write!(f, "merkle distribution has not expired"),
            MerkleError::InvalidIndex => write!(f, "invalid merkle leaf index"),
            MerkleError::InvalidAmount => write!(f, "invalid distribution amount"),
            MerkleError::AlreadyExists => write!(f, "merkle distribution already exists"),
            MerkleError::StorageError(msg) => write!(f, "storage error: {msg}"),
        }
    }
}

impl std::error::Error for MerkleError {}
