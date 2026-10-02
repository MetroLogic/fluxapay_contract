# FluxaPay External Smart Contract Audit — Scope Document

**Version:** 1.0  
**Status:** Draft (pending auditor engagement)  
**Target network:** Stellar Mainnet  
**Issue tracker:** #381

## Executive Summary

FluxaPay is a Soroban-based payment protocol that handles real USDC transfers, role-based access control, refunds, disputes, and merchant KYC tiering. This document defines the scope for an independent external security audit required before mainnet deployment.

## In-Scope Contracts

All five deployable contracts in the `fluxapay` workspace:

| Contract | Source | Responsibility |
| -------- | ------ | -------------- |
| **PaymentProcessor** | `fluxapay/src/lib.rs` | Payment creation, settlement, fee splits, disputes, subscriptions, cross-contract orchestration |
| **RefundManager** | `fluxapay/src/lib.rs` | Refund lifecycle, cooldown enforcement, collaborative settlement, DEX-routed refunds |
| **FXOracle** | `fluxapay/src/fx_oracle.rs` | Exchange rate storage, staleness checks, oracle role management |
| **MerchantRegistry** | `fluxapay/src/merchant_registry.rs` | Merchant registration, KYC tiers, volume caps, tier auto-upgrades |
| **PaymentLinkManager** | `fluxapay/src/payment_link.rs` | Payment link creation, usage tracking, multi-currency fiat config |

## In-Scope Shared Modules

These modules are compiled into the above contracts and must be reviewed as part of cross-contract security analysis:

| Module | Source | Relevance |
| ------ | ------ | --------- |
| **AccessControl** | `fluxapay/src/access_control.rs` | Role grants/revocations, admin transfer proposals, recovery keys, revocation cooldowns |
| **MerchantAuth** | `fluxapay/src/merchant_auth.rs` | Merchant authorization and pre-auth flows |
| **DexRouter** | `fluxapay/src/dex_router.rs` | Token swap routing for `swap_and_pay` and swap-based refunds |
| **AccountAbstraction** | `fluxapay/src/account_abstraction.rs` | Account abstraction helpers used in payment flows |
| **Stream** | `fluxapay/src/stream.rs` | Streaming payment logic |
| **Utils** | `fluxapay/src/utils.rs` | Shared validation and helper functions |

## Security Focus Areas

The auditor should prioritize the following threat categories:

### 1. Access Control & Privilege Escalation

- Role grant/revoke flows (`ADMIN`, `ORACLE`, `MERCHANT`, `SETTLEMENT_OPERATOR`, `ARBITRATOR`)
- Admin transfer proposals, multi-sig thresholds, and recovery key mechanisms
- Revocation cooldown bypass attempts
- Unauthorized contract initialization or re-initialization

### 2. Fund Safety & Token Handling

- USDC (and other SAC token) transfer correctness — amount, recipient, reentrancy
- Fee split arithmetic (treasury, developer, merchant) and rounding
- Refund routing: direct vs. DEX swap paths
- Dispute bond escrow and release conditions
- Subscription retry and cancellation fund handling

### 3. Business Logic & State Machine Integrity

- Payment status transitions (`PENDING` → `SETTLED` → `DISPUTED` → `REFUNDED`)
- Refund cooldown (`REFUND_COOLDOWN_SECS`) and expiry enforcement
- Idempotency key handling and duplicate payment prevention
- Rate limiting (per-merchant, per-payer, create-payment window)
- KYC tier volume caps and automatic tier upgrades
- Arbitration voting threshold (`ARBITRATOR_VOTING_THRESHOLD`)

### 4. Oracle & Price Manipulation

- FX rate staleness checks and stale-rate rejection
- Oracle role authorization for rate updates
- Multi-currency payment link fiat config validation
- DEX swap slippage and path manipulation in `swap_and_pay`

### 5. Cross-Contract Interactions

- PaymentProcessor → MerchantRegistry verification calls
- PaymentProcessor → RefundManager refund creation/processing
- PaymentProcessor → FXOracle rate lookups
- PaymentProcessor → PaymentLinkManager link usage
- RefundManager → DexRouter swap execution
- Reentrancy locks across inter-contract call boundaries

### 6. Denial of Service & Resource Exhaustion

- Storage TTL bump logic and ledger resource consumption
- Unbounded storage growth vectors
- Payment spam mitigation (`CREATE_PAYMENT_MAX_PER_WINDOW`)
- Pause/unpause emergency controls

## Out of Scope

- Stellar/Soroban platform vulnerabilities
- Front-end applications and SDK client code (`sdk/`, `bindings/`)
- Off-chain indexer (`indexer/`)
- Third-party DEX protocol internals (DexRouter interface only)
- Infrastructure, CI/CD, and deployment scripts (except where they affect on-chain security assumptions)
- Economic/game-theoretic analysis of fee parameters (unless exploitable on-chain)

## Deliverables

1. **Full audit report** with severity-classified findings (Critical, High, Medium, Low, Informational)
2. **Remediation verification** re-review of all Critical and High findings
3. **Executive summary** suitable for public disclosure
4. **Test coverage assessment** against the in-scope attack surface

## Acceptance Criteria

Per issue #381:

- [ ] Engagement letter signed with a reputable Soroban/Rust auditing firm
- [ ] All Critical and High severity findings resolved before mainnet
- [ ] Audit report (or public summary) published and linked from `SECURITY.md`
- [ ] `audits/external-audit-status.json` updated to reflect completion
- [ ] Bug bounty program launched post-audit (referenced in `SECURITY.md`)

## Candidate Auditors

The following firms have Soroban/Stellar or Rust smart contract audit experience and are under evaluation:

| Firm | Notes |
| ---- | ----- |
| [OtterSec](https://osec.io/) | Stellar ecosystem experience |
| [Trail of Bits](https://www.trailofbits.com/) | Rust/LLVM security expertise |
| [Halborn](https://www.halborn.com/) | Blockchain audit practice |
| [CertiK](https://www.certik.com/) | Formal verification and audit services |

> Final auditor selection is pending engagement negotiations. Update `audits/external-audit-status.json` when a firm is selected.

## Repository References

- Architecture overview: [`docs/architecture.md`](../docs/architecture.md)
- Security policy: [`SECURITY.md`](../SECURITY.md)
- Deployment checklist: [`DEPLOYMENT.md`](../DEPLOYMENT.md)
- Audit status (machine-readable): [`external-audit-status.json`](./external-audit-status.json)
