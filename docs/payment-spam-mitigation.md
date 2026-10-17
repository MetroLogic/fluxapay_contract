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

## Issue #1032: Implemented vs. Planned

**Implemented:**

- **Deposit address reuse prevention (contract level).** `create_payment`
  rejects a `deposit_address` that is already assigned to another open
  (`Pending`, unexpired) payment with `Error::DepositAddressInUse`. The check
  is lazy: an expired or no-longer-`Pending` payment automatically frees its
  address for reuse, with no explicit release step required. See
  `DataKey::DepositAddressPending` in `data_keys.rs` and the check at the top
  of `PaymentProcessor::create_payment`.
- **Per-IP rate limiting at the API level.** The indexer's
  `createRateLimitMiddleware` (`indexer/src/rate-limit.ts`) now applies a
  dedicated, always-per-IP budget — default `30` requests per `60s`
  (`RATE_LIMIT_PAYMENT_CREATE_RPW` / `RATE_LIMIT_PAYMENT_CREATE_WINDOW_SECS`),
  matching `CREATE_PAYMENT_MAX_PER_WINDOW` / `CREATE_PAYMENT_WINDOW_SECS` — to
  payment-creation-adjacent routes (currently `POST
  /v1/payment-links/:id/view`, the only pre-auth, payment-related POST route
  this indexer exposes).

**Not applicable as originally scoped:** the issue described rate-limiting
`POST /v1/payments`, but this indexer has no such route — payment creation is
an on-chain `PaymentProcessor::create_payment` call made directly against the
Soroban contract, not a REST endpoint proxied through the indexer. The
per-merchant window described above (Option B) is the on-chain equivalent,
and the new per-IP budget is wired so that if a `POST /v1/payments`
convenience endpoint is ever added to the indexer, it only needs to be added
to `PAYMENT_CREATE_ROUTES` in `rate-limit.ts` to inherit the same protection.

**Still planned:**

- Tune limits by KYC tier (unchanged from above).
- Optional fee-gate for high-risk/low-trust merchants (unchanged from above).
