/**
 * Issue #1263 — serialized event polling.
 *
 * `EventSubscriber.start()` used to hand `pollEvents()` to `setInterval`
 * without any guard, so a slow `getEvents` response or a slow batch let the
 * next tick start a second poll from the same ledger cursor. These tests use a
 * deliberately delayed event source to prove that:
 *
 * 1. at most one poll/batch runs at a time,
 * 2. a full page hands off through the paging cursor instead of jumping past
 *    events that were never read, and
 * 3. a failed cycle keeps polling and leaves the ledger cursor untouched.
 */

import assert from "node:assert";
import test, { describe, it } from "node:test";
import { EventSubscriber, type EventSubscriptionConfig } from "../src/index";
import type { Database } from "../src/database";

const DELAY_MS = 25;

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function baseConfig(overrides: Partial<EventSubscriptionConfig> = {}): EventSubscriptionConfig {
  return {
    rpcUrl: "http://localhost:8000",
    contractIds: ["C_TEST"],
    dbConnectionString: "postgres://localhost/test",
    pollInterval: 1000,
    startLedger: 1,
    dlqRetryIntervalMs: 60_000,
    dlqMinAgeSeconds: 60,
    apiPort: 3001,
    ...overrides,
  };
}

/** Minimal in-memory Database stand-in: records stored events in arrival order. */
function createMockDatabase() {
  const state = {
    stored: [] as string[],
    storeAttempts: 0,
  };

  const mockDb = {
    state,
    async initialize() {},
    async close() {},
    async storeEvent(event: { id: string }) {
      state.storeAttempts += 1;
      if (state.stored.includes(event.id)) return false;
      state.stored.push(event.id);
      return true;
    },
    async storeDeadLetterEvent() {},
  };

  return mockDb as unknown as Database & { state: typeof state };
}

function rpcEvent(ledger: number, index: number, pagingToken = `token-${index}`) {
  return {
    id: `evt-${index}`,
    ledger,
    ledgerClosedAt: "2026-01-01T00:00:00Z",
    txHash: `tx-${index}`,
    contractId: "C_TEST",
    topic: ["PAYMENT", "CREATED"],
    value: {
      payment_id: `payment-${index}`,
      merchant_id: "merchant-1",
      amount: "100",
    },
    pagingToken,
  };
}

describe("ISSUE #1263 — serialized indexer polling", () => {
  it("never overlaps poll cycles when the event source is slower than the interval", async () => {
    const mockDb = createMockDatabase();
    let activeRequests = 0;
    let maxActiveRequests = 0;

    const events = [rpcEvent(100, 1), rpcEvent(101, 2), rpcEvent(102, 3)];
    const subscriber = new EventSubscriber(baseConfig({ pollInterval: 5 }), mockDb, {
      eventSource: {
        async getEvents(request: { startLedger?: number }) {
          activeRequests += 1;
          maxActiveRequests = Math.max(maxActiveRequests, activeRequests);
          try {
            // Deliberately slower than the poll interval.
            await delay(DELAY_MS);
            const startLedger = request.startLedger ?? 0;
            const page = events.filter((event) => event.ledger >= startLedger);
            return { events: page, latestLedger: 102 };
          } finally {
            activeRequests -= 1;
          }
        },
      } as any,
    });

    await subscriber.start();
    try {
      await delay(DELAY_MS * 5);
    } finally {
      await subscriber.shutdown();
    }

    assert.strictEqual(
      maxActiveRequests,
      1,
      "at most one poll cycle may be active per subscriber",
    );
    assert.deepStrictEqual(
      mockDb.state.stored,
      ["100-tx-1-evt-1", "101-tx-2-evt-2", "102-tx-3-evt-3"],
      "each event must be processed exactly once, in order",
    );
  });

  it("does not skip events past a full page when the RPC pages results", async () => {
    const mockDb = createMockDatabase();
    // 250 events in one ledger force three pages (100 + 100 + 50).
    const events = Array.from({ length: 250 }, (_, index) =>
      rpcEvent(500, index + 1, `cursor-${index + 1}`),
    );
    const requests: Array<{ startLedger?: number; cursor?: string; limit?: number }> = [];

    const subscriber = new EventSubscriber(baseConfig(), mockDb, {
      eventSource: {
        async getEvents(request: { startLedger?: number; cursor?: string; limit?: number }) {
          requests.push({ ...request });
          const limit = request.limit ?? 100;
          let offset = 0;
          if (request.cursor) {
            offset = events.findIndex((event) => event.pagingToken === request.cursor) + 1;
          } else {
            offset = events.findIndex((event) => event.ledger >= (request.startLedger ?? 0));
            if (offset === -1) offset = events.length;
          }
          return { events: events.slice(offset, offset + limit), latestLedger: 500 };
        },
      } as any,
    });

    await subscriber.pollEvents();

    assert.strictEqual(mockDb.state.stored.length, 250, "all 250 events must be stored");
    assert.strictEqual(new Set(mockDb.state.stored).size, 250, "no event may be stored twice");
    assert.strictEqual(requests.length, 3, "one request per page");
    assert.strictEqual(requests[0].cursor, undefined);
    assert.strictEqual(requests[1].cursor, "cursor-100");
    assert.strictEqual(requests[2].cursor, "cursor-200");

    // The next cycle starts after the last processed ledger and finds nothing.
    const before = mockDb.state.storeAttempts;
    await subscriber.pollEvents();
    assert.strictEqual(requests.length, 4);
    assert.strictEqual(requests[3].startLedger, 501);
    assert.strictEqual(mockDb.state.storeAttempts, before, "no duplicate work after a drain");
  });

  it("keeps polling after a failed cycle without advancing the cursor", async () => {
    const mockDb = createMockDatabase();
    const requests: Array<{ startLedger?: number }> = [];
    let failNext = true;
    const loggedErrors: unknown[][] = [];
    const originalConsoleError = console.error;
    console.error = (...args: unknown[]) => {
      loggedErrors.push(args);
    };

    const subscriber = new EventSubscriber(baseConfig({ pollInterval: 10 }), mockDb, {
      eventSource: {
        async getEvents(request: { startLedger?: number }) {
          requests.push({ ...request });
          if (failNext) {
            failNext = false;
            throw new Error("RPC unavailable");
          }
          const startLedger = request.startLedger ?? 0;
          const page = [rpcEvent(10, 1), rpcEvent(11, 2)].filter(
            (event) => event.ledger >= startLedger,
          );
          return { events: page, latestLedger: 11 };
        },
      } as any,
    });

    try {
      await subscriber.start();
      await delay(DELAY_MS * 6);
    } finally {
      await subscriber.shutdown();
      console.error = originalConsoleError;
    }

    assert.strictEqual(requests.length >= 2, true, "the subscriber must poll again after a failure");
    assert.strictEqual(
      requests[1].startLedger,
      1,
      "a failed cycle must not advance the ledger cursor",
    );
    assert.deepStrictEqual(
      mockDb.state.stored,
      ["10-tx-1-evt-1", "11-tx-2-evt-2"],
      "events from the recovered cycle must be processed",
    );
    assert.strictEqual(
      loggedErrors.some((args) => String(args[0]).includes("Error polling events")),
      true,
      "the failure must be surfaced through the logger",
    );
  });

  test("does not advance the cursor when a batch fails midway", async () => {
    const state = { stored: [] as string[], fail: true };
    const mockDb = {
      state,
      async initialize() {},
      async close() {},
      async storeEvent(event: { id: string }) {
        // Fail the event write *and* the dead-letter write so the batch itself
        // cannot complete and the cursor must stay put.
        if (state.fail) throw new Error("database write failed");
        if (state.stored.includes(event.id)) return false;
        state.stored.push(event.id);
        return true;
      },
      async storeDeadLetterEvent() {
        if (state.fail) throw new Error("dead-letter write failed");
      },
    } as unknown as Database & { state: typeof state };
    const requests: Array<{ startLedger?: number; cursor?: string }> = [];
    const events = [rpcEvent(20, 1), rpcEvent(21, 2)];

    const subscriber = new EventSubscriber(baseConfig(), mockDb, {
      eventSource: {
        async getEvents(request: { startLedger?: number; cursor?: string }) {
          requests.push({ ...request });
          const startLedger = request.startLedger ?? 0;
          return {
            events: events.filter((event) => event.ledger >= startLedger),
            latestLedger: 21,
          };
        },
      } as any,
    });

    await assert.rejects(() => subscriber.pollEvents(), /write failed/);

    // The batch never completed, so the cursor must be re-read from the same
    // ledger rather than skipping the events it still owes.
    state.fail = false;
    requests.length = 0;
    await subscriber.pollEvents();

    assert.strictEqual(requests[0].startLedger, 1);
    assert.deepStrictEqual(
      mockDb.state.stored,
      ["20-tx-1-evt-1", "21-tx-2-evt-2"],
      "the retried batch must process every event",
    );
  });
});
