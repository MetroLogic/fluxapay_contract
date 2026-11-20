"use client";

/**
 * Issue #1165: minimal client-side auth helpers shared by the route guard and
 * the navigation shell. The JWT is issued by the SEP-10 wallet-connect flow.
 */
export const AUTH_TOKEN_KEY = "fluxapay_jwt";

export function isAuthenticated(): boolean {
  if (typeof window === "undefined") return false;
  try {
    return Boolean(window.localStorage.getItem(AUTH_TOKEN_KEY));
  } catch {
    return false;
  }
}

/** Return the stored JWT for authenticated API calls, if present. */
export function getJwt(): string | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage.getItem(AUTH_TOKEN_KEY);
  } catch {
    return null;
  }
}

/**
 * Start the SEP-10 wallet connect flow. The Freighter extension exposes
 * `window.freighter`; the actual challenge/response JWT exchange happens on
 * the auth route. Kept defensive so the dashboard renders without the
 * extension installed.
 */
export async function connectWallet(): Promise<string> {
  const freighter = (window as unknown as { freighter?: { getAddress(): Promise<{ address: string }> } }).freighter;
  if (!freighter) throw new Error("Freighter wallet extension is not installed");
  const { address } = await freighter.getAddress();
  return address;
}

export function signOut(): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.removeItem(AUTH_TOKEN_KEY);
  } catch {
    /* storage unavailable */
  }
}
