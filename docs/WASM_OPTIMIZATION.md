# WASM Size Optimization Guide

This document provides strategies and procedures for managing the WASM contract size in the FluxaPay project. The CI enforces a `MAX_WASM_SIZE_KB=100` limit on the compiled WASM artifact.

## Current Status

| Metric | Value |
|--------|-------|
| **WASM Size Limit** | 100 KB |
| **Optimization Tool** | `stellar contract optimize` (applies `wasm-opt -O3`) |
| **Current Optimized Size** | _Run `make optimize` to check_ |

## Building and Optimizing

### Quick Build & Optimize

```bash
cd fluxapay && make optimize
```

This command:
1. Builds the contract with `stellar contract build`
2. Runs `stellar contract optimize` (applies wasm-opt -O3)
3. Checks the optimized size against the 100KB limit

### Manual Optimization

```bash
# Build
stellar contract build

# Optimize
stellar contract optimize --wasm target/wasm32-unknown-unknown/release/fluxapay.wasm

# Check size
ls -lh target/wasm32-unknown-unknown/release/*.wasm
```

## Strategies for Reducing WASM Size

When the WASM size approaches or exceeds the limit, consider these strategies in order of impact:

### 1. Remove Dead Code with Feature Flags

Use `#[cfg(feature = "...")]` to conditionally compile code:

```rust
// Only include debug/test utilities in non-WASM builds
#[cfg(not(target_arch = "wasm32"))]
mod debug_utils { ... }

// Optional features for specific functionality
#[cfg(feature = "arbitrage-detection")]
mod arbitrage_detector { ... }
```

In `Cargo.toml`:
```toml
[features]
default = []
arbitrage-detection = []
advanced-streaming = []
```

### 2. Optimize Dependencies

- **Audit dependencies**: Remove unused dependencies from `Cargo.toml`
- **Use `default-features = false`**: Many crates include features you don't need
- **Prefer smaller alternatives**: Consider lighter-weight crates for common functionality

Example:
```toml
[dependencies]
# Instead of full soroban-sdk with all features
soroban-sdk = { version = "22", default-features = false }

# Only include what you need
hex = { version = "0.4", default-features = false, features = ["alloc"] }
```

### 3. Split into Multiple Contracts

If a single contract becomes too large, consider splitting functionality:

- **PaymentProcessor**: Core payment processing
- **RefundManager**: Refund and dispute management (already separate)
- **SubscriptionManager**: Subscription billing (could be extracted)
- **StreamManager**: Payment streaming (could be extracted)

Benefits:
- Each contract stays under the size limit
- Easier to audit and maintain
- Can upgrade components independently

### 4. Code-Level Optimizations

- **Reduce generic monomorphization**: Excessive use of generics can bloat binary size
- **Inline strategically**: `#[inline]` can help or hurt - measure impact
- **Use `no_std`**: Already in use, but ensure all dependencies are `no_std` compatible
- **Minimize string literals**: Use `Symbol` instead of `String` where possible

### 5. Analyze Binary Size

Use these tools to understand what's contributing to size:

```bash
# Install cargo-bloat for size analysis
cargo install cargo-bloat

# Analyze the WASM binary
cargo bloat --release --target wasm32-unknown-unknown --crates

# Detailed crate-level breakdown
cargo bloat --release --target wasm32-unknown-unknown -n 50 --crates
```

## CI Integration

The CI pipeline automatically:
1. Builds the contract
2. Runs `stellar contract optimize` on the WASM
3. Checks the optimized size against the 100KB limit
4. Fails the build if the limit is exceeded

### Local Pre-commit Check

Add this to your pre-commit workflow:

```bash
#!/bin/bash
# .git/hooks/pre-commit

cd fluxapay
stellar contract build
stellar contract optimize --wasm target/wasm32-unknown-unknown/release/fluxapay.wasm

MAX_SIZE=$((100 * 1024))
SIZE=$(stat -c%s target/wasm32-unknown-unknown/release/fluxapay.wasm)

if [ "$SIZE" -gt "$MAX_SIZE" ]; then
    echo "❌ WASM size ($SIZE bytes) exceeds limit ($MAX_SIZE bytes)"
    exit 1
fi
```

## When the Limit is Breached

If you've hit the 100KB limit:

1. **Don't panic** - this is expected as features grow
2. **Run optimization analysis**: `cargo bloat --release --target wasm32-unknown-unknown --crates`
3. **Apply strategies above** in order of impact
4. **Consider if the limit needs adjustment**: The 100KB limit is conservative; discuss with the team if adjustment is needed
5. **Document the decision**: If increasing the limit, update this document and CI configuration

## References

- [Soroban WASM Size Best Practices](https://developers.stellar.org/docs/build/smart-contracts/getting-started/deploy-to-testnet)
- [wasm-opt Documentation](https://github.com/WebAssembly/binaryen/wiki/wasm-opt)
- [cargo-bloat](https://github.com/nicholasbishop/cargo-bloat)