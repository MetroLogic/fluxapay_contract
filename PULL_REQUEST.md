# PR: Add Scoped Pausing, Dispute Evidence, Subscription Invoices, and SEP-10 Auth

## Description

Implements four related merchant and contract features in one PR:

- **Granular contract pauses (#1055):** Adds operation-level pause scopes for payment creation, verification, settlement, refunds, and swaps, plus the global scope. Existing pause entry points remain compatible, and callers can query active scopes.
- **Dispute evidence uploads (#1051):** Adds party-authorized evidence upload and retrieval, IPFS pinning, server-side SHA-256 calculation, and on-chain hash anchoring.
- **Subscription invoices (#1050):** Generates cycle-linked invoices from successful subscription charge events and provides invoice lookup and PDF access.
- **SEP-10 wallet authentication (#1048):** Adds challenge-response sign-in, single-use challenge validation, and bearer JWT authentication for protected API routes.

## Issues Closed

Closes #1055
Closes #1051
Closes #1050
Closes #1048

## Verification

Tests and builds were not run for this change.
