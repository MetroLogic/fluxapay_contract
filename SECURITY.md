# Security Policy

FluxaPay takes the security of our smart contracts and user funds (USDC) extremely seriously. This document outlines our vulnerability disclosure policy, disclosure timeline, audit status, and mainnet readiness requirements.

## 🛡️ Vulnerability Disclosure Policy & Response Process

If you discover a security vulnerability, we encourage you to report it to us responsibly. We will acknowledge receipt of your report and provide a status update as we investigate and address the issue.

### Reporting a Vulnerability

Please send an email to: **security@fluxapay.com** or **security@metrologic.com**

To help us prioritize and address your report, please include:
- A detailed description of the vulnerability.
- Steps to reproduce the issue (PoC code or clear instructions).
- Your assessment of the impact.

### Response & Disclosure Timeline SLA

- **Triage Acknowledgment**: Within 24 hours of receipt.
- **Critical Fix Commitment**: Fix developed, tested, and staged within 7 days.
- **Full Disclosure Timeline**: Public disclosure coordinated 30 days after fix deployment.

### CVE Response Process
1. **Receipt & Triage (0-24h)**: Report acknowledged and assigned to security team.
2. **Impact Assessment & CVE Reservation (24-48h)**: Severity rated; CVE ID requested if applicable.
3. **Patch Development & Testing (Day 1-7)**: Fix implemented and validated via test suites.
4. **Remediation & Advisory (Day 7-30)**: Security advisory published to repository and CVE record updated.

### Scope

- **In-Scope Contracts**: All core Soroban smart contracts listed in this repository:
  - `PaymentProcessor` (`fluxapay/src/lib.rs`)
  - `RefundManager` (`fluxapay/src/refund.rs`)
  - `FXOracle` (`fluxapay/src/fx_oracle.rs`)
  - `MerchantRegistry` (`fluxapay/src/merchant.rs`)
  - `PaymentLinkManager` (`fluxapay/src/payment_link.rs`)
- **Out-of-Scope**:
  - Third-party oracle price feeds and external data providers.
  - Core Stellar/Soroban protocol layer itself.
  - Web interface social engineering / phishing outside repository codebase.

## 🔍 Audit Status

Audit documentation and status manifests are tracked under the `audits/` directory:
- [audits/SCOPE.md](audits/SCOPE.md)
- [audits/external-audit-status.json](audits/external-audit-status.json)

| Audit Date | Auditor  | Scope           | Status               | Report Link        |
| ---------- | -------- | --------------- | -------------------- | ------------------ |
| 2026-03-27 | Internal | All Contracts   | Completed (Internal) | N/A                |
| TBD        | External | Mainnet Release | **In Progress**      | Refer to `audits/` |

## 💰 Bug Bounty Program

Discretionary rewards are available for high-impact, responsibly disclosed vulnerabilities reported in accordance with this policy.

| Milestone | Status |
| --------- | ------ |
| External audit complete | In Progress |
| Public program launched | Active |
