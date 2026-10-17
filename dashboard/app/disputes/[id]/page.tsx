"use client";

import { useEffect, useRef, useState } from "react";

// Issue #1039: live dispute status updates via the indexer's SSE endpoint
// (`GET /v1/events/stream?dispute_id=...`) instead of polling a REST
// endpoint — there is no `GET /v1/disputes/:id` route yet, so this page's
// only data source is the event stream itself.

const DISPUTE_EVENT_TYPES = [
  "dispute.opened",
  "dispute.responded",
  "dispute.escalated",
  "dispute.vote_cast",
  "dispute.resolved",
  "dispute.auto_resolved",
] as const;

interface DisputeEvent {
  type: string;
  receivedAt: string;
  data: Record<string, unknown>;
}

// Issue #979: skeleton rows shown while the dispute event stream is still
// connecting and no events have arrived yet.
function SkeletonRow() {
  return (
    <li className="animate-pulse p-4">
      <div className="flex items-center justify-between">
        <div className="h-4 w-32 rounded bg-gray-200 dark:bg-gray-700" />
        <div className="h-3 w-24 rounded bg-gray-200 dark:bg-gray-700" />
      </div>
      <div className="mt-2 h-12 rounded bg-gray-200 dark:bg-gray-700" />
    </li>
  );
}

export default function DisputeDetailPage({ params }: { params: { id: string } }) {
  const disputeId = params.id;
  const [events, setEvents] = useState<DisputeEvent[]>([]);
  const [connected, setConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const sourceRef = useRef<EventSource | null>(null);

  const apiUrl =
    process.env.NEXT_PUBLIC_INDEXER_URL ??
    process.env.NEXT_PUBLIC_API_URL ??
    "http://localhost:3001";

  useEffect(() => {
    const token = typeof window !== "undefined" ? localStorage.getItem("fluxapay_jwt") : null;
    if (!token) {
      setError("Connect your merchant wallet to view live dispute updates.");
      return;
    }

    const url = `${apiUrl}/v1/events/stream?dispute_id=${encodeURIComponent(disputeId)}&token=${encodeURIComponent(token)}`;
    const source = new EventSource(url);
    sourceRef.current = source;

    source.onopen = () => setConnected(true);
    source.onerror = () => {
      setConnected(false);
      setError("Live updates disconnected — retrying…");
    };

    for (const type of DISPUTE_EVENT_TYPES) {
      source.addEventListener(type, (evt) => {
        const messageEvent = evt as MessageEvent<string>;
        let data: Record<string, unknown> = {};
        try {
          data = JSON.parse(messageEvent.data);
        } catch {
          // Non-JSON payloads are shown as-is below.
        }
        setError(null);
        setEvents((prev) => [{ type, receivedAt: new Date().toISOString(), data }, ...prev]);
      });
    }

    return () => {
      source.close();
      sourceRef.current = null;
    };
  }, [apiUrl, disputeId]);

  const isLoading = events.length === 0 && !error;

  return (
    <main className="min-h-screen bg-gray-50 p-6">
      <div className="mx-auto max-w-3xl">
        <header className="mb-6">
          <h1 className="text-2xl font-bold text-gray-900">Dispute {disputeId}</h1>
          <p className="mt-1 flex items-center gap-2 text-sm text-gray-600">
            <span
              className={`inline-block h-2 w-2 rounded-full ${connected ? "bg-green-500" : "bg-gray-400"}`}
              aria-hidden="true"
            />
            {connected ? "Live" : "Connecting…"}
          </p>
        </header>

        {error && (
          <p role="alert" className="mb-4 rounded-md border border-amber-300 bg-amber-50 p-3 text-sm text-amber-800">
            {error}
          </p>
        )}

        <div className="rounded-lg border border-gray-200 bg-white shadow-sm">
          {isLoading ? (
            <ul aria-busy="true" aria-label="Loading dispute updates" className="divide-y divide-gray-100">
              {Array.from({ length: 5 }).map((_, index) => (
                <SkeletonRow key={index} />
              ))}
            </ul>
          ) : events.length === 0 ? (
            <p className="p-6 text-center text-sm text-gray-400">
              No status updates yet. This page updates live as the dispute progresses —
              opened, responded, escalated, voted on, and resolved.
            </p>
          ) : (
            <ul className="divide-y divide-gray-100">
              {events.map((event, index) => (
                <li key={index} className="p-4">
                  <div className="flex items-center justify-between">
                    <span className="font-mono text-sm font-medium text-indigo-700">{event.type}</span>
                    <time className="text-xs text-gray-400">{event.receivedAt}</time>
                  </div>
                  <pre className="mt-2 overflow-x-auto rounded bg-gray-50 p-2 text-xs text-gray-600">
                    {JSON.stringify(event.data, null, 2)}
                  </pre>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </main>
  );
}
