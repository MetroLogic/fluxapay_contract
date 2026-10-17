# FluxaPay Threat Model

This document describes the principal assets, trust assumptions, attack surfaces,
and mitigations for FluxaPay's Soroban contracts. It is a guide for development
and review, not a guarantee of security or a substitute for an independent audit.
The implementation and deployment configuration can change; verify the linked
source before relying on any control described here.

## 1. Assets at risk

| Asset | Why it matters | Primary source |
| ----- | -------------- | -------------- |
| Merchant and payer funds | USDC and other allowlisted tokens move through payment, settlement, refund, and dispute flows. Unauthorized transfers, incorrect amounts, or blocked withdrawals can cause direct loss. | [`fluxapay/src/lib.rs`](../fluxapay/src/lib.rs), [`fluxapay/src/dex_router.rs`](../fluxapay/src/dex_router.rs) |
| Treasury and fee balances | Accumulated fees and contract-held balances may be withdrawn or redirected by privileged actions or vulnerable contract logic. | [`fluxapay/src/lib.rs`](../fluxapay/src/lib.rs) |
| Administrator and operator keys | Admins configure roles and parameters; oracle and settlement operators can affect rates and payment outcomes. Key theft can therefore affect both funds and availability. | [`fluxapay/src/access_control.rs`](../fluxapay/src/access_control.rs), [`fluxapay/src/lib.rs`](../fluxapay/src/lib.rs) |
| FX oracle data and oracle authority | Settlement conversion depends on the correctness and freshness of stored rates and on who can submit them. | [`fluxapay/src/fx_oracle.rs`](../fluxapay/src/fx_oracle.rs) |
| Payment, refund, and dispute state | IDs, statuses, idempotency records, votes, and stakes constrain whether operations can be repeated and how funds are resolved. | [`fluxapay/src/lib.rs`](../fluxapay/src/lib.rs), [`fluxapay/src/access_control.rs`](../fluxapay/src/access_control.rs) |

## 2. Oracle manipulation

**Description.** A compromised or malicious oracle operator, manipulated upstream
price feed, or incorrect rate configuration can store a rate that misstates the
value used for currency conversion or settlement. An attacker who can prevent
updates may also leave users with unavailable rates.

**Potential impact.** Incorrect conversion amounts can underpay or overpay a
merchant or payer. A stale or missing rate can halt affected settlement flows.

**Current mitigations.** `FXOracle` requires the `ORACLE` role to submit rates.
Rate reads enforce a staleness threshold capped at 24 hours and a ledger-sequence
gap limit; stale rates are rejected. Per-pair deviation limits are available when
configured. An admin may configure a quorum of exact-matching submissions, and
permissionless staleness probes and events support monitoring. See
[`fx_oracle.rs`](../fluxapay/src/fx_oracle.rs) and the
[oracle staleness notes in SECURITY.md](../SECURITY.md#-fx-oracle-staleness-risk).

**Residual risk — High.** The quorum is optional (default 1), compares exact
rate-and-decimal values rather than aggregating independent prices, and does not
deduplicate submissions by operator. The batch update entry point stores rates
directly rather than applying that quorum. A compromised authorized operator or
upstream source can therefore still submit a bad rate; deviation limits also
depend on their configured values. Staleness guards limit old data, not
manipulation of fresh data. Independent feed operation, quorum configuration,
deviation limits, and monitoring remain operational responsibilities.

## 3. Admin key compromise

**Description.** Theft or misuse of an admin key grants privileged access to
configuration and role-management operations. Impact depends on which contract
is affected and how its controls are configured.

**Potential impact.** An attacker may grant privileged roles, change
configurations, disrupt payment processing, or eventually execute an authorized
WASM upgrade that changes contract behavior and puts funds at risk.

**Current mitigations.** Admin authorization is checked for privileged
operations. A two-step admin transfer and configurable signer-threshold
proposals exist in [`access_control.rs`](../fluxapay/src/access_control.rs).
The PaymentProcessor queues selected critical actions—including WASM upgrades,
fee-rate changes, and KYC limit changes—behind a default 48-hour timelock, with
pending actions publicly queryable before execution. See
[`lib.rs`](../fluxapay/src/lib.rs).

**Residual risk — High.** The multisig initializes as one signer with a 1-of-1
threshold, so it provides no separation of authority until deliberately
reconfigured. Several admin operations are not covered by the timelock or
multisig proposal flow. The admin can also change the timelock delay, including
setting it to zero. A compromised admin can therefore still cause substantial
damage; controls must be configured, keys secured, and changes monitored by
operators.

## 4. Replay attacks and payment ID collisions

**Description.** Replaying an authorized operation or reusing a payment ID or
idempotency token could otherwise cause duplicate processing or confuse
off-chain reconciliation.

**Potential impact.** A successful replay or collision could create duplicate
payment records, trigger repeated business actions, or associate an off-chain
payment with the wrong on-chain record.

**Current mitigations.** Contract entry points use Soroban authorization via
`require_auth`, which delegates authorization nonce, expiration, and invocation
validation to the Soroban host. Payment creation rejects a `payment_id` that
already has a stored payment, rejects duplicate IDs within a batch, and
optionally maps a client token to the created payment to make retries
idempotent. Stateful checks also prevent repeated processing of completed
refunds and resolved disputes. See [`lib.rs`](../fluxapay/src/lib.rs). The
repository does not implement a custom relayer-based meta-transaction verifier
or application-defined domain separator; do not treat those as existing
controls.

**Residual risk — Medium.** Payment IDs and client tokens are not permanent
cryptographic replay guards: their uniqueness checks rely on stored contract
state, and payment records and idempotency records have bounded TTLs. Soroban
authorization and the state checks protect the implemented invocation flows,
but integrations that add signed messages or relayers must define and validate
their own replay protection, including network and contract binding where
appropriate.

## 5. Dispute system abuse

**Description.** Attackers may file excessive or low-quality disputes, attempt
to influence arbitration, or exploit the economic incentives of the bond and
stake-weighted voting design.

**Potential impact.** Abuse can consume contract resources, delay dispute
resolution, lock funds, or produce an unfair outcome that causes a refund or
payment to be released to the wrong party.

**Current mitigations.** Dispute creation requires the disputer's Soroban
authorization, validates payment and evidence inputs, requires both the
disputer and merchant to transfer a configured stablecoin-denominated bond,
and applies per-disputer open-dispute and global hourly rate limits.
Arbitration votes require locked stake and may be cast only once per arbitrator
per dispute; minority stake is subject to a 10% slash. Relevant implementation
is in [`lib.rs`](../fluxapay/src/lib.rs).

**Residual risk — Medium.** A bond and rate limits increase the cost and bound
the volume of abuse but cannot eliminate it; the global limit can also restrict
legitimate use. Voting weight is proportional to stake, so a large holder can
dominate a vote, and the incentives depend on the value and governance of the
stake token. Bond and rate-limit parameters, arbitrator independence, and
resolution operations require ongoing review.

## 6. Soroban-specific risks

**Description.** Contract state and execution depend on Soroban ledger rules,
including persistent-entry archival and restoration, storage TTL, and the
correctness of uploaded and installed WASM during upgrades.

**Potential impact.** An expired or archived entry may make a payment, role, or
other state temporarily unavailable and require a restoration transaction; if
state is not restored in time, affected flows can be disrupted. An unsafe or
malicious WASM upgrade can alter all contract behavior and put stored assets at
risk.

**Current mitigations.** The contracts explicitly bump TTLs for selected
long-lived state and provide bounded bulk TTL-maintenance entry points. The
PaymentProcessor queues WASM upgrades behind its default 48-hour timelock.
Relevant code is in [`lib.rs`](../fluxapay/src/lib.rs),
[`stream.rs`](../fluxapay/src/stream.rs), and
[`access_control.rs`](../fluxapay/src/access_control.rs).

**Residual risk — Medium.** TTL bumps cover selected records, not every entry
indefinitely; they require transactions and operational monitoring. Archival and
restoration availability depend on Soroban protocol behavior and network
operations. The timelock does not validate whether an uploaded WASM binary is
safe, and the admin can change its delay. Maintain restoration procedures,
monitor TTLs, review WASM artifacts and hashes, and treat upgrade authority as
highly privileged.

## 7. Out of scope

**Description.** FluxaPay's contracts do not control the security or availability
of all software, services, and infrastructure that interact with them.

**Potential impact.** Failures in excluded components or external systems can
still cause loss, privacy exposure, delayed settlement, or service disruption,
even if the on-chain contract behaves as designed.

**Current mitigations.** The on-chain code applies role checks, allowlists, and
contract-call validations to its own trust boundaries, but those controls do
not secure external systems. The security policy identifies excluded areas in
[`SECURITY.md`](../SECURITY.md#-out-of-scope).

**Residual risk — High / external.** Unless an issue has a direct on-chain
security impact, the following are outside this contract threat model: Stellar
network consensus, validator or RPC availability, user wallet and device
security, front ends and off-chain APIs, the indexer and deployment
infrastructure, fiat anchors and banking rails, and the internal security of
third-party tokens or DEX protocols. Economic and market risks are also not
eliminated by contract controls. These systems require their own threat models,
monitoring, and incident response.
