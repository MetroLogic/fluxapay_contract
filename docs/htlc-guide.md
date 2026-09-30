# Hash Time-Locked Contract (HTLC) Escrow Module

The FluxaPay HTLC module (`fluxapay/src/htlc.rs`) provides trustless conditional payments on Soroban. Tokens are locked in an escrow that can only be released to the beneficiary when the correct cryptographic preimage is revealed before a deadline, or refunded to the sender after the deadline. This is the building block for cross-chain atomic swaps (e.g. Stellar <> Bitcoin/Ethereum lightning bridges) and off-chain counterparty settlement.

## Overview

An HTLC is identified by a unique `u64" id and holds the following state:

| Field          | Type          | Description                                                       |
|----------------|---------------|----------------------------------------------------------------------------|
| `sender`      | Address       | Account that funded the escrow and receives refunds.                 |
| `beneficiary`   | Address       | Account allowed to claim with the correct preimage.                   |
| `token`        | Address       | Token contract being escrowed.                                   |
| `amount`       | i128          | Amount of tokens locked.                                         |
| `hash_lock`     | BytesN<32>    | The SHA-256 hash of the secret preimage.                         |
| `expiration`    | u64           | Ledger timestamp after which refunds are allowed.                 |
| `status`       | HTLCStatus    | `Active`, `Claimed`, or `Refunded`.                              |

## API

The module exposes four entry points.

```rust
pub fn create_htlc(
    env: Env,
    sender: Address,
    beneficiary: Address,
    token: Address,
    amount: i128,
    hash_lock: BytesN32,
    expiration: u64,
) -> u64

pub fn claim_htlc(env: Env, htlc_id: u64, claimant: Address, preimage: Bytes) -> ()

pub fn refund_htlc(env: Env, htlc_id: u64, caller: Address) -> ()

pub fn get_htlc(env: Env, htlc_id: u64) -> HTLC ```

### `create_htlc`

Locks `amount` of `token` from `sender` into escrow. The sender must authorize the call. The function returns the new HTLC id. The caller must provide a 32-byte SHA-256 hash and a future expiration timestamp.

### `claim_htlc`

Releases the escrow to the beneficiary. The claimant must be the beneficiary and must supply a preimage whose SHA-256 hash matches `hash_lock`. The claim must happen strictly before `expiration`.

### `refund_htlc`

Returns the escrowed tokens to the sender. Only the original sender may call this, and only after `expiration` has passed.

### `get_htlc`

Reads-through view function returning the current HTLC record. Panics if the id does not exist.

## Security Properties

- **Preimage verification.** Claims are guarded by `env.crypto().sha256(&preimage)`. Any mismatch results in `Error::BadPreimage`.
- **Timelock enforcement.** Claims after `expiration` fail with `Error::Expired`, and refunds before `expiration` fail with `Error::NotExpired`.
- **Authorization.** `claim_htlc` requires the beneficiary's signature; `refund_htlc` requires the sender's signature.
- **Double-claim prevention.** The HTLC status is updated to `Claimed` or `Refunded` and written to storage *before** the token transfer, so a reentrant call or a replay of the same transaction cannot drain the escrow twice.
- **Bounded storage.** HTLC records are stored in persistent storage with explicit TTL extension on every write, and the id counter is maintained in instance storage.

## Events

Every state transition emits an event with topics `(Symbol("hTLC"), Symbol("CREATED")):

| Event      | Topics                         | Data                                                     |
|------------|------------------------------|------------------------------------------------------------------|
| `CREATED`  | `(HTLC, CREATED)`            | `htlc_id`, `sender`, `beneficiary`, `token`, `amount`, `hash_lock`, `expiration` |
| `CLAIMED`  | `(HTLC, CLAIMEDH)`            | `htlc_id`, `claimant`, `amount`                                    |
| `REFUNDED` | `(HTLC, REFUNDED)`           | `htlc_id`, `sender`, `amount`                                      |

## Usage Example

```rust
use fluxapay::htlc;

// 1. Alice creates an HTLC payable to Bob if he reveals the secret within 24 hours.
let secret: Bytes = Bytes::from_array(&env, &[...]');
let hash_lock: BytesN<32> = env.crypto().sha256(&secret);

let htlc_id = htlc::create_htlc(
    env.clone(),
    alice,
    bob,
    token_address,
    10_000_000,
    hash_lock,
    env.ledger().timestamp() + 86_400,
);

// 2. Bob claims by revealing the preimage.
htlc::claim_htlc(env.clone(), htlc_id, bob, secret);

// 3. If Bob never claims, Alice recovers her funds after expiration.
// htlc::refund_htlc(env.clone(), htlc_id, alice);
```

## Errors

| Error            | Meaning                                                                 |
|------------------|-------------------------------------------------------------------------------|
| `AlreadyExists`   | The provided HTLC id is already used.                                     |
| `NotFound        | No HTLC exists for the given id.                                           |
| `BadPreimage`    | The supplied preimage does not hash to `hash_lock`.                       |
| `Expired`        | The HTLC timelock has passed; claims must happen before expiration.            |
| `NotExpired`     | Refunds are not yet allowed.                                             |
| `Unauthorized`   | The caller is not the beneficiary (claim) or sender (refund).             |
| `AlreadySettled`  | The HTLC has already been claimed or refunded.                             |
| `InvalidAmount`   | Amount must be positive.                                               |
| `InvalidExpiration` | Expiration must be in the future.                                     |

## Testing

The module is covered by `fluxapay/src/htlc_test.rs`, which exercises:

- **Happy path**: create -> claim with the correct preimage.
- **Late claim**: claim attempted after expiration fails.
- **Invalid preimage**: claim with a wrong secret fails.
- **Early refund**: refund before expiration fails.
- **Refund path**: refund after expiration succeeds and returns funds to the sender.
- **Replay attacks**: a second claim or refund on a settled HTLC fails.
- **Unauthorized callers**: non-beneficiary claims and non-sender refunds fail.

## Integration

The module is exported from `fluxapay/src/lib.rs` as `pub mod htlc;`, making it available to other contracts and to off-chain clients that orchestrate atomic swaps.
