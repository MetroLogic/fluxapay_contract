# Dispute Resolution Guide

This guide explains how disputes work in FluxaPay from the merchant, customer, and operator point of view. It covers the on-chain lifecycle, evidence requirements, review and escalation flow, and how outcomes are settled.

For the event catalog and exact webhook mapping, see [docs/events.md](events.md) and [docs/webhooks.md](webhooks.md).

## 1) What triggers a dispute

A dispute is opened against a confirmed payment when the customer says they did not receive the item or service, the merchant failed to fulfill the order, or a settlement dispute needs to be reviewed.

The contract requires the payment to be in `Confirmed` status before a dispute can be created. A dispute cannot exceed the original payment amount and the total of active disputes plus refunds cannot exceed the original payment amount.

Typical triggers:

- Item never arrived
- Merchant failed to provide a promised service
- Unauthorized or fraudulent payment claim
- Marketplace or fulfillment failure

---

## 2) Dispute creation and required data

A customer or buyer opens a dispute by calling `create_dispute`.

### Required fields

- `payment_id` — the original confirmed payment
- `amount` — disputed amount, must be > 0 and <= payment amount
- `reason` — short explanation
- `evidence` — supporting documentation or proof
- `disputer` — the address filing the dispute
- `payout_splits` — optional marketplace payout split configuration

### Evidence format

By default, non-empty evidence must be a valid IPFS CID (`CIDv0` or `CIDv1`). This protects the system from junk strings and makes evidence auditable.

Examples:

```bash
# CIDv0
QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG

# CIDv1
bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi
```

If the environment is configured with `require_evidence_cid = false`, arbitrary strings can be used for local development or testing.

### Bond requirement

Opening a dispute requires a dispute bond on both sides:

- the disputer must lock a bond
- the merchant must also lock a bond

This prevents spam disputes and ensures both parties have skin in the game. The bond is returned to the winning side or forfeited to the treasury/collector depending on the final result.

### CLI example

```bash
stellar contract invoke \
  --id $REFUND_MANAGER_ID \
  --network testnet \
  --source $TEST_CUSTOMER_ADDRESS \
  -- create_dispute \
  --payment_id "inv_20260329_001" \
  --amount 1000000000 \
  --reason "Item not received" \
  --evidence "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG" \
  --disputer $TEST_CUSTOMER_ADDRESS
```

Expected result:

```json
"dispute_1"
```

If the payment is not `Confirmed`, or the amount exceeds the payment, the call fails with `PaymentAlreadyProcessed` or `InvalidAmount`.

---

## 3) Dispute states and status transitions

The contract defines these dispute states:

| Status | Meaning |
|--------|---------|
| `Open` | Dispute filed and waiting for operator review |
| `UnderReview` | Operator has moved the dispute into manual review |
| `Resolved` | Dispute was decided in favor of the disputer and a refund was issued |
| `Rejected` | Dispute was rejected and the merchant keeps the payment |

The state progression is:

```text
Open -> UnderReview -> Resolved
      \-> UnderReview -> Rejected
      \-> Escalated -> reviewed/resolved
```

Important: once a dispute reaches `Resolved` or `Rejected`, the contract rejects any further attempts to mutate it with `DisputeAlreadyResolved`.

---

## 4) Operator review workflow

Once a dispute is created, the operator can begin the review process.

### Review step

Only an address with the `settlement_operator` or `oracle` role may review a dispute.

```bash
stellar contract invoke \
  --id $REFUND_MANAGER_ID \
  --network testnet \
  --source $ADMIN_ADDRESS \
  -- review_dispute \
  --operator $ADMIN_ADDRESS \
  --dispute_id "dispute_1"
```

This transitions the dispute from `Open` to `UnderReview` and emits `DISPUTE/REVIEWED`.

### Resolution step

Operators can resolve a dispute with a refund:

```bash
stellar contract invoke \
  --id $REFUND_MANAGER_ID \
  --network testnet \
  --source $ADMIN_ADDRESS \
  -- resolve_dispute_with_refund \
  --operator $ADMIN_ADDRESS \
  --dispute_id "dispute_1" \
  --resolution_notes "Verified: item not delivered. Refund approved." \
  --operator_signature "base64_or_opaque_operator_signature"
```

This does three things:

1. creates a refund for the disputed amount,
2. processes the refund immediately,
3. marks the dispute as `Resolved`.

The contract also persists an operator note and emits `DISPUTE/OPERATOR_NOTE` and `DISPUTE/RESOLVED`.

### Reject step

Operators may reject a dispute if the evidence or merchant fulfillment is adequate:

```bash
stellar contract invoke \
  --id $REFUND_MANAGER_ID \
  --network testnet \
  --source $ADMIN_ADDRESS \
  -- reject_dispute \
  --operator $ADMIN_ADDRESS \
  --dispute_id "dispute_1" \
  --resolution_notes "Merchant provided shipping confirmation and proof of delivery." \
  --operator_signature "base64_or_opaque_operator_signature"
```

This marks the dispute `Rejected` and emits `DISPUTE/REJECTED`.

---

## 5) Arbitration voting

When a dispute is `UnderReview`, arbitrators may vote by role.

The contract supports role-gated arbitrator voting using `vote_dispute` with `ArbitratorVoteChoice::Approve` or `ArbitratorVoteChoice::Reject`.

### Key rules

- only addresses with the `ARBITRATOR` role may vote
- each arbitrator can vote once per dispute
- the required threshold is `ARBITRATOR_VOTING_THRESHOLD = 3`
- the first side to reach the threshold triggers automatic resolution

### CLI example

```bash
stellar contract invoke \
  --id $REFUND_MANAGER_ID \
  --network testnet \
  --source $ARBITRATOR_SECRET \
  -- vote_dispute \
  --arbitrator $ARBITRATOR_ADDRESS \
  --dispute_id "dispute_1" \
  --choice Approve
```

If enough arbitrators vote the same way, the dispute is auto-resolved and emits `DISPUTE/AUTO_RESOLVED`.

This is the simple `ARBITRATOR` flow. There are also stake-weighted voting paths for more advanced dispute resolution logic, but the role-gated `vote_dispute` flow is the merchant-facing, operator-friendly version.

---

## 6) Dispute bond and return/forfeiture rules

The dispute bond is a core anti-abuse mechanism.

### Why it exists

- stops low-quality or spam disputes
- ensures disputers have economic skin in the game
- aligns incentives for both buyer and merchant

### How it is handled

When a dispute is opened, both the disputer and merchant bonds are collected by the contract. When the dispute resolves:

- if the dispute is resolved in the disputer's favor, the disputer bond is returned to them
- if the dispute is rejected, the merchant's bond is returned to the merchant and the disputer's bond is forfeited to the treasury/collector

This behavior is emitted as `DISPUTE/BOND_RETURNED` and `DISPUTE/BOND_FORFEITED`.

---

## 7) Escalation and deadlines

Every dispute has a computed deadline based on size:

- small disputes: 3-day deadline
- larger disputes: 7-day deadline

The exact threshold is configurable by the admin, but the default behavior is built into `computed_dispute_deadline_secs`.

### Review deadline handling

An operator can set a custom `review_deadline` with `set_dispute_deadline`:

```bash
DEADLINE=$(($(date +%s) + 86400))

stellar contract invoke \
  --id $REFUND_MANAGER_ID \
  --network testnet \
  --source $ADMIN_ADDRESS \
  -- set_dispute_deadline \
  --operator $ADMIN_ADDRESS \
  --dispute_id "dispute_1" \
  --deadline $DEADLINE
```

If the deadline is exceeded and the dispute is still unresolved, the dispute becomes `escalated` and emits `DISPUTE/ESCALATED`.

Anyone may also trigger an escalation check with:

```bash
stellar contract invoke \
  --id $REFUND_MANAGER_ID \
  --network testnet \
  -- check_dispute_deadline \
  --dispute_id "dispute_1"
```

This is the operational safety net that prevents disputes from stalling indefinitely.

---

## 8) Resolution outcomes and payout routing

A dispute resolves in one of two ways:

### Resolved in favor of buyer

- dispute becomes `Resolved`
- refund is created and processed
- dispute bond is returned to the disputer
- the payment can no longer be disputed again

### Rejected

- dispute becomes `Rejected`
- merchant keeps the payment
- merchant bond is returned
- disputer bond is forfeited to the treasury/collector

### Marketplace splits

If a marketplace or multi-party payout configuration is set, the contract supports `payout_splits`. In that case, funds are routed to the configured recipients rather than a single buyer refund. Splits must sum to the dispute amount, or the resolution fails with `InvalidSplitSum`.

The event `DISPUTE/SPLIT_RESOLVED` is emitted for that payout flow.

---

## 9) Merchant score and suspension risk

FluxaPay tracks merchant dispute activity. If a merchant accumulates too many disputes relative to their payment volume, the contract may auto-suspend the merchant.

The contract checks dispute rate against payment count and auto-suspends when the ratio crosses the threshold.

Current behavior:

- merchant dispute count is tracked per merchant
- dispute rate is measured against total payment count
- if the rate exceeds 10% after enough payments are recorded, the merchant is auto-suspended
- rejected disputes are not counted against the merchant's active dispute threshold once rejected

This is important for merchants operating high-volume marketplaces: a rising dispute rate may lead to temporary suspension or operational review, even before a human review escalates the case.

---

## 10) Recommended merchant workflow

For merchants, the practical workflow is:

1. Create a payment and confirm it.
2. Store the payment ID and customer order details.
3. If a dispute is raised, collect evidence immediately.
4. Validate the evidence is a real IPFS CID if required.
5. Call `review_dispute` to move it into `UnderReview`.
6. Resolve or reject with a clear operator note and signature.
7. Monitor `DISPUTE/CREATED`, `DISPUTE/REVIEWED`, `DISPUTE/RESOLVED`, and `DISPUTE/REJECTED` in your webhook pipeline.

---

## 11) Operational checklist

Use this checklist when handling disputes in production:

- Confirm the payment exists and is `Confirmed`
- Check the disputed amount is within the original payment amount
- Validate the evidence CID is acceptable
- Set the review deadline if needed
- Watch for escalation events when the deadline is exceeded
- Keep a permanent operator note and signature record
- Reconcile the final bond return or bond forfeiture in your settlement ledger

---

## 12) Related docs

- [docs/events.md](events.md)
- [docs/webhooks.md](webhooks.md)
- [docs/faq.md](faq.md)
- [docs/local-invoke.md](local-invoke.md)
- [fluxapay/DISPUTE_HANDLING.md](../fluxapay/DISPUTE_HANDLING.md)
