/**
 * Issue #662: Unit tests verifying that `FLUXAPAY_CONTRACT_ERROR_MAP` (and
 * the `toFluxapayError` mapper built on top of it) stay in sync with the
 * contract's `Error` enum (`fluxapay/src/lib.rs`) and with
 * `docs/error-codes.md`.
 *
 * Run via `npm test` (see the `sdk/package.json` "test" script), which
 * invokes this file with `tsx --test`. Uses Node's built-in test runner
 * (`node:test`) and `node:assert/strict` — no extra test-framework
 * dependency required.
 */
import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

import { FLUXAPAY_CONTRACT_ERROR_MAP, FluxapayError, toFluxapayError } from "../src/index.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const sdkDir = join(__dirname, "..");
const repoRoot = join(sdkDir, "..");

/** Build a synthetic error the way `withMappedContractError` sees them from the RPC layer. */
function contractError(code: number): Error {
  return new Error(`Error(Contract, #${code})`);
}

describe("FLUXAPAY_CONTRACT_ERROR_MAP / toFluxapayError", () => {
  test("every code in the map produces a FluxapayError with the correct code and message", () => {
    for (const [codeStr, name] of Object.entries(FLUXAPAY_CONTRACT_ERROR_MAP)) {
      const code = Number(codeStr);
      const err = toFluxapayError(contractError(code));

      assert.ok(err instanceof FluxapayError, `code ${code}: expected a FluxapayError instance`);
      assert.equal(err.code, code, `code ${code}: FluxapayError.code mismatch`);
      assert.equal(
        err.contractErrorName,
        name,
        `code ${code}: contractErrorName should be "${name}", got "${err.contractErrorName}"`,
      );
      assert.equal(
        err.message,
        `${name} (contract error #${code})`,
        `code ${code}: unexpected error message`,
      );
      assert.equal(err.name, `${name}Error`);
    }
  });

  test("test_unknown_error_code_produces_generic_error", () => {
    // 999999 is guaranteed not to be a real contract error code.
    const err = toFluxapayError(contractError(999999));

    assert.ok(err instanceof FluxapayError);
    assert.equal(err.code, 999999);
    assert.equal(err.contractErrorName, "UnknownContractError");
    assert.match(err.message, /^UnknownContractError \(contract error #999999\)$/);
  });

  test("a plain Error with no recognizable contract-error shape is rethrown as-is", () => {
    const original = new Error("network timeout");
    assert.throws(() => toFluxapayError(original), (thrown: unknown) => thrown === original);
  });
});

describe("test_all_documented_codes_present_in_map", () => {
  test("every code documented in the Core table of docs/error-codes.md is a key in the SDK map", () => {
    const docsPath = join(repoRoot, "docs", "error-codes.md");
    const docsSource = readFileSync(docsPath, "utf8");

    // Extract just the "Core" table: from its heading to the next "## " heading.
    const coreSectionMatch = docsSource.match(
      /## Core: `PaymentProcessor` \/ `RefundManager`[\s\S]*?\n(?=## )/,
    );
    assert.ok(coreSectionMatch, "Could not find the Core error table in docs/error-codes.md");
    const coreSection = coreSectionMatch![0];

    // Table rows look like: | 1 | `Unauthorized` | ... |
    const rowRegex = /^\|\s*(\d+)\s*\|\s*`([A-Za-z0-9_]+)`/gm;
    const documentedCodes = new Set<number>();
    let match: RegExpExecArray | null;
    while ((match = rowRegex.exec(coreSection)) !== null) {
      documentedCodes.add(Number(match[1]));
    }

    assert.ok(documentedCodes.size > 0, "Parsed zero documented codes from docs/error-codes.md");

    const mapCodes = new Set(Object.keys(FLUXAPAY_CONTRACT_ERROR_MAP).map(Number));

    const missingFromMap = [...documentedCodes].filter((c) => !mapCodes.has(c));
    const missingFromDocs = [...mapCodes].filter((c) => !documentedCodes.has(c));

    assert.deepEqual(
      missingFromMap,
      [],
      `Codes documented in docs/error-codes.md but missing from FLUXAPAY_CONTRACT_ERROR_MAP: ${missingFromMap.join(", ")}`,
    );
    assert.deepEqual(
      missingFromDocs,
      [],
      `Codes in FLUXAPAY_CONTRACT_ERROR_MAP but not documented in docs/error-codes.md: ${missingFromDocs.join(", ")}`,
    );
  });
});

describe("test_error_map_matches_check_error_map_sync_script", () => {
  test("scripts/check-error-map-sync.ts reports zero differences against fluxapay/src/lib.rs::Error", () => {
    // Mirrors the `check-error-map-sync` script in sdk/package.json: run
    // from the sdk/ directory (where `tsx` is a devDependency) against the
    // repo-relative script path.
    //
    // The script itself exits non-zero and prints a diagnostic list on any
    // drift between FLUXAPAY_CONTRACT_ERROR_MAP and the Rust `Error` enum;
    // exit code 0 is our "zero differences" assertion.
    assert.doesNotThrow(() => {
      execFileSync("npx", ["tsx", "../scripts/check-error-map-sync.ts"], {
        cwd: sdkDir,
        stdio: "pipe",
      });
    }, "check-error-map-sync.ts reported drift between the SDK map and fluxapay/src/lib.rs::Error (run `npm run check-error-map-sync` in sdk/ for details)");
  });
});
