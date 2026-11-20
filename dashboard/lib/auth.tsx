"use client";

/**
 * Issue #1158: Missing ../lib/auth module.
 *
 * dashboard/app/page.tsx imports connectWallet, isAuthenticated, and getJwt
 * from ../lib/auth, and layout.tsx imports AuthProvider from @/lib/auth.
 * No lib/auth module existed, so the app could not build.
 *
 * This module provides:
 *   connectWallet()    — connects Freighter, runs SEP-10 challenge/token
 *   isAuthenticated()   — checks for a stored JWT
 *   getJwt()           — returns the stored JWT (or null)
 *   AuthProvider       — React context so nested pages can read auth state
 *   useAuth()          — hook consuming the context
 *
 * The JWT is stored in localStorage under "fluxapay_jwt", consistent with the
 * convention already used by invoices, payment-links, and disputes pages.
 */

import React, {
  createContext,
  useContext,
  useEffect,
  useState,
} from "react";

const JWT_STORAGE_KEY = "fluxapay_jwt";
const WALLET_STORAGE_KEY = "fluxapay_wallet_address";

/**
 * Base URL of the FluxaPay backend (SEP-10 endpoints live under /auth).
 * Falls back to the indexer URL if only that is configured.
 */
function getApiBaseUrl(): string {
  return (
    process.env.NEXT_PUBLIC_API_URL ??
    process.env.NEXT_PUBLIC_INDEXER_URL ??
    process.env.NEXT_PUBLIC_FLUXAPAY_API_URL ??
    "http://localhost:3001"
  );
}

/** Freighter API types — the extension injects itself onto window. */
interface FreighterWindow extends Window {
  freighterApi?: {
    isConnected: () => Promise<boolean>;
    requestAccess: () => Promise<{ address: string }>;
    getPublicKey: () => Promise<{ address: string }>;
    signTransaction: (
      xdr: string,
      opts?: { networkPassphrase?: string },
    ) => Promise<{ signedTxXdr: string }>;
    getNetwork: () => Promise<{
      network: string;
      networkPassphrase: string;
    }>;
  };
}

function getFreighterWindow(): FreighterWindow {
  return (typeof window !== "undefined" ? window : {}) as FreighterWindow;
}

/**
 * Perform the SEP-10 challenge/token flow against the backend.
 *
 * 1. GET /auth/challenge?account=<publicKey> — returns an unsigned XDR.
 * 2. Freighter signs the XDR.
 * 3. POST /auth/token with the signed XDR — returns a JWT.
 */
async function sep10Auth(
  publicKey: string,
  networkPassphrase: string,
): Promise<string> {
  const apiBase = getApiBaseUrl();

  // 1. Request challenge
  const challengeUrl = `${apiBase}/auth/challenge?account=${encodeURIComponent(publicKey)}`;
  const challengeResp = await fetch(challengeUrl);
  if (!challengeResp.ok) {
    const body = await challengeResp.json().catch(() => ({}));
    throw new Error(body.error ?? `Challenge request failed (${challengeResp.status})`);
  }
  const challenge = (await challengeResp.json()) as { transaction: string };
  const unsignedXdr = challenge.transaction;

  // 2. Ask Freighter to sign
  const freighter = getFreighterWindow().freighterApi;
  if (!freighter) {
    throw new Error("Freighter wallet extension not found. Please install Freighter and try again.");
  }
  const signed = await freighter.signTransaction(unsignedXdr, { networkPassphrase });
  const signedXdr = signed.signedTxXdr;

  // 3. Exchange signed challenge for JWT
  const tokenResp = await fetch(`${apiBase}/auth/token`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ transaction: signedXdr }),
  });
  if (!tokenResp.ok) {
    const body = await tokenResp.json().catch(() => ({}));
    throw new Error(body.error ?? `Token exchange failed (${tokenResp.status})`);
  }
  const tokenBody = (await tokenResp.json()) as { token: string };
  return tokenBody.token;
}

export async function connectWallet(): Promise<void> {
  const freighter = getFreighterWindow().freighterApi;
  if (!freighter) {
    throw new Error("Freighter wallet extension not found. Please install Freighter and try again.");
  }

  const connected = await freighter.isConnected();
  if (!connected) {
    await freighter.requestAccess();
  }

  const { address: publicKey } = await freighter.getPublicKey();
  let networkPassphrase = "Test SDF Network ; September 2015";
  try {
    const network = await freighter.getNetwork();
    if (network?.networkPassphrase) {
      networkPassphrase = network.networkPassphrase;
    }
  } catch {
    // getNetwork may not be available in all Freighter versions — fall back to testnet.
  }

  const jwt = await sep10Auth(publicKey, networkPassphrase);

  if (typeof window !== "undefined") {
    localStorage.setItem(JWT_STORAGE_KEY, jwt);
    localStorage.setItem(WALLET_STORAGE_KEY, publicKey);
  }
}

export function isAuthenticated(): boolean {
  if (typeof window === "undefined") return false;
  return Boolean(localStorage.getItem(JWT_STORAGE_KEY));
}

export function getJwt(): string | null {
  if (typeof window === "undefined") return null;
  return localStorage.getItem(JWT_STORAGE_KEY);
}

export function getWalletAddress(): string | null {
  if (typeof window === "undefined") return null;
  return localStorage.getItem(WALLET_STORAGE_KEY);
}

export function disconnectWallet(): void {
  if (typeof window === "undefined") return;
  localStorage.removeItem(JWT_STORAGE_KEY);
  localStorage.removeItem(WALLET_STORAGE_KEY);
}

// ------------------------------------------------------------------ React context

interface AuthContextValue {
  jwt: string | null;
  walletAddress: string | null;
  ready: boolean;
}

const AuthContext = createContext<AuthContextValue>({
  jwt: null,
  walletAddress: null,
  ready: false,
});

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [jwt, setJwt] = useState<string | null>(null);
  const [walletAddress, setWalletAddress] = useState<string | null>(null);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    if (typeof window === "undefined") return;
    setJwt(localStorage.getItem(JWT_STORAGE_KEY));
    setWalletAddress(localStorage.getItem(WALLET_STORAGE_KEY));
    setReady(true);
  }, []);

  return (
    <AuthContext.Provider value={{ jwt, walletAddress, ready }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth(): AuthContextValue {
  return useContext(AuthContext);
}
