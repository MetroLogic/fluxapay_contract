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
   - [`lib.rs` Module Boundaries](#73-librs-module-boundaries-issue-1222)
   - [Entrypoint Reference](#74-entrypoint-reference)
   - [Storage Key Layout Reference](#75-storage-key-layout-reference)
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
| cargo-deny | 0.20.2 | `cargo install --locked cargo-deny --version 0.20.2` |

### Environment Variables

Copy the example and populate with your testnet credentials:

```bash
cp .env.example .env
# Edit .env — do NOT commit this file
```

To verify that your `.env` defines every key listed in `.env.example`, run:

```bash
# from the repository root
bsh scripts/check-env.sh
```

The script is non-fatal: for each key missing from `.env` it prints a
warning to stderr and exits 0. Run it after pulling changes that touch
`.env.example` to catch newly required variables.

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

### Nightly full test run (Issue #1219)

`.github/workflows/nightly.yml` runs every day at 02:00 UTC (and on demand
via **Actions → Nightly full test run → Run workflow**). It runs the whole
suite in a way PR CI does not:

| Step | What it catches |
|------|-----------------|
| `cargo test --workspace --all-features --all-targets` | breakage in any contract feature, unit or integration target |
| `cargo test --workspace --all-targets` | code that only compiles or passes with extra features on |
| same as the first step with `--test-threads=1` | order- and interleaving-dependent tests |

`PROPTEST_CASES=4096` is set for the whole job, so property tests get
16x the PR budget.

When a scheduled run fails, the `notify-failure` job opens an issue titled
**Nightly full test run failing**, labelled `nightly-failure`, with a link
to the run. Later failures comment on that same issue while it stays open.
Close it once the fix lands.

To reproduce a nightly failure locally:

```bash
PROPTEST_CASES=4096 cargo test --workspace --all-features --all-targets -- --test-threads=1
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

CI runs `cargo deny check bans licenses advisories --warn unsound --warn unmaintained` automatically in the `Security Scan` job (see `.github/workflows/ci.yml`), gated by `deny.toml`; the final CI success check requires it to pass. Unsound and unmaintained advisories remain visible as warnings, matching the repository's existing non-blocking policy; vulnerabilities remain errors.

The duplicate-version check excludes dev-dependency edges
(`multiple-versions-include-dev = false` in `deny.toml`), so test-only crates do
not add noise to production dependency version checks. Explicit crate bans still
apply to the dependency graph cargo-deny evaluates. If a production dependency
must be exempted from a future ban or duplicate-version rule, add a narrowly
scoped entry to `[bans].skip` with the exact crate name and version plus a
`reason` describing why it is necessary and how it is limited. Do not exempt a
crate merely because it appears in tests if it is also reachable through a
production dependency. Review the dependency tree (`cargo tree -e normal`) and
remove obsolete exceptions when upgrading dependencies.

### Dependency license policy

Every crate in the production dependency graph must use a license on the
allowlist in `deny.toml` (`[licenses].allow`). The check runs as part of
`cargo deny check bans licenses advisories` in both the `Security Scan` and
`Cargo Deny Check` CI jobs, so a pull request that adds a dependency with a
disallowed license fails CI with a `rejected` license error naming the crate.

Currently allowed (all permissive and compatible with this repository's MIT
license):

| License | Typical source |
|---|---|
| `MIT`, `Apache-2.0`, `Apache-2.0 WITH LLVM-exception` | most of the Rust ecosystem |
| `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib` | small utility and crypto crates |
| `OpenSSL` | `ring` and OpenSSL-derived code |
| `Unicode-3.0` | `unicode-ident` and Unicode data tables |

Copyleft licenses (`GPL-*`, `LGPL-*`, `AGPL-*`) and weak-copyleft licenses
(`MPL-2.0`, `EPL-*`) are not allowed by default. Dev-dependencies are not
checked (`include-dev = false`) because they never ship in the contract WASM.

Check a change locally before opening a PR:

```bash
# Licenses only
cargo deny check licenses

# Show which license each dependency resolved to
cargo deny list
```

If a dependency you need is rejected:

1. Look for an alternative crate under an allowed license first.
2. If there is none, add a per-crate entry to `[licenses].exceptions` in
   `deny.toml` (for example `{ crate = "some-crate", allow = ["MPL-2.0"] }`)
   rather than adding the license to the global `allow` list.
3. Explain in the PR description why the crate is needed and why its license
   is acceptable. A maintainer must approve any change to the allowlist or
   exceptions.

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
SIZE=$(stat -c%s target/wasm32-unknown-unknown/release/fluxapay*.wasm | head -1)
# or on macOS:
# SIZE=$(stat -f%z target/wasm32-unknown-unknown/release/fluxapay.wasm)

echo "$SIZE" > .wasm-size-baseline
git add .wasm-size-baseline
git commit -m "chore(ci): bump WASM size baseline to ${SIZE} after <reason>"
```

Do not silently raise the baseline in an unrelated feature commit.

## 4a. Pre-commit Hooks

We use [lefthook](https://github.com/evilmartians/lefthook) to run `cargo fmt`, `cargo clippy`, TypeScript type checks, `eslint`, and secret scanning before each commit.

### Installation

```bash
# Install lefthook (macOS / Linux) via Homebrew
brew install lefthook

# ...or via npm
npm install -g lefthook

# ...or via Go
go install github.com/evilmartians/lefthook@latest

# Install hooks for this repo
lefthook install
```

Secret scanning requires [`gitleaks`](https://github.com/gitleaks/gitleaks) on your `PATH`:

```bash
brew install gitleaks
# or
go install github.com/gitleaks/gitleaks/v8@latest
```

If `gitleaks` isn't installed, the hook prints a warning and skips the scan rather than blocking the commit -- CI still runs the full scan.

### What the hooks do

- **pre-commit** (Rust files): runs `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` on staged `.rs` files
- **pre-commit** (TypeScript files): runs `npx tsc --noEmit` in `sdk/` on staged `.ts` files
- **pre-commit** (TypeScript/TSX files in `sdk/`, `indexer/`, `backend/`, `dashboard/`): runs `npx eslint` on only the staged files
- **pre-commit** (all staged files): runs `gitleaks protect --staged` to block commits that introduce secrets (API keys, private keys, tokens)

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

```feat/stream-rate-decrease
fix/payment-id-validation
docs/contributing-guide
security/audit-remediation
```

---

## 6. Commit Message Format

We follow [Conventional Commits](https://www.conventionalcommits.org/):

````<type>(<scope>): <short summary>

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
- [ ] If your change affects the compiled contract (new dependency, new public function, large refactor), run `./scripts/update-wasm-baseline.sh` and commit the updated `.wasm-size-baseline` so the CI size gate compares against the right number
- [ ] If your change implements, removes, or changes the status of a feature listed in the README's ["Feature Status"](README.md#feature-status) matrix (Issue #1189) — e.g. moves the DEX router from "Partial (simulated)" to "Implemented" once real Soroswap routing lands (ADR-0005) — update that row in the same PR

### Changelog Format

Every user-facing PR **must** update [`CHANGELOG.md`](CHANGELOG.md) under the
`## Unreleased` section, **or** carry the `skip-changelog` label (CI/CD, docs-only,
or internal refactors with no user-facing impact).

The [changelog-check](.github/workflows/changelog-check.yml) workflow:

1. Fails the PR if `CHANGELOG.md` was not touched and the PR lacks `skip-changelog`.
2. Allows a full bypass when the PR is labelled `skip-changelog`.
3. When `CHANGELOG.md` is updated, requires the Unreleased section to contain at
   least one bullet entry that references this PR number (e.g. `PR #123`).

Follow [Keep a Changelog](https://keepachangelog.com/) categories:

```markdown
## Unreleased

### Added
- **Issue #831 / PR #123**: Multi-payee payment streams with proportional withdraw.

### Changed
- **Issue #1013 / PR #124**: `withdraw_treasury` now allows single-admin execution
  below `TreasurySingleAdminThreshold`; larger withdrawals still require multisig.

### Fixed
- **PR #123**: `payment_id` format validation now enforces 3–64 alphanumeric/-/_ characters.
```

A single PR commonly only needs one of these subsections — add `### Changed` /
`### Fixed` / etc. only when that category applies, in the order given by
[Keep a Changelog](https://keepachangelog.com/) (`Added`, `Changed`, `Deprecated`,
`Removed`, `Fixed`, `Security`).

Use the `skip-changelog` label only for CI/CD, docs, or internal refactors with no user-facing impact.

After opening your PR, add the PR number to the Unreleased bullet (CI will fail
until the entry references `#<pr-number>`).

---

## 7.1. Dependabot PRs

Whis repository uses [Dependabot](https://docs.github.com/en/code-security/dependabot) to automatically create PRs for dependency updates:

- **Cargo dependencies** (Rust crates)
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
- Struct added to `fluxapay/src/events.rs` under a `// Merchant Registry Events` section
- Emitted from `merchant_registry::suspend_merchant`
- Documented in `docs/events.md` under `## MERCHANT / SUSPENDED`
- Subscribed to in `indexer/sync.yml` under the `merchant_registry` contract mapping
- Typed in the SDK alongside the other merchant registry events

### Adding SDK Error Locales (Issue #852)

Contributors can add support for new languages to the SDK error system:
1. Create a new JSON file in `sdk/src/locales/<locale>.json` (e.g., `de.json`, `sw.json`).
2. Map each contract error code number to its translated, user-friendly message (refer to `sdk/src/locales/en.json` for the complete list of error codes).
3. Register the new locale in `sdk/src/locales/index.ts` under `SUPPORTED_LOCALES` and the `MESSAGES` map.
4. Run `npm test` in `sdk/` to ensure all tests pass.

---

## 7.3. `lib.rs` Module Boundaries (Issue #1222)

`fluxapay/src/lib.rs` is a thin wiring file: module declarations, re-exports, shared
cross-module `#[contracttype]`/`#[contracterror]` enums, and `#[contractimpl]` bodies that
**delegate** to submodule logic. Do not add business logic, storage helpers, or state machines
to `lib.rs` — put them in the relevant submodule (`payment_processor.rs`, `refund_manager.rs`,
`payment_state_machine.rs`, etc.).

This is enforced in CI by `scripts/check_librs_boundaries.sh` (see
[ADR-0006](docs/ADR-0006-module-boundaries-lib-rs.md)), which fails the build if `lib.rs` grows
beyond the frozen baseline in `.librs-baseline` (line count, with a small tolerance, or `pub fn`
count, strict). If your PR moves logic **out** of `lib.rs`, lower the numbers in
`.librs-baseline` accordingly; never raise them.

---

## 7.4. Entrypoint Reference

[`docs/entrypoint-reference.md`](docs/entrypoint-reference.md) is the single reference
listing every public contract entrypoint (every `pub fn` inside a `#[contractimpl] impl`
block) across all FluxaPay contracts, with its signature, authorization requirement,
emitted events, and the `Error` variants it can return.

**Any PR that adds, removes, or changes the signature, auth, or events of a `pub fn`
entrypoint must update the corresponding entry in `docs/entrypoint-reference.md` in the
same PR.** This keeps the reference trustworthy without requiring a separate audit pass —
reviewers should treat a missing update here the same as a missing CHANGELOG entry.

---

## 7.5. Storage Key Layout Reference

[`docs/storage-layout.md`](docs/storage-layout.md) lists every `DataKey` /
`MerchantDataKey` (and other contract-local key enum) variant, how its TTL is managed,
and the migration history of keys that have changed shape or location.

**Any PR that adds, removes, renames, or changes the stored type of a storage key must
update the corresponding table/section of `docs/storage-layout.md` in the same PR.** If
the change is breaking per [`docs/BREAKING_CHANGES.md`](docs/BREAKING_CHANGES.md) (see
"Storage Key Changes" there), also add a dated entry under `docs/storage-layout.md`'s own
"Migration notes" section, in addition to the required `CHANGELOG.md` entry.

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
