/**
 * Node.js entry point for `@fluxapay/sdk` (issue #834).
 *
 * Re-exports the full browser SDK surface and applies Node-friendly Stellar SDK
 * defaults so callers do not need to polyfill `fetch` or tune TLS manually on
 * Node.js 18+.
 *
 * Usage:
 * ```ts
 * import { FluxapayClient } from "@fluxapay/sdk/node";
 * ```
 */

import * as StellarSdk from "@stellar/stellar-sdk";

type AllowHttpConfig = { setAllowHttp?: (allow: boolean) => void };

// Allow plain-http RPC endpoints (common for local/dev Horizon/Soroban).
const sdkRoot = StellarSdk as unknown as {
  Config?: AllowHttpConfig;
  config?: AllowHttpConfig;
};
const stellarConfig = sdkRoot.Config ?? sdkRoot.config;
stellarConfig?.setAllowHttp?.(true);

// Node 18+ ships a global fetch. Ensure it is visible before the Stellar SDK
// HTTP layer is first used.
const g = globalThis as typeof globalThis & { fetch?: typeof fetch };
if (typeof g.fetch !== "function") {
  throw new Error(
    "@fluxapay/sdk/node requires Node.js 18+ (global fetch). Upgrade Node or polyfill fetch before importing.",
  );
}

export * from "./index.js";
