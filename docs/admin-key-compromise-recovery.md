# Admin key compromise recovery runbook (Issue #1173)

This runbook covers loss or compromise of the `ADMIN` key for a deployed FluxPay
contract. The recovery path deliberately **cannot** be completed quickly: it is
protected by a 30-day lock-in so the community can detect and contest a
malicious recovery.

## Key facts

- `initialize_with_recovery(admin, recovery_key)` sets the admin and an offline
  recovery address at deployment.
- `set_recovery_key(admin, recovery_key)` rotates the recovery key while the
  admin key is still trusted.
- `recovery_initiate_admin_transfer(recovery_key, new_admin)` proposes a new
  admin **without** the compromised admin key. It records the earliest
  acceptance ledger as current ledger + 518,400 (~30 days at ~5s/ledger) and
  sets `AdminTransferLockIn` to 2,592,000 seconds (30 days).
- `accept_admin(new_admin)` completes the transfer only after that ledger
  threshold. Acceptance before then panics ("Admin transfer timelock has not
  expired"); the pending transfer is cleared only on success.
- The recovery key is not changed by the transfer, so it can be used again
  after a future incident. The old admin's `ADMIN` role is revoked.

## Drill / incident procedure

1. **Detect and freeze.** Confirm the compromise, pause affected payment
   scopes using the existing pause flow, and notify stakeholders. Move any
   remaining operational funds using pre-agreed emergency procedures.
2. **Confirm the recovery key.** Verify `get_recovery_key()` matches the
   offline key held by the incident responder. If the admin key is still
   usable, rotate it first with `set_recovery_key`. If the recovery key is
   also compromised, this path cannot be used; follow the governance
   escalation in `SECURITY.md`.
3. **Initiate.** From the recovery key, submit
   `recovery_initiate_admin_transfer(recovery_key, new_admin)`. Record the
   emitted `ACCESS_CONTROL / RECOVERY_ADMIN_TRANSFER_PROPOSED` event and the
   pending `(new_admin, earliest_ledger)` from `get_pending_admin_transfer()`.
4. **Watch the 30-day lock-in.** Monitor `get_admin_transfer_lock_in()` (must
   equal 2,592,000 seconds) and the pending earliest ledger. Attempting
   `accept_admin` early must fail. Use this window to verify the new admin's
   identity out-of-band and publish the incident notice. The same timelock
   acts as the cooldown between proposal and acceptance; the normal admin
   transfer path uses the shorter 17,280-ledger timelock and is not a valid
   substitute for a compromised-key recovery.
5. **Accept and verify.** Once `ledger sequence >= earliest_ledger`, call
   `accept_admin(new_admin)` (auth from the new admin). Verify `get_admin()`
   is the new address, `get_pending_admin_transfer()` is empty, the old admin
   no longer holds `ADMIN`, and `get_recovery_key()` is unchanged.
6. **Post-incident.** Rotate the recovery key with the new admin, rotate all
   downstream API keys, and review the `ACCESS_CONTROL / ADMIN_TRANSFER_COMPLETED`
   event.

## Rehearsal

`fluxapay/src/recovery_test.rs` is the automated full-recovery drill. It
initializes with a recovery key, rotates that key, initiates a transfer,
asserts the 30-day lock-in and early-acceptance rejection, advances the ledger
past the lock-in, completes the transfer and verifies final state. Run it with:

```sh
cargo test -p fluxapay recovery_test::test_full_admin_key_compromise_recovery_transfer
```

Re-run the drill on every release that touches `access_control.rs`.
