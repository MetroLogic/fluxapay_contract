# ADR-0003: KYC Tier System

- Status: Accepted
- Date: 2026-08-29

## Context

FluxaPay processes merchant payments, refunds, and settlement flows at scale, and the protocol needs a way to classify merchants by compliance and operational risk. A single “verified / unverified” signal is too coarse: it cannot represent meaningful differences in merchant volume, settlement exposure, or onboarding risk.

The contract therefore models a four-tier KYC ladder:

- `Unverified`
- `Basic`
- `Full`
- `Business`

This model exists to balance three competing goals:

1. Protect the protocol from excessive volume or fraud exposure from lightly vetted merchants.
2. Keep onboarding friction low for smaller merchants and early-stage businesses.
3. Allow higher-volume or more trusted merchants to access broader payment capabilities without imposing unnecessary friction on all users.

The actual tier caps and auto-upgrade thresholds are defined in `fluxapay/src/constants.rs` and enforced in `fluxapay/src/lib.rs` / `fluxapay/src/payment_processor.rs` via the `MerchantMonthlyVolume` and `MerchantCumulativeVolume` checks.

## Decision

Adopt a four-tier KYC system with increasing monthly payment caps and cumulative auto-upgrade criteria.

### Tier caps

The current contract values are:

| Tier | Monthly cap (USDC) | Contract constant |
|------|--------------------|------------------|
| `Unverified` | $500 | `TIER_CAP_UNVERIFIED` |
| `Basic` | $10,000 | `TIER_CAP_BASIC` |
| `Full` | $100,000 | `TIER_CAP_FULL` |
| `Business` | Unlimited | `TIER_CAP_BUSINESS` |

These values are stored as stroops, using the standard stablecoin denomination in the codebase:

- $500 = `5_000_000_000` stroops
- $10,000 = `100_000_000_000` stroops
- $100,000 = `1_000_000_000_000` stroops
- Business = `i128::MAX` (effectively unlimited)

### Why these amounts

The caps are intentionally stepped to reflect a compliance progression rather than a flat fee or rate limit:

- `Unverified` merchants are permitted only a small risk envelope before they need additional verification.
- `Basic` merchants can scale into a moderate processing footprint and are expected to complete more complete verification as they grow.
- `Full` merchants are trusted for enterprise-scale volume under normal merchant risk controls.
- `Business` merchants are treated as high-trust, higher-volume counterparties and are not blocked by a monthly limit in the default configuration.

This gives the protocol a strong defense-in-depth posture: the default values are conservative enough to contain exposure while still allowing straightforward commercial use for small and medium merchants.

## Auto-upgrade thresholds

The contract does not require a manual admin action to advance merchants through the tiers. Instead, it upgrades a merchant automatically when cumulative historical volume crosses the transition threshold.

The thresholds are:

| Upgrade | Cumulative volume trigger |
|---------|---------------------------|
| `Unverified -> Basic` | $500 |
| `Basic -> Full` | $10,000 |
| `Full -> Business` | $100,000 |

The code expresses this as:

- `TIER_UPGRADE_THRESHOLD_BASIC = TIER_CAP_UNVERIFIED`
- `TIER_UPGRADE_THRESHOLD_FULL = TIER_CAP_BASIC`
- `TIER_UPGRADE_THRESHOLD_BUSINESS = TIER_CAP_FULL`

The actual logic is implemented in `maybe_upgrade_kyc_tier`, which reads the merchant’s cumulative volume from `MerchantCumulativeVolume` and upgrades them when they cross the threshold.

### Why cumulative volume

Cumulative thresholds align better with operational trust than a strict monthly snapshot alone:

- A merchant who has been consistently processing legitimate volume over time should be able to graduate to a higher tier without manual review friction every month.
- The protocol still retains a monthly cap as a risk control, so a newly upgraded merchant cannot instantly become a much larger exposure in a single month.
- The combination of cumulative historical volume and per-month caps creates a layered risk model: “trust over time” plus “hard monthly exposure ceiling.”

## Monthly rolling window

The monthly cap is enforced using a month bucket keyed by the ledger timestamp:

```rust
let month_epoch = (env.ledger().timestamp() / 2_592_000) as u32;
let key = DataKey::MerchantMonthlyVolume(merchant_id.clone(), month_epoch);
```

This means the protocol tracks a merchant’s rolling monthly volume against the current calendar month bucket and rejects a payment if adding the payment would exceed the merchant’s tier cap for that bucket.

### Reset behavior

The monthly counter resets naturally when the merchant crosses into a new month bucket. No explicit admin reset is required, and the data is stored under the merchant + month key rather than a single shared aggregate.

This design makes the cap easy to reason about:

- current month volume is always available,
- per-merchant history is preserved in a compact key scheme,
- the monthly cap remains isolated from other risk or settlement systems.

## Consequences

### Benefits

- Clear risk ladder with a straightforward path from low-volume to high-volume merchant onboarding.
- Conservative default caps reduce protocol exposure for under-verified accounts.
- Auto-upgrade based on cumulative volume reduces manual friction without abandoning compliance gates.
- Monthly cap enforcement ensures a newly upgraded merchant cannot bypass risk controls by immediately pushing large monthly volume.

### Costs

- Some merchants will encounter friction as they cross tier thresholds or hit monthly caps.
- The protocol is intentionally conservative: lower tiers may require operational steps before higher processing limits are available.
- Tier progression is based on transaction history, which means merchants with legitimate but bursty growth may still face broader review if they exceed risk ceilings abruptly.

## Alternatives considered

### 1. Flat rate limiting for all merchants

A single global cap or a single rate limit would be simpler to reason about, but it would be too blunt for a marketplace with varying compliance and risk posture. It would create a poor UX for low-risk merchants and inadequate protection for higher-risk merchants.

### 2. Off-chain KYC bridge only

Routing all tier decisions to an external KYC provider or off-chain compliance data source would reduce on-chain complexity, but it would create a stronger dependency on an external system and make the on-chain payment cap logic harder to audit or reason about deterministically. The current model keeps major trust boundaries visible on chain and aligned with actual payment behavior.

## Related implementation references

- `fluxapay/src/constants.rs` — `TIER_CAP_*` and `TIER_UPGRADE_THRESHOLD_*`
- `fluxapay/src/lib.rs` — monthly cap enforcement and auto-upgrade logic
- `fluxapay/src/merchant_registry.rs` — `KycTier` enum and merchant tier model

This ADR intentionally documents the design the contract currently enforces, so future changes to tier policy can be evaluated against a clear historical rationale.
