# Unified Contract Error Code Reference

This document is the single source of truth for every `#[contracterror]` enum
across the Fluxapay contracts. Each error surfaces to callers as
`Error(Contract, #<code>)`; use the tables below to look up what a numeric
code means, what typically causes it, and how to fix it.

`sdk/src/index.ts` exports `FLUXAPAY_CONTRACT_ERROR_MAP`, which mirrors the
**Core (`PaymentProcessor` / `RefundManager`)** table below — the only
contract whose error map is safe to expose as a single flat `code -> name`
map in the SDK (see the note on code collisions at the end of this
document). `scripts/check-error-map-sync.ts` parses the Rust source and this
document on every CI run to catch drift between them.

## Core: `PaymentProcessor` / `RefundManager` (`fluxapay/src/lib.rs::Error`)

| Code | Name | Description | Common Cause | Remediation |
|------|------|-------------|---------------|-------------|
| 1 | `Unauthorized` | Caller lacks the required role/permission. | Calling an admin/merchant/oracle-only function from an unauthorized address. | Grant the correct role via `access_control`, or call from an authorized address. |
| 2 | `PaymentAlreadyExists` | A payment with this `payment_id` was already created. | Reusing a `payment_id` (often a retry without a fresh ID). | Generate a new unique `payment_id`, or fetch the existing payment instead. |
| 3 | `PaymentExpired` | The payment's `expires_at` has passed. | Verifying/paying after the payment window closed. | Create a new payment; consider extending `duration_secs` for long-lived flows. |
| 4 | `InvalidPaymentId` | `payment_id` failed format validation (length/charset). | ID shorter than 3 or longer than 64 chars, or contains disallowed characters. | Use 3–64 chars of `[a-zA-Z0-9_-]` only. |
| 8 | `RefundAlreadyProcessed` | The refund is not in a state that can be processed again. | Calling approve/reject/process on a refund that's already `Completed` or `Rejected`. | Check `get_refund` status before acting; this is typically not retryable. |
| 9 | `DisputeNotFound` | No dispute exists with the given ID. | Typo'd dispute ID, or dispute was never created. | Verify the dispute ID via `get_dispute`/list endpoints. |
| 12 | `DisputeAlreadyResolved` | Dispute has already reached a final resolution. | Voting/resolving a dispute twice. | No action needed — the dispute outcome is final. |
| 14 | `PaymentAlreadyProcessed` | Payment is not `Confirmed`/`Overpaid`, so refund/settlement isn't allowed. | Refunding a `Pending`, `PartiallyPaid`, or already-refunded payment. | Wait for confirmation, or check `payment.status` first. |
| 15 | `AccessControlError` | An underlying access-control check failed (see `AccessControlError` table). | Role/admin operation failed in the shared access-control module. | Inspect the wrapped `AccessControlError` variant for specifics. |
| 16 | `RefundExceedsPayment` | Sum of non-rejected refunds would exceed the original payment amount. | Requesting a refund larger than the remaining refundable balance. | Request an amount ≤ `payment.amount - sum(non-rejected refunds)`. |
| 17 | `ContractPaused` | The contract is globally paused. | An admin invoked the emergency pause. | Wait for an admin to unpause, or contact the operator. |
| 18 | `RateLimitExceeded` | Caller exceeded a configured rate limit. | Too many requests/payments in a short window. | Back off and retry after the rate-limit window resets. |
| 19 | `RefundCancelled` | The refund was cancelled and cannot be processed. | Acting on a refund after it was explicitly cancelled. | Create a new refund request if still eligible. |
| 20 | `UnsupportedToken` | The token address is not an accepted asset. | Using a token not configured for this deployment. | Use one of the supported token addresses for this environment. |
| 21 | `AmountBelowMin` | Amount is below the configured minimum. | Payment/stake amount too small. | Increase the amount to at least the configured minimum. |
| 22 | `AmountAboveMax` | Amount exceeds the configured maximum. | Payment/stake amount too large. | Reduce the amount, or split into multiple payments. |
| 23 | `InvalidExpiry` | `expires_at` is invalid (e.g. in the past, or before `start_time`). | Passing a stale or malformed timestamp. | Pass a future Unix timestamp greater than "now". |
| 24 | `InvalidSettlement` | Settlement parameters/signatures failed validation. | Malformed or mismatched collaborative-settlement data. | Re-derive settlement data per `docs/` settlement guide and retry. |
| 25 | `DuplicateIdempotencyKey` | `client_token` was already used for a different payment. | Retrying a request with the same idempotency key but different params. | Reuse the key only for byte-identical retries, or mint a new key. |
| 26 | `InvalidAddress` | An address argument failed validation. | Passing a malformed or zero address. | Pass a valid Stellar/Soroban `Address`. |
| 27 | `ArbitrageDetected` | Swap path forms a circular/arbitrage route. | DEX router path validation rejected the route. | Use a direct, non-circular swap path. |
| 28 | `SwapPathInvalid` | DEX swap path or quoted return failed validation. | Malformed path, or quote inconsistent with path. | Re-quote the path via the DEX router before submitting. |
| 29 | `OraclePriceDeviation` | DEX quote deviates too far from the oracle reference price. | Stale quote, or thin liquidity causing slippage. | Re-quote closer to execution time, or reduce trade size. |
| 30 | `SubscriptionInGracePeriod` | Subscription payment failed but is within its retry grace period. | Informational — not a terminal failure. | No action required; the daemon will retry automatically. |
| 31 | `SubscriptionRetryExhausted` | Subscription exhausted all retries and is now cancelled. | Payment method kept failing through all retry attempts. | Re-subscribe with a valid, funded payment method. |
| 32 | `InvalidResumeTimestamp` | Resume timestamp is in the past or otherwise invalid. | Resuming a paused subscription/stream with a bad timestamp. | Pass a resume timestamp ≥ current ledger time. |
| 33 | `MerchantAuthError` | An underlying `MerchantAuthError` occurred (see that table). | Pre-authorization pull failed a sub-check. | Inspect the wrapped `MerchantAuthError` variant for specifics. |
| 34 | `InvalidSplitSum` | Dispute payout_splits amounts don't sum to the dispute amount. | Miscalculated split allocation in dispute resolution. | Ensure payout_splits sum equals the dispute amount. |
| 35 | `MissingReceiptHash` | Refund policy requires a `receipt_hash` but none was provided. | Creating a refund without a required receipt attachment. | Supply a valid receipt hash with the refund request. |
| 36 | `RefundExpired` | The refund's `expiry_at` deadline has passed. | Approving/rejecting a refund after `expiry_at`. | Requester must create a new refund request. |
| 37 | `AlreadyVoted` | Arbitrator has already cast a vote on this dispute. | Double-voting on the same dispute. | Each arbitrator may vote once per dispute. |
| 38 | `TierVolumeLimitExceeded` | Merchant exceeded their KYC tier's monthly volume cap. | Processing volume exceeded the tier's `AmountLimits`. | Request a tier upgrade, or wait for the monthly window to reset. |
| 39 | `BatchTooLarge` | Batch payment request exceeds the supported maximum size. | Submitting too many payments in one batch call. | Split into smaller batches under the documented max size. |
| 40 | `InsufficientArbitrators` | Not enough arbitrators are available to vote on a dispute. | Dispute arbitration pool too small. | Register additional arbitrators before opening disputes. |
| 41 | `ArbitrationVotingThresholdNotMet` | Dispute voting threshold hasn't been reached yet. | Resolving a dispute before enough arbitrators voted. | Wait for more votes before attempting resolution. |
| 42 | `RefundCooldownNotElapsed` | Refund requested before the post-confirmation cooldown elapsed. | Requesting a refund immediately after payment confirmation. | Wait until `confirmed_at + refund_cooldown_secs` has passed. |
| 43 | `FeeProposalNotReady` | Fee proposal hasn't matured past the required 7-day timelock. | Applying a fee change before the timelock elapses. | Wait until 7 days after the proposal was created. |
| 44 | `NoFeeProposal` | No active fee-change proposal exists. | Trying to apply/cancel a proposal that was never created. | Create a fee proposal first via the appropriate admin call. |
| 45 | `InvalidEvidenceFormat` | Dispute evidence is not a valid IPFS multihash (CIDv0/CIDv1). | Passing an arbitrary string instead of a CID. | Pass a valid IPFS CIDv0/CIDv1 hash as evidence. |
| 46 | `DisputeRateLimitExceeded` | Dispute creation rate limit exceeded (per-payer open cap or global hourly cap). | Opening too many disputes in a short window. | Back off and retry after the rate-limit window resets. |
| 47 | `InvalidSettlementSignature` | One or both collaborative-settlement signatures are invalid. | Signature doesn't match the expected signer/payload. | Re-sign the settlement payload with the correct key. |
| 48 | `StaleOracleRate` | FX oracle rate is stale or unavailable. | Oracle hasn't been updated within the staleness threshold. | Wait for (or trigger) an oracle rate update before retrying. |
| 49 | `LinkExpired` | Payment link has expired. | Using an expired payment link. | Create a new payment link. |
| 50 | `Reentrancy` | Reentrancy detected in `process_refund_internal`/`settle_payment`. | Nested/reentrant contract call during a guarded operation. | Not user-fixable — indicates a caller bug; avoid recursive invocations. |
| 51 | `UpgradeFailed` | Contract upgrade rejected the new WASM hash. | Invalid or incompatible upgrade payload. | Verify the WASM hash and upgrade authorization, then retry. |
| 52 | `InsufficientTreasuryBalance` | Treasury balance is smaller than the requested withdrawal amount. | Withdrawing more than the treasury holds. | Reduce the withdrawal amount, or wait for treasury deposits. |
| 53 | `MetadataTooLarge` | Metadata map has more than 20 keys. | Attaching too many metadata fields to a payment. | Reduce to ≤ 20 keys, or store extra data off-chain. |
| 54 | `MetadataValueTooLong` | A metadata value exceeds 256 characters. | Oversized value in the `metadata` map. | Shorten the value to ≤ 256 characters. |
| 55 | `InvalidMemoType` | `memo_type` is not one of `Text`, `Id`, `Hash`, `Return`. | Typo or unsupported memo type string. | Use exactly one of the four supported memo types. |
| 56 | `MemoTooLong` | Text memo exceeds the 28-byte Stellar limit. | Memo text too long for a Stellar `MEMO_TEXT`. | Shorten the memo to ≤ 28 bytes. |
| 57 | `InvalidMemoId` | `Id`-type memo is not parseable as a `u64`. | Passing a non-numeric string as an `Id` memo. | Pass a valid unsigned 64-bit integer as a string. |
| 58 | `PayerNotWhitelisted` | Payer address is not on the merchant's customer whitelist. | Merchant has whitelist mode enabled and payer isn't listed. | Merchant must add the payer via the whitelist management call. |
| 59 | `LinkMaxUsesReached` | Payment link has reached its configured `max_uses` limit. | Payment link was used too many times. | Create a new payment link with a higher `max_uses`. |
| 60 | `DirectTransferNotDisputable` | Payment was created via a `direct_transfer` link and disputes are not allowed. | Attempting to dispute a direct-transfer payment. | Direct-transfer payments are non-disputable by design. |
| 61 | `MaxRetriesExceeded` | Maximum retry chain depth (3) exceeded for payment retry. | Payment retry chain too deep. | Resolve the underlying payment failure before retrying. |
| 347 | `RetryChainTooDeep` | A retry would create a chain deeper than three payments. | Retrying a payment that is already three links from its original payment. | Resolve the failure or start a new payment. |
| 62 | `InvalidStatusTransition` | Invalid payment status transition attempted. | Attempting a disallowed state change (e.g. `Confirmed` → `Pending`). | Check the payment's current status and allowed transitions. |
| 63 | `RefundNotApproved` | Customer called `claim_refund` before an operator approved it. | Claiming a refund that hasn't been operator-approved yet. | Wait for an operator to approve the refund first. |
| 64 | `RouterNotAllowed` | DEX router is not in the allowed routers list. | Using a router not configured for this deployment. | Use an approved router, or have an admin update the allowed list. |
| 65 | `RouteOutputInsufficient` | Aggregate route output is less than minimum output amount. | Swap output too low due to slippage or thin liquidity. | Re-quote with a lower minimum output, or reduce trade size. |
| 66 | `BatchContainsDuplicates` | Batch payment creation contains duplicate payment IDs. | Submitting a batch where two or more entries share the same `payment_id`. | Ensure all `payment_id` values in the batch are unique. |
| 67 | `InputTooLong` | A user-supplied string field exceeds its maximum allowed length. | `reason` > 256 chars in refund creation; `evidence` > 512 chars in dispute creation; `resolution_notes` > 512 chars in dispute rejection. | Shorten the field: `reason` ≤ 256 chars, `evidence` ≤ 512 chars, `resolution_notes` ≤ 512 chars. |
| 68 | `TimelockNotExpired` | A timelocked admin action was executed before delay expired. | Executing action before timelock delay has elapsed. | Wait for the delay period to elapse. |
| 69 | `InvalidEvidenceCid` | Evidence field is not a valid IPFS CID. | Passing a non-CID string as dispute evidence. | Pass a valid IPFS CID (v0 starting with Qm or v1 starting with bafy). |
| 70 | `InvalidPaymentLink` | Payment link configuration or parameters are invalid. | Malformed payment link creation parameters. | Verify payment link arguments and try again. |
| 71 | `KycLimitExceeded` | Payment amount or cumulative monthly volume exceeds merchant's KYC tier cap. | Single payment exceeds tier max, or cumulative monthly volume exceeds tier cap under ADR-0003. | Upgrade merchant KYC tier to increase payment volume limits. |
| 87 | `InvalidMerkleLeafCount` | `leaf_count` passed to `create_batch` is zero or exceeds the maximum. | Creating a batch distribution with 0 or more than 10,000 leaves. | Pass a `leaf_count` between 1 and `MAX_MERKLE_BATCH_LEAVES`. |
| 88 | `MerkleDistributionNotFound` | No Merkle batch distribution exists with the given `distribution_id`. | Typo'd or stale `distribution_id`. | Verify the ID via `get_batch`. |
| 89 | `MerkleAlreadyClaimed` | This `(distribution_id, index)` allocation was already claimed. | Submitting the same claim twice. | No action needed — the recipient already received their funds. |
| 90 | `InvalidMerkleProof` | The submitted proof does not resolve to the distribution's stored Merkle root. | Wrong `index`/`amount`/sibling hashes, or a proof built against a different tree. | Regenerate the proof with `sdk/src/merkle.ts` from the exact leaf set used to create the batch. |
| 91 | `MerkleDistributionExpired` | The batch distribution's `expires_at` has passed. | Claiming after the distribution's claim window closed. | Claims must happen before `expires_at`; contact the creator about a new batch. |
| 92 | `MerkleDistributionNotExpired` | Attempted to `defund_batch` before `expires_at`. | Calling `defund_batch` while claims are still open. | Wait until after `expires_at` to defund. |
| 93 | `MerkleDistributionAlreadyDefunded` | The batch distribution was already defunded by its creator. | Calling `claim_batch_payment` or `defund_batch` again after defunding. | No action needed — the distribution is closed. |
| 94 | `TreasuryExceedsSingleAdminThreshold` | Withdrawal amount exceeds the configured single-admin threshold. | Calling `withdraw_treasury` for more than `get_treasury_single_admin_threshold()` (default 1000 USDC). | Use `propose_treasury_withdrawal` / `approve_treasury_withdrawal` / `execute_treasury_withdrawal` for larger amounts, or lower the amount. |
| 95 | `ExceedsTierSingleLimit` | Payment amount exceeds the merchant's KYC tier single-payment limit. | Merchant's KYC tier (`TIER_0_MAX_SINGLE` / `TIER_1_MAX_SINGLE` / `TIER_2_MAX_SINGLE`) can't cover this payment. | Reduce the amount, or upgrade the merchant's KYC tier. |
| 96 | `ExceedsTierMonthlyLimit` | Payment would exceed the merchant's KYC tier monthly volume limit. | Cumulative volume for the current calendar month (UTC) plus this payment exceeds the tier's monthly cap. | Wait for the next calendar month, reduce the amount, or upgrade the merchant's KYC tier. |
| 404 | `PaymentNotFound` | No payment exists with the given `payment_id`. | Typo'd ID, or payment was never created. | Verify the ID via `get_payment` / listing endpoints. |
| 405 | `RefundNotFound` | No refund exists with the given `refund_id`. | Typo'd ID, or refund was never created. | Verify the ID via `get_refund` / `get_payment_refunds`. |
| 406 | `InvalidAmount` | Amount is zero, negative, or otherwise invalid. | Passing a non-positive amount to a payment/refund call. | Pass a strictly positive `i128` amount. |
| 88 | `OracleStale` | FX rate used by `confirm_payment`/`verify_payment` is older than the PaymentProcessor's own configured staleness threshold. | `MAX_ORACLE_STALENESS_SECS` (default 5 minutes) or the admin-configured override via `set_oracle_staleness_threshold` has elapsed since the rate was last updated, or the FX oracle itself reports the rate as stale. | Trigger (or wait for) an oracle rate update, or raise the threshold via `set_oracle_staleness_threshold` if 5 minutes is too strict for your deployment. |
| 89 | `DepositAddressInUse` | The requested `deposit_address` is already assigned to another open (`Pending`, unexpired) payment. | Reusing a deposit address across two concurrently-open payments (Issue #1032 spam mitigation). | Use a fresh deposit address, or wait for the existing payment on that address to resolve or expire. |

> **Note:** Every `Error` variant has a unique discriminant — there are no code collisions in the current Rust source.

## `AccessControlError` (`fluxapay/src/access_control.rs`)

| Code | Name | Description | Common Cause | Remediation |
|------|------|-------------|---------------|-------------|
| 1 | `Unauthorized` | Caller does not hold the required role. | Calling a role-gated function without that role. | Have an admin grant the role first. |
| 2 | `RoleAlreadyGranted` | The account already holds this role. | Granting a role that's already active. | No action needed — the role is already in effect. |
| 3 | `RoleNotGranted` | The account does not hold this role. | Revoking/checking a role the account never had. | Confirm the role assignment via a role-query call. |
| 4 | `CannotRenounceAdmin` | The sole admin attempted to renounce their own role. | Removing the last remaining admin. | Transfer admin to another address first. |
| 5 | `InvalidAdmin` | Proposed admin address is invalid. | Passing a zero/malformed address as new admin. | Pass a valid, distinct `Address`. |
| 6 | `RevocationCooldownActive` | A role revocation cooldown is still active. | Re-revoking before the cooldown window elapsed. | Wait for the cooldown to expire before retrying. |
| 7 | `NoPendingRevocation` | No pending revocation exists to act on. | Confirming/cancelling a revocation that was never proposed. | Propose the revocation first. |
| 8 | `RecoveryKeyNotSet` | No recovery key configured for this account. | Attempting account recovery without a registered recovery key. | Register a recovery key before relying on recovery flows. |
| 9 | `ProposalNotFound` | No admin proposal exists with the given ID/nonce. | Voting on a proposal that doesn't exist. | Verify the proposal nonce before voting. |
| 10 | `ProposalAlreadyVoted` | This account already voted on the proposal. | Double-voting on the same admin proposal. | Each account may vote once per proposal. |
| 11 | `ProposalExpired` | The proposal's voting window has closed. | Voting/executing after the proposal expired. | Create a new proposal. |
| 12 | `ProposalThresholdNotMet` | Not enough approvals to execute the proposal. | Executing a multi-sig admin action before quorum. | Collect additional approvals before executing. |
| 13 | `PendingAdminTransfer` | An admin transfer is already pending. | Proposing a new transfer while one is in flight. | Wait for the pending transfer to complete or expire. |
| 14 | `InvalidRecovery` | Recovery attempt failed validation. | Wrong recovery key or malformed recovery payload. | Retry with the correct registered recovery key. |
| 15 | `UnknownRole` | Role symbol is not in the `KNOWN_ROLES` registry. | Granting/revoking a typo'd or invented role (e.g. `SETLMENT_OPERATOR`). | Use one of: `ADMIN`, `ORACLE`, `MERCHANT`, `SETTLEMENT_OPERATOR`, `ARBITRATOR`. |

## `StreamError` (`fluxapay/src/stream.rs`)

| Code | Name | Description | Common Cause | Remediation |
|------|------|-------------|---------------|-------------|
| 1 | `StreamNotFound` | No stream exists with the given ID. | Typo'd stream ID, or stream was never created. | Verify the ID via a stream-lookup call. |
| 2 | `Unauthorized` | Caller is not the sender of the stream. | A non-sender tried to modify/cancel the stream. | Only the stream's sender may perform this action. |
| 3 | `RateNotDecreased` | New rate must be strictly less than the current rate. | Attempting to increase a stream's rate (disallowed by design). | Only decrease the rate; increases require a new stream. |
| 4 | `InvalidRate` | Rate cannot be zero or negative. | Passing a non-positive `rate_per_second`. | Pass a strictly positive rate. |
| 5 | `StreamAlreadyExists` | A stream with that ID already exists. | Reusing a stream ID. | Use a new, unique stream ID. |
| 6 | `InvalidDeposit` | Deposit must be positive. | Creating a stream with a zero/negative deposit. | Pass a strictly positive deposit amount. |
| 7 | `StreamNotActive` | Stream is not active. | Withdrawing from/modifying a cancelled or finished stream. | No action possible — the stream has ended. |
| 8 | `DestinationNotSet` | No destination configured for a permissionless withdrawal. | Calling permissionless withdraw before setting a destination. | Sender must configure a withdrawal destination first. |
| 9 | `ContractPaused` | The contract is globally paused. | An admin invoked the emergency pause. | Wait for an admin to unpause. |
| 10 | `MilestoneNotApproved` | Distributions are locked until the sender approves milestones. | Withdrawing before the sender approved the current milestone. | Sender must call the milestone-approval function first. |
| 11 | `WithdrawalInProgress` | Withdrawal already in progress (reentrancy guard). | A second withdrawal call while one is still executing. | Wait for the in-flight withdrawal to complete, then retry. |
| 12 | `RateBelowMinimum` | New rate is below the minimum allowed rate for this stream. | Decreasing the rate past the configured `min_rate_per_second` floor. | Choose a rate ≥ the stream's minimum (DoS-protection floor). |
| 13 | `StreamNotPaused` | Stream is not paused. | Resuming a stream that was never paused. | Confirm stream status before calling resume. |
| 14 | `InvalidReceiver` | Receiver address cannot equal the sender address. | Creating a stream that pays back to its own sender. | Use a different address for the receiver. |
| 15 | `BatchTooLarge` | A bulk operation was passed more stream IDs than the per-call cap (50). | Calling `bulk_bump_stream_ttls` with more than 50 IDs. | Split the IDs into batches of ≤ 50. |
| 16 | `CliffNotReached` | Stream accrual is locked until its cliff timestamp. | Attempting to withdraw from a vesting stream before its cliff. | Wait until the configured cliff timestamp. |

## `FXOracleError` (`fluxapay/src/fx_oracle.rs`)

| Code | Name | Description | Common Cause | Remediation |
|------|------|-------------|---------------|-------------|
| 1 | `RateNotFound` | No rate is recorded for the requested currency pair. | Querying a pair the oracle hasn't been given a rate for. | Have the oracle publish a rate for that pair first. |
| 2 | `RateStale` | The recorded rate is older than the staleness threshold. | Oracle hasn't updated the rate recently enough. | Wait for (or trigger) a fresh oracle update. |
| 3 | `Unauthorized` | Caller is not an authorized oracle/admin. | Calling an oracle-only update function without the role. | Grant the oracle role, or call from an authorized address. |
| 4 | `BatchTooLarge` | Batch rate update exceeds the max of 20 pairs. | Submitting > 20 pairs in one batch update. | Split batch updates to ≤ 20 pairs. |
| 5 | `RateDeviationExceeded` | Rate deviation exceeds the configured limit. | Publishing a rate too far from the previous value. | Check the feed or adjust the pair's deviation limit. |
| 7 | `InvalidStalenessThreshold` | Staleness threshold is outside the allowed 60-second to 24-hour range. | Setting a threshold below 60 seconds or above 86,400 seconds. | Use a threshold between 60 and 86,400 seconds. |

## `MerchantError` (`fluxapay/src/merchant_registry.rs`)

| Code | Name | Description | Common Cause | Remediation |
|------|------|-------------|---------------|-------------|
| 1 | `MerchantAlreadyExists` | A merchant with this ID is already registered. | Re-registering the same merchant ID. | Use `update_merchant` instead, or a new merchant ID. |
| 2 | `MerchantNotFound` | No merchant exists with the given ID. | Typo'd merchant ID, or merchant never registered. | Verify the ID via a merchant-lookup call. |
| 3 | `Unauthorized` | Caller lacks permission for this merchant operation. | Non-admin/non-owner calling a gated merchant function. | Call from the merchant owner or an admin address. |
| 4 | `NotVerified` | Merchant has not completed KYC verification. | Performing an action gated behind verification. | Complete KYC verification for the merchant first. |
| 5 | `AdminAlreadySet` | The registry has already been initialized with an admin. | Calling `initialize` a second time. | `initialize` is one-time only; use admin-management calls instead. |
| 6 | `PayoutAddressNotWhitelisted` | Payout address is not on the merchant's approved list. | Withdrawing to an address that wasn't whitelisted. | Whitelist the payout address before withdrawing to it. |
| 7 | `WhitelistModeRequiresBusinessTier` | Only Business-tier merchants may enable whitelist mode. | Enabling customer whitelist mode below Business tier. | Upgrade the merchant to Business tier first. |
| 8 | `PayerNotWhitelisted` | Payer is not in the merchant's customer whitelist. | Whitelist mode is on and the payer isn't listed. | Merchant must add the payer to the whitelist. |
| 12 | `KycLimitExceeded` | Single payment or cumulative monthly volume exceeds merchant's KYC tier cap. | Merchant at tier 0 or tier 1 breached per-payment or monthly limit. | Upgrade merchant KYC tier. |
| 15 | `TierSingleLimitExceeded` | Payment amount exceeds the merchant's KYC tier single-payment limit. | `check_kyc_limit` called from `create_payment` with `amount > tier_single_max`. | Reduce the amount, or upgrade the merchant's KYC tier. |
| 16 | `TierMonthlyLimitExceeded` | Payment would exceed the merchant's KYC tier monthly volume limit. | `check_kyc_limit` / `check_monthly_limit_only` found current-month volume plus this amount exceeds the tier's monthly cap. | Wait for the next calendar month, reduce the amount, or upgrade the merchant's KYC tier. |

## `MerchantAuthError` (`fluxapay/src/merchant_auth.rs`)

| Code | Name | Description | Common Cause | Remediation |
|------|------|-------------|---------------|-------------|
| 1 | `AuthorizationNotFound` | No pre-authorization exists for this (customer, merchant) pair. | Pulling funds without a prior grant. | Customer must grant a pre-authorization first. |
| 2 | `AuthorizationInactive` | The authorization has been revoked or is inactive. | Pulling against a revoked/inactive authorization. | Customer must grant a new authorization. |
| 3 | `LimitExceeded` | Requested pull exceeds the remaining period limit. | Pulling more than `limit_per_period` allows in the current window. | Pull a smaller amount, or wait for the next period. |
| 4 | `InvalidAmount` | Amount must be positive. | Passing a zero/negative pull amount. | Pass a strictly positive amount. |
| 5 | `Unauthorized` | Caller is not the authorized merchant. | Some other address attempting to pull funds. | Only the merchant named in the authorization may pull. |
| 6 | `AuthorizationAlreadyExists` | An authorization already exists for this pair. | Granting a second authorization without revoking the first. | Revoke the existing authorization before granting a new one. |

## `DexRouterError` (`fluxapay/src/dex_router.rs`)

| Code | Name | Description | Common Cause | Remediation |
|------|------|-------------|---------------|-------------|
| 1 | `SwapFailed` | The underlying DEX swap execution failed. | Router/pool call reverted. | Retry with a fresh quote, or a different path. |
| 2 | `InvalidPath` | The swap path is malformed or unsupported. | Empty path, or a hop the router doesn't support. | Use a valid, router-supported swap path. |
| 3 | `InsufficientLiquidity` | Not enough liquidity to fill the requested swap. | Trade size too large for the pool's depth. | Reduce trade size, or split across multiple swaps. |
| 4 | `SlippageExceeded` | Output amount fell below the minimum acceptable. | Price moved between quote and execution. | Increase slippage tolerance, or re-quote closer to execution. |
| 5 | `PriceImpactExceeded` | Trade's price impact exceeds the configured guard. | Trade size too large relative to pool depth. | Reduce trade size to stay within the price-impact guard. |
| 6 | `NoOutputAmount` | Swap would return zero output. | Degenerate/zero-value swap request. | Increase the input amount. |
| 7 | `Refunded` | Swap failed and input was refunded via fallback logic. | Router fallback path triggered after a failed swap. | Informational — funds were returned; retry if desired. |

## `AccountAbstractionError` (`fluxapay/src/account_abstraction.rs`)

| Code | Name | Description | Common Cause | Remediation |
|------|------|-------------|---------------|-------------|
| 1 | `Unauthorized` | Caller is not authorized to use this session key. | Executing a payload with a session key you don't own. | Use a session key granted to your account. |
| 2 | `SessionNotFound` | No session exists for the given (account, session_key) pair. | Typo'd session key, or session was never granted. | Grant a session key before attempting execution. |
| 3 | `SessionExpired` | The session key has passed its expiry time. | Executing after the session's validity window closed. | Grant a new session key. |
| 4 | `InvalidPayload` | The execution payload failed validation. | Malformed or empty payload/hash, or a negative spend cap. | Pass a `SessionInvocation` XDR payload and non-negative policy caps. |
| 5 | `SignerRevoked` | The session key was revoked before execution. | The account owner called `revoke_session_key` while a payload was in flight. | Register a new session key. |
| 6 | `SpendLimitExceeded` | One call exceeds `max_amount_per_tx`. | The invocation amount is above the per-transaction cap. | Split the charge or raise the cap. |
| 7 | `WindowBudgetExceeded` | The rolling window budget is exhausted. | Cumulative spend in the current window would pass `max_amount_per_window`. | Wait for the window to roll, or raise the budget. |
| 8 | `ContractNotAllowed` | The target contract is not on the session allowlist. | Payload `target_contract` is outside `allowed_contracts`. | Call an allowlisted contract, or register a wider policy. |
| 9 | `FunctionNotAllowed` | The function selector is not on the session allowlist. | Payload `function` is outside `allowed_functions`. | Call an allowlisted function, or register a wider policy. |

---

*Generated for #458. Keep this table in sync with the `#[contracterror]` enums it documents — `scripts/check-error-map-sync.ts` checks `FLUXAPAY_CONTRACT_ERROR_MAP` (Core table) against `fluxapay/src/lib.rs` on every CI run.*

## Tiered Refund Fee Reference (Issue #1016)

`process_refund` (via `process_refund_internal`) and `refund_instantly`
look up the merchant's KYC tier through `MerchantRegistry` and apply the
matching fee rate below, instead of a single flat rate for every merchant.
Both entry points emit the applied rate as the `fee_bps` field in the
`REFUND/COMPLETED` event payload.

| KYC Tier | Refund Fee (bps) | Refund Fee (%) | Constant |
|---|---|---|---|
| Unverified | 100 (falls back to `CurrentFee`, default `REFUND_FEE_BPS`) | 1.0% | `REFUND_FEE_BPS` |
| Basic | 100 | 1.0% | `REFUND_FEE_BPS_BASIC` |
| Full | 80 | 0.8% | `REFUND_FEE_BPS_FULL` |
| Business | 50 | 0.5% | `REFUND_FEE_BPS_BUSINESS` |

Notes:

- Unverified merchants use whatever the admin has configured as the
  contract-wide default fee (`DataKey::CurrentFee`, falling back to the
  `REFUND_FEE_BPS` constant of 100 bps) rather than a tier-specific
  constant, since "Unverified" has no dedicated constant of its own.
- `refund_instantly` only accepts callers whose tier is already `Full` or
  `Business` (see the `is_high_trust` check in `fluxapay/src/lib.rs`), so
  in practice it only ever applies `REFUND_FEE_BPS_FULL` or
  `REFUND_FEE_BPS_BUSINESS`; the `Basic`/`Unverified` match arms exist for
  defensive completeness and future tier expansion.
- A merchant's tier is read at the moment the refund is processed, not at
  the moment the original payment or refund request was created. A tier
  upgrade (see #1015) that lands between refund creation and processing is
  reflected in the fee actually charged.
- `RefundManager` needs its own `MerchantRegistry` address configured via
  `set_merchant_registry_address` (admin-only) — it is a separate contract
  from `PaymentProcessor` and does not share that configuration
  automatically. If no registry address is set, every tier falls back to
  the flat default fee.

## Error Enum (`fluxapay::Error`)

| Variant | Code |
|---|---|
| Unauthorized | 1 |
| PaymentAlreadyExists | 2 |
| PaymentExpired | 3 |
| InvalidPaymentId | 4 |
| RefundAlreadyProcessed | 8 |
| DisputeNotFound | 9 |
| DisputeAlreadyResolved | 12 |
| PaymentAlreadyProcessed | 14 |
| AccessControlError | 15 |
| RefundExceedsPayment | 16 |
| ContractPaused | 17 |
| RateLimitExceeded | 18 |
| RefundCancelled | 19 |
| UnsupportedToken | 20 |
| AmountBelowMin | 21 |
| AmountAboveMax | 22 |
| InvalidExpiry | 23 |
| InvalidSettlement | 24 |
| DuplicateIdempotencyKey | 25 |
| InvalidAddress | 26 |
| ArbitrageDetected | 27 |
| SwapPathInvalid | 28 |
| OraclePriceDeviation | 29 |
| SubscriptionInGracePeriod | 30 |
| SubscriptionRetryExhausted | 31 |
| InvalidResumeTimestamp | 32 |
| MerchantAuthError | 33 |
| TierVolumeLimitExceeded | 34 |
| BatchTooLarge | 35 |
| RefundExpired | 36 |
| InsufficientArbitrators | 37 |
| ArbitrationVotingThresholdNotMet | 38 |
| FeeProposalNotReady | 39 |
| InvalidEvidenceFormat | 40 |
| InvalidSettlementSignature | 41 |
| RefundCooldownNotElapsed | 42 |
| Reentrancy | 43 |
| NoFeeProposal | 44 |
| StaleOracleRate | 45 |
| LinkExpired / InsufficientTreasuryBalance | 46 |
| MetadataValueTooLong | 47 |
| UpgradeFailed | 48 |
| MetadataTooLarge | 49 |
| InvalidMemoType | 50 |
| MemoTooLong | 51 |
| InvalidMemoId | 52 |
| PayerNotWhitelisted | 53 |
| LinkMaxUsesReached / DirectTransferNotDisputable / MaxRetriesExceeded / InvalidStatusTransition | 54 |
| RateDeviationExceeded | 55 |
| DisputeRateLimitExceeded | 54 |
| PaymentNotFound | 404 |
| RefundNotFound | 405 |
| InvalidAmount | 406 |

> Several variants above intentionally or unintentionally share the same
> numeric discriminant (e.g. `46`, `54`) in the current contract source.
> This table reflects the enum as written in `fluxapay/src/lib.rs` rather
> than resolving that pre-existing duplication, which is out of scope for
> this change. Resolving it would alter wire-level error codes for
> existing integrators and should be its own tracked change.

## Refund-Specific Errors Most Relevant to Tiered Fees

| Error | When it's returned |
|---|---|
| `PaymentNotFound` | `process_refund_internal` cannot find the `Payment` record for the refund's `payment_id` (needed to resolve the merchant for the tier lookup). |
| `RefundAlreadyProcessed` | The refund is not `Pending` (already completed, rejected, or cancelled). |
| `RefundExpired` | Called after the refund's `expiry_at` has passed. |
| `Unauthorized` | `refund_instantly` rejects merchants whose KYC tier is not `Full` or `Business`, or whose `require_auth()`/blacklist checks fail. |
| `Reentrancy` | Reentrant or concurrent `process_refund` calls for the same `refund_id`, guarded by `DataKey::ReentrancyLock` / `DataKey::RefundLock`. |
