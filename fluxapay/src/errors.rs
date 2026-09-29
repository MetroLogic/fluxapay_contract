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
    /// The leaf at the given index has already been claimed.
    AlreadyClaimed,
    /// The distribution has expired and cannot be claimed.
    Expired,
    /// The distribution has not yet expired and cannot be defunded.
    NotExpired,
    /// The distribution has already been defunded.
    AlreadyDefunded,
    /// The provided leaf index is out of output range for the tree.
    InvalidIndex,
    /// The provided leaf data does not match the committed leaf hash.
    InvalidLeaf,
    /// The distribution has no remaining unclaimed balance to defund.
    NothingToDefund,
    /// A generic storage or persistence failure.
    StorageError(String),
}

impl fmt::Display for MerkleError {
    fn fmt(&self, f: &mut fmt::Formatter<_>) -> fmt::Result {
        match self {
            MerkleError::NotFound => write!(f, "merkle distribution not found"),
            MerkleError::InvalidProof => write!(f, "invalid merkle inclusion proof"),
            MerkleError::AlreadyClaimed => write!(f, "leaf has already been claimed"),
            MerkleError::Expired => write!(f, "merkle distribution has expired"),
            MerkleError::NotExpired => write!(f, "merkle distribution has not expired"),
            MerkleError::AlreadyDefunded => write!(f, "merkle distribution has already been defunded"),
            MerkleError::InvalidIndex => write!(f, "leaf index is out of range"),
            MerkleError::InvalidLeaf => write!(f, "leaf data does not match committed hash"),
            MerkleError::NothingToDefund => write!(f, "no unclaimed balance to defund"),
            MerkleError::StorageError(msg) => write!(f, "merkle storage error: {msg}"),
        }
    }
}

impl std::error::Error for MerkleError {}
