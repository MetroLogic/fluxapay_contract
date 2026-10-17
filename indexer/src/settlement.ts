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
 *   3. `settle()` uses `sep6_endpoint` if present, else falls back to
 *      `sep24_endpoint`.
 *   4. POSTs the SEP-6 withdrawal request.
 *   5. Polls the anchor for transaction status, up to `POLL_MAX_RETRIES`
 *      times, `POLL_INTERVAL_MS` apart, before giving up and marking the
 *      settlement `failed`.
 *   6. Persists progress in the `settlements` table (via `Database`) and
 *      emits `SETTLEMENT/FIAT_SENT` or `SETTLEMENT/FIAT_FAILED` to any
 *      registered listeners (e.g. a webhook notifier wired up by the host
 *      service).
 *
 * The whole flow is idempotent on `payment_id`: a re-delivered event (e.g.
 * after an RPC reorg) short-circuits once a settlement has reached a
 * terminal state.
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

export type SettlementStatus = "pending" | "fiat_sent" | "failed";

export interface SettlementRecord {
  paymentId: string;
  merchantId: string;
  anchorDomain: string;
  anchorTxnId: string | null;
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
const TERMINAL_FAILURE = new Set(["error", "no_market", "too_small", "too_large"]);

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
      console.log(
        `Settlement for payment ${payload.paymentId} already ${existing.status}, skipping`
      );
      return;
    }

    // Prefer SEP-6 (no UI required); fall back to SEP-24's transfer server
    // only if no SEP-6 endpoint was configured for the merchant's anchor.
    const endpoint = payload.sep6Endpoint || payload.sep24Endpoint;
    if (!endpoint) {
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
      const anchorTxnId = await this.initiateWithdrawal(endpoint, payload);

      await this.store.upsertSettlement({
        paymentId: payload.paymentId,
        merchantId: payload.merchantId,
        anchorDomain: payload.anchorDomain,
        amount: payload.amount,
        currency: payload.currency,
        status: "pending",
        anchorTxnId,
      });

      const finalStatus = await this.pollUntilTerminal(endpoint, anchorTxnId);

      if (TERMINAL_SUCCESS.has(finalStatus)) {
        await this.store.upsertSettlement({
          paymentId: payload.paymentId,
          merchantId: payload.merchantId,
          anchorDomain: payload.anchorDomain,
          amount: payload.amount,
          currency: payload.currency,
          status: "fiat_sent",
          anchorTxnId,
        });
        this.emit({
          type: "SETTLEMENT/FIAT_SENT",
          paymentId: payload.paymentId,
          anchorTxnId,
          status: "fiat_sent",
        });
      } else {
        await this.markFailed(
          payload,
          anchorTxnId,
          `anchor reported terminal status: ${finalStatus}`
        );
      }
    } catch (error) {
      await this.markFailed(
        payload,
        existing?.anchorTxnId ?? null,
        error instanceof Error ? error.message : String(error)
      );
    }
  }

  private async markFailed(
    payload: AnchorWithdrawPayload,
    anchorTxnId: string | null,
    reason: string
  ): Promise<void> {
    console.error(`Settlement failed for payment ${payload.paymentId}: ${reason}`);
    await this.store.upsertSettlement({
      paymentId: payload.paymentId,
      merchantId: payload.merchantId,
      anchorDomain: payload.anchorDomain,
      amount: payload.amount,
      currency: payload.currency,
      status: "failed",
      anchorTxnId,
    });
    this.emit({
      type: "SETTLEMENT/FIAT_FAILED",
      paymentId: payload.paymentId,
      anchorTxnId,
      status: "failed",
      reason,
    });
  }

  /** POST {sep6_endpoint}/transactions/withdrawal */
  private async initiateWithdrawal(
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

  /** GET {sep6_endpoint}/transaction?id=... — poll until a terminal status. */
  private async pollUntilTerminal(
    endpoint: string,
    anchorTxnId: string
  ): Promise<string> {
    const url = `${endpoint.replace(/\/+$/, "")}/transaction?id=${encodeURIComponent(
      anchorTxnId
    )}`;

    for (let attempt = 0; attempt < this.maxRetries; attempt++) {
      const response = await httpRequest("GET", url);
      if (response.status >= 200 && response.status < 300) {
        const body = response.json as { transaction?: { status?: string } } | null;
        const status = body?.transaction?.status ?? "incomplete";
        if (TERMINAL_SUCCESS.has(status) || TERMINAL_FAILURE.has(status)) {
          return status;
        }
      }

      if (attempt < this.maxRetries - 1) {
        await sleep(this.pollIntervalMs);
      }
    }

    // Retries exhausted without reaching a terminal state — treat as failed.
    return "error";
  }
}
