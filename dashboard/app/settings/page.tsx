"use client";

// Issue #1163: merchant settings — webhook signing key rotation, fee
// recipient, and API key lifecycle. All calls use the merchant's SEP-10 JWT.

import { useCallback, useEffect, useState } from "react";

interface ApiKey {
  id: string;
  label: string;
  masked_key: string;
  lifetime_days: number;
  created_at: string;
  expires_at: string;
}

interface Settings {
  webhook_signing_key: string;
  fee_recipient: string | null;
  api_keys: ApiKey[];
}

const EMPTY_SETTINGS: Settings = {
  webhook_signing_key: "",
  fee_recipient: null,
  api_keys: [],
};

export default function SettingsPage() {
  const apiUrl =
    process.env.NEXT_PUBLIC_INDEXER_URL ??
    process.env.NEXT_PUBLIC_API_URL ??
    "http://localhost:3001";
  const [settings, setSettings] = useState<Settings>(EMPTY_SETTINGS);
  const [feeRecipient, setFeeRecipient] = useState("");
  const [label, setLabel] = useState("");
  const [lifetimeDays, setLifetimeDays] = useState(30);
  const [newKey, setNewKey] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const authHeaders = useCallback((): HeadersInit => {
    const token =
      typeof window !== "undefined" ? localStorage.getItem("fluxapay_jwt") : null;
    if (!token) throw new Error("Connect your merchant wallet to manage settings.");
    return { Authorization: `Bearer ${token}`, "Content-Type": "application/json" };
  }, []);

  const loadSettings = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const response = await fetch(`${apiUrl}/v1/merchant/settings`, {
        headers: authHeaders(),
      });
      if (!response.ok) throw new Error(`Failed to load settings (${response.status})`);
      const body = (await response.json()) as Settings;
      setSettings(body);
      setFeeRecipient(body.fee_recipient ?? "");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoading(false);
    }
  }, [apiUrl, authHeaders]);

  useEffect(() => {
    void loadSettings();
  }, [loadSettings]);

  const rotateWebhookKey = async () => {
    if (!confirm("Rotate the webhook signing key? Update your webhook verifier.")) return;
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(`${apiUrl}/v1/merchant/settings/webhook-key/rotate`, {
        method: "POST",
        headers: authHeaders(),
      });
      if (!response.ok) throw new Error(`Rotation failed (${response.status})`);
      await loadSettings();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const saveFeeRecipient = async () => {
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(`${apiUrl}/v1/merchant/settings/fee-recipient`, {
        method: "PUT",
        headers: authHeaders(),
        body: JSON.stringify({ fee_recipient: feeRecipient }),
      });
      if (!response.ok) throw new Error(`Update failed (${response.status})`);
      await loadSettings();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const createApiKey = async () => {
    setBusy(true);
    setError(null);
    setNewKey(null);
    try {
      const response = await fetch(`${apiUrl}/v1/merchant/api-keys`, {
        method: "POST",
        headers: authHeaders(),
        body: JSON.stringify({ label, lifetime_days: lifetimeDays }),
      });
      if (!response.ok) throw new Error(`Create failed (${response.status})`);
      const body = (await response.json()) as { api_key: string };
      setNewKey(body.api_key);
      setLabel("");
      await loadSettings();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const revokeApiKey = async (key: ApiKey) => {
    if (!confirm(`Revoke API key "${key.label}"? This cannot be undone.`)) return;
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(
        `${apiUrl}/v1/merchant/api-keys/${encodeURIComponent(key.id)}`,
        { method: "DELETE", headers: authHeaders() },
      );
      if (!response.ok) throw new Error(`Revoke failed (${response.status})`);
      await loadSettings();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main className="min-h-screen bg-gray-50 p-6">
      <div className="mx-auto max-w-4xl">
        <header className="mb-6 flex flex-wrap items-center justify-between gap-4">
          <div>
            <h1 className="text-2xl font-bold text-gray-900">Merchant Settings</h1>
            <p className="mt-1 text-sm text-gray-600">
              Webhook signing key, fee recipient, and API keys.
            </p>
          </div>
          <a href="/settings/webhooks" className="text-sm font-medium text-indigo-700 hover:text-indigo-900">
            Webhook deliveries
          </a>
        </header>

        {error && (
          <p role="alert" className="mb-4 rounded-md border border-red-300 bg-red-50 p-3 text-sm text-red-700">
            {error}
          </p>
        )}

        <section className="mb-6 rounded-lg border border-gray-200 bg-white p-5 shadow-sm">
          <h2 className="text-lg font-semibold text-gray-900">Webhook signing key</h2>
          <p className="mt-1 text-xs text-gray-500">
            Rotations can also be submitted as sponsored meta-transactions
            (<code>META_TX_FUNCTIONS.rotateWebhookKey</code>).
          </p>
          <div className="mt-3 flex flex-wrap items-center gap-3">
            <code className="min-w-0 flex-1 break-all rounded bg-gray-50 p-2 text-xs">
              {loading ? "…" : settings.webhook_signing_key || "—"}
            </code>
            <button
              type="button"
              onClick={() => void rotateWebhookKey()}
              disabled={loading || busy}
              className="rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
            >
              Rotate key
            </button>
          </div>
        </section>

        <section className="mb-6 rounded-lg border border-gray-200 bg-white p-5 shadow-sm">
          <h2 className="text-lg font-semibold text-gray-900">Fee recipient</h2>
          <p className="mt-1 text-xs text-gray-500">
            Stellar address that receives platform fees (meta-transaction action
            <code> set_fee_recipient</code>).
          </p>
          <div className="mt-3 flex flex-wrap items-center gap-3">
            <input
              type="text"
              value={feeRecipient}
              onChange={(event) => setFeeRecipient(event.target.value)}
              placeholder="G…"
              className="min-w-0 flex-1 rounded-md border border-gray-300 px-3 py-2 text-sm"
            />
            <button
              type="button"
              onClick={() => void saveFeeRecipient()}
              disabled={loading || busy || !feeRecipient.trim()}
              className="rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
            >
              Save
            </button>
          </div>
        </section>

        <section className="rounded-lg border border-gray-200 bg-white p-5 shadow-sm">
          <h2 className="text-lg font-semibold text-gray-900">API keys</h2>
          <div className="mt-3 flex flex-wrap items-end gap-3">
            <label className="text-sm text-gray-600">
              Label
              <input
                type="text"
                value={label}
                onChange={(event) => setLabel(event.target.value)}
                className="mt-1 block rounded-md border border-gray-300 px-3 py-2 text-sm"
              />
            </label>
            <label className="text-sm text-gray-600">
              Lifetime (days)
              <input
                type="number"
                min={1}
                max={365}
                value={lifetimeDays}
                onChange={(event) => setLifetimeDays(Number(event.target.value))}
                className="mt-1 block w-28 rounded-md border border-gray-300 px-3 py-2 text-sm"
              />
            </label>
            <button
              type="button"
              onClick={() => void createApiKey()}
              disabled={loading || busy || !label.trim()}
              className="rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
            >
              Create key
            </button>
          </div>
          {newKey && (
            <p className="mt-3 break-all rounded-md border border-green-300 bg-green-50 p-3 text-xs text-green-800">
              Copy this key now — it will not be shown again: <code>{newKey}</code>
            </p>
          )}
          <div className="mt-4 overflow-x-auto">
            <table className="min-w-full divide-y divide-gray-200 text-sm">
              <thead className="bg-gray-50">
                <tr>
                  {["Label", "Key", "Lifetime", "Expires", "Actions"].map((heading) => (
                    <th key={heading} className="px-3 py-2 text-left text-xs font-semibold uppercase text-gray-500">
                      {heading}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100">
                {loading ? (
                  <tr><td colSpan={5} className="py-6 text-center text-gray-400">Loading…</td></tr>
                ) : settings.api_keys.length === 0 ? (
                  <tr><td colSpan={5} className="py-6 text-center text-gray-400">No API keys yet.</td></tr>
                ) : (
                  settings.api_keys.map((key) => (
                    <tr key={key.id}>
                      <td className="px-3 py-2">{key.label}</td>
                      <td className="px-3 py-2 font-mono text-xs">{key.masked_key}</td>
                      <td className="px-3 py-2 tabular-nums">{key.lifetime_days} days</td>
                      <td className="px-3 py-2 text-gray-600">
                        {new Date(key.expires_at).toLocaleDateString()}
                      </td>
                      <td className="px-3 py-2">
                        <button
                          type="button"
                          onClick={() => void revokeApiKey(key)}
                          disabled={busy}
                          className="text-sm font-medium text-red-600 hover:text-red-800 disabled:opacity-50"
                        >
                          Revoke
                        </button>
                      </td>
                    </tr>
                  ))
                )}
              </tbody>
            </table>
          </div>
        </section>
      </div>
    </main>
  );
}
