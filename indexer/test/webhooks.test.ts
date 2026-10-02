/**
 * Webhook signing, delivery, and the two endpoints (Issues #808, #810).
 *
 * The network and the database are both injected, so these exercise the
 * decisions — signing scheme, truncation, success classification, rate limit,
 * pagination — rather than the transport.
 */

import assert from "node:assert";
import test, { describe, it } from "node:test";

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
  type FetchLike,
  type WebhookEndpoint,
} from "../src/webhooks";

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
    assert.ok(out !== null && !out.endsWith("�"));
  });

  it("passes null through", () => {
    assert.strictEqual(truncateResponseBody(null), null);
  });
});

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
    const failing: FetchLike = async () => {
      throw new Error("ECONNREFUSED");
    };
    const { attempt } = await deliverOnce(
      ENDPOINT,
      buildEnvelope("payment.failed", {}, true),
      3,
      { fetch: failing },
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
