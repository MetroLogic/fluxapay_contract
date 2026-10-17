'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { connectWallet, isAuthenticated, getJwt } from '../lib/auth';

export default function HomePage() {
  const router = useRouter();
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (isAuthenticated()) {
      router.replace('/payments');
    }
  }, [router]);

  async function handleConnect() {
    setConnecting(true);
    setError(null);
    try {
      await connectWallet();
      if (getJwt()) {
        router.push('/payments');
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Wallet connection failed');
    } finally {
      setConnecting(false);
    }
  }

  return (
    <main className="flex min-h-screen flex-col items-center justify-center bg-slate-50 px-4">
      <div className="w-full max-w-md rounded-2xl bg-white p-8 shadow-sm">
        <h1 className="text-2xl font-semibold text-slate-900">FluxaPay Merchant Dashboard</h1>
        <p className="mt-2 text-sm text-slate-600">
          Connect your Freighter wallet to authenticate via SEP-10 and manage payments.
        </p>

        <button
          type="button"
          onClick={handleConnect}
          disabled={connecting}
          className="mt-6 w-full rounded-lg bg-slate-900 px-4 py-3 text-sm font-medium text-white transition hover:bg-slate-700 disabled:cursor-not-allowed disabled:opacity-60"
        >
          {connecting ? 'Connecting…' : 'Connect Freighter Wallet'}
        </button>

        {error ? (
          <p className="mt-4 rounded-lg bg-red-50 px-3 py-2 text-sm text-red-700">{error}</p>
        ) : null}
      </div>
    </main>
  );
}
