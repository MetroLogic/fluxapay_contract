"use client";

// Issue #1162: merchant refund view. Refunds are fetched from the indexer's
// merchant-scoped view with the merchant's SEP-10 JWT.

import { useCallback, useEffect, useState } from "react";

interface Refund {
  refund_id: string;
  payment_id: string;
  amount_usdc: string;
  fee_usdc: string;
  status: string;
  created_at: string | null;
}

const STATUS_OPTIONS = ["All", "pending", "completed", "failed", "cancelled"] as const;

export default function RefundsPage() {
  const apiUrl =
    process.env.NEXT_PUBLIC_INDEXER_URL ??
    process.env.NEXT_PUBLIC_API_URL ??
    "http://localhost:3001";
  const [status, setStatus] = useState<(typeof STATUS_OPTIONS)[number]>("All");
  const [refunds, setRefunds] = useState<Refund[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadRefunds = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const token =
        typeof window !== "undefined" ? localStorage.getItem("fluxapay_jwt") : null;
      if (!token) throw new Error("Connect your merchant wallet to view refunds.");
      const params = new URLSearchParams();
      if (status !== "All") params.set("status", status);
      const response = await fetch(`${apiUrl}/v1/merchant/refunds?${params}`, {
        headers: { Authorization: `Bearer ${token}`, Accept: "application/json" },
      });
      if (!response.ok) {
        throw new Error(`Failed to fetch refunds: ${response.status} ${response.statusText}`);
      }
      const body = (await response.json()) as { refunds?: Refund[] };
      setRefunds(body.refunds ?? []);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoading(false);
    }
  }, [apiUrl, status]);

  useEffect(() => {
    void loadRefunds();
  }, [loadRefunds]);

  return (
    <main className="min-h-screen bg-gray-50 p-6">
      <div className="mx-auto max-w-6xl">
        <header className="mb-6 flex flex-wrap items-center justify-between gap-4">
          <div>
            <h1 className="text-2xl font-bold text-gray-900">Refunds</h1>
            <p className="mt-1 text-sm text-gray-600">
              Refund requests grouped by payment, with status, amount, and fees.
            </p>
          </div>
          <div className="flex items-center gap-3">
            <label className="text-sm font-medium text-gray-700">
              Status
              <select
                value={status}
                onChange={(event) =>
                  setStatus(event.target.value as (typeof STATUS_OPTIONS)[number])
                }
                className="ml-2 rounded-md border border-gray-300 bg-white px-2 py-1"
              >
                {STATUS_OPTIONS.map((option) => (
                  <option key={option} value={option}>
                    {option}
                  </option>
                ))}
              </select>
            </label>
            <a href="/invoices" className="text-sm font-medium text-indigo-700 hover:text-indigo-900">
              Invoices
            </a>
            <a href="/settlements" className="text-sm font-medium text-indigo-700 hover:text-indigo-900">
              Settlements
            </a>
            <button
              type="button"
              onClick={() => void loadRefunds()}
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
                {["Refund ID", "Payment ID", "Status", "Amount (USDC)", "Fee (USDC)", "Created"].map(
                  (label) => (
                    <th
                      key={label}
                      scope="col"
                      className="px-4 py-3 text-left text-xs font-semibold uppercase tracking-wide text-gray-500"
                    >
                      {label}
                    </th>
                  ),
                )}
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {loading && refunds.length === 0 ? (
                <tr><td colSpan={6} className="py-8 text-center text-gray-400">Loading…</td></tr>
              ) : refunds.length === 0 ? (
                <tr><td colSpan={6} className="py-8 text-center text-gray-400">No refunds found.</td></tr>
              ) : (
                refunds.map((refund) => (
                  <tr key={refund.refund_id} className="hover:bg-gray-50">
                    <td className="px-4 py-3 font-mono text-xs text-gray-700">{refund.refund_id}</td>
                    <td className="px-4 py-3 font-mono text-xs text-gray-700">{refund.payment_id}</td>
                    <td className="px-4 py-3">
                      <span className="rounded-full bg-slate-100 px-2 py-0.5 text-xs font-medium text-slate-700">
                        {refund.status}
                      </span>
                    </td>
                    <td className="px-4 py-3 tabular-nums">{refund.amount_usdc}</td>
                    <td className="px-4 py-3 tabular-nums">{refund.fee_usdc}</td>
                    <td className="px-4 py-3 text-gray-600">
                      {refund.created_at ? new Date(refund.created_at).toLocaleString() : "—"}
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
        <p className="mt-3 text-xs text-gray-500">
          Fee shown is the fee charged on the original payment; refund events do not record a separate fee.
        </p>
      </div>
    </main>
  );
}
