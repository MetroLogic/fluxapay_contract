'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';

const API_BASE = process.env.NEXT_PUBLIC_API_BASE_URL ?? '';
const PAGE_SIZE = 25;
const FETCH_LIMIT = 50;

type DeliveryStatus = 'delivered' | 'failed' | 'pending';

interface WebhookDelivery {
  id: string;
  event_type: string;
  endpoint_url: string;
  status_code: number | null;
  attempt_count: number;
  delivered_at: string | null;
  status: DeliveryStatus;
  payload: unknown;
}

interface DeliveriesResponse {
  deliveries: WebhookDelivery[];
}

function isSuccess(delivery: WebhookDelivery): boolean {
  return (
    delivery.status === 'delivered' ||
    (delivery.status_code !== null && delivery.status_code >= 200 && delivery.status_code < 300)
  );
}

function formatTimestamp(value: string | null): string {
  if (!value) return '—';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function highlightJson(json: string): string {
  const escaped = json
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
  return escaped.replace(
    /("(?:\\u[a-fA-F0-9]{4}|\\[^u]|[^\\"])*"(\s*:)?|\b(?:true|false|null)\b|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/g,
    (match) => {
      let cls = 'text-amber-300';
      if (/^\"/.test(match)) {
        cls = /:$/.test(match) ? 'text-sky-300' : 'text-emerald-300';
      } else if (/^(true|false)$/.test(match)) {
        cls = 'text-purple-300';
      } else if (match === 'null') {
        cls = 'text-slate-400';
      }
      return `<span class="${cls}">${match}</span>`;
    },
  );
}

export default function WebhookSettingsPage() {
  const [deliveries, setDeliveries] = useState<WebhookDelivery[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [page, setPage] = useState(0);
  const [redelivering, setRedelivering] = useState<Record<string, boolean>>({});
  const [redeliverError, setRedeliverError] = useState<string | null>(null);
  const [payloadDelivery, setPayloadDelivery] = useState<WebhookDelivery | null>(null);

  const loadDeliveries = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch(`${API_BASE}/v1/webhooks/deliveries?limit=${FETCH_LIMIT}`, {
        credentials: 'include',
      });
      if (!res.ok) {
        throw new Error(`Failed to load deliveries (${res.status})`);
      }
      const data = (await res.json()) as DeliveriesResponse;
      setDeliveries(Array.isArray(data.deliveries) ? data.deliveries : []);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load deliveries');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadDeliveries();
  }, [loadDeliveries]);

  const pageCount = Math.max(1, Math.ceil(deliveries.length / PAGE_SIZE));
  const currentPage = Math.min(page, pageCount - 1);
  const visibleDeliveries = useMemo(
    () => deliveries.slice(currentPage * PAGE_SIZE, currentPage * PAGE_SIZE + PAGE_SIZE),
    [deliveries, currentPage],
  );

  const handleRedeliver = useCallback(
    async (delivery: WebhookDelivery) => {
      setRedeliverError(null);
      setRedelivering((prev) => ({ ...prev, [delivery.id]: true }));
      try {
        const res = await fetch(
          `${API_BASE}/v1/webhooks/deliveries/${encodeURIComponent(delivery.id)}/redeliver`,
          { method: 'POST', credentials: 'include' },
        );
        if (res.status === 429) {
          throw new Error('Redelivery rate limit reached (10 per hour). Try again later.');
        }
        if (!res.ok) {
          throw new Error(`Redelivery failed (${res.status})`);
        }
        await loadDeliveries();
      } catch (err) {
        setRedeliverError(err instanceof Error ? err.message : 'Redelivery failed');
      } finally {
        setRedelivering((prev) => {
          const next = { ...prev };
          delete next[delivery.id];
          return next;
        });
      }
    },
    [loadDeliveries],
  );

  const payloadJson = useMemo(() => {
    if (!payloadDelivery) return '';
    try {
      return JSON.stringify(payloadDelivery.payload, null, 2);
    } catch {
      return String(payloadDelivery.payload);
    }
  }, [payloadDelivery]);

  return (
    <div className="mx-auto max-w-6xl px-4 py-8">
      <div className="mb-6 flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold text-slate-900">Webhook Settings</h1>
          <p className="mt-1 text-sm text-slate-500">
            Review recent webhook delivery attempts and redeliver failed events.
          </p>
        </div>
        <button
          type="button"
          onClick={() => void loadDeliveries()}
          className="rounded-md border border-slate-300 px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50"
        >
          Refresh
        </button>
      </div>

      {redeliverError && (
        <div className="mb-4 rounded-md border border-red-200 bg-red-50 px-4 py-2 text-sm text-red-700">
          {redeliverError}
        </div>
      )}

      <div className="overflow-hidden rounded-lg border border-slate-200">
        <table className="min-w-full divide-y divide-slate-200 text-sm">
          <thead className="bg-slate-50">
            <tr>
              <th className="px-4 py-3 text-left font-medium text-slate-600">Event</th>
              <th className="px-4 py-3 text-left font-medium text-slate-600">Endpoint</th>
              <th className="px-4 py-3 text-left font-medium text-slate-600">Status</th>
              <th className="px-4 py-3 text-left font-medium text-slate-600">Attempts</th>
              <th className="px-4 py-3 text-left font-medium text-slate-600">Delivered At</th>
              <th className="px-4 py-3 text-right font-medium text-slate-600">Actions</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-100 bg-white">
            {loading && (
              <tr>
                <td colSpan={6} className="px-4 py-6 text-center text-slate-500">
                  Loading deliveries…
                </td>
              </tr>
            )}
            {!loading && error && (
              <tr>
                <td colSpan={6} className="px-4 py-6 text-center text-red-600">
                  {error}
                </td>
              </tr>
            )}
            {!loading && !error && visibleDeliveries.length === 0 && (
              <tr>
                <td colSpan={6} className="px-4 py-6 text-center text-slate-500">
                  No webhook deliveries yet.
                </td>
              </tr>
            )}
            {!loading &&
              !error &&
              visibleDeliveries.map((delivery) => {
                const success = isSuccess(delivery);
                return (
                  <tr key={delivery.id} className={success ? 'bg-green-50' : 'bg-red-50'}>
                    <td className="px-4 py-3 font-mono text-xs text-slate-800">
                      {delivery.event_type}
                    </td>
                    <td className="max-w-xs truncate px-4 py-3 text-slate-700">
                      {delivery.endpoint_url}
                    </td>
                    <td className="px-4 py-3">
                      <span
                        className={
                          success
                            ? 'rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-800'
                            : 'rounded-full bg-red-100 px-2 py-0.5 text-xs font-medium text-red-800'
                        }
                      >
                        {delivery.status_code ?? delivery.status}
                      </span>
                    </td>
                    <td className="px-4 py-3 text-slate-700">{delivery.attempt_count}</td>
                    <td className="px-4 py-3 text-slate-700">
                      {formatTimestamp(delivery.delivered_at)}
                    </td>
                    <td className="px-4 py-3 text-right">
                      <div className="flex justify-end gap-2">
                        <button
                          type="button"
                          onClick={() => setPayloadDelivery(delivery)}
                          className="rounded-md border border-slate-300 px-2 py-1 text-xs font-medium text-slate-700 hover:bg-slate-100"
                        >
                          View Payload
                        </button>
                        {!success && (
                          <button
                            type="button"
                            disabled={Boolean(redelivering[delivery.id])}
                            onClick={() => void handleRedeliver(delivery)}
                            className="rounded-md bg-slate-900 px-2 py-1 text-xs font-medium text-white hover:bg-slate-700 disabled:opacity-50"
                          >
                            {redelivering[delivery.id] ? 'Redelivering…' : 'Redeliver'}
                          </button>
                        )}
                      </div>
                    </td>
                  </tr>
                );
              })}
          </tbody>
        </table>
      </div>

      <div className="mt-4 flex items-center justify-between text-sm text-slate-600">
        <span>
          Page {currentPage + 1} of {pageCount}
        </span>
        <div className="flex gap-2">
          <button
            type="button"
            disabled={currentPage === 0}
            onClick={() => setPage((p) => Math.max(0, p - 1))}
            className="rounded-md border border-slate-300 px-3 py-1 font-medium hover:bg-slate-50 disabled:opacity-50"
          >
            Previous
          </button>
          <button
            type="button"
            disabled={currentPage >= pageCount - 1}
            onClick={() => setPage((p) => Math.min(pageCount - 1, p + 1))}
            className="rounded-md border border-slate-300 px-3 py-1 font-medium hover:bg-slate-50 disabled:opacity-50"
          >
            Next
          </button>
        </div>
      </div>

      {payloadDelivery && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
          role="dialog"
          aria-modal="true"
          onClick={() => setPayloadDelivery(null)}
        >
          <div
            className="max-h-[80vh] w-full max-w-2xl overflow-hidden rounded-lg bg-slate-900 shadow-xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between border-b border-slate-700 px-4 py-3">
              <h2 className="text-sm font-semibold text-slate-100">
                Payload · {payloadDelivery.event_type}
              </h2>
              <button
                type="button"
                onClick={() => setPayloadDelivery(null)}
                className="text-slate-400 hover:text-slate-200"
                aria-label="Close payload modal"
              >
                ✕
              </button>
            </div>
            <pre className="max-h-[65vh] overflow-auto px-4 py-3 text-xs leading-relaxed text-slate-200">
              <code dangerouslySetInnerHTML={{ __html: highlightJson(payloadJson) }} />
            </pre>
          </div>
        </div>
      )}
    </div>
  );
}
