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

## Merchant → sub-account delegation (Issue #1029)

Separate from session keys above: a merchant can delegate specific payment
operations to a sub-account (a POS terminal, an employee key, etc.). The
sub-account signs the delegated call directly with its own key — the
merchant does not sign again at call time.

### Scopes

| Scope | Covers |
|-------|--------|
| `CreatePayment` | `PaymentProcessor::create_payment_delegated` |
| `CreateRefund` | `PaymentProcessor::create_refund_delegated` |
| `ReadOnly` | Reserved for future read-only delegated queries; not enforced on any write path yet |
| `All` | Every delegated operation above |

### Entry points

| Function | Caller | Effect |
|----------|--------|--------|
| `grant_delegation(merchant, sub_account, scope, expires_at)` | Merchant | Creates or replaces the sub-account's delegation |
| `revoke_delegation(merchant, sub_account)` | Merchant | Marks the delegation revoked (idempotent) |
| `list_delegations(merchant)` | Anyone | Read-only audit view, including revoked/expired entries |
| `create_payment_delegated(sub_account, args)` | Sub-account | Creates a payment for `args.merchant_id`, requiring a live `CreatePayment` (or `All`) delegation |
| `create_refund_delegated(sub_account, payment_id, refund_amount, reason, receipt_hash)` | Sub-account | Creates a refund for the payment's merchant, requiring a live `CreateRefund` (or `All`) delegation |

### Errors

| Name | When |
|------|------|
| `Error::DelegationExpired` | `now > delegation.expires_at` |
| `Error::DelegationRevoked` | The merchant called `revoke_delegation` for this sub-account |
| `Error::Unauthorized` | No delegation exists, or the granted scope does not cover the requested operation |

A re-`grant_delegation` call for the same `(merchant, sub_account)` pair
replaces the previous grant (new scope/expiry, `revoked` reset to `false`),
mirroring `register_session_key`'s re-registration semantics above.

## Offline signing (air-gapped transactions)

For high-security environments where the signing key must never be exposed to a
networked machine, FluxaPay supports a full offline signing workflow (Issue #1059).

### Flow

```
Online machine                     Air-gapped signer
─────────────────────────────────────────────────────
1. buildUnsignedTransaction()
   → unsigned XDR + txHash
                               ──────────────────────▶
                                 2. OfflineSigner.sign()
                                    (zero network calls)
                                    → signedXdr
                               ◀──────────────────────
3. submitSignedTransaction()
   → confirmed result
```

### Usage

```ts
import { OfflineSigner } from "@fluxapay/sdk";

// ── Step 1: build unsigned XDR (online machine) ──────────────────────────────
const signer = new OfflineSigner(client, contractId, networkPassphrase);

const { xdr, txHash } = await signer.buildUnsignedTransaction("create_payment", {
  payment_id: "pay_001",
  merchant_id: "GCMERCHANT...",
  amount: BigInt(100_000_000),
  currency: "USDC",
  deposit_address: "GCDEPOSIT...",
});

// Serialize `xdr` and `txHash` and transfer to air-gapped machine
// (e.g., QR code, USB drive, offline file transfer).

// ── Step 2: sign on the air-gapped machine (zero network calls) ──────────────
// import { OfflineSigner } from "@fluxapay/sdk";
// import { Keypair } from "@stellar/stellar-sdk";

const signerKeypair = Keypair.fromSecret("SXXXXXXX...");
const signedXdr = await OfflineSigner.sign({ xdr, signerKeypair });

// Transfer `signedXdr` back to the online machine.

// ── Step 3: broadcast the signed transaction (online machine) ────────────────
const result = await signer.submitSignedTransaction(signedXdr);
console.log("Transaction confirmed:", result);
```

### `buildUnsignedTransaction(method, args)`

Calls the specified contract method, simulates it, and returns:
- `xdr` — base64-encoded unsigned Soroban transaction envelope  
- `txHash` — hex-encoded transaction hash for display and verification

No private key is needed at this stage.

### `OfflineSigner.sign({ xdr, signerKeypair })`

Static method with **zero network dependencies**. Parses the unsigned envelope,
signs it with the provided Stellar keypair, and returns the signed base64 XDR.
Safe to call on an air-gapped machine that has no internet access.

### `submitSignedTransaction(signedXdr)`

Broadcasts a pre-signed transaction envelope to the Soroban RPC configured in
the client. Waits for the transaction to be confirmed and returns the result.

### Key guarantees

- `OfflineSigner.sign` makes no HTTP calls.
- The signing keypair is never passed to a networked function.
- Works with any Stellar keypair: Keypair, HSM-backed signer, Ledger hardware wallet.
- Round-trip result is identical to a standard online `client.method(args)` call.
