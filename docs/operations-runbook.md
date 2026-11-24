# Operator Runbook: Contract Upgrade & Admin Recovery

This runbook covers the two highest-stakes admin operations on the deployed
`PaymentProcessor` and `RefundManager` contracts: upgrading the contract WASM
and transferring (or recovering) the `ADMIN` role. Both are timelocked —
neither takes effect immediately — so plan around the delays below rather
than being surprised by them during an incident.

It complements [DEPLOYMENT.md](../DEPLOYMENT.md) (which covers the CI/CD
pipeline and network configuration) and
[docs/mainnet-deployment-checklist.md](mainnet-deployment-checklist.md)
(pre-launch checklist); this document is specifically about the on-chain
`stellar contract invoke` calls an operator runs against an already-deployed
contract.

## Table of Contents

- [Before you start](#before-you-start)
- [Contract upgrade (propose → timelock → execute)](#contract-upgrade-propose--timelock--execute)
- [Admin transfer and recovery](#admin-transfer-and-recovery)
- [Rollback considerations](#rollback-considerations)

## Before you start

- You need the `ADMIN` role on the target contract for every call in this
  document except `accept_admin` (called by the *new* admin) and the
  (currently unreachable — see
  [Recovery-key transfer](#recovery-key-transfer-currently-unreachable))
  recovery-key flow.
- `--source` below is the signing key's identity in your local
  `stellar keys` / CLI config, matching the convention already used in
  [DEPLOYMENT.md](../DEPLOYMENT.md#-admin-key-rotation).
- Every timestamp/ledger check in this runbook is enforced on-chain by the
  contract itself (`fluxapay/src/lib.rs` and `fluxapay/src/access_control.rs`)
  — the CLI will simply return an error (`TimelockNotExpired`, or a panic for
  the admin-transfer ledger check) if you try to execute early. There is no
  way to force an early execution.

## Contract upgrade (propose → timelock → execute)

Upgrades are two steps: `upgrade_contract` **queues** the new WASM hash
behind a timelock, and `execute_timelocked_action` **applies** it once the
delay has elapsed. There is no one-call "upgrade now" — this is intentional,
so a compromised or mistaken admin key can't swap the contract's code out
from under users without a window for the community/other signers to notice
and react.

### 1. Build and install the new WASM, get its hash

```bash
cd fluxapay
stellar contract build
stellar contract optimize --wasm target/wasm32-unknown-unknown/release/fluxapay.wasm

stellar contract install \
  --wasm target/wasm32-unknown-unknown/release/fluxapay.optimized.wasm \
  --network testnet \
  --source <ADMIN_SECRET>
# -> prints the new WASM's hash, e.g. 1a2b3c...  (this is `new_wasm_hash` below)
```

### 2. (Optional) Check or adjust the timelock delay

```bash
stellar contract invoke --id <CONTRACT_ID> --network testnet --source <ADMIN_SECRET> \
  -- get_timelock_delay
# Default: 172800 (48 hours, in seconds — DEFAULT_TIMELOCK_SECS in lib.rs)
```

> [!WARNING]
> `set_timelock_delay` takes effect **immediately** — it is not itself
> timelocked. A malicious or compromised admin key could call
> `set_timelock_delay(0)` immediately before `upgrade_contract` to bypass the
> delay entirely. Treat a `TIMELOCK/DELAY_CHANGED`-style anomaly (watch the
> `get_timelock_delay` value, or diff it between deployments) with the same
> urgency as an unexpected upgrade proposal, and prefer multisig/hardware-key
> custody of the admin key specifically because of this gap.

### 3. Propose the upgrade

```bash
stellar contract invoke --id <CONTRACT_ID> --network testnet --source <ADMIN_SECRET> \
  -- upgrade_contract --admin <ADMIN_ADDRESS> --new_wasm_hash <NEW_WASM_HASH>
# Returns the queued action's ID, e.g. "tl_3"
```

This stores a `PendingTimelockAction { kind: UpgradeContract(new_wasm_hash),
execute_after: now + timelock_delay, proposed_by: admin }` — it does **not**
touch the running WASM yet.

### 4. Wait for the timelock, verifying along the way

Anyone (no auth required) can list queued actions and their `execute_after`
timestamps:

```bash
stellar contract invoke --id <CONTRACT_ID> --network testnet --source <ANY_ADDRESS> \
  -- get_pending_admin_actions
```

Use this window to:

- Announce the upgrade to users/integrators (new error codes, new entry
  points — see [docs/error-codes.md](error-codes.md) and
  [docs/events.md](events.md) if either changed).
- Re-verify the installed WASM hash matches what you intended to ship
  (`stellar contract install` is deterministic for the same source + toolchain).
- Confirm the WASM size is within CI's `MAX_WASM_SIZE_KB=100` budget (see
  [DEPLOYMENT.md's WASM Size Management](../DEPLOYMENT.md#-wasm-size-management)).

There is **no cancel entry point** for a queued timelocked action today — if
you queued the wrong hash, your only recourse is to let it sit unexecuted
(never call `execute_timelocked_action` for that `action_id`) and queue a
corrected proposal. The bad proposal stays visible forever in
`get_pending_admin_actions` until executed, so don't rely on it "expiring."

### 5. Execute once `execute_after` has passed

```bash
stellar contract invoke --id <CONTRACT_ID> --network testnet --source <ADMIN_SECRET> \
  -- execute_timelocked_action --admin <ADMIN_ADDRESS> --action_id "tl_3"
```

This replaces the running WASM (`env.deployer().update_current_contract_wasm`),
bumps the on-chain version string, and emits a `CONTRACT`/`UPGRADED` event
with the old and new version. Confirm it landed:

```bash
stellar contract invoke --id <CONTRACT_ID> --network testnet --source <ANY_ADDRESS> \
  -- version
```

## Admin transfer and recovery

### Standard transfer (current admin still has their key)

1. **Propose** — current admin names the successor:

   ```bash
   stellar contract invoke --id <CONTRACT_ID> --network testnet --source <CURRENT_ADMIN_SECRET> \
     -- propose_admin --current_admin <CURRENT_ADMIN_ADDRESS> --new_admin <NEW_ADMIN_ADDRESS>
   ```

   (`transfer_admin` is an identical alias for the same call, used by
   [DEPLOYMENT.md's Admin Key Rotation](../DEPLOYMENT.md#-admin-key-rotation).)

   This fails with `PendingAdminTransfer` if a transfer is already queued —
   cancel it first (step 3) if you need to change the proposed successor.

2. **Wait for the timelock** — `MIN_TIMELOCK_LEDGERS = 17_280` ledgers from
   the proposing ledger (≈24 hours at Stellar's ~5s ledger close time).
   Check status:

   ```bash
   stellar contract invoke --id <CONTRACT_ID> --network testnet --source <ANY_ADDRESS> \
     -- get_pending_admin_transfer
   # -> (new_admin_address, earliest_acceptance_ledger)
   ```

3. **Accept** (new admin, after the ledger threshold) — or **cancel**
   (current admin, any time before acceptance):

   ```bash
   # New admin claims the role:
   stellar contract invoke --id <CONTRACT_ID> --network testnet --source <NEW_ADMIN_SECRET> \
     -- accept_admin --new_admin <NEW_ADMIN_ADDRESS>
   # (claim_admin / accept_admin_transfer are aliases for the same call)

   # OR current admin aborts the proposal instead:
   stellar contract invoke --id <CONTRACT_ID> --network testnet --source <CURRENT_ADMIN_SECRET> \
     -- cancel_admin_transfer --current_admin <CURRENT_ADMIN_ADDRESS>
   ```

   `accept_admin` reverts if called by anyone other than the proposed
   `new_admin`, or before the ledger threshold.

### Recovery-key transfer (currently unreachable)

`fluxapay/src/access_control.rs` implements a *separate*, key-based recovery
path — `AccessControl::set_recovery_key` / `recovery_initiate_admin_transfer`
— intended to let a pre-registered recovery key initiate an admin transfer
without the current admin's signature (e.g. if the admin key is lost).

**As of this writing, neither `PaymentProcessor` nor `RefundManager` exposes
these as a public contract entry point** (`initialize_with_recovery`, the
function that would set the initial recovery key, is never called either).
There is currently **no way to invoke this recovery path against a deployed
contract** — do not rely on it operationally. If you need this capability,
treat wiring a `set_recovery_key` / `recovery_initiate_admin_transfer` entry
point onto `PaymentProcessor` as its own tracked change (and note that
`lib.rs`'s `pub fn` count is frozen by `scripts/check_librs_boundaries.sh` —
see [CONTRIBUTING.md §7.3](../CONTRIBUTING.md#73-librs-module-boundaries-issue-1222)
— so new entry points belong in a submodule, not `lib.rs` directly).

**Practical consequence:** today, if the current admin's key is truly lost
(not just temporarily inaccessible), there is no on-chain recovery path —
`propose_admin` / `accept_admin` both require the *current* admin's
signature. Mitigate this operationally, not on-chain, until the recovery-key
path is wired up:

- Hold the admin key in a multisig/hardware-backed custody setup rather than
  a single hot key.
- Keep a documented, rehearsed key-holder quorum and backup/escrow procedure
  outside the contract.
- Treat loss of all admin-key custodians as requiring a new contract
  deployment (with a fresh admin), not an in-place recovery.

## Rollback considerations

There is no dedicated "rollback" entry point — rolling back is just another
upgrade, subject to the same propose → timelock → execute flow and the same
`timelock_delay` (48 hours by default). Plan for that lead time; there is no
expedited path even for a bad upgrade.

- **Keep the previous WASM hash on hand.** Before executing an upgrade,
  record the currently-installed WASM hash (and keep the `.wasm` artifact
  itself) so a rollback is "queue `upgrade_contract` with the old hash," not
  a scramble to rebuild a historical commit under time pressure.
- **Storage compatibility, not just code, has to roll back cleanly.** If the
  new version wrote data in a new `#[contracttype]` shape or a new `DataKey`
  variant, rolling back the WASM does not undo those writes — the old code
  may fail to deserialize storage the new code wrote, or silently ignore
  fields it doesn't know about. Test the specific forward-then-backward
  upgrade path on testnet before you need it on mainnet, especially for any
  change touching `fluxapay/src/types.rs` / `fluxapay/src/data_keys.rs`.
- **The version string does not auto-decrement.** `execute_timelocked_action`
  always bumps the version (see `bump_version_string`) regardless of
  direction; after a rollback, call `set_version` explicitly if you need the
  on-chain version to reflect the reverted code accurately.
- **Pause first for anything riskier than a routine upgrade.** `set_global_pause`
  (admin-only, immediate, not timelocked) blocks `create_payment`,
  `verify_payment`, and `cancel_payment` while you execute a sensitive
  upgrade or rollback:

  ```bash
  stellar contract invoke --id <CONTRACT_ID> --network testnet --source <ADMIN_SECRET> \
    -- set_global_pause --admin <ADMIN_ADDRESS> --paused true --reason "scheduled upgrade"
  ```

  Remember to unpause (`--paused false`) once the upgrade is confirmed via
  `version`.
- **A queued-but-wrong proposal cannot be cancelled** (see
  [step 4 above](#4-wait-for-the-timelock-verifying-along-the-way)) — only
  left unexecuted. Audit `get_pending_admin_actions` after any rollback to
  confirm no stale proposal (yours or an attacker's) is still sitting there
  waiting for someone to execute it.
