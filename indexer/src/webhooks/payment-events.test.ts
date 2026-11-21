import express from "express";
import { getWebhookEventType } from "../index";
import { registerWebhookRoutes } from "./routes";
import { WEBHOOK_EVENT_TYPES } from "./types";
import { WebhookStore } from "./store";

describe("payment lifecycle webhook event types", () => {
  it.each([
    ["CREATED", "payment.created"],
    ["PENDING", "payment.pending"],
    ["CONFIRMED", "payment.confirmed"],
    ["VERIFIED", "payment.confirmed"],
    ["EXPIRED", "payment.expired"],
    ["SETTLED", "payment.settled"],
    ["PARTIALLY_PAID", "payment.partially_paid"],
    ["OVERPAID", "payment.overpaid"],
    ["FAILED", "payment.failed"],
    ["CANCELLED", "payment.cancelled"],
  ] as const)("maps PAYMENT/%s to %s", (action, expected) => {
    expect(getWebhookEventType({ topic: ["PAYMENT", action] })).toBe(expected);
  });

  it("does not map unsupported payment topics", () => {
    expect(getWebhookEventType({ topic: ["PAYMENT", "UNKNOWN"] })).toBeNull();
  });
});

describe("POST /v1/webhooks/test event type validation", () => {
  const app = express();
  let baseUrl: string;
  let server: ReturnType<typeof app.listen>;

  beforeAll(async () => {
    app.use(express.json());
    registerWebhookRoutes(app, {
      store: {
        getEndpoint: jest.fn().mockResolvedValue({
          id: "endpoint_1",
          merchantId: "merchant_1",
          url: "https://merchant.example/webhooks",
          signingSecret: "whsec_test",
          eventTypes: [],
          enabled: true,
        }),
        countRecentTestDeliveries: jest.fn().mockResolvedValue(0),
        recordAttempt: jest.fn().mockResolvedValue("delivery_1"),
      } as unknown as WebhookStore,
      fetch: async () => ({ status: 200, text: async () => "" }),
      merchantIdFromRequest: () => "merchant_1",
    });

    server = app.listen(0);
    await new Promise<void>((resolve) => server.once("listening", resolve));
    const address = server.address();
    if (!address || typeof address === "string") {
      throw new Error("Unable to start webhook test server");
    }
    baseUrl = `http://127.0.0.1:${address.port}`;
  });

  afterAll(async () => {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  });

  it.each(WEBHOOK_EVENT_TYPES)("accepts documented event type %s", async (eventType) => {
    const response = await fetch(`${baseUrl}/v1/webhooks/test`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ endpoint_id: "endpoint_1", event_type: eventType }),
    });

    expect(response.status).toBe(200);
    expect((await response.json()).event_type).toBe(eventType);
  });

  it("rejects an unsupported event type", async () => {
    const response = await fetch(`${baseUrl}/v1/webhooks/test`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ endpoint_id: "endpoint_1", event_type: "payment.unknown" }),
    });

    expect(response.status).toBe(400);
    expect(await response.json()).toMatchObject({
      error: { code: "UnsupportedEventType" },
    });
  });
});
