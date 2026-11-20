# External Audit Findings Register

**Issue:** #1166  
**Scope:** [`audits/SCOPE.md`](SCOPE.md)  
**Status:** No external audit has been performed yet. This register is ready to receive auditor findings.

All external audit findings must be recorded here with a severity and a remediation
status before they can be considered for the mainnet release gate. Critical and High
findings must be `remediated` and `re-checked` before any mainnet deployment.

| ID | Severity | Title | Affected component | Status | Remediation / re-check |
| -- | -------- | ----- | ------------------ | ------ | ---------------------- |
| — | — | No external audit findings recorded yet | — | — | — |

## Severity levels

- **Critical** — direct loss of user funds or full access-control bypass. Blocks mainnet.
- **High** — realistic path to fund loss, lockup, or unauthorized privileged action. Blocks mainnet.
- **Medium** — limited-impact logic or availability issue.
- **Low / Informational** — hardening and best-practice notes.

## Status values

`open` → `in_progress` → `remediated` → `re-checked` (and `accepted_risk` only for
Low/Informational findings with an explicit owner sign-off).

## Mainnet gate

Mainnet deployment remains blocked until an audit report is published and
`audits/external-audit-status.json` reports `audit_complete: true`,
`critical_findings_resolved: true`, and `high_findings_resolved: true`.
This file does not claim an audit or remediation has occurred.
