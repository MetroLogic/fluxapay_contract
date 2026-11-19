# Storage Key Layout and Migration Reference

This is the single reference for every persistent/instance storage key used across the
FluxaPay contracts: what each `DataKey` / `MerchantDataKey` variant stores, how its TTL
(time-to-live) is managed, and the migration history of keys that have changed shape or
location since the contracts were first written. It exists so that anyone changing a
storage key's type, adding a new key, or auditing an upgrade can see the full key space
in one place instead of grepping `fluxapay/src/*.rs`.

## Scope and a known architecture wrinkle

The two canonical, tested, deployed contracts (`PaymentProcessor` and `RefundManager`,
both defined at the crate root in `fluxapay/src/lib.rs` — see
[`docs/entrypoint-reference.md`](entrypoint-reference.md) for why the root definitions,
not the `fluxapay/src/payment_processor.rs` / `refund_manager.rs` module copies, are the
ones every test registers) share **one** `DataKey` enum, re-exported from
`fluxapay/src/data_keys.rs` via `pub use data_keys::*;` in `lib.rs`. This is the enum
documented below.

**Known wrinkle:** `fluxapay/src/lib.rs` *also* declares its own `pub enum DataKey { .. }`
directly (currently with `Payment(String)` instead of `data_keys::DataKey`'s
`Payment(BytesN<32>)`). Because an explicit local item always wins over a glob-imported
one with the same name in Rust's name resolution, unqualified `DataKey` usage inside
`lib.rs` (and in any file that does `use crate::*;`) resolves to this **local** `lib.rs`
copy, not the `data_keys.rs` one re-exported for external consumers. The two enums have
drifted out of sync (the local copy was not updated when `DataKey::Payment` was migrated
to a hashed `BytesN<32>` key — see Issue #853 below). This reference documents the
`data_keys.rs` definition, since that is the one `payment_id_to_key`-based call sites
(e.g. `register_payment`) are written against and the one intended as current; the
`lib.rs`-local copy should be treated as stale and is flagged here, not silently hidden,
so it gets cleaned up rather than rediscovered from scratch later.

`MerchantRegistry` (`fluxapay/src/merchant_registry.rs`) has its own, independent
`MerchantDataKey` enum with no such duplication.

Several other contracts keep their own small, contract-local key enums that never
overlap with `DataKey`/`MerchantDataKey` (different `#[contracttype]`, different
contract instance storage). These are listed briefly at the end rather than given full
tables, since each is a handful of variants: `StreamDataKey` (`stream.rs`), `HtlcDataKey`
(`htlc.rs`), `AccessControlDataKey` (`access_control.rs`), plus small `DataKey` enums
local to `gas_estimator.rs` and `subscription.rs`.

## TTL model

Soroban persistent storage entries expire after their TTL (in ledgers, ~5s each) unless
extended. FluxaPay uses two named tiers, defined once near the top of `lib.rs`:

| Constant | Value (ledgers) | Approx. wall time | Used for |
|---|---|---|---|
| `SHORT_LIVE_TTL` | 120,960 | ~1 week | Keys backing a short-lived/in-progress state (e.g. a `Pending` payment, an `Open`/`UnderReview` dispute) |
| `LONG_LIVE_TTL` | 18,921,600 | ~3 years | Keys backing a settled/terminal state, or admin/config state that should not lapse |

Two keys get a bespoke TTL instead of one of the two tiers above, each tied to a
specific window rather than the state-machine status of the record:

| Constant | Value (ledgers) | Approx. wall time | Key |
|---|---|---|---|
| `PAYMENT_IDEMPOTENCY_TTL_LEDGERS` | `(24*60*60)/5` = 17,280 | 24 hours | `DataKey::PaymentIdempotencyKey` (Issue #761) |
| `REFUND_IDEMPOTENCY_TTL_LEDGERS` | `(30*24*60*60)/5` = 518,400 | 30 days | `DataKey::RefundIdempotencyKey` (Issue #638), matching the `client_token` retention window |

The dominant pattern (`PaymentProcessor::bump_ttl` / `bump_payment_ttl`, mirrored in
`RefundManager`) is: on every write to a status-bearing record, call
`env.storage().persistent().extend_ttl(key, threshold, ttl)` where `ttl` is chosen by a
small per-record-kind function and `threshold` is `max(1, ttl / TTL_BUMP_THRESHOLD_DIVISOR)`
(`TTL_BUMP_THRESHOLD_DIVISOR = 5`, i.e. the entry is refreshed once it's within 20% of
expiring). The two concrete mappings in the source today:

- **`payment_ttl(status)`**: `Pending` → `SHORT_LIVE_TTL`; `Confirmed`, `Settled`,
  `Expired`, `Failed`, `PartiallyPaid`, `Overpaid`, `Disputed` → `LONG_LIVE_TTL`.
- **`dispute_ttl(status)`**: `Open`, `UnderReview` → `SHORT_LIVE_TTL`; `Resolved`,
  `Rejected` → `LONG_LIVE_TTL`.

Counters, indexes, and admin/config keys (e.g. `RefundCounter`, `DisputeIndex`,
`RefundFeeBps`, `TreasuryMultisigConfig`) are generally bumped to `LONG_LIVE_TTL` on
write via the same `bump_ttl` helper, since they must never lapse while the contract is
in use. Keys not listed with a specific TTL below follow this long-lived default unless
their own doc comment in source says otherwise.

## `DataKey` (shared by `PaymentProcessor` and `RefundManager`)

Source: `fluxapay/src/data_keys.rs` (117 variants).

| Key | Semantics |
|---|---|
| `DataKey::Payment(BytesN<32>)` | _(see variant name; no doc comment in source)_ |
| `DataKey::PaymentStatusHistory(String)` | _(see variant name; no doc comment in source)_ |
| `DataKey::MerchantPayments(Address)` | _(see variant name; no doc comment in source)_ |
| `DataKey::MerchantRateLimit(Address)` | _(see variant name; no doc comment in source)_ |
| `DataKey::Refund(String)` | _(see variant name; no doc comment in source)_ |
| `DataKey::PaymentRefunds(String)` | _(see variant name; no doc comment in source)_ |
| `DataKey::RefundCounter` | _(see variant name; no doc comment in source)_ |
| `DataKey::Dispute(String)` | _(see variant name; no doc comment in source)_ |
| `DataKey::PaymentDisputes(String)` | _(see variant name; no doc comment in source)_ |
| `DataKey::DisputeCounter` | _(see variant name; no doc comment in source)_ |
| `DataKey::DisputeIndex` | Global append-only index of all dispute IDs, for paginated cross-merchant enumeration via `get_all_disputes`. |
| `DataKey::Stream(String)` | _(see variant name; no doc comment in source)_ |
| `DataKey::TreasuryBalance` | _(see variant name; no doc comment in source)_ |
| `DataKey::TreasuryBalanceByToken(Address)` | _(see variant name; no doc comment in source)_ |
| `DataKey::UsdcToken` | _(see variant name; no doc comment in source)_ |
| `DataKey::Paused` | _(see variant name; no doc comment in source)_ |
| `DataKey::CreationPaused` | _(see variant name; no doc comment in source)_ |
| `DataKey::MerchantRegistryAddress` | _(see variant name; no doc comment in source)_ |
| `DataKey::AllowedToken(Address)` | _(see variant name; no doc comment in source)_ |
| `DataKey::Blacklisted(Address)` | _(see variant name; no doc comment in source)_ |
| `DataKey::MerchantAmountLimits(Address)` | _(see variant name; no doc comment in source)_ |
| `DataKey::GlobalAmountLimits` | _(see variant name; no doc comment in source)_ |
| `DataKey::IdempotencyKey(String)` | _(see variant name; no doc comment in source)_ |
| `DataKey::SubscriptionPlan(String)` | _(see variant name; no doc comment in source)_ |
| `DataKey::Subscription(String)` | _(see variant name; no doc comment in source)_ |
| `DataKey::PayerSubscriptions(Address)` | _(see variant name; no doc comment in source)_ |
| `DataKey::SubscriptionCounter` | _(see variant name; no doc comment in source)_ |
| `DataKey::StreamCounter` | _(see variant name; no doc comment in source)_ |
| `DataKey::DisputeOperatorNote(String)` | Stores operator notes keyed by dispute_id for on-chain transparency. |
| `DataKey::DisputeArbitratorVotes(String)` | Stores all arbitrators who have voted on a dispute. |
| `DataKey::DisputeStake(String, Address)` | Locked stake for a dispute arbitrator: (dispute_id, arbitrator) → amount |
| `DataKey::DisputeVote(String, Address)` | Vote cast by an arbitrator: (dispute_id, arbitrator) → StakeWeightedVote |
| `DataKey::DisputeVoteTally(String)` | Tally of votes for a dispute |
| `DataKey::WeightedQuorumBps` | Issue #843: Admin-configurable weighted quorum in basis noints. |
| `DataKey::PendingWasmUpgrade` | Issue #846: Pending time-locked WASM upgrade proposal. |
| `DataKey::FxOracleAddress` | Cross-contract address of the configured FX oracle (Issue #304). |
| `DataKey::RequireReceiptHash` | Whether `process_refund` requires a `receipt_hash` on refunds (Issue #176). |
| `DataKey::DexRouterAddress` | Cross-contract address of the configured DEX router (Issue #173). |
| `DataKey::RefundExpirySecs` | Configurable refund expiry window in seconds (Issue #170). |
| `DataKey::ArbitratorVote(String, Address)` | Vote cast by an arbitrator under the simple ARBITRATOR-role voting flow: (dispute_id, arbitrator) → ArbitratorVoteChoice. |
| `DataKey::ArbitratorVoteTally(String)` | Tally of ARBITRATOR-role votes for a dispute. |
| `DataKey::FeeSplitConfig` | Issue #168: Fee split configuration (treasury_bps, developer_bps, treasury_addr, developer_addr) |
| `DataKey::MerchantMonthlyVolume(Address, u32)` | Monthly volume tracker: (merchant_id, month_epoch) → i128 cumulative amount |
| `DataKey::MerchantCumulativeVolume(Address)` | Cumulative all-time payment volume per merchant for KYC tier auto-upgrades (issue #207). |
| `DataKey::FeeProposal` | _(see variant name; no doc comment in source)_ |
| `DataKey::CurrentFee` | _(see variant name; no doc comment in source)_ |
| `DataKey::GlobalRateLimit` | _(see variant name; no doc comment in source)_ |
| `DataKey::MerchantSpecificRateLimit(Address)` | _(see variant name; no doc comment in source)_ |
| `DataKey::PayerRateLimit(Address)` | _(see variant name; no doc comment in source)_ |
| `DataKey::MerchantDisputeCount(Address)` | Issue #184: Total disputes filed against a merchant (keyed by merchant address). |
| `DataKey::MerchantPaymentCount(Address)` | Issue #184: Total confirmed payments registered for a merchant (keyed by merchant address). |
| `DataKey::CollaborativeSettlement(String)` | Issue #185: Collaborative settlement record for a dispute. |
| `DataKey::UsageMetricsLog(String)` | Issue #664: Append-only log of `UsageMetrics` records for a subscription, keyed by subscription_id. |
| `DataKey::SupportedTokens` | Issue #301: List of supported token addresses for enumeration. |
| `DataKey::KycTierLimitsConfig` | Issue #303: KYC tier limits configuration. |
| `DataKey::ActiveSubscriptions` | Issue #302: List of active subscription IDs for process_due_subscriptions. |
| `DataKey::FXOracleAddress` | Issue #304: FX Oracle contract address for rate staleness checks. |
| `DataKey::SubscriptionTickCounter` | Issue #302: Counter for subscription tick payment IDs. |
| `DataKey::ReentrancyLock` | Issue #313: Reentrancy lock for process_refund_internal and settle_payment. |
| `DataKey::RefundLock(String)` | Per-refund reentrancy flag set for the duration of `process_refund_internal`. |
| `DataKey::DisputeRateLimits` | Admin-configurable dispute rate limits (`DisputeRateLimitConfig`). |
| `DataKey::PayerOpenDisputeCount(Address)` | Number of open/under-review disputes for a disputer address. |
| `DataKey::GlobalDisputeCreationRate` | Fixed-window global dispute creation counter (`DisputeCreationRateState`). |
| `DataKey::RequireEvidenceCid` | When true, non-empty dispute evidence must be a valid IPFS CID. |
| `DataKey::ContractVersion` | Contract version string, updated on each successful upgrade. |
| `DataKey::SettlementFeeRate` | Configurable settlement fee rate in basis noints (issue: settle_payment fee). |
| `DataKey::DisputeBondAmount` | Configurable dispute bond amount in stablecoin stroops (overrides DISPUTE_BOND_AMOUNT const). |
| `DataKey::DisputeDeadlineThresholdAmount` | Admin-configurable amount threshold for 3-day versus 7-day dispute deadlines. |
| `DataKey::TierVolumeCap(KycTier)` | Configurable monthly volume cap per KYC tier in stablecoin stroops (overrides TIER_CAP_* const). |
| `DataKey::RefundFeeBps` | Configurable refund fee in basis noints (overrides REFUND_FEE_BPS const). |
| `DataKey::AutoRefundOverpayment` | Issue #471: Whether overpaid payments automatically create a pending refund. |
| `DataKey::RefundCooldownSecs` | Configurable refund cooldown period in seconds (overrides REFUND_COOLDOWN_SECS const). |
| `DataKey::FeeWaiverCode(String)` | Admin-managed reusable fee-waiver code registry for per-payment promotions. Keyed by the code string itself. |
| `DataKey::AllowProratedRefunds` | When true, `cancel_subscription` may create a prorated pending refund. |
| `DataKey::TreasuryWithdrawalHistory` | Paginated log of treasury withdrawals (newest-first, capped at 100). |
| `DataKey::TreasuryMultisigConfig` | Treasury multisig configuration for withdrawal governance (M-of-N + timelock). |
| `DataKey::TreasuryProposalCounter` | Counter for treasury withdrawal proposal IDs. |
| `DataKey::TreasuryWithdrawalProposal(String)` | Treasury withdrawal proposals keyed by proposal ID. |
| `DataKey::TokenTreasuryBalance(Address)` | Per-token treasury balance for multi-token support. |
| `DataKey::DirectTransferPayment(String)` | Issue #485: Marks a payment as created from a direct_transfer payment link. Prevents future disputes from being created for this payment. (Note: the preceding `TokenTreasuryBalance` variant in source carries a second, orphaned doc line reading "Per-token treasury balance keyed by token address (issue: RefundManager.TreasuryBalance)." which appears to be leftover from a rename and is reproduced here as a flag for cleanup rather than attributed to this key.) |
| `DataKey::TokenCurrency(Address)` | Issue #483: Maps token address to its currency symbol (e.g., USDC, EURC, BRLT). |
| `DataKey::Invoice(String)` | _(see variant name; no doc comment in source)_ |
| `DataKey::MerchantInvoices(Address)` | _(see variant name; no doc comment in source)_ |
| `DataKey::InvoiceCounter` | _(see variant name; no doc comment in source)_ |
| `DataKey::PaymentRetries(String)` | Issue #482: Payment retry chain tracking - maps original_id to list of retry payment IDs |
| `DataKey::MaxRateDeviation(Symbol)` | Issue #478: FX oracle max rate deviation per currency pair in basis noints |
| `DataKey::DisputeThreshold` | Issue #481: Admin-configurable dispute threshold for auto-suspension |
| `DataKey::MinPaymentDurationSecs` | Minimum payment duration in seconds (default: CREATE_PAYMENT_WINDOW_SECS = 60). |
| `DataKey::MaxPaymentDurationSecs` | Maximum payment duration in seconds (default: 30 days). |
| `DataKey::MetadataHashPayment(BytesN<32>)` | Issue #489: Reverse index from metadata_hash to payment_id for order reconciliation. |
| `DataKey::CustomerProfile(Address, Address)` | Issue #492: Customer profile keyed by (merchant_id, customer_id) for CRM features. |
| `DataKey::AllowedRouter(Address)` | Issue #437: Allowlisted DEX router address |
| `DataKey::AllowedRoutersList` | Issue #437: List of allowlisted DEX router addresses |
| `DataKey::WrappedXlmContract` | Issue #434: Wrapped XLM (WXLM) token contract address |
| `DataKey::PaymentsByExpiry(u32)` | Issue #504: Payment IDs grouped by approximate expiry ledger bucket. |
| `DataKey::PaymentExpiryBuckets` | Issue #504: Sorted set of expiry buckets that currently contain payment IDs. |
| `DataKey::DailyPaymentIndex(Address, u64)` | Issue #678: Daily-bucketed payment ID index for O(days) analytics queries. Key: (merchant_id, day_bucket = created_at / 86_400) → Vec<payment_id>. |
| `DataKey::FeeCollectionHistory` | Issue #666: Paginated log of platform-fee collection events (newest-first, capped at `FEE_COLLECTION_HISTORY_CAP`), consumed by `get_platform_fee_report`. |
| `DataKey::PaymentLinkManagerAddress` | Address of the configured PaymentLinkManager contract for invoice cross-calls. |
| `DataKey::InvoiceGracePeriodSecs` | Configurable invoice overdue grace period in seconds (Issue #607). |
| `DataKey::ContractMetadata(Symbol)` | Issue #667: Arbitrary on-chain contract metadata keyed by an admin-chosen symbol. |
| `DataKey::MerchantGrossVolume(Address)` | Issue #628: Cumulative gross payment volume per merchant. |
| `DataKey::TrackedMerchants` | Issue #628: Merchants that have had at least one payment created. |
| `DataKey::RefundIdempotencyKey(String)` | Issue #638: Refund idempotency key to `RefundIdempotencyRecord`. |
| `DataKey::PlanSubscribers(String)` | Issue #633: Subscription IDs for a plan. Appended to preserve earlier discriminants. |
| `DataKey::TimelockDelaySecs` | Issue #624: Timelock delay in seconds for critical admin operations. |
| `DataKey::PendingTimelockAction(String)` | Issue #624: Pending timelocked action keyed by action ID. |
| `DataKey::TimelockActionCounter` | Issue #624: Counter for pending action IDs. |
| `DataKey::PaymentIdempotencyKey(String)` | Issue #761: Payment idempotency key to payment id. |
| `DataKey::AutoRefundQueue` | Queued auto-refund created when a partially funded payment expires. Appended so earlier discriminants stay stable. |
| `DataKey::RefundIndex` | Global append-only index of all refund IDs for paginated enumeration. |
| `DataKey::PauseScope(PauseScope)` | Pause state keyed by operation scope. Appended to preserve existing storage keys. |
| `DataKey::OracleStalenessThreshold` | Issue #1033: Admin-configurable override (in seconds) for `MAX_ORACLE_STALENESS_SECS`, enforced in `confirm_payment` independently of the FX oracle's own internal staleness threshold. |
| `DataKey::DepositAddressPending(Address)` | Issue #1032: Maps a `deposit_address` to the `payment_id` currently holding it while that payment is `Pending`, so a new `create_payment` can detect and reject concurrent reuse of the same address. |
| `DataKey::MerkleDistributionCounter` | Issues #1026 / #1027: Counter for Merkle batch distribution IDs. |
| `DataKey::MerkleDistribution(u64)` | Issues #1026 / #1027: Merkle batch distribution record keyed by distribution_id. |
| `DataKey::TreasurySingleAdminThreshold` | Issue #1013: Configurable amount below which `withdraw_treasury` may be executed by a single admin without going through multisig governance. |

## `MerchantDataKey`

Source: `fluxapay/src/merchant_registry.rs` (22 variants). Instance storage unless noted;
`MerchantRegistry` uses the same `bump_ttl`-style refresh-on-write pattern, generally at
`LONG_LIVE_TTL` since merchant records are long-lived reference data.

| Key | Semantics |
|---|---|
| `MerchantDataKey::Merchant(Address)` | _(see variant name; no doc comment in source)_ |
| `MerchantDataKey::Admin` | _(see variant name; no doc comment in source)_ |
| `MerchantDataKey::MerchantList` | Stores the list of all registered merchants for enumeration |
| `MerchantDataKey::RefundManagerAddress` | Optional: Address of the RefundManager contract for automatic MERCHANT role granting |
| `MerchantDataKey::FeeConfig` | Platform fee configuration |
| `MerchantDataKey::PaymentProcessorAddress` | Address of the PaymentProcessor contract for automatic KYC tier upgrades (issue #207) |
| `MerchantDataKey::MerchantPayoutHistory(Address)` | Ordered list of previous payout addresses for a merchant (audit trail). |
| `MerchantDataKey::TierLimit(KycTier)` | KYC tier limits |
| `MerchantDataKey::MerchantCustomerWhitelist(Address)` | Customer whitelist for a merchant in whitelist mode (issue #516) |
| `MerchantDataKey::MerchantPendingSettlement(Address)` | Issue #480: Accumulated confirmed payment net amounts awaiting settlement. |
| `MerchantDataKey::DisputeThreshold` | Issue #481: Admin-configurable dispute threshold for auto-suspension (default 10) |
| `MerchantDataKey::GlobalPaymentTolerance` | Global payment tolerance in basis points |
| `MerchantDataKey::SuspensionProposal(u64)` | _(see variant name; no doc comment in source)_ |
| `MerchantDataKey::SuspensionVote(u64, Address)` | _(see variant name; no doc comment in source)_ |
| `MerchantDataKey::SuspensionProposalCounter` | _(see variant name; no doc comment in source)_ |
| `MerchantDataKey::SuspensionThreshold` | _(see variant name; no doc comment in source)_ |
| `MerchantDataKey::ContractMetadata(Symbol)` | Issue #667: Arbitrary on-chain contract metadata (description, deployment notes, audit commit hash, etc.), keyed by an admin-chosen Symbol. |
| `MerchantDataKey::MerchantMonthlyVolume(Address, u32)` | Issue #777: Monthly processing volume per merchant, keyed by (merchant_address, month_code) |
| `MerchantDataKey::ContractVersion` | Stored semantic contract version. Appended to preserve existing key discriminants. |
| `MerchantDataKey::MerchantReserveBuckets(Address)` | Issue #1017: FIFO queue of rolling-reserve buckets currently held for a merchant. |
| `MerchantDataKey::MerchantReserveLocked(Address)` | Issue #1017: Running total of reserve currently locked (unreleased) for a merchant. |
| `MerchantDataKey::MerchantReserveBucketCounter(Address)` | Issue #1017: Monotonic reserve bucket id counter, unique per merchant. |

## Migration notes (breaking storage changes)

Each entry below is sourced from [`CHANGELOG.md`](../CHANGELOG.md); see that file for
full PR/issue context. Per `docs/BREAKING_CHANGES.md`, a storage-key rename, removal, or
type/structure change without a migration path is a breaking change.

### `DataKey::Payment`: `String` → `BytesN<32>` (Issue #853)

`DataKey::Payment` switched from holding the raw `payment_id` `String` to a fixed-size
`BytesN<32>` — the SHA-256 hash of the payment ID, computed by
`payment_id_to_key(env, payment_id)` (`fluxapay/src/data_keys.rs`). This reduces Soroban
storage rent and CPU cost on create/read/update for high-volume merchants, since a fixed
32-byte key is cheaper to hash into the ledger than an arbitrary-length string.

- **Migration:** existing `Payment(String)` entries written before this change are not
  automatically re-keyed; readers must know which key shape a given record was written
  under. New code should always go through `payment_id_to_key`/`register_payment` rather
  than constructing `DataKey::Payment(..)` by hand.
- **Status:** as noted in "Scope and a known architecture wrinkle" above, the `lib.rs`
  local `DataKey` enum was not updated to match — it still declares
  `Payment(String)`. Any code path that resolves `DataKey` to that local copy is on the
  pre-#853 key shape; this divergence needs reconciling as part of closing out the
  migration, not just documenting it.

### `DataKey::RefundFeeBps` moved to instance storage (Issue #379)

The admin-configurable refund fee (`RefundFeeBps`, basis points, default 100 = 1%,
0–1000 allowed) moved from being read only via a persistent-storage, multisig-gated path
to being initialized into **instance** storage during `initialize_refund_manager`, with
a plain `set_refund_fee_bps(admin, bps)` setter and `get_refund_fee_bps()` getter.
`process_refund` now reads the fee from instance storage on every call.

- **Migration:** a `RefundManager` instance deployed before this change has no
  `RefundFeeBps` instance entry until `initialize_refund_manager` (or an explicit
  `set_refund_fee_bps` call) runs again; `get_refund_fee_bps()` falls back to the
  `REFUND_FEE_BPS` constant default (100 bps) when the instance key is absent.

### `DataKey::IdempotencyKey` TTL change + new reverse-map key (Issue #399)

`DataKey::IdempotencyKey` entries are now stored with a TTL derived from the payment's
own expiry window (`(expires_at − now) / 5` ledgers, minimum `SHORT_LIVE_TTL`) instead of
the permanent `LONG_LIVE_TTL` used previously. A new key,
`DataKey::PaymentIdempotencyToken(payment_id)`, was added as a **reverse map** from
`payment_id` back to the `client_token`, so `cancel_payment` and `expire_payment` can
proactively delete the forward `IdempotencyKey` entry and free the `client_token` for
reuse immediately, rather than waiting out the old permanent TTL.

- **New key:** `DataKey::PaymentIdempotencyToken(String)` — written and removed
  alongside `DataKey::IdempotencyKey`.
- **Migration:** `IdempotencyKey` entries written by contract versions before this
  change carry the old `LONG_LIVE_TTL` and have no corresponding
  `PaymentIdempotencyToken` reverse-map entry; they are not retroactively fixed; they
  will simply expire naturally under their original (longer) TTL rather than being
  proactively cleaned up on cancel/expire.

### Treasury accounting: fee accumulation into `DataKey::TreasuryBalance` (Issue #291)

Refund-time fees now accumulate into `DataKey::TreasuryBalance` (and, for multi-token
support, `DataKey::TokenTreasuryBalance(Address)`), with `get_treasury_balance()` and
`withdraw_treasury(admin, amount, destination)` added for admin withdrawals. Introduces
`Error::InsufficientTreasuryBalance`. Withdrawals are paginated/logged via
`DataKey::TreasuryWithdrawalHistory` and, for larger amounts, go through the
`DataKey::TreasuryMultisigConfig` / `DataKey::TreasuryWithdrawalProposal(String)` /
`DataKey::TreasuryProposalCounter` governance keys rather than a single-admin call
(see `DataKey::TreasurySingleAdminThreshold`, Issue #1013, for the single-admin-allowed
threshold below which multisig is skipped).

## Other contract-local key enums

These never collide with `DataKey`/`MerchantDataKey` — each is a separate
`#[contracttype]` scoped to its own contract's instance storage.

- **`StreamDataKey`** (`fluxapay/src/stream.rs`, backs the `PaymentStreaming` contract):
  `Stream(String)`, `MultiStream(String)`, `MultiStreamCounter`, `StreamFeeBps`,
  `StreamFeeRecipient`.
- **`HtlcDataKey`** (`fluxapay/src/htlc.rs`, backs the `Htlc` contract): `Counter`
  (monotonically increasing HTLC id generator), `Htlc(String)`.
- **`AccessControlDataKey`** (`fluxapay/src/access_control.rs`, used by every contract's
  role/admin checks via `AccessControl::has_role` etc.): `Role(Symbol, Address)`,
  `Admin`, `RoleMembers(Symbol)`, `PendingRevocation(Symbol, Address)`, `RecoveryKey`,
  `PendingAdminTransfer`, `AdminTransferLockIn`, `MultisigConfig`, `NextProposalNonce`,
  `Proposal(u64)`.
- **`DataKey`** local to `fluxapay/src/gas_estimator.rs` (backs the `GasEstimator`
  contract — unrelated to the shared `DataKey` above despite the same name, since it's a
  different module/type): `CachedBalance`, `PaymentProcessor` (the configured
  `PaymentProcessor` address whose pause flag this estimator treats as authoritative).
- **`DataKey`** local to `fluxapay/src/subscription.rs` (backs `SubscriptionContract` —
  again a distinct, same-named type, not to be confused with the shared `DataKey`):
  `Plan(Symbol)`, `Subscription(Symbol, Address)`, `MerchantPlans(Address)`, `Admin`.

## Keeping this in sync

Per [CONTRIBUTING.md](../CONTRIBUTING.md#75-storage-key-layout-reference), any PR that adds,
removes, renames, or changes the stored type of a `DataKey` / `MerchantDataKey` (or other
contract-local key enum) variant must update the corresponding table/section of this file
in the same PR, and — if the change is breaking per
[`docs/BREAKING_CHANGES.md`](BREAKING_CHANGES.md) — add a dated entry under "Migration
notes" here in addition to the required `CHANGELOG.md` entry.
