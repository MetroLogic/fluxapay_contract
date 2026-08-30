# Security Policy

FluxaPay takes the security of our Soroban smart contracts and user funds (USDC) extremely seriously. This document is the authoritative reference for our vulnerability disclosure policy, CVE assignment process, bug bounty scope, audit status, and contact details.

> [!IMPORTANT]
> This project is in **active development** and has not yet completed an external security audit. Do not deploy to mainnet until the external audit is complete. See [Audit Status](#-audit-status) below.

---

## 📬 Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Report security issues privately via one of the following channels:

| Channel | Details |
| ------- | ------- |
| **Email** | security@fluxapay.com |
| **GitHub Private Advisory** | [Submit via GitHub Security Advisories](../../security/advisories/new) |

To help us triage quickly, please include:

- A clear description of the vulnerability and its impact.
- The affected contract(s) and function(s) (see [In-Scope](#-in-scope) below).
- Step-by-step reproduction instructions or a proof-of-concept.
- Your assessment of severity (Critical / High / Medium / Low).
- Any suggested mitigations or patches you have already identified.

We do **not** require a PGP-encrypted submission, but if you prefer to encrypt your report, contact us first at security@fluxapay.com to exchange keys.

---

## ⏱️ Disclosure Timeline

We follow a structured, time-boxed process from receipt to public disclosure.

| Stage | SLA | Description |
| ----- | --- | ----------- |
| **Acknowledgment** | 24 hours | We confirm receipt of your report and assign a triage owner. |
| **Triage** | 72 hours | We reproduce and classify severity (Critical / High / Medium / Low). |
| **Fix commitment — Critical / High** | 7 days | We commit to a patch or mitigation plan with an estimated release date. |
| **Fix commitment — Medium / Low** | 30 days | We commit to a patch schedule. |
| **Patch release** | Varies | Critical: target ≤ 7 days from triage. High: ≤ 14 days. Medium/Low: next scheduled release. |
| **Full public disclosure** | 30 days after patch | We publish a post-mortem or advisory. May be accelerated by mutual agreement with the reporter. |

If we need more time than the above (e.g. coordinated third-party disclosure), we will communicate an updated timeline with the reporter before the deadline lapses. We will never unilaterally delay disclosure beyond **90 days** from initial receipt without the reporter's explicit agreement.

---

## 🆔 CVE Assignment Process

We work with CVE Numbering Authorities (CNAs) and MITRE to assign CVE IDs for publicly disclosed vulnerabilities that meet the bar for a CVE.

1. Once a patch is ready and tested, we request a CVE ID from [MITRE](https://cveform.mitre.org/) or via GitHub's CNA program.
2. The CVE is kept **reserved/private** until the patch is released.
3. On public disclosure day (≤ 30 days post-patch), we publish the full advisory including the CVE ID, affected versions, patched versions, CVSS score, and mitigation steps.
4. We reference the CVE from the GitHub Security Advisory and from the relevant entry in `CHANGELOG.md`.

Reporters who wish to be credited in the CVE advisory should indicate this in their report. Credit is given by name or handle at the reporter's preference.

---

## 🎯 Bug Bounty Program

A formal public bug bounty program is **planned for launch after the external audit completes** (see issue #381). Until then, we offer **discretionary rewards** for high-impact, responsibly disclosed vulnerabilities.

### Reward Tiers (Pre-Launch Discretionary)

| Severity | Criteria | Indicative Reward |
| -------- | -------- | ----------------- |
| **Critical** | Funds at risk (direct loss of USDC, unauthorized minting/burning, privilege escalation to admin) | Up to $10,000 USDC |
| **High** | Significant disruption (DoS of core payment flows, unauthorized role grant, oracle manipulation) | Up to $2,500 USDC |
| **Medium** | Limited impact, partial bypass of controls | Up to $500 USDC |
| **Low / Info** | Best-effort, no guarantee | N/A |

Rewards are at our sole discretion until the formal program launches. Duplicate reports, reports of known issues, and out-of-scope reports are ineligible.

---

## ✅ In-Scope

The following Soroban contracts and shared modules are fully in scope for security research:

### Deployable Contracts

| Contract | Source File | Key Responsibilities |
| -------- | ----------- | -------------------- |
| **PaymentProcessor** | `fluxapay/src/lib.rs` | Payment creation & settlement, fee splits, dispute lifecycle, subscriptions, streaming payments, cross-contract orchestration |
| **RefundManager** | `fluxapay/src/lib.rs` | Refund lifecycle, cooldown enforcement, collaborative settlement, DEX-routed refunds |
| **FXOracle** | `fluxapay/src/fx_oracle.rs` | Exchange rate storage, staleness checks, oracle role management |
| **MerchantRegistry** | `fluxapay/src/merchant_registry.rs` | Merchant registration, KYC tiers, volume caps, tier auto-upgrades |
| **PaymentLinkManager** | `fluxapay/src/payment_link.rs` | Payment link creation, usage tracking, multi-currency fiat config |
| **GasEstimator** | `fluxapay/src/gas_estimator.rs` | On-chain operation cost estimation |

### Shared Modules (compiled into the above)

| Module | Source File |
| ------ | ----------- |
| **AccessControl** | `fluxapay/src/access_control.rs` |
| **MerchantAuth** | `fluxapay/src/merchant_auth.rs` |
| **DexRouter** | `fluxapay/src/dex_router.rs` |
| **AccountAbstraction** | `fluxapay/src/account_abstraction.rs` |
| **PaymentStreaming** | `fluxapay/src/stream.rs` |
| **Utils** | `fluxapay/src/utils.rs` |

### High-Priority Attack Surfaces

- Unauthorized fund transfers (USDC theft, over-refund, fee bypass)
- Privilege escalation (unauthorized admin / role grant)
- Reentrancy or cross-contract call ordering bugs
- Oracle price manipulation or staleness bypass
- Idempotency key collision / duplicate payment
- Rate limit bypass or DoS of core payment flows
- Multi-sig proposal threshold bypass
- KYC tier cap bypass enabling volume over-limit

## Governance and Admin Controls

FluxaPay includes an on-chain multisig governance flow to require multiple approvals before high-risk administrative actions are executed. The design and rationale are documented in [docs/ADR-0004-multisig-admin-governance.md](docs/ADR-0004-multisig-admin-governance.md).

The policy is intentionally scoped to the `AdminAction` proposal set in the `AccessControl` module: fee-rate changes, treasury-related risk configuration, dispute bond and tier-cap adjustments, and protected role-management actions all require the configured signer threshold to be met before execution. The same document also explains the nonce-based replay protection and the 48-hour expiry window for unexecuted proposals.

---

## ❌ Out of Scope

The following are **not eligible** for rewards or coordinated disclosure:

- Stellar / Soroban platform vulnerabilities (report to Stellar Development Foundation)
- Front-end web applications, dashboards, or off-chain APIs
- TypeScript SDK (`sdk/`) — unless the issue causes an on-chain security impact
- Off-chain indexer (`indexer/`)
- Third-party DEX protocol internals (only the DexRouter interface to them is in scope)
- Infrastructure, CI/CD scripts, and deployment automation (except where they affect on-chain security assumptions)
- Economic / game-theoretic analysis of fee parameters (unless directly exploitable on-chain)
- Issues already listed in a public GitHub issue or known limitation in documentation
- Vulnerabilities requiring physical access to a developer's machine
- Social engineering attacks

---

## 🔍 Audit Status

Current audit status is tracked in machine-readable form at [`audits/external-audit-status.json`](audits/external-audit-status.json). Human-readable summary:

| Audit | Auditor | Scope | Status | Report |
| ----- | ------- | ----- | ------ | ------ |
| Internal (2026-03-27) | Internal team | All contracts | ✅ Completed | Internal only |
| External | TBD | Mainnet release scope | 🔄 Pending engagement | [Placeholder] |

Key milestones for the external audit (full scope in [`audits/SCOPE.md`](audits/SCOPE.md)):

| Milestone | Status |
| --------- | ------ |
| Audit scope document | ✅ Draft complete |
| Audit firm selection | 🔄 In progress (OtterSec, Trail of Bits, Halborn, CertiK under evaluation) |
| Engagement letter signed | ⏳ Pending |
| Audit execution | ⏳ Not started |
| Critical / High findings resolved | ⏳ Pending |
| Audit report published | ⏳ Pending |
| Bug bounty program launched | ⏳ Pending |

**Mainnet gate:** Production deployments are CI-blocked until `audits/external-audit-status.json` confirms `audit_complete`, `critical_findings_resolved`, and `high_findings_resolved` are all `true`. See [DEPLOYMENT.md](DEPLOYMENT.md) for the full gate checklist.

---

## 🔐 Code Ownership

Security-critical files require mandatory review from the security team as defined in [`.github/CODEOWNERS`](.github/CODEOWNERS). The following files trigger required reviews:

- `fluxapay/src/access_control.rs`
- `fluxapay/src/lib.rs`
- `fluxapay/src/merchant_auth.rs`
- `audits/external-audit-status.json`
- `SECURITY.md` / `.github/SECURITY.md`

---

## 📊 FX Oracle Staleness Risk

The `FXOracle` contract (`fluxapay/src/fx_oracle.rs`) provides exchange rates used for fiat settlement. Rate freshness is validated using both Stellar ledger close time and ledger sequence number.

| Risk | Description | Mitigation |
| ---- | ----------- | ---------- |
| **Ledger timestamp manipulation** | Validators can shift ledger close time within a small window (~seconds). | Hard 24-hour staleness cap (`MAX_RATE_AGE_SECS`) enforced regardless of admin-configured threshold. |
| **Misconfigured threshold** | An admin could set an excessively long threshold, allowing stale rates. | `effective_threshold = min(configured, MAX_RATE_AGE_SECS)` — the hard cap always wins. |
| **Compromised oracle key** | A delayed or malicious oracle could stop updating rates. | Ledger-sequence circuit breaker (`MAX_LEDGER_GAP`): if no update occurs within ~24 h of ledgers, `get_rate` and `get_settlement_amount` reject the rate and emit a `RATE/STALE_ALERT` event. |
| **Timestamp-only false positives** | Legitimate rates could be rejected on minor ledger time drift. | Accepted residual risk (seconds). A dual timestamp+sequence AND-check is tracked as follow-up #384. |

### Operational Guidance

- Monitor `RATE/STALE_ALERT` events via the indexer or webhook pipeline.
- Rotate oracle operator keys promptly if compromise is suspected.
- Keep the off-chain rate feed latency well below the 24-hour hard cap.
- Do not raise the admin staleness threshold above 24 hours — the hard cap cannot be bypassed.

---

## 🔒 Refund Reentrancy Protection

`process_refund` / `process_refund_internal` follow the **checks → effects → interactions** pattern to prevent double-refund via malicious token callbacks or nested cross-contract calls:

1. **Checks** — auth, role, refund exists, status is `Pending`, not expired.
2. **Effects** — set `refund.status = Completed` and persist under `DataKey::Refund` *before* any token transfer.
3. **Interactions** — call `token::transfer` only after effects are committed.

### Explicit locks

| Lock | Scope | Behavior |
|------|-------|----------|
| `DataKey::ReentrancyLock` | Contract-wide | Set for the duration of `process_refund_internal` / settle paths; cleared via `Drop` guard. |
| `DataKey::RefundLock(refund_id)` | Per refund ID | Set while processing that refund; concurrent/nested calls for the same ID return `Error::Reentrancy`. |

A second `process_refund` for an already-completed refund returns `RefundAlreadyProcessed`. `resolve_dispute_with_refund` persists dispute resolution effects before returning dispute bonds.

---

## 🤝 Responsible Disclosure Commitment

We commit to:

- Responding within 24 hours to confirm receipt.
- Keeping you informed of fix progress throughout the process.
- Crediting you publicly (by name or handle) in the advisory, unless you prefer anonymity.
- Not pursuing legal action against researchers who act in good faith and follow this policy.
- Publishing a post-mortem or public advisory within 30 days of the patch.

We ask reporters to:

- Give us reasonable time to investigate and patch before public disclosure.
- Not access, modify, or delete any user data beyond what is needed to demonstrate the vulnerability.
- Not perform denial-of-service attacks or spam.
- Report only to the channels listed above — not via social media, public forums, or third-party bug platforms until coordinated disclosure is complete.

---

*Last updated: 2026-07-24. For questions about this policy, email security@fluxapay.com.*
