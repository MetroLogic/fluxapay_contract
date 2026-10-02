# Payment Creation Spam Mitigation

## Decision

Chosen approach: **Option B (policy limit)** on `create_payment` in `PaymentProcessor`.

- Limit: `30` successful `create_payment` calls per merchant per `60s` window.
- Enforcement: on-chain state in `DataKey::MerchantRateLimit(Address)`.
- Error on limit: `RateLimitExceeded`.

## Why This Approach

- No additional token transfer UX at checkout.
- Deterministic and simple to enforce on-chain.
- Prevents unbounded `PaymentCharge` growth from a single merchant key.

## Trade-offs

- Better UX than fee-gating (no extra fee handling).
- Weaker Sybil resistance than fee-gating (attacker can rotate merchant accounts).
- Fixed limits may throttle legitimate burst traffic.

## Future Hardening

- Tune limits by KYC tier.
- Add optional fee-gate for high-risk or low-trust merchants.
- Move to sliding-window or weighted quotas if needed.
