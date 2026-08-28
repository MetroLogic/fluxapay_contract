#!/usr/bin/env node
/**
 * FluxaPay FX Oracle Updater (issue #631)
 *
 * Polls an off-chain price feed on a fixed interval, converts the quotes to the
 * contract's 7-decimal fixed-point rate format, and pushes them to the FXOracle
 * contract in a single atomic `set_rates_batch` call signed with the oracle
 * keypair.
 *
 * Design goals:
 *   - No silent staleness. If the feed cannot be fetched (after retries) the
 *     cycle is skipped and an alert is emitted — the contract keeps its last
 *     good rates and its own staleness guard takes over.
 *   - Deviation-aware. If the contract rejects the batch with
 *     `FXOracleError::RateDeviationExceeded` (#5) the rejection is logged and
 *     alerted rather than crashing the loop.
 *   - Structured logs. Every line is a single JSON object on stdout for easy
 *     ingestion by a log pipeline.
 *
 * Usage:
 *   ORACLE_SECRET=S... FX_ORACLE_CONTRACT_ID=C... node scripts/fx-oracle-updater.js
 *   node scripts/fx-oracle-updater.js --once      # run a single cycle and exit
 *
 * Environment variables (see scripts/README.md for the full table):
 *   ORACLE_SECRET           – oracle keypair secret (ORACLE role).           [required]
 *   FX_ORACLE_CONTRACT_ID   – FXOracle contract address (C...).              [required]
 *   PRICE_FEED_URL          – price feed endpoint.
 *                             default: https://api.exchangerate.host/latest?base=USD
 *   PRICE_FEED_RATES_PATH   – dot path to the {code: rate} object in the
 *                             feed response. default: "rates"
 *   FX_PAIRS                – comma list of `SYMBOL[:feedKey]` entries. The
 *                             stored Symbol is the amount of that currency per
 *                             1 USD. default: "EUR,BRL,USD"
 *   FX_RATE_DECIMALS        – fixed-point decimals for stored rates. default: 7
 *   UPDATE_INTERVAL_MS      – poll interval in ms. default: 60000
 *   MAX_FEED_RETRIES        – feed fetch attempts per cycle. default: 3
 *   FEED_RETRY_BACKOFF_MS   – base backoff between feed retries. default: 2000
 *   STELLAR_RPC_URL         – Soroban RPC endpoint.
 *                             default: https://soroban-testnet.stellar.org
 *   NETWORK_PASSPHRASE      – Stellar network passphrase. default: testnet
 *   ALERT_WEBHOOK_URL       – optional; POSTed a JSON body on every alert.
 */

"use strict";

const StellarSdk = require("@stellar/stellar-sdk");

const {
  TransactionBuilder,
  Networks,
  Keypair,
  Contract,
  nativeToScVal,
  scValToNative,
  BASE_FEE,
} = StellarSdk;

// stellar-sdk exposed the RPC client as `SorobanRpc` (<= v12) and later as
// `rpc` (>= v13). Support both so the script is version-tolerant.
const RpcNamespace = StellarSdk.rpc || StellarSdk.SorobanRpc;

// ── Configuration ────────────────────────────────────────────────────────────

const CONFIG = {
  oracleSecret: process.env.ORACLE_SECRET,
  contractId: process.env.FX_ORACLE_CONTRACT_ID || process.env.CONTRACT_ID,
  rpcUrl: process.env.STELLAR_RPC_URL || "https://soroban-testnet.stellar.org",
  networkPassphrase: process.env.NETWORK_PASSPHRASE || Networks.TESTNET,
  priceFeedUrl:
    process.env.PRICE_FEED_URL || "https://api.exchangerate.host/latest?base=USD",
  priceFeedRatesPath: process.env.PRICE_FEED_RATES_PATH || "rates",
  pairs: parsePairs(process.env.FX_PAIRS || "EUR,BRL,USD"),
  rateDecimals: parseInt(process.env.FX_RATE_DECIMALS || "7", 10),
  updateIntervalMs: parseInt(process.env.UPDATE_INTERVAL_MS || "60000", 10),
  maxFeedRetries: parseInt(process.env.MAX_FEED_RETRIES || "3", 10),
  feedRetryBackoffMs: parseInt(process.env.FEED_RETRY_BACKOFF_MS || "2000", 10),
  alertWebhookUrl: process.env.ALERT_WEBHOOK_URL || null,
  runOnce: process.argv.includes("--once"),
};

// FXOracleError discriminants (fluxapay/src/fx_oracle.rs).
const FX_ORACLE_ERRORS = {
  1: "RateNotFound",
  2: "RateStale",
  3: "Unauthorized",
  4: "BatchTooLarge",
  5: "RateDeviationExceeded",
  6: "PairNotFound",
};
// FXOracle::set_rates_batch caps a batch at 20 pairs.
const MAX_BATCH_RATES = 20;

// ── Logging ──────────────────────────────────────────────────────────────────

function log(level, event, fields = {}) {
  process.stdout.write(
    JSON.stringify({ ts: new Date().toISOString(), level, event, ...fields }) + "\n",
  );
}

async function alert(reason, fields = {}) {
  log("alert", reason, fields);
  if (!CONFIG.alertWebhookUrl) return;
  try {
    await fetch(CONFIG.alertWebhookUrl, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ source: "fx-oracle-updater", reason, ...fields }),
    });
  } catch (err) {
    log("error", "alert_webhook_failed", { message: String(err && err.message) });
  }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

function parsePairs(raw) {
  return raw
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean)
    .map((entry) => {
      const [symbol, feedKey] = entry.split(":");
      return { symbol: symbol.trim(), feedKey: (feedKey || symbol).trim() };
    });
}

function getByPath(obj, path) {
  return path.split(".").reduce((acc, key) => (acc == null ? acc : acc[key]), obj);
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Convert a floating-point quote to an integer fixed-point rate with
 * `decimals` places. Uses string math to avoid FP rounding surprises.
 */
function toFixedPoint(value, decimals) {
  if (!Number.isFinite(value) || value <= 0) {
    throw new Error(`invalid quote: ${value}`);
  }
  const [intPart, fracPartRaw = ""] = String(value).split(".");
  const fracPart = (fracPartRaw + "0".repeat(decimals)).slice(0, decimals);
  return BigInt(intPart + fracPart);
}

// ── Price feed ───────────────────────────────────────────────────────────────

/**
 * Fetch the price feed with retries and exponential backoff. Throws if every
 * attempt fails so the caller can skip the cycle (no silent staleness).
 */
async function fetchPriceFeed() {
  let lastErr;
  for (let attempt = 1; attempt <= CONFIG.maxFeedRetries; attempt++) {
    try {
      const res = await fetch(CONFIG.priceFeedUrl, {
        headers: { accept: "application/json" },
        signal: AbortSignal.timeout(15_000),
      });
      if (!res.ok) {
        throw new Error(`HTTP ${res.status} ${res.statusText}`);
      }
      const body = await res.json();
      const rates = getByPath(body, CONFIG.priceFeedRatesPath);
      if (!rates || typeof rates !== "object") {
        throw new Error(
          `feed response has no rates object at path "${CONFIG.priceFeedRatesPath}"`,
        );
      }
      log("info", "feed_ok", { attempt, url: CONFIG.priceFeedUrl });
      return rates;
    } catch (err) {
      lastErr = err;
      log("warn", "feed_attempt_failed", {
        attempt,
        max: CONFIG.maxFeedRetries,
        message: String(err && err.message),
      });
      if (attempt < CONFIG.maxFeedRetries) {
        await sleep(CONFIG.feedRetryBackoffMs * attempt);
      }
    }
  }
  throw new Error(
    `price feed unavailable after ${CONFIG.maxFeedRetries} attempts: ${
      lastErr && lastErr.message
    }`,
  );
}

/**
 * Build the `Vec<(Symbol, i128, u32)>` argument for `set_rates_batch` from the
 * raw feed rates. Missing or invalid quotes are skipped (and logged) rather
 * than aborting the whole batch.
 */
function buildRateTuples(feedRates) {
  const tuples = [];
  for (const { symbol, feedKey } of CONFIG.pairs) {
    const quote = feedKey === "USD" && feedRates.USD == null ? 1 : feedRates[feedKey];
    try {
      const fixed = toFixedPoint(Number(quote), CONFIG.rateDecimals);
      tuples.push({ symbol, rate: fixed, decimals: CONFIG.rateDecimals, quote });
    } catch (err) {
      log("warn", "quote_skipped", {
        symbol,
        feedKey,
        quote,
        message: String(err && err.message),
      });
    }
  }
  return tuples;
}

// ── Contract call ────────────────────────────────────────────────────────────

function makeServer() {
  return new RpcNamespace.Server(CONFIG.rpcUrl, {
    allowHttp: CONFIG.rpcUrl.startsWith("http://"),
  });
}

function tuplesToScVal(tuples) {
  return nativeToScVal(
    tuples.map((t) => [
      nativeToScVal(t.symbol, { type: "symbol" }),
      nativeToScVal(t.rate, { type: "i128" }),
      nativeToScVal(t.decimals, { type: "u32" }),
    ]),
  );
}

/** Decode `Error(Contract, #N)` out of a simulation / send failure. */
function extractContractErrorCode(payload) {
  const text = typeof payload === "string" ? payload : JSON.stringify(payload || "");
  const match = text.match(/Error\(Contract,\s*#(\d+)\)/);
  return match ? Number(match[1]) : null;
}

/**
 * Submit `set_rates_batch`. Returns `{ status: "ok", count }` on success or
 * `{ status: "rejected", code, name }` when the contract rejects the batch.
 */
async function submitBatch(server, keypair, tuples) {
  const contract = new Contract(CONFIG.contractId);
  const account = await server.getAccount(keypair.publicKey());

  const tx = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: CONFIG.networkPassphrase,
  })
    .addOperation(
      contract.call(
        "set_rates_batch",
        nativeToScVal(keypair.publicKey(), { type: "address" }),
        tuplesToScVal(tuples),
      ),
    )
    .setTimeout(30)
    .build();

  let prepared;
  try {
    prepared = await server.prepareTransaction(tx);
  } catch (err) {
    const code = extractContractErrorCode(err && (err.message || err));
    if (code != null) {
      return { status: "rejected", code, name: FX_ORACLE_ERRORS[code] || `#${code}` };
    }
    throw err;
  }

  prepared.sign(keypair);
  const sendResult = await server.sendTransaction(prepared);
  if (sendResult.status === "ERROR") {
    const code = extractContractErrorCode(sendResult.errorResult);
    if (code != null) {
      return { status: "rejected", code, name: FX_ORACLE_ERRORS[code] || `#${code}` };
    }
    throw new Error(`sendTransaction ERROR: ${JSON.stringify(sendResult.errorResult)}`);
  }

  let getResult;
  for (let i = 0; i < 12; i++) {
    await sleep(2500);
    getResult = await server.getTransaction(sendResult.hash);
    if (getResult.status !== "NOT_FOUND") break;
  }

  if (getResult && getResult.status === "SUCCESS") {
    let count = tuples.length;
    try {
      if (getResult.returnValue) count = Number(scValToNative(getResult.returnValue));
    } catch {
      /* keep the fallback count */
    }
    return { status: "ok", count, hash: sendResult.hash };
  }

  const code = extractContractErrorCode(getResult && getResult.resultXdr);
  if (code != null) {
    return { status: "rejected", code, name: FX_ORACLE_ERRORS[code] || `#${code}` };
  }
  throw new Error(
    `transaction did not succeed: ${getResult && getResult.status} (${sendResult.hash})`,
  );
}

// ── Main loop ────────────────────────────────────────────────────────────────

async function runCycle(server, keypair) {
  const startedAt = Date.now();
  log("info", "cycle_start", { pairs: CONFIG.pairs.map((p) => p.symbol) });

  let feedRates;
  try {
    feedRates = await fetchPriceFeed();
  } catch (err) {
    // Explicitly do NOT push anything — never overwrite good rates with stale
    // or partial data, and never let the feed outage pass unnoticed.
    await alert("price_feed_unavailable", { message: String(err && err.message) });
    return;
  }

  const tuples = buildRateTuples(feedRates);
  if (tuples.length === 0) {
    await alert("no_valid_quotes", { feedKeys: CONFIG.pairs.map((p) => p.feedKey) });
    return;
  }
  if (tuples.length > MAX_BATCH_RATES) {
    await alert("batch_too_large", { count: tuples.length, max: MAX_BATCH_RATES });
    return;
  }

  log("info", "rates_prepared", {
    rates: tuples.map((t) => ({ symbol: t.symbol, quote: t.quote, fixed: t.rate.toString() })),
  });

  let result;
  try {
    result = await submitBatch(server, keypair, tuples);
  } catch (err) {
    await alert("submit_failed", { message: String(err && err.message) });
    return;
  }

  if (result.status === "rejected") {
    // Contract-side rejection (e.g. RateDeviationExceeded #5). Surface it
    // loudly; a deviating feed is an incident, not a transient error.
    await alert("batch_rejected", {
      code: result.code,
      name: result.name,
      rates: tuples.map((t) => ({ symbol: t.symbol, fixed: t.rate.toString() })),
    });
    return;
  }

  log("info", "cycle_ok", {
    updated: result.count,
    hash: result.hash,
    durationMs: Date.now() - startedAt,
  });
}

function validateConfig() {
  const missing = [];
  if (!CONFIG.oracleSecret) missing.push("ORACLE_SECRET");
  if (!CONFIG.contractId) missing.push("FX_ORACLE_CONTRACT_ID");
  if (missing.length) {
    log("error", "missing_config", { missing });
    process.exit(1);
  }
  if (!RpcNamespace || !RpcNamespace.Server) {
    log("error", "stellar_sdk_rpc_unavailable", {
      hint: "install @stellar/stellar-sdk >= 13 in the workspace",
    });
    process.exit(1);
  }
}

async function main() {
  validateConfig();
  const keypair = Keypair.fromSecret(CONFIG.oracleSecret);
  const server = makeServer();

  log("info", "startup", {
    contractId: CONFIG.contractId,
    oracle: keypair.publicKey(),
    rpcUrl: CONFIG.rpcUrl,
    intervalMs: CONFIG.updateIntervalMs,
    pairs: CONFIG.pairs,
    runOnce: CONFIG.runOnce,
  });

  await runCycle(server, keypair);
  if (CONFIG.runOnce) return;

  // Non-overlapping scheduler: wait for the cycle to finish before arming the
  // next timer so a slow RPC round never stacks cycles.
  const tick = async () => {
    try {
      await runCycle(server, keypair);
    } catch (err) {
      log("error", "cycle_crash", { message: String(err && err.message) });
    } finally {
      setTimeout(tick, CONFIG.updateIntervalMs);
    }
  };
  setTimeout(tick, CONFIG.updateIntervalMs);
}

main().catch((err) => {
  log("error", "fatal", { message: String(err && err.message), stack: err && err.stack });
  process.exit(1);
});
