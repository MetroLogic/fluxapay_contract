/**
 * Issue #1153: MerchantRegistryClient tests.
 *
 * Run with: npx ts-node --transpile-only test/merchant-registry.test.ts
 */
import assert from "assert";
import http from "http";
import { MerchantRegistryClient, buildMerchantIdResolver } from "../src/lib/merchantRegistry";

// ---------------------------------------------------------------------------
// Minimal in-process mock registry server
// ---------------------------------------------------------------------------

function startMockRegistry(
  handler: (
    path: string,
  ) => { status: number; body: unknown } | null,
): Promise<{ url: string; close: () => void }> {
  return new Promise((resolve) => {
    const server = http.createServer((req, res) => {
      const result = handler(req.url ?? "");
      if (!result) {
        res.writeHead(500);
        res.end(JSON.stringify({ error: "unhandled" }));
        return;
      }
      res.writeHead(result.status, { "Content-Type": "application/json" });
      res.end(JSON.stringify(result.body));
    });
    server.listen(0, "127.0.0.1", () => {
      const addr = server.address() as { port: number };
      resolve({
        url: `http://127.0.0.1:${addr.port}`,
        close: () => server.close(),
      });
    });
  });
}

async function main() {
  // --- Successful lookup ---
  {
    const { url, close } = await startMockRegistry((path) => {
      if (path.includes("GABC")) {
        return { status: 200, body: { merchant_id: "merch_001" } };
      }
      return { status: 404, body: { error: "not found" } };
    });

    const client = new MerchantRegistryClient({ registryUrl: url });
    const id = await client.resolveMerchantId("GABC");
    assert.strictEqual(id, "merch_001", "resolves registered merchant id");
    close();
  }

  // --- 404 → undefined ---
  {
    const { url, close } = await startMockRegistry(() => ({
      status: 404,
      body: { error: "not found" },
    }));
    const client = new MerchantRegistryClient({ registryUrl: url });
    const id = await client.resolveMerchantId("GUNKNOWN");
    assert.strictEqual(id, undefined, "unregistered account → undefined");
    close();
  }

  // --- Caching: second call doesn't hit the server ---
  {
    let callCount = 0;
    const { url, close } = await startMockRegistry(() => {
      callCount += 1;
      return { status: 200, body: { merchant_id: "merch_cache" } };
    });
    const client = new MerchantRegistryClient({ registryUrl: url, cacheTtlMs: 60_000 });
    await client.resolveMerchantId("GCACHE");
    await client.resolveMerchantId("GCACHE");
    assert.strictEqual(callCount, 1, "second call served from cache");
    close();
  }

  // --- Timeout → fail open (returns undefined) ---
  {
    const { url, close } = await startMockRegistry((_path) => {
      // Simulate slow response by never resolving — but we close the server
      // immediately so the client gets a connection reset.
      return null;
    });
    close(); // immediately close so the client gets ECONNREFUSED
    const client = new MerchantRegistryClient({ registryUrl: url, timeoutMs: 500 });
    const id = await client.resolveMerchantId("GTIMEOUT");
    assert.strictEqual(id, undefined, "unreachable registry → undefined (fail open)");
  }

  // --- invalidate clears cache ---
  {
    let callCount = 0;
    const { url, close } = await startMockRegistry(() => {
      callCount += 1;
      return { status: 200, body: { merchant_id: "merch_inv" } };
    });
    const client = new MerchantRegistryClient({ registryUrl: url, cacheTtlMs: 60_000 });
    await client.resolveMerchantId("GINV");
    client.invalidate("GINV");
    await client.resolveMerchantId("GINV");
    assert.strictEqual(callCount, 2, "invalidate forces re-fetch");
    close();
  }

  // --- buildMerchantIdResolver with no MERCHANT_REGISTRY_URL ---
  {
    const { resolveMerchantId, client } = buildMerchantIdResolver({});
    assert.strictEqual(client, null, "no URL → client is null");
    const id = await resolveMerchantId("GANY");
    assert.strictEqual(id, undefined, "no URL → always undefined");
  }

  // --- buildMerchantIdResolver with URL ---
  {
    const { url, close } = await startMockRegistry(() => ({
      status: 200,
      body: { id: "merch_built" },
    }));
    const { resolveMerchantId, client } = buildMerchantIdResolver({
      MERCHANT_REGISTRY_URL: url,
    });
    assert.ok(client !== null, "URL present → client created");
    const id = await resolveMerchantId("GBUILT");
    assert.strictEqual(id, "merch_built", "resolver returns id from registry");
    close();
  }

  console.log("MerchantRegistryClient tests passed");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
