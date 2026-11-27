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

Configure the governance token before opening proposals. The first vote freezes the total registered stake weight used as that proposal's quorum denominator.

### `set_suspension_governance_token`
Configures the token that must be locked to vote on merchant suspension proposals.

- **Parameters:** `admin: Address`, `token_address: Address`
- **Authorization:** Admin role.

### `set_suspension_quorum_bps`
Sets the approval quorum in basis points. The default is `5100` (51%).

- **Parameters:** `admin: Address`, `quorum_bps: u32` (1–10,000)
- **Authorization:** Admin role.

### `set_suspension_proposal_ttl`
Sets the proposal lifetime in seconds. The default is seven days.

- **Parameters:** `admin: Address`, `ttl_secs: u64` (must be greater than zero)
- **Authorization:** Admin role.

### `propose_merchant_suspension`
Creates a suspension proposal. The proposer must hold the `ARBITRATOR` role or have at least the minimum governance-token stake.

- **Parameters:** `proposer: Address`, `merchant_id: Address`, `reason: String`
- **Return Type:** `Result<u64, MerchantError>`
- **Authorization:** Proposer authentication.
- **Events Emitted:** `(MERCHANT, SUSPENSION_PROPOSED)`

### `lock_suspension_stake`
Locks governance tokens for a proposal. Each voter may lock once per proposal; weight is `amount / VOTE_WEIGHT_UNIT`. The minimum lock is `MIN_ARBITRATOR_STAKE`. Stake registration closes when the first vote is cast, fixing the proposal's quorum denominator.

- **Parameters:** `voter: Address`, `proposal_id: u64`, `token_address: Address`, `amount: i128`
- **Authorization:** Voter authentication.

### `vote_merchant_suspension`
Casts one approve or reject vote using the voter's locked stake weight.

- **Parameters:** `voter: Address`, `proposal_id: u64`, `approve: bool`
- **Authorization:** Voter authentication and a locked stake for the proposal.

### `execute_merchant_suspension`
Executes a proposal once approve weight reaches its configured quorum. Anyone may execute; the proposal must not have expired.

- **Parameters:** `proposal_id: u64`
- **Return Type:** `Result<(), MerchantError>`
- **Events Emitted:** `(MERCHANT, SUSPENDED_BY_VOTE)` and `(MERCHANT, SUSPENDED)`

### `release_suspension_stake`
Returns a voter's locked governance tokens after the proposal is executed or expires.

- **Parameters:** `voter: Address`, `proposal_id: u64`
- **Authorization:** Voter authentication.

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

---

## Scoped API Keys (Issue #854)

Merchants can issue granular, scoped API keys to restrict integrations or services to specific capabilities.

### Supported Scopes
- `read:payments`: Read payment statuses, transaction histories, and disputes.
- `write:payments`: Create payment intents and charge customer authorizations.
- `read:analytics`: Query event aggregations and payment analytics.
- `manage:webhooks`: Register, rotate, and delete webhook destinations.
- `admin`: Super-admin wildcard granting all scopes.

### Error Response for Unauthorized Scopes
When an API key lacks the required scope for an endpoint, indexer services return HTTP `403 Forbidden`:
```json
{
  "error": "Insufficient Scope",
  "required_scope": "read:payments"
}
```

### Contract Methods

#### `create_api_key`
Creates a new scoped API key record identified by its SHA-256 key hash.
- **Parameters:**
  - `merchant: Address` — Merchant address (must sign transaction).
  - `key_hash: BytesN<32>` — SHA-256 hash of the API key secret.
  - `scopes: Vec<String>` — Array of allowed scope strings.
- **Return Type:** `Result<ApiKeyRecord, MerchantAuthError>`
- **Authorization:** Merchant signature required.

#### `get_api_key`
Retrieves an API key record by its key hash.
- **Parameters:**
  - `key_hash: BytesN<32>` — SHA-256 hash of the API key secret.
- **Return Type:** `Result<ApiKeyRecord, MerchantAuthError>`
- **Authorization:** Public read.

#### `revoke_api_key`
Revokes an existing API key, permanently disabling it.
- **Parameters:**
  - `merchant: Address` — Merchant address (must sign transaction).
  - `key_hash: BytesN<32>` — Key hash to revoke.
- **Return Type:** `Result<(), MerchantAuthError>`
- **Authorization:** Merchant signature required.
