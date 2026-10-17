"use client";

/**
 * Issue #1067: Invoice Dashboard Page
 *
 * Displays the merchant's invoice list with lifecycle status badges
 * (Pending, Paid, Overdue, Voided). Supports filtering by status.
 * Voiding an invoice calls DELETE /v1/invoices/{id} on the indexer API.
 */

import React, { useCallback, useEffect, useState } from "react";

// ------------------------------------------------------------------ types

type InvoiceStatus = "Pending" | "Paid" | "Overdue" | "Voided";

interface InvoiceRow {
  invoiceId: string;
  merchantId: string;
  totalAmount: string;
  status: string;
  dueDate: string | null;
  paidAt: string | null;
  voidedAt: string | null;
  linkedPaymentId: string | null;
  createdAt: string | null;
}

// ------------------------------------------------------------------ status badge

const STATUS_STYLES: Record<string, string> = {
  Pending: "bg-yellow-100 text-yellow-800 border border-yellow-300",
  Paid: "bg-green-100 text-greengreen-800 border border-green-300",
  Overdue: "bg-red-100 text-red-800 border border-red-300",
  Voided: "bg-gray-100 text-gray-500 border border-gray-300 line-through",
};

function StatusBadge({ status }: { status: string }) {
  const classes =
    STATUS_STYLES[status] ?? "bg-gray-100 text-gray-600 border border-gray-300";
  return (
    <span
      className={`inline-block rounded-full px-2 py-0.5 text-xs font-semibold ${classes}`}
    >
      {status}
    </span>
  );
}

// ------------------------------------------------------------------ skeleton

function SkeletonRow() {
  return (
    <tr className="animate-pulse">
      <td className="px-4 py-3">
        <div className="h-4 w-24 rounded bg-gray-200 dark:bg-gray-700" />
      </td>
      <td className="px-4 py-3">
        <div className="h-4 w-20 rounded bg-gray-200 dark:bg-gray-700" />
      </td>
      <td className="px-4 py-3">
        <div className="h-4 w-16 rounded bg-gray-200 dark:bg-gray-700" />
      </td>
      <td className="px-4 py-3">
        <div className="h-4 w-24 rounded bg-gray-200 dark:bg-gray-700" />
      </td>
      <td className="px-4 py-3">
        <div className="h-4 w-24 rounded bg-gray-200 dark:bg-gray-700" />
      </td>
      <td className="px-4 py-3">
        <div className="h-4 w-16 rounded bg-gray-200 dark:bg-gray-700" />
      </td>
    </tr>
  );
}

// ------------------------------------------------------------------ helpers

function formatAmount(raw: string | null): string {
  if (!raw) return "—";
  try {
    // Amounts are stored in smallest unit (7 decimals for USDC).
    const num = Number(BigInt(raw)) / 1e7;
    return num.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  } catch {
    return raw;
  }
}

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

// ------------------------------------------------------------------ page

const FILTER_OPTIONS: Array<{ label: string; value: string }> = [
  { label: "All", value: "" },
  { label: "Pending", value: "Pending" },
  { label: "Paid", value: "Paid" },
  { label: "Overdue", value: "Overdue" },
  { label: "Voided", value: "Voided" },
];

export default function InvoicesPage() {
  const [invoices, setInvoices] = useState<InvoiceRow[]>([]);
  const [statusFilter, setStatusFilter] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [voidingId, setVoidingId] = useState<string | null>(null);

  const apiUrl =
    process.env.NEXT_PUBLIC_INDEXER_URL ??
    process.env.NEXT_PUBLIC_API_URL ??
    "http://localhost:3001";

  const fetchInvoices = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const params = new URLSearchParams({ limit: "50", offset: "0" });
      if (statusFilter) params.set("status", statusFilter);

      const resp = await fetch(`${apiUrl}/v1/invoices?${params.toString()}`, {
        headers: { Accept: "application/json" },
      });
      if (!resp.ok) {
        throw new Error(`Failed to fetch invoices: ${resp.status} ${resp.statusText}`);
      }
      const data = await resp.json();
      setInvoices(data.invoices ?? []);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [apiUrl, statusFilter]);

  useEffect(() => {
    void fetchInvoices();
  }, [fetchInvoices]);

  const handleVoid = async (invoiceId: string) => {
    if (!confirm(`Void invoice ${invoiceId}? This cannot be undone.`)) return;
    setVoidingId(invoiceId);
    try {
      const token =
        typeof window !== "undefined" ? localStorage.getItem("fluxapay_jwt") : null;

      const resp = await fetch(`${apiUrl}/v1/invoices/${invoiceId}`, {
        method: "DELETE",
        headers: {
          "Content-Type": "application/json",
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
        },
      });
      if (!resp.ok) {
        const body = await resp.json().catch(() => ({}));
        throw new Error(body.error ?? `HTTP ${resp.status}`);
      }
      // Optimistically update status in local state.
      setInvoices((prev) =>
        prev.map((inv) =>
          inv.invoiceId === invoiceId ? { ...inv, status: "Voided" } : inv,
        ),
      );
    } catch (err: unknown) {
      alert(`Failed to void invoice: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setVoidingId(null);
    }
  };

  return (
    <main className="min-h-screen bg-gray-50 p-6">
      <div className="mx-auto max-w-5xl">
        {/* Header */}
        <div className="mb-6 flex items-center justify-between">
          <h1 className="text-2xl font-bold text-gray-900">Invoices</h1>
          <div className="flex items-center gap-3">
            <a href="/payment-links" className="text-sm font-medium text-indigo-700 hover:text-indigo-900">
              Payment links
            </a>
            <a href="/subscriptions" className="text-sm font-medium text-indigo-700 hover:text-indigo-900">
              Subscriptions
            </a>
            <button
              onClick={() => void fetchInvoices()}
              disabled={loading}
              className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
            >
              {loading ? "Loading…" : "Refresh"}
            </button>
          </div>
        </div>

        {/* Status filter tabs */}
        <div className="mb-4 flex gap-2" role="tablist" aria-label="Filter by status">
          {FILTER_OPTIONS.map((opt) => (
            <button
              key={opt.value}
              role="tab"
              aria-selected={statusFilter === opt.value}
              onClick={() => setStatusFilter(opt.value)}
              className={`rounded-full px-3 py-1 text-sm font-medium transition-colors ${
                statusFilter === opt.value
                  ? "bg-indigo-600 text-white"
                  : "bg-white text-gray-600 border border-gray-300 hover:bg-gray-100"
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
            className="mb-4 rounded-md border border-red-300 bg-red-50 p-3 text-sm text-red-700"
          >
            {error}
          </div>
        )}

        {/* Table */}
        <div className="overflow-hidden rounded-lg border border-gray-200 bg-white shadow-sm">
          <table
            className="min-w-full divide-y divide-gray-200 text-sm"
            aria-busy={loading}
          >
            <thead className="bg-gray-50">
              <tr>
                {["Invoice ID", "Amount (USDC)", "Status", "Due Date", "Created", "Actions"].map(
                  (col) => (
                    <th
                      key={col}
                      scope="col"
                      className="px-4 py-3 text-left text-xs font-semibold uppercase tracking-wide text-gray-500"
                    >
                      {col}
                    </th>
                  ),
                )}
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {loading ? (
                Array.from({ length: 5 }).map((_, i) => <SkeletonRow key={i} />)
              ) : invoices.length === 0 ? (
                <tr>
                  <td colSpan={6} className="py-8 text-center text-gray-400">
                    No invoices found.
                  </td>
                </tr>
              ) : (
                invoices.map((inv) => (
                  <tr key={inv.invoiceId} className="hover:bg-gray-50">
                    <td className="px-4 py-3 font-mono text-xs text-gray-700">
                      {inv.invoiceId}
                    </td>
                    <td className="px-4 py-3 text-gray-900">
                      {formatAmount(inv.totalAmount)}
                    </td>
                    <td className="px-4 py-3">
                      <StatusBadge status={inv.status} />
                    </td>
                    <td className="px-4 py-3 text-gray-600">
                      {formatDate(inv.dueDate)}
                    </td>
                    <td className="px-4 py-3 text-gray-600">
                      {formatDate(inv.createdAt)}
                    </td>
                    <td className="px-4 py-3">
                      {inv.status !== "Voided" && inv.status !== "Paid" ? (
                        <button
                          onClick={() => void handleVoid(inv.invoiceId)}
                          disabled={voidingId === inv.invoiceId}
                          className="text-xs font-medium text-red-600 hover:text-red-800 disabled:opacity-50"
                        >
                          {voidingId === inv.invoiceId ? "Voiding…" : "Void"}
                        </button>
                      ) : (
                        <span className="text-xs text-gray-400">—</span>
                      )}
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
