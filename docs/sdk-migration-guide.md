# SDK Migration Guide

This guide walks through the breaking changes between major versions of
`@fluxapay/sdk` and how to update your integration. For the full list of
changes in each release, see [`sdk/CHANGELOG.md`](../sdk/CHANGELOG.md).

The SDK is currently pre-1.0 (`0.x`), so the API can still change between
minor versions. The sections below are organized by the version boundary
they apply to: the changes already shipped on `main` that will be part of
the `1.0.0` release, and the changes planned for `2.0.0`. Update this guide
each time a major version actually ships, moving its section from "planned"
to a dated release entry.

## Table of contents

- [v0.x → v1.0](#v0x--v10)
- [v1.x → v2.0 (planned)](#v1x--v20-planned)
- [General migration principles](#general-migration-principles)
- [Testing your migration](#testing-your-migration)

## v0.x → v1.0

### `createPayment` gained `metadata` and `fee_waiver_code`

`CreatePaymentArgs` grew two new optional fields. Both are additive and
`undefined`/omitted by default, so existing calls keep working — but if you
were passing a positional-style object literal without a type annotation,
add the new fields explicitly once you upgrade so TypeScript can catch
typos in your metadata keys.

**Before (v0.x):**

```typescript
const payment = await client.createPayment({
  paymentId: "pay_123",
  merchantId: "G...",
  amount: 1000000n,
  currency: "USDC",
  depositAddress: "G...",
  expiresAt: BigInt(Math.floor(Date.now() / 1000) + 3600),
  memo: "Order #42",
});
```

**After (v1.0):**

```typescript
const payment = await client.createPayment({
  paymentId: "pay_123",
  merchantId: "G...",
  amount: 1000000n,
  currency: "USDC",
  depositAddress: "G...",
  expiresAt: BigInt(Math.floor(Date.now() / 1000) + 3600),
  memo: "Order #42",
  // New in v1.0: arbitrary merchant-supplied key/value metadata.
  // Limits: <=20 keys, key <=64 chars, value <=256 chars.
  // On-chain errors if exceeded: MetadataTooLarge (#49), MetadataValueTooLong (#47).
  metadata: { orderId: "42", source: "web-checkout" },
  // New in v1.0: redeem an admin-issued fee waiver code for this payment.
  // Invalid/expired/exhausted codes are ignored by settle_payment, not rejected
  // at create_payment time.
  feeWaiverCode: "LAUNCH2026",
});
```

Nothing to change if you don't use these fields — omit them and behavior is
identical to v0.x.

### `PaymentCharge` gained `payment_link_id`

Payments created through `use_link` now carry a `payment_link_id` field so
you can trace a `PaymentCharge` back to the payment link that generated it
(issue #668). Payments created directly via `createPayment` /
`swap_and_pay` have `payment_link_id: undefined`.

**Before (v0.x):** no way to tell which link (if any) produced a payment
without separately calling `getLinkPayments` for every link and
cross-referencing payment IDs.

**After (v1.0):**

```typescript
const payment = await client.getPayment("lnk_pay_123");

if (payment.payment_link_id) {
  console.log(`Payment ${payment.payment_id} came from link ${payment.payment_link_id}`);
} else {
  console.log(`Payment ${payment.payment_id} was created directly (no link)`);
}
```

No code changes are required to keep working — this is a new, optional
field. Update TypeScript types that narrow `PaymentCharge` with an object
literal type (rather than importing the SDK's `PaymentCharge` interface) to
include `payment_link_id?: string`.

## v1.x → v2.0 (planned)

> These changes are not yet released. This section documents the intended
> shape of the v2.0 upgrade so integrators can prepare; update it with real
> before/after snippets once v2.0 actually ships and remove this notice.

### `FeeConfig` restructuring

`FeeConfig` currently looks like:

```typescript
export interface FeeConfig {
  platform_fee_bps: i64;
  fixed_fee: i128;
  fee_recipient: Option<string>;
}
```

v2.0 is expected to split the flat `fee_recipient` into a structured
recipient descriptor (to support routing a fee split across treasury and
developer addresses, mirroring the on-chain `FeeSplitConfig`). Plan to:

- Replace direct reads of `feeConfig.fee_recipient` with a helper that
  falls back to the merchant's admin address when unset.
- Re-check any code that constructs a `FeeConfig` literal (e.g. in tests or
  admin tooling) against the new shape once it lands.

### `FluxapayError` becomes the primary error type

The SDK already wraps contract errors in a `FluxapayError` (see
`sdk/src/index.ts`), which exposes `code`, `contractErrorName`, and the
original `cause`:

```typescript
try {
  await client.createPayment(args);
} catch (err) {
  if (err instanceof FluxapayError) {
    console.error(err.contractErrorName, err.code);
  }
}
```

v2.0 is expected to make `FluxapayError` the *only* error type thrown by
`FluxapayClient` methods (today, non-contract errors — network failures,
malformed responses — can still surface as plain `Error`s). Once that
lands, `instanceof FluxapayError` checks become safe to use without an
`instanceof Error` fallback branch.

## General migration principles

- **Always check errors via the contract error map, not string matching.**
  Use `err instanceof FluxapayError` and switch on `err.contractErrorName`
  or `err.code` rather than parsing `err.message`. The mapping lives in
  `FLUXAPAY_CONTRACT_ERROR_MAP` (`sdk/src/index.ts`) and is kept in sync
  with the contract's `Error` enum by `scripts/check-error-map-sync.ts`.
- **Treat new optional fields as additive.** Every breaking-in-spirit
  change the SDK makes to request/response shapes is landed as an
  `Option`/optional field first. Code that doesn't read the new field
  keeps compiling and behaving the same; only code that does exhaustive
  object-shape checks (e.g. `Object.keys(payment).length === N`) needs
  updating.
- **Pin your SDK version** in `package.json` (avoid `^0.x` ranges) until
  you've read the changelog for the next major release — pre-1.0 semver
  does not guarantee backwards compatibility across minor versions.
- **Re-generate contract bindings after a contract upgrade.** If you
  vendor `sdk/src/contracts/fluxapay/src/index.ts` directly instead of
  depending on `@fluxapay/sdk`, re-run `scripts/generate-sdk.sh` after any
  contract change that adds fields to a `#[contracttype]` struct — hand
  edits will drift from the deployed contract's actual ABI.

## Testing your migration

Before deploying a version bump, re-run this matrix against testnet:

| Area | What to test |
| --- | --- |
| Payment creation | `createPayment` with and without the new optional fields (`metadata`, `feeWaiverCode`) |
| Payment link payments | `useLink` → `getPayment` round-trip; assert `payment_link_id` matches the link used, and is `undefined` for a payment created via `createPayment` |
| Error handling | Trigger at least one known contract error (e.g. duplicate `paymentId`) and assert it surfaces as `FluxapayError` with the expected `contractErrorName` |
| Fee configuration | Read back `FeeConfig` / `MaybeFeeConfig` for a merchant and assert your code handles both `None` and `Some` variants |
| Type-level check | `tsc --noEmit` against your integration code with the new SDK version installed, to catch any narrowed/duplicated local type definitions that fell out of sync |

If you maintain your own mirror of the generated contract types, diff them
against `sdk/src/contracts/fluxapay/src/index.ts` on every SDK bump.
