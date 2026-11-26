import { describe, expect, it, jest } from "@jest/globals";
import { WebhookStore } from "./store";

describe("WebhookStore endpoint management", () => {
  it("lists endpoints only for the requested merchant", async () => {
    const pool = {
      query: jest.fn(async () => ({
        rows: [{
          id: "wh_1",
          merchant_id: "merchant-a",
          url: "https://example.com/hooks",
          signing_secret: "internal-secret",
          event_types: ["payment.confirmed"],
          enabled: true,
          signing_algorithm: "hmac_sha256",
        }],
      })),
    };
    const store = new WebhookStore(pool as any);

    const endpoints = await store.listEndpoints("merchant-a");

    expect(pool.query).toHaveBeenCalledWith(expect.stringContaining("merchant_id = $1"), ["merchant-a"]);
    expect(endpoints).toHaveLength(1);
    expect(endpoints[0].merchantId).toBe("merchant-a");
  });

  it("deletes only an endpoint owned by the merchant", async () => {
    const pool = {
      query: jest.fn(async () => ({ rowCount: 1 })),
    };
    const store = new WebhookStore(pool as any);

    expect(await store.deleteEndpoint("wh_1", "merchant-a")).toBe(true);
    expect(pool.query).toHaveBeenCalledWith(
      expect.stringContaining("merchant_id = $2"),
      ["wh_1", "merchant-a"],
    );
  });

  it("returns null when updating another merchant's endpoint", async () => {
    const pool = {
      query: jest.fn(async () => ({ rows: [] })),
    };
    const store = new WebhookStore(pool as any);

    expect(await store.updateEndpoint("wh_1", "merchant-b", { enabled: false })).toBeNull();
  });
});
