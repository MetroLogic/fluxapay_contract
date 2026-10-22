To address the task, I will provide the exact code fixes and steps to complete the requirements.

### Step 1: Create `SECURITY.md`

Create a new file named `SECURITY.md` in the repository root with the following content:
```markdown
# Security Policy

## Vulnerability Disclosure

If you believe you've found a security vulnerability in FluxaPay contracts, please email us at [security@fluxapay.com](mailto:security@fluxapay.com). We'll respond within 3 business days.

## Scope

The following are in scope for vulnerability disclosure:

* Access control mechanisms
* Smart contract logic
* Data storage and handling

The following are out of scope:

* Third-party dependencies
* Non-security related issues

## Bug Bounty

We offer a bug bounty program for security vulnerabilities. Please see our [bug bounty page](https://fluxapay.com/bug-bounty) for more information.

## Audit Status

Our contracts have not been audited yet. We are currently in the process of scheduling an audit with a reputable firm.

## Response SLA

We will respond to all security-related emails within 3 business days.
```

### Step 2: Document Current Audit Status

Update the `SECURITY.md` file to reflect the current audit status:
```markdown
## Audit Status

Our contracts have not been audited yet. We are currently in the process of scheduling an audit with a reputable firm.
```
Once the audit is complete, update this section to reflect the audit status, including the date and auditor:
```markdown
## Audit Status

Our contracts were audited by [Auditor Name] on [Date]. The audit report is available [here](link to audit report).
```

### Step 3: Link to Security Policy from `README.md`

Add a link to the `SECURITY.md` file from the `README.md` file:
```markdown
## Security

See our [security policy](SECURITY.md) for information on vulnerability disclosure and audit status.
```

### Step 4: Add `CODEOWNERS` File

Create a new file named `CODEOWNERS` in the repository root with the following content:
```markdown
# Assign security-sensitive files to a security reviewer
access_control.rs @fluxapay-security
lib.rs @fluxapay-security
```
Replace `@fluxapay-security` with the actual GitHub username or team name of the security reviewer.

Commit these changes and push them to the repository to complete the task.