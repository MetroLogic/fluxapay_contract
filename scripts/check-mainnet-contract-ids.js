#!/usr/bin/env node
/**
 * Warns (without failing CI) when `FLUXAPAY_CONTRACT_IDS.mainnet` in
 * `sdk/src/network-profiles.ts` still contains placeholder addresses.
 *
 * Once the mainnet deployment lands, replace `UNSET_CONTRACT_ID` in the
 * `mainnet` block with the real deployed contract addresses (see #461).
 */
const fs = require("fs");
const path = require("path");

const filePath = path.join(__dirname, "..", "sdk", "src", "network-profiles.ts");
const source = fs.readFileSync(filePath, "utf8");

const mainnetMatch = source.match(/mainnet:\s*{([^}]*)}/s);
if (!mainnetMatch) {
  console.warn(`::warning::Could not locate the mainnet block in ${filePath}`);
  process.exit(0);
}

const mainnetBlock = mainnetMatch[1];
const placeholderFields = [...mainnetBlock.matchAll(/(\w+):\s*UNSET_CONTRACT_ID/g)].map(
  (m) => m[1],
);

if (placeholderFields.length > 0) {
  console.warn(
    `::warning::FLUXAPAY_CONTRACT_IDS.mainnet still has placeholder contract IDs for: ${placeholderFields.join(", ")}. ` +
      `Populate them in sdk/src/network-profiles.ts once the mainnet deployment lands (see issue #461).`,
  );
} else {
  console.log("FLUXAPAY_CONTRACT_IDS.mainnet has no placeholder contract IDs.");
}

// Informational only — never fails the build.
process.exit(0);
