# Payment Dispute Handling

This document describes the dispute handling functionality implemented in the FluxaPay smart contract system.

## Overview

The dispute handling system allows customers to raise disputes for confirmed payments and provides a structured workflow for operators to review and resolve these disputes, including automatic refund processing.

## Data Structures

### DisputeStatus
```rust
pub enum DisputeStatus {
    Open,           // Dispute has been created and awaiting review
    UnderReview,    // Dispute is being reviewed by an operator
    Resolved,       // Dispute resolved in favor of customer (refund issued)
    Rejected,       // Dispute rejected (no refund)
}
```

### Dispute
```rust
pub struct Dispute {
    pub dispute_id: String,              // Unique dispute identifier
    pub payment_id: String,              // Associated payment ID
    pub refund_id: Option<String>,       // Refund ID if dispute resolved
    pub amount: i128,                    // Disputed amount
    pub reason: String,                  // Reason for dispute
    pub evidence: String,                // Evidence provided by customer
    pub status: DisputeStatus,           // Current dispute status
    pub disputer: Address,               // Customer who raised the dispute
    pub created_at: u64,                 // Timestamp when dispute was created
    pub resolved_at: Option<u64>,        // Timestamp when dispute was resolved
    pub resolution_notes: Option<String>, // Notes from operator
}
```

## Functions

### 1. create_dispute
Creates a new payment dispute.

**Parameters:**
- `payment_id`: ID of the payment being disputed
- `amount`: Amount being disputed (must be > 0)
- `reason`: Reason for the dispute
- `evidence`: Supporting evidence for the dispute
- `disputer`: Address of the customer raising the dispute

**Returns:** `Result<String, Error>` - The dispute ID if successful

**Authorization:** Requires authentication from the disputer

**Example:**
```rust
let dispute_id = refund_client.create_dispute(
    &payment_id,
    &1000i128,
    &String::from_str(&env, "Product not received"),
    &String::from_str(&env, "Tracking shows delivery failed"),
    &customer_address,
);
```

### 2. review_dispute
Marks a dispute as under review.

**Parameters:**
- `operator`: Address of the operator reviewing the dispute
- `dispute_id`: ID of the dispute to review

**Returns:** `Result<(), Error>`

**Authorization:** Requires SETTLEMENT_OPERATOR or ORACLE role

**Example:**
```rust
refund_client.review_dispute(&operator, &dispute_id);
```

### 3. resolve_dispute_with_refund
Resolves a dispute in favor of the customer and automatically creates and processes a refund.

**Parameters:**
- `operator`: Address of the operator resolving the dispute
- `dispute_id`: ID of the dispute to resolve
- `resolution_notes`: Notes explaining the resolution

**Returns:** `Result<String, Error>` - The refund ID if successful

**Authorization:** Requires SETTLEMENT_OPERATOR or ORACLE role

**Example:**
```rust
let refund_id = refund_client.resolve_dispute_with_refund(
    &operator,
    &dispute_id,
    &String::from_str(&env, "Dispute valid, issuing full refund"),
);
```

### 4. reject_dispute
Rejects a dispute without issuing a refund.

**Parameters:**
- `operator`: Address of the operator rejecting the dispute
- `dispute_id`: ID of the dispute to reject
- `resolution_notes`: Notes explaining why the dispute was rejected

**Returns:** `Result<(), Error>`

**Authorization:** Requires SETTLEMENT_OPERATOR or ORACLE role

**Example:**
```rust
refund_client.reject_dispute(
    &operator,
    &dispute_id,
    &String::from_str(&env, "Insufficient evidence, dispute rejected"),
);
```

### 5. get_dispute
Retrieves dispute details by ID.

**Parameters:**
- `dispute_id`: ID of the dispute to retrieve

**Returns:** `Result<Dispute, Error>`

**Example:**
```rust
let dispute = refund_client.get_dispute(&dispute_id);
```

### 6. get_payment_disputes
Retrieves all disputes associated with a payment.

**Parameters:**
- `payment_id`: ID of the payment

**Returns:** `Result<Vec<Dispute>, Error>`

**Example:**
```rust
let disputes = refund_client.get_payment_disputes(&payment_id);
```

## Workflow

### Typical Dispute Resolution Flow

1. **Customer Creates Dispute**
   - Customer calls `create_dispute()` with payment details, reason, and evidence
   - Dispute status is set to `Open`
   - Dispute ID is generated and returned

2. **Operator Reviews Dispute**
   - Operator with SETTLEMENT_OPERATOR or ORACLE role calls `review_dispute()`
   - Dispute status changes to `UnderReview`
   - Operator examines the evidence and payment details

3. **Resolution**
   
   **Option A: Approve Dispute**
   - Operator calls `resolve_dispute_with_refund()`
   - System automatically creates a refund for the disputed amount
   - Refund is immediately processed
   - Dispute status changes to `Resolved`
   - Resolution notes and timestamp are recorded
   
   **Option B: Reject Dispute**
   - Operator calls `reject_dispute()`
   - Dispute status changes to `Rejected`
   - Resolution notes and timestamp are recorded
   - No refund is issued

## Error Handling

The system returns the following errors:

- `InvalidAmount` (3): Dispute amount is <= 0
- `Unauthorized` (10): Caller doesn't have required role
- `DisputeNotFound` (11): Dispute ID doesn't exist
- `DisputeAlreadyResolved` (12): Dispute has already been resolved or rejected

## Access Control

Dispute operations require specific roles:

- **create_dispute**: Requires authentication from the disputer (customer)
- **review_dispute**: Requires SETTLEMENT_OPERATOR or ORACLE role
- **resolve_dispute_with_refund**: Requires SETTLEMENT_OPERATOR or ORACLE role
- **reject_dispute**: Requires SETTLEMENT_OPERATOR or ORACLE role
- **get_dispute**: No authorization required (read-only)
- **get_payment_disputes**: No authorization required (read-only)

## Integration Notes

1. The dispute system is integrated with the existing refund mechanism
2. When a dispute is resolved with a refund, the refund is automatically processed
3. Multiple disputes can be created for the same payment
4. Dispute IDs are automatically generated in the format "dispute_1", "dispute_2", etc.
5. All dispute data is stored persistently on-chain

## Testing

Comprehensive tests are available in `src/dispute_test.rs` covering:
- Creating disputes
- Reviewing disputes
- Resolving disputes with refunds
- Rejecting disputes
- Retrieving dispute information
- Error cases (invalid amounts, unauthorized access)
