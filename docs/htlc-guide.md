# Hash Time-Locked Contract (HTLC) Escrow Module

The FluxaPay HTLC module (`fluxapay/src/htlc.rs`) is its own Soroban contract (`Htlc`, feature flag `contract-htlc`) that provides trustless conditional payments. Tokens are locked in an escrow that can only be released to the receiver when the correct cryptographic preimage is revealed before a timelock expires, or refunded to the sender after it expires. This is the building block for cross-chain atomic swaps (e.g. Stellar <> Bitcoin/Ethereum lightning bridges) and off-chain counterparty settlement.

## Overview

An HTLC is identified by a unique `String` id and holds the following state:

| Field            | Type                | Description                                                                 |
|------------------|---------------------|-------------------------------------------------------------------------------|
| `id`             | String              | Unique id, minted from an instance-storage counter (`htlc_<n>`).              |
| `sender`         | Address             | Account that funded the escrow and receives refunds.                          |
| `receiver`       | Address             | Account allowed to claim with the correct preimage.                           |
| `token`          | Address             | Token contract being escrowed.                                                |
| `amount`         | i128                | Amount of tokens locked.                                                       |
| `hashlock`       | BytesN<32>          | The SHA-256 hash of the secret preimage.                                       |
| `timeout_ledger` | u32                 | Ledger sequence after which refunds/timeout-sweeps are allowed.               |
| `status`         | HtlcStatus          | `Active`, `Claimed`, or `Refunded`.                                            |
| `created_at`     | u64                 | Ledger timestamp at creation.                                                  |
| `resolved_at`    | Option<u64>         | Ledger timestamp at which the HTLC was claimed or refunded, if resolved.      |

## API

The module exposes five entry points.

```rust
pub fn create_htlc(
    env: Env,
    sender: Address,
    receiver: Address,
    token: Address,
    amount: i128,
    hashlock: BytesN<32>,
    timeout_ledger: u32,
) -> Result<String, HtlcError>

pub fn claim_htlc(env: Env, htlc_id: String, claimant: Address, preimage: Bytes) -> Result<(), HtlcError>

pub fn refund_htlc(env: Env, htlc_id: String, caller: Address) -> Result<(), HtlcError>

pub fn claim_htlc_timeout(env: Env, htlc_id: String) -> Result<(), HtlcError>

pub fn get_htlc(env: Env, htlc_id: String) -> Result<HtlcRecord, HtlcError>
```

### `create_htlc`

Locks `amount` of `token` from `sender` into escrow. The sender must authorize the call. The function returns the new HTLC id. `timeout_ledger` must be a future ledger sequence (strictly greater than `env.ledger().sequence()` at creation time).

### `claim_htlc`

Releases the escrow to the receiver. The claimant must be the HTLC's `receiver` and must supply a preimage whose SHA-256 hash matches `hashlock`. The claim must happen strictly before `timeout_ledger`.

### `refund_htlc`

Returns the escrowed tokens to the sender. Only the original sender may call this, and only once `env.ledger().sequence() >= timeout_ledger`.

### `claim_htlc_timeout` (Issue #1021)

The automation-friendly counterpart to `refund_htlc`: **any account may call it**, with no signature required from the sender or receiver, once `env.ledger().sequence() >= timeout_ledger`. It always refunds to the original `sender`, never to the caller, so a disinterested third party (or a bot, see [Sweep daemon](#sweep-daemon-scriptshtlc-sweepjs) below) can sweep expired HTLCs without holding any keys for either party.

- Reverts with `HtlcError::NotExpired` if called before `timeout_ledger`.
- Reverts with `HtlcError::AlreadyClaimed` if the HTLC was already claimed or refunded (including by a previous `claim_htlc_timeout` or `refund_htlc` call).
- Emits `HTLC/TIMEOUT_REFUNDED` on success.

### `get_htlc`

Read-only lookup. Returns `HtlcError::NotFound` if the id does not exist.

## Security Properties

- **Preimage verification.** Claims are guarded by `env.crypto().sha256(&preimage).to_bytes() == hashlock`. Any mismatch results in `HtlcError::PreimageMismatch`.
- **Timelock enforcement.** Claims at or after `timeout_ledger` fail with `HtlcError::Expired`; refunds and timeout-sweeps before `timeout_ledger` fail with `HtlcError::NotExpired`.
- **Authorization.** `claim_htlc` requires the receiver's signature; `refund_htlc` requires the sender's signature; `claim_htlc_timeout` requires no signature at all by design (the contract itself, not the caller, authorizes the outbound transfer of its own escrowed balance).
- **Double-claim prevention.** The HTLC status is updated to `Claimed` or `Refunded` and written to storage *before* the token transfer, so a reentrant call, a replay, or a race between `refund_htlc` and `claim_htlc_timeout` cannot drain the same escrow twice.
- **Bounded storage.** HTLC records are stored in persistent storage with explicit TTL extension on every write, and the id counter is maintained in instance storage.

## Events

| Event              | Topics                      | Data                                                              |
|--------------------|------------------------------|---------------------------------------------------------------------|
| `CREATED`          | `(HTLC, CREATED)`            | `id`, `sender`, `receiver`, `token`, `amount`, `timeout_ledger`      |
| `CLAIMED`          | `(HTLC, CLAIMED)`            | `id`, `receiver`, `amount`                                           |
| `REFUNDED`         | `(HTLC, REFUNDED)`           | `id`, `sender`, `amount`                                             |
| `TIMEOUT_REFUNDED` | `(HTLC, TIMEOUT_REFUNDED)`   | `id`, `sender`, `amount`                                             |

## Usage Example

```rust
use fluxapay::{Htlc, HtlcClient};

// 1. Alice creates an HTLC payable to Bob if he reveals the secret before
//    ledger sequence `env.ledger().sequence() + 17_280` (~24h at ~5s/ledger).
let secret = Bytes::from_array(&env, &[1, 2, 3, 4]);
let hashlock: BytesN<32> = env.crypto().sha256(&secret).to_bytes();

let htlc_id = client.create_htlc(
    &alice,
    &bob,
    &token_address,
    &10_000_000i128,
    &hashlock,
    &(env.ledger().sequence() + 17_280),
);

// 2. Bob claims by revealing the preimage.
client.claim_htlc(&htlc_id, &bob, &secret);

// 3. If Bob never claims, Alice recovers her funds after the timeout...
// client.refund_htlc(&htlc_id, &alice);
//
// ...or anyone (e.g. the sweep daemon) can trigger the same refund for her:
// client.claim_htlc_timeout(&htlc_id);
```

## Errors

| Error               | Meaning                                                                 |
|---------------------|-------------------------------------------------------------------------|
| `InvalidAmount`     | `amount` must be positive.                                              |
| `InvalidTimeout`    | `timeout_ledger` must be in the future.                                 |
| `NotFound`          | No HTLC exists for the given id.                                        |
| `AlreadyClaimed`    | The HTLC has already been claimed or refunded.                          |
| `Unauthorized`      | The caller is not the receiver (claim) or sender (refund).              |
| `PreimageMismatch`  | The supplied preimage does not hash to `hashlock`.                      |
| `Expired`           | The HTLC timelock has passed; claims must happen before `timeout_ledger`. |
| `NotExpired`        | `timeout_ledger` has not passed yet; refunds/sweeps are not yet allowed. |

## Testing

The module is covered by `fluxapay/src/htlc_test.rs`, which exercises:

- **Happy path**: create -> claim with the correct preimage.
- **Late claim**: claim attempted at/after `timeout_ledger` fails with `Expired`.
- **Invalid preimage**: claim with a wrong secret fails with `PreimageMismatch`.
- **Early refund**: `refund_htlc` before `timeout_ledger` fails with `NotExpired`.
- **Refund path**: `refund_htlc` after `timeout_ledger` succeeds and returns funds to the sender.
- **Timeout sweep before expiry**: `claim_htlc_timeout` before `timeout_ledger` fails with `NotExpired`.
- **Timeout sweep at exact expiry**: `claim_htlc_timeout` at `timeout_ledger` succeeds, refunding the sender, even when called by an unrelated third account.
- **Timeout sweep twice**: a second `claim_htlc_timeout` (or a `refund_htlc`/`claim_htlc` after a successful sweep) fails with `AlreadyClaimed`.
- **Unauthorized callers**: non-receiver claims and non-sender refunds fail with `Unauthorized`.

## Sweep daemon (`scripts/htlc-sweep.js`)

`scripts/htlc-sweep.js` is a small Node daemon that polls the indexer (or the contract directly) for HTLCs whose `timeout_ledger` has passed and status is still `Active`, then invokes `claim_htlc_timeout` for each one. It takes no keypair for the sender or receiver - only a funding account to pay the sweep transaction's fee - since `claim_htlc_timeout` itself requires no authorization from either party. See the script header for configuration (`HTLC_SWEEP_POLL_INTERVAL_MS`, `HTLC_CONTRACT_ID`, `STELLAR_NETWORK`, `SWEEP_SECRET_KEY`).

## Integration

The module is its own contract, exported from `fluxapay/src/lib.rs` as `pub mod htlc;` plus `pub use htlc::{Htlc, HtlcClient, HtlcDataKey, HtlcError, HtlcRecord, HtlcStatus};`, and built behind the `contract-htlc` feature (see `fluxapay/Cargo.toml`), consistent with the other single-purpose contracts in this crate (`FXOracle`, `RefundManager`, `MerchantRegistry`, `DexRouter`).
