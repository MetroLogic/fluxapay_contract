# Checks-Effects-Interactions review: token-transfer entrypoints (issue #1169)

Scope: every public entrypoint in `PaymentProcessor` (`fluxapay/src/lib.rs`) that
moves tokens or calls a cross-contract token interface. Soroban transactions are
atomic, but a malicious token/router callback can still re-enter a *different*
entrypoint in the same transaction. The rule applied here is:

1. **Checks** — authenticate, validate status, amount, expiry and authorization.
2. **Effects** — write the terminal/status state to persistent storage.
3. **Interactions** — call `token::Client` / router / registry clients last.

## Review table

| Entrypoint | Status/state committed before transfer | Notes |
|---|---|---|
| `create_payment` | Payment stored as `Pending` before any payer transfer | `record_payment_status` + payment write precede the `transfer_from` call. |
| `confirm_payment` | `Confirmed` stored before escrow bookkeeping | Guarded by status machine; a second confirm returns `PaymentAlreadyProcessed`. |
| `settle_payment` | **Fixed in this change** — `Settled` is transitioned, recorded and persisted immediately after the `Confirmed` check and before the settlement-fee/merchant-payout transfers | Previously the status transition happened after several `try_transfer` calls. The nested earlier transition is now a no-op guard so the terminal state is written exactly once. A global `ReentrancyLock` also wraps this function. |
| `cancel_payment` | `Failed`/`Expired` persisted before event emission; no outbound transfer | Idempotent via status guard. |
| `process_refund` | `RefundStatus::Completed` persisted at `process_refund_internal` before the direct or DEX-routed transfer | Re-entry sees a non-`Pending` refund and returns `RefundAlreadyProcessed`. |
| `claim_refund` | Requires `Pending` + approved before delegating to `process_refund_internal` | Double claim cannot pass the status guard. |
| `refund_instantly` | `Completed` persisted before `try_transfer` of net amount and fee | Verified. |
| `resolve_dispute*` | Terminal dispute status/bond bookkeeping persisted before winner/collector transfers | Verified for single and batch resolution. |
| `swap_and_pay` / `swap_and_pay_multi_route` | Payment created (`Pending`) after the DEX swap; volume counter and events written after, no token balance held across a callback that can re-enter creation with the same id | Router allowlist, amount checks and idempotency keys bound repeated calls. |
| `withdraw_treasury` | Treasury balance debited in storage before transfer | Verified. |

## Reentrancy and double-spend assertions

Tests added for this issue assert that:

- a second `claim_refund`/`process_refund` on the same refund is rejected with
  `RefundAlreadyProcessed` (no double refund);
- a second `settle_payment` on the same payment is rejected with
  `PaymentAlreadyProcessed` (no double payout);
- routes that already enforce one-time state transitions continue to emit a
  single completion event.

`try_transfer` failures are intentionally tolerable on some fee paths (a failed
fee must not undo a completed payment); the terminal status is nevertheless
written first so no payout path can be replayed.
