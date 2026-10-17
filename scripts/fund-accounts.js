#!/usr/bin/env node

/**
 * Faucet Fund Script for Localnet and Testnet Accounts
 *
 * This script funds one or more Stellar accounts using the Stellar Friendbot
 * (for testnet) or a local faucet endpoint (for localnet/sandbox), and
 * optionally establishes a USDC trustline on testnet so the account can
 * receive USDC payments.
 *
 * Usage:
 *   npx node scripts/fund-accounts.js <address1> [address2] [address3] ...
 *   npx node scripts/fund-accounts.js --config ./accounts.json
 *   npx node scripts/fund-accounts.js --trustline <address1> [address2] ...
 *
 * Environment variables:
 *   FAUCET_URL - Faucet endpoint (defaults to testnet Friendbot if not set)
 *               For localnet: http://localhost:8000
 *               For testnet: https://friendbot.stellar.org
 *   HORIZON_URL - Horizon endpoint (default: https://horizon-testnet.stellar.org)
 *   USDC_ISSUER - USDC issuer account (default: testnet issuer below)
 *   USDC_ASSET_CODE - USDC asset code (default: USDC)
 *   TRUSTLINE_SECRET - Secret key used to sign trustline changes. When set,
 *               trustlines are established automatically after funding.
 *
 * Configuration file format (accounts.json):
 *   {
 *     "accounts": [
 *       { "address": "GXXXXXX...", "name": "admin" },
 *       { "address": "GYYYYYY...", "name": "merchant" }
 *     ]
 *   }
 *
 * Exit codes:
 *   0: All accounts funded successfully
 *   1: One or more accounts failed (check logs for details)
 */

const fs = require("fs");
const path = require("path");

// Default faucet URL for testnet; override with FAUCET_URL env var
const defaultFaucetUrl = "https://friendbot.stellar.org";
const faucetUrl = process.env.FAUCET_URL || defaultFaucetUrl;

// Horizon endpoint used for trustline checks/establishment
const horizonUrl =
  process.env.HORIZON_URL || "https://horizon-testnet.stellar.org";

// USDC asset configuration (testnet defaults)
const usdcAssetCode = process.env.USDC_ASSET_CODE || "USDC";
const usdcIssuer =
  process.env.USDC_ISSUER ||
  "GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5";

// Secret key used to sign trustline changes (optional)
const trustlineSecret = process.env.TRUSTLINE_SECRET || null;

/**
 * Validate a Stellar address format
 */
function isValidAddress(address) {
  // Stellar public key addresses start with 'G' and are 56 characters
  return /^G[A-Z2-7]{55}$/.test(address);
}

/**
 * Fund a single account via the faucet endpoint
 *
 * @param {string} address - Stellar account address
 * @param {string} name - Optional account name/label for logging
 * @returns {Promise<Object>} - Result object with status and message
 */
async function fundAccount(address, name) {
  const label = name ? `${name} (${address})` : address;

  if (!isValidAddress(address)) {
    console.warn(`⚠️  SKIP: Invalid address format: ${label}`);
    return { status: "skipped", address, reason: "invalid_format" };
  }

  try {
    console.log(`⏳ Funding: ${label}`);

    // Construct faucet request URL
    const url = `${faucetUrl}?addr=${encodeURIComponent(address)}`;

    const response = await fetch(url, {
      method: "GET",
      timeout: 10000,
    });

    if (!response.ok) {
      // Handle specific error cases gracefully
      if (response.status === 400) {
        // Account may already be funded or invalid
        const errorText = await response.text();
        if (
          errorText.includes("already funded") ||
          errorText.includes("Tap", "Forbidden")
        ) {
          console.log(`✓ ALREADY FUNDED: ${label}`);
          return { status: "already_funded", address };
        }
        console.warn(`⚠️  WARN: ${label} - ${errorText}`);
        return { status: "already_funded", address, reason: "400_response" };
      }

      throw new Error(
        `HTTP ${response.status}: ${response.statusText} - ${await response.text()}`
      );
    }

    // Success response
    const responseText = await response.text();
    console.log(`✓ SUCCESS: ${label} funded`);

    return { status: "success", address };
  } catch (error) {
    const errorMsg =
      error instanceof Error ? error.message : String(error);
    console.error(`✗ FAILED: ${label} - ${errorMsg}`);
    return {
      status: "failed",
      address,
      error: errorMsg,
    };
  }
}

/**
 * Check whether an account already has a USDC trustline via Horizon.
 *
 * @param {string} address - Stellar account address
 * @returns {Promise<boolean>} - true if the USDC trustline exists
 */
async function hasUsdcTrustline(address) {
  const url = `${horizonUrl}/accounts/${encodeURIComponent(address)}`;
  const response = await fetch(url, { method: "GET", timeout: 10000 });

  if (response.status === 404) {
    return false;
  }

  if (!response.ok) {
    throw new Error(
      `Horizon HTTP ${response.status}: ${response.statusText}`
    );
  }

  const account = await response.json();
  const balances = Array.isArray(account.balances) ? account.balances : [];

  return balances.some(
    (b) =>
      b.asset_type !== "native" &&
      b.asset_code === usdcAssetCode &&
      b.asset_issuer === usdcIssuer
  );
}

/**
 * Establish a USDC trustline on an account using the configured secret key.
 *
 * @param {string} address - Stellar account address
 * @returns {Promise<Object>} - Result object with status and message
 */
async function establishTrustline(address) {
  if (!trustlineSecret) {
    console.warn(
      `⚠️  SKIP trustline: TRUSTLINE_SECRET not set for ${address}`
    );
    return { status: "skipped", address, reason: "no_secret" };
  }

  try {
    if (await hasUsdcTrustline(address)) {
      console.log(`✓ TRUSTLINE EXISTS: ${address}`);
      return { status: "already_trusted", address };
    }

    // Lazy-load the Stellar SDK so the script still works for plain funding
    // when the dependency is unavailable.
    const StellarSdk = require("@stellar/stellar-sdk");
    const server = new StellarSdk.Horizon.Server(horizonUrl);
    const sourceKeypair = StellarSdk.Keypair.fromSecret(trustlineSecret);
    const sourceAccount = await server.loadAccount(sourceKeypair.publicKey());

    const transaction = new StellarSdk.TransactionBuilder(sourceAccount, {
      fee: StellarSdk.BASE_FEE,
      networkPassphrase: StellarSdk.Networks.TESTNET,
    })
      .addOperation(
        StellarSdk.Operation.changeTrust({
          asset: new StellarSdk.Asset(usdcAssetCode, usdcIssuer),
          source: address,
        })
      )
      .setTimeout(30)
      .build();

    transaction.sign(sourceKeypair);
    await server.submitTransaction(transaction);

    console.log(`✓ TRUSTLINE ESTABLISHED: ${address}`);
    return { status: "trustline_established", address };
  } catch (error) {
    const errorMsg =
      error instanceof Error ? error.message : String(error);
    console.error(`✗ TRUSTLINE FAILED: ${address} - ${errorMsg}`);
    return { status: "failed", address, error: errorMsg };
  }
}

/**
 * Load accounts from a configuration file
 */
function loadConfigFile(filePath) {
  const fullPath = path.resolve(filePath);

  if (!fs.existsSync(fullPath)) {
    throw new Error(`Config file not found: ${fullPath}`);
  }

  const content = fs.readFileSync(fullPath, "utf-8");
  const config = JSON.parse(content);

  if (!config.accounts || !Array.isArray(config.accounts)) {
    throw new Error("Config file must contain an 'accounts' array");
  }

  return config.accounts;
}

/**
 * Main function
 */
async function main() {
  const args = process.argv.slice(2);

  if (args.length === 0) {
    console.error("Usage:");
    console.error(
      "  npx node scripts/fund-accounts.js <address1> [address2] ..."
    );
    console.error("  npx node scripts/fund-accounts.js --config <file.json>");
    console.error(
      "  npx node scripts/fund-accounts.js --trustline <address1> [address2] ..."
    );
    console.error("");
    console.error("Environment variables:");
    console.error("  FAUCET_URL - Faucet endpoint (default: testnet Friendbot)");
    console.error("  HORIZON_URL - Horizon endpoint (default: testnet)");
    console.error("  USDC_ISSUER - USDC issuer account (default: testnet issuer)");
    console.error("  TRUSTLINE_SECRET - Secret key to sign trustline changes");
    process.exit(1);
  }

  // Parse arguments
  let accountsToFund = [];
  let establishTrustlines = false;

  if (args[0] === "--config" && args[1]) {
    // Load from config file
    try {
      const configAccounts = loadConfigFile(args[1]);
      accountsToFund = configAccounts.map((acc) => ({
        address: acc.address,
        name: acc.name,
      }));
      console.log(`📋 Loaded ${accountsToFund.length} accounts from config`);
    } catch (error) {
      console.error(`❌ ${error instanceof Error ? error.message : String(error)}`);
      process.exit(1);
    }
  } else if (args[0] === "--trustline") {
    // Establish trustlines only (no funding)
    establishTrustlines = true;
    accountsToFund = args.slice(1).map((address) => ({ address, name: null }));
  } else {
    // Parse from command line arguments
    accountsToFund = args.map((address) => ({ address, name: null }));
  }

  if (accountsToFund.length === 0) {
    console.error("❌ No accounts provided");
    process.exit(1);
  }

  console.log(`🚀 Funding accounts via faucet: ${faucetUrl}`);
  console.log("");

  // Fund all accounts in sequence
  const results = [];
  for (const account of accountsToFund) {
    const result = await fundAccount(account.address, account.name);
    results.push(result);

    // Small delay between requests to avoid rate limiting
    if (accountsToFund.indexOf(account) < accountsToFund.length - 1) {
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
  }

  // Establish USDC trustlines on testnet when a signing secret is available
  // or when explicitly requested via --trustline.
  if (trustlineSecret || establishTrustlines) {
    console.log("");
    console.log(`🔗 Establishing USDC trustlines (issuer: ${usdcIssuer})`);
    for (const account of accountsToFund) {
      const result = await establishTrustline(account.address);
      results.push(result);
    }
  }

  console.log("");
  console.log("═".repeat(50));

  // Summary report
  const successful = results.filter((r) => r.status === "success").length;
  const alreadyFunded = results.filter((r) => r.status === "already_funded")
    .length;
  const trustlines = results.filter(
    (r) => r.status === "trustline_established" || r.status === "already_trusted"
  ).length;
  const failed = results.filter((r) => r.status === "failed").length;
  const skipped = results.filter((r) => r.status === "skipped").length;

  console.log(`Summary:`);
  console.log(`  ✓ Successfully funded: ${successful}`);
  console.log(`  ✓ Already funded: ${alreadyFunded}`);
  console.log(`  ✓ USDC trustlines ready: ${trustlines}`);
  console.log(`  ✗ Failed: ${failed}`);
  console.log(`  ⊘ Skipped: ${skipped}`);

  const totalProcessed = successful + alreadyFunded;
  console.log(`Total processed: ${totalProcessed}/${accountsToFund.length}`);

  // Exit with error code if any failed
  if (failed > 0) {
    console.error("");
    console.error("Failed accounts:");
    results
      .filter((r) => r.status === "failed")
      .forEach((r) => {
        console.error(`  - ${r.address}: ${r.error}`);
      });
    process.exit(1);
  }

  process.exit(0);
}

main();
