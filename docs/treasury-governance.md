# Treasury Governance: M-of-N Multi-Sig with Timelock

## Overview

The FluxaPay RefundManager contract implements institutional-grade treasury governance using an **on-chain M-of-N multi-signature approval flow combined with a time-locked proposal queue**. This ensures that treasury withdrawals above a configurable threshold require multiple authorized signers and a mandatory delay period before execution, preventing single-key compromise from draining funds.

## Architecture

### Core Components

1. **TreasuryMultisigConfig** - Governance configuration
2. **TreasuryWithdrawalProposal** - Proposal lifecycle state
3. **Token-specific Treasury Balances** - Multi-token support
4. **Event System** - Full audit trail

### Data Structures

```rust
/// Treasury withdrawal proposal with M-of-N multi-sig and timelock.
pub struct TreasuryWithdrawalProposal {
    pub proposal_id: String,              // Unique proposal identifier (e.g., "tw_1")
    pub token_address: Address,           // Token contract address
    pub destination: Address,             // Withdrawal destination
    pub amount: i128,                     // Amount to withdraw
    pub proposer: Address,                // Signer who created the proposal
    pub approvals: Vec<Address>,          // Signers who have approved
    pub created_at: u64,                  // Creation timestamp
    pub earliest_execution_time: u64,     // Timelock expiry timestamp
    pub executed: bool,                   // Whether proposal has been executed
    pub cancelled: bool,                  // Whether proposal was cancelled
}

/// Treasury multisig configuration for withdrawal governance.
pub struct TreasuryMultisigConfig {
    pub required_approvals: u32,          // M: minimum approvals needed
    pub admin_signers: Vec<Address>,      // N: authorized signers
    pub min_delay_secs: u64,              // Minimum timelock (1h–48h)
    pub max_delay_secs: u64,              // Maximum proposal validity (1h–48h)
}
```

## Configuration

### `configure_treasury_multisig`

**Admin-only.** Sets up the multi-signature governance parameters.

```rust
pub fn configure_treasury_multisig(
    env: Env,
    admin: Address,
    required_approvals: u32,     // M (1–N)
    admin_signers: Vec<Address>, // N authorized signers (max 10)
    min_delay_secs: u64,         // Minimum timelock: 1 hour – 48 hours
    max_delay_secs: u64,         // Maximum validity: min_delay – 48 hours
) -> Result<(), Error>
```

**Validation Rules:**
- `required_approvals > 0` and `required_approvals ≤ admin_signers.len()`
- `admin_signers.len() ≤ 10` (MAX_TREASURY_SIGNERS)
- `min_delay_secs ≥ 3,600` (1 hour) and `≤ 172,800` (48 hours)
- `max_delay_secs ≥ min_delay_secs` and `≤ 172,800` (48 hours)
- All signers must be unique

**Default Values:**
- `DEFAULT_TREASURY_REQUIRED_APPROVALS = 3`
- `DEFAULT_TREASURY_TIMELOCK_SECS = 86,400` (24 hours)
- `MAX_TREASURY_TIMELOCK_SECS = 172,800` (48 hours)

### `get_treasury_multisig_config`

**Public read.** Returns current governance configuration.

```rust
pub fn get_treasury_multisig_config(env: Env) -> Option<TreasuryMultisigConfig>
```

## Proposal Lifecycle

### 1. Propose → `propose_treasury_withdrawal`

**Authorized signer only.** Creates a withdrawal proposal.

```rust
pub fn propose_treasury_withdrawal(
    env: Env,
    proposer: Address,        // Must be in admin_signers
    token_address: Address,   // Token to withdraw (must be allowed)
    destination: Address,     // Recipient address
    amount: i128,             // Amount > 0
) -> Result<String, Error>  // Returns proposal_id
```

**Behavior:**
- Validates caller is an authorized signer
- Checks token is allowed (configured via `allow_token` or default USDC)
- Verifies sufficient treasury balance for the token
- Auto-registers proposer's approval
- Sets `earliest_execution_time = now + min_delay_secs`
- Emits `TREASURY/WITHDRAWAL_PROPOSED` event

### 2. Approve → `approve_treasury_withdrawal`

**Authorized signer only.** Adds approval to a pending proposal.

```rust
pub fn approve_treasury_withdrawal(
    env: Env,
    approver: Address,    // Must be in admin_signers
    proposal_id: String,
) -> Result<(), Error>
```

**Behavior:**
- Validates caller is an authorized signer
- Rejects if proposal executed, cancelled, or expired (`now > created_at + max_delay_secs`)
- Prevents duplicate approvals from same signer
- Emits `TREASURY/WITHDRAWAL_APPROVED` event with current/target approval counts

### 3. Execute → `execute_treasury_withdrawal`

**Any caller (typically a signer).** Executes approved proposal after timelock.

```rust
pub fn execute_treasury_withdrawal(
    env: Env,
    executor: Address,
    proposal_id: String,
) -> Result<(), Error>
```

**Preconditions (all must pass):**
- `now ≥ earliest_execution_time` (timelock expired)
- `now ≤ created_at + max_delay_secs` (not expired)
- `approvals.len() ≥ required_approvals` (threshold met)
- Proposal not executed or cancelled
- Sufficient token balance at execution time

**Behavior:**
- Transfers tokens from contract to destination
- Updates token-specific treasury balance
- Marks proposal as executed
- Records withdrawal in history
- Emits `TREASURY/WITHDRAWAL_EXECUTED` event

### 4. Cancel → `cancel_treasury_withdrawal`

**Authorized signer only.** Vetoes a pending proposal.

```rust
pub fn cancel_treasury_withdrawal(
    env: Env,
    canceller: Address,   // Must be in admin_signers
    proposal_id: String,
) -> Result<(), Error>
```

**Behavior:**
- Validates caller is an authorized signer
- Rejects if proposal already executed or cancelled
- Marks proposal as cancelled (cannot be revived)
- Emits `TREASURY/WITHDRAWAL_CANCELLED` event

### 5. Query → `get_treasury_withdrawal_proposal`

**Public read.** Returns full proposal state.

```rust
pub fn get_treasury_withdrawal_proposal(
    env: Env,
    proposal_id: String,
) -> Result<TreasuryWithdrawalProposal, Error>
```

## Multi-Token Support

Each token maintains a separate treasury balance:

```rust
/// Per-token treasury balance (new)
DataKey::TokenTreasuryBalance(token_address: Address) → i128

/// Global USDC balance (legacy, deprecated)
DataKey::TreasuryBalance → i128
```

- **Proposals are per-token**: Separate proposals for USDC, EURC, etc.
- **Balance checks are token-specific**: USDC balance doesn't cover EURC withdrawals
- **Fees accrue to token's treasury**: Refund fees in EURC go to EURC treasury

## Events

All governance actions emit structured events for indexers:

| Event | Topics | Data |
|-------|--------|------|
| `TREASURY/WITHDRAWAL_PROPOSED` | — | `(proposal_id, token, destination, amount, proposer, earliest_execution_time)` |
| `TREASURY/WITHDRAWAL_APPROVED` | — | `(proposal_id, approver, approvals_count, required_approvals)` |
| `TREASURY/WITHDRAWAL_EXECUTED` | — | `(proposal_id, token, destination, amount, executor)` |
| `TREASURY/WITHDRAWAL_CANCELLED` | — | `(proposal_id, canceller)` |
| `TREASURY/MULTISIG_CONFIGURED` | — | `(required_approvals, min_delay_secs, max_delay_secs)` |

## Security Model

### Threat Mitigation

| Threat | Mitigation |
|--------|------------|
| Single admin key compromise | M-of-N requires multiple independent keys |
| Hasty/malicious execution | Mandatory `min_delay_secs` (1h–48h) review window |
| Stale proposal execution | `max_delay_secs` expiry (max 48h) |
| Rogue signer approval | Duplicate approval prevention |
| Unauthorized proposer | Signer allowlist validation |
| Insufficient funds | Balance check at propose AND execute time |

### Operational Best Practices

1. **Signer Distribution**: Distribute signer keys across multiple people/devices/organizations
2. **Threshold Selection**: Use `M = ceil(N/2) + 1` for strong consensus (e.g., 3-of-5, 4-of-7)
3. **Timelock Duration**: Set `min_delay_secs ≥ 24h` for production; shorter for testing
4. **Monitoring**: Index `TREASURY/*` events for real-time alerts
5. **Key Rotation**: Use `configure_treasury_multisig` to update signers (requires admin)

## Integration Guide

### Initial Setup

```rust
// 1. Deploy contract and initialize
let admin = Address::from_str("GABC...");
client.initialize_refund_manager(&admin, &usdc_address);

// 2. Configure multisig (3-of-5, 24h min / 48h max delay)
let signers = vec![
    &env,
    Address::from_str("GSIGNER1..."),
    Address::from_str("GSIGNER2..."),
    Address::from_str("GSIGNER3..."),
    Address::from_str("GSIGNER4..."),
    Address::from_str("GSIGNER5..."),
];
client.configure_treasury_multisig(
    &admin,
    &3,
    &signers,
    &86_400,    // 24 hours
    &172_800,   // 48 hours
);

// 3. Fund treasury
usdc_client.mint(&contract_address, &10_000_000_000); // 100,000 USDC
```

### Withdrawal Flow

```rust
// 1. Signer 1 proposes
let proposal_id = client.propose_treasury_withdrawal(
    &signer1,
    &usdc_address,
    &destination,
    &1_000_000_000, // 10,000 USDC
);

// 2. Signer 2 approves
client.approve_treasury_withdrawal(&signer2, &proposal_id);

// 3. Signer 3 approves (threshold met)
client.approve_treasury_withdrawal(&signer3, &proposal_id);

// 4. Wait for timelock (24h)
// ... time passes ...

// 5. Any signer executes
client.execute_treasury_withdrawal(&signer1, &proposal_id);
```

### Emergency Cancellation

```rust
// Any signer can cancel before execution
client.cancel_treasury_withdrawal(&signer4, &proposal_id);
```

## Error Codes

| Code | Error | Cause |
|------|-------|-------|
| 71 | `TreasuryMultisigNotConfigured` | Governance not initialized |
| 72 | `NotAuthorizedTreasurySigner` | Caller not in `admin_signers` |
| 73 | `TreasuryProposalNotFound` | Invalid proposal ID |
| 74 | `TreasuryProposalAlreadyExecuted` | Proposal already executed |
| 75 | `TreasuryProposalCancelled` | Proposal was cancelled |
| 76 | `TreasuryTimelockNotExpired` | `now < earliest_execution_time` |
| 77 | `TreasuryProposalExpired` | `now > created_at + max_delay_secs` |
| 78 | `TreasuryAlreadyApproved` | Signer already approved |
| 79 | `TreasuryInsufficientApprovals` | `approvals < required_approvals` |
| 80 | `InvalidTreasuryThreshold` | Invalid M/N configuration |
| 81 | `InsufficientTokenTreasuryBalance` | Token balance < amount |

## Testing

Run the comprehensive test suite:

```bash
cargo test treasury_multisig -- --nocapture
```

Tests cover:
- ✅ Threshold approvals (M-of-N)
- ✅ Duplicate signature rejection
- ✅ Premature execution revert (timelock)
- ✅ Post-timelock execution success
- ✅ Cancellation by any signer
- ✅ Unauthorized caller rejection
- ✅ Multi-token proposal isolation
- ✅ Proposal expiry handling
- ✅ Event emission verification

## Migration from Legacy `withdraw_treasury`

The legacy single-admin `withdraw_treasury` function remains for backward compatibility but **should be deprecated** in favor of the multisig flow. 

**Migration path:**
1. Configure multisig with `required_approvals = 1` and single admin as signer
2. Use `propose → approve (auto) → wait timelock → execute` flow
3. Increase `required_approvals` and add signers progressively
4. Remove `withdraw_treasury` access once multisig is validated

## Constants Reference

```rust
pub const DEFAULT_TREASURY_REQUIRED_APPROVALS: u32 = 3;
pub const MAX_TREASURY_SIGNERS: u32 = 10;
pub const DEFAULT_TREASURY_TIMELOCK_SECS: u64 = 24 * 60 * 60;      // 24h
pub const MAX_TREASURY_TIMELOCK_SECS: u64 = 48 * 60 * 60;          // 48h
pub const MIN_TREASURY_TIMELOCK_SECS: u64 = 60 * 60;               // 1h
pub const TREASURY_WITHDRAWAL_HISTORY_CAP: u32 = 100;
```
