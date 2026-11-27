"use client";

import { useEffect, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { connectWallet, isAuthenticated, AUTH_TOKEN_KEY } from "@/lib/auth";

/** Issue #1165: unauthenticated users land here and can sign in with Freighter. */
export default function LoginPage() {
  const router = useRouter();
  const params = useSearchParams();
  const [error, setError] = useState<string | null>(null);

  const next = params.get("next") || "/payments";
  useEffect(() => {
    if (isAuthenticated()) router.replace(next);
  }, [next, router]);

  async function signIn() {
    try {
      await connectWallet();
      // The backend issues the SEP-10 JWT; placeholder for the auth exchange.
      window.localStorage.setItem(AUTH_TOKEN_KEY, "pending-sep10-jwt");
      router.replace(next);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Wallet connection failed");
    }
  }

  return (
    <div className="mx-auto max-w-md rounded border border-slate-200 p-8 text-center dark:border-slate-800">
      <h1 className="mb-2 text-xl font-semibold">Sign in to FluxaPay</h1>
      <p className="mb-6 text-sm text-slate-500">Connect your Stellar wallet to continue.</p>
      <button
        type="button"
        onClick={signIn}
        className="rounded bg-sky-600 px-4 py-2 text-white hover:bg-sky-700"
      >
        Connect wallet
      </button>
      {error && <p role="alert" className="mt-4 text-sm text-red-600">{error}</p>}
    </div>
  );
}
