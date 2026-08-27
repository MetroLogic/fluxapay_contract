# Changelog

All notable changes to `@fluxapay/sdk` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial npm publish workflow (`sdk/v*` tags) and scoped package configuration.
- [SDK Migration Guide](../docs/sdk-migration-guide.md) documenting breaking
  changes and upgrade steps between major SDK versions.

## [0.1.0] - 2026-06-25

### Added
- `FluxapayClient` high-level wrapper for payment, refund, and merchant contract calls.
- `RefundManagerClient` and `MerchantRegistryClient` for dedicated contract interactions.
- Typed contract models (`Merchant`, `PaymentCharge`, `Refund`, `Dispute`, etc.).
- Network profile presets for testnet and mainnet.
- Offline/hardware wallet signing helpers via `FluxapayOfflineSigner`.
- Contract error mapping with `FluxapayError` and `FLUXAPAY_CONTRACT_ERROR_MAP`.

[Unreleased]: https://github.com/MetroLogic/fluxapay_contract/compare/sdk/v0.1.0...HEAD
[0.1.0]: https://github.com/MetroLogic/fluxapay_contract/releases/tag/sdk/v0.1.0
