"use client";

// Issue #1164: merchant settlement / treasury view. Uses the merchant's
// SEP-10 JWT to read the settlement history from the indexer.

import { useCallback, useEffect, useState } from "react";

interface Settlement {
  payment_id: string;
  amount_usdc: string;
  status: string;
  fiat_amount: string | null;
  fiat_currency: string | null;
  created_at: string | null;
  settled_at: string | null;
}

interface SettlementView {
  treasury_balance_usdc: string;
  payout_status: string;
  settlements: Settlement[];
}

const EMPTY_VIEW: SettlementView = {
  treasury_balance_usdc: "0.00",
  payout_status: "—",
  settlements: [],
};

export default function SettlementsPage() {
  const apiUrl =
    process.env.NEXT_PUBLIC_INDEXER_URL ??
    process.env.NEXT_PUBLIC_API_URL ??
    "http://localhost:3001";
  const [view, setView] = useState<SettlementView>(EMPTY_VIEW);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadSettlements = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const token =
        typeof window !== "undefined" ? localStorage.getItem("fluxapay_jwt") : null;
      if (!token) throw new Error("Connect your merchant wallet to view settlements.");
      const response = await fetch(`${apiUrl}/v1/merchant/settlements`, {
        headers: { Authorization: `Bearer ${token}`, Accept: "application/json" },
      });
      if (!response.ok) {
        throw new Error(`Failed to fetch settlements: ${response.status} ${response.statusText}`);
      }
      const body = (await response.json()) as SettlementView;
      setView({
        treasury_balance_usdc: body.treasury_balance_usdc ?? "0.00",
        payout_status: body.payout_status ?? "—",
        settlements: Array.isArray(body.settlements) ? body.settlements : [],
      });
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoading(false);
    }
  }, [apiUrl]);

  useEffect(() => {
    void loadSettlements();
  }, [loadSettlements]);

  return (
    <main className="min-h-screen bg-gray-50 p-6">
      <div className="mx-auto max-w-6xl">
        <header className="mb-6 flex flex-wrap items-center justify-between gap-4">
          <div>
            <h1 className="text-2xl font-bold text-gray-900">Settlements &amp; Treasury</h1>
            <p className="mt-1 text-sm text-gray-600">
              Settlement history, treasury balance, and fiat payout status.
            </p>
          </div>
          <div className="flex items-center gap-3">
            <a href="/refunds" className="text-sm font-medium text-indigo-700 hover:text-indigo-900">
              Refunds
            </a>
            <button
              type="button"
              onClick={() => void loadSettlements()}
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

        <section aria-label="Treasury summary" className="mb-6 grid gap-4 sm:grid-cols-2">
          <article className="rounded-lg border border-gray-200 bg-white p-5 shadow-sm">
            <h2 className="text-sm font-medium text-gray-500">Treasury balance (pending payouts)</h2>
            <p className="mt-2 text-2xl font-semibold text-gray-900">
              {loading ? "…" : `${view.treasury_balance_usdc} USDC`}
            </p>
          </article>
          <article className="rounded-lg border border-gray-200 bg-white p-5 shadow-sm">
            <h2 className="text-sm font-medium text-gray-500">Fiat payout status</h2>
            <p className="mt-2 text-2xl font-semibold text-gray-900">
              {loading ? "…" : view.payout_status}
            </p>
          </article>
        </section>

        <div className="overflow-x-auto rounded-lg border border-gray-200 bg-white shadow-sm">
          <table className="min-w-full divide-y divide-gray-200 text-sm">
            <thead className="bg-gray-50">
              <tr>
                {["Payment ID", "Amount (USDC)", "Status", "Fiat payout", "Created", "Settled"].map(
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
              {loading && view.settlements.length === 0 ? (
                <tr><td colSpan={6} className="py-8 text-center text-gray-400">Loading…</td></tr>
              ) : view.settlements.length === 0 ? (
                <tr><td colSpan={6} className="py-8 text-center text-gray-400">No settlements found.</td></tr>
              ) : (
                view.settlements.map((settlement) => (
                  <tr key={settlement.payment_id} className="hover:bg-gray-50">
                    <td className="px-4 py-3 font-mono text-xs text-gray-700">{settlement.payment_id}</td>
                    <td className="px-4 py-3 tabular-nums">{settlement.amount_usdc}</td>
                    <td className="px-4 py-3">
                      <span className="rounded-full bg-slate-100 px-2 py-0.5 text-xs font-medium text-slate-700">
                        {settlement.status}
                      </span>
                    </td>
                    <td className="px-4 py-3">
                      {settlement.fiat_amount === null
                        ? "—"
                        : `${settlement.fiat_amount} ${settlement.fiat_currency ?? ""}`.trim()}
                    </td>
                    <td className="px-4 py-3 text-gray-600">
                      {settlement.created_at ? new Date(settlement.created_at).toLocaleString() : "—"}
                    </td>
                    <td className="px-4 py-3 text-gray-600">
                      {settlement.settled_at ? new Date(settlement.settled_at).toLocaleString() : "—"}
                    </td>
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
