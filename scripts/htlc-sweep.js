#!/usr/bin/env node
/**
 * FluxaPay HTLC Timeout Sweep Daemon
 *
 * Issue #1021: expired HTLCs (`timeout_ledger` has passed, status still
 * `Active`) sit locked forever unless someone calls `claim_htlc_timeout` on
 * the `Htlc` contract. That entry point is intentionally permissionless -
 * it requires no signature from the original sender or receiver, and always
 * refunds the sender - so this daemon can sweep them on a schedule using
 * only its own funding keypair to pay the transaction fee.
 *
 * HTLC ids to check are read from a local JSON index (see
 * `HTLC_INDEX_PATH` below) rather than scanned on-chain, since this contract
 * has no "list all HTLCs" entry point. `create_htlc` callers are expected to
 * append `{ id, timeout_ledger }` to that index when they create an HTLC;
 * the daemon removes an id from the index once it has been swept (or found
 * already resolved).
 *
 * Usage:
 *   node scripts/htlc-sweep.js
 *   node scripts/htlc-sweep.js --once   # run a single cycle and exit
 *
 * Environment variables:
 *   STELLAR_RPC_URL   – Soroban RPC endpoint (default: soroban-testnet.stellar.org)
 *   HTLC_CONTRACT_ID  – Deployed `Htlc` contract address (required)
 *   SWEEP_SECRET_KEY  – Funding keypair that pays the sweep transaction fee (required)
 *   POLL_INTERVAL_MS  – How often to poll in ms (default: 60000)
 *   NETWORK_PASSPHRASE – Stellar network passphrase (default: Networks.TESTNET)
 *   HTLC_INDEX_PATH   – Path to the local JSON index of tracked HTLC ids
 *                        (default: /tmp/fluxapay_htlcs.json)
 */

"use strict";

const fs = require("fs");
const {
  SorobanRpc,
  TransactionBuilder,
  Networks,
  Keypair,
  Contract,
  nativeToScVal,
  BASE_FEE,
} = require("@stellar/stellar-sdk");

const RPC_URL = process.env.STELLAR_RPC_URL || "https://soroban-testnet.stellar.org";
const HTLC_CONTRACT_ID = process.env.HTLC_CONTRACT_ID;
const SWEEP_SECRET_KEY = process.env.SWEEP_SECRET_KEY;
const POLL_INTERVAL_MS = parseInt(process.env.POLL_INTERVAL_MS || "60000", 10);
const NETWORK_PASSPHRASE = process.env.NETWORK_PASSPHRASE || Networks.TESTNET;
const HTLC_INDEX_PATH = process.env.HTLC_INDEX_PATH || "/tmp/fluxapay_htlcs.json";
const CONFIRMATION_POLL_ATTEMPTS = 10;
const CONFIRMATION_POLL_DELAY_MS = 3000;

function getServer(rpcUrl = RPC_URL) {
  return new SorobanRpc.Server(rpcUrl, { allowHttp: rpcUrl.startsWith("http://") });
}

function getSweeperKeypair(secret = SWEEP_SECRET_KEY) {
  if (!secret) return null;
  return Keypair.fromSecret(secret);
}

function getContract(contractId = HTLC_CONTRACT_ID) {
  if (!contractId) return null;
  return new Contract(contractId);
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Load the local index of tracked HTLCs: `{ htlcs: [{ id, timeout_ledger, status }] }`.
 */
function loadIndex(indexPath = HTLC_INDEX_PATH) {
  try {
    if (fs.existsSync(indexPath)) {
      const data = fs.readFileSync(indexPath, "utf8");
      return JSON.parse(data);
    }
  } catch (err) {
    console.error(`[htlc-sweep] Error reading index from ${indexPath}:`, err.message);
  }
  return { htlcs: [] };
}

function saveIndex(index, indexPath = HTLC_INDEX_PATH) {
  try {
    fs.writeFileSync(indexPath, JSON.stringify(index, null, 2), "utf8");
  } catch (err) {
    console.error(`[htlc-sweep] Error writing index to ${indexPath}:`, err.message);
  }
}

/**
 * Remove an id from the local index (it has been resolved one way or another).
 */
function removeFromIndex(htlcId, indexPath = HTLC_INDEX_PATH) {
  const index = loadIndex(indexPath);
  index.htlcs = (index.htlcs || []).filter((entry) => entry.id !== htlcId);
  saveIndex(index, indexPath);
  return index;
}

/**
 * Returns ids from the local index whose `timeout_ledger` is <= the given
 * current ledger sequence and that are still marked `Active` locally.
 * (The on-chain status is the source of truth; this is just a candidate
 * filter so the daemon doesn't have to submit a transaction for every
 * tracked HTLC on every cycle.)
 */
function findExpiredCandidates(currentLedgerSeq, indexPath = HTLC_INDEX_PATH) {
  const index = loadIndex(indexPath);
  return (index.htlcs || [])
    .filter((entry) => (entry.status || "Active") === "Active")
    .filter((entry) => entry.timeout_ledger <= currentLedgerSeq)
    .map((entry) => entry.id);
}

/**
 * Submits `claim_htlc_timeout(htlc_id)` and waits for confirmation.
 * Returns `{ status: "SUCCESS" | "ALREADY_RESOLVED" | "NOT_EXPIRED" | "ERROR", hash }`.
 */
async function sweepHtlc(htlcId, customServer = null, customKeypair = null) {
  const rpcServer = customServer || getServer();
  const sweeper = customKeypair || getSweeperKeypair();
  const contractInstance = getContract();

  if (!sweeper) {
    throw new Error("SWEEP_SECRET_KEY must be set to sweep HTLCs");
  }
  if (!contractInstance) {
    throw new Error("HTLC_CONTRACT_ID must be set to sweep HTLCs");
  }

  const account = await rpcServer.getAccount(sweeper.publicKey());

  const tx = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(
      // claim_htlc_timeout(env, htlc_id) - no caller/auth argument. The
      // sweeper keypair only funds and signs the transaction envelope;
      // the contract itself authorizes moving its own escrowed balance.
      contractInstance.call("claim_htlc_timeout", nativeToScVal(htlcId, { type: "string" }))
    )
    .setTimeout(30)
    .build();

  let preparedTx;
  try {
    preparedTx = await rpcServer.prepareTransaction(tx);
  } catch (err) {
    const message = String(err.message || err);
    if (message.includes("NotExpired")) {
      return { status: "NOT_EXPIRED", error: message };
    }
    if (message.includes("AlreadyClaimed") || message.includes("NotFound")) {
      return { status: "ALREADY_RESOLVED", error: message };
    }
    throw err;
  }

  preparedTx.sign(sweeper);
  const sendResult = await rpcServer.sendTransaction(preparedTx);
  if (sendResult.status === "ERROR") {
    throw new Error(`Transaction submission error: ${JSON.stringify(sendResult.errorResult)}`);
  }

  const txHash = sendResult.hash;

  let getResult = null;
  for (let i = 0; i < CONFIRMATION_POLL_ATTEMPTS; i++) {
    await sleep(CONFIRMATION_POLL_DELAY_MS);
    try {
      getResult = await rpcServer.getTransaction(txHash);
      if (getResult.status !== "NOT_FOUND") break;
    } catch (err) {
      console.warn(`[htlc-sweep] RPC poll attempt ${i + 1} errored: ${err.message}`);
    }
  }

  if (!getResult || getResult.status === "NOT_FOUND") {
    return { status: "TIMEOUT_UNCONFIRMED", hash: txHash };
  }
  if (getResult.status === "SUCCESS") {
    return { status: "SUCCESS", hash: txHash };
  }
  return { status: "ERROR", hash: txHash, raw: getResult.status };
}

async function runCycle(options = {}) {
  const indexPath = options.indexPath || HTLC_INDEX_PATH;
  const rpcServer = options.server || getServer();

  console.log(`[htlc-sweep] ${new Date().toISOString()} - starting cycle`);

  let currentLedgerSeq;
  try {
    const latest = await rpcServer.getLatestLedger();
    currentLedgerSeq = latest.sequence;
  } catch (err) {
    console.error("[htlc-sweep] Failed to fetch latest ledger sequence:", err.message);
    return { swept: 0, alreadyResolved: 0, errors: 1 };
  }

  const candidates = findExpiredCandidates(currentLedgerSeq, indexPath);
  if (candidates.length === 0) {
    console.log(`[htlc-sweep] No expired HTLCs at ledger ${currentLedgerSeq}.`);
    return { swept: 0, alreadyResolved: 0, errors: 0 };
  }

  console.log(
    `[htlc-sweep] Found ${candidates.length} expired HTLC(s) at ledger ${currentLedgerSeq}. Sweeping...`
  );

  let swept = 0;
  let alreadyResolved = 0;
  let errors = 0;

  for (const htlcId of candidates) {
    try {
      const result = await sweepHtlc(htlcId, rpcServer);
      if (result.status === "SUCCESS") {
        console.log(`[htlc-sweep] Swept ${htlcId} (tx ${result.hash}).`);
        removeFromIndex(htlcId, indexPath);
        swept++;
      } else if (result.status === "ALREADY_RESOLVED") {
        console.log(`[htlc-sweep] ${htlcId} already claimed/refunded; dropping from index.`);
        removeFromIndex(htlcId, indexPath);
        alreadyResolved++;
      } else if (result.status === "NOT_EXPIRED") {
        // Clock skew between our ledger read and simulation; leave it for next cycle.
        console.log(`[htlc-sweep] ${htlcId} not yet expired on simulation; will retry.`);
      } else {
        console.error(`[htlc-sweep] Sweep of ${htlcId} did not confirm cleanly:`, result);
        errors++;
      }
    } catch (err) {
      console.error(`[htlc-sweep] Error sweeping ${htlcId}:`, err.message || err);
      errors++;
    }
  }

  console.log(
    `[htlc-sweep] Cycle complete: ${swept} swept, ${alreadyResolved} already resolved, ${errors} error(s).`
  );
  return { swept, alreadyResolved, errors };
}

async function main() {
  if (!HTLC_CONTRACT_ID || !SWEEP_SECRET_KEY) {
    console.error("ERROR: HTLC_CONTRACT_ID and SWEEP_SECRET_KEY must be set.");
    process.exit(1);
  }

  const sweeper = getSweeperKeypair();
  const runOnce = process.argv.includes("--once");

  console.log(
    `[htlc-sweep] Starting FluxaPay HTLC sweep daemon (poll every ${POLL_INTERVAL_MS}ms)`
  );
  console.log(`[htlc-sweep] Contract: ${HTLC_CONTRACT_ID}`);
  console.log(`[htlc-sweep] Sweeper account: ${sweeper.publicKey()}`);
  console.log(`[htlc-sweep] Index: ${HTLC_INDEX_PATH}`);

  await runCycle();
  if (runOnce) return;

  setInterval(runCycle, POLL_INTERVAL_MS);
}

if (require.main === module) {
  main().catch((err) => {
    console.error("[htlc-sweep] Fatal:", err);
    process.exit(1);
  });
}

module.exports = {
  loadIndex,
  saveIndex,
  removeFromIndex,
  findExpiredCandidates,
  sweepHtlc,
  runCycle,
  POLL_INTERVAL_MS,
};
