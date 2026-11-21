/**
 * FluxaPay Settlement Service
 *
 * Implements the off-chain half of the SEP-6 / SEP-24 anchor integration
 * described in `docs/sep6-sep24-anchor-integration.md`.
 *
 * On-chain, `PaymentProcessor::settle_payment` already emits a
 * `(PAYMENT, ANCHOR_WITHDRAW, merchant_id, anchor_domain)` event once USDC
 * has been transferred to the merchant's payout address (see
 * `fluxapay/src/lib.rs`). This module is the off-chain consumer of that
 * event: it calls the merchant's configured SEP-6 transfer server to move
 * the USDC the rest of the way to fiat in the merchant's bank account.
 *
 * Flow:
 *   1. PaymentProcessor emits PAYMENT/ANCHOR_WITHDRAW on settlement.
 *   2. The indexer (see `index.ts`) decodes the event and calls
 *      `SettlementService.settle(payload)`.
 * 3. `settle()` starts SEP-6 when configured; an explicit `incomplete`
 *    response (or no SEP-6 endpoint) starts a SEP-24 interactive withdrawal.
 * 4. Persists the SEP-24 session and exposes its URL through the merchant
 *    settlement-status endpoint.
 * 5. Resumes anchor status polling when the merchant polls after the UI
 *    handoff. Only authoritative terminal anchor statuses complete/fail it.
 *   6. Persists progress in the `settlements` table (via `Database`) and
 *      emits `SETTLEMENT/FIAT_SENT` or `SETTLEMENT/FIAT_FAILED` to any
 *      registered listeners (e.g. a webhook notifier wired up by the host
 *      service).
 *
 * The flow is idempotent on `payment_id`: a re-delivered event reuses a
 * persisted SEP-24 session and short-circuits once a terminal state is reached.
 */

import * as http from "http";
import * as https from "https";
import { URL } from "url";

/** Decoded payload for a PAYMENT/ANCHOR_WITHDRAW contract event. */
export interface AnchorWithdrawPayload {
  paymentId: string;
  merchantId: string;
  amount: number;
  currency: string;
  payoutAddress: string;
  anchorDomain: string;
  sep6Endpoint: string;
  sep24Endpoint: string;
  supportedCurrencies: string[];
  ledgerTimestamp: number;
}

export type SettlementStatus = "pending" | "awaiting_user" | "fiat_sent" | "failed";

export interface SettlementRecord {
  paymentId: string;
  merchantId: string;
  anchorDomain: string;
  anchorTxnId: string | null;
  sep24Endpoint?: string | null;
  interactiveUrl?: string | null;
  amount: number;
  currency: string;
  status: SettlementStatus;
}

export interface SettlementEvent {
  type: "SETTLEMENT/FIAT_SENT" | "SETTLEMENT/FIAT_FAILED";
  paymentId: string;
  anchorTxnId: string | null;
  status: SettlementStatus;
  reason?: string;
}

/** The subset of `Database` the settlement service depends on. */
export interface SettlementStore {
  getSettlement(paymentId: string): Promise<SettlementRecord | null>;
  upsertSettlement(record: SettlementRecord): Promise<void>;
}

type SettlementEventListener = (event: SettlementEvent) => void;

export const POLL_MAX_RETRIES = 10;
export const POLL_INTERVAL_MS = 30_000;

const TERMINAL_SUCCESS = new Set(["completed"]);
const TERMINAL_FAILURE = new Set([
  "cancelled",
  "error",
  "expired",
  "no_market",
  "refunded",
  "too_large",
  "too_small",
]);

interface HttpResponse {
  status: number;
  json: unknown;
}

/**
 * Minimal HTTP client built on Node's core `http`/`https` modules (rather
 * than global `fetch`) so that tests can reliably intercept requests with
 * `nock`.
 */
function httpRequest(
  method: "GET" | "POST",
  url: string,
  body?: unknown
): Promise<HttpResponse> {
  return new Promise((resolve, reject) => {
    let parsed: URL;
    try {
      parsed = new URL(url);
    } catch (err) {
      reject(err as Error);
      return;
    }

    const lib = parsed.protocol === "https:" ? https : http;
    const payload = body !== undefined ? JSON.stringify(body) : undefined;

    const req = lib.request(
      {
        hostname: parsed.hostname,
        port: parsed.port || undefined,
        path: `${parsed.pathname}${parsed.search}`,
        method,
        headers: {
          "Content-Type": "application/json",
          ...(payload ? { "Content-Length": Buffer.byteLength(payload) } : {}),
        },
      },
      (res) => {
        let data = "";
        res.setEncoding("utf8");
        res.on("data", (chunk) => (data += chunk));
        res.on("end", () => {
          let json: unknown = null;
          if (data) {
            try {
              json = JSON.parse(data);
            } catch {
              json = null;
            }
          }
          resolve({ status: res.statusCode ?? 0, json });
        });
      }
    );

    req.on("error", reject);
    if (payload) req.write(payload);
    req.end();
  });
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export class SettlementService {
  private store: SettlementStore;
  private listeners: SettlementEventListener[] = [];
  private pollIntervalMs: number;
  private maxRetries: number;

  constructor(
    store: SettlementStore,
    options: { pollIntervalMs?: number; maxRetries?: number } = {}
  ) {
    this.store = store;
    this.pollIntervalMs = options.pollIntervalMs ?? POLL_INTERVAL_MS;
    this.maxRetries = options.maxRetries ?? POLL_MAX_RETRIES;
  }

  onSettlementEvent(listener: SettlementEventListener): void {
    this.listeners.push(listener);
  }

  private emit(event: SettlementEvent): void {
    for (const listener of this.listeners) {
      listener(event);
    }
  }

  /**
   * Entry point called by the indexer for every decoded
   * `PAYMENT/ANCHOR_WITHDRAW` event. Idempotent on `payment_id`.
   */
  async settle(payload: AnchorWithdrawPayload): Promise<void> {
    const existing = await this.store.getSettlement(payload.paymentId);
    if (existing && existing.status !== "pending") {
      if (existing.status !== "awaiting_user") {
        console.log(
          `Settlement for payment ${payload.paymentId} already ${existing.status}, skipping`
        );
        return;
      }
    }

    if (existing?.anchorTxnId && existing.sep24Endpoint) {
      await this.resume(payload.paymentId);
      return;
    }

    if (!payload.sep6Endpoint && !payload.sep24Endpoint) {
      console.warn(
        `No anchor endpoint configured for payment ${payload.paymentId}; skipping offramp`
      );
      return;
    }

    await this.store.upsertSettlement({
      paymentId: payload.paymentId,
      merchantId: payload.merchantId,
      anchorDomain: payload.anchorDomain,
      amount: payload.amount,
      currency: payload.currency,
      status: "pending",
      anchorTxnId: existing?.anchorTxnId ?? null,
    });

    try {
      if (!payload.sep6Endpoint) {
        await this.initiateInteractiveWithdrawal(payload.sep24Endpoint, payload);
        return;
      }

      const anchorTxnId = await this.initiateSep6Withdrawal(payload.sep6Endpoint, payload);

      await this.store.upsertSettlement({
        paymentId: payload.paymentId,
        merchantId: payload.merchantId,
        anchorDomain: payload.anchorDomain,
        amount: payload.amount,
        currency: payload.currency,
        status: "pending",
        anchorTxnId,
      });

      const finalStatus = await this.pollUntilTerminal(payload.sep6Endpoint, anchorTxnId);

      if (finalStatus === "incomplete" && payload.sep24Endpoint) {
        await this.initiateInteractiveWithdrawal(payload.sep24Endpoint, payload);
      } else if (finalStatus) {
        await this.applyAnchorStatus(
          {
            ...payload,
            anchorTxnId,
            sep24Endpoint: null,
            interactiveUrl: null,
            status: "pending",
          },
          finalStatus
        );
      }
    } catch (error) {
      console.error(
        `Settlement request failed for payment ${payload.paymentId}: ${
          error instanceof Error ? error.message : String(error)
        }`
      );
    }
  }

  /**
   * Polls a persisted SEP-24 transaction once when a merchant asks for its
   * settlement status after returning from the anchor UI.
   */
  async resume(paymentId: string): Promise<SettlementRecord | null> {
    const record = await this.store.getSettlement(paymentId);
    if (
      !record ||
      !record.anchorTxnId ||
      !record.sep24Endpoint ||
      (record.status !== "awaiting_user" && record.status !== "pending")
    ) {
      return record;
    }

    try {
      const status = await this.pollOnce(record.sep24Endpoint, record.anchorTxnId);
      if (status) await this.applyAnchorStatus(record, status);
    } catch (error) {
      console.error(
        `Settlement status check failed for payment ${paymentId}: ${
          error instanceof Error ? error.message : String(error)
        }`
      );
    }
    return this.store.getSettlement(paymentId);
  }

  private async applyAnchorStatus(
    record: SettlementRecord,
    status: string
  ): Promise<void> {
    if (!TERMINAL_SUCCESS.has(status) && !TERMINAL_FAILURE.has(status)) {
      await this.store.upsertSettlement({
        ...record,
        status:
          status === "incomplete" && record.interactiveUrl
            ? "awaiting_user"
            : "pending",
      });
      return;
    }

    const succeeded = TERMINAL_SUCCESS.has(status);
    const settlementStatus = succeeded ? "fiat_sent" : "failed";
    await this.store.upsertSettlement({ ...record, status: settlementStatus });
    this.emit({
      type: succeeded ? "SETTLEMENT/FIAT_SENT" : "SETTLEMENT/FIAT_FAILED",
      paymentId: record.paymentId,
      anchorTxnId: record.anchorTxnId,
      status: settlementStatus,
      ...(!succeeded ? { reason: `anchor reported terminal status: ${status}` } : {}),
    });
  }

  /** POST {sep6_endpoint}/transactions/withdrawal */
  private async initiateSep6Withdrawal(
    endpoint: string,
    payload: AnchorWithdrawPayload
  ): Promise<string> {
    const url = `${endpoint.replace(/\/+$/, "")}/transactions/withdrawal`;
    const response = await httpRequest("POST", url, {
      asset_code: "USDC",
      amount: String(payload.amount),
      account: payload.payoutAddress,
      memo: payload.paymentId,
      memo_type: "text",
    });

    if (response.status < 200 || response.status >= 300) {
      throw new Error(
        `SEP-6 withdrawal request failed with status ${response.status}`
      );
    }

    const body = response.json as { id?: string } | null;
    if (!body?.id) {
      throw new Error("SEP-6 withdrawal response missing transaction id");
    }
    return body.id;
  }

  /** POST {sep24_endpoint}/transactions/withdraw/interactive */
  private async initiateInteractiveWithdrawal(
    endpoint: string,
    payload: AnchorWithdrawPayload
  ): Promise<void> {
    const url = `${endpoint.replace(/\/+$/, "")}/transactions/withdraw/interactive`;
    const response = await httpRequest("POST", url, {
      asset_code: "USDC",
      amount: String(payload.amount),
      account: payload.payoutAddress,
      memo: payload.paymentId,
      memo_type: "text",
    });
    if (response.status < 200 || response.status >= 300) {
      throw new Error(
        `SEP-24 interactive withdrawal failed with status ${response.status}`
      );
    }

    const body = response.json as { id?: unknown; url?: unknown } | null;
    if (typeof body?.id !== "string" || !body.id.trim()) {
      throw new Error("SEP-24 response missing transaction id");
    }
    if (typeof body.url !== "string") {
      throw new Error("SEP-24 response missing interactive URL");
    }

    let interactiveUrl: URL;
    try {
      interactiveUrl = new URL(body.url);
    } catch {
      throw new Error("SEP-24 response contains an invalid interactive URL");
    }
    if (
      interactiveUrl.protocol !== "https:" ||
      interactiveUrl.username ||
      interactiveUrl.password
    ) {
      throw new Error("SEP-24 response contains an unsafe interactive URL");
    }

    await this.store.upsertSettlement({
      paymentId: payload.paymentId,
      merchantId: payload.merchantId,
      anchorDomain: payload.anchorDomain,
      anchorTxnId: body.id,
      sep24Endpoint: endpoint,
      interactiveUrl: interactiveUrl.toString(),
      amount: payload.amount,
      currency: payload.currency,
      status: "awaiting_user",
    });
  }

  /** GET {sep_endpoint}/transaction?id=... — return the anchor's status. */
  private async pollUntilTerminal(
    endpoint: string,
    anchorTxnId: string
  ): Promise<string | null> {
    const url = `${endpoint.replace(/\/+$/, "")}/transaction?id=${encodeURIComponent(
      anchorTxnId
    )}`;

    for (let attempt = 0; attempt < this.maxRetries; attempt++) {
      const response = await httpRequest("GET", url);
      if (response.status >= 200 && response.status < 300) {
        const body = response.json as { transaction?: { status?: string } } | null;
        const status = body?.transaction?.status;
        if (typeof status !== "string") {
          if (attempt < this.maxRetries - 1) await sleep(this.pollIntervalMs);
          continue;
        }
        if (
          status === "incomplete" ||
          TERMINAL_SUCCESS.has(status) ||
          TERMINAL_FAILURE.has(status)
        ) {
          return status;
        }
      }

      if (attempt < this.maxRetries - 1) {
        await sleep(this.pollIntervalMs);
      }
    }

    return null;
  }

  private async pollOnce(endpoint: string, anchorTxnId: string): Promise<string | null> {
    const url = `${endpoint.replace(/\/+$/, "")}/transaction?id=${encodeURIComponent(
      anchorTxnId
    )}`;
    const response = await httpRequest("GET", url);
    if (response.status < 200 || response.status >= 300) return null;
    const body = response.json as { transaction?: { status?: string } } | null;
    return typeof body?.transaction?.status === "string"
      ? body.transaction.status
      : null;
  }
}
