# Breaking Change Policy

This document defines what constitutes a breaking change in Fluxapay contracts, the required deprecation notice period, communication requirements, and versioning strategy.

## What Constitutes a Breaking Change

A breaking change is any modification to a deployed contract that causes existing client code, on-chain integrations, or dependent systems to fail without a code update or migration step.

### Examples of Breaking Changes

#### 1. **ABI Changes**
- Removing or renaming a public contract function
- Changing function parameter types or counts
- Changing return types of public functions
- Modifying function visibility (e.g., public → private)
- **Impact**: Any backend or frontend calling the contract will receive a function-not-found error

#### 2. **Storage Key Changes**
- Renaming or removing persistent storage keys
- Changing the structure of stored data without a migration path
- **Impact**: Contract upgrades fail or stored data becomes inaccessible

#### 3. **Error Code Changes**
- Renaming error variants
- Changing error codes or their numeric values
- Removing error types that callers may be catching
- **Impact**: Error handling logic in client applications breaks

#### 4. **Behavior Changes**
- Changing transaction validation rules (e.g., amount limits, expiration handling)
- Altering authorization or access control requirements
- Modifying fee calculations or settlement logic
- **Impact**: Legitimate transactions may be unexpectedly rejected

#### 5. **Function Removals**
- Deleting a public function entirely
- Removing helper contracts or oracles that other contracts depend on
- **Impact**: Integrations fail with immediate function-not-found errors

#### 6. **Type Structure Changes**
- Removing or renaming fields in `#[contracttype]` structs
- Changing the type of struct fields
- **Impact**: Deserialization fails; stored contracts stop functioning

---

## Non-Breaking Changes

The following are **not** considered breaking changes:

- Adding new public functions
- Adding optional parameters with defaults
- Adding new error variants (as long as existing ones are preserved)
- Expanding contract storage (as long as existing keys are not modified)
- Improving performance or efficiency without changing behavior
- Internal refactoring that maintains the same ABI
- Bug fixes that restore intended behavior (document clearly if a fix changes behavior)

---

## Deprecation Notice Period

### Standard Timeline

A minimum **30-day deprecation notice period** is required before a breaking change is deployed to:
- **Testnet**: Warnings must be published 30 days in advance
- **Production (Public Network)**: Warnings must be published 30 days in advance, with community review

### Exceptions

- **Security vulnerabilities**: If a breaking change fixes a critical security issue, the notice period may be reduced to **7 days** with explicit security advisory communication
- **Protocol updates**: If Stellar/Soroban requires network-level changes, notice may be adjusted accordingly

---

## Communication Requirements

### Before Deployment

1. **CHANGELOG.md Entry**
   - Add entry under an "Upcoming" or "Next Release" section
   - Tag with `[BREAKING]` prefix
   - Clearly describe:
     - What is changing
     - Why the change is necessary
     - Migration steps required for clients
     - Timeline for removal
   - Example:
     ```
     ## [Unreleased]
     
     ### Breaking Changes
     - [BREAKING] Removed `get_merchant_balance()` function (deprecated 2026-04-30). 
       Use `get_merchant()` and read the `balance` field instead.
       Migration: Update all calls from `get_merchant_balance(merchant_id)` 
       to `get_merchant(merchant_id).balance`.
     ```

2. **Pull Request Description**
   - Label PR with `[BREAKING]` tag in title
   - Explain in the PR description:
     - Justification for the breaking change
     - Steps affected clients must take
     - Deprecation timeline
   - Request review from maintainers and stakeholders
   - Allow at least 7 days for community feedback

3. **GitHub Release Notes**
   - Publish a detailed breaking-change notice in the Release notes
   - Link to migration guides in the docs/
   - Provide code examples for migration
   - Include the deprecation deadline

4. **Developer Announcement (if applicable)**
   - Post to developer channels (Discord, Slack, forums)
   - Include migration guide link
   - Provide a support contact for questions

### During the Deprecation Period

- Monitor for questions and feedback in community channels
- Provide migration support to affected teams
- Update documentation and examples
- Offer a grace period for partners to upgrade (notify if extensions are needed)

---

## Versioning Strategy

Fluxapay contracts follow **Semantic Versioning** (SemVer):

```
MAJOR.MINOR.PATCH
```

### Versioning Rules

- **MAJOR version bump** (e.g., 1.0.0 → 2.0.0):
  - Required for any breaking change deployed to production
  - Indicates clients **must** upgrade
  - CHANGELOG breaking section summarizes all migration steps

- **MINOR version bump** (e.g., 1.2.0 → 1.3.0):
  - Used for new features that are backward-compatible
  - New functions, new optional fields, expanded functionality

- **PATCH version bump** (e.g., 1.2.3 → 1.2.4):
  - Used for bug fixes and security patches
  - No API changes

### Version Deployment

1. **Testnet releases** are published first for 14 days of community testing
2. **Production releases** require explicit version approval and change audit
3. Release notes must clearly note the version number and SemVer classification

---

## Examples

### Example 1: Function Removal (Breaking)

**Change**: Remove the `verify_payment()` function in favor of a new `check_payment_status()` function.

**Deprecation Process**:
1. Release v1.5.0 (current version)
   - Mark `verify_payment()` as deprecated in doc comments
   - CHANGELOG: "Deprecated: `verify_payment()` will be removed in v2.0"
2. Notify: Email, Discord announcement, CHANGELOG with 30-day countdown
3. Monitor feedback for 30 days
4. Release v2.0.0 (breaking)
   - Remove `verify_payment()` function
   - CHANGELOG includes migration: "Use `check_payment_status()` instead"
   - Release notes include code example

### Example 2: Security Bug Fix

**Change**: Fix a critical vulnerability in the authorization check that inadvertently allowed unauthorized payments.

**Action**:
- Publish security advisory immediately with 7-day notice (exception to 30-day rule)
- Detailed fix description in CHANGELOG under "Security"
- Bump version to v1.4.1 (patch) or higher depending on scope
- Release notes include workarounds for clients who cannot upgrade immediately

---

## Enforcement

- **Code review**: All PRs are reviewed for breaking changes; PRs are blocked if unannounced
- **CI checks**: Contract ABI is logged; ABI diffs trigger review alerts
- **Audit**: Monthly review of contract deployments against this policy
- **Community feedback**: Breaking changes require stakeholder sign-off on testnet

---

## Migration Guides

For each breaking change, provide:

1. **Migration Guide** (in docs/)
   - Problem: What broke
   - Solution: How to fix it
   - Code before/after examples
   - Testing steps

2. **Release Notes Code Examples**
   - Show old code and new code side-by-side
   - Link to detailed migration guide

3. **Slack/Email Template**
   - Automated notification to partner integrations
   - Include deadline and migration guide link

---

## Questions and Support

For questions about breaking changes or migration support:
- GitHub Issues: Use `[BREAKING]` label
- Email: support@fluxapay.io
- Discord: #contract-upgrades channel

---

## Document History

| Date | Version | Change |
|------|---------|--------|
| 2026-05-30 | 1.0 | Initial breaking change policy |

---

## Related Documents

- [CHANGELOG.md](../CHANGELOG.md) — Version history and breaking change announcements
- [DEPLOYMENT.md](../DEPLOYMENT.md) — Deployment procedures
- [docs/](../docs/) — All technical documentation
