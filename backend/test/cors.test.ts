/**
 * Issue #1156: CORS allowed/blocked origin tests.
 *
 * Run with: npx ts-node --transpile-only test/cors.test.ts
 * (from the backend/ directory)
 */
import assert from "assert";
import express from "express";
import { corsMiddleware } from "../src/middleware/cors";

function createTestApp() {
  const app = express();
  app.use(express.json());
  app.use(corsMiddleware);
  app.get("/health", (_req, res) => res.status(200).json({ status: "ok" }));
  app.post("/data", (_req, res) => res.status(200).json({ ok: true }));
  return app;
}

async function request(
  app: express.Express,
  method: string,
  path: string,
  opts: { origin?: string; contentType?: string; body?: string } = {},
): Promise<{ status: number; headers: Record<string, string>; body: any }> {
  const server = app.listen(0);
  const addr = server.address();
  if (!addr || typeof addr === "string") {
    server.close();
    throw new Error("failed to bind test server");
  }
  try {
    const headers: Record<string, string> = {};
    if (opts.origin) headers["Origin"] = opts.origin;
    if (opts.contentType) headers["Content-Type"] = opts.contentType;
    const res = await fetch(`http://127.0.0.1:${addr.port}${path}`, {
      method,
      headers,
      body: opts.body,
    });
    const body = await res.json().catch(() => null);
    const headersObj: Record<string, string> = {};
    res.headers.forEach((value, key) => {
      headersObj[key.toLowerCase()] = value;
    });
    return { status: res.status, headers: headersObj, body };
  } finally {
    server.close();
  }
}

async function main() {
  const app = createTestApp();

  // ---- Allowed origin gets CORS headers ----

  const allowedGet = await request(app, "GET", "/health", {
    origin: "http://localhost:3000",
  });
  assert.strictEqual(allowedGet.status, 200, "allowed origin GET should succeed");
  assert.strictEqual(
    allowedGet.headers["access-control-allow-origin"],
    "http://localhost:3000",
    "allowed origin should be reflected in ACAO header",
  );
  assert.strictEqual(
    allowedGet.headers["access-control-allow-credentials"],
    "true",
    "credentials should be enabled",
  );
  assert.strictEqual(
    allowedGet.headers["vary"],
    "Origin",
    "Vary: Origin should be set",
  );

  // ---- Blocked origin gets no CORS headers ----

  const blockedGet = await request(app, "GET", "/health", {
    origin: "https://evil.example.com",
  });
  assert.strictEqual(blockedGet.status, 200, "blocked origin GET still succeeds server-side");
  assert.strictEqual(
    blockedGet.headers["access-control-allow-origin"],
    undefined,
    "blocked origin should NOT get ACAO header",
  );

  // ---- No origin header (non-browser request) gets no CORS headers ----

  const noOrigin = await request(app, "GET", "/health");
  assert.strictEqual(noOrigin.status, 200);
  assert.strictEqual(
    noOrigin.headers["access-control-allow-origin"],
    undefined,
    "no origin header should not get ACAO",
  );

  // ---- Preflight OPTIONS from allowed origin ----

  const preflight = await request(app, "OPTIONS", "/data", {
    origin: "http://localhost:3000",
  });
  assert.strictEqual(preflight.status, 204, "preflight should return 204");
  assert.strictEqual(
    preflight.headers["access-control-allow-origin"],
    "http://localhost:3000",
    "preflight from allowed origin should get ACAO",
  );
  assert.ok(
    preflight.headers["access-control-allow-methods"]?.includes("GET"),
    "preflight should list GET in allowed methods",
  );
  assert.ok(
    preflight.headers["access-control-allow-methods"]?.includes("POST"),
    "preflight should list POST in allowed methods",
  );
  assert.ok(
    preflight.headers["access-control-allow-headers"]?.includes("Authorization"),
    "preflight should allow Authorization header",
  );
  assert.ok(
    preflight.headers["access-control-allow-headers"]?.includes("Content-Type"),
    "preflight should allow Content-Type header",
  );
  assert.ok(
    preflight.headers["access-control-max-age"],
    "preflight should set max-age",
  );

  // ---- Preflight OPTIONS from blocked origin ----

  const blockedPreflight = await request(app, "OPTIONS", "/data", {
    origin: "https://evil.example.com",
  });
  assert.strictEqual(blockedPreflight.status, 204, "preflight still returns 204");
  assert.strictEqual(
    blockedPreflight.headers["access-control-allow-origin"],
    undefined,
    "blocked origin preflight should not get ACAO",
  );

  // ---- Second allowed origin (localhost:3001) ----

  const secondAllowed = await request(app, "GET", "/health", {
    origin: "http://localhost:3001",
  });
  assert.strictEqual(secondAllowed.status, 200);
  assert.strictEqual(
    secondAllowed.headers["access-control-allow-origin"],
    "http://localhost:3001",
    "second default origin should be allowed",
  );

  console.log("CORS tests passed");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
