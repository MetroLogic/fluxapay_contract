# Storage-exhaustion review (issue #1170)

## Unbounded maps and their strategy

| Storage index | Key | Strategy |
| --- | --- | --- |
| `Payment` | payment id | Entries are TTL-bumped on creation/status change (`bump_payment_ttl`); creation is gated by the per-merchant cap below. |
| `MerchantPayments` | merchant address | **Capped** at `MAX_PAYMENTS_PER_MERCHANT = 10_000` (constants.rs). `create_payment` returns `Error::StorageEntryLimitExceeded` (code 1210) before writing once the cap is reached. |
| `MerchantPaymentCount` | merchant address | Count cannot exceed the same cap; the test-helper registration paths stop incrementing at the cap. |
| `FeeCollectionHistory` | single key (ring buffer) | Trimmed to `FEE_COLLECTION_HISTORY_CAP` on write. |
| `TreasuryWithdrawalHistory` | single key (ring buffer) | Trimmed to `TREASURY_WITHDRAWAL_HISTORY_CAP` on write. |
| Idempotency keys | token hash | Written with a bounded TTL tied to the payment/expiry window. |

## Rate-limit coverage review

Existing request-frequency controls (`CREATE_PAYMENT_MAX_PER_WINDOW`,
dispute rate limits, merchant amount limits) remain in place. They limit how
fast entries are created but not the total; the 10,000-entry cap closes the
total-growth gap. Rate limiting is applied on the creation paths, and caps are
checked before any persistent write or event emission.

## Tests

`fluxapay/src/storage_cap_test.rs` seeds the per-merchant index to the cap and
asserts that `create_payment` rejects the next entry with
`StorageEntryLimitExceeded`.

Note: the repository currently contains pre-existing compile errors in
unrelated test modules, so the full test binary could not be built here.
