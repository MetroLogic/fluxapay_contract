# Merchant Rolling Reserves

This document describes the tier-based rolling reserve system implemented in the
FluxaPay `MerchantRegistry` and its integration with `PaymentProcessor` settlement.

The goal of this feature is to protect merchants and the FluxaPay platform from
chargeback insolvency, fraud, and sudden dispute spikes by holding a configurable
percentage of payment volumes in a rolling reserve vault for a defined holding
window before those funds become available for withdrawal.

## Overview

Every merchant registered in the `MerchantRegistry` is assigned a KYC tier. Each
tier maps to a `RollingReservePolicy` that defines:

- `reserve_bps` — the percentage of each payment held in reserve, expressed in
  basis links (1 bps = 0.01%).
- `holding_period_ledgers` — the number of ledgers (or seconds, depending on
  the configuration) that a reserve bucket must remain locked before it can be
  released.

The default tier policies are:

| KYC Tier | Reserve (bps) | Holding Period (ledgers) | Notes |
|----------|----------------|--------------------------|--------|
| Tier 0   | 1500 (15%)     | 100                      | New/unverified merchants |
| Tier 1   | 1000 (10%)     | 70                       | Basic KYC complete |
| Tier 2   | 500 (5%)       | 30                       | Verified merchants |
| Tier 3   | 0 (0%)         | 0                        | Fully trusted, immediate settlement |

Higher tiers qualify for lower reserve percentages and shorter holding windows. Tier 3
payments are settled immediately with no reserve.

## Settlement Flow

When a payment is processed by `PaymentProcessor`, the merchant's proceeds are
split into two components based on the merchant's current KYC tier:

1. **Immediately claimable funds** — the remainder after the reserve deduction.
2. **Rolling reserve deposit** — locked in a `RollingReserveBucket` with an
   unlock ledger computed as `current_ledger + holding_period_ledgers`.

Rounding is always performed in favor of the merchant (floor on the reserve
portion), so the immediate amount never underflows and the two components always
add up to the original proceeds.

## Reserve Maturity Queue

Each reserve deposit creates a `RollingReserveBucket`:

```rust
pub struct RollingReserveBucket {
    public amount: i128,
    public unlock_ledger: u32,
    public created_ledger: u32,
}
```

Buckets are kept in cronological order by `unlock_ledger`. Multiple deposits accumulate
independently and are released in the order they mature.

## API

### `RollingReservePolicy`

- `RollingReservePolicy::new(reserve_bps, holding_period_ledgers)...` — constructs a
  policy and validates that `reserve_bps` ≤ 10000 and `holding_period_ledgers` > 0.
- `RollingReservePolicy::for_tier(tier) -> Result<RollingReservePolicy, _>`
  returns the canonical policy for a KYC tier.
- `policy.split(amount) -> (i128, i128)` returns `immediate` and `reserve`
  components.

### `ReserveVault`

- `deposit(merchant, amount, unlock_ledger)` — locks `amount` into a new
  bucket for the merchant and emits `RESERVE/FUNDS_HELD`.
- `release_matured_reserves(merchant, current_ledger)` — automatically
  releases all buckets whose `holding_period_ledgers` have elapsed and
  emits `RESERVE/FUNDS_RELEASED`.
- `slash_reserve_for_dispute(merchant, amount)` — drains buckets in
  chronological order to satisfy a lost dispute payout and emits `RESERVE/FUNDS_SLASHED`.
  Callable only by authorized dispute modules.
- `get_merchant_reserve_balance(merchant)` — returns a `MerchantReserveBalance`
  containing `total_locked`, `matured`, `upcoming` schedule, and the raw bucket list.

## Events

| Event topic              | Payload |
|------------------------|---------|
| `RESERVE/FUNDS_HELD`    | merchant, amount, unlock_ledger |
| `RESERV/FUNDS_RELEASED` | merchant, amount, bucket_count |
| `RESERVE/FUNDS_SLASHED`  | merchant, amount, reason |

## Tier Upgrades

When a merchant is upgraded to a higher KYC tier, new payments are split using the
new tier's policy. Existing buckets retain their original holding period and are
released on their scheduled ledger. This ensures that tier upgrades do not
retroactively unlock funds that were already committed to the reserve.

## Dispute Slashing

When a dispute is lost, an authorized dispute module calls `slash_reserve_for_dispute`.
The vault drains buckets in chronological order (oldest first) until the requested
amount is satisfied or the reserve is exhausted. The function returns the amount
actually slashed, which may be lower than the requested amount if the reserve is
largely matured or already released.

## Security Considerations

- Reserve buckets are immutable once created; only maturity release or authorized
  dispute slashing can remove funds.
- Release is idempotent: calling `release_matured_reserves` multiple times at the
  same ledger returns zero after the first call.
- Slashing is capped at the available locked balance; it never overdraws a
  merchant's reserve.
- Rounding is always in favor of the merchant to prevent dust losses.

## Testing

Unit and edge-case tests live in `fluxapay/src/merchant_reserve_test.rs` and cover:

- multi-bucket accumulation,
- chronological release and idempotency,
- dispute slashing (including capping),
- tier upgrades,
- event emission,
- query endpoints for balance and upcoming releases.

Run the suite with:

```bash
cargo test -p fluxapay --test merchant_reserve_test
```
