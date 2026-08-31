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
| 404 | `PaymentNotFound` | No payment exists with the given `payment_id`. | Typo'd ID, or payment was never created. | Verify the ID via `get_payment` / listing endpoints. |
| 405 | `RefundNotFound` | No refund exists with the given `refund_id`. | Typo'd ID, or refund was never created. | Verify the ID via `get_refund` / `get_payment_refunds`. |
| 406 | `InvalidAmount` | Amount is zero, negative, or otherwise invalid. | Passing a non-positive amount to a payment/refund call. | Pass a strictly positive `i128` amount. |

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

## `FXOracleError` (`fluxapay/src/fx_oracle.rs`)

| Code | Name | Description | Common Cause | Remediation |
|------|------|-------------|---------------|-------------|
| 1 | `RateNotFound` | No rate is recorded for the requested currency pair. | Querying a pair the oracle hasn't been given a rate for. | Have the oracle publish a rate for that pair first. |
| 2 | `RateStale` | The recorded rate is older than the staleness threshold. | Oracle hasn't updated the rate recently enough. | Wait for (or trigger) a fresh oracle update. |
| 3 | `Unauthorized` | Caller is not an authorized oracle/admin. | Calling an oracle-only update function without the role. | Grant the oracle role, or call from an authorized address. |
| 4 | `BatchTooLarge` **/** `RateDeviationExceeded` ⚠️ | Batch rate update exceeds the max of 20 pairs, **or** rate deviation exceeds the configured limit — both variants share code 4 in the source. | Submitting > 20 pairs in one batch update, or a rate too far from the previous value. | Split batch updates to ≤ 20 pairs; check deviation limits separately. |

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
| 4 | `InvalidPayload` | The execution payload failed validation. | Malformed or empty payload/hash. | Pass a well-formed payload matching the expected schema. |

---

*Generated for #458. Keep this table in sync with the `#[contracterror]` enums it documents — `scripts/check-error-map-sync.ts` checks `FLUXAPAY_CONTRACT_ERROR_MAP` (Core table) against `fluxapay/src/lib.rs` on every CI run.*
