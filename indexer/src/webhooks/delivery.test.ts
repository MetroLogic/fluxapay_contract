import { buildEnvelope, deliverOnce, type FetchLike } from "./delivery";
import type { WebhookEndpoint } from "./types";

const ISO_8601_UTC_REGEX = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/;

describe("WebhookEnvelope v1 wire contract", () => {
  const fixedNowMs = Date.parse("2024-03-09T16:00:00.000Z");

  it("serializes exact v1 top-level keys and ISO-8601 UTC createdAt", () => {
    const envelope = buildEnvelope(
      "payment.confirmed",
      {
        payment_id: "pay_abc123",
        merchant_id: "G...",
        amount: "10000000",
        currency: "USDC",
        status: "confirmed",
      },
      true,
      () => fixedNowMs,
    );

    const parsed = JSON.parse(JSON.stringify(envelope)) as Record<string, unknown>;

    expect(Object.keys(parsed)).toEqual([
      "id",
      "event_id",
      "type",
      "createdAt",
      "livemode",
      "data",
    ]);
    expect(parsed.id).toEqual(expect.stringMatching(/^whd_/));
    expect(parsed.event_id).toEqual(expect.stringMatching(/^evt_/));
    expect(parsed.type).toBe("payment.confirmed");
    expect(typeof parsed.createdAt).toBe("string");
    expect(parsed.createdAt).toBe("2024-03-09T16:00:00.000Z");
    expect(parsed.createdAt).toMatch(ISO_8601_UTC_REGEX);
    expect(parsed.livemode).toBe(true);
    expect("created_at" in parsed).toBe(false);
    expect("api_version" in parsed).toBe(false);
    expect("replayed" in parsed).toBe(false);
  });

  it("includes replayed=true only when explicitly replayed", () => {
    const envelope = buildEnvelope(
      "payment.confirmed",
      { payment_id: "pay_abc123" },
      true,
      () => fixedNowMs,
      { eventId: "evt_stable_123", replayed: true },
    );

    const parsed = JSON.parse(JSON.stringify(envelope)) as Record<string, unknown>;

    expect(Object.keys(parsed)).toEqual([
      "id",
      "event_id",
      "type",
      "createdAt",
      "livemode",
      "data",
      "replayed",
    ]);
    expect(parsed.event_id).toBe("evt_stable_123");
    expect(parsed.replayed).toBe(true);
  });

  it("delivers the exact signed JSON payload matching the v1 envelope schema", async () => {
    const endpoint: WebhookEndpoint = {
      id: "wh_ep_1",
      merchantId: "GMERCHANT",
      url: "https://merchant.example.com/webhooks/fluxapay",
      signingSecret: "whsec_test_secret",
      eventTypes: ["payment.confirmed"],
      enabled: true,
    };

    const envelope = buildEnvelope(
      "payment.confirmed",
      {
        payment_id: "pay_abc123",
        merchant_id: "GMERCHANT",
        amount: "10000000",
        currency: "USDC",
        status: "confirmed",
      },
      false,
      () => fixedNowMs,
    );

    let capturedBody = "";
    const fakeFetch: FetchLike = async (_url, init) => {
      capturedBody = init.body;
      return {
        status: 200,
        text: async () => JSON.stringify({ received: true }),
      };
    };

    const result = await deliverOnce(endpoint, envelope, 1, {
      fetch: fakeFetch,
      now: () => fixedNowMs,
    });

    expect(result.attempt.success).toBe(true);
    expect(result.signedBody).toBe(capturedBody);

    const wirePayload = JSON.parse(capturedBody) as Record<string, unknown>;
    expect(Object.keys(wirePayload)).toEqual([
      "id",
      "event_id",
      "type",
      "createdAt",
      "livemode",
      "data",
    ]);
    expect(wirePayload.createdAt).toBe("2024-03-09T16:00:00.000Z");
    expect(wirePayload.livemode).toBe(false);
    expect("created_at" in wirePayload).toBe(false);
    expect("api_version" in wirePayload).toBe(false);
  });
});
