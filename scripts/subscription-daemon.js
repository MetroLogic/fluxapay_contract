#!/usr/bin/env node
/**
 * FluxaPay Subscription Indexer Daemon
 *
 * Polls the chain for due subscriptions and calls charge_subscription / process_due_subscriptions
 * on the RefundManager contract. Designed to run as a cron job or long-running process.
 *
 * Issue #776: Handles Soroban RPC timeouts during network congestion:
 * - Persists transaction hash to local retry queue upon RPC timeout
 * - Dedicated poller checks Horizon for transaction status
 * - Re-submits with keypair and sequence guard if not_found after ledger close window
 * - Maximum 3 retry attempts with exponential backoff
 * - Emits charge.failed webhook and logs ERROR upon retry exhaustion
 *
 * Usage:
 *   node scripts/subscription-daemon.js
 *
 * Environment variables:
 *   STELLAR_RPC_URL        – Soroban RPC endpoint
 *   HORIZON_URL            – Stellar Horizon API endpoint
 *   CONTRACT_ID            – RefundManager contract address
 *   OPERATOR_SECRET        – Operator secret key (settlement_operator role)
 *   POLL_INTERVAL_MS       – How often to poll in ms (default: 60000)
 *   NETWORK_PASSPHRASE     – Stellar network passphrase
 *   RETRY_QUEUE_PATH       – File path for local retry queue persistence
 *   WEBHOOK_URL            – URL to dispatch charge.failed webhooks
 */

"use strict";

const fs = require("fs");
const path = require("path");
const {
  SorobanRpc,
  TransactionBuilder,
  Networks,
  Keypair,
  Contract,
  nativeToScVal,
  BASE_FEE,
  xdr,
} = require("@stellar/stellar-sdk");

const RPC_URL = process.env.STELLAR_RPC_URL || "https://soroban-testnet.stellar.org";
const HORIZON_URL = process.env.HORIZON_URL || "https://horizon-testnet.stellar.org";
const CONTRACT_ID = process.env.CONTRACT_ID;
const OPERATOR_SECRET = process.env.OPERATOR_SECRET;
const POLL_INTERVAL_MS = parseInt(process.env.POLL_INTERVAL_MS || "60000", 10);
const NETWORK_PASSPHRASE = process.env.NETWORK_PASSPHRASE || Networks.TESTNET;
const RETRY_QUEUE_PATH =
  process.env.RETRY_QUEUE_PATH ||
  path.join(process.cwd(), "subscription_retry_queue.json");
const WEBHOOK_URL = process.env.WEBHOOK_URL || null;
const MAX_RETRY_ATTEMPTS = 3;
const BASE_BACKOFF_MS = 1000;
const LEDGER_CLOSE_WINDOW_MS = 30000; // ~30 seconds ledger close window

// Lazily initialized client objects
function getServer(rpcUrl = RPC_URL) {
  return new SorobanRpc.Server(rpcUrl, { allowHttp: rpcUrl.startsWith("http://") });
}

function getOperatorKeypair(secret = OPERATOR_SECRET) {
  if (!secret) return null;
  return Keypair.fromSecret(secret);
}

function getContract(contractId = CONTRACT_ID) {
  if (!contractId) return null;
  return new Contract(contractId);
}

/**
 * Load persisted retry queue from disk.
 */
function loadRetryQueue(queuePath = RETRY_QUEUE_PATH) {
  try {
    if (fs.existsSync(queuePath)) {
      const data = fs.readFileSync(queuePath, "utf8");
      return JSON.parse(data);
    }
  } catch (err) {
    console.error(`[daemon] Error reading retry queue from ${queuePath}:`, err.message);
  }
  return [];
}

/**
 * Persist retry queue to disk.
 */
function saveRetryQueue(queue, queuePath = RETRY_QUEUE_PATH) {
  try {
    fs.writeFileSync(queuePath, JSON.stringify(queue, null, 2), "utf8");
  } catch (err) {
    console.error(`[daemon] Error writing retry queue to ${queuePath}:`, err.message);
  }
}

/**
 * Add or update an entry in the retry queue.
 */
function addToRetryQueue(record, queuePath = RETRY_QUEUE_PATH) {
  const queue = loadRetryQueue(queuePath);
  const existingIdx = queue.findIndex(
    (item) =>
      item.id === record.id ||
      (record.subscription_id && item.subscription_id === record.subscription_id)
  );

  if (existingIdx >= 0) {
    queue[existingIdx] = { ...queue[existingIdx], ...record };
  } else {
    queue.push(record);
  }
  saveRetryQueue(queue, queuePath);
  return queue;
}

/**
 * Remove an entry from the retry queue.
 */
function removeFromRetryQueue(identifier, queuePath = RETRY_QUEUE_PATH) {
  const queue = loadRetryQueue(queuePath);
  const updated = queue.filter(
    (item) =>
      item.id !== identifier &&
      item.transaction_hash !== identifier &&
      item.subscription_id !== identifier
  );
  saveRetryQueue(updated, queuePath);
  return updated;
}

/**
 * Emit a webhook (e.g. charge.failed) when retry attempts are exhausted.
 */
async function emitWebhook(eventType, payload, webhookUrl = WEBHOOK_URL) {
  const body = {
    event: eventType,
    timestamp: new Date().toISOString(),
    data: payload,
  };

  if (!webhookUrl) {
    console.log(`[daemon] [Webhook: ${eventType}]`, JSON.stringify(body));
    return { delivered: false, simulated: true, body };
  }

  try {
    const res = await fetch(webhookUrl, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    return { delivered: res.ok, status: res.status, body };
  } catch (err) {
    console.error(`[daemon] Failed to send webhook to ${webhookUrl}:`, err.message);
    return { delivered: false, error: err.message, body };
  }
}

/**
 * Check transaction status on Horizon.
 * Returns: { status: "SUCCESS" | "FAILED" | "NOT_FOUND", data: any }
 */
async function checkHorizonTransaction(txHash, horizonUrl = HORIZON_URL, fetchFn = global.fetch) {
  try {
    const res = await fetchFn(`${horizonUrl}/transactions/${txHash}`);
    if (res.status === 404) {
      return { status: "NOT_FOUND" };
    }
    if (!res.ok) {
      return { status: "ERROR", statusCode: res.status };
    }
    const data = await res.json();
    if (data.successful) {
      return { status: "SUCCESS", data };
    } else {
      return { status: "FAILED", data };
    }
  } catch (err) {
    return { status: "NETWORK_ERROR", error: err.message };
  }
}

/**
 * Poll Horizon for all transactions in the retry queue and handle re-submissions.
 */
async function pollRetryQueue(options = {}) {
  const queuePath = options.queuePath || RETRY_QUEUE_PATH;
  const horizonUrl = options.horizonUrl || HORIZON_URL;
  const maxRetries = options.maxRetries || MAX_RETRY_ATTEMPTS;
  const baseBackoffMs = options.baseBackoffMs || BASE_BACKOFF_MS;
  const ledgerCloseWindowMs = options.ledgerCloseWindowMs || LEDGER_CLOSE_WINDOW_MS;
  const fetchFn = options.fetchFn || global.fetch;
  const reSubmitFn = options.reSubmitFn || chargeSubscriptionInternal;

  const queue = loadRetryQueue(queuePath);
  if (queue.length === 0) return { processed: 0, reSubmitted: 0, completed: 0, failed: 0 };

  let reSubmitted = 0;
  let completed = 0;
  let failed = 0;
  const now = Date.now();

  for (const record of queue) {
    // Check Horizon for transaction confirmation
    const horizonStatus = await checkHorizonTransaction(
      record.transaction_hash,
      horizonUrl,
      fetchFn
    );

    if (horizonStatus.status === "SUCCESS") {
      console.log(
        `[daemon] Confirmed transaction ${record.transaction_hash} on Horizon for subscription ${record.subscription_id}. Charge succeeded.`
      );
      removeFromRetryQueue(record.id, queuePath);
      completed++;
      continue;
    }

    if (horizonStatus.status === "FAILED") {
      console.error(
        `[daemon] ERROR: Transaction ${record.transaction_hash} failed on-chain for subscription ${record.subscription_id}.`
      );
      await emitWebhook("charge.failed", {
        subscription_id: record.subscription_id,
        transaction_hash: record.transaction_hash,
        attempts: record.attempts,
        reason: "Transaction failed on ledger",
      });
      removeFromRetryQueue(record.id, queuePath);
      failed++;
      continue;
    }

    // If still not_found after ledger close window, check retry eligibility
    if (horizonStatus.status === "NOT_FOUND") {
      const elapsedSinceAttempt = now - record.last_attempt_at;

      if (elapsedSinceAttempt >= ledgerCloseWindowMs && now >= (record.next_retry_at || 0)) {
        if (record.attempts < maxRetries) {
          console.log(
            `[daemon] Transaction ${record.transaction_hash} not found after ${Math.round(elapsedSinceAttempt / 1000)}s. Retrying charge for subscription ${record.subscription_id} (attempt ${record.attempts + 1}/${maxRetries})...`
          );

          try {
            const reSubmitResult = await reSubmitFn(record.subscription_id);
            const newAttempts = record.attempts + 1;
            const backoff = baseBackoffMs * Math.pow(2, newAttempts - 1);

            addToRetryQueue(
              {
                ...record,
                transaction_hash: reSubmitResult.hash || record.transaction_hash,
                attempts: newAttempts,
                last_attempt_at: now,
                next_retry_at: now + backoff,
              },
              queuePath
            );
            reSubmitted++;
          } catch (err) {
            console.error(
              `[daemon] Re-submission attempt ${record.attempts + 1} failed:`,
              err.message
            );
          }
        } else {
          // Max retries exhausted
          console.error(
            `[daemon] ERROR: Max retries (${maxRetries}) exhausted for subscription ${record.subscription_id}. Emitting charge.failed webhook.`
          );
          await emitWebhook("charge.failed", {
            subscription_id: record.subscription_id,
            transaction_hash: record.transaction_hash,
            attempts: record.attempts,
            reason: "RPC timeout retry exhausted (not included in ledger)",
          });
          removeFromRetryQueue(record.id, queuePath);
          failed++;
        }
      }
    }
  }

  return { processed: queue.length, reSubmitted, completed, failed };
}

/**
 * Fetch all active subscription IDs from contract storage / local fallback.
 */
async function fetchDueSubscriptionIds() {
  const fsModule = require("fs");
  const indexPath = process.env.SUBSCRIPTION_INDEX_PATH || "/tmp/fluxapay_subscriptions.json";

  if (!fsModule.existsSync(indexPath)) {
    console.warn(`[daemon] No subscription index found at ${indexPath}. Skipping cycle.`);
    return [];
  }

  const index = JSON.parse(fsModule.readFileSync(indexPath, "utf8"));
  const nowSecs = Math.floor(Date.now() / 1000);

  return (index.subscriptions || [])
    .filter(
      (s) =>
        s.status === "Active" &&
        (s.next_payment_at <= nowSecs || (s.next_retry_at && s.next_retry_at <= nowSecs))
    )
    .map((s) => s.subscription_id);
}

/**
 * Charge an individual subscription with RPC timeout resilience.
 */
async function chargeSubscription(subscriptionId, customServer = null, customKeypair = null) {
  return chargeSubscriptionInternal(subscriptionId, customServer, customKeypair);
}

async function chargeSubscriptionInternal(
  subscriptionId,
  customServer = null,
  customKeypair = null
) {
  const rpcServer = customServer || getServer();
  const operator = customKeypair || getOperatorKeypair();
  const contractInstance = getContract();

  if (!operator) {
    throw new Error("OPERATOR_SECRET must be set to charge subscription");
  }
  if (!contractInstance) {
    throw new Error("CONTRACT_ID must be set to charge subscription");
  }

  let txHash = null;

  try {
    const account = await rpcServer.getAccount(operator.publicKey());

    const tx = new TransactionBuilder(account, {
      fee: BASE_FEE,
      networkPassphrase: NETWORK_PASSPHRASE,
    })
      .addOperation(
        contractInstance.call(
          "charge_subscription",
          nativeToScVal(operator.publicKey(), { type: "address" }),
          nativeToScVal(subscriptionId, { type: "string" })
        )
      )
      .setTimeout(30)
      .build();

    const preparedTx = await rpcServer.prepareTransaction(tx);
    preparedTx.sign(operator);

    const sendResult = await rpcServer.sendTransaction(preparedTx);
    if (sendResult.status === "ERROR") {
      throw new Error(`Transaction submission error: ${JSON.stringify(sendResult.errorResult)}`);
    }

    txHash = sendResult.hash;

    // Poll RPC for confirmation
    let getResult = null;
    let timedOut = true;

    for (let i = 0; i < 10; i++) {
      await sleep(3000);
      try {
        getResult = await rpcServer.getTransaction(txHash);
        if (getResult.status !== "NOT_FOUND") {
          timedOut = false;
          break;
        }
      } catch (err) {
        // RPC network or timeout error during getTransaction
        console.warn(`[daemon] RPC poll attempt ${i + 1} timed out / errored: ${err.message}`);
      }
    }

    if (timedOut || !getResult || getResult.status === "NOT_FOUND") {
      // RPC Timeout encountered: persist to local retry queue
      console.warn(
        `[daemon] RPC timeout encountered for subscription ${subscriptionId}. Persisting retry record for tx ${txHash}...`
      );
      addToRetryQueue({
        id: `sub_${subscriptionId}`,
        subscription_id: subscriptionId,
        transaction_hash: txHash,
        attempts: 1,
        first_attempt_at: Date.now(),
        last_attempt_at: Date.now(),
        next_retry_at: Date.now() + BASE_BACKOFF_MS,
        status: "pending_retry",
      });
      return { status: "TIMEOUT_QUEUED", hash: txHash };
    }

    if (getResult.status === "SUCCESS") {
      removeFromRetryQueue(`sub_${subscriptionId}`);
      return { status: "SUCCESS", hash: txHash, returnValue: getResult.returnValue };
    }

    throw new Error(`Transaction failed on ledger with status: ${getResult.status}`);
  } catch (err) {
    const isTimeout =
      err.message?.toLowerCase().includes("timeout") ||
      err.message?.toLowerCase().includes("etimedout") ||
      err.code === "ETIMEDOUT" ||
      err.code === "ECONNABORTED";

    if (isTimeout && txHash) {
      console.warn(
        `[daemon] Network/RPC timeout caught for subscription ${subscriptionId}. Adding tx ${txHash} to retry queue.`
      );
      addToRetryQueue({
        id: `sub_${subscriptionId}`,
        subscription_id: subscriptionId,
        transaction_hash: txHash,
        attempts: 1,
        first_attempt_at: Date.now(),
        last_attempt_at: Date.now(),
        next_retry_at: Date.now() + BASE_BACKOFF_MS,
        status: "pending_retry",
      });
      return { status: "TIMEOUT_QUEUED", hash: txHash };
    }

    throw err;
  }
}

/**
 * Call process_due_subscriptions on the contract with timeout resilience.
 */
async function triggerProcessDue(customServer = null, customKeypair = null) {
  const rpcServer = customServer || getServer();
  const operator = customKeypair || getOperatorKeypair();
  const contractInstance = getContract();

  if (!operator) {
    throw new Error("OPERATOR_SECRET must be set");
  }
  if (!contractInstance) {
    throw new Error("CONTRACT_ID must be set");
  }

  let txHash = null;

  try {
    const account = await rpcServer.getAccount(operator.publicKey());

    const tx = new TransactionBuilder(account, {
      fee: BASE_FEE,
      networkPassphrase: NETWORK_PASSPHRASE,
    })
      .addOperation(
        contractInstance.call(
          "process_due_subscriptions",
          nativeToScVal(operator.publicKey(), { type: "address" })
        )
      )
      .setTimeout(30)
      .build();

    const preparedTx = await rpcServer.prepareTransaction(tx);
    preparedTx.sign(operator);

    const sendResult = await rpcServer.sendTransaction(preparedTx);
    if (sendResult.status === "ERROR") {
      throw new Error(`Transaction failed: ${JSON.stringify(sendResult.errorResult)}`);
    }

    txHash = sendResult.hash;

    // Poll for confirmation.
    let getResult = null;
    let timedOut = true;

    for (let i = 0; i < 10; i++) {
      await sleep(3000);
      try {
        getResult = await rpcServer.getTransaction(txHash);
        if (getResult.status !== "NOT_FOUND") {
          timedOut = false;
          break;
        }
      } catch (err) {
        console.warn(`[daemon] RPC poll attempt ${i + 1} timed out / errored: ${err.message}`);
      }
    }

    if (timedOut || !getResult || getResult.status === "NOT_FOUND") {
      console.warn(
        `[daemon] RPC timeout encountered for process_due_subscriptions. Persisting tx ${txHash} to retry queue.`
      );
      addToRetryQueue({
        id: `process_due_${Date.now()}`,
        subscription_id: "process_due_batch",
        transaction_hash: txHash,
        attempts: 1,
        first_attempt_at: Date.now(),
        last_attempt_at: Date.now(),
        next_retry_at: Date.now() + BASE_BACKOFF_MS,
        status: "pending_retry",
      });
      return { status: "TIMEOUT_QUEUED", hash: txHash, processed: 0 };
    }

    if (getResult.status === "SUCCESS") {
      const processed = getResult.returnValue
        ? parseInt(getResult.returnValue.value(), 10)
        : 0;
      return { status: "SUCCESS", hash: txHash, processed };
    }

    throw new Error(`Transaction did not succeed: ${getResult.status}`);
  } catch (err) {
    const isTimeout =
      err.message?.toLowerCase().includes("timeout") ||
      err.message?.toLowerCase().includes("etimedout") ||
      err.code === "ETIMEDOUT";

    if (isTimeout && txHash) {
      console.warn(
        `[daemon] RPC timeout during process_due_subscriptions. Adding tx ${txHash} to retry queue.`
      );
      addToRetryQueue({
        id: `process_due_${Date.now()}`,
        subscription_id: "process_due_batch",
        transaction_hash: txHash,
        attempts: 1,
        first_attempt_at: Date.now(),
        last_attempt_at: Date.now(),
        next_retry_at: Date.now() + BASE_BACKOFF_MS,
        status: "pending_retry",
      });
      return { status: "TIMEOUT_QUEUED", hash: txHash, processed: 0 };
    }
    throw err;
  }
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function runCycle() {
  console.log(`[daemon] ${new Date().toISOString()} – starting cycle`);
  try {
    // 1. Process pending items in retry queue via Horizon poller
    const retryStats = await pollRetryQueue();
    if (retryStats.processed > 0) {
      console.log(
        `[daemon] Polled retry queue: ${retryStats.processed} items (${retryStats.completed} completed, ${retryStats.reSubmitted} re-submitted, ${retryStats.failed} failed).`
      );
    }

    // 2. Poll due subscriptions
    const dueIds = await fetchDueSubscriptionIds();
    if (dueIds.length === 0) {
      console.log("[daemon] No due subscriptions found.");
      return;
    }
    console.log(`[daemon] Found ${dueIds.length} due subscription(s). Triggering contract call…`);
    const result = await triggerProcessDue();
    console.log(`[daemon] process_due_subscriptions returned: ${result.processed || 0} processed.`);
  } catch (err) {
    console.error("[daemon] Cycle error:", err.message || err);
  }
}

async function main() {
  if (!CONTRACT_ID || !OPERATOR_SECRET) {
    console.error("ERROR: CONTRACT_ID and OPERATOR_SECRET must be set.");
    process.exit(1);
  }

  const operator = getOperatorKeypair();
  console.log(`[daemon] Starting FluxaPay subscription daemon (poll every ${POLL_INTERVAL_MS}ms)`);
  console.log(`[daemon] Contract: ${CONTRACT_ID}`);
  console.log(`[daemon] Operator: ${operator.publicKey()}`);
  console.log(`[daemon] Retry Queue: ${RETRY_QUEUE_PATH}`);

  // Run immediately, then on interval.
  await runCycle();
  setInterval(runCycle, POLL_INTERVAL_MS);
}

if (require.main === module) {
  main().catch((err) => {
    console.error("[daemon] Fatal:", err);
    process.exit(1);
  });
}

module.exports = {
  fetchDueSubscriptionIds,
  triggerProcessDue,
  chargeSubscription,
  loadRetryQueue,
  saveRetryQueue,
  addToRetryQueue,
  removeFromRetryQueue,
  emitWebhook,
  checkHorizonTransaction,
  pollRetryQueue,
  runCycle,
  MAX_RETRY_ATTEMPTS,
  BASE_BACKOFF_MS,
  LEDGER_CLOSE_WINDOW_MS,
};
