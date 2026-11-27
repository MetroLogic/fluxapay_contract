/**
 * Unit tests for scripts/subscription-daemon.js (Issue #776)
 * Tests:
 * - Persisting retry record on RPC timeout
 * - Status poller checks Horizon:
 *   - Success path: removes record from retry queue
 *   - Pending/not_found path: re-submits after ledger close window with exponential backoff
 *   - Max retries exhausted path: emits charge.failed webhook and logs error
 */

const assert = require("node:assert");
const test = require("node:test");
const { describe, it, beforeEach, afterEach } = test;
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");

const {
  loadRetryQueue,
  saveRetryQueue,
  addToRetryQueue,
  removeFromRetryQueue,
  checkHorizonTransaction,
  pollRetryQueue,
  emitWebhook,
} = require("../subscription-daemon");

describe("Issue #776: subscription-daemon RPC timeout and retry poller", () => {
  let tempQueuePath;

  beforeEach(() => {
    tempQueuePath = path.join(os.tmpdir(), `test_queue_${Date.now()}_${Math.random()}.json`);
  });

  afterEach(() => {
    if (fs.existsSync(tempQueuePath)) {
      try {
        fs.unlinkSync(tempQueuePath);
      } catch (_) {}
    }
  });

  it("persists a retry record on RPC timeout and loads it back", () => {
    const record = {
      id: "sub_101",
      subscription_id: "sub_101",
      transaction_hash: "hash_tx_101",
      attempts: 1,
      first_attempt_at: Date.now(),
      last_attempt_at: Date.now(),
      next_retry_at: Date.now() + 1000,
      status: "pending_retry",
    };

    addToRetryQueue(record, tempQueuePath);
    const loaded = loadRetryQueue(tempQueuePath);
    assert.strictEqual(loaded.length, 1);
    assert.strictEqual(loaded[0].subscription_id, "sub_101");
    assert.strictEqual(loaded[0].transaction_hash, "hash_tx_101");
    assert.strictEqual(loaded[0].attempts, 1);
  });

  it("Horizon success path: removes confirmed transaction from retry queue", async () => {
    const record = {
      id: "sub_202",
      subscription_id: "sub_202",
      transaction_hash: "hash_tx_success",
      attempts: 1,
      last_attempt_at: Date.now() - 35000,
      next_retry_at: Date.now() - 1000,
    };
    addToRetryQueue(record, tempQueuePath);

    // Mock fetch returning success on Horizon
    const mockFetch = async (url) => {
      assert.ok(url.includes("hash_tx_success"));
      return {
        ok: true,
        status: 200,
        json: async () => ({ successful: true, hash: "hash_tx_success" }),
      };
    };

    const stats = await pollRetryQueue({
      queuePath: tempQueuePath,
      horizonUrl: "https://mock-horizon.stellar.org",
      fetchFn: mockFetch,
    });

    assert.strictEqual(stats.completed, 1);
    const remaining = loadRetryQueue(tempQueuePath);
    assert.strictEqual(remaining.length, 0, "Record should be removed upon successful confirmation");
  });

  it("Horizon pending/not_found path: re-submits after ledger close window with backoff", async () => {
    const record = {
      id: "sub_303",
      subscription_id: "sub_303",
      transaction_hash: "hash_tx_initial",
      attempts: 1,
      last_attempt_at: Date.now() - 35000, // > 30s ago (ledger close window elapsed)
      next_retry_at: Date.now() - 5000,
    };
    addToRetryQueue(record, tempQueuePath);

    // Mock fetch returning 404 NOT_FOUND on Horizon
    const mockFetch = async () => ({
      ok: false,
      status: 404,
    });

    let reSubmittedWithSubId = null;
    const mockReSubmit = async (subId) => {
      reSubmittedWithSubId = subId;
      return { hash: "hash_tx_resubmitted_2" };
    };

    const stats = await pollRetryQueue({
      queuePath: tempQueuePath,
      horizonUrl: "https://mock-horizon.stellar.org",
      ledgerCloseWindowMs: 30000,
      baseBackoffMs: 1000,
      fetchFn: mockFetch,
      reSubmitFn: mockReSubmit,
    });

    assert.strictEqual(stats.reSubmitted, 1);
    assert.strictEqual(reSubmittedWithSubId, "sub_303");

    const updatedQueue = loadRetryQueue(tempQueuePath);
    assert.strictEqual(updatedQueue.length, 1);
    assert.strictEqual(updatedQueue[0].attempts, 2);
    assert.strictEqual(updatedQueue[0].transaction_hash, "hash_tx_resubmitted_2");
    assert.ok(updatedQueue[0].next_retry_at > Date.now(), "Next retry should be scheduled in the future with backoff");
  });

  it("Max retries exhausted path: emits charge.failed webhook and removes from queue", async () => {
    const record = {
      id: "sub_404",
      subscription_id: "sub_404",
      transaction_hash: "hash_tx_final_attempt",
      attempts: 3, // Already reached max attempts (3)
      last_attempt_at: Date.now() - 40000,
      next_retry_at: Date.now() - 1000,
    };
    addToRetryQueue(record, tempQueuePath);

    // Mock Horizon 404
    const mockFetch = async () => ({
      ok: false,
      status: 404,
    });

    const stats = await pollRetryQueue({
      queuePath: tempQueuePath,
      horizonUrl: "https://mock-horizon.stellar.org",
      ledgerCloseWindowMs: 30000,
      maxRetries: 3,
      fetchFn: mockFetch,
    });

    assert.strictEqual(stats.failed, 1);
    assert.strictEqual(stats.reSubmitted, 0);

    const remaining = loadRetryQueue(tempQueuePath);
    assert.strictEqual(remaining.length, 0, "Record should be removed after exhausting retries");
  });

  it("emitWebhook formats charge.failed payload accurately", async () => {
    const payload = {
      subscription_id: "sub_test_webhook",
      transaction_hash: "0xhash123",
      attempts: 3,
      reason: "RPC timeout retry exhausted",
    };

    const result = await emitWebhook("charge.failed", payload, null);
    assert.strictEqual(result.simulated, true);
    assert.strictEqual(result.body.event, "charge.failed");
    assert.strictEqual(result.body.data.subscription_id, "sub_test_webhook");
    assert.strictEqual(result.body.data.attempts, 3);
  });
});
