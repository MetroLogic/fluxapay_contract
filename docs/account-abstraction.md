# Session keys and spending policies

A session key is a delegated signer for a parent account. The parent authorizes the key once. Until the key expires or is revoked, that signer can submit `execute_with_session` without another signature from the parent.

`register_session_key` stores an unrestricted key. It checks only `expires_at` and revocation, and it still accepts an opaque payload. Use it when the delegated signer is fully trusted for the whole session.

`register_session_key_with_policy` stores the same key plus a `SessionPolicy`. `execute_with_session` then enforces that policy before the call is accepted. A leaked checkout key can no longer move an arbitrary amount or call an arbitrary contract.

## Policy

| Field | Meaning |
|-------|---------|
| `max_amount_per_tx` | Largest `amount` accepted in one call. `None` disables the cap. |
| `max_amount_per_window` | Cumulative `amount` accepted inside the current window. `None` disables the budget. |
| `window_secs` | Length of the rolling window in seconds. `0` keeps one window for the life of the key. Ignored when the cumulative cap is `None`. |
| `allowed_contracts` | Contracts the key may call. Empty allows every contract. |
| `allowed_functions` | Function selectors the key may call. Empty allows every function. |

Caps must be `None` or `>= 0`. A negative cap is rejected at registration with `InvalidPayload`.

An empty allowlist is the unrestricted default. It does not mean "allow nothing". To restrict calls, pass the contracts and functions the key is permitted to use.

Re-registering a key replaces the policy and clears spend already counted against the previous window. `revoke_session_key` deletes the policy and the spend counter, and a later execution of that signer still fails with `SignerRevoked` until the key is registered again.

## Invocation payload

Unrestricted keys accept any non-empty payload.

A key with a spend cap or a non-empty allowlist requires the payload to be the ScVal XDR of `SessionInvocation`:

| Field | Type | Meaning |
|-------|------|---------|
| `target_contract` | `Address` | Contract the call is addressed to |
| `function` | `Symbol` | Function selector |
| `amount` | `i128` | Amount this call spends. Must be `>= 0`. |

Build it with Soroban `ToXdr` (Rust) or the equivalent ScVal XDR encoding from a Stellar SDK. `from_xdr` rejects a value that is not a `SessionInvocation`. Bytes that are not ScVal XDR trap inside the host.

Checks run in this order:

1. The session key must authenticate, must not be revoked, and must be unexpired. The payload must be non-empty.
2. `target_contract` must be in `allowed_contracts` when that list is non-empty.
3. `function` must be in `allowed_functions` when that list is non-empty.
4. `amount` must be `<= max_amount_per_tx` when that cap is set. A call equal to the cap is accepted.
5. When `max_amount_per_window` is set, the call is added to `spent_in_window`. If `window_secs > 0` and `now >= window_start + window_secs`, the counter is reset to `0` and `window_start` becomes `now` before the new amount is added. A call that would make the total greater than the budget is rejected and does not move the counter.

The window reset is stored only when the call is accepted.

## Events

Topics are `(SESSION, ACTION, account)`, the same shape as `SESSION/REGISTERED`, `SESSION/REVOKED`, and `SESSION/EXECUTED`.

| Action | When | Data |
|--------|------|------|
| `POLICY_REGISTERED` | `register_session_key_with_policy` succeeds | `(session_key, policy)` |
| `POLICY_VIOLATION` | A configured policy rejects the call | `(session_key, reason)` |
| `WINDOW_ROLLOVER` | An accepted call opens a new spend window | `(session_key, previous_start, window_start)` |

`reason` is one of `max_per_tx`, `window_budget`, `contract`, or `function`.

## Errors

`AccountAbstractionError` codes used by policy enforcement:

| Code | Name | When |
|------|------|------|
| 4 | `InvalidPayload` | Empty payload, negative amount, negative cap, or a payload that is not a `SessionInvocation` |
| 6 | `SpendLimitExceeded` | `amount > max_amount_per_tx` |
| 7 | `WindowBudgetExceeded` | The call would exceed `max_amount_per_window` in the current window |
| 8 | `ContractNotAllowed` | `target_contract` is outside `allowed_contracts` |
| 9 | `FunctionNotAllowed` | `function` is outside `allowed_functions` |

Registration, expiry, and revocation still return `Unauthorized`, `SessionNotFound`, `SessionExpired`, and `SignerRevoked`.
