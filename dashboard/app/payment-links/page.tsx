"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import QRCode from "qrcode";

interface PaymentLinkAnalytics {
  link_id: string;
  shareable_url?: string;
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

function PaymentLinkModal({
  link,
  onClose,
}: {
  link: PaymentLinkAnalytics;
  onClose: () => void;
}) {
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const copyTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const shareableUrl = link.shareable_url;

  useEffect(() => {
    let cancelled = false;
    if (!shareableUrl) {
      setQrDataUrl(null);
      return;
    }
    QRCode.toDataURL(shareableUrl, { width: 512, margin: 2 })
      .then((url) => {
        if (!cancelled) setQrDataUrl(url);
      })
      .catch(() => {
        if (!cancelled) setQrDataUrl(null);
      });
    return () => {
      cancelled = true;
    };
  }, [shareableUrl]);

  useEffect(() => {
    return () => {
      if (copyTimer.current) clearTimeout(copyTimer.current);
    };
  }, []);

  const downloadPng = useCallback(async () => {
    if (!shareableUrl) return;
    const dataUrl = await QRCode.toDataURL(shareableUrl, { width: 512, margin: 2 });
    const anchor = document.createElement("a");
    anchor.href = dataUrl;
    anchor.download = `fluxapay-link-${link.link_id}.png`;
    anchor.click();
  }, [shareableUrl, link.link_id]);

  const downloadSvg = useCallback(async () => {
    if (!shareableUrl) return;
    const svg = await QRCode.toString(shareableUrl, { type: "svg", margin: 2 });
    const blob = new Blob([svg], { type: "image/svg+xml" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `fluxapay-link-${link.link_id}.svg`;
    anchor.click();
    URL.revokeObjectURL(url);
  }, [shareableUrl, link.link_id]);

  const copyLink = useCallback(async () => {
    if (!shareableUrl) return;
    try {
      await navigator.clipboard.writeText(shareableUrl);
      setCopied(true);
      if (copyTimer.current) clearTimeout(copyTimer.current);
      copyTimer.current = setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
    }
  }, [shareableUrl]);

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={`Payment link ${link.link_id}`}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      onClick={onClose}
    >
      <div
        className="w-full max-w-md rounded-lg bg-white p-6 shadow-xl"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-lg font-semibold text-gray-900">Payment link</h2>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="rounded-md px-2 py-1 text-gray-500 hover:bg-gray-100 hover:text-gray-700"
          >
            ✕
          </button>
        </div>

        <p className="mb-4 break-all font-mono text-xs text-gray-700">{link.link_id}</p>

        {shareableUrl ? (
          <div className="flex flex-col items-center gap-4">
            {qrDataUrl ? (
              <img
                src={qrDataUrl}
                alt={`QR code for payment link ${link.link_id}`}
                tabIndex={0}
                className="h-48 w-48 rounded-md border border-gray-200 focus:outline-none focus:ring-2 focus:ring-indigo-500"
              />
            ) : (
              <div className="flex h-48 w-48 items-center justify-center rounded-md border border-gray-200 text-sm text-gray-400">
                Generating QR…
              </div>
            )}

            <p className="w-full break-all rounded-md bg-gray-50 p-2 text-center font-mono text-xs text-gray-700">
              {shareableUrl}
            </p>

            <div className="flex flex-wrap justify-center gap-2">
              <button
                type="button"
                onClick={() => void downloadPng()}
                className="rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white hover:bg-indigo-700"
              >
                Download PNG
              </button>
              <button
                type="button"
                onClick={() => void downloadSvg()}
                className="rounded-md border border-indigo-600 px-3 py-2 text-sm font-medium text-indigo-700 hover:bg-indigo-50"
              >
                Download SVG
              </button>
              <button
                type="button"
                onClick={() => void copyLink()}
                className="rounded-md border border-gray-300 px-3 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50"
              >
                {copied ? "Copied!" : "Copy Link"}
              </button>
            </div>
          </div>
        ) : (
          <p className="text-sm text-gray-500">No shareable URL available for this link.</p>
        )}
      </div>
    </div>
  );
}

export default function PaymentLinksPage() {
  const [links, setLinks] = useState<PaymentLinkAnalytics[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activeLink, setActiveLink] = useState<PaymentLinkAnalytics | null>(null);
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
                {["Link", "Views", "Attempts", "Conversions", "Conversion rate", "Revenue (USDC)", "7-day trend", ""].map(
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
                <tr><td colSpan={8} className="py-8 text-center text-gray-400">Loading…</td></tr>
              ) : links.length === 0 ? (
                <tr><td colSpan={8} className="py-8 text-center text-gray-400">No payment links found.</td></tr>
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
                    <td className="px-4 py-3">
                      <button
                        type="button"
                        onClick={() => setActiveLink(link)}
                        className="rounded-md border border-gray-300 px-3 py-1 text-xs font-medium text-gray-700 hover:bg-gray-50"
                      >
                        Share
                      </button>
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </div>

      {activeLink && (
        <PaymentLinkModal link={activeLink} onClose={() => setActiveLink(null)} />
      )}
    </main>
  );
}
