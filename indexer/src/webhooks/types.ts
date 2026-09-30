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
  /**
   * The delivery ID (`whd_`-prefixed). This is the idempotency key
   * merchants deduplicate on. It is stable across retries of the same
   * event, so a retried delivery carries the same value and can be
   * collapsed with the original.
   */
  id: string;
  /**
   * The event ID—a stable identifier for the event itself, independent of
   * how many times it is delivered. Merchants deduplicate on this field
   * when a retried delivery arrives at their endpoint more than once.
   */
  event_id: string;
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
