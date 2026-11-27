/**
 * Issue #839: In-memory cache of oracle FX rates, updated from indexed
 * FX_ORACLE / ORACLE events. The convert preview endpoint reads from this
 * cache (no live on-chain call).
 */

export interface CachedFxRate {
  /** Human-readable rate (quote per 1 base), e.g. 1520 for USDC→NGN. */
  rate: number;
  /** Unix seconds when the rate was last observed / updated. */
  updatedAt: number;
  /** Pair key as stored, e.g. "USDC_NGN". */
  pair: string;
}

const rates = new Map<string, CachedFxRate>();

export function pairKey(from: string, to: string): string {
  return `${from.toUpperCase()}_${to.toUpperCase()}`;
}

export function setCachedRate(pair: string, rate: number, updatedAt = Math.floor(Date.now() / 1000)): void {
  rates.set(pair.toUpperCase(), { pair: pair.toUpperCase(), rate, updatedAt });
}

export function getCachedRate(from: string, to: string): CachedFxRate | null {
  const direct = rates.get(pairKey(from, to));
  if (direct) return direct;

  // Fall back to inverse pair if present.
  const inverse = rates.get(pairKey(to, from));
  if (inverse && inverse.rate !== 0) {
    return {
      pair: pairKey(from, to),
      rate: 1 / inverse.rate,
      updatedAt: inverse.updatedAt,
    };
  }
  return null;
}

/** Issue #957: snapshot of every cached pair, for the bulk rates endpoint. */
export function listCachedRates(): CachedFxRate[] {
  return Array.from(rates.values());
}

export function clearCachedRates(): void {
  rates.clear();
}

export function cachedRateCount(): number {
  return rates.size;
}
