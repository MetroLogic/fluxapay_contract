use std::fmt;

/// Errors that can occur during subscription operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscriptionError {
    /// The subscription could not be found.
    NotFound,
    /// The caller is not authorized to perform this action.
    Unauthorized,
    /// The subscription is not active and cannot be charged.
    NotActive,
    /// The billing interval has not yet elapsed.
    IntervalNotElapsed,
    /// The subscription has reached its maximum number of billing cycles.
    MaxCyclesReached,
    /// The subscription is paused and cannot be charged.
    Paused,
    /// The subscription is not paused.
    NotPaused,
    /// The maximum number of dunning retries has been exceeded.
    MaxDunningRetriesExceeded,
    /// The retry backoff window has not yet elapsed.
    RetryBackoffNotElapsed,
    /// A generic storage or persistence failure.
    StorageError(String),
}

impl fmt::Display for SubscriptionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubscriptionError::NotFound => write!(f, "subscription not found"),
            SubscriptionError::Unauthorized => write!(f, "unauthorized: caller is not the subscriber"),
            SubscriptionError::NotActive => write!(f, "subscription is not active"),
            SubscriptionError::IntervalNotElapsed => write!(f, "billing interval has not elapsed"),
            SubscriptionError::MaxCyclesReached => write!(f, "subscription has reached max cycles"),
            SubscriptionError::Paused => write!(f, "subscription is paused"),
            SubscriptionError::NotPaused => write!(f, "subscription is not paused"),
            SubscriptionError::MaxDunningRetriesExceeded => write!(f, "max dunning retries exceeded"),
            SubscriptionError::RetryBackoffNotElapsed => write!(f, "retry backoff window has not elapsed"),
            SubscriptionError::StorageError(msg) => write!(f, "storage error: {msg}"),
        }
    }
}

impl std::error::Error for SubscriptionError {}

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
