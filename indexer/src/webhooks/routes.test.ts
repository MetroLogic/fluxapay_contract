import { describe, expect, it, jest } from "@jest/globals";
import { registerWebhookRoutes } from "./routes";

function responseMock() {
  const res: any = {
    statusCode: 200,
    body: undefined,
  };
  res.status = jest.fn((code: number) => { res.statusCode = code; return res; });
  res.json = jest.fn((body: unknown) => { res.body = body; return res; });
  res.send = jest.fn((body?: unknown) => { res.body = body; return res; });
  res.setHeader = jest.fn();
  return res;
}

describe("merchant webhook endpoint routes", () => {
  function setup() {
    const handlers = new Map<string, Function>();
    const app: any = {
      get: jest.fn((path: string, ...handlersForPath: Function[]) =>
        handlers.set(`GET ${path}`, handlersForPath.at(-1)!)),
      post: jest.fn((path: string, ...handlersForPath: Function[]) =>
        handlers.set(`POST ${path}`, handlersForPath.at(-1)!)),
      patch: jest.fn((path: string, ...handlersForPath: Function[]) =>
        handlers.set(`PATCH ${path}`, handlersForPath.at(-1)!)),
      put: jest.fn((path: string, ...handlersForPath: Function[]) =>
        handlers.set(`PUT ${path}`, handlersForPath.at(-1)!)),
      delete: jest.fn((path: string, ...handlersForPath: Function[]) =>
        handlers.set(`DELETE ${path}`, handlersForPath.at(-1)!)),
    };
    const store: any = {
      listEndpoints: jest.fn(async () => [{
        id: "wh_1",
        merchantId: "merchant-a",
        url: "https://example.com/hooks",
        signingSecret: "must-not-leak",
        eventTypes: ["payment.confirmed"],
        enabled: true,
        signingAlgorithm: "hmac_sha256",
      }]),
      createEndpoint: jest.fn(),
      updateEndpoint: jest.fn(async () => null),
      deleteEndpoint: jest.fn(async () => true),
      rotateSigningSecret: jest.fn(async () => ({
        endpoint: {
          id: "wh_1",
          merchantId: "merchant-a",
          url: "https://example.com/hooks",
          signingSecret: "replacement-must-be-reported",
          eventTypes: [],
          enabled: true,
          signingAlgorithm: "hmac_sha256",
        },
        signingSecret: "new-secret",
      })),
      getEndpoint: jest.fn(),
      countRecentTestDeliveries: jest.fn(),
      listDeliveries: jest.fn(),
      recordAttempt: jest.fn(),
    };
    registerWebhookRoutes(app, {
      store,
      merchantIdFromRequest: () => "merchant-a",
    });
    return { handlers, store };
  }

  it("rejects invalid endpoint URLs", async () => {
    const { handlers, store } = setup();
    const res = responseMock();
    const handler = handlers.get("POST /v1/webhooks")!;
    await handler({ body: { url: "ftp://example.com/hooks" } }, res, jest.fn());

    expect(res.statusCode).toBe(400);
    expect(store.createEndpoint).not.toHaveBeenCalled();
  });

  it("redacts signing secrets from list responses", async () => {
    const { handlers } = setup();
    const res = responseMock();
    const handler = handlers.get("GET /v1/webhooks")!;
    await handler({}, res, jest.fn());

    expect(res.statusCode).toBe(200);
    expect(JSON.stringify(res.body)).not.toContain("must-not-leak");
  });

  it("does not expose a previous secret during rotation", async () => {
    const { handlers } = setup();
    const res = responseMock();
    const req: any = { params: { endpointId: "wh_1" } };
    const handler = handlers.get("POST /v1/webhooks/:endpointId/rotate-secret")!;
    await handler(req, res, jest.fn());

    expect(res.statusCode).toBe(200);
    expect(res.body.signing_secret).toBe("new-secret");
  });

  it("returns 404 for an endpoint owned by another merchant", async () => {
    const { handlers, store } = setup();
    store.updateEndpoint.mockResolvedValue(null);
    const res = responseMock();
    const handler = handlers.get("PATCH /v1/webhooks/:endpointId")!;
    await handler({
      params: { endpointId: "wh_other" },
      body: { enabled: false },
    }, res, jest.fn());

    expect(res.statusCode).toBe(404);
  });
});
