"use client";

import { useCallback, useEffect, useState } from "react";

interface PaymentLinkAnalytics {
  link_id: string;
  views: number;
  attempts: number;
  conversions: number;
  conversion_rate: number | null;
  total_revenue_usdc: string;
  last_used_at: string | null;
  daily_conversions: number[];
}

function Sparkline({ values }: { values: number[] }) {
  const max = Math.max(1, ...values);
  const points = values
    .map((value, index) => `${(index / Math.max(values.length - 1, 1)) * 100},${24 - (value / max) * 20}`)
    .join(" ");

  return (
    <svg
      viewBox="0 0 100 26"
      role="img"
      aria-label={`Daily conversions: ${values.join(", ")}`}
      className="h-7 w-24 overflow-visible"
    >
      <polyline
        points={points}
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinejoin="round"
        strokeLinecap="round"
        className="text-indigo-600"
      />
    </svg>
  );
}

export default function PaymentLinksPage() {
  const [links, setLinks] = useState<PaymentLinkAnalytics[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const apiUrl =
    process.env.NEXT_PUBLIC_INDEXER_URL ??
    process.env.NEXT_PUBLIC_API_URL ??
    "http://localhost:3001";

  const loadLinks = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const token =
        typeof window !== "undefined" ? localStorage.getItem("fluxapay_jwt") : null;
      if (!token) throw new Error("Connect your merchant wallet to view payment links.");

      const response = await fetch(`${apiUrl}/v1/payment-links`, {
        headers: { Authorization: `Bearer ${token}`, Accept: "application/json" },
      });
      if (!response.ok) {
        throw new Error(`Failed to fetch payment links: ${response.status} ${response.statusText}`);
      }
      const body = (await response.json()) as { payment_links?: PaymentLinkAnalytics[] };
      setLinks(body.payment_links ?? []);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoading(false);
    }
  }, [apiUrl]);

  useEffect(() => {
    void loadLinks();
  }, [loadLinks]);

  return (
    <main className="min-h-screen bg-gray-50 p-6">
      <div className="mx-auto max-w-6xl">
        <header className="mb-6 flex items-center justify-between">
          <div>
            <h1 className="text-2xl font-bold text-gray-900">Payment links</h1>
            <p className="mt-1 text-sm text-gray-600">Views, usage, and daily conversions for your links.</p>
          </div>
          <div className="flex items-center gap-3">
            <a href="/invoices" className="text-sm font-medium text-indigo-700 hover:text-indigo-900">
              Invoices
            </a>
            <a href="/subscriptions" className="text-sm font-medium text-indigo-700 hover:text-indigo-900">
              Subscriptions
            </a>
            <button
              type="button"
              onClick={() => void loadLinks()}
              disabled={loading}
              className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
            >
              {loading ? "Loading…" : "Refresh"}
            </button>
          </div>
        </header>

        {error && (
          <p role="alert" className="mb-4 rounded-md border border-red-300 bg-red-50 p-3 text-sm text-red-700">
            {error}
          </p>
        )}

        <div className="overflow-x-auto rounded-lg border border-gray-200 bg-white shadow-sm">
          <table className="min-w-full divide-y divide-gray-200 text-sm">
            <thead className="bg-gray-50">
              <tr>
                {["Link", "Views", "Attempts", "Conversions", "Conversion rate", "Revenue (USDC)", "7-day trend"].map(
                  (label) => (
                    <th key={label} className="px-4 py-3 text-left text-xs font-semibold uppercase tracking-wide text-gray-500">
                      {label}
                    </th>
                  ),
                )}
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {loading && links.length === 0 ? (
                <tr><td colSpan={7} className="py-8 text-center text-gray-400">Loading…</td></tr>
              ) : links.length === 0 ? (
                <tr><td colSpan={7} className="py-8 text-center text-gray-400">No payment links found.</td></tr>
              ) : (
                links.map((link) => (
                  <tr key={link.link_id} className="hover:bg-gray-50">
                    <td className="px-4 py-3 font-mono text-xs text-gray-700">{link.link_id}</td>
                    <td className="px-4 py-3 tabular-nums">{link.views}</td>
                    <td className="px-4 py-3 tabular-nums">{link.attempts}</td>
                    <td className="px-4 py-3 tabular-nums">{link.conversions}</td>
                    <td className="px-4 py-3 tabular-nums">
                      {link.conversion_rate === null ? "—" : `${(link.conversion_rate * 100).toFixed(1)}%`}
                    </td>
                    <td className="px-4 py-3 tabular-nums">{link.total_revenue_usdc}</td>
                    <td className="px-4 py-3"><Sparkline values={link.daily_conversions} /></td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </div>
    </main>
  );
}
