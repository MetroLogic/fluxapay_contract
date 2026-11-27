//! Payment status state machine for enforcing valid transitions.
//!
//! Valid transitions for PaymentStatus:
//! - Pending → Confirmed (payment verified/confirmed)
//! - Pending → Expired (payment expires without confirmation)
//! - Pending → Failed (payment verification fails)
//! - Pending → PartiallyPaid (amount received is less than required)
//! - Pending → Overpaid (amount received is more than required)
//! - PartiallyPaid → PartiallyPaid (additional partial payment received)
//! - PartiallyPaid → Confirmed (remaining balance paid in full)
//! - PartiallyPaid → Overpaid (cumulative payment exceeds required amount)
//! - PartiallyPaid → Expired (payment expires before being fully paid)
//! - PartiallyPaid → Failed (payment verification fails)
//! - Confirmed → Settled (payment is settled)
//! - Confirmed → Disputed (dispute is created on confirmed payment)
//! - Disputed → Settled (dispute is resolved with settlement)
//!
//! ## Transition table
//!
//! | From          | To            |
//! |---------------|---------------|
//! | Pending       | Confirmed     |
//! | Pending       | Expired       |
//! | Pending       | Failed        |
//! | Pending       | PartiallyPaid |
//! | Pending       | Overpaid      |
//! | PartiallyPaid | PartiallyPaid |
//! | PartiallyPaid | Confirmed     |
//! | PartiallyPaid | Overpaid      |
//! | PartiallyPaid | Expired       |
//! | PartiallyPaid | Failed        |
//! | Confirmed     | Settled       |
//! | Confirmed     | Disputed      |
//! | Disputed      | Settled       |
//!
//! Any pair not listed above is invalid.

use crate::{Error, PaymentStatus};

/// Every `PaymentStatus` variant, used to exhaustively enumerate transition pairs.
///
/// The `all_statuses_are_listed` test below forces this list to stay in sync
/// with the enum: adding a variant without updating this array (and the
/// transition rules) fails the test.
pub(crate) const ALL_STATUSES: [PaymentStatus; 8] = [
    PaymentStatus::Pending,
    PaymentStatus::PartiallyPaid,
    PaymentStatus::Confirmed,
    PaymentStatus::Overpaid,
    PaymentStatus::Expired,
    PaymentStatus::Failed,
    PaymentStatus::Settled,
    PaymentStatus::Disputed,
];

/// Returns `true` if the transition from `from` to `to` is allowed by the
/// payment state machine. This is the single source of truth for valid
/// transitions; adding a new status only requires updating this match.
fn is_valid_transition(from: &PaymentStatus, to: &PaymentStatus) -> bool {
    matches!(
        (from, to),
        // From Pending
        (PaymentStatus::Pending, PaymentStatus::Confirmed)
            | (PaymentStatus::Pending, PaymentStatus::Expired)
            | (PaymentStatus::Pending, PaymentStatus::Failed)
            | (PaymentStatus::Pending, PaymentStatus::PartiallyPaid)
            | (PaymentStatus::Pending, PaymentStatus::Overpaid)
            // From PartiallyPaid (Issue #767)
            | (PaymentStatus::PartiallyPaid, PaymentStatus::PartiallyPaid)
            | (PaymentStatus::PartiallyPaid, PaymentStatus::Confirmed)
            | (PaymentStatus::PartiallyPaid, PaymentStatus::Overpaid)
            | (PaymentStatus::PartiallyPaid, PaymentStatus::Expired)
            | (PaymentStatus::PartiallyPaid, PaymentStatus::Failed)
            // From Confirmed
            | (PaymentStatus::Confirmed, PaymentStatus::Settled)
            | (PaymentStatus::Confirmed, PaymentStatus::Disputed)
            // From Disputed
            | (PaymentStatus::Disputed, PaymentStatus::Settled)
    )
}

/// Validates a payment status transition without performing it.
///
/// Returns `Ok(())` if the transition from `from` to `to` is valid, or an
/// `InvalidStatusTransition` error otherwise. Callers that need the resulting
/// status should use [`transition_status`].
pub fn validate_transition(from: PaymentStatus, to: PaymentStatus) -> Result<(), Error> {
    if is_valid_transition(&from, &to) {
        Ok(())
    } else {
        Err(Error::InvalidStatusTransition)
    }
}

/// Validates and transitions a payment between statuses.
/// Returns the new status or an InvalidStatusTransition error if the transition is invalid.
pub fn transition_status(
    current: &PaymentStatus,
    next: PaymentStatus,
) -> Result<PaymentStatus, Error> {
    validate_transition(current.clone(), next.clone())?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_pending_to_confirmed() {
        let result = transition_status(&PaymentStatus::Pending, PaymentStatus::Confirmed);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PaymentStatus::Confirmed);
    }

    #[test]
    fn test_valid_pending_to_expired() {
        let result = transition_status(&PaymentStatus::Pending, PaymentStatus::Expired);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PaymentStatus::Expired);
    }

    #[test]
    fn test_valid_pending_to_failed() {
        let result = transition_status(&PaymentStatus::Pending, PaymentStatus::Failed);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PaymentStatus::Failed);
    }

    #[test]
    fn test_valid_pending_to_partially_paid() {
        let result = transition_status(&PaymentStatus::Pending, PaymentStatus::PartiallyPaid);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PaymentStatus::PartiallyPaid);
    }

    #[test]
    fn test_valid_pending_to_overpaid() {
        let result = transition_status(&PaymentStatus::Pending, PaymentStatus::Overpaid);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PaymentStatus::Overpaid);
    }

    #[test]
    fn test_valid_partially_paid_self_transition() {
        let result = transition_status(&PaymentStatus::PartiallyPaid, PaymentStatus::PartiallyPaid);
        assert_eq!(result.unwrap(), PaymentStatus::PartiallyPaid);
    }

    #[test]
    fn test_valid_partially_paid_to_confirmed() {
        let result = transition_status(&PaymentStatus::PartiallyPaid, PaymentStatus::Confirmed);
        assert_eq!(result.unwrap(), PaymentStatus::Confirmed);
    }

    #[test]
    fn test_valid_partially_paid_to_overpaid() {
        let result = transition_status(&PaymentStatus::PartiallyPaid, PaymentStatus::Overpaid);
        assert_eq!(result.unwrap(), PaymentStatus::Overpaid);
    }

    #[test]
    fn test_valid_partially_paid_to_expired() {
        let result = transition_status(&PaymentStatus::PartiallyPaid, PaymentStatus::Expired);
        assert_eq!(result.unwrap(), PaymentStatus::Expired);
    }

    #[test]
    fn test_valid_partially_paid_to_failed() {
        let result = transition_status(&PaymentStatus::PartiallyPaid, PaymentStatus::Failed);
        assert_eq!(result.unwrap(), PaymentStatus::Failed);
    }

    #[test]
    fn test_valid_confirmed_to_settled() {
        let result = transition_status(&PaymentStatus::Confirmed, PaymentStatus::Settled);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PaymentStatus::Settled);
    }

    #[test]
    fn test_valid_confirmed_to_disputed() {
        let result = transition_status(&PaymentStatus::Confirmed, PaymentStatus::Disputed);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PaymentStatus::Disputed);
    }

    #[test]
    fn test_valid_disputed_to_settled() {
        let result = transition_status(&PaymentStatus::Disputed, PaymentStatus::Settled);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PaymentStatus::Settled);
    }

    #[test]
    fn test_invalid_confirmed_to_pending() {
        let result = transition_status(&PaymentStatus::Confirmed, PaymentStatus::Pending);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_settled_to_confirmed() {
        let result = transition_status(&PaymentStatus::Settled, PaymentStatus::Confirmed);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_pending_to_settled() {
        let result = transition_status(&PaymentStatus::Pending, PaymentStatus::Settled);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_expired_to_confirmed() {
        let result = transition_status(&PaymentStatus::Expired, PaymentStatus::Confirmed);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_failed_to_confirmed() {
        let result = transition_status(&PaymentStatus::Failed, PaymentStatus::Confirmed);
        assert!(result.is_err());
    }

    #[test]
    fn test_same_status_invalid() {
        let result = transition_status(&PaymentStatus::Confirmed, PaymentStatus::Confirmed);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_transition_ok() {
        assert!(validate_transition(PaymentStatus::Pending, PaymentStatus::Confirmed).is_ok());
    }

    #[test]
    fn test_validate_transition_err() {
        assert!(validate_transition(PaymentStatus::Settled, PaymentStatus::Pending).is_err());
    }

    /// The exhaustive matrix: for every (from, to) pair, the result of
    /// `validate_transition` must match the expected table below. Any pair not
    /// explicitly listed as valid is asserted to be rejected.
    #[test]
    fn test_full_transition_matrix() {
        let valid: &[(PaymentStatus, PaymentStatus)] = &[
            (PaymentStatus::Pending, PaymentStatus::Confirmed),
            (PaymentStatus::Pending, PaymentStatus::Expired),
            (PaymentStatus::Pending, PaymentStatus::Failed),
            (PaymentStatus::Pending, PaymentStatus::PartiallyPaid),
            (PaymentStatus::Pending, PaymentStatus::Overpaid),
            (PaymentStatus::PartiallyPaid, PaymentStatus::PartiallyPaid),
            (PaymentStatus::PartiallyPaid, PaymentStatus::Confirmed),
            (PaymentStatus::PartiallyPaid, PaymentStatus::Overpaid),
            (PaymentStatus::PartiallyPaid, PaymentStatus::Expired),
            (PaymentStatus::PartiallyPaid, PaymentStatus::Failed),
            (PaymentStatus::Confirmed, PaymentStatus::Settled),
            (PaymentStatus::Confirmed, PaymentStatus::Disputed),
            (PaymentStatus::Disputed, PaymentStatus::Settled),
        ];

        for from in ALL_STATUSES.iter() {
            for to in ALL_STATUSES.iter() {
                let expected = valid.iter().any(|(f, t)| f == from && t == to);
                let actual = validate_transition(from.clone(), to.clone()).is_ok();
                assert_eq!(
                    actual, expected,
                    "transition {:?} -> {:?}: expected valid={}, got valid={}",
                    from, to, expected, actual
                );
            }
        }
    }

    /// Guards against a new `PaymentStatus` variant being added without being
    /// included in `ALL_STATUSES`, which would silently skip it in the matrix.
    #[test]
    fn all_statuses_are_listed() {
        fn variant_index(status: &PaymentStatus) -> usize {
            // Exhaustive match: adding a variant breaks compilation here until
            // it is handled, and the count below must then be updated.
            match status {
                PaymentStatus::Pending => 0,
                PaymentStatus::PartiallyPaid => 1,
                PaymentStatus::Confirmed => 2,
                PaymentStatus::Overpaid => 3,
                PaymentStatus::Expired => 4,
                PaymentStatus::Failed => 5,
                PaymentStatus::Settled => 6,
                PaymentStatus::Disputed => 7,
            }
        }
        let mut seen = [false; 8];
        for status in ALL_STATUSES.iter() {
            seen[variant_index(status)] = true;
        }
        assert!(
            seen.iter().all(|&s| s),
            "ALL_STATUSES is missing a PaymentStatus variant; update the matrix"
        );
    }
}
