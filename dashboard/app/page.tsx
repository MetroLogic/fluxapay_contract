'use client';

import { useEffect, useRef, useState } from 'react';
import { useRouter } from 'next/navigation';
import { connectWallet, isAuthenticated, getJwt } from '../lib/auth';

type PaymentStatus = 'Pending' | 'Confirmed' | 'Expired' | 'Partial';

const STATUS_LABELS: Record<PaymentStatus, string> = {
  Pending: '⏳ Waiting for payment...',
  Confirmed: '✅ Payment confirmed!',
  Expired: '❌ Payment expired',
  Partial: '⚠️ Payment received but insufficient',
};

export default function HomePage() {
  const router = useRouter();
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<PaymentStatus>('Pending');
  const [remaining, setRemaining] = useState<string | null>(null);
  const [paymentId, setPaymentId] = useState<string | null>(null);
  const [successUrl, setSuccessUrl] = useState<string | null>(null);
  const [dark, setDark] = useState(false);
  const redirectTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (isAuthenticated()) {
      router.replace('/payments');
    }
  }, [router]);

  useEffect(() => {
    if (typeof window === 'undefined') return;
    const params = new URLSearchParams(window.location.search);
    setPaymentId(params.get('payment_id'));
    setSuccessUrl(params.get('success_url'));
  }, []);

  useEffect(() => {
    if (typeof window === 'undefined') return;
    const stored = localStorage.getItem('theme');
    const prefersDark =
      typeof window.matchMedia === 'function' &&
      window.matchMedia('(prefers-color-scheme: dark)').matches;
    setDark(stored ? stored === 'dark' : prefersDark);
  }, []);

  useEffect(() => {
    if (typeof window === 'undefined') return;
    document.documentElement.classList.toggle('dark', dark);
    localStorage.setItem('theme', dark ? 'dark' : 'light');
  }, [dark]);

  useEffect(() => {
    if (!paymentId) return;

    let source: EventSource | null = null;
    let pollTimer: ReturnType<typeof setInterval> | null = null;
    let closed = false;

    function applyStatus(next: PaymentStatus, remainingAmount?: string) {
      setStatus(next);
      setRemaining(remainingAmount ?? null);
      if (next === 'Confirmed' && successUrl && !redirectTimer.current) {
        redirectTimer.current = setTimeout(() => {
          window.location.href = successUrl;
        }, 2000);
      }
    }

    function handlePayload(data: string) {
      try {
        const parsed = JSON.parse(data) as {
          status?: PaymentStatus;
          remaining_amount?: string;
        };
        if (parsed.status && parsed.status in STATUS_LABELS) {
          applyStatus(parsed.status, parsed.remaining_amount);
        }
      } catch {
        // Ignore malformed events; keep last known status.
      }
    }

    function startPolling() {
      if (pollTimer) return;
      pollTimer = setInterval(async () => {
        try {
          const res = await fetch(`/v1/payments/${encodeURIComponent(paymentId as string)}`);
          if (!res.ok) return;
          const data = (await res.json()) as {
            status?: PaymentStatus;
            remaining_amount?: string;
          };
          if (data.status && data.status in STATUS_LABELS) {
            applyStatus(data.status, data.remaining_amount);
          }
        } catch {
          // Network hiccup; retry on next tick.
        }
      }, 5000);
    }

    if (typeof window !== 'undefined' && 'EventSource' in window) {
      source = new EventSource(
        `/v1/events/stream?payment_id=${encodeURIComponent(paymentId)}`,
      );
      source.onmessage = (event) => handlePayload(event.data);
      source.onerror = () => {
        if (closed) return;
        source?.close();
        source = null;
        startPolling();
      };
    } else {
      startPolling();
    }

    return () => {
      closed = true;
      source?.close();
      if (pollTimer) clearInterval(pollTimer);
      if (redirectTimer.current) {
        clearTimeout(redirectTimer.current);
        redirectTimer.current = null;
      }
    };
  }, [paymentId, successUrl]);

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
    <main className="flex min-h-screen flex-col items-center justify-center bg-slate-50 px-4 dark:bg-slate-950">
      <div className="w-full max-w-md rounded-2xl bg-white p-8 shadow-sm dark:bg-slate-900 dark:shadow-none">
        <div className="flex items-start justify-between gap-4">
          <h1 className="text-2xl font-semibold text-slate-900 dark:text-slate-100">
            FluxaPay Merchant Dashboard
          </h1>
          <button
            type="button"
            onClick={() => setDark((prev) => !prev)}
            aria-label={dark ? 'Switch to light mode' : 'Switch to dark mode'}
            aria-pressed={dark}
            className="rounded-lg border border-slate-200 px-2.5 py-1.5 text-xs font-medium text-slate-700 transition hover:bg-slate-100 dark:border-slate-700 dark:text-slate-200 dark:hover:bg-slate-800"
          >
            {dark ? '☀️ Light' : '🌙 Dark'}
          </button>
        </div>
        <p className="mt-2 text-sm text-slate-600 dark:text-slate-400">
          Connect your Freighter wallet to authenticate via SEP-10 and manage payments.
        </p>

        {paymentId ? (
          <div
            role="status"
            aria-live="polite"
            aria-atomic="true"
            className="mt-6 rounded-lg bg-slate-100 px-3 py-2 text-sm text-slate-800 dark:bg-slate-800 dark:text-slate-200"
          >
            <p>{STATUS_LABELS[status]}</p>
            {status === 'Partial' && remaining ? (
              <p className="mt-1 text-xs text-slate-600 dark:text-slate-400">Remaining: {remaining}</p>
            ) : null}
            {status === 'Expired' ? (
              <button
                type="button"
                onClick={() => window.location.reload()}
                className="mt-2 rounded-lg bg-slate-900 px-3 py-1.5 text-xs font-medium text-white transition hover:bg-slate-700 dark:bg-slate-100 dark:text-slate-900 dark:hover:bg-slate-300"
              >
                Retry
              </button>
            ) : null}
          </div>
        ) : null}

        <button
          type="button"
          onClick={handleConnect}
          disabled={connecting}
          className="mt-6 w-full rounded-lg bg-slate-900 px-4 py-3 text-sm font-medium text-white transition hover:bg-slate-700 disabled:cursor-not-allowed disabled:opacity-60 dark:bg-slate-100 dark:text-slate-900 dark:hover:bg-slate-300"
        >
          {connecting ? 'Connecting…' : 'Connect Freighter Wallet'}
        </button>

        {error ? (
          <p className="mt-4 rounded-lg bg-red-50 px-3 py-2 text-sm text-red-700 dark:bg-red-950 dark:text-red-300">
            {error}
          </p>
        ) : null}
      </div>
    </main>
  );
}
