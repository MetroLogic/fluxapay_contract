# MerchantRegistry API Reference

Complete entry point reference for the `MerchantRegistry` contract in FluxaPay.

---

## Core Registration & Verification

### `register_merchant`
Registers a new merchant in the system with an initial KYC tier of `Unverified`.

- **Parameters:**
  - `merchant_id: Address` — Public key / address of the merchant.
  - `payout_address: Address` — Stellar address where settled funds will be disbursed.
  - `metadata: String` — JSON or string metadata (business details, contact info).
- **Return Type:** `Result<(), MerchantError>`
- **Authorization:** `merchant_id.require_auth()`
- **Events Emitted:** `(MERCHANT, REGISTERED, merchant_id)`

---

### `update_merchant`
Updates existing merchant payout address or profile metadata.

- **Parameters:**
  - `merchant_id: Address` — Merchant address.
  - `payout_address: Option<Address>` — Optional new payout address (subject to 48h change cooldown).
  - `metadata: Option<String>` — Optional updated metadata string.
- **Return Type:** `Result<(), MerchantError>`
- **Authorization:** `merchant_id.require_auth()`

---

### `verify_merchant`
Admin entry point to mark a merchant as verified (upgrading status to Active).

- **Parameters:**
  - `admin: Address` — Admin address.
  - `merchant_id: Address` — Merchant to verify.
- **Return Type:** `Result<(), MerchantError>`
- **Authorization:** Requires Admin role (`admin.require_auth()`)
- **Events Emitted:** `(MERCHANT, VERIFIED, merchant_id)`

---

### `verify_merchant_with_signature`
Verifies a merchant via signed cryptographic payload from an authorized verifier off-chain.

- **Parameters:**
  - `merchant_id: Address` — Target merchant address.
  - `signature: BytesN<64>` — Ed25519 signature from verifier.
  - `nonce: u64` — Single-use replay protection nonce.
- **Return Type:** `Result<(), MerchantError>`
- **Authorization:** Signature payload validation against authorized verifier key.

---

## KYC & Tier Management

### `set_kyc_tier`
Manually sets the KYC tier of a merchant.

- **Parameters:**
  - `admin: Address` — Admin address.
  - `merchant_id: Address` — Target merchant address.
  - `tier: KycTier` — New tier (`Unverified`, `Tier1`, `Tier2`, `Tier3`).
- **Return Type:** `Result<(), MerchantError>`
- **Authorization:** Requires Admin role (`admin.require_auth()`)

---

### `auto_upgrade_kyc_tier`
Evaluates cumulative processing volume for a merchant and automatically upgrades their tier if threshold is reached.

- **Parameters:**
  - `merchant_id: Address` — Target merchant address.
- **Return Type:** `Result<KycTier, MerchantError>`
- **Authorization:** Public / contract internal trigger.

---

## Fees & Platform Config

### `set_fee_config`
Configures custom percentage and flat fee structure for platform fee extraction.

- **Parameters:**
  - `admin: Address` — Admin address.
  - `fee_bps: i128` — Fee basis points (e.g. 50 = 0.5%).
  - `flat_fee: i128` — Flat fee amount per transaction in token base units.
  - `recipient: Address` — Fee recipient treasury address.
- **Return Type:** `Result<(), MerchantError>`
- **Authorization:** Requires Admin role (`admin.require_auth()`)

---

### `calculate_platform_fee`
Calculates platform fee split for a target payment amount.

- **Parameters:**
  - `amount: i128` — Transaction total amount.
- **Return Type:** `(i128, Address)` — Tuple of `(fee_amount, recipient_address)`.
- **Authorization:** Read-only / public query.

---

## Whitelist Management

### `add_to_whitelist`
Adds an address to a merchant's allowed payer whitelist.

- **Parameters:**
  - `merchant_id: Address` — Merchant address.
  - `target_address: Address` — Customer address to whitelist.
- **Return Type:** `Result<(), MerchantError>`
- **Authorization:** `merchant_id.require_auth()`

---

### `is_address_whitelisted`
Checks if an address is on a merchant's whitelist.

- **Parameters:**
  - `merchant_id: Address` — Merchant address.
  - `target_address: Address` — Customer address to check.
- **Return Type:** `bool`
- **Authorization:** Read-only / public query.

---

## Suspension & Reinstatement

### `suspend_merchant`
Suspends a merchant due to compliance, risk, or policy violations.

- **Parameters:**
  - `authority: Address` — Admin or Settlement Operator address.
  - `merchant_id: Address` — Target merchant.
  - `reason: String` — Detailed suspension reason.
- **Return Type:** `Result<(), MerchantError>`
- **Authorization:** Admin or Settlement Operator authentication.
- **Events Emitted:** `(MERCHANT, SUSPENDED, merchant_id, reason)`

---

### `reinstate_merchant`
Reinstates a suspended merchant to active status.

- **Parameters:**
  - `authority: Address` — Admin address.
  - `merchant_id: Address` — Target merchant.
- **Return Type:** `Result<(), MerchantError>`
- **Authorization:** Admin authentication.
- **Events Emitted:** `(MERCHANT, REINSTATED, merchant_id, reinstated_by)`

---

## Pagination & Lookup

### `get_all_merchants`
Fetches a paginated list of registered merchant records.

- **Parameters:**
  - `env: Env` — Soroban environment.
  - `offset: u32` — Zero-based offset.
  - `limit: u32` — Maximum records to return.
- **Return Type:** `Vec<Merchant>`
- **Authorization:** Read-only / public query.
