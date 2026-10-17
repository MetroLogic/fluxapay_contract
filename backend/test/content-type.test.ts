/**
 * Issue #838: form-post rejected; JSON accepted.
 *
 * Run with: npx ts-node --transpile-only test/content-type.test.ts
 * (from the backend/ directory)
 */
import assert from "assert";
import express from "express";
import { requireJsonContentType } from "../src/middleware/requireJsonContentType";

function createTestApp() {
  const app = express();
  app.use(express.json());
  app.use(requireJsonContentType);
  app.post("/auth/token", (_req, res) => res.status(200).json({ ok: true }));
  app.get("/auth/challenge", (_req, res) => res.status(200).json({ ok: true }));
  return app;
}

async function request(
  app: express.Express,
  method: string,
  path: string,
  opts: { contentType?: string; body?: string } = {},
): Promise<{ status: number; body: any }> {
  const server = app.listen(0);
  const addr = server.address();
  if (!addr || typeof addr === "string") {
    server.close();
    throw new Error("failed to bind test server");
  }
  try {
    const headers: Record<string, string> = {};
    if (opts.contentType) headers["Content-Type"] = opts.contentType;
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

async function main() {
  const app = createTestApp();

  const form = await request(app, "POST", "/auth/token", {
    contentType: "application/x-www-form-urlencoded",
    body: "transaction=x&account=GTEST",
  });
  assert.strictEqual(form.status, 415, "form-post must be rejected with 415");
  assert.strictEqual(form.body.error, "Unsupported Media Type");

  const json = await request(app, "POST", "/auth/token", {
    contentType: "application/json",
    body: JSON.stringify({ transaction: "x", account: "GTEST" }),
  });
  assert.strictEqual(json.status, 200, "JSON POST must be accepted");

  const get = await request(app, "GET", "/auth/challenge");
  assert.strictEqual(get.status, 200, "GET must be unaffected");

  console.log("content-type CSRF tests passed");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
