# Mainnet Deployment Checklist

This checklist covers every step required to deploy FluxaPay to Stellar mainnet. Each step includes the responsible role.

## Pre-Deployment

- [ ] **External audit completed and all findings resolved** — Security
- [ ] **SECURITY.md updated with audit report and status** — Security
- [ ] **All CI tests passing on `main`** — Dev
- [ ] **Testnet smoke test passing end-to-end** — Dev / QA
- [ ] **CHANGELOG.md updated with release notes** — Dev

## Contract Configuration

- [ ] **USDC token address configured** (Stellar mainnet USDC issuer) — Admin
- [ ] **Multi-sig threshold configured** (minimum 2-of-3 for admin operations) — Admin / Security
- [ ] **KYC tier limits set for production values** — Admin
- [ ] **Admin key stored in hardware wallet or HSM** — Security

## Infrastructure

- [ ] **FX oracle deployed on mainnet and oracle role granted** — DevOps
- [ ] **Subscription daemon configured and running** — DevOps
- [ ] **FX oracle updater script running** — DevOps
- [ ] **Monitoring and alerting configured** (contract errors, rate staleness, treasury balance) — DevOps

## Contract IDs

- [ ] **Contract IDs populated in `sdk/src/network-profiles.ts`** — Dev
- [ ] **Contract IDs updated in `DEPLOYMENT.md`** — Dev
- [ ] **`scripts/check-mainnet-contract-ids.js` CI check passes** — CI
- [ ] **SDK version published with mainnet contract IDs** — Dev

## Deployment Execution

- [ ] **Deployment approved by 2 team members** (GitHub Environment Protection) — Team
- [ ] **Run `deploy-testnet.sh` with `STELLAR_NETWORK=mainnet`** — DevOps
- [ ] **Verify contract deployment** (`stellar contract info interface`) — DevOps
- [ ] **Verify admin configuration** (`get_admin` on each contract) — Admin
- [ ] **Verify merchant registry state** — DevOps
- [ ] **Run health check simulation** — DevOps

## Post-Deployment

- [ ] **Monitor initial transactions** on Stellar Expert — DevOps
- [ ] **Verify webhook delivery** for first real payment — Dev
- [ ] **Confirm FX rate updates are flowing** — DevOps
- [ ] **Confirm subscription daemon is polling successfully** — DevOps
- [ ] **Rollback plan documented and tested** — DevOps / Security

## Emergency Contacts

- **Security Team**: security@fluxapay.com
- **Operations Lead**: operations@fluxapay.com
