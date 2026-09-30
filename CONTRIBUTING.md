# Contributing to FluxaPay

Thank you for your interest in contributing to FluxaPay!  
This document covers everything you need to get started: environment setup, build and test commands, code standards, branch and commit conventions, and the PR process.

For security vulnerabilities, see [SECURITY.md](SECURITY.md) instead of opening a public issue.

---

## Table of Contents

1. [Local Development Setup](#1-local-development-setup)
2. [Building the Contract](#2-building-the-contract)
3. [Running Tests](#3-running-tests)
4. [Linting, Formatting, and Auditing](#4-linting-formatting-and-auditing)
5. [Branch Naming Conventions](#5-branch-naming-conventions)
6. [Commit Message Format](#6-commit-message-format)
7. [Pull Request Requirements](#7-pull-request-requirements)
   - [Adding Contract Events](#72-adding-contract-events)
8. [Issue Workflow](#8-issue-workflow)

---

## 1. Local Development Setup

### Required Tools

| Tool | Version | Install |
|---|---|---|
| Rust (pinned) | 1.85.0 | Automatically installed via `rust-toolchain.toml` |
| wasm32 target | — | Automatically installed via `rust-toolchain.toml` |
| Stellar CLI | 21.x | [stellar.org/docs](https://developers.stellar.org/docs/tools/developer-tools/stellar-cli) |
| cargo-audit | latest | `cargo install cargo-audit` |
| cargo-deny | latest | `cargo install cargo-deny` |

### Environment Variables

Copy the example and populate with your testnet credentials:

```bash
cp .env.example .env
# Edit .env — do NOT commit this file
```

Run the environment check script to verify that all required keys from `.env.example` are present in your local `.env`:

```bash
bash scripts/check-env.sh
```

The script prints a non-fatal warning for each key that is missing from `.env`.

See [scripts/README.md](scripts/README.md) for details on all operational scripts and their required environment variables.

See [docs/local-invoke.md](docs/local-invoke.md) for step-by-step recipes to invoke contract functions on Stellar testnet.

To deploy the contracts to testnet, run `bash scripts/deploy-testnet.sh` from the repository root after setting `STELLAR_SECRET_KEY` and `STELLAR_NETWORK`.

---

## 2. Building the Contract

```bash
cd fluxapay
stellar contract build
```

Or via the Makefile shortcut:

```bash
cd fluxapay && make build
```

---

## 3. Running Tests

### Unit and integration tests

```bash
cd fluxapay && cargo test --all-features
```

### Property-based tests (bounded)

```bash
PROPTEST_CASES=64 cargo test -p fluxapay proptests:: --all-features -- --test-threads=1
```

### All tests via Makefile

```bash
cd fluxapay && make test
```

For testnet testing, see [docs/local-invoke.md](docs/local-invoke.md).

---

## 4. Linting, Formatting, and Auditing

Run all of these before opening a PR:

```bash
# Format check
cd fluxapay && cargo fmt --check

# Lint (warnings are errors)
cargo clippy --all-targets --all-features - -D warnings

# Security audit
cargo audit --deny warnings

# Dependency checks (bans, licenses, advisories)
cargo deny check bans licenses advisories
```

Or via Makefile:

```bash
cd fluxapay && make fmt && cargo clippy --all-targets --all-features
```

CI runs `cargo deny check bans licenses advisories` automatically in the `Security Scan` job (see `.github/workflows/ci.yml`), gated by `deny.toml`, and it must pass before the build job runs.

### Linting GitHub Actions Workflows (actionlint)

We use [actionlint](https://github.com/rhysd/actionlint) to statically check all `.github/workflows/*.yml` files.
CI runs it automatically via the `rhysd/actionlint@v1` action — **do not commit the binary to the repo**.

To run it locally, install the tool for your platform and execute it from the repo root:

```bash
# macOS (Homebrew)
brew install actionlint

# Linux (go install)
go install github.com/rhysd/actionlint/cmd/actionlint@latest

# Run against all workflows
actionlint
```

### WASM Size Regression Baseline (Issue #813)

CI compares the built/optimized contract WASM against `.wasm-size-baseline`
and **fails if the binary grows by more than 5%**.

The baseline file contains a single integer: the size in **bytes** of the
primary `fluxapay` WASM after `stellar contract optimize`.

When a change legitimately increases WASM size (new feature, unavoidable
growth), update the baseline in a **dedicated commit** with an explanation:

```bash
# After building + optimizing:
SIZE=$(stat -c%s target/wasm32-unknown-unknown/release/fluxapay*[.wasm | head -1)
# or on macOS:
# SIZE=$(stat -f%z target/wasm32-unknown-unknown/release/fluxapay.wasm)

echo "$SIZE" > .wasm-size-baseline
git add .wasm-size-baseline
git commit -m "chore(ci): bump WASM size baseline to ${SIZE} after <reason>"
```

Do not silently raise the baseline in an unrelated feature commit.

## 4a. Pre-commit Hooks

Use [lefthook](https://github.com/evilmartians/lefthook) to run `cargo fmt`, `cargo clippy`, and TypeScript type checks before each commit.

### Installation

```bash
# Install lefthook (macOS / Linux)
brew install lefthook
# or via Go:
go install github.com/evilmartians/lefthook@latest

# Install hooks for this repo
lefthook install
```

### What the hooks do

- **Pre-commit** (Rust files): runs `cargo fmt --check` and `cargo clippy --all-targets - -D warnings` on staged `.rs` files
- **Pre-commit** (TypeScript files): runs `npx tsc --noEmit` in `sdk/` on staged `.ts` files

### Skipping hooks

For emergency commits, you can bypass all hooks:

```bash
SKIP_HOOKS=1 git commit -m "emergency fix"
```

## 5. Branch Naming Conventions

| Prefix | Use case |
|---|---|
| `feat/` | New feature or capability |
| `fix/` | Bug fix |
| `chore/` | Build, CI, dependency, or tooling changes |
| `docs/` | Documentation-only changes |
| `security/` | Security patches or hardening |

**Examples:**

```
feat/stream-rate-decrease
fix/payment-id-validation
docs/contributing-guide
security/audit-remediation
```

---

## 6. Commit Message Format

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <short summary>

[optional body]

[optional footer: Closes #<issue>]
```

**Types:** `feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `security`

**Scope:** the contract or module affected (e.g. `payment-processor`, `stream`, `access-control`, `refund-manager`)

**Examples:**

```
feat(stream): implement decrease_rate_per_second with checkpoint and surplus refund
fix(payment-processor): add payment_id format validation in create_payment
docs: add CONTRIBUTING.md with setup, standards, and PR process
feat(access-control): expose get_role_members and has_role on public contract ABI
```

---

## 7. Pull Request Requirements

Before marking a PR ready for review:

- [ ] All tests pass (`make test`)
- [ ] No new Clippy warnings (`cargo clippy --all-targets --all-features - -D warnings`)
- [ ] Automated CI security checks (`cargo-deny` and `cargo-audit`) pass (enforced via branch protection rules targeting `main`)
- [ ] `CHANGELOG.md` updated under `## Unreleased` (or PR has the `skip-changelog` label for non-user-facing changes)
- [ ] New features and bug fixes include tests
- [ ] PR title follows Conventional Commits format
- [ ] PR description explains *what* changed, *why*, and how it was tested

### Changelog Format

Every user-facing PR **must** update [`CHANGELOG.md`](CHANGELOG.md) under the
`## Unreleased` section, **or** carry the `skip-changelog` label (CI/CD, docs-only,
or internal refactors with no user-facing impact).

The [changelog-check](.github/workflows/changelog-check.yml) workflow:

1. Fails the PR if `CHANGELOG.md` was not touched and the PR lacks `skip-changelog`.
2. Allows a full bypass when the PR is labelled `skip-changelog`.
3. When `CHANGELOG.md` updated, requires the Unreleased section to contain at
last one bullet entry that references this PR number (e.g. `PR #123`).

Follow [Keep a Changelog](https://keepachangelog.com/) categories:

```markdown
## Unreleased

### Added
- **Issue #831 / PR #123**: Multi-payee payment streams with proportional withdraw.

### Fixed
- **PR #123**: `payment_id` format validation now enforces 3–64 alphanumeric/-/_ characters.
```

Use the `skip-changelog` label only for CI/CD, docs, or internal refactors with no user-facing impact.

After opening your PR, add the PR number to the Unreleased bullet (CI will fail
until the entry references `#<pr-number>`).

---

## 7.1. Dependabot PRs

This repository uses [Dependabot](https://docs.github.com/en/code-security/dependabot) to automatically create PRs for dependency updates:

- **Cargo dependencies** (Rust crates)
- **GitHub Actions** (CI workflow actions)
- **npm dependencies** (SDK packages)

### Handling Dependabot PRs

1. **Review the changes**: Dependabot PRs will have labels indicating the package ecosystem (`rust`, `github-actions`, `npm`)
2. **Verify CI passes**: All dependency update PRs must pass the full CI pipeline
3. **Check for breaking changes**: Review the changelog links in the PR description
4. **Merge when ready**: Once CI passes and the update looks safe, merge the PR
5. **For major version bumps**: Additional manual testing may be required before merging

> **Note**: Dependabot runs weekly. If you need an urgent security update, you can manually trigger it via the [Dependabot dashboard](https://github.com/Yunusabdul38/fluxapay_contract/security/dependabot).

---

## 7.2. Adding Contract Events

FluxaPay contract events are defined with `#[contractevent]`. Follow these steps when adding a new one:

1. **Define the event struct** in [`fluxapay/src/events.rs`](fluxapay/src/events.rs):

   ```rust
   #[contractevent]
   #[derive(Clone, Debug)]
   pub struct MerchantSuspended {
       pub merchant_id: Address,
       pub reason: Symbol,
   }
   ```

2. **Emit it in the correct entry point** — call `.publish()` on the event from the contract function that triggers it:

   ```rust
   MerchantSuspended {
       merchant_id: merchant_id.clone(),
       reason: Symbol::new(&env, "compliance_hold"),
   }
   .publish(&env);
   ```

3. **Update the event catalog** in [`docs/events.md`](docs/events.md) — add a section documenting the event's fields, emitting function(s), and an example payload.

4. **Add an indexer subscription** in [`indexer/sync.yml`](indexer/sync.yml) under the relevant contract's `events` list so the indexer picks up the new event.

5. **Add the SDK event type** in `sdk/src` so consumers of the TypeScript SDK get typed access to the new event.

**Worked example — adding `MerchantSuspended`:**
/- Struct added to `fluxapay/src/events.rs` under a `// Merchant Registry Events` section
/- Emitted from `merchant_registry::suspend_merchant`
/- Documented in `docs/events.md` under `## MERCHANT / SUSPENDED`
/- Subscribed to in `indexer/sync.yml` under the `merchant_registry` contract mapping
/- Typed in the SDK alongside the other merchant registry events

### Adding SDK Error Locales (Issue #852)

Contributors can add support for new languages to the SDK error system:
1. Create a new JSON file in `sdk/src/locales/<locale>.json` (e.g., `de.json`, `sw.json`).
2. Map each contract error code number to its translated, user-friendly message (refer to `sdk/src/locales/en.json` for the complete list of error codes).
3. Register the new locale in `sdk/src/locales/index.ts` under `SUPPORTED_LOCALES` and the `MESSAGES` map.
4. Run `npm test` in `sdk/` to ensure all tests pass.

---

## 8. Issue Workflow

### Labels

| Label | Meaning |
|---|---|
| `bug` | Something is broken |
| `feat` | New feature request |
| `docs` | Documentation improvement |
| `security` | Security-related issue |
| `chore` | Maintenance, CI, or tooling |
| `skip-changelog` | PR exempt from changelog requirement |
| `breaking-change` | Requires a deprecation notice per [BREAKING_CHANGES.md](docs/BREAKING_CHANGES.md) |

### Reporting Bugs

Open an issue using the **Bug Report** template. Include:
- Contract function name and call arguments
- Expected vs. actual behaviour
- Network (testnet / devnet) and contract ID if applicable

### Feature Requests

Open an issue using the **Feature Request** template. Describe the use case before proposing a solution.

### Security Vulnerabilities

Do 
