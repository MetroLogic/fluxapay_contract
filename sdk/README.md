# FluxaPay SDK

TypeScript SDK for interacting with FluxaPay Soroban contracts on Stellar.

## Installation

```bash
npm install @fluxapay/sdk
```

## Quick Start

```typescript
import { FluxaPayClient } from '@fluxapay/sdk';

const client = new FluxaPayClient({
  rpcUrl: 'https://soroban-testnet.stellar.org',
  networkPassphrase: 'Test SDF Network ; September 2015',
  contractId: 'C...',
});

const payment = await client.payments.createPayment({
  amount: '100.00',
  asset: 'USDC',
  destination: 'G...',
});
```

## Pre-flight Simulation

All write operations run a Soroban `simulateTransaction` pre-flight check before
submission. This predicts failures (insufficient balance, invalid state,
authorization errors) before a base transaction fee is consumed, and surfaces
them as typed `FluxaPayError` instances with human-readable messages.

```typescript
try {
  const payment = await client.payments.createPayment({
    amount: '100.00',
    asset: 'USDC',
    destination: 'G...',
  });
} catch (err) {
  if (err instanceof FluxaPayError) {
    console.error(err.code, err.message);
  }
}
```

### Dry run (simulation only)

Pass `simulate: true` to any write method to run the pre-flight simulation
without submitting the transaction. The method returns the simulation result
instead of a submitted transaction, which is useful for validating inputs and
estimating resource usage before committing fees.

```typescript
const sim = await client.payments.createPayment({
  amount: '100.00',
  asset: 'USDC',
  destination: 'G...',
  simulate: true,
});

console.log(sim.success); // true when the transaction would succeed
console.log(sim.gasEstimate); // estimated resource cost
```

### Skipping simulation

Set the `SKIP_SIMULATION=true` environment variable to bypass pre-flight
simulation entirely. This is intended for testing only.

```bash
SKIP_SIMULATION=true npm test
```

When simulation runs, the gas estimate is logged at debug level.

## Error Handling

Simulation failures are parsed into `FluxaPayError` instances. Each error
exposes a `code` and a human-readable `message` describing the contract
failure.

## License

MIT
