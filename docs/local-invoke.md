# Local Testnet Invoke Recipes

This guide shows how to invoke Fluxapay contract functions locally on Stellar testnet using the Stellar CLI.

## Prerequisites

1. **Install Stellar CLI**: [stellar-cli](https://github.com/stellar/rs-soroban-cli)
2. **Set up environment variables**: Copy `.env.example` to `.env` and populate with your testnet values:
   ```bash
   cp .env.example .env
   ```
3. **Generate test keypairs** (if needed):
   ```bash
   stellar keys generate --name test-admin
   stellar keys generate --name test-merchant
   stellar keys generate --name test-customer
   ```
4. **Fund test accounts** on testnet via the [Stellar Friendbot](https://friendbot.stellar.org/)

---

## Setup & Configuration

### Load Environment Variables

```bash
# Load from .env (supported by many shells)
export $(cat .env | grep -v '#' | xargs)

# Or load manually for your shell
source .env        # bash/zsh
set -a; source .env; set +a  # sh
```

### Verify Network Configuration

```bash
# Check testnet connectivity
stellar contract info interface \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet
```

---

## Core Contract Functions

### 1. Register Merchant

Register a new merchant on the Merchant Registry contract. The merchant must authenticate and provide KYC details.

#### Function Signature

```rust
pub fn register_merchant(
    env: Env,
    merchant_id: Address,           // Merchant's Stellar address
    business_name: String,          // Legal business name
    settlement_currency: String,    // e.g. "USD", "EUR", "NGN"
    payout_address: Option<Address>,// Optional payout wallet address
    bank_account: Option<String>,   // Optional bank account reference
) -> Result<(), MerchantError>
```

#### Invoke Command

```bash
stellar contract invoke \
  --id $MERCHANT_REGISTRY_ID \
  --network testnet \
  --source $TEST_MERCHANT_ADDRESS \
  -- register_merchant \
  --merchant_id $TEST_MERCHANT_ADDRESS \
  --business_name "TechCorp Nigeria Limited" \
  --settlement_currency "NGN" \
  --payout_address $ADMIN_ADDRESS \
  --bank_account "0123456789"
```

#### Expected Output

Success:
```json
{
  "status": "success"
}
```

#### Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `MerchantAlreadyExists` | Merchant already registered | Use a different address or verify registration |
| `Unauthorized` | Request not signed by merchant | Ensure `--source` matches `--merchant_id` |

#### Verification

After registration, verify the merchant was created:

```bash
stellar contract invoke \
  --id $MERCHANT_REGISTRY_ID \
  --network testnet \
  -- get_merchant \
  --merchant_id $TEST_MERCHANT_ADDRESS
```

---

### 2. Create Payment

Create a payment charge that a customer must fulfill by sending USDC to the deposit address.

#### Function Signature

```rust
pub fn create_payment(
    env: Env,
    payment_id: String,             // Unique payment identifier
    merchant_id: Address,           // Merchant creating the charge
    amount: i128,                   // Amount in stroops (1 USDC = 10^7 stroops)
    currency: Symbol,               // e.g. "USDC"
    deposit_address: Address,       // Where customer sends funds
    expires_at: u64,                // Unix timestamp when payment expires
    memo: Option<String>,           // Optional memo/invoice reference
    memo_type: Option<String>,      // Stellar memo type: "Text", "Id", "Hash", or "Return"
) -> Result<PaymentCharge, Error>
```

#### Pre-requisites

1. Merchant must be registered (see [Register Merchant](#1-register-merchant))
2. Merchant must be verified by admin (contact operations team for testnet)
3. Deposit address must be a valid Stellar address
4. Expiration timestamp must be in the future

#### Invoke Command

```bash
# Calculate expiration (current time + 1 hour, adjust as needed)
EXPIRES_AT=$(($(date +%s) + 3600))

stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $TEST_MERCHANT_ADDRESS \
  -- create_payment \
  --payment_id "inv_20260329_001" \
  --merchant_id $TEST_MERCHANT_ADDRESS \
  --amount 1000000000 \
  --currency USDC \
  --deposit_address $ADMIN_ADDRESS \
  --expires_at $EXPIRES_AT \
  --memo "Order-12345-USD" \
  --memo_type "Text"
```

#### Expected Output

Success returns a `PaymentCharge` object:
```json
{
  "payment_id": "inv_20260329_001",
  "merchant_id": "GXXXXXX...",
  "amount": 1000000000,
  "currency": "USDC",
  "status": "Pending",
  "created_at": 1711776000,
  "expires_at": 1711779600,
  "memo": "Order-12345-USD",
  "memo_type": "Text"
}
```

#### Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `Unauthorized` | Merchant not verified or role not granted | Contact admin to verify merchant |
| `InvalidAmount` | Amount ≤ 0 | Specify positive amount in stroops |
| `PaymentAlreadyExists` | Payment ID already used | Use a unique payment ID |
| `ContractPaused` | Contract is paused | Contact admin to unpause |
| `InvalidMemoType` | `memo_type` is not one of `Text`, `Id`, `Hash`, `Return` | Use a valid Stellar memo type |
| `MemoTooLong` | Text memo exceeds 28 bytes | Shorten the memo to ≤ 28 bytes |
| `InvalidMemoId` | Id memo is not a valid u64 decimal string | Use a numeric string e.g. `"123456789"` |

#### Field Conversion Guide

- **Amount**: Stellar uses stroops (1 USDC = 10^7 stroops)
  - 1 USDC → `10000000` stroops (7 decimal places)
  - 100 USDC → `1000000000` stroops

---

### 3. Get Payment Status

Retrieve the current status of a payment charge.

#### Function Signature

```rust
pub fn get_payment(
    env: Env,
    payment_id: String,  // Payment identifier
) -> Result<PaymentCharge, Error>
```

#### Invoke Command

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  -- get_payment \
  --payment_id "inv_20260329_001"
```

#### Expected Output

Returns the full `PaymentCharge` object with current status:
```json
{
  "payment_id": "inv_20260329_001",
  "merchant_id": "GXXXXXX...",
  "amount": 1000000000,
  "currency": "USDC",
  "status": "Confirmed",
  "amount_received": 1000000000,
  "created_at": 1711776000,
  "expires_at": 1711779600,
  "memo": "Order-12345-USD",
  "memo_type": "Text"
}
```

#### Payment Status Values

| Status | Meaning | Next Step |
|--------|---------|-----------|
| `Pending` | Awaiting payment | Customer sends USDC to deposit address |
| `Confirmed` | Payment received in full | Merchant fulfills order |
| `PartiallyPaid` | Underpayment received | Request additional funds or refund |
| `Overpaid` | Overpayment received | Refund excess or reconcile |
| `Expired` | Payment deadline passed | Issue new payment link |
| `Failed` | Payment verification failed | Troubleshoot or create new charge |

#### Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `PaymentNotFound` | Payment ID doesn't exist | Verify payment ID matches created payment |

---

## Refund Functions

### 4. Create Refund

Request a refund against a confirmed payment. The requester must authenticate.

#### Invoke Command

```bash
stellar contract invoke \
  --id $REFUND_MANAGER_ID \
  --network testnet \
  --source $TEST_MERCHANT_ADDRESS \
  -- create_refund \
  --payment_id "inv_20260329_001" \
  --refund_amount 500000000 \
  --reason "Customer requested cancellation" \
  --requester $TEST_MERCHANT_ADDRESS
```

#### Expected Output

Returns the new refund ID:
```json
"refund_1"
```

#### Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `InvalidAmount` | Amount ≤ 0 or exceeds payment amount | Use a positive amount within the original payment amount |
| `PaymentNotFound` | Payment ID does not exist | Verify the payment ID |
| `RefundExceedsPayment` | Total refunds + disputes exceed payment amount | Reduce refund amount |

---

### 5. Process Refund

Execute a pending refund and transfer USDC back to the requester. Requires `settlement_operator` or `oracle` role.

#### Invoke Command

```bash
stellar contract invoke \
  --id $REFUND_MANAGER_ID \
  --network testnet \
  --source $ADMIN_ADDRESS \
  -- process_refund \
  --operator $ADMIN_ADDRESS \
  --refund_id "refund_1"
```

#### Expected Output

```json
null
```

#### Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `Unauthorized` | Caller lacks `settlement_operator` or `oracle` role | Grant the correct role to the operator |
| `RefundNotFound` | Refund ID does not exist | Verify the refund ID from `create_refund` output |
| `RefundAlreadyProcessed` | Refund already completed or rejected | Check refund status with `get_refund` |

---

## Dispute Functions

### 6. Create Dispute

Open a dispute against a confirmed payment. The disputer must authenticate.

#### Invoke Command

```bash
stellar contract invoke \
  --id $REFUND_MANAGER_ID \
  --network testnet \
  --source $TEST_CUSTOMER_ADDRESS \
  -- create_dispute \
  --payment_id "inv_20260329_001" \
  --amount 1000000000 \
  --reason "Item not received" \
  --evidence "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG" \
  --disputer $TEST_CUSTOMER_ADDRESS
```

#### Evidence format (IPFS CID)

When `require_evidence_cid` is enabled (default), non-empty `--evidence` must be a valid IPFS multihash:

| Format | Rule | Example |
|--------|------|---------|
| CIDv0 | Starts with `Qm`, exactly 46 characters | `QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG` |
| CIDv1 | Starts with `bafy` (or `BAFY` / `f…`), length ≥ 59 (or ≥ 34 for hex `f`) | `bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi` |

Empty evidence is always allowed. For local/testnet convenience, disable enforcement:

```bash
stellar contract invoke \
  --id $REFUND_MANAGER_ID \
  --network testnet \
  --source $ADMIN_ADDRESS \
  -- set_require_evidence_cid \
  --admin $ADMIN_ADDRESS \
  --require_cid false
```

Invalid non-empty evidence returns `InvalidEvidenceFormat`.

#### Expected Output

Returns the new dispute ID:
```json
"dispute_1"
```

#### Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `PaymentNotFound` | Payment ID does not exist | Verify the payment ID |
| `PaymentAlreadyProcessed` | Payment is not in `Confirmed` state | Only confirmed payments can be disputed |
| `InvalidAmount` | Amount ≤ 0 or exceeds payment amount | Use a positive amount within the original payment amount |
| `RefundExceedsPayment` | Combined disputes + refunds exceed payment | Reduce dispute amount |
| `InvalidEvidenceFormat` | Evidence is not a CIDv0/CIDv1 IPFS multihash | Pass a CID or disable `require_evidence_cid` |
| `DisputeRateLimitExceeded` | Too many open disputes for payer or global hourly cap | Wait, resolve open disputes, or ask admin to adjust limits |

---

### 7. Set Dispute Deadline

Set a review deadline for an open dispute. Requires `settlement_operator` or `oracle` role. If the deadline passes without resolution, the dispute is automatically escalated.

#### Invoke Command

```bash
DEADLINE=$(($(date +%s) + 86400))   # 24 hours from now

stellar contract invoke \
  --id $REFUND_MANAGER_ID \
  --network testnet \
  --source $ADMIN_ADDRESS \
  -- set_dispute_deadline \
  --operator $ADMIN_ADDRESS \
  --dispute_id "dispute_1" \
  --deadline $DEADLINE
```

#### Expected Output

```json
null
```

#### Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `Unauthorized` | Caller lacks required role | Grant `settlement_operator` or `oracle` role |
| `DisputeNotFound` | Dispute ID does not exist | Verify the dispute ID |
| `DisputeAlreadyResolved` | Dispute is already resolved or rejected | No action needed |

---

### 8. Resolve Dispute with Refund

Resolve an open dispute by issuing a refund for the disputed amount. Requires `settlement_operator` or `oracle` role.

#### Invoke Command

```bash
stellar contract invoke \
  --id $REFUND_MANAGER_ID \
  --network testnet \
  --source $ADMIN_ADDRESS \
  -- resolve_dispute_with_refund \
  --operator $ADMIN_ADDRESS \
  --dispute_id "dispute_1" \
  --resolution_notes "Verified: item not delivered. Refund approved."
```

#### Expected Output

Returns the refund ID created for this resolution:
```json
"refund_2"
```

#### Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `Unauthorized` | Caller lacks required role | Grant `settlement_operator` or `oracle` role |
| `DisputeNotFound` | Dispute ID does not exist | Verify the dispute ID |
| `DisputeAlreadyResolved` | Dispute already resolved or rejected | Check dispute status with `get_dispute` |

---

## Payment Lifecycle Functions

### 9. Verify Payment

Mark a payment as confirmed after on-chain verification. Requires `oracle` role.

#### Invoke Command

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $ADMIN_ADDRESS \
  -- verify_payment \
  --oracle $ADMIN_ADDRESS \
  --payment_id "inv_20260329_001" \
  --transaction_hash "[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31]" \
  --payer_address $TEST_CUSTOMER_ADDRESS \
  --amount_received 1000000000
```

#### Expected Output

Returns the resulting payment status:
```json
"Confirmed"
```

#### Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `Unauthorized` | Caller lacks `oracle` role | Grant `oracle` role to the operator |
| `PaymentNotFound` | Payment ID does not exist | Verify the payment ID |
| `PaymentAlreadyProcessed` | Payment is not in `Pending` state | Check current status with `get_payment` |
| `PaymentExpired` | Payment deadline has passed | Create a new payment |
| `ContractPaused` | Contract is paused | Contact admin to unpause |

---

### 10. Settle Payment

Move a confirmed payment to `Settled` state and record the treasury address. Requires `settlement_operator` role.

#### Invoke Command

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $ADMIN_ADDRESS \
  -- settle_payment \
  --operator $ADMIN_ADDRESS \
  --payment_id "inv_20260329_001" \
  --treasury_address $ADMIN_ADDRESS
```

#### Expected Output

```json
null
```

#### Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `Unauthorized` | Caller lacks `settlement_operator` role | Grant the correct role |
| `PaymentNotFound` | Payment ID does not exist | Verify the payment ID |
| `PaymentAlreadyProcessed` | Payment is not in `Confirmed` state | Only confirmed payments can be settled |

---

## Admin Functions

### 11. Set Paused (Global Pause)

Pause or unpause the `PaymentProcessor` contract. While paused, `create_payment` and `verify_payment` are blocked. Requires `admin` role.

#### Invoke Command

```bash
# Pause the contract
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $ADMIN_ADDRESS \
  -- set_paused \
  --admin $ADMIN_ADDRESS \
  --paused true

# Unpause the contract
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $ADMIN_ADDRESS \
  -- set_paused \
  --admin $ADMIN_ADDRESS \
  --paused false
```

#### Expected Output

```json
null
```

#### Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `Unauthorized` | Caller lacks `admin` role | Use the admin address that initialized the contract |

---

## FX Oracle Functions

### 12. Set Rate

Publish an exchange rate for a currency pair. Requires `oracle` role on the FX Oracle contract.

#### Invoke Command

```bash
# Set USDC/NGN rate: 1 USDC = 1600 NGN
# rate is stored with `decimals` decimal places, so 1600 NGN with decimals=2 means rate=160000
stellar contract invoke \
  --id $FX_ORACLE_ID \
  --network testnet \
  --source $ADMIN_ADDRESS \
  -- set_rate \
  --operator $ADMIN_ADDRESS \
  --pair USDCNGN \
  --rate 160000 \
  --decimals 2
```

#### Expected Output

```json
null
```

#### Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `Unauthorized` | Caller lacks `oracle` role on FX Oracle | Grant `oracle` role via `oracle_grant_role` |
| `InvalidRate` | Rate ≤ 0 | Provide a positive rate value |

#### Verification

```bash
stellar contract invoke \
  --id $FX_ORACLE_ID \
  --network testnet \
  -- get_rate \
  --pair USDCNGN
```

---

## Payment Link Functions

### 13. Create Link

Create a reusable payment link for a merchant. The merchant must authenticate.

#### Invoke Command

```bash
LINK_EXPIRES=$(($(date +%s) + 604800))   # 7 days from now

stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $TEST_MERCHANT_ADDRESS \
  -- create_link \
  --merchant $TEST_MERCHANT_ADDRESS \
  --link_id "link_shop_001" \
  --amount 500000000 \
  --currency USDC \
  --description "Pay for T-shirt" \
  --expires_at $LINK_EXPIRES \
  --max_uses 100
```

> **Note:** `amount`, `expires_at`, and `max_uses` are optional. Pass `null` to omit them.

#### Expected Output

Returns the link ID:
```json
"link_shop_001"
```

#### Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `Unauthorized` | Caller is not the merchant | Ensure `--source` matches `--merchant` |

---

### 14. Use Link

Pay via a payment link. The payer must authenticate.

#### Invoke Command

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $TEST_CUSTOMER_ADDRESS \
  -- use_link \
  --payer $TEST_CUSTOMER_ADDRESS \
  --link_id "link_shop_001" \
  --amount 500000000
```

#### Expected Output

Returns a virtual payment ID for tracking:
```json
"lnk_pay_1748620800"
```

#### Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `Unauthorized` | Link is inactive | Check link status with `get_link` |
| `PaymentExpired` | Link has passed its expiry | Create a new link |
| `PaymentAlreadyProcessed` | Link has reached its `max_uses` limit | Create a new link |
| `InvalidAmount` | Amount does not match the fixed link amount | Use the exact amount set on the link |

---

## Advanced Recipes

### Verify Merchant Payments (Enumeration)

List all payments for a specific merchant:

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  -- get_merchant_payments \
  --merchant_id $TEST_MERCHANT_ADDRESS
```

### Paginated Merchant Payments

Fetch merchant payments with pagination:

```bash
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  -- get_merchant_payments_paginated \
  --merchant_id $TEST_MERCHANT_ADDRESS \
  --offset 0 \
  --limit 10 \
  --status_filter null
```

Set `status_filter` to a `PaymentStatus` value such as `Pending` or
`Confirmed` to paginate only matching merchant payment IDs. Use `null` to
preserve the unfiltered behavior.

### Paginated Full Payment Records (Issue #396)

Fetch paginated `PaymentCharge` structs (not just IDs) for merchant dashboards.
`limit` is silently capped at 50 per call.

```bash
# Get total count first (for pagination UI)
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  -- get_merchant_payment_count \
  --merchant_id $TEST_MERCHANT_ADDRESS

# Fetch first page of full PaymentCharge records
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  -- get_merchant_payments_full \
  --merchant_id $TEST_MERCHANT_ADDRESS \
  --offset 0 \
  --limit 20
```

Returns an array of full `PaymentCharge` objects. Returns an empty array
(not an error) when `offset` exceeds the total count.

### Idempotency Window (Issue #399)

Idempotency keys (`client_token`) are now stored with a TTL matching the
payment expiry window rather than a permanent TTL. After a payment expires
or is cancelled the key is proactively removed so the same `client_token`
can be reused for a new payment.

**Behaviour summary:**

| Scenario | `client_token` reusable? |
|----------|--------------------------|
| Payment active (Pending) | No — `DuplicateIdempotencyKey` |
| Same `payment_id` retry | Yes — returns existing payment |
| Payment cancelled | Yes — key freed immediately |
| Payment expired | Yes — key freed immediately |

### Monitor Payment Events

Monitor payment verification events in real-time:

```bash
# View recent ledger entries for payment events
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  -- --monitor-events
```

---

## Troubleshooting

### Contract Not Found

```bash
# Error: Contract not found
# Solution: Verify contract ID is deployed and correct
stellar contract info interface \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet
```

### Invalid Network Passphrase

```bash
# Error: Invalid network passphrase
# Solution: Ensure you're using testnet and correct settings
export STELLAR_NETWORK=testnet
```

### Insufficient Balance

```bash
# Error: Account requires minimum balance
# Solution: Fund account via Friendbot
# https://friendbot.stellar.org/?addr=<YOUR_ADDRESS>
```

### Insufficient Signatures

```bash
# Error: Signature verification failed
# Solution: Ensure --source matches the signer for the operation
# Most operations require authentication from the affected party
```

---

## Tips for Local Development

1. **Use consistent merchants**: Register one test merchant and reuse its ID for multiple payments
2. **Generate future timestamps**: Use `date +%s` and add seconds for realistic expiration times
3. **Batch operations**: Chain multiple invocations in a shell script for integration testing
4. **Save outputs**: Redirect results to JSON files for audit trails:
   ```bash
   stellar contract invoke ... >> payment_log.json
   ```
5. **Validate before submitting**: Always check payment amounts and expiration dates before creating charges

---

## Quick Test Loop

```bash
#!/bin/bash
source .env

# Setup
EXPIRES_AT=$(($(date +%s) + 3600))
PAYMENT_ID="test_$(date +%s)"

# 1. Register merchant (one-time)
echo "Registering merchant..."
stellar contract invoke \
  --id $MERCHANT_REGISTRY_ID \
  --network testnet \
  --source $TEST_MERCHANT_ADDRESS \
  -- register_merchant \
  --merchant_id $TEST_MERCHANT_ADDRESS \
  --business_name "Test Shop" \
  --settlement_currency "USD"

# 2. Create payment
echo "Creating payment..."
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  --source $TEST_MERCHANT_ADDRESS \
  -- create_payment \
  --payment_id $PAYMENT_ID \
  --merchant_id $TEST_MERCHANT_ADDRESS \
  --amount 1000000000 \
  --currency USDC \
  --deposit_address $ADMIN_ADDRESS \
  --expires_at $EXPIRES_AT

# 3. Verify payment created
echo "Verifying payment status..."
stellar contract invoke \
  --id $PAYMENT_PROCESSOR_ID \
  --network testnet \
  -- get_payment \
  --payment_id $PAYMENT_ID
```

---

## Deployment

### Deploy to Testnet

Use `scripts/deploy-testnet.sh` to build the WASM and deploy all contracts to testnet. The script writes the resulting contract IDs to `.env.testnet`.

#### Prerequisites

- Stellar CLI installed (`cargo install --locked stellar-cli`)
- `STELLAR_SECRET_KEY` and `STELLAR_NETWORK` set in your environment (or in `.env`)

#### Steps

```bash
# 1. Set required environment variables
export STELLAR_SECRET_KEY=<YOUR_SECRET_KEY>
export STELLAR_NETWORK=testnet          # or mainnet

# 2. Run the deploy script
bash scripts/deploy-testnet.sh

# 3. Load the deployed contract IDs
source .env.testnet
echo "PaymentProcessor: $PAYMENT_PROCESSOR_ID"
echo "RefundManager:    $REFUND_MANAGER_ID"
echo "MerchantRegistry: $MERCHANT_REGISTRY_ID"
echo "FX Oracle:        $FX_ORACLE_ID"
```

#### What the Script Does

1. Fails immediately if `STELLAR_SECRET_KEY` or `STELLAR_NETWORK` is unset.
2. Builds all contracts with `cargo build --target wasm32-unknown-unknown --release`.
3. Deploys each WASM via `stellar contract deploy` and captures the contract ID.
4. Writes all four contract IDs to `.env.testnet` (overwrites any previous run — idempotent).

#### Expected Output

```
[1/4] Deploying PaymentProcessor...  CXXX...
[2/4] Deploying RefundManager...     CYYY...
[3/4] Deploying MerchantRegistry...  CZZZ...
[4/4] Deploying FXOracle...          CAAA...
Contract IDs written to .env.testnet
```

#### Error Scenarios

| Situation | Behaviour |
|-----------|-----------|
| `STELLAR_SECRET_KEY` not set | Script exits with error before building |
| `STELLAR_NETWORK` not set | Script exits with error before building |
| `stellar contract deploy` fails | Script exits immediately (non-zero exit code) |

---

## Additional Resources

- [Stellar CLI Documentation](https://github.com/stellar/rs-soroban-cli)
- [Soroban SDK Examples](https://github.com/stellar/rs-soroban-sdk)
- [Stellar Testnet](https://testnet.stellar.org/)
- [Fluxapay API Documentation](../README.md)
- [Deployment Guide](../DEPLOYMENT.md)
