/**
 * Issue #1151: analytics router tests.
 *
 * Run with: npx ts-node --transpile-only test/analytics.test.ts
 */
import assert from "assert";
import express from "express";
import { createHmac } from "crypto";
import { createAnalyticsRouter } from "../src/routes/analytics";

const JWT_SECRET = "test-analytics-secret";

function makeJwt(merchantId: string): string {
  const header = Buffer.from(JSON.stringify({ alg: "HS256", typ: "JWT" })).toString(
    "base64url",
  );
  const payload = {
    merchant_id: merchantId,
    sub: "GABC123",
    exp: Math.floor(Date.now() / 1000) + 3600,
  };
  const body = Buffer.from(JSON.stringify(payload)).toString("base64url");
  const sig = createHmac("sha256", JWT_SECRET)
    .update(`${header}.${body}`)
    .digest("base64url");
  return `${header}.${body}.${sig}`;
}

function createTestApp() {
  const app = express();
  app.use(express.json());
  const analyticsRouter = createAnalyticsRouter({
    serverPublicKey: "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN",
    jwtSecret: JWT_SECRET,
  });
  app.use("/v1/analytics", analyticsRouter);
  app.use("/v1", analyticsRouter);
  return app;
}

async function request(
  app: express.Express,
  path: string,
  auth?: string,
): Promise<{ status: number; body: any }> {
  const server = app.listen(0);
  const addr = server.address() as { port: number };
  try {
    const headers: Record<string, string> = {};
    if (auth) headers["Authorization"] = auth;
    const res = await fetch(`http://127.0.0.1:${addr.port}${path}`, { headers });
    const body = await res.json().catch(() => ({}));
    return { status: res.status, body };
  } finally {
    server.close();
  }
}

async function main() {
  const app = createTestApp();
  const token = makeJwt("merch_test_001");

  // --- /v1/analytics/revenue requires auth ---
  {
    const res = await request(app, "/v1/analytics/revenue");
    assert.strictEqual(res.status, 401, "unauthenticated revenue request → 401");
  }

  // --- /v1/analytics/revenue with valid token ---
  {
    const res = await request(app, "/v1/analytics/revenue", `Bearer ${token}`);
    assert.strictEqual(res.status, 200, "authenticated revenue request → 200");
    assert.ok(Array.isArray(res.body.daily), "response has daily array");
    assert.ok(Array.isArray(res.body.weekly), "response has weekly array");
    assert.ok(Array.isArray(res.body.monthly), "response has monthly array");
    assert.strictEqual(res.body.merchant_id, "merch_test_001", "merchant_id in response");
    assert.strictEqual(res.body.daily.length, 7, "7 days in daily breakdown");
    assert.strictEqual(res.body.weekly.length, 4, "4 weeks in weekly breakdown");
    assert.strictEqual(res.body.monthly.length, 3, "3 months in monthly breakdown");
  }

  // --- /v1/analytics/payment-links/:id/stats requires auth ---
  {
    const res = await request(app, "/v1/analytics/payment-links/link_abc/stats");
    assert.strictEqual(res.status, 401, "unauthenticated stats request → 401");
  }

  // --- /v1/analytics/payment-links/:id/stats with valid token ---
  {
    const res = await request(
      app,
      "/v1/analytics/payment-links/link_abc/stats",
      `Bearer ${token}`,
    );
    assert.strictEqual(res.status, 200, "authenticated stats request → 200");
    assert.ok(typeof res.body.link_id === "string", "link_id present");
    assert.ok(typeof res.body.views === "number", "views is a number");
    assert.ok(typeof res.body.completions === "number", "completions is a number");
    assert.ok(typeof res.body.total_volume === "string", "total_volume is a string");
    assert.ok(
      typeof res.body.conversion_rate === "string",
      "conversion_rate is a string",
    );
    assert.ok(
      res.body.conversion_rate.endsWith("%"),
      "conversion_rate ends with %",
    );
  }

  // --- /v1/payment-links/:id/stats direct mount (Issue #803 / #1150) ---
  {
    const res = await request(
      app,
      "/v1/payment-links/link_direct/stats",
      `Bearer ${token}`,
    );
    assert.strictEqual(res.status, 200, "direct payment-links stats request → 200");
    assert.strictEqual(res.body.link_id, "link_direct", "link_id matched");
    assert.ok(
      typeof res.body.conversion_rate === "string",
      "conversion_rate is a string",
    );
    assert.ok(
      res.body.conversion_rate.endsWith("%"),
      "conversion_rate ends with %",
    );
  }

  // --- /v1/payment-links/:id/stats returns 404 for missing link ---
  {
    const res = await request(
      app,
      "/v1/payment-links/not_found/stats",
      `Bearer ${token}`,
    );
    assert.strictEqual(res.status, 404, "missing payment link → 404");
  }

  // --- expired token → 401 ---
  {
    const header = Buffer.from(JSON.stringify({ alg: "HS256", typ: "JWT" })).toString(
      "base64url",
    );
    const payload = { merchant_id: "merch_exp", exp: 1 }; // epoch 1 = expired
    const body = Buffer.from(JSON.stringify(payload)).toString("base64url");
    const sig = createHmac("sha256", JWT_SECRET)
      .update(`${header}.${body}`)
      .digest("base64url");
    const expiredToken = `${header}.${body}.${sig}`;

    const res = await request(app, "/v1/analytics/revenue", `Bearer ${expiredToken}`);
    assert.strictEqual(res.status, 401, "expired token → 401");
  }

  console.log("analytics router tests passed");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
