/**
 * Issue #1154: structured request logging middleware tests.
 *
 * Run with: npx ts-node --transpile-only test/request-logger.test.ts
 */
import assert from "assert";
import express from "express";
import { requestLogger, RequestLogEntry } from "../src/middleware/requestLogger";
import { createHmac } from "crypto";

function makeJwt(secret: string, payload: Record<string, unknown>): string {
  const header = Buffer.from(JSON.stringify({ alg: "HS256", typ: "JWT" })).toString("base64url");
  const body = Buffer.from(JSON.stringify(payload)).toString("base64url");
  const sig = createHmac("sha256", secret)
    .update(`${header}.${body}`)
    .digest("base64url");
  return `${header}.${body}.${sig}`;
}

function createTestApp(
  lines: string[],
  opts?: { level?: "info" | "debug" | "silent"; jwtSecret?: string },
) {
  const app = express();
  app.use(express.json());
  app.use(
    requestLogger({
      level: opts?.level ?? "info",
      jwtSecret: opts?.jwtSecret,
      write: (line) => lines.push(line),
    }),
  );
  app.get("/ok", (_req, res) => res.status(200).json({ ok: true }));
  app.get("/fail", (_req, res) => res.status(500).json({ error: "boom" }));
  app.post("/data", (_req, res) => res.status(201).json({ created: true }));
  return app;
}

async function request(
  app: express.Express,
  method: string,
  path: string,
  opts: { body?: unknown; auth?: string } = {},
): Promise<{ status: number }> {
  const server = app.listen(0);
  const addr = server.address() as { port: number };
  try {
    const headers: Record<string, string> = {
      "Content-Type": "application/json",
    };
    if (opts.auth) headers["Authorization"] = opts.auth;
    const res = await fetch(`http://127.0.0.1:${addr.port}${path}`, {
      method,
      headers,
      body: opts.body !== undefined ? JSON.stringify(opts.body) : undefined,
    });
    return { status: res.status };
  } finally {
    server.close();
  }
}

async function main() {
  // --- Basic log emission ---
  {
    const lines: string[] = [];
    const app = createTestApp(lines);

    await request(app, "GET", "/ok");
    assert.strictEqual(lines.length, 1, "one log line emitted per request");
    const entry: RequestLogEntry = JSON.parse(lines[0]);
    assert.strictEqual(entry.method, "GET");
    assert.strictEqual(entry.path, "/ok");
    assert.strictEqual(entry.status, 200);
    assert.strictEqual(entry.level, "info");
    assert.ok(typeof entry.latency_ms === "number", "latency_ms is a number");
    assert.ok(entry.latency_ms >= 0, "latency_ms is non-negative");
    assert.ok(typeof entry.ts === "string", "ts is a string");
    assert.strictEqual(entry.merchant_id, null, "unauthenticated → merchant_id null");
  }

  // --- 5xx response logs at error level ---
  {
    const lines: string[] = [];
    const app = createTestApp(lines);
    await request(app, "GET", "/fail");
    const entry: RequestLogEntry = JSON.parse(lines[0]);
    assert.strictEqual(entry.level, "error", "5xx → error level");
    assert.strictEqual(entry.status, 500);
  }

  // --- merchant_id extracted from JWT ---
  {
    const lines: string[] = [];
    const SECRET = "test-secret-123";
    const app = createTestApp(lines, { jwtSecret: SECRET });
    const token = makeJwt(SECRET, {
      merchant_id: "merch_abc",
      sub: "GABC",
      exp: Math.floor(Date.now() / 1000) + 3600,
    });

    await request(app, "GET", "/ok", { auth: `Bearer ${token}` });
    const entry: RequestLogEntry = JSON.parse(lines[0]);
    assert.strictEqual(entry.merchant_id, "merch_abc", "merchant_id extracted from JWT");
  }

  // --- silent level emits nothing ---
  {
    const lines: string[] = [];
    const app = createTestApp(lines, { level: "silent" });
    await request(app, "GET", "/ok");
    assert.strictEqual(lines.length, 0, "silent level → no output");
  }

  // --- debug level emits for all requests ---
  {
    const lines: string[] = [];
    const app = createTestApp(lines, { level: "debug" });
    await request(app, "GET", "/ok");
    assert.strictEqual(lines.length, 1, "debug level emits for 2xx");
    const entry: RequestLogEntry = JSON.parse(lines[0]);
    assert.strictEqual(entry.level, "debug");
  }

  // --- request_id forwarded when present ---
  {
    const lines: string[] = [];
    const appInner = express();
    appInner.use(
      requestLogger({ write: (l) => lines.push(l) }),
    );
    appInner.get("/ok", (_req, res) => res.status(200).json({ ok: true }));

    const server = appInner.listen(0);
    const addr = server.address() as { port: number };
    await fetch(`http://127.0.0.1:${addr.port}/ok`, {
      headers: { "x-request-id": "req-xyz-123" },
    });
    server.close();

    const entry: RequestLogEntry = JSON.parse(lines[0]);
    assert.strictEqual(entry.request_id, "req-xyz-123", "request_id forwarded");
  }

  console.log("request logger tests passed");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
