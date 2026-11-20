/**
 * Issue #1153: MerchantRegistryClient
 *
 * Resolves a Stellar signing account (G… public key) to its registered
 * FluxaPay merchant id.  The registry is queried over HTTP; results are
 * cached for `cacheTtlMs` to avoid a round-trip on every auth token request.
 *
 * When no registry URL is configured the client falls back to returning
 * `undefined`, which causes the auth router to use the account's public key
 * as the merchant id — the same behaviour as before this change.
 */

export interface MerchantRegistryClientConfig {
  /** Base URL of the merchant registry service, e.g. http://localhost:4000 */
  registryUrl: string;
  /** Bearer token used to authenticate with the registry service. */
  authToken?: string;
  /** How long (ms) to cache a successful lookup. Defaults to 60 000. */
  cacheTtlMs?: number;
  /** How long (ms) to cache a negative (not-found) lookup. Defaults to 5 000. */
  negativeCacheTtlMs?: number;
  /** Request timeout in ms. Defaults to 3 000. */
  timeoutMs?: number;
}

interface CacheEntry {
  merchantId: string | undefined;
  expiresAt: number;
}

export class MerchantRegistryClient {
  private readonly registryUrl: string;
  private readonly authToken?: string;
  private readonly cacheTtlMs: number;
  private readonly negativeCacheTtlMs: number;
  private readonly timeoutMs: number;
  private readonly cache = new Map<string, CacheEntry>();

  constructor(config: MerchantRegistryClientConfig) {
    this.registryUrl = config.registryUrl.replace(/\/$/, "");
    this.authToken = config.authToken;
    this.cacheTtlMs = config.cacheTtlMs ?? 60_000;
    this.negativeCacheTtlMs = config.negativeCacheTtlMs ?? 5_000;
    this.timeoutMs = config.timeoutMs ?? 3_000;
  }

  /**
   * Look up the merchant id for a Stellar account.
   *
   * Returns `undefined` when:
   *  - the account is not registered, or
   *  - the registry is unreachable (fail-open: auth flow continues with the
   *    account public key as a fallback merchant id).
   */
  async resolveMerchantId(account: string): Promise<string | undefined> {
    const now = Date.now();
    const cached = this.cache.get(account);
    if (cached && cached.expiresAt > now) {
      return cached.merchantId;
    }

    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);

    try {
      const headers: Record<string, string> = { Accept: "application/json" };
      if (this.authToken) {
        headers["Authorization"] = `Bearer ${this.authToken}`;
      }

      const response = await fetch(
        `${this.registryUrl}/v1/merchants/by-account/${encodeURIComponent(account)}`,
        { headers, signal: controller.signal },
      );

      if (response.status === 404) {
        // Account exists on Stellar but is not registered — cache the miss.
        this.cache.set(account, {
          merchantId: undefined,
          expiresAt: now + this.negativeCacheTtlMs,
        });
        return undefined;
      }

      if (!response.ok) {
        // Unexpected error — do not cache, fail open.
        console.warn(
          `[merchant-registry] unexpected status ${response.status} for account ${account}`,
        );
        return undefined;
      }

      const body = (await response.json()) as { merchant_id?: string; id?: string };
      const merchantId = body.merchant_id ?? body.id;

      this.cache.set(account, {
        merchantId,
        expiresAt: now + this.cacheTtlMs,
      });

      return merchantId;
    } catch (err) {
      if (err instanceof Error && err.name === "AbortError") {
        console.warn(
          `[merchant-registry] lookup timed out for account ${account} (${this.timeoutMs}ms)`,
        );
      } else {
        console.warn(
          `[merchant-registry] lookup failed for account ${account}:`,
          err instanceof Error ? err.message : String(err),
        );
      }
      // Fail open: let the auth flow continue with the public key as the id.
      return undefined;
    } finally {
      clearTimeout(timer);
    }
  }

  /** Evict a single entry (useful in tests or after a merchant registers). */
  invalidate(account: string): void {
    this.cache.delete(account);
  }

  /** Clear the entire in-memory cache. */
  clearCache(): void {
    this.cache.clear();
  }
}

/**
 * Build a `resolveMerchantId` callback suitable for `AuthRouterConfig`.
 *
 * When `MERCHANT_REGISTRY_URL` is not configured the callback returns
 * `undefined` immediately (falls back to the account public key).
 *
 * Documented fallback behaviour:
 *  - Registry unreachable: returns `undefined` → JWT `merchant_id` = account pubkey
 *  - Account not registered: returns `undefined` → JWT `merchant_id` = account pubkey
 *  - Successful lookup: returns the registered merchant id string
 */
export function buildMerchantIdResolver(env: NodeJS.ProcessEnv = process.env): {
  resolveMerchantId: (account: string) => Promise<string | undefined>;
  client: MerchantRegistryClient | null;
} {
  const registryUrl = env.MERCHANT_REGISTRY_URL;

  if (!registryUrl) {
    // No registry configured — always fall back to the account key.
    return {
      resolveMerchantId: async () => undefined,
      client: null,
    };
  }

  const client = new MerchantRegistryClient({
    registryUrl,
    authToken: env.MERCHANT_REGISTRY_TOKEN,
    cacheTtlMs: env.MERCHANT_REGISTRY_CACHE_TTL_MS
      ? Number(env.MERCHANT_REGISTRY_CACHE_TTL_MS)
      : undefined,
    timeoutMs: env.MERCHANT_REGISTRY_TIMEOUT_MS
      ? Number(env.MERCHANT_REGISTRY_TIMEOUT_MS)
      : undefined,
  });

  return {
    resolveMerchantId: (account: string) => client.resolveMerchantId(account),
    client,
  };
}
