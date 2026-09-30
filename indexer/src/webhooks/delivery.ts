import { randomUUID } from "node:crypto";
import {
  buildSignatureHeader,
  DELIVERY_ID_HEADER,
  EVENT_ID_HEADER,
  EVENT_TYPE_HEADER,
  SIGNATURE_HEADER,
  TIMESTAMP_HEADER,
} from "./signing";
import type { DeliveryAttempt, WebhookEndpoint, WebhookEnvelope } from "./types";

/**
 * Webhook delivery (Issues #808, #810).
 *
 * Every attempt — success or failure — is recorded, which is the whole point
 * of #810: a merchant debugging an integration currently has no way to see
 * whether anything was sent, what came back, or how many times it was tried.
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

export function truncateResponseBody(
  body: string | null,
  maxBytes: number = MAX_RESPONSE_BODY_BYTES,
): string | null {
  if (body === null) return null;

  const buf = Buffer.from(body, "utf8");
  if (buf.byteLength <= maxBytes) return body;

  // Slice on a byte boundary, then drop any trailing partial UTF-8 sequence
  // rather than storing a replacement character mid-word.
  return buf.subarray(0, maxBytes).toString("utf8").replace(/¿+$/, "");
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
        // The idempotency key is also a header so a merchant can dedupe
        // without parsing the body. The body carries the same value as
        // `event_id` for clients that only look at the payload.
        [EVENT_ID_HEADER]: envelope.eventId,
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
  eventId: string = `evt_${randomUUID()}`,
): WebhookEnvelope {
  return {
    id: `whd_${randomUUID()}`,
    eventId,
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
