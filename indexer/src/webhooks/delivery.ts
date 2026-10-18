import { randomUUID } from "node:crypto";
import {
  buildSignatureHeader,
  DELIVERY_ID_HEADER,
  EVENT_TYPE_HEADER,
  SIGNATURE_HEADER,
  TIMESTAMP_HEADER,
} from "./signing";
import type { DeliveryAttempt, WebhookEndpoint, WebhookEnvelope } from "./types";
import type { WebhookDLQStore } from "./dlq-store";
import type { WebhookStore } from "./store";

/**
 * Webhook delivery (Issues #808, #810, #961).
 *
 * Every attempt — success or failure — is recorded, which is the whole point
 * of #810: a merchant debugging an integration currently has no way to see
 * whether anything was sent, what came back, or how many times it was tried.
 *
 * Issue #961 adds a retry daemon that picks up DLQ items on an exponential
 * backoff schedule: immediately → 1 min → 5 min → 30 min → 2 hr → 8 hr → 24 hr.
 * After 7 failed attempts the item is marked permanently_failed and the
 * merchant notification hook is called.
 */

/**
 * Response bodies are truncated before storage.
 *
 * A broken endpoint commonly returns a full HTML error page. Storing those
 * whole would make the delivery log the largest table in the database within
 * a week of one misconfigured merchant, and the first kilobyte already
 * contains the status line and any JSON error a merchant would act on.
 */
export const MAX_RESPONSE_BODY_BYTES = 1024;

/** A delivery that hangs holds a connection; cap it well under the retry gap. */
export const DELIVERY_TIMEOUT_MS = 10_000;

/**
 * Default retry daemon poll interval (ms).
 *
 * Configurable via WEBHOOK_RETRY_INTERVAL_MS environment variable so
 * integration tests can drive the daemon faster.
 */
export const DEFAULT_RETRY_INTERVAL_MS = 60_000;

export function truncateResponseBody(
  body: string | null,
  maxBytes: number = MAX_RESPONSE_BODY_BYTES,
): string | null {
  if (body === null) return null;

  const buf = Buffer.from(body, "utf8");
  if (buf.byteLength <= maxBytes) return body;

  // Slice on a byte boundary, then drop any trailing partial UTF-8 sequence
  // rather than storing a replacement character mid-word.
  return buf.subarray(0, maxBytes).toString("utf8").replace(/\uFFFD+$/, "");
}

export interface DeliveryResult {
  attempt: DeliveryAttempt;
  /** The exact body that was signed, so a caller can echo it for debugging. */
  signedBody: string;
}

export type FetchLike = (
  url: string,
  init: {
    method: string;
    headers: Record<string, string>;
    body: string;
    signal?: AbortSignal;
  },
) => Promise<{ status: number; text: () => Promise<string> }>;

/**
 * Deliver one envelope to one endpoint and describe the attempt.
 *
 * Never throws for a delivery failure: a failed webhook is a normal outcome
 * that must be logged, not an exception that aborts the caller's loop over
 * the other endpoints.
 */
export async function deliverOnce(
  endpoint: WebhookEndpoint,
  envelope: WebhookEnvelope,
  attemptNumber: number,
  deps: { fetch: FetchLike; now?: () => number } = {
    fetch: globalThis.fetch as unknown as FetchLike,
  },
): Promise<DeliveryResult> {
  const now = deps.now ?? (() => Date.now());
  const body = JSON.stringify(envelope);
  const startedAt = now();

  const paymentId =
    typeof envelope.data.payment_id === "string" ? envelope.data.payment_id : null;

  const base: Omit<DeliveryAttempt, "httpStatus" | "responseBody" | "durationMs" | "success"> = {
    endpointId: endpoint.id,
    eventType: envelope.type,
    paymentId,
    attemptNumber,
    livemode: envelope.livemode,
  };

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), DELIVERY_TIMEOUT_MS);

  try {
    const response = await deps.fetch(endpoint.url, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        [SIGNATURE_HEADER]: buildSignatureHeader(
          endpoint.signingSecret,
          body,
          Math.floor(startedAt / 1000),
          endpoint.signingAlgorithm ?? "hmac_sha256",
        ),
        [TIMESTAMP_HEADER]: String(Math.floor(startedAt / 1000)),
        [EVENT_TYPE_HEADER]: envelope.type,
        [DELIVERY_ID_HEADER]: envelope.id,
      },
      body,
      signal: controller.signal,
    });

    const text = await response.text().catch(() => "");

    return {
      signedBody: body,
      attempt: {
        ...base,
        httpStatus: response.status,
        responseBody: truncateResponseBody(text),
        durationMs: now() - startedAt,
        // 2xx only. A 3xx is a misconfigured endpoint, not a delivery, and
        // counting it as success hides the redirect from the merchant.
        success: response.status >= 200 && response.status < 300,
      },
    };
  } catch (error) {
    return {
      signedBody: body,
      attempt: {
        ...base,
        httpStatus: null,
        responseBody: truncateResponseBody(
          error instanceof Error ? error.message : String(error),
        ),
        durationMs: now() - startedAt,
        success: false,
      },
    };
  } finally {
    clearTimeout(timer);
  }
}

/** Build a signed envelope for an event. */
export function buildEnvelope(
  type: WebhookEnvelope["type"],
  data: Record<string, unknown>,
  livemode: boolean,
  now: () => number = Date.now,
): WebhookEnvelope {
  return {
    id: `whd_${randomUUID()}`,
    type,
    createdAt: new Date(now()).toISOString(),
    livemode,
    data,
  };
}

/**
 * Synthetic payload for the test endpoint (Issue #808).
 *
 * Identifiers are prefixed `test_` so a merchant whose handler reaches a
 * database cannot mistake this for a real payment, even if they ignore
 * `livemode`.
 */
export function buildTestPayload(type: WebhookEnvelope["type"]): Record<string, unknown> {
  return {
    payment_id: "test_pay_000000000000",
    merchant_id: "test_merchant_000000",
    amount: "1000000",
    currency: "USDC",
    status: type.split(".")[1] ?? "confirmed",
    note: "Synthetic delivery from POST /v1/webhooks/test. No payment exists for this id.",
  };
}

// ── Retry daemon (Issue #961) ──────────────────────────────────────────────

/**
 * Dependencies injected into the retry daemon.
 *
 * Kept as a plain object so unit tests can stub the database and network
 * without standing up a real server.
 */
export interface RetryDaemonDeps {
  dlqStore: WebhookDLQStore;
  webhookStore: WebhookStore;
  fetch: FetchLike;
  /** Called when an item is permanently failed. Implementations should send
   *  a merchant notification (email, etc.). The daemon does not throw on
   *  notification failures — a notification glitch must not prevent the DLQ
   *  from moving forward. */
  onPermanentlyFailed?: (item: {
    id: string;
    merchantId: string;
    endpointId: string;
    eventType: string;
    retryCount: number;
    lastError: string | null;
  }) => Promise<void>;
  now?: () => number;
  logger?: { info: (msg: string) => void; error: (msg: string) => void };
}

export interface RetryDaemonHandle {
  stop: () => void;
  /** Run one sweep immediately (exposed for testing and manual operation). */
  runOnce: () => Promise<{ attempted: number; succeeded: number; failed: number }>;
}

/**
 * Process one DLQ item: look up its endpoint, attempt delivery, and record
 * the result.
 *
 * Returns true if the delivery succeeded.
 */
export async function attemptDLQDelivery(
  item: {
    id: string;
    endpoint_id: string;
    merchant_id: string;
    event_type: string;
    payment_id: string | null;
    payload: unknown;
    retry_count: number;
    last_error: string | null;
  },
  deps: RetryDaemonDeps,
): Promise<boolean> {
  const logger = deps.logger ?? console;

  const endpoint = await deps.webhookStore.getEndpoint(item.endpoint_id).catch(() => null);
  if (!endpoint) {
    // Endpoint deleted while the item was queued. Remove from DLQ.
    await deps.dlqStore.recordAttemptResult(item.id, true, null).catch(() => {});
    logger.info(`[webhook-retry] dlq_item=${item.id} endpoint_not_found, dropping`);
    return true;
  }

  if (!endpoint.enabled) {
    // Disabled endpoints are skipped but kept in the DLQ so the merchant can
    // re-enable and replay manually.
    logger.info(`[webhook-retry] dlq_item=${item.id} endpoint_disabled, skipping`);
    return false;
  }

  const envelope = item.payload as import("./types").WebhookEnvelope;
  const { attempt } = await deliverOnce(endpoint, envelope, item.retry_count + 1, {
    fetch: deps.fetch,
    now: deps.now,
  });

  // Always record in the delivery log so the merchant's history page shows
  // every attempt including daemon retries.
  await deps.webhookStore.recordAttempt(attempt).catch((err) => {
    logger.error(
      `[webhook-retry] failed to record attempt for dlq_item=${item.id}: ${
        err instanceof Error ? err.message : String(err)
      }`,
    );
  });

  const errorMsg = attempt.success
    ? null
    : attempt.httpStatus !== null
      ? `HTTP ${attempt.httpStatus}: ${attempt.responseBody ?? ""}`
      : (attempt.responseBody ?? "network error");

  const newStatus = await deps.dlqStore.recordAttemptResult(
    item.id,
    attempt.success,
    errorMsg,
  );

  if (newStatus === "permanently_failed") {
    logger.info(
      `[webhook-retry] dlq_item=${item.id} permanently_failed after ${item.retry_count + 1} attempts`,
    );
    if (deps.onPermanentlyFailed) {
      await deps.onPermanentlyFailed({
        id: item.id,
        merchantId: item.merchant_id,
        endpointId: item.endpoint_id,
        eventType: item.event_type,
        retryCount: item.retry_count + 1,
        lastError: errorMsg,
      }).catch((err) => {
        logger.error(
          `[webhook-retry] notification hook failed for dlq_item=${item.id}: ${
            err instanceof Error ? err.message : String(err)
          }`,
        );
      });
    }
  } else if (attempt.success) {
    logger.info(`[webhook-retry] dlq_item=${item.id} delivered successfully`);
  } else {
    logger.info(
      `[webhook-retry] dlq_item=${item.id} attempt_count=${item.retry_count + 1} failed, next_retry scheduled`,
    );
  }

  return attempt.success;
}

/**
 * Start the webhook retry daemon (Issue #961).
 *
 * Runs a sweep every `intervalMs` milliseconds. Each sweep fetches all DLQ
 * items whose next_retry_at window has arrived and attempts redelivery.
 *
 * The interval is configurable via WEBHOOK_RETRY_INTERVAL_MS so integration
 * tests can drive the daemon at a much shorter period without sleeping.
 */
export function startRetryDaemon(
  deps: RetryDaemonDeps,
  options: { intervalMs?: number } = {},
): RetryDaemonHandle {
  const intervalMs =
    options.intervalMs ??
    (parseInt(process.env.WEBHOOK_RETRY_INTERVAL_MS ?? "", 10) || DEFAULT_RETRY_INTERVAL_MS);

  const logger = deps.logger ?? console;

  const runOnce = async (): Promise<{
    attempted: number;
    succeeded: number;
    failed: number;
  }> => {
    let attempted = 0;
    let succeeded = 0;
    let failed = 0;

    try {
      const due = await deps.dlqStore.getDueDLQItems();
      for (const item of due) {
        attempted++;
        const ok = await attemptDLQDelivery(item, deps).catch((err) => {
          logger.error(
            `[webhook-retry] unhandled error for dlq_item=${item.id}: ${
              err instanceof Error ? err.message : String(err)
            }`,
          );
          return false;
        });
        if (ok) succeeded++;
        else failed++;
      }
    } catch (err) {
      logger.error(
        `[webhook-retry] sweep error: ${err instanceof Error ? err.message : String(err)}`,
      );
    }

    if (attempted > 0) {
      logger.info(
        `[webhook-retry] sweep complete: attempted=${attempted} succeeded=${succeeded} failed=${failed}`,
      );
    }

    return { attempted, succeeded, failed };
  };

  const timer = setInterval(() => void runOnce(), intervalMs);
  // A maintenance timer must not hold the process open.
  timer.unref?.();

  return {
    stop: () => clearInterval(timer),
    runOnce,
  };
}
