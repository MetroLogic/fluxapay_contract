/**
 * Webhook signing, delivery, retry daemon, and DLQ routes (Issues #808, #810, #961).
 *
 * The network and the database are both injected, so these exercise the
 * decisions — signing scheme, truncation, success classification, rate limit,
 * pagination, exponential backoff schedule, permanently_failed promotion,
 * and merchant replay — rather than the transport.
 */

import assert from "node:assert";
import { describe, it } from "node:test";

import {
  buildEnvelope,
  buildSignatureHeader,
  buildTestPayload,
  computeSignature,
  deliverOnce,
  MAX_RESPONSE_BODY_BYTES,
  parseSignatureHeader,
  signedPayload,
  truncateResponseBody,
  verifySignature,
  isWebhookEventType,
  startRetryDaemon,
  attemptDLQDelivery,
  DEFAULT_RETRY_INTERVAL_MS,
  type FetchLike,
  type WebhookEndpoint,
  type RetryDaemonDeps,
} from "../src/webhooks";

import {
  RETRY_DELAYS_MS,
  MAX_RETRY_ATTEMPTS,
  retryDelayMs,
  nextRetryAt,
  WebhookDLQStore,
} from "../src/webhooks/dlq-store";

import type { WebhookDLQItem, WebhookEnvelope } from "../src/webhooks/types";

// ── Helpers ───────────────────────────────────────────────────────────────

const ENDPOINT: WebhookEndpoint = {
  id: "wh_abc123",
  merchantId: "merchant_1",
  url: "https://merchant.example/hooks",
  signingSecret: "whsec_test_secret",
  eventTypes: [],
  enabled: true,
};

function okFetch(status = 200, body = "ok"): FetchLike {
  return async () => ({ status, text: async () => body });
}

function failingFetch(message = "ECONNREFUSED"): FetchLike {
  return async () => {
    throw new Error(message);
  };
}

/** Minimal in-memory DLQ store that satisfies the WebhookDLQStore interface. */
class InMemoryDLQStore {
  private items = new Map<string, WebhookDLQItem>();
  private seq = 0;

  async enqueue(input: {
    endpointId: string;
    merchantId: string;
    eventType: string;
    paymentId: string | null;
    payload: WebhookEnvelope;
    error: string;
  }): Promise<string> {
    const id = `dlq_${++this.seq}`;
    const now = new Date().toISOString();
    this.items.set(id, {
      id,
      endpoint_id: input.endpointId,
      merchant_id: input.merchantId,
      event_type: input.eventType,
      payment_id: input.paymentId,
      payload: input.payload,
      retry_count: 0,
      next_retry_at: null,
      status: "pending",
      last_error: input.error,
      created_at: now,
      updated_at: now,
    });
    return id;
  }

  async getDueDLQItems(): Promise<WebhookDLQItem[]> {
    const now = new Date();
    return [...this.items.values()].filter(
      (i) =>
        i.status === "pending" &&
        (i.next_retry_at === null || new Date(i.next_retry_at) <= now),
    );
  }

  async getById(id: string): Promise<WebhookDLQItem | null> {
    return this.items.get(id) ?? null;
  }

  async recordAttemptResult(
    id: string,
    succeeded: boolean,
    error: string | null,
  ): Promise<"pending" | "permanently_failed" | null> {
    const item = this.items.get(id);
    if (!item) return null;

    if (succeeded) {
      this.items.delete(id);
      return null;
    }

    const newCount = item.retry_count + 1;
    if (newCount >= MAX_RETRY_ATTEMPTS) {
      item.retry_count = newCount;
      item.status = "permanently_failed";
      item.last_error = error;
      item.updated_at = new Date().toISOString();
      return "permanently_failed";
    }

    const delay = RETRY_DELAYS_MS[newCount] ?? 60_000;
    item.retry_count = newCount;
    item.next_retry_at = delay > 0 ? new Date(Date.now() + delay).toISOString() : null;
    item.last_error = error;
    item.updated_at = new Date().toISOString();
    return "pending";
  }

  async listForMerchant(
    merchantId: string,
    options: { status?: string } = {},
  ): Promise<WebhookDLQItem[]> {
    return [...this.items.values()].filter(
      (i) =>
        i.merchant_id === merchantId &&
        (options.status === undefined || i.status === options.status),
    );
  }

  async requeueItem(id: string): Promise<boolean> {
    const item = this.items.get(id);
    if (!item) return false;
    item.status = "pending";
    item.next_retry_at = null;
    item.updated_at = new Date().toISOString();
    return true;
  }

  async requeueAllForMerchant(merchantId: string): Promise<number> {
    let count = 0;
    for (const item of this.items.values()) {
      if (item.merchant_id === merchantId) {
        item.status = "pending";
        item.next_retry_at = null;
        item.updated_at = new Date().toISOString();
        count++;
      }
    }
    return count;
  }
}

/** Minimal in-memory WebhookStore that satisfies the interface used by retry daemon. */
class InMemoryWebhookStore {
  private attempts: unknown[] = [];
  private endpointMap: Map<string, WebhookEndpoint>;

  constructor(endpoints: WebhookEndpoint[] = [ENDPOINT]) {
    this.endpointMap = new Map(endpoints.map((e) => [e.id, e]));
  }

  async getEndpoint(id: string): Promise<WebhookEndpoint | null> {
    return this.endpointMap.get(id) ?? null;
  }

  async recordAttempt(attempt: unknown): Promise<string> {
    this.attempts.push(attempt);
    return `del_${this.attempts.length}`;
  }

  getRecordedAttempts(): unknown[] {
    return this.attempts;
  }
}

// ── Signing tests ─────────────────────────────────────────────────────────

describe("webhook signing", () => {
  it("separates timestamp from body in the signed material", () => {
    // Without the separator, ("1", "23"+body) and ("12", "3"+body) produce
    // identical signed strings — a collision an attacker controls.
    assert.notStrictEqual(signedPayload(1, "23x"), signedPayload(12, "3x"));
  });

  it("round-trips a signature it produced", () => {
    const body = JSON.stringify({ hello: "world" });
    const header = buildSignatureHeader(ENDPOINT.signingSecret, body, 1_700_000_000);
    assert.strictEqual(
      verifySignature(ENDPOINT.signingSecret, header, body, 1_700_000_000),
      true,
    );
  });

  it("rejects a body that changed after signing", () => {
    const header = buildSignatureHeader(ENDPOINT.signingSecret, "{}", 1_700_000_000);
    assert.strictEqual(
      verifySignature(ENDPOINT.signingSecret, header, '{"a":1}', 1_700_000_000),
      false,
    );
  });

  it("rejects the wrong secret", () => {
    const body = "{}";
    const header = buildSignatureHeader(ENDPOINT.signingSecret, body, 1_700_000_000);
    assert.strictEqual(verifySignature("whsec_other", header, body, 1_700_000_000), false);
  });

  it("rejects a replayed delivery outside the tolerance", () => {
    // Binding the timestamp is what bounds a capture-replay window.
    const body = "{}";
    const header = buildSignatureHeader(ENDPOINT.signingSecret, body, 1_700_000_000);
    assert.strictEqual(
      verifySignature(ENDPOINT.signingSecret, header, body, 1_700_000_000 + 3600),
      false,
    );
  });

  it("rejects a malformed header rather than throwing", () => {
    assert.strictEqual(parseSignatureHeader("garbage"), null);
    assert.strictEqual(verifySignature("s", "garbage", "{}"), false);
  });

  it("produces a stable signature for the same inputs", () => {
    const a = computeSignature("s", 1, "b");
    const b = computeSignature("s", 1, "b");
    assert.strictEqual(a, b);
  });
});

// ── Response body truncation ──────────────────────────────────────────────

describe("response body truncation", () => {
  it("leaves a small body alone", () => {
    assert.strictEqual(truncateResponseBody("short"), "short");
  });

  it("caps a large body at the limit", () => {
    // A broken endpoint returns a full HTML error page; storing those whole
    // would make this the largest table in the database.
    const big = "x".repeat(MAX_RESPONSE_BODY_BYTES * 4);
    const out = truncateResponseBody(big);
    assert.ok(out !== null && Buffer.byteLength(out, "utf8") <= MAX_RESPONSE_BODY_BYTES);
  });

  it("does not leave a partial UTF-8 sequence", () => {
    const out = truncateResponseBody("é".repeat(MAX_RESPONSE_BODY_BYTES), 5);
    assert.ok(out !== null && !out.endsWith("\uFFFD"));
  });

  it("passes null through", () => {
    assert.strictEqual(truncateResponseBody(null), null);
  });
});

// ── Single delivery ───────────────────────────────────────────────────────

describe("delivery", () => {
  it("counts a 2xx as success", async () => {
    const { attempt } = await deliverOnce(
      ENDPOINT,
      buildEnvelope("payment.confirmed", { payment_id: "pay_1" }, true),
      1,
      { fetch: okFetch(200) },
    );
    assert.strictEqual(attempt.success, true);
    assert.strictEqual(attempt.httpStatus, 200);
    assert.strictEqual(attempt.paymentId, "pay_1");
  });

  it("does not count a 3xx as success", async () => {
    // A redirect is a misconfigured endpoint, not a delivery. Counting it
    // would hide the misconfiguration from the merchant.
    const { attempt } = await deliverOnce(
      ENDPOINT,
      buildEnvelope("payment.confirmed", {}, true),
      1,
      { fetch: okFetch(302) },
    );
    assert.strictEqual(attempt.success, false);
  });

  it("records a network failure instead of throwing", async () => {
    // A failed webhook is a normal outcome that must be logged, not an
    // exception that aborts the loop over the other endpoints.
    const { attempt } = await deliverOnce(
      ENDPOINT,
      buildEnvelope("payment.failed", {}, true),
      3,
      { fetch: failingFetch("ECONNREFUSED") },
    );
    assert.strictEqual(attempt.success, false);
    assert.strictEqual(attempt.httpStatus, null);
    assert.strictEqual(attempt.attemptNumber, 3);
    assert.match(String(attempt.responseBody), /ECONNREFUSED/);
  });

  it("signs the exact body it sends", async () => {
    let seenBody = "";
    let seenSignature = "";
    const capturing: FetchLike = async (_url, init) => {
      seenBody = init.body;
      seenSignature = init.headers["x-fluxapay-signature"];
      return { status: 200, text: async () => "" };
    };

    await deliverOnce(ENDPOINT, buildEnvelope("payment.created", {}, true), 1, {
      fetch: capturing,
    });

    assert.strictEqual(
      verifySignature(ENDPOINT.signingSecret, seenSignature, seenBody),
      true,
    );
  });
});

// ── Test payload (Issue #808) ─────────────────────────────────────────────

describe("test payload (Issue #808)", () => {
  it("is marked not livemode", () => {
    const envelope = buildEnvelope("payment.confirmed", buildTestPayload("payment.confirmed"), false);
    assert.strictEqual(envelope.livemode, false);
  });

  it("uses clearly fake identifiers", () => {
    // A handler that ignores `livemode` still cannot mistake this for real.
    const payload = buildTestPayload("payment.confirmed");
    assert.match(String(payload.payment_id), /^test_/);
    assert.match(String(payload.merchant_id), /^test_/);
  });

  it("recognises only known event types", () => {
    assert.strictEqual(isWebhookEventType("payment.confirmed"), true);
    assert.strictEqual(isWebhookEventType("payment.teleported"), false);
  });
});

// ── Retry schedule (Issue #961) ───────────────────────────────────────────

describe("retry schedule (Issue #961)", () => {
  it("exports the correct number of delay steps", () => {
    // 7 steps: immediately, 1 min, 5 min, 30 min, 2 hr, 8 hr, 24 hr
    assert.strictEqual(RETRY_DELAYS_MS.length, 7);
  });

  it("first delay is 0 (immediate retry)", () => {
    assert.strictEqual(RETRY_DELAYS_MS[0], 0);
  });

  it("follows the documented schedule", () => {
    const MINUTE = 60_000;
    const HOUR = 60 * MINUTE;
    assert.strictEqual(RETRY_DELAYS_MS[1], 1 * MINUTE, "attempt 1: 1 min");
    assert.strictEqual(RETRY_DELAYS_MS[2], 5 * MINUTE, "attempt 2: 5 min");
    assert.strictEqual(RETRY_DELAYS_MS[3], 30 * MINUTE, "attempt 3: 30 min");
    assert.strictEqual(RETRY_DELAYS_MS[4], 2 * HOUR, "attempt 4: 2 hr");
    assert.strictEqual(RETRY_DELAYS_MS[5], 8 * HOUR, "attempt 5: 8 hr");
    assert.strictEqual(RETRY_DELAYS_MS[6], 24 * HOUR, "attempt 6: 24 hr");
  });

  it("retryDelayMs returns 0 for attempt 0", () => {
    assert.strictEqual(retryDelayMs(0), 0);
  });

  it("retryDelayMs caps out-of-range attempts at 24 hr", () => {
    const max = 24 * 60 * 60_000;
    assert.ok(retryDelayMs(99) <= max, "out-of-range attempts must not exceed 24 hr cap");
  });

  it("nextRetryAt returns null for immediate (attempt 0)", () => {
    assert.strictEqual(nextRetryAt(0), null);
  });

  it("nextRetryAt schedules a future time for attempt >= 1", () => {
    const before = Date.now();
    const ts = nextRetryAt(1);
    assert.ok(ts !== null, "should return a Date");
    assert.ok(ts!.getTime() > before, "should be in the future");
    assert.ok(
      ts!.getTime() <= before + RETRY_DELAYS_MS[1] + 100,
      "should be close to 1 min from now",
    );
  });

  it("MAX_RETRY_ATTEMPTS is 7", () => {
    assert.strictEqual(MAX_RETRY_ATTEMPTS, 7);
  });
});

// ── DLQ store (in-memory) ─────────────────────────────────────────────────

describe("WebhookDLQStore (in-memory stub)", () => {
  it("enqueues a failed delivery and marks it pending", async () => {
    const store = new InMemoryDLQStore();
    const envelope = buildEnvelope("payment.failed", {}, true);
    const id = await store.enqueue({
      endpointId: "wh_abc123",
      merchantId: "merchant_1",
      eventType: "payment.failed",
      paymentId: null,
      payload: envelope,
      error: "HTTP 500",
    });
    assert.ok(id, "should return a non-empty id");

    const item = await store.getById(id);
    assert.ok(item, "item should exist");
    assert.strictEqual(item!.status, "pending");
    assert.strictEqual(item!.retry_count, 0);
  });

  it("getDueDLQItems returns items with null next_retry_at immediately", async () => {
    const store = new InMemoryDLQStore();
    const envelope = buildEnvelope("payment.failed", {}, true);
    await store.enqueue({
      endpointId: "wh_abc123",
      merchantId: "merchant_1",
      eventType: "payment.failed",
      paymentId: null,
      payload: envelope,
      error: "ECONNREFUSED",
    });
    const due = await store.getDueDLQItems();
    assert.strictEqual(due.length, 1);
  });

  it("recordAttemptResult removes item on success", async () => {
    const store = new InMemoryDLQStore();
    const envelope = buildEnvelope("payment.confirmed", {}, true);
    const id = await store.enqueue({
      endpointId: "wh_abc123",
      merchantId: "merchant_1",
      eventType: "payment.confirmed",
      paymentId: null,
      payload: envelope,
      error: "first failure",
    });
    const newStatus = await store.recordAttemptResult(id, true, null);
    assert.strictEqual(newStatus, null, "success returns null");
    assert.strictEqual(await store.getById(id), null, "row deleted on success");
  });

  it("increments retry_count and sets next_retry_at on failure", async () => {
    const store = new InMemoryDLQStore();
    const envelope = buildEnvelope("payment.confirmed", {}, true);
    const id = await store.enqueue({
      endpointId: "wh_abc123",
      merchantId: "merchant_1",
      eventType: "payment.confirmed",
      paymentId: null,
      payload: envelope,
      error: "initial",
    });
    const newStatus = await store.recordAttemptResult(id, false, "HTTP 503");
    assert.strictEqual(newStatus, "pending");
    const item = await store.getById(id);
    assert.strictEqual(item!.retry_count, 1);
    assert.ok(item!.next_retry_at !== null, "should have a next_retry_at");
    assert.strictEqual(item!.last_error, "HTTP 503");
  });

  it("promotes to permanently_failed after MAX_RETRY_ATTEMPTS failures", async () => {
    const store = new InMemoryDLQStore();
    const envelope = buildEnvelope("payment.confirmed", {}, true);
    const id = await store.enqueue({
      endpointId: "wh_abc123",
      merchantId: "merchant_1",
      eventType: "payment.confirmed",
      paymentId: null,
      payload: envelope,
      error: "initial",
    });

    let lastStatus: "pending" | "permanently_failed" | null = "pending";
    for (let i = 0; i < MAX_RETRY_ATTEMPTS; i++) {
      lastStatus = await store.recordAttemptResult(id, false, `error attempt ${i + 1}`);
    }
    assert.strictEqual(lastStatus, "permanently_failed");

    const item = await store.getById(id);
    assert.strictEqual(item!.status, "permanently_failed");
  });

  it("requeueItem resets status to pending with no next_retry_at", async () => {
    const store = new InMemoryDLQStore();
    const envelope = buildEnvelope("payment.failed", {}, true);
    const id = await store.enqueue({
      endpointId: "wh_abc123",
      merchantId: "merchant_1",
      eventType: "payment.failed",
      paymentId: null,
      payload: envelope,
      error: "initial",
    });
    // Fail it permanently
    for (let i = 0; i < MAX_RETRY_ATTEMPTS; i++) {
      await store.recordAttemptResult(id, false, "fail");
    }
    const item = await store.getById(id);
    assert.strictEqual(item!.status, "permanently_failed");

    await store.requeueItem(id);
    const requeued = await store.getById(id);
    assert.strictEqual(requeued!.status, "pending");
    assert.strictEqual(requeued!.next_retry_at, null);
  });

  it("requeueAllForMerchant resets all items for that merchant", async () => {
    const store = new InMemoryDLQStore();
    const envelope = buildEnvelope("payment.failed", {}, true);
    for (let i = 0; i < 3; i++) {
      const id = await store.enqueue({
        endpointId: "wh_abc123",
        merchantId: "merchant_1",
        eventType: "payment.failed",
        paymentId: null,
        payload: envelope,
        error: "err",
      });
      for (let j = 0; j < MAX_RETRY_ATTEMPTS; j++) {
        await store.recordAttemptResult(id, false, "fail");
      }
    }
    const count = await store.requeueAllForMerchant("merchant_1");
    assert.strictEqual(count, 3);
    const items = await store.listForMerchant("merchant_1");
    assert.ok(
      items.every((i) => i.status === "pending"),
      "all should be pending after requeue",
    );
  });
});

// ── Retry daemon (Issue #961) ─────────────────────────────────────────────

describe("retry daemon (Issue #961)", () => {
  it("processes due items and removes them on success", async () => {
    const dlqStore = new InMemoryDLQStore();
    const webhookStore = new InMemoryWebhookStore();

    const envelope = buildEnvelope("payment.confirmed", { payment_id: "pay_1" }, true);
    const id = await dlqStore.enqueue({
      endpointId: ENDPOINT.id,
      merchantId: ENDPOINT.merchantId,
      eventType: "payment.confirmed",
      paymentId: "pay_1",
      payload: envelope,
      error: "original failure",
    });

    const deps: RetryDaemonDeps = {
      dlqStore: dlqStore as unknown as WebhookDLQStore,
      webhookStore: webhookStore as any,
      fetch: okFetch(200),
    };

    const { attempted, succeeded, failed } = await startRetryDaemon(deps).runOnce();
    assert.strictEqual(attempted, 1);
    assert.strictEqual(succeeded, 1);
    assert.strictEqual(failed, 0);

    // Row deleted on success
    assert.strictEqual(await dlqStore.getById(id), null);
  });

  it("re-schedules an item on failure and does not delete it", async () => {
    const dlqStore = new InMemoryDLQStore();
    const webhookStore = new InMemoryWebhookStore();

    const envelope = buildEnvelope("payment.confirmed", {}, true);
    const id = await dlqStore.enqueue({
      endpointId: ENDPOINT.id,
      merchantId: ENDPOINT.merchantId,
      eventType: "payment.confirmed",
      paymentId: null,
      payload: envelope,
      error: "initial",
    });

    const deps: RetryDaemonDeps = {
      dlqStore: dlqStore as unknown as WebhookDLQStore,
      webhookStore: webhookStore as any,
      fetch: okFetch(503), // non-2xx = failure
    };

    await startRetryDaemon(deps).runOnce();

    const item = await dlqStore.getById(id);
    assert.ok(item, "item should still exist");
    assert.strictEqual(item!.retry_count, 1);
    assert.strictEqual(item!.status, "pending");
    // next_retry_at should now be in the future (1 min delay)
    assert.ok(
      item!.next_retry_at !== null,
      "next_retry_at should be set after first failure",
    );
    assert.ok(
      new Date(item!.next_retry_at!).getTime() > Date.now(),
      "next retry should be in the future",
    );
  });

  it("calls onPermanentlyFailed after MAX_RETRY_ATTEMPTS failures", async () => {
    const dlqStore = new InMemoryDLQStore();
    const webhookStore = new InMemoryWebhookStore();
    const notifiedItems: string[] = [];

    const envelope = buildEnvelope("payment.confirmed", {}, true);
    const id = await dlqStore.enqueue({
      endpointId: ENDPOINT.id,
      merchantId: ENDPOINT.merchantId,
      eventType: "payment.confirmed",
      paymentId: null,
      payload: envelope,
      error: "initial",
    });

    const deps: RetryDaemonDeps = {
      dlqStore: dlqStore as unknown as WebhookDLQStore,
      webhookStore: webhookStore as any,
      fetch: okFetch(503),
      onPermanentlyFailed: async (item) => {
        notifiedItems.push(item.id);
      },
    };

    // Manually drive the retry_count to MAX_RETRY_ATTEMPTS - 1 so the next
    // call via attemptDLQDelivery tips it over the limit.
    const raw = await dlqStore.getById(id);
    if (raw) {
      for (let i = 0; i < MAX_RETRY_ATTEMPTS - 1; i++) {
        await dlqStore.recordAttemptResult(id, false, "accumulated failure");
      }
    }

    // Override next_retry_at so the item is due immediately
    await dlqStore.requeueItem(id);

    await startRetryDaemon(deps).runOnce();

    const failed = await dlqStore.getById(id);
    assert.strictEqual(failed!.status, "permanently_failed");
    assert.strictEqual(notifiedItems.length, 1, "notification hook should fire once");
    assert.strictEqual(notifiedItems[0], id);
  });

  it("skips items that are not yet due", async () => {
    const dlqStore = new InMemoryDLQStore();
    const webhookStore = new InMemoryWebhookStore();

    const envelope = buildEnvelope("payment.confirmed", {}, true);
    const id = await dlqStore.enqueue({
      endpointId: ENDPOINT.id,
      merchantId: ENDPOINT.merchantId,
      eventType: "payment.confirmed",
      paymentId: null,
      payload: envelope,
      error: "initial",
    });

    // Set next_retry_at far in the future so the daemon won't pick it up
    const item = await dlqStore.getById(id);
    if (item) {
      item.next_retry_at = new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString();
    }

    const deps: RetryDaemonDeps = {
      dlqStore: dlqStore as unknown as WebhookDLQStore,
      webhookStore: webhookStore as any,
      fetch: okFetch(200),
    };

    const { attempted } = await startRetryDaemon(deps).runOnce();
    assert.strictEqual(attempted, 0, "should not process items not yet due");
  });

  it("handles missing endpoint gracefully by dropping the item", async () => {
    const dlqStore = new InMemoryDLQStore();
    // Store with no endpoints registered
    const emptyWebhookStore = new InMemoryWebhookStore([]);

    const envelope = buildEnvelope("payment.confirmed", {}, true);
    const id = await dlqStore.enqueue({
      endpointId: "wh_deleted",
      merchantId: "merchant_1",
      eventType: "payment.confirmed",
      paymentId: null,
      payload: envelope,
      error: "initial",
    });

    const deps: RetryDaemonDeps = {
      dlqStore: dlqStore as unknown as WebhookDLQStore,
      webhookStore: emptyWebhookStore as any,
      fetch: okFetch(200),
    };

    const { attempted, succeeded } = await startRetryDaemon(deps).runOnce();
    assert.strictEqual(attempted, 1);
    // Item dropped (endpoint gone), counted as succeeded to avoid infinite retry
    assert.strictEqual(succeeded, 1);
    assert.strictEqual(await dlqStore.getById(id), null);
  });

  it("records each retry attempt in the delivery log", async () => {
    const dlqStore = new InMemoryDLQStore();
    const webhookStore = new InMemoryWebhookStore();

    const envelope = buildEnvelope("payment.confirmed", { payment_id: "pay_42" }, true);
    await dlqStore.enqueue({
      endpointId: ENDPOINT.id,
      merchantId: ENDPOINT.merchantId,
      eventType: "payment.confirmed",
      paymentId: "pay_42",
      payload: envelope,
      error: "initial",
    });

    const deps: RetryDaemonDeps = {
      dlqStore: dlqStore as unknown as WebhookDLQStore,
      webhookStore: webhookStore as any,
      fetch: okFetch(200),
    };

    await startRetryDaemon(deps).runOnce();
    assert.strictEqual(
      webhookStore.getRecordedAttempts().length,
      1,
      "one attempt should be recorded in delivery log",
    );
  });

  it("DEFAULT_RETRY_INTERVAL_MS is 60 seconds", () => {
    assert.strictEqual(DEFAULT_RETRY_INTERVAL_MS, 60_000);
  });

  it("daemon can be stopped without throwing", () => {
    const dlqStore = new InMemoryDLQStore();
    const webhookStore = new InMemoryWebhookStore();

    const deps: RetryDaemonDeps = {
      dlqStore: dlqStore as unknown as WebhookDLQStore,
      webhookStore: webhookStore as any,
      fetch: okFetch(200),
    };

    const handle = startRetryDaemon(deps, { intervalMs: 100_000 });
    assert.doesNotThrow(() => handle.stop());
  });
});

// ── attemptDLQDelivery (Issue #961) ───────────────────────────────────────

describe("attemptDLQDelivery (Issue #961)", () => {
  it("returns true and removes item on successful delivery", async () => {
    const dlqStore = new InMemoryDLQStore();
    const webhookStore = new InMemoryWebhookStore();

    const envelope = buildEnvelope("payment.confirmed", { payment_id: "pay_ok" }, true);
    const id = await dlqStore.enqueue({
      endpointId: ENDPOINT.id,
      merchantId: ENDPOINT.merchantId,
      eventType: "payment.confirmed",
      paymentId: "pay_ok",
      payload: envelope,
      error: "initial",
    });

    const item = (await dlqStore.getById(id))!;
    const ok = await attemptDLQDelivery(item, {
      dlqStore: dlqStore as unknown as WebhookDLQStore,
      webhookStore: webhookStore as any,
      fetch: okFetch(200),
    });

    assert.strictEqual(ok, true);
    assert.strictEqual(await dlqStore.getById(id), null);
  });

  it("returns false and keeps item on failed delivery", async () => {
    const dlqStore = new InMemoryDLQStore();
    const webhookStore = new InMemoryWebhookStore();

    const envelope = buildEnvelope("payment.confirmed", {}, true);
    const id = await dlqStore.enqueue({
      endpointId: ENDPOINT.id,
      merchantId: ENDPOINT.merchantId,
      eventType: "payment.confirmed",
      paymentId: null,
      payload: envelope,
      error: "initial",
    });

    const item = (await dlqStore.getById(id))!;
    const ok = await attemptDLQDelivery(item, {
      dlqStore: dlqStore as unknown as WebhookDLQStore,
      webhookStore: webhookStore as any,
      fetch: okFetch(500),
    });

    assert.strictEqual(ok, false);
    assert.ok(await dlqStore.getById(id), "item should still exist");
  });
});
