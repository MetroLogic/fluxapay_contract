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
import { Keypair, TransactionBuilder } from "stellar-sdk";

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

export interface AnchorSEP10Credential {
  secretKey: string;
  webAuthEndpoint?: string;
  networkPassphrase?: string;
}

export interface AnchorSEP10Options {
  secretKey?: string;
  webAuthEndpoint?: string;
  networkPassphrase?: string;
  credentials?: Record<string, AnchorSEP10Credential>;
}

interface CachedAnchorToken {
  token: string;
  expiresAtSeconds: number;
}

interface AnchorAuthResult {
  token: string;
  expiresAtSeconds?: number;
}

/**
 * Minimal HTTP client built on Node's core `http`/`https` modules (rather
 * than global `fetch`) so that tests can reliably intercept requests with
 * `nock`.
 */
function httpRequest(
  method: "GET" | "POST",
  url: string,
  body?: unknown,
  extraHeaders: Record<string, string> = {}
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
          ...extraHeaders,
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
  private sep10: AnchorSEP10Options;
  private tokenCache = new Map<string, CachedAnchorToken>();
  private nowSeconds: () => number;

  constructor(
    store: SettlementStore,
    options: {
      pollIntervalMs?: number;
      maxRetries?: number;
      sep10?: AnchorSEP10Options;
      anchorSep10?: AnchorSEP10Options;
      anchorCredentials?: Record<string, AnchorSEP10Credential>;
      nowSeconds?: () => number;
    } = {}
  ) {
    this.store = store;
    this.pollIntervalMs = options.pollIntervalMs ?? POLL_INTERVAL_MS;
    this.maxRetries = options.maxRetries ?? POLL_MAX_RETRIES;
    this.nowSeconds = options.nowSeconds ?? (() => Math.floor(Date.now() / 1000));

    let envCredentials: Record<string, AnchorSEP10Credential> = {};
    if (process.env.ANCHOR_SEP10_CREDENTIALS) {
      try {
        envCredentials = JSON.parse(process.env.ANCHOR_SEP10_CREDENTIALS);
      } catch {
        console.error("ANCHOR_SEP10_CREDENTIALS is invalid JSON; anchor authentication is disabled");
        envCredentials = {};
      }
    }

    this.sep10 = {
      ...(options.sep10 ?? {}),
      ...(options.anchorSep10 ?? {}),
      secretKey:
        options.sep10?.secretKey ??
        options.anchorSep10?.secretKey ??
        process.env.ANCHOR_SEP10_SECRET_KEY ??
        process.env.STELLAR_SECRET_KEY,
      webAuthEndpoint:
        options.sep10?.webAuthEndpoint ??
        options.anchorSep10?.webAuthEndpoint ??
        process.env.ANCHOR_SEP10_WEB_AUTH_ENDPOINT,
      networkPassphrase:
        options.sep10?.networkPassphrase ??
        options.anchorSep10?.networkPassphrase ??
        process.env.ANCHOR_SEP10_NETWORK_PASSPHRASE,
      credentials: {
        ...envCredentials,
        ...(options.anchorCredentials ?? {}),
        ...(options.sep10?.credentials ?? {}),
        ...(options.anchorSep10?.credentials ?? {}),
      },
    };
  }

  onSettlementEvent(listener: SettlementEventListener): void {
    this.listeners.push(listener);
  }

  private emit(event: SettlementEvent): void {
    for (const listener of this.listeners) {
      listener(event);
    }
  }

  private normalizeAnchorDomain(domain: string): string {
    return domain.trim().toLowerCase().replace(/^https?:\/\//, "").replace(/\/+$/, "");
  }

  private credentialFor(domain: string): AnchorSEP10Credential | null {
    const normalized = this.normalizeAnchorDomain(domain);
    const perAnchor = this.sep10.credentials?.[normalized]
      ?? this.sep10.credentials?.[domain]
      ?? Object.entries(this.sep10.credentials ?? {}).find(
        ([key]) => this.normalizeAnchorDomain(key) === normalized,
      )?.[1];
    if (perAnchor?.secretKey) return perAnchor;

    if (this.sep10.secretKey) {
      return {
        secretKey: this.sep10.secretKey,
        webAuthEndpoint: this.sep10.webAuthEndpoint,
        networkPassphrase: this.sep10.networkPassphrase,
      };
    }
    return null;
  }

  private tokenExpiry(token: string): number {
    try {
      const payload = token.split(".")[1];
      if (!payload) return this.nowSeconds() + 300;
      const claims = JSON.parse(Buffer.from(payload, "base64url").toString("utf8"));
      return typeof claims.exp === "number" ? claims.exp : this.nowSeconds() + 300;
    } catch {
      return this.nowSeconds() + 300;
    }
  }

  private async authenticateAnchor(
    anchorDomain: string,
    credential: AnchorSEP10Credential,
  ): Promise<AnchorAuthResult> {
    const keypair = Keypair.fromSecret(credential.secretKey);
    const webAuthEndpoint = credential.webAuthEndpoint
      ?? this.sep10.webAuthEndpoint
      ?? `https://${this.normalizeAnchorDomain(anchorDomain)}/auth`;

    const challengeUrl = new URL(webAuthEndpoint);
    challengeUrl.searchParams.set("account", keypair.publicKey());
    challengeUrl.searchParams.set("home_domain", this.normalizeAnchorDomain(anchorDomain));

    const challengeResponse = await httpRequest("GET", challengeUrl.toString());
    if (challengeResponse.status < 200 || challengeResponse.status >= 300) {
      throw new Error("SEP-10 challenge request was rejected");
    }

    const challengeBody = challengeResponse.json as {
      transaction?: string;
      challenge_transaction?: string;
      network_passphrase?: string;
    } | null;
    const challenge = challengeBody?.transaction ?? challengeBody?.challenge_transaction;
    if (!challenge) throw new Error("SEP-10 challenge response missing transaction");

    const networkPassphrase = credential.networkPassphrase
      ?? this.sep10.networkPassphrase
      ?? challengeBody?.network_passphrase
      ?? process.env.STELLAR_NETWORK_PASSPHRASE;
    if (!networkPassphrase) {
      throw new Error("SEP-10 network passphrase is not configured");
    }

    let signedTransaction: string;
    try {
      const transaction = TransactionBuilder.fromXDR(challenge, networkPassphrase);
      transaction.sign(keypair);
      signedTransaction = transaction.toXDR();
    } catch {
      throw new Error("SEP-10 challenge transaction could not be signed");
    }

    const tokenResponse = await httpRequest("POST", webAuthEndpoint, {
      transaction: signedTransaction,
    });
    if (tokenResponse.status < 200 || tokenResponse.status >= 300) {
      throw new Error("SEP-10 credential authentication was rejected");
    }

    const tokenBody = tokenResponse.json as {
      token?: string;
      jwt?: string;
      expires_in?: number;
      expires_at?: number | string;
    } | null;
    const token = tokenBody?.token ?? tokenBody?.jwt;
    if (!token) throw new Error("SEP-10 authentication response missing token");

    let expiresAtSeconds: number | undefined;
    if (typeof tokenBody?.expires_in === "number") {
      expiresAtSeconds = this.nowSeconds() + tokenBody.expires_in;
    } else if (typeof tokenBody?.expires_at === "number") {
      expiresAtSeconds = tokenBody.expires_at;
    } else if (typeof tokenBody?.expires_at === "string") {
      const parsed = Date.parse(tokenBody.expires_at);
      if (!Number.isNaN(parsed)) expiresAtSeconds = Math.floor(parsed / 1000);
    }
    return { token, expiresAtSeconds };
  }

  private async getAnchorToken(anchorDomain: string): Promise<string | null> {
    const normalized = this.normalizeAnchorDomain(anchorDomain);
    const cached = this.tokenCache.get(normalized);
    if (cached && cached.expiresAtSeconds - 60 > this.nowSeconds()) {
      return cached.token;
    }

    const credential = this.credentialFor(anchorDomain);
    if (!credential) return null;

    const auth = await this.authenticateAnchor(anchorDomain, credential);
    this.tokenCache.set(normalized, {
      token: auth.token,
      expiresAtSeconds: auth.expiresAtSeconds ?? this.tokenExpiry(auth.token),
    });
    return auth.token;
  }

  private invalidateAnchorToken(anchorDomain: string): void {
    this.tokenCache.delete(this.normalizeAnchorDomain(anchorDomain));
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

      const finalStatus = await this.pollUntilTerminal(endpoint, anchorTxnId, payload.anchorDomain);

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
    const body = {
      asset_code: "USDC",
      amount: String(payload.amount),
      account: payload.payoutAddress,
      memo: payload.paymentId,
      memo_type: "text",
    };

    let token = await this.getAnchorToken(payload.anchorDomain);
    let response = await httpRequest(
      "POST",
      url,
      token ? { ...body, jwt: token } : body,
      token ? { Authorization: `Bearer ${token}` } : {},
    );

    if ((response.status === 401 || response.status === 403) && token) {
      this.invalidateAnchorToken(payload.anchorDomain);
      token = await this.getAnchorToken(payload.anchorDomain);
      if (token) {
        response = await httpRequest(
          "POST",
          url,
          { ...body, jwt: token },
          { Authorization: `Bearer ${token}` },
        );
      }
    }

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
    anchorTxnId: string,
    anchorDomain: string,
  ): Promise<string> {
    const url = `${endpoint.replace(/\/+$/, "")}/transaction?id=${encodeURIComponent(
      anchorTxnId
    )}`;

    for (let attempt = 0; attempt < this.maxRetries; attempt++) {
      let token = await this.getAnchorToken(anchorDomain);
      let response = await httpRequest(
        "GET",
        url,
        undefined,
        token ? { Authorization: `Bearer ${token}` } : {},
      );

      if ((response.status === 401 || response.status === 403) && token) {
        this.invalidateAnchorToken(anchorDomain);
        token = await this.getAnchorToken(anchorDomain);
        if (token) {
          response = await httpRequest(
            "GET",
            url,
            undefined,
            { Authorization: `Bearer ${token}` },
          );
        }
      }

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
