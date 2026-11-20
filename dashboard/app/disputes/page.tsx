"use client";

/**
 * Issue #1161: Disputes management page.
 *
 * Lists merchant disputes with status filtering (open / escalated / resolved).
 * Each row links to the detail view (`/disputes/[id]`) which already exists
 * and shows the live SSE event stream. The detail page also displays
 * evidence and supports a merchant response (evidence upload).
 *
 * Data source: the indexer's `GET /merchants/:merchantId/disputes?status=`
 * endpoint, proxied through the backend. The merchant ID is derived from the
 * SEP-10 JWT subject (stored in localStorage as `fluxapay_jwt`).
 */

import { useCallback, useEffect, useState } from "react";

// ------------------------------------------------------------------ types

type DisputeStatusFilter = "" | "Open" | "Escalated" | "Resolved";

interface DisputeRow {
  dispute_id: string;
  payment_id: string;
  disputer: string | null;
  merchant_id: string;
  amount: string | null;
  status: string;
  evidence: unknown[] | null;
  escalated: boolean | null;
  resolved_at: string | null;
  created_at: string | null;
}

// ------------------------------------------------------------------ status badge

const STATUS_STYLES: Record<string, string> = {
  Open:
    "bg-yellow-100 text-yellow-800 border border-yellow-300 dark:bg-yellow-900/40 dark:text-yellow-200 dark:border-yellow-700",
  UnderReview:
    "bg-blue-100 text-blue-800 border border-blue-300 dark:bg-blue-900/40 dark:text-blue-200 dark:border-blue-700",
  Resolved:
    "bg-green-100 text-green-800 border border-green-300 dark:bg-green-900/40 dark:text-green-200 dark:border-green-700",
  Rejected:
    "bg-gray-100 text-gray-500 border border-gray-300 dark:bg-gray-800 dark:text-gray-400 dark:border-gray-600",
};

function StatusBadge({ status }: { status: string }) {
  const classes =
    STATUS_STYLES[status] ??
    "bg-gray-100 text-gray-600 border border-gray-300 dark:bg-gray-800 dark:text-gray-300 dark:border-gray-600";
  return (
    <span
      className={`inline-block rounded-full px-2 py-0.5 text-xs font-semibold ${classes}`}
    >
      {status}
    </span>
  );
}

// ------------------------------------------------------------------ helpers

function formatDate(iso: string | null): string {
  if (!iso) return "—";
  try {
    return new Date(iso).toLocaleDateString("en-US", {
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  } catch {
    return iso;
  }
}

function formatAmount(raw: string | null): string {
  if (!raw) return "—";
  try {
    const num = Number(BigInt(raw)) / 1e7;
    return num.toLocaleString("en-US", {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    });
  } catch {
    return raw;
  }
}

/** Map the filter button values to the status param the indexer expects. */
function statusToParam(filter: DisputeStatusFilter): string {
  if (filter === "Escalated") return "Open";
  return filter;
}

// ------------------------------------------------------------------ page

const FILTER_OPTIONS: Array<{ label: string; value: DisputeStatusFilter }> = [
  { label: "All", value: "" },
  { label: "Open", value: "Open" },
  { label: "Escalated", value: "Escalated" },
  { label: "Resolved", value: "Resolved" },
];

export default function DisputesPage() {
  const [disputes, setDisputes] = useState<DisputeRow[]>([]);
  const [statusFilter, setStatusFilter] = useState<DisputeStatusFilter>("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const apiUrl =
    process.env.NEXT_PUBLIC_INDEXER_URL ??
    process.env.NEXT_PUBLIC_API_URL ??
    "http://localhost:3001";

  const fetchDisputes = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const token =
        typeof window !== "undefined"
          ? localStorage.getItem("fluxapay_jwt")
          : null;
      if (!token) {
        throw new Error("Connect your merchant wallet to view disputes.");
      }

      // Decode the JWT sub (merchant account) to use as merchantId.
      const payload = JSON.parse(
        atob(token.split(".")[1]),
      ) as { sub?: string; merchant_id?: string };
      const merchantId = payload.merchant_id ?? payload.sub;
      if (!merchantId) {
        throw new Error("Could not determine merchant ID from JWT.");
      }

      const params = new URLSearchParams();
      const statusParam = statusToParam(statusFilter);
      if (statusParam) params.set("status", statusParam);

      const resp = await fetch(
        `${apiUrl}/merchants/${encodeURIComponent(merchantId)}/disputes${params.toString() ? `?${params.toString()}` : ""}`,
        {
          headers: {
            Accept: "application/json",
            Authorization: `Bearer ${token}`,
          },
        },
      );
      if (!resp.ok) {
        throw new Error(
          `Failed to fetch disputes: ${resp.status} ${resp.statusText}`,
        );
      }
      const data = (await resp.json()) as { disputes?: DisputeRow[] };
      let rows = data.disputes ?? [];

      // Client-side filter for "Escalated" since the indexer status field is
      // "Open" for both open and escalated disputes; the `escalated` boolean
      // distinguishes them.
      if (statusFilter === "Escalated") {
        rows = rows.filter((d) => d.escalated === true);
      } else if (statusFilter === "Open") {
        rows = rows.filter((d) => d.escalated !== true);
      }

      setDisputes(rows);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [apiUrl, statusFilter]);

  useEffect(() => {
    void fetchDisputes();
  }, [fetchDisputes]);

  return (
    <main className="min-h-screen bg-gray-50 p-6 dark:bg-gray-900">
      <div className="mx-auto max-w-5xl">
        {/* Header */}
        <div className="mb-6 flex items-center justify-between">
          <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Disputes
          </h1>
          <div className="flex items-center gap-3">
            <a
              href="/invoices"
              className="text-sm font-medium text-indigo-700 hover:text-indigo-900 dark:text-indigo-400 dark:hover:text-indigo-300"
            >
              Invoices
            </a>
            <a
              href="/payment-links"
              className="text-sm font-medium text-indigo-700 hover:text-indigo-900 dark:text-indigo-400 dark:hover:text-indigo-300"
            >
              Payment links
            </a>
            <button
              type="button"
              onClick={() => void fetchDisputes()}
              disabled={loading}
              className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50 dark:bg-indigo-500 dark:hover:bg-indigo-400"
            >
              {loading ? "Loading…" : "Refresh"}
            </button>
          </div>
        </div>

        {/* Status filter tabs */}
        <div
          className="mb-4 flex gap-2"
          role="tablist"
          aria-label="Filter by status"
        >
          {FILTER_OPTIONS.map((opt) => (
            <button
              key={opt.value}
              role="tab"
              aria-selected={statusFilter === opt.value}
              onClick={() => setStatusFilter(opt.value)}
              className={`rounded-full px-3 py-1 text-sm font-medium transition-colors ${
                statusFilter === opt.value
                  ? "bg-indigo-600 text-white dark:bg-indigo-500"
                  : "bg-white text-gray-600 border border-gray-300 hover:bg-gray-100 dark:bg-gray-800 dark:text-gray-300 dark:border-gray-600 dark:hover:bg-gray-700"
              }`}
            >
              {opt.label}
            </button>
          ))}
        </div>

        {/* Error banner */}
        {error && (
          <div
            role="alert"
            className="mb-4 rounded-md border border-red-300 bg-red-50 p-3 text-sm text-red-700 dark:border-red-700 dark:bg-red-900/40 dark:text-red-200"
          >
            {error}
          </div>
        )}

        {/* Table */}
        <div className="overflow-hidden rounded-lg border border-gray-200 bg-white shadow-sm dark:border-gray-700 dark:bg-gray-800">
          <table className="min-w-full divide-y divide-gray-200 text-sm dark:divide-gray-700">
            <thead className="bg-gray-50 dark:bg-gray-900">
              <tr>
                {[
                  "Dispute ID",
                  "Payment ID",
                  "Amount (USDC)",
                  "Status",
                  "Evidence",
                  "Created",
                  "Actions",
                ].map((col) => (
                  <th
                    key={col}
                    scope="col"
                    className="px-4 py-3 text-left text-xs font-semibold uppercase tracking-wide text-gray-500 dark:text-gray-400"
                  >
                    {col}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100 dark:divide-gray-700">
              {loading && disputes.length === 0 ? (
                <tr>
                  <td
                    colSpan={7}
                    className="py-8 text-center text-gray-400 dark:text-gray-500"
                  >
                    Loading…
                  </td>
                </tr>
              ) : disputes.length === 0 ? (
                <tr>
                  <td
                    colSpan={7}
                    className="py-8 text-center text-gray-400 dark:text-gray-500"
                  >
                    No disputes found.
                  </td>
                </tr>
              ) : (
                disputes.map((d) => {
                  const evidenceCount = Array.isArray(d.evidence)
                    ? d.evidence.length
                    : 0;
                  return (
                    <tr
                      key={d.dispute_id}
                      className="hover:bg-gray-50 dark:hover:bg-gray-700/50"
                    >
                      <td className="px-4 py-3 font-mono text-xs text-gray-700 dark:text-gray-300">
                        {d.dispute_id}
                      </td>
                      <td className="px-4 py-3 font-mono text-xs text-gray-700 dark:text-gray-300">
                        {d.payment_id}
                      </td>
                      <td className="px-4 py-3 text-gray-900 dark:text-gray-100">
                        {formatAmount(d.amount)}
                      </td>
                      <td className="px-4 py-3">
                        <StatusBadge status={d.status} />
                        {d.escalated ? (
                          <span className="ml-1 text-xs text-amber-600 dark:text-amber-400">
                            ⚑
                          </span>
                        ) : null}
                      </td>
                      <td className="px-4 py-3 text-gray-600 dark:text-gray-400">
                        {evidenceCount > 0 ? `${evidenceCount} item(s)` : "—"}
                      </td>
                      <td className="px-4 py-3 text-gray-600 dark:text-gray-400">
                        {formatDate(d.created_at)}
                      </td>
                      <td className="px-4 py-3">
                        <a
                          href={`/disputes/${encodeURIComponent(d.dispute_id)}`}
                          className="text-sm font-medium text-indigo-600 hover:text-indigo-800 dark:text-indigo-400 dark:hover:text-indigo-300"
                        >
                          View →
                        </a>
                      </td>
                    </tr>
                  );
                })
              )}
            </tbody>
          </table>
        </div>
      </div>
    </main>
  );
}
