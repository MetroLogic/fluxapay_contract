# ADR-0001: Access Control Across PaymentProcessor and RefundManager

- Status: Accepted
- Date: 2026-03-28

## Context

`PaymentProcessor` and `RefundManager` are separate contracts (separate storage), even though both reuse the same `AccessControl` module code. Role membership is therefore not shared automatically.

## Decision

Keep **duplicate role storage per contract** and define explicit provisioning/synchronization procedures.

## Rationale

- Lowest-risk change for current architecture.
- Avoids introducing a new shared admin contract dependency now.
- Keeps each contract independently deployable and upgradable.

## Operational Policy

When provisioning oracle/operator/admin keys:

1. Grant required roles on `PaymentProcessor`.
2. Grant required roles on `RefundManager`.
3. Verify both role sets before enabling traffic.

For oracle keys specifically, maintain the same oracle set on both contracts when workflows depend on both payment verification and refund/dispute actions.

## Consequences

- Pros: Simpler rollout, no cross-contract auth dependency.
- Cons: Operational overhead and drift risk between role sets.

## Revisit Trigger

If role drift causes incidents or operational burden grows, move to a shared governance/admin contract for both processors and document upgrade/migration sequencing.
