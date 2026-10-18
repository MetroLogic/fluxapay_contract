/**
 * Webhook domain types (Issues #808, #810).
 *
 * The event type list mirrors the table in `docs/webhooks.md`. Keeping it a
 * closed union rather than a free string is what lets the test endpoint
 * return `UnsupportedEventType` instead of cheerfully signing a typo and
 * leaving the merchant to wonder why nothing arrived.
 */

export const WEBHOOK_EVENT_TYPES = [
  "payment.created",
  "payment.pending",
  "payment.confirmed",
  "payment.partially_paid",
  "payment.overpaid",
  "payment.failed",
  "payment.cancelled",
  "refund.created",
  "refund.completed",
  "dispute.opened",
  "dispute.resolved",
] as const;

export type WebhookEventType = (typeof WEBHOOK_EVENT_TYPES)[number];

export function isWebhookEventType(value: string): value is WebhookEventType {
  return (WEBHOOK_EVENT_TYPES as readonly string[]).includes(value);
}

export type WebhookSigningAlgorithm = "hmac_sha256" | "ed25519";

export interface WebhookEndpoint {
  id: string;
  merchantId: string;
  url: string;
  signingSecret: string;
  eventTypes: string[];
  enabled: boolean;
  signingAlgorithm?: WebhookSigningAlgorithm;
}

export interface WebhookEnvelope {
  id: string;
  type: WebhookEventType;
  createdAt: string;
  /**
   * False for synthetic deliveries from the test endpoint. Merchants are told
   * in the docs to branch on this so a test cannot move real money.
   */
  livemode: boolean;
  data: Record<string, unknown>;
}

export interface DeliveryAttempt {
  endpointId: string;
  eventType: string;
  paymentId: string | null;
  attemptNumber: number;
  httpStatus: number | null;
  responseBody: string | null;
  durationMs: number;
  success: boolean;
  livemode: boolean;
}

export interface DeliveryLogRow {
  id: string;
  endpoint_id: string;
  event_type: string;
  payment_id: string | null;
  attempt_number: number;
  delivered_at: string;
  http_status: number | null;
  response_body: string | null;
  duration_ms: number | null;
  success: boolean;
  livemode: boolean;
}

/** Status values for a webhook DLQ entry (Issue #961). */
export type WebhookDLQStatus = "pending" | "permanently_failed";

/**
 * A row from the `webhook_dlq` table.
 *
 * Represents a failed webhook delivery that is awaiting automatic retry or
 * has been permanently failed after exhausting all attempts.
 */
export interface WebhookDLQItem {
  id: string;
  endpoint_id: string;
  merchant_id: string;
  event_type: string;
  payment_id: string | null;
  /** The full envelope that should be re-delivered. */
  payload: WebhookEnvelope;
  retry_count: number;
  next_retry_at: string | null;
  status: WebhookDLQStatus;
  last_error: string | null;
  created_at: string;
  updated_at: string;
}

/**
 * Input for enqueueing a failed delivery into the DLQ.
 *
 * Called by the delivery layer whenever deliverOnce returns success=false.
 */
export interface EnqueueDLQInput {
  endpointId: string;
  merchantId: string;
  eventType: string;
  paymentId: string | null;
  payload: WebhookEnvelope;
  error: string;
}
