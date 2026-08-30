# ADR-0004: Multi-Sig Admin Governance

- Status: Accepted
- Date: 2026-08-29

## Context

FluxaPay relies on privileged administrative actions to maintain the contract set, configure risk parameters, and manage attacker-containment controls. A single admin key is operationally convenient, but it creates a single point of failure: if that key is compromised, the attacker can change critical settings, revoke or grant roles, or alter settlement risk parameters without any independent validation.

The contract therefore includes an on-chain multi-sig proposal path in `fluxapay/src/access_control.rs` and exposes it through `create_proposal`, `vote_proposal`, and `execute_proposal` in the main contract entry points. The goal is to decentralize approval for dangerous administrative actions while keeping the execution model simple and auditable.

## Decision

Adopt an N-of-M governance model in which a configurable signer set validates high-risk actions before they are executed.

### Configurable threshold

The signer set and minimum approval requirement are stored as:

```rust
(MultisigConfig) = (threshold: u32, signers: Vec<Address>)
```

- `threshold` is configurable by the current admin.
- `signers` is a fixed allowlist of addresses eligible to create or approve proposals.
- The default configuration is an `1-of-1` model with only the admin as signer to preserve a safe fallback during bootstrap.
- The contract rejects `threshold == 0` and any threshold greater than the signer count.

### Proposal lifecycle

A proposal follows this flow:

1. A signer creates a proposal with `create_proposal(signer, action)`.
2. The proposal is persisted under a monotonically increasing `nonce`.
3. Other authorized signers call `vote_proposal(signer, nonce)` to add their approval.
4. Once `approvals.len() >= threshold`, a privileged executor can call `execute_proposal(executor, nonce)`.
5. If the proposal is not executed within its expiry window, it is rejected as expired.

This is intentionally a small, explicit governance mechanism: no separate governance token, no quorum calculation beyond the configured signer set, and no delegated voting model.

## Why nonce-based replay protection

The contract uses a `NextProposalNonce` counter and stores each proposal under `Proposal(nonce)`. This is the replay protection mechanism.

### Rationale

- A nonce is unique and monotonic; the same action payload can be re-submitted later as a different proposal with a new nonce.
- The contract prevents duplicate approvals by checking whether a signer already appears in `proposal.approvals` before pushing a new vote.
- Proposal execution is bound to a specific stored record (`Proposal(nonce)`), so a stale or duplicated request cannot be replayed against a different state or reused to re-trigger an already-executed action.
- This is safer than using a proposal ID derived from arbitrary action content or a hash of the payload alone, because action content can legitimately repeat across time while the governance state must remain unique and auditable per proposal instance.

In short: the nonce identifies the governance operation instance, while the action payload describes what would happen. Those are intentionally separate concepts, which makes replay or accidental re-execution much harder.

## Proposal expiry

Unexecuted proposals expire after 48 hours:

```rust
let expiry = proposal.created_at + 48 * 60 * 60;
if now > expiry {
    return Err(AccessControlError::ProposalExpired);
}
```

### Why 48 hours

- It gives the signer set enough time to review, discuss, and endorse the change.
- It prevents stale governance actions from remaining pending indefinitely.
- It reduces the risk of an old proposal being executed after the operational context has changed.

A proposal that has not reached threshold before expiry is rejected; after expiry it is no longer executable even if the required approvals are later collected.

## Actions currently gated by the multi-sig proposal path

The current `AdminAction` enum defines the governance actions that flow through this proposal mechanism. The contract executes them only after approvals meet the configured threshold:

- `SetGlobalPause(bool, Symbol)`
- `AllowToken(Address)`
- `GrantRole(Symbol, Address)`
- `RevokeRole(Symbol, Address)`
- `TransferAdmin(Address)`
- `EmergencyRevokeRole(Symbol, Address)`
- `SetDisputeBond(i128)`
- `SetVolumeCap(KycTier, i128)`
- `SetRefundFeeBps(i128)`
- `SetRateLimit(u32, u64)`

These cover the highest-risk runtime configuration changes: pause state, token allowlisting, privileged role changes, business critical parameters, and settlement risk limits.

### Important boundary

The multi-sig proposal path is not used for every admin operation. In the current implementation, direct admin-auth or timelocked actions such as contract upgrades and treasury withdrawals remain outside the `AdminAction` proposal list and follow their own execution flow. This keeps the governance surface explicit and prevents the proposal system from becoming a catch-all for unrelated privileged actions.

## Alternatives considered

### 1. Timelock only

A timelock provides delay and visibility, but it does not provide independent signer approval. It is useful for reducing the speed of a malicious or mistaken admin action, but it does not create the same “multiple-key authorization” condition as N-of-M governance.

### 2. Off-chain governance

Off-chain voting or signers coordinating outside the contract can work for some organizations, but it creates a trust gap: the on-chain contract has no direct proof that a quorum was met, and the final execution path cannot be independently audited from chain state alone. On-chain proposal records are easier to inspect, replay-check, and reason about under Soroban execution semantics.

## Consequences

### Benefits

- Stronger compromise resistance than single-admin control.
- Clear approval trail on-chain via `nonce`, `approvals`, and event publication.
- Configurable threshold lets operators tune risk vs. operational speed.
- Expiry prevents stale governance actions from lingering in the proposal set.

### Costs

- More coordination is required for any sensitive change.
- The signer set must be managed carefully; threshold and membership are critical security parameters.
- M-of-N approval introduces operational latency compared to direct admin calls.

## Security notes

The governance system is designed to protect against lone-admin misuse and key compromise. It is not a substitute for secure key custody or operational controls: signer wallets, threshold configuration, and proposal review must still be handled with proper multisig wallet management and secure operational procedures.
