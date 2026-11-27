/**
 * Issue #1152: Unit tests for the settlements router module.
 *
 * Run with: npx ts-node --transpile-only test/settlements.test.ts
 * (from the backend/ directory)
 */
import assert from "assert";
import express from "express";
import { createSettlementRouter } from "../src/routes/settlements";
import { requireJsonContentType } from "../src/middleware/requireJsonContentType";

function createTestApp(jwtSecret?: string) {
  const app = express();
  app.use(express.json());
  app.use(requireJsonContentType);
  app.use("/settlements", createSettlementRouter({
    networkPassphrase: "Test SDF Network ; September 2015",
    homeDomain: "fluxapay.stellar.org",
    jwtSecret,
  }));
  return app;
}

async function request(
  app: express.Express,
  method: string,
  path: string,
  opts: { auth?: string; body?: string; contentType?: string } = {},
): Promise<{ status: number; body: any }> {
  const server = app.listen(0);
  const addr = server.address();
  if (!addr || typeof addr === "string") {
    server.close();
    throw new Error("failed to bind test server");
  }
  try {
    const headers: Record<string, string> = {};
    if (opts.contentType) {
      headers["Content-Type"] = opts.contentType;
    } else if (opts.body !== undefined) {
      headers["Content-Type"] = "application/json";
    }
    if (opts.auth) {
      headers["Authorization"] = `Bearer ${opts.auth}`;
    }
    const res = await fetch(`http://127.0.0.1:${addr.port}${path}`, {
      method,
      headers,
      body: opts.body,
    });
    const body = await res.json().catch(() => ({}));
    return { status: res.status, body };
  } finally {
    server.close();
  }
}

/** Create a minimal JWT-like token. With a jwtSecret configured, we build a
 *  properly signed token; without it, any base64url string with a payload
 *  containing `sub` is accepted. */
function makeToken(sub: string, jwtSecret?: string): string {
  const header = Buffer.from(JSON.stringify({ alg: "HS256", typ: "JWT" })).toString("base64url");
  const payload = Buffer.from(JSON.stringify({ sub, merchant_id: sub, iat: Math.floor(Date.now() / 1000), exp: Math.floor(Date.now() / 1000) + 3600 })).toString("base64url");

  if (!jwtSecret) {
    return `${header}.${payload}.sig`;
  }

  const crypto = require("crypto");
  const sig = crypto.createHmac("sha256", jwtSecret).update(`${header}.${payload}`).digest("base64url");
  return `${header}.${payload}.${sig}`;
}

async function main() {
  const JWT_SECRET = "test-jwt-secret";
  const app = createTestApp(JWT_SECRET);
  const merchantA = "GMERCHANT_A";
  const tokenA = makeToken(merchantA, JWT_SECRET);
  const tokenB = makeToken("GMERCHANT_B", JWT_SECRET);

  // ---- No auth → 401

  {
    const res = await request(app, "GET", "/settlements");
    assert.strictEqual(res.status, 401, "GET /settlements without auth should return 401");
    assert.strictEqual(res.body.error, "merchant authentication required");
  }

  {
    const res = await request(app, "GET", "/settlements/stl_nonexistent");
    assert.strictEqual(res.status, 401, "GET /settlements/:id without auth should return 401");
  }

  // ---- Invalid token → 401

  {
    const res = await request(app, "GET", "/settlements", { auth: "invalid-token" });
    assert.strictEqual(res.status, 401, "invalid token should return 401");
    assert.strictEqual(res.body.error, "invalid or expired token");
  }

  // ---- List settlements (merchant A has seeded data for GMERCHANT_1, not A)
  //      The list should be empty for a merchant with no settlements.

  {
    const res = await request(app, "GET", "/settlements", { auth: tokenA });
    assert.strictEqual(res.status, 200, "GET /settlements with auth should return 200");
    assert.strictEqual(res.body.count, 0, "merchant A should have no settlements");
    assert.strictEqual(res.body.settlements.length, 0);
  }

  // ---- Payout on non-existent settlement → 404

  {
    const res = await request(app, "POST", "/settlements/stl_nonexistent/payout", {
      auth: tokenA,
      body: JSON.stringify({}),
    });
    assert.strictEqual(res.status, 404, "payout on non-existent settlement should return 404");
    assert.strictEqual(res.body.error, "settlement stl_nonexistent not found");
  }

  // ---- Cross-merchant isolation: merchant B should not see merchant A's settlements

  {
    // Use the seeded settlement for GMERCHANT_1 (first seeded record).
    // Find it by listing all without filter — but we need the ID. Since
    // merchant A has no settlements, we test cross-merchant by creating a
    // settlement through the payout flow is not possible. Instead, we verify
    // the get-by-id returns 404 for a merchant who doesn't own it.
    // We'll use the seeded data: GMERCHANT_1 has a settlement. Merchant A
    // requesting it should get 404.

    // List all settlements for merchant 1 to find the ID.
    const tokenM1 = makeToken("GDEV_MERCHANT_1", JWT_SECRET);
    const listRes = await request(app, "GET", "/settlements", { auth: tokenM1 });
    assert.strictEqual(listRes.status, 200);
    assert.strictEqual(listRes.body.count, 1, "merchant 1 should have 1 seeded settlement");

    const settlementId = listRes.body.settlements[0].settlement_id;

    // Merchant A should get 404 (not their settlement).
    const crossRes = await request(app, "GET", `/settlements/${settlementId}`, { auth: tokenA });
    assert.strictEqual(crossRes.status, 404, "merchant A should not see merchant 1's settlement");

    // Merchant B should also get 404.
    const crossResB = await request(app, "GET", `/settlements/${settlementId}`, { auth: tokenB });
    assert.strictEqual(crossResB.status, 404, "merchant B should not see merchant 1's settlement");
  }

  // ---- Payout flow: initiate and check status

  {
    const tokenM1 = makeToken("GDEV_MERCHANT_1", JWT_SECRET);
    const listRes = await request(app, "GET", "/settlements", { auth: tokenM1 });
    const settlementId = listRes.body.settlements[0].settlement_id;

    // Initiate payout.
    const payoutRes = await request(app, "POST", `/settlements/${settlementId}/payout`, {
      auth: tokenM1,
      body: JSON.stringify({ payout_channel_id: "ch_bank_ngn" }),
    });
    assert.strictEqual(payoutRes.status, 202, "payout initiation should return 202");
    assert.strictEqual(payoutRes.body.status, "Processing");
    assert.strictEqual(payoutRes.body.payout_channel_id, "ch_bank_ngn");

    // Verify status changed.
    const getRes = await request(app, "GET", `/settlements/${settlementId}`, { auth: tokenM1 });
    assert.strictEqual(getRes.status, 200);
    assert.strictEqual(getRes.body.status, "Processing");
    assert.strictEqual(getRes.body.payout_channel_id, "ch_bank_ngn");

    // Second payout → 409 (already processing).
    const dupRes = await request(app, "POST", `/settlements/${settlementId}/payout`, {
      auth: tokenM1,
      body: JSON.stringify({}),
    });
    assert.strictEqual(dupRes.status, 409, "duplicate payout should return 409");
    assert.strictEqual(dupRes.body.error, "payout already in progress");
  }

  // ---- Status filter

  {
    const tokenM1 = makeToken("GDEV_MERCHANT_1", JWT_SECRET);
    const listRes = await request(app, "GET", "/settlements?status=Pending", { auth: tokenM1 });
    assert.strictEqual(listRes.status, 200);
    // The seeded merchant 1 settlement was transitioned to Processing above,
    // so Pending filter should return 0.
    assert.strictEqual(listRes.body.count, 0, "no Pending settlements after payout");

    const processingRes = await request(app, "GET", "/settlements?status=Processing", { auth: tokenM1 });
    assert.strictEqual(processingRes.status, 200);
    assert.strictEqual(processingRes.body.count, 1, "1 Processing settlement after payout");
  }

  // ---- Content-type enforcement on POST

  {
    const tokenM1 = makeToken("GDEV_MERCHANT_1", JWT_SECRET);
    const listRes = await request(app, "GET", "/settlements", { auth: tokenM1 });
    const settlementId = listRes.body.settlements[0].settlement_id;

    const formRes = await request(app, "POST", `/settlements/${settlementId}/payout`, {
      auth: tokenM1,
      contentType: "application/x-www-form-urlencoded",
      body: "payout_channel_id=x",
    });
    assert.strictEqual(formRes.status, 415, "form-post should be rejected with 415");
  }

  console.log("settlements router tests passed");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
