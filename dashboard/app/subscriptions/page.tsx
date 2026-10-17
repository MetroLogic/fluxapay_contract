"use client";

import { useCallback, useEffect, useState } from "react";

interface SubscriptionMetrics {
  period: string;
  mrr_usdc: string;
  active_subscribers: number;
  new_subscribers: number;
  churned_subscribers: number;
  churn_rate: number | null;
  trial_conversions: number;
  trial_conversion_rate: number | null;
  past_due_subscribers: number;
  dunning_recovery_rate: number | null;
}

const EMPTY_METRICS: Omit<SubscriptionMetrics, "period"> = {
  mrr_usdc: "0.00",
  active_subscribers: 0,
  new_subscribers: 0,
  churned_subscribers: 0,
  churn_rate: null,
  trial_conversions: 0,
  trial_conversion_rate: null,
  past_due_subscribers: 0,
  dunning_recovery_rate: null,
};

function formatRate(rate: number | null): string {
  return rate === null ? "—" : `${(rate * 100).toFixed(1)}%`;
}

export default function SubscriptionsPage() {
  const [period, setPeriod] = useState(() => new Date().toISOString().slice(0, 7));
  const [metrics, setMetrics] = useState<SubscriptionMetrics>({
    period,
    ...EMPTY_METRICS,
  });
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const apiUrl =
    process.env.NEXT_PUBLIC_INDEXER_URL ??
    process.env.NEXT_PUBLIC_API_URL ??
    process.env.NEXT_PUBLIC_FLUXAPAY_API_URL ??
    "http://localhost:3001";

  const loadMetrics = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const apiKey = localStorage.getItem("fluxapay_api_key");
      const merchantId = localStorage.getItem("fluxapay_merchant_id");
      if (!apiKey || !merchantId) {
        throw new Error("Set your merchant API key and merchant ID to view subscription metrics.");
      }

      const query = new URLSearchParams({ merchant_id: merchantId, period });
      const response = await fetch(`${apiUrl}/v1/subscriptions/metrics?${query}`, {
        headers: { "x-api-key": apiKey, Accept: "application/json" },
      });
      if (!response.ok) {
        throw new Error(`Failed to fetch subscription metrics: ${response.status} ${response.statusText}`);
      }
      setMetrics((await response.json()) as SubscriptionMetrics);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoading(false);
    }
  }, [apiUrl, period]);

  useEffect(() => {
    void loadMetrics();
  }, [loadMetrics]);

  const cards: Array<{ label: string; value: string | number }> = [
    { label: "Monthly recurring revenue", value: `$${metrics.mrr_usdc} USDC` },
    { label: "Active subscribers", value: metrics.active_subscribers },
    { label: "New subscribers", value: metrics.new_subscribers },
    { label: "Churned subscribers", value: metrics.churned_subscribers },
    { label: "Churn rate", value: formatRate(metrics.churn_rate) },
    { label: "Trial conversions", value: metrics.trial_conversions },
    { label: "Trial conversion rate", value: formatRate(metrics.trial_conversion_rate) },
    { label: "Past due subscribers", value: metrics.past_due_subscribers },
    { label: "Dunning recovery rate", value: formatRate(metrics.dunning_recovery_rate) },
  ];

  return (
    <main className="min-h-screen bg-gray-50 p-6">
      <div className="mx-auto max-w-6xl">
        <header className="mb-6 flex flex-wrap items-center justify-between gap-4">
          <div>
            <h1 className="text-2xl font-bold text-gray-900">Subscription metrics</h1>
            <p className="mt-1 text-sm text-gray-600">Recurring revenue and subscriber health.</p>
          </div>
          <div className="flex items-center gap-4">
            <a href="/invoices" className="text-sm font-medium text-indigo-700 hover:text-indigo-900">
              Invoices
            </a>
            <a href="/payment-links" className="text-sm font-medium text-indigo-700 hover:text-indigo-900">
              Payment links
            </a>
            <label className="text-sm font-medium text-gray-700">
              Period
              <input
                type="month"
                value={period}
                onChange={(event) => setPeriod(event.target.value)}
                className="ml-2 rounded-md border border-gray-300 bg-white px-2 py-1"
              />
            </label>
            <button
              type="button"
              onClick={() => void loadMetrics()}
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

        <section aria-label="Subscription metrics" className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {cards.map((card) => (
            <article key={card.label} className="rounded-lg border border-gray-200 bg-white p-5 shadow-sm">
              <h2 className="text-sm font-medium text-gray-500">{card.label}</h2>
              <p className="mt-2 text-2xl font-semibold text-gray-900">
                {loading ? "…" : card.value}
              </p>
            </article>
          ))}
        </section>
      </div>
    </main>
  );
}
