# FluxaPay TypeScript SDK Error Reference

This guide documents every `FluxapayError` subclass surfaced by the FluxaPay TypeScript SDK, its corresponding on-chain Soroban contract error code, when it is thrown, recommended handling and user-facing messages, and example catch blocks.

---

## Overview

When interacting with FluxaPay smart contracts via the TypeScript SDK, errors returned by contract invocations (such as `Error(Contract, #code)`) are automatically converted into typed subclasses of `FluxapayError`.

You can catch specific errors using `instanceof`:

```typescript
import {
  FluxapayClient,
  FluxapayError,
  PaymentExpiredError,
  KycLimitExceededError,
  PaymentNotFoundError,
} from "@fluxapay/sdk";

try {
  const payment = await client.createPayment({ ... });
} catch (error) {
  if (error instanceof KycLimitExceededError) {
    // Handle KYC limit breach (e.g., prompt merchant to upgrade KYC)
    console.error("KYC limit exceeded:", error.localizedMessage);
  } else if (error instanceof PaymentExpiredError) {
    // Handle expired payment
    console.error("Payment has expired:", error.message);
  } else if (error instanceof FluxapayError) {
    // Catch-all for any other contract error
    console.error(`Contract error #${error.code} (${error.contractErrorName}):`, error.localizedMessage);
  } else {
    throw error;
  }
}
```

---

## Error Classes Reference

| Error Class | Code | Rust Variant | Description & Handling |
|---|---|---|---|
| `UnauthorizedError` | `#1` | `Unauthorized` | Caller lacks required role or permission. |
| `PaymentAlreadyExistsError` | `#2` | `PaymentAlreadyExists` | Payment ID already exists in storage. |
| `PaymentExpiredError` | `#3` | `PaymentExpired` | Payment expiration timestamp has passed. |
| `InvalidPaymentIdError` | `#4` | `InvalidPaymentId` | Payment ID fails length, prefix, or format rules. |
| `RefundAlreadyProcessedError` | `#8` | `RefundAlreadyProcessed` | Refund has already been processed or completed. |
| `DisputeNotFoundError` | `#9` | `DisputeNotFound` | Specified dispute record was not found. |
| `DisputeAlreadyResolvedError` | `#12` | `DisputeAlreadyResolved` | Dispute has already been resolved or rejected. |
| `PaymentAlreadyProcessedError` | `#14` | `PaymentAlreadyProcessed` | Payment is already in terminal or confirmed state. |
| `AccessControlContractError` | `#15` | `AccessControlError` | Access control verification failed. |
| `RefundExceedsPaymentError` | `#16` | `RefundExceedsPayment` | Refund amount exceeds original payment amount. |
| `ContractPausedError` | `#17` | `ContractPaused` | Contract is currently paused by admin emergency switch. |
| `RateLimitExceededError` | `#18` | `RateLimitExceeded` | Too many operations in current rate limit window. |
| `RefundCancelledError` | `#19` | `RefundCancelled` | Requested refund has been cancelled. |
| `UnsupportedTokenError` | `#20` | `UnsupportedToken` | Token contract address is not in approved allowlist. |
| `AmountBelowMinError` | `#21` | `AmountBelowMin` | Payment amount is below merchant/tier minimum threshold. |
| `AmountAboveMaxError` | `#22` | `AmountAboveMax` | Payment amount exceeds merchant/tier maximum threshold. |
| `InvalidExpiryError` | `#23` | `InvalidExpiry` | Payment duration or expiry timestamp is outside valid bounds. |
| `InvalidSettlementError` | `#24` | `InvalidSettlement` | Settlement amounts or addresses are invalid. |
| `DuplicateIdempotencyKeyError` | `#25` | `DuplicateIdempotencyKey` | Idempotency client token already used for different payment. |
| `InvalidAddressError` | `#26` | `InvalidAddress` | Address is null, zero, or malformed strkey. |
| `ArbitrageDetectedError` | `#27` | `ArbitrageDetected` | DEX router swap slippage or price sandwich detected. |
| `SwapPathInvalidError` | `#28` | `SwapPathInvalid` | Route swap path between tokens is invalid or empty. |
| `OraclePriceDeviationError` | `#29` | `OraclePriceDeviation` | Oracle price deviates beyond configured tolerance threshold. |
| `SubscriptionInGracePeriodError` | `#30` | `SubscriptionInGracePeriod` | Subscription payment is currently within grace period. |
| `SubscriptionRetryExhaustedError` | `#31` | `SubscriptionRetryExhausted` | Maximum retry attempts for subscription charge exceeded. |
| `InvalidResumeTimestampError` | `#32` | `InvalidResumeTimestamp` | Resume timestamp for paused subscription is invalid. |
| `MerchantAuthContractError` | `#33` | `MerchantAuthError` | Merchant pre-authorization validation error. |
| `InvalidSplitSumError` | `#34` | `InvalidSplitSum` | Sum of settlement splits does not equal 100% (10,000 bps). |
| `MissingReceiptHashError` | `#35` | `MissingReceiptHash` | On-chain settlement receipt hash is missing. |
| `RefundExpiredError` | `#36` | `RefundExpired` | Refund window has elapsed without execution. |
| `AlreadyVotedError` | `#37` | `AlreadyVoted` | Arbitrator has already cast a vote for this dispute. |
| `TierVolumeLimitExceededError` | `#38` | `TierVolumeLimitExceeded` | Legacy volume limit exceeded. |
| `BatchTooLargeContractError` | `#39` | `BatchTooLarge` | Batch operation exceeds maximum allowed items (e.g., > 20 or 50). |
| `InsufficientArbitratorsError` | `#40` | `InsufficientArbitrators` | Not enough active arbitrators to reach quorum. |
| `ArbitrationVotingThresholdNotMetError` | `#41` | `ArbitrationVotingThresholdNotMet` | Voting threshold not reached within deadline. |
| `RefundCooldownNotElapsedError` | `#42` | `RefundCooldownNotElapsed` | Mandatory cooldown before executing refund has not elapsed. |
| `FeeProposalNotReadyError` | `#43` | `FeeProposalNotReady` | Fee proposal timelock has not elapsed yet. |
| `NoFeeProposalError` | `#44` | `NoFeeProposal` | No pending fee proposal found to apply. |
| `InvalidEvidenceFormatError` | `#45` | `InvalidEvidenceFormat` | Evidence payload exceeds size limits or is malformed. |
| `DisputeRateLimitExceededError` | `#46` | `DisputeRateLimitExceeded` | Too many disputes created in short interval. |
| `InvalidSettlementSignatureError` | `#47` | `InvalidSettlementSignature` | Settlement signature is invalid or unrecognized. |
| `StaleOracleRateError` | `#48` | `StaleOracleRate` | Oracle exchange rate timestamp is older than max allowed age. |
| `LinkExpiredError` | `#49` | `LinkExpired` | Payment link has expired. |
| `ReentrancyError` | `#50` | `Reentrancy` | Reentrancy guard triggered. |
| `UpgradeFailedError` | `#51` | `UpgradeFailed` | Contract Wasm code upgrade failed verification. |
| `InsufficientTreasuryBalanceError` | `#52` | `InsufficientTreasuryBalance` | Treasury contract balance cannot cover payout. |
| `MetadataTooLargeError` | `#53` | `MetadataTooLarge` | Metadata map exceeds maximum allowed entries (20). |
| `MetadataValueTooLongError` | `#54` | `MetadataValueTooLong` | Metadata key or value exceeds maximum length. |
| `InvalidMemoTypeError` | `#55` | `InvalidMemoType` | Stellar memo type is unrecognized (must be Text, Id, Hash, Return). |
| `MemoTooLongError` | `#56` | `MemoTooLong` | Memo text exceeds maximum allowed byte length. |
| `InvalidMemoIdError` | `#57` | `InvalidMemoId` | Memo ID cannot be parsed as a 64-bit unsigned integer. |
| `PayerNotWhitelistedError` | `#58` | `PayerNotWhitelisted` | Merchant whitelist mode is active and payer is not on whitelist. |
| `LinkMaxUsesReachedError` | `#59` | `LinkMaxUsesReached` | Payment link has reached its maximum configured use count. |
| `DirectTransferNotDisputableError` | `#60` | `DirectTransferNotDisputable` | Direct payments cannot be disputed via arbitration. |
| `MaxRetriesExceededError` | `#61` | `MaxRetriesExceeded` | Maximum retry count exceeded. |
| `RetryChainTooDeepError` | `#347` | `RetryChainTooDeep` | Payment retry link depth exceeds maximum depth of 3. |
| `InvalidStatusTransitionError` | `#62` | `InvalidStatusTransition` | Illegal payment state transition attempted in state machine. |
| `RefundNotApprovedError` | `#63` | `RefundNotApproved` | Refund has not received required approvals. |
| `RouterNotAllowedError` | `#64` | `RouterNotAllowed` | DEX router address is not in admin allowlist. |
| `RouteOutputInsufficientError` | `#65` | `RouteOutputInsufficient` | Output token amount from DEX swap is less than minimum requested. |
| `BatchContainsDuplicatesError` | `#66` | `BatchContainsDuplicates` | Batch submission contains duplicate payment IDs. |
| `InputTooLongError` | `#67` | `InputTooLong` | String or byte input exceeds maximum length. |
| `TimelockNotExpiredError` | `#68` | `TimelockNotExpired` | Operation timelock delay has not elapsed. |
| `InvalidEvidenceCidError` | `#69` | `InvalidEvidenceCid` | Dispute evidence IPFS CID format is invalid. |
| `InvalidPaymentLinkError` | `#70` | `InvalidPaymentLink` | Payment link configuration or parameters are invalid. |
| `KycLimitExceededError` | `#71` | `KycLimitExceeded` | Single payment amount or monthly volume exceeds merchant's KYC tier cap. |
| `PaymentNotFoundError` | `#404` | `PaymentNotFound` | Payment ID was not found in contract storage. |
| `RefundNotFoundError` | `#405` | `RefundNotFound` | Refund ID was not found in contract storage. |
| `InvalidAmountError` | `#406` | `InvalidAmount` | Payment or refund amount must be strictly greater than 0. |

---

## Detailed Error Handling & Examples

### 1. `KycLimitExceededError` (`#71`)
- **When Thrown**: When a merchant creates a payment that exceeds their single payment limit or cumulative monthly volume cap under ADR-0003:
  - **Tier 0**: Max 100 USDC single, 500 USDC monthly
  - **Tier 1**: Max 10,000 USDC single, 50,000 USDC monthly
- **Recommended User Message**: `"Your transaction exceeds your current KYC tier limit. Please complete merchant KYC verification to increase your processing limits."`
- **Example**:
```typescript
try {
  await client.createPayment({
    paymentId: "pay_123",
    merchantId: merchantAddress,
    amount: 150_0000000n, // 150 USDC for a Tier 0 merchant
    currency: "USDC",
    depositAddress: merchantAddress,
  });
} catch (error) {
  if (error instanceof KycLimitExceededError) {
    showUpgradeModal({
      title: "KYC Tier Limit Reached",
      message: "Please upgrade your merchant verification level to accept larger payments.",
    });
  }
}
```

### 2. `PaymentExpiredError` (`#3`)
- **When Thrown**: Attempting to confirm, verify, or pay an invoice after `expires_at` timestamp.
- **Recommended User Message**: `"This payment invoice has expired. Please request a new checkout session."`
- **Example**:
```typescript
try {
  await client.verifyPayment({ ... });
} catch (error) {
  if (error instanceof PaymentExpiredError) {
    ui.notify("The payment window timed out. A new invoice has been generated.");
  }
}
```

### 3. `PaymentNotFoundError` (`#404`)
- **When Thrown**: Querying `getPayment` or performing actions on a non-existent or garbage-collected payment ID.
- **Recommended User Message**: `"Payment record not found."`
- **Example**:
```typescript
try {
  const payment = await client.getPayment("pay_nonexistent");
} catch (error) {
  if (error instanceof PaymentNotFoundError) {
    console.warn("Payment not found on-chain");
  }
}
```

### 4. `PaymentAlreadyProcessedError` (`#14`)
- **When Thrown**: Attempting to confirm or verify a payment that is already in `Confirmed`, `Overpaid`, or terminal state, or `PartiallyPaid` without `allow_partial`.
- **Recommended User Message**: `"Payment has already been processed."`

### 5. `PayerNotWhitelistedError` (`#58`)
- **When Thrown**: Merchant has enabled `whitelist_mode` and customer address is not enrolled.
- **Recommended User Message**: `"Your wallet address is not authorized by this merchant."`
