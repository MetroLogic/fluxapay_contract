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
 *   - an expected source file cannot be found,
 *   - a variant declared in `Error` is missing from the SDK map,
 *   - a code in the SDK map doesn't correspond to any declared variant, or
 *   - a code in the SDK map has no corresponding entry in `sdk/src/errors.ts`
 *     (i.e. no `FluxaPayError` with `code`, `contractCode`, `message`,
 *     `hint`, and `docsUrl`).
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, "..");

const libRsPath = join(repoRoot, "fluxapay", "src", "lib.rs");
const indexTsPath = join(repoRoot, "sdk", "src", "index.ts");
const errorsTsPath = join(repoRoot, "sdk", "src", "errors.ts");

function readSourceFile(path: string): string {
  try {
    return readFileSync(path, "utf8");
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    console.error(
      `check-error-map-sync: expected source file not found: ${path}\n  ${reason}`,
    );
    process.exit(1);
  }
}

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

interface ErrorEntry {
  code: string;
  contractCode: number;
  hasMessage: boolean;
  hasHint: boolean;
  hasDocsUrl: boolean;
}

/**
 * Parses `sdk/src/errors.ts` for `FluxaPayError` definitions. Each definition
 * is expected to carry a `code`, `contractCode`, `message`, `hint`, and
 * `docsUrl`. We scan object literals passed to `new FluxaPayError({ ... })`
 * as well as entries in a `FLUXAPAY_ERRORS`-style table keyed by contract code.
 */
function parseSdkErrors(source: string): Map<number, ErrorEntry> {
  const byCode = new Map<number, ErrorEntry>();

  // Match object literals that contain a `contractCode` field. This covers
  // both `new FluxaPayError({ ... })` calls and table entries.
  const objectRegex = /\{([^{}]*contractCode[^{}]*)\}/g;

  let match: RegExpExecArray | null;
  while ((match = objectRegex.exec(source)) !== null) {
    const body = match[1];

    const contractCodeMatch = body.match(/contractCode\s*:\s*(\d+)/);
    if (!contractCodeMatch) continue;
    const contractCode = Number(contractCodeMatch[1]);

    const codeMatch = body.match(/code\s*:\s*["']([^"']+)["']/);
    const code = codeMatch ? codeMatch[1] : "";

    const hasMessage = /message\s*:\s*["'`]/.test(body);
    const hasHint = /hint\s*:\s*["'`]/.test(body);
    const hasDocsUrl = /docsUrl\s*:\s*["'`]/.test(body);

    byCode.set(contractCode, {
      code,
      contractCode,
      hasMessage,
      hasHint,
      hasDocsUrl,
    });
  }

  return byCode;
}

function main(): void {
  const rustVariants = parseRustErrorEnum(readSourceFile(libRsPath));
  const sdkMap = parseSdkErrorMap(readSourceFile(indexTsPath));
  const sdkErrors = parseSdkErrors(readSourceFile(errorsTsPath));

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

  // Every contract error surfaced by the SDK map must have a corresponding
  // FluxaPayError definition with a code, message, hint, and docsUrl.
  for (const [code, name] of sdkMap) {
    const entry = sdkErrors.get(code);
    if (entry === undefined) {
      problems.push(
        `Code ${code} (${name}) has no FluxaPayError entry in sdk/src/errors.ts.`,
      );
      continue;
    }
    if (entry.code === "") {
      problems.push(
        `FluxaPayError for code ${code} (${name}) is missing a string \`code\` field.`,
      );
    }
    if (!entry.hasMessage) {
      problems.push(
        `FluxaPayError for code ${code} (${name}) is missing a \`message\` field.`,
      );
    }
    if (!entry.hasHint) {
      problems.push(
        `FluxaPayError for code ${code} (${name}) is missing an actionable \`hint\` field.`,
      );
    }
    if (!entry.hasDocsUrl) {
      problems.push(
        `FluxaPayError for code ${code} (${name}) is missing a \`docsUrl\` field.`,
      );
    }
  }

  if (problems.length > 0) {
    console.error("FLUXAPAY_CONTRACT_ERROR_MAP is out of sync with fluxapay/src/lib.rs::Error:\n");
    for (const problem of problems) {
      console.error(`  - ${problem}`);
    }
    console.error(
      "\nUpdate sdk/src/index.ts, sdk/src/errors.ts (and docs/error-codes.md) to match, then re-run this check.",
    );
    process.exit(1);
  }

  console.log(
    `FLUXAPAY_CONTRACT_ERROR_MAP is in sync with fluxapay/src/lib.rs::Error (${rustVariants.size} codes checked, ${sdkErrors.size} FluxaPayError entries verified).`,
  );
}

main();
