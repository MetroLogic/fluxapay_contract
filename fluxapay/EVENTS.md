# Fluxapay Contract Events

This document defines the on-chain events emitted by the Fluxapay smart contracts. Off-chain listeners (webhooks, indexers) can subscribe selectively using the standardised topic tuple.

## Topic Format

All events follow a three-element topic tuple:

```
(contract_symbol: Symbol, event_type: Symbol, entity_id: String)
```

| Field | Description |
|---|---|
| `contract_symbol` | Top-level namespace: `PAYMENT`, `REFUND`, `DISPUTE`, `MERCHANT`, `LINK` |
| `event_type` | Specific lifecycle transition (e.g. `CREATED`, `VERIFIED`) |
| `entity_id` | Primary identifier of the affected entity (payment_id, refund_id, etc.) |

The data payload always includes `merchant_id` and `amount` for payment events, enabling downstream indexing without a secondary lookup.

---

## Payment Events

Emitted by the `PaymentProcessor` contract.

### PAYMENT/CREATED
Emitted when a new payment charge is created by a verified merchant.
- **Topics**: `(PAYMENT, CREATED, payment_id)`
- **Data**: `(merchant_id: Address, amount: i128)`

### PAYMENT/VERIFIED
Emitted when a payment is confirmed on-chain (amount within tolerance).
- **Topics**: `(PAYMENT, VERIFIED, payment_id)`
- **Data**: `(merchant_id: Address, amount: i128, amount_received: i128)`

### PAYMENT/PARTIALLY_PAID
Emitted when the received amount is meaningfully below the expected amount (outside tolerance).
- **Topics**: `(PAYMENT, PARTIALLY_PAID, payment_id)`
- **Data**: `(merchant_id: Address, amount: i128, amount_received: i128)`

### PAYMENT/OVERPAID
Emitted when the received amount is meaningfully above the expected amount (outside tolerance).
- **Topics**: `(PAYMENT, OVERPAID, payment_id)`
- **Data**: `(merchant_id: Address, amount: i128, amount_received: i128)`

### PAYMENT/FAILED
Emitted when payment verification fails for an unclassified reason.
- **Topics**: `(PAYMENT, FAILED, payment_id)`
- **Data**: `(merchant_id: Address, amount: i128, amount_received: i128)`

### PAYMENT/CANCELLED
Emitted when a pending payment is cancelled by the merchant or an oracle before expiry.
- **Topics**: `(PAYMENT, CANCELLED, payment_id)`
- **Data**: `(merchant_id: Address, amount: i128)`

### PAYMENT/EXPIRED
Emitted when a pending payment is marked expired after its deadline has passed.
- **Topics**: `(PAYMENT, EXPIRED, payment_id)`
- **Data**: `(merchant_id: Address, amount: i128)`

### PAYMENT/SETTLED
Emitted when a confirmed payment is swept to the treasury by a settlement operator.
- **Topics**: `(PAYMENT, SETTLED, payment_id)`
- **Data**: `(merchant_id: Address, amount: i128)`

---

## Refund Events

Emitted by the `RefundManager` contract.

### REFUND/CREATED
Emitted when a refund request is initiated.
- **Topics**: `(REFUND, CREATED)`
- **Data**: `(payment_id: String, refund_id: String, refund_amount: i128)`

### REFUND/COMPLETED
Emitted when a refund is successfully processed and funds are transferred.
- **Topics**: `(REFUND, COMPLETED)`
- **Data**: `(payment_id: String, refund_id: String, refund_amount: i128)`

### REFUND/REJECTED
Emitted when a refund request is rejected by an operator.
- **Topics**: `(REFUND, REJECTED)`
- **Data**: `(payment_id: String, refund_id: String, refund_amount: i128)`

---

## Dispute Events

Emitted by the `RefundManager` contract.

### DISPUTE/CREATED
Emitted when a new dispute is opened for a payment.
- **Topics**: `(DISPUTE, CREATED)`
- **Data**: `(dispute_id: String, payment_id: String)`

### DISPUTE/REVIEWED
Emitted when a dispute's status is changed to under review by an operator.
- **Topics**: `(DISPUTE, REVIEWED)`
- **Data**: `(dispute_id: String, payment_id: String)`

### DISPUTE/RESOLVED
Emitted when a dispute is resolved in favour of the customer (refund issued).
- **Topics**: `(DISPUTE, RESOLVED)`
- **Data**: `(dispute_id: String, payment_id: String)`

### DISPUTE/REJECTED
Emitted when a dispute is rejected by an operator.
- **Topics**: `(DISPUTE, REJECTED)`
- **Data**: `(dispute_id: String, payment_id: String)`

---

## Merchant Events

Emitted by the `MerchantRegistry` contract.

### MERCHANT/REGISTERED
Emitted when a new merchant registers on the platform.
- **Topics**: `(MERCHANT, REGISTERED)`
- **Data**: `(merchant_id: Address, settlement_currency: String)`

### MERCHANT/VERIFIED
Emitted when a merchant's KYC status is verified by an admin.
- **Topics**: `(MERCHANT, VERIFIED)`
- **Data**: `merchant_id: Address`

### MERCHANT/UPDATED
Emitted when a merchant's profile or configuration is updated.
- **Topics**: `(MERCHANT, UPDATED)`
- **Data**: `merchant_id: Address`

### KYC/TIER_UPGRADED
Emitted when a merchant's KYC tier is automatically upgraded.
- **Topics**: `(KYC, TIER_UPGRADED)`
- **Data**: `(merchant_id: Address, old_tier: KycTier, new_tier: KycTier)`

---

## Payment Link Events

Emitted by the `PaymentLinkManager` contract.

### LINK/CREATED
Emitted when a merchant creates a new payment link.
- **Topics**: `(LINK, CREATED)`
- **Data**: `(link_id: String, merchant_id: Address)`

### LINK/USED
Emitted when a payer uses a payment link to initiate a payment. When the link has a `fiat` config, the `amount` in the data payload is the resolved USDC equivalent computed via the FX oracle.
- **Topics**: `(LINK, USED)`
- **Data**: `(link_id: String, payer: Address, amount: i128, payment_id: String)`

### LINK/DEACTIVATED
Emitted when a merchant deactivates a payment link.
- **Topics**: `(LINK, DEACTIVATED)`
- **Data**: `link_id: String`

---

## Contract Upgrade Events

Emitted by any upgradeable contract (`PaymentProcessor`, `RefundManager`, `FXOracle`, `MerchantRegistry`, `PaymentLinkManager`).

### CONTRACT/UPGRADED
Emitted when a contract's WASM is upgraded by an admin.
- **Topics**: `(CONTRACT, UPGRADED)`
- **Data**: `(old_version: String, new_version: String)`

---

## Subscription Events

Emitted by the `RefundManager` contract.

### SUBSCRIPTION/CREATED
Emitted when a new subscription is created.
- **Topics**: `(SUBSCRIPTION, CREATED)`
- **Data**: `(subscription_id: String, payer: Address, plan_id: String)`

### SUBSCRIPTION/CHARGED
Emitted when a recurring subscription payment is successfully processed.
- **Topics**: `(SUBSCRIPTION, CHARGED)`
- **Data**: `(subscription_id: String, payer: Address, merchant_id: Address, amount: i128, total_payments: u32)`

### SUBSCRIPTION/CANCELLED
Emitted when an active subscription is cancelled by the payer or merchant.
- **Topics**: `(SUBSCRIPTION, CANCELLED)`
- **Data**: `(subscription_id: String, cancelled_by: Address)`

---

## Swap Events

Emitted by the `PaymentProcessor` contract.

### SWAP/EXECUTED
Emitted when a DEX swap completes as part of `swap_and_pay`.
- **Topics**: `(SWAP, EXECUTED)`
- **Data**: `(payment_id: String, sender: Address, amount_in: i128, amount_out: i128)`

---

## Stream Events

Emitted by the `PaymentProcessor` contract.

### STREAM/CREATED
Emitted when a new payment stream is created.
- **Topics**: `(STREAM, CREATED)`
- **Data**: `(stream_id: String, sender: Address, amount: i128)`

### STREAM/WITHDRAWN
Emitted when accrued tokens are withdrawn from a stream.
- **Topics**: `(STREAM, WITHDRAWN, stream_id)`
- **Data**: `(receiver: Address, destination: Address, amount: i128, remaining_deposit: i128)`
