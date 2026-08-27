# FluxaPay On-Chain Event Catalog

All events use a **2-tuple topic** `(namespace: Symbol, action: Symbol)` and are emitted via
`env.events().publish((namespace, action), payload)`.

---

## PAYMENT

### PAYMENT / CREATED

Emitted by `create_payment`, `create_payments_batch`, and internal subscription billing.

| Field | Type | Description |
|-------|------|-------------|
| `payment_id` | `String` | Unique payment identifier |
| `merchant_id` | `Address` | Merchant receiving the payment |
| `amount` | `i128` | Payment amount (in token minor units) |
| `metadata` | `Option<Map<String,String>>` | Caller-supplied key/value metadata (max 20 keys, 256 chars/value) |

**Example payload:**
```
("pay_abc123", GA…MERCHANT, 1000000, Some({"order_id": "ORD-9"}))
```

---

### PAYMENT / CONFIRMED

Emitted by `verify_payment` when a payment deposit is confirmed.

| Field | Type | Description |
|-------|------|-------------|
| `payment_id` | `String` | Payment identifier |
| `merchant_id` | `Address` | Merchant |
| `amount` | `i128` | Confirmed amount |

---

### PAYMENT / SETTLED

Emitted by `settle_payment` when funds are released to the merchant.

| Field | Type | Description |
|-------|------|-------------|
| `payment_id` | `String` | Payment identifier |
| `merchant_id` | `Address` | Merchant |
| `net_amount` | `i128` | Amount after fees |

---

### PAYMENT / EXPIRED

Emitted by `expire_payment`.

| Field | Type | Description |
|-------|------|-------------|
| `payment_id` | `String` | Payment identifier |

---

## REFUND

### REFUND / REQUESTED

Emitted when a refund is initiated.

| Field | Type | Description |
|-------|------|-------------|
| `refund_id` | `String` | Refund identifier |
| `payment_id` | `String` | Original payment |
| `amount` | `i128` | Requested refund amount |
| `requester` | `Address` | Who requested the refund |

---

### REFUND / PROCESSED

Emitted when a refund is executed.

| Field | Type | Description |
|-------|------|-------------|
| `refund_id` | `String` | Refund identifier |
| `payment_id` | `String` | Original payment |
| `amount` | `i128` | Refunded amount |

---

### REFUND / REJECTED

Emitted when a refund is rejected.

| Field | Type | Description |
|-------|------|-------------|
| `refund_id` | `String` | Refund identifier |
| `payment_id` | `String` | Original payment |

---

## DISPUTE

### DISPUTE / CREATED

| Field | Type | Description |
|-------|------|-------------|
| `payment_id` | `String` | Disputed payment |
| `dispute_id` | `String` | Dispute identifier |
| `amount` | `i128` | Disputed amount |

---

### DISPUTE / REVIEWED

| Field | Type | Description |
|-------|------|-------------|
| `payment_id` | `String` | Payment |
| `dispute_id` | `String` | Dispute |

---

### DISPUTE / RESOLVED

| Field | Type | Description |
|-------|------|-------------|
| `payment_id` | `String` | Payment |
| `dispute_id` | `String` | Dispute |
| `ruling` | `Symbol` | Outcome (e.g. `MERCHANT_WINS`, `BUYER_WINS`) |

---

### DISPUTE / REJECTED

| Field | Type | Description |
|-------|------|-------------|
| `payment_id` | `String` | Payment |
| `dispute_id` | `String` | Dispute |

---

### DISPUTE / ESCALATED

| Field | Type | Description |
|-------|------|-------------|
| `payment_id` | `String` | Payment |
| `dispute_id` | `String` | Dispute |
| `amount` | `i128` | Disputed amount |

---

### DISPUTE / BOND_RETURNED

Issue #677. Emitted when a dispute bond is released back to its owner —
either the disputer (when `resolve_dispute_with_refund` rules in their
favor) or the merchant (when `reject_dispute` rules in their favor).

| Field | Type | Description |
|-------|------|-------------|
| `dispute_id` | `String` | Dispute |
| `recipient` | `Address` | Address the bond was returned to |
| `amount` | `i128` | Bond amount returned |

---

### DISPUTE / BOND_FORFEITED

Issue #677. Emitted when a dispute bond is forfeited to the treasury /
fee collector. Currently emitted from `reject_dispute` for the disputer's
bond.

| Field | Type | Description |
|-------|------|-------------|
| `dispute_id` | `String` | Dispute |
| `recipient` | `Address` | Treasury / collector address the bond was forfeited to |
| `amount` | `i128` | Bond amount forfeited |

---

## MERCHANT

### MERCHANT / REGISTERED

Emitted by `register_merchant`.

| Field | Type | Description |
|-------|------|-------------|
| `merchant_id` | `Address` | New merchant address |
| `business_name` | `String` | Display name |

---

### MERCHANT / UPDATED

| Field | Type | Description |
|-------|------|-------------|
| `merchant_id` | `Address` | Merchant address |

---

### MERCHANT / VERIFIED

| Field | Type | Description |
|-------|------|-------------|
| `merchant_id` | `Address` | Merchant address |

---

### MERCHANT / SUSPENDED

| Field | Type | Description |
|-------|------|-------------|
| `merchant_id` | `Address` | Merchant address |

---

### MERCHANT / REINSTATED

| Field | Type | Description |
|-------|------|-------------|
| `merchant_id` | `Address` | Merchant address |

---

## LINK

### LINK / CREATED

Emitted by `create_link`.

| Field | Type | Description |
|-------|------|-------------|
| `link_id` | `String` | Link identifier |
| `merchant_id` | `Address` | Owner |

---

### LINK / USED

| Field | Type | Description |
|-------|------|-------------|
| `link_id` | `String` | Link identifier |
| `payer` | `Address` | Who used the link |
| `amount` | `i128` | Amount paid |
| `payment_id` | `String` | Generated payment ID |

---

### LINK / DEACTIVATED

| Field | Type | Description |
|-------|------|-------------|
| `link_id` | `String` | Link identifier |

---

## SUBSCRIPTION

### SUBSCRIPTION / CREATED

| Field | Type | Description |
|-------|------|-------------|
| `subscription_id` | `String` | Subscription identifier |
| `payer` | `Address` | Subscriber |
| `merchant_id` | `Address` | Merchant |
| `amount` | `i128` | Per-cycle amount |

---

### SUBSCRIPTION / CANCELLED

| Field | Type | Description |
|-------|------|-------------|
| `subscription_id` | `String` | Subscription identifier |
| `payer` | `Address` | Subscriber |

---

### SUBSCRIPTION / EXPIRED

| Field | Type | Description |
|-------|------|-------------|
| `subscription_id` | `String` | Subscription identifier |
| `payer` | `Address` | Subscriber |

---

## STREAM

All stream events include `stream_id` as the first payload field (moved from topic to payload during the #284 normalization).

### STREAM / CREATED

| Field | Type | Description |
|-------|------|-------------|
| `stream_id` | `String` | Stream identifier |
| `sender` | `Address` | Fund source |
| `receiver` | `Address` | Beneficiary |
| `deposit` | `i128` | Initial deposit |

---

### STREAM / TOPPED_UP

| Field | Type | Description |
|-------|------|-------------|
| `stream_id` | `String` | Stream identifier |
| `sender` | `Address` | Who topped up |
| `amount` | `i128` | Added amount |

---

### STREAM / WITHDRAWN

| Field | Type | Description |
|-------|------|-------------|
| `stream_id` | `String` | Stream identifier |
| `receiver` | `Address` | Recipient |
| `destination` | `Address` | Destination wallet |
| `amount` | `i128` | Amount withdrawn |
| `remaining` | `i128` | Remaining deposit |

---

### STREAM / CANCELLED

| Field | Type | Description |
|-------|------|-------------|
| `stream_id` | `String` | Stream identifier |
| `sender` | `Address` | Canceller |
| `accrued` | `i128` | Amount accrued to receiver |
| `refund` | `i128` | Amount refunded to sender |

---

### STREAM / PAUSED

| Field | Type | Description |
|-------|------|-------------|
| `stream_id` | `String` | Stream identifier |
| `sender` | `Address` | Who paused |

---

### STREAM / RESUMED

| Field | Type | Description |
|-------|------|-------------|
| `stream_id` | `String` | Stream identifier |
| `sender` | `Address` | Who resumed |

---

### STREAM / RATE_UPDATED

| Field | Type | Description |
|-------|------|-------------|
| `stream_id` | `String` | Stream identifier |
| `sender` | `Address` | Who updated |
| `old_rate` | `i128` | Previous rate per second |
| `new_rate` | `i128` | New rate per second |
| `surplus` | `i128` | Refunded surplus deposit |

---

### STREAM / RATE_DECREASED

| Field | Type | Description |
|-------|------|-------------|
| `stream_id` | `String` | Stream identifier |
| `sender` | `Address` | Who decreased |
| `old_rate` | `i128` | Previous rate per second |
| `new_rate` | `i128` | New rate per second |
| `surplus` | `i128` | Refunded surplus |

---

### STREAM / MILESTONE_APPROVED

| Field | Type | Description |
|-------|------|-------------|
| `stream_id` | `String` | Stream identifier |
| `sender` | `Address` | Approver |

---

### STREAM / DESTINATION_SET

| Field | Type | Description |
|-------|------|-------------|
| `stream_id` | `String` | Stream identifier |
| `recipient` | `Address` | Receiver |
| `destination` | `Address` | New destination |

---

### STREAM / CLOSED

| Field | Type | Description |
|-------|------|-------------|
| `stream_id` | `String` | Stream identifier |
| `sender` | `Address` | Who closed |
| `receiver` | `Address` | Beneficiary |
| `residual` | `i128` | Final residual amount returned |

---

## RATE (FX Oracle)

### RATE / UPDATED

Emitted by `update_rate`.

| Field | Type | Description |
|-------|------|-------------|
| `pair` | `Symbol` | Trading pair (e.g. `XLMUSDC`) |
| `rate` | `i128` | New rate (scaled) |
| `timestamp` | `u64` | Ledger timestamp |

---

## ACCESS_CONTROL

### ACCESS_CONTROL / ROLE_GRANTED

| Field | Type | Description |
|-------|------|-------------|
| `role` | `Symbol` | Role name |
| `account` | `Address` | Account granted the role |

---

### ACCESS_CONTROL / ROLE_REVOKED

| Field | Type | Description |
|-------|------|-------------|
| `role` | `Symbol` | Role name |
| `account` | `Address` | Account whose role was revoked |

---

## FEE_SPLIT

### FEE_SPLIT / UPDATED

| Field | Type | Description |
|-------|------|-------------|
| `flat_fee` | `i128` | New flat fee |
| `bps` | `u32` | New basis-point fee |

---

## TREASURY

### TREASURY / WITHDRAWN

| Field | Type | Description |
|-------|------|-------------|
| `admin` | `Address` | Admin who withdrew |
| `amount` | `i128` | Withdrawn amount |

---

## CONTRACT

### CONTRACT / UPGRADED

| Field | Type | Description |
|-------|------|-------------|
| `old_version` | `String` | Previous version string |
| `new_version` | `String` | New version string |

---

## Topic Format Reference

Every event uses a **2-tuple** topic:

```rust
env.events().publish(
    (Symbol::new(&env, "NAMESPACE"), Symbol::new(&env, "ACTION")),
    payload,
)
```

This ensures consistent indexing and filtering by off-chain listeners via the Stellar Horizon API `?topic[]=...` filter parameter.
