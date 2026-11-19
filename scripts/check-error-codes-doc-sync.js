#!/usr/bin/env node
/**
 * Issue #1193: Verifies that the "Core" error table in `docs/error-codes.md`
 * stays in sync with the numeric discriminants of `pub enum Error` in
 * `fluxapay/src/lib.rs`.
 *
 * This is distinct from `scripts/check-error-map-sync.ts` (issue #829 fixed a
 * bug in that script): that script checks the SDK's
 * `FLUXAPAY_CONTRACT_ERROR_MAP` against the Rust enum. This script checks the
 * *documentation* table against the same Rust enum, since `docs/error-codes.md`
 * is hand-maintained and has no other mechanical guard against drifting when
 * `Error` variants are added, renamed, renumbered, or removed.
 *
 * Exits non-zero (fails CI) if:
 *   - an expected source file cannot be found,
 *   - a `(code, name)` pair declared in `Error` has no matching row in the
 *     doc's Core table, or
 *   - a `(code, name)` pair in the doc's Core table no longer exists in
 *     `Error`.
 *
 * Usage: node scripts/check-error-codes-doc-sync.js
 */
"use strict";

const fs = require("fs");
const path = require("path");

const repoRoot = path.join(__dirname, "..");
const libRsPath = path.join(repoRoot, "fluxapay", "src", "lib.rs");
const docPath = path.join(repoRoot, "docs", "error-codes.md");

function readSourceFile(filePath) {
  try {
    return fs.readFileSync(filePath, "utf8");
  } catch (error) {
    console.error(
      `check-error-codes-doc-sync: expected source file not found: ${filePath}\n  ${error.message}`,
    );
    process.exit(1);
  }
}

/**
 * Parses `pub enum Error { ... }` out of `fluxapay/src/lib.rs` and returns a
 * `Map<number, Set<string>>` of code -> variant names declared with that
 * discriminant (several codes are intentionally/unintentionally shared by
 * more than one variant in the current source — see the collision note in
 * `docs/error-codes.md`).
 */
function parseRustErrorEnum(source) {
  const enumMatch = source.match(/pub enum Error\s*{([\s\S]*?)\n}/);
  if (!enumMatch) {
    throw new Error("Could not find `pub enum Error { ... }` in fluxapay/src/lib.rs");
  }

  const body = enumMatch[1];
  const variantRegex = /(\w+)\s*=\s*(\d+),/g;
  const byCode = new Map();

  let match;
  while ((match = variantRegex.exec(body)) !== null) {
    const [, name, codeStr] = match;
    const code = Number(codeStr);
    if (!byCode.has(code)) byCode.set(code, new Set());
    byCode.get(code).add(name);
  }

  return byCode;
}

/**
 * Parses the "Core" table in `docs/error-codes.md` (the table under the
 * `## Core: ...` heading, up to the next `## ` heading) and returns a
 * `Map<number, Set<string>>` of code -> documented variant name(s). A doc
 * row's Name column may list several `` `Name` `` values joined by ` / ` to
 * document a shared discriminant.
 */
function parseDocCoreTable(source) {
  const sectionMatch = source.match(
    /## Core:[^\n]*\n\n\|[^\n]*\n\|[-| ]*\n([\s\S]*?)\n\n/,
  );
  if (!sectionMatch) {
    throw new Error(
      "Could not find the Core error table in docs/error-codes.md (expected a `## Core: ...` heading followed by a Markdown table)",
    );
  }

  const body = sectionMatch[1];
  const byCode = new Map();

  for (const line of body.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed.startsWith("|")) continue;
    const cells = trimmed
      .split("|")
      .map((cell) => cell.trim())
      .filter((cell) => cell.length > 0);
    if (cells.length < 2) continue;

    const code = Number(cells[0]);
    if (!Number.isInteger(code)) continue; // skip stray/divider rows

    const names = cells[1]
      .split("/")
      .map((n) => n.trim().replace(/^`+|`+$/g, ""))
      .filter((n) => n.length > 0);

    if (!byCode.has(code)) byCode.set(code, new Set());
    for (const name of names) byCode.get(code).add(name);
  }

  return byCode;
}

function setsEqual(a, b) {
  if (a.size !== b.size) return false;
  for (const value of a) {
    if (!b.has(value)) return false;
  }
  return true;
}

function formatNames(set) {
  return Array.from(set).sort().join(", ");
}

function main() {
  const rustVariants = parseRustErrorEnum(readSourceFile(libRsPath));
  const docVariants = parseDocCoreTable(readSourceFile(docPath));

  const problems = [];

  for (const [code, names] of rustVariants) {
    const documented = docVariants.get(code);
    if (documented === undefined) {
      problems.push(
        `Code ${code} (${formatNames(names)}) is declared in \`Error\` but has no row in docs/error-codes.md's Core table.`,
      );
    } else if (!setsEqual(names, documented)) {
      problems.push(
        `Code ${code}: \`Error\` declares {${formatNames(names)}} but docs/error-codes.md documents {${formatNames(documented)}}.`,
      );
    }
  }

  for (const [code, names] of docVariants) {
    if (!rustVariants.has(code)) {
      problems.push(
        `Code ${code} (${formatNames(names)}) is documented in docs/error-codes.md's Core table but no longer declared in \`Error\`.`,
      );
    }
  }

  if (problems.length > 0) {
    console.error(
      "docs/error-codes.md's Core table is out of sync with fluxapay/src/lib.rs::Error:\n",
    );
    for (const problem of problems) {
      console.error(`  - ${problem}`);
    }
    console.error(
      "\nUpdate docs/error-codes.md's Core table (see CONTRIBUTING.md's 'Error code documentation' section) to match, then re-run this check.",
    );
    process.exit(1);
  }

  console.log(
    `docs/error-codes.md's Core table is in sync with fluxapay/src/lib.rs::Error (${rustVariants.size} codes checked).`,
  );
}

main();
