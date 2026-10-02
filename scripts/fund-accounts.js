#!/usr/bin/env node

/**
 * Faucet Fund Script for Localnet and Testnet Accounts
 *
 * This script funds one or more Stellar accounts using the Stellar Friendbot
 * (for testnet) or a local faucet endpoint (for localnet/sandbox).
 *
 * Usage:
 *   npx node scripts/fund-accounts.js <address1> [address2] [address3] ...
 *   npx node scripts/fund-accounts.js --config ./accounts.json
 *
 * Environment variables:
 *   FAUCET_URL - Faucet endpoint (defaults to testnet Friendbot if not set)
 *               For localnet: http://localhost:8000
 *               For testnet: https://friendbot.stellar.org
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
    console.error("");
    console.error("Environment variables:");
    console.error("  FAUCET_URL - Faucet endpoint (default: testnet Friendbot)");
    process.exit(1);
  }

  // Parse arguments
  let accountsToFund = [];

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
  } else {
    // Parse from command line arguments
    accountsToFund = args.map((address) => ({ address, name: null }));
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

  console.log("");
  console.log("═".repeat(50));

  // Summary report
  const successful = results.filter((r) => r.status === "success").length;
  const alreadyFunded = results.filter((r) => r.status === "already_funded")
    .length;
  const failed = results.filter((r) => r.status === "failed").length;
  const skipped = results.filter((r) => r.status === "skipped").length;

  console.log(`Summary:`);
  console.log(`  ✓ Successfully funded: ${successful}`);
  console.log(`  ✓ Already funded: ${alreadyFunded}`);
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
