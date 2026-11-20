"use client";

import { useCallback, useEffect, useRef, useState } from "react";

// Issue #1039: live dispute status updates via the indexer's SSE endpoint
// (`GET /v1/events/stream?dispute_id=...`) instead of polling a REST
// endpoint — there is no `GET /v1/disputes/:id` route yet, so this page's
// only data source is the event stream itself.
//
// Issue #1161: adds evidence display and a merchant response (evidence
// upload) action alongside the live event stream.

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

interface EvidenceItem {
  ipfs_cid?: string;
  evidence_hash?: string;
  ipfs_gateway_url?: string;
  filename?: string;
  content_type?: string;
  uploaded_by?: string;
  tx_hash?: string;
  created_at?: string;
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
  const [evidence, setEvidence] = useState<EvidenceItem[]>([]);
  const [evidenceLoading, setEvidenceLoading] = useState(false);
  const [evidenceError, setEvidenceError] = useState<string | null>(null);
  const [uploading, setUploading] = useState(false);
  const [uploadError, setUploadError] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const sourceRef = useRef<EventSource | null>(null);

  const apiUrl =
    process.env.NEXT_PUBLIC_INDEXER_URL ??
    process.env.NEXT_PUBLIC_API_URL ??
    "http://localhost:3001";

  const getToken = useCallback(() => {
    return typeof window !== "undefined" ? localStorage.getItem("fluxapay_jwt") : null;
  }, []);

  // Issue #1161: fetch evidence list from the indexer.
  const fetchEvidence = useCallback(async () => {
    const token = getToken();
    if (!token) return;
    setEvidenceLoading(true);
    setEvidenceError(null);
    try {
      const resp = await fetch(
        `${apiUrl}/v1/disputes/${encodeURIComponent(disputeId)}/evidence`,
        {
          headers: {
            Accept: "application/json",
            Authorization: `Bearer ${token}`,
          },
        },
      );
      if (!resp.ok) {
        throw new Error(`Failed to fetch evidence: ${resp.status} ${resp.statusText}`);
      }
      const data = (await resp.json()) as { evidence?: EvidenceItem[] };
      setEvidence(data.evidence ?? []);
    } catch (err: unknown) {
      setEvidenceError(err instanceof Error ? err.message : String(err));
    } finally {
      setEvidenceLoading(false);
    }
  }, [apiUrl, disputeId, getToken]);

  // Issue #1161: merchant response — upload evidence file.
  const handleUploadEvidence = useCallback(async () => {
    const token = getToken();
    if (!token) {
      setUploadError("Connect your merchant wallet to upload evidence.");
      return;
    }
    const file = fileInputRef.current?.files?.[0];
    if (!file) {
      setUploadError("Select a file to upload.");
      return;
    }

    setUploading(true);
    setUploadError(null);
    try {
      const formData = new FormData();
      formData.append("file", file);

      const resp = await fetch(
        `${apiUrl}/v1/disputes/${encodeURIComponent(disputeId)}/evidence`,
        {
          method: "POST",
          headers: {
            Authorization: `Bearer ${token}`,
          },
          body: formData,
        },
      );
      if (!resp.ok) {
        const body = await resp.json().catch(() => ({ error: `HTTP ${resp.status}` }));
        throw new Error(body.error ?? `HTTP ${resp.status}`);
      }
      // Reset file input and refresh evidence list.
      if (fileInputRef.current) fileInputRef.current.value = "";
      await fetchEvidence();
    } catch (err: unknown) {
      setUploadError(err instanceof Error ? err.message : String(err));
    } finally {
      setUploading(false);
    }
  }, [apiUrl, disputeId, getToken, fetchEvidence]);

  useEffect(() => {
    const token = getToken();
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

    // Issue #1161: also fetch evidence on mount.
    void fetchEvidence();

    return () => {
      source.close();
      sourceRef.current = null;
    };
  }, [apiUrl, disputeId, getToken, fetchEvidence]);

  const isLoading = events.length === 0 && !error;

  return (
    <main className="min-h-screen bg-gray-50 p-6 dark:bg-gray-900">
      <div className="mx-auto max-w-3xl">
        <header className="mb-6">
          <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Dispute {disputeId}</h1>
          <p className="mt-1 flex items-center gap-2 text-sm text-gray-600 dark:text-gray-400">
            <span
              className={`inline-block h-2 w-2 rounded-full ${connected ? "bg-green-500" : "bg-gray-400"}`}
              aria-hidden="true"
            />
            {connected ? "Live" : "Connecting…"}
          </p>
          <a
            href="/disputes"
            className="mt-2 inline-block text-sm font-medium text-indigo-600 hover:text-indigo-800 dark:text-indigo-400 dark:hover:text-indigo-300"
          >
            ← Back to disputes
          </a>
        </header>

        {error && (
          <p role="alert" className="mb-4 rounded-md border border-amber-300 bg-amber-50 p-3 text-sm text-amber-800 dark:border-amber-700 dark:bg-amber-900/40 dark:text-amber-200">
            {error}
          </p>
        )}

        {/* Issue #1161: Evidence section */}
        <section className="mb-6 rounded-lg border border-gray-200 bg-white p-4 shadow-sm dark:border-gray-700 dark:bg-gray-800" aria-label="Dispute evidence">
          <h2 className="mb-3 text-sm font-semibold text-gray-700 dark:text-gray-300">Evidence</h2>

          {evidenceError && (
            <p role="alert" className="mb-3 text-sm text-red-600 dark:text-red-400">
              {evidenceError}
            </p>
          )}

          {evidenceLoading ? (
            <p className="text-sm text-gray-400 dark:text-gray-500">Loading evidence…</p>
          ) : evidence.length === 0 ? (
            <p className="text-sm text-gray-400 dark:text-gray-500">No evidence has been submitted yet.</p>
          ) : (
            <ul className="divide-y divide-gray-100 dark:divide-gray-700">
              {evidence.map((item, index) => (
                <li key={index} className="py-2">
                  <div className="flex items-center justify-between">
                    <span className="text-sm font-medium text-gray-700 dark:text-gray-300">
                      {item.filename ?? "Unnamed file"}
                    </span>
                    <time className="text-xs text-gray-400 dark:text-gray-500">
                      {item.created_at ?? "—"}
                    </time>
                  </div>
                  {item.ipfs_gateway_url && (
                    <a
                      href={item.ipfs_gateway_url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="mt-1 inline-block text-xs text-indigo-600 hover:text-indigo-800 dark:text-indigo-400 dark:hover:text-indigo-300"
                    >
                      View on IPFS ↗
                    </a>
                  )}
                  {item.uploaded_by && (
                    <p className="mt-0.5 text-xs text-gray-400 dark:text-gray-500">
                      Uploaded by: {item.uploaded_by}
                    </p>
                  )}
                </li>
              ))}
            </ul>
          )}

          {/* Issue #1161: Merchant response — evidence upload */}
          <div className="mt-4 border-t border-gray-100 pt-3 dark:border-gray-700">
            <h3 className="mb-2 text-xs font-semibold text-gray-600 dark:text-gray-400">
              Submit a response
            </h3>
            {uploadError && (
              <p role="alert" className="mb-2 text-sm text-red-600 dark:text-red-400">
                {uploadError}
              </p>
            )}
            <div className="flex items-center gap-2">
              <input
                ref={fileInputRef}
                type="file"
                className="text-sm text-gray-600 dark:text-gray-300 file:mr-3 file:rounded-md file:border file:border-gray-300 file:bg-white file:px-3 file:py-1.5 file:text-sm file:font-medium file:text-gray-700 hover:file:bg-gray-50 dark:file:border-gray-600 dark:file:bg-gray-800 dark:file:text-gray-200"
                aria-label="Select evidence file"
              />
              <button
                type="button"
                onClick={() => void handleUploadEvidence()}
                disabled={uploading}
                className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50 dark:bg-indigo-500 dark:hover:bg-indigo-400"
              >
                {uploading ? "Uploading…" : "Upload evidence"}
              </button>
            </div>
          </div>
        </section>

        {/* Live event stream */}
        <div className="rounded-lg border border-gray-200 bg-white shadow-sm dark:border-gray-700 dark:bg-gray-800">
          <h2 className="border-b border-gray-100 p-4 text-sm font-semibold text-gray-700 dark:border-gray-700 dark:text-gray-300">
            Live updates
          </h2>
          {isLoading ? (
            <ul aria-busy="true" aria-label="Loading dispute updates" className="divide-y divide-gray-100 dark:divide-gray-700">
              {Array.from({ length: 5 }).map((_, index) => (
                <SkeletonRow key={index} />
              ))}
            </ul>
          ) : events.length === 0 ? (
            <p className="p-6 text-center text-sm text-gray-400 dark:text-gray-500">
              No status updates yet. This page updates live as the dispute progresses —
              opened, responded, escalated, voted on, and resolved.
            </p>
          ) : (
            <ul className="divide-y divide-gray-100 dark:divide-gray-700">
              {events.map((event, index) => (
                <li key={index} className="p-4">
                  <div className="flex items-center justify-between">
                    <span className="font-mono text-sm font-medium text-indigo-700 dark:text-indigo-400">{event.type}</span>
                    <time className="text-xs text-gray-400 dark:text-gray-500">{event.receivedAt}</time>
                  </div>
                  <pre className="mt-2 overflow-x-auto rounded bg-gray-50 p-2 text-xs text-gray-600 dark:bg-gray-900 dark:text-gray-400">
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
