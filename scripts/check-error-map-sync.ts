#!/usr/bin/env node
/**
 * Verifies that `FLUXAPAY_CONTRACT_ERROR_MAP` in `sdk/src/index.ts` stays in
 * sync with the `Error` enum in `fluxapay/src/lib.rs` (the enum shared by
 * the `PaymentProcessor` and `RefundManager` contracts).
 *
 * This intentionally checks only the "Core" contract's error enum: the SDK
 * map is a single flat `code -> name` table, which cannot safely represent
 * the other contracts' independent, overlapping code spaces (see
 * `docs/error-codes.md`).
 *
 * Exits non-zero (fails CI) if:
 *   - a variant declared in `Error` is missing from the SDK map, or
 *   - a code in the SDK map doesn't correspond to any declared variant.
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, "..");

const libRsPath = join(repoRoot, "fluxapay", "src", "lib.rs");
const indexTsPath = join(repoRoot, "sdk", "src", "index.ts");

function parseRustErrorEnum(source: string): Map<number, string[]> {
  const enumMatch = source.match(/pub enum Error\s*{([\s\S]*?)\n}/);
  if (!enumMatch) {
    throw new Error("Could not find `pub enum Error { ... }` in fluxapay/src/lib.rs");
  }

  const body = enumMatch[1];
  const variantRegex = /(\w+)\s*=\s*(\d+),/g;
  const byCode = new Map<number, string[]>();

  let match: RegExpExecArray | null;
  while ((match = variantRegex.exec(body)) !== null) {
    const [, name, codeStr] = match;
    const code = Number(codeStr);
    const existing = byCode.get(code) ?? [];
    existing.push(name);
    byCode.set(code, existing);
  }

  return byCode;
}

function parseSdkErrorMap(source: string): Map<number, string> {
  const mapMatch = source.match(
    /FLUXAPAY_CONTRACT_ERROR_MAP: Record<number, string> = {([\s\S]*?)\n};/,
  );
  if (!mapMatch) {
    throw new Error("Could not find FLUXAPAY_CONTRACT_ERROR_MAP in sdk/src/index.ts");
  }

  const body = mapMatch[1];
  const entryRegex = /(\d+):\s*"([^"]+)"/g;
  const byCode = new Map<number, string>();

  let match: RegExpExecArray | null;
  while ((match = entryRegex.exec(body)) !== null) {
    const [, codeStr, name] = match;
    byCode.set(Number(codeStr), name);
  }

  return byCode;
}

function main(): void {
  const rustVariants = parseRustErrorEnum(readFileSync(libRsPath, "utf8"));
  const sdkMap = parseSdkErrorMap(readFileSync(indexTsPath, "utf8"));

  const problems: string[] = [];

  for (const [code, names] of rustVariants) {
    const mapped = sdkMap.get(code);
    if (mapped === undefined) {
      problems.push(
        `Code ${code} (${names.join(" / ")}) is declared in Error but missing from FLUXAPAY_CONTRACT_ERROR_MAP.`,
      );
    } else if (!names.includes(mapped)) {
      problems.push(
        `Code ${code} maps to "${mapped}" in the SDK, but Error declares: ${names.join(", ")}.`,
      );
    }
  }

  for (const code of sdkMap.keys()) {
    if (!rustVariants.has(code)) {
      problems.push(
        `Code ${code} is present in FLUXAPAY_CONTRACT_ERROR_MAP but no longer declared in Error.`,
      );
    }
  }

  if (problems.length > 0) {
    console.error("FLUXAPAY_CONTRACT_ERROR_MAP is out of sync with fluxapay/src/lib.rs::Error:\n");
    for (const problem of problems) {
      console.error(`  - ${problem}`);
    }
    console.error(
      "\nUpdate sdk/src/index.ts (and docs/error-codes.md) to match, then re-run this check.",
    );
    process.exit(1);
  }

  console.log(
    `FLUXAPAY_CONTRACT_ERROR_MAP is in sync with fluxapay/src/lib.rs::Error (${rustVariants.size} codes checked).`,
  );
}

main();
