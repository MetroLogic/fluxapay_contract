# Changelog

All notable changes to the FluxaPay TypeScript SDK will be documented in this file.

## [Unreleased]

### Added

- Automatic retry with exponential backoff for `429 Too Many Requests` and `5xx` server errors. All SDK HTTP calls now go through `fetchWithRetry` by default.
- `Retry-After` header is respected on `429` responses when present.
- Default of 3 retries with backoff delays of 1s, 2s, and 4s.
- `maxRetries` option on the `FluxaPayClient` constructor to configure retry behavior.
- Retry attempts are logged at debug level with the attempt number and delay.

### Notes

- Non-retriable errors (`400`, `401`, `403`, `404`, `422`) are returned immediately without retrying.
