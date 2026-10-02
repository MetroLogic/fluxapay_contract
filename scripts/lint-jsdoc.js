#!/usr/bin/env node
/**
 * Validates that all public methods on `FluxapayClient` in `sdk/src/index.ts`
 * have JSDoc comments with `@throws` annotations.
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, "..");
const indexPath = join(repoRoot, "sdk", "src", "index.ts");

const content = readFileSync(indexPath, "utf8");

// Extract the FluxapayClient class body
const clientMatch = content.match(/export class FluxapayClient\s*\{([\s\S]*?)\n\}/);
if (!clientMatch) {
  console.error("lint-jsdoc: Could not find `export class FluxapayClient` in sdk/src/index.ts");
  process.exit(1);
}

const clientBody = clientMatch[1];
const lines = clientBody.split("\n");

let inJsDoc = false;
let currentJsDoc = "";
let missingThrows = [];
let checkedMethods = 0;

for (let i = 0; i < lines.length; i++) {
  const line = lines[i];
  const trimmed = line.trim();

  if (trimmed.startsWith("/**")) {
    inJsDoc = true;
    currentJsDoc = trimmed;
    if (trimmed.endsWith("*/")) {
      inJsDoc = false;
    }
    continue;
  }

  if (inJsDoc) {
    currentJsDoc += "\n" + trimmed;
    if (trimmed.endsWith("*/")) {
      inJsDoc = false;
    }
    continue;
  }

  // Match method declaration: e.g. async methodName( or methodName(
  const methodMatch = trimmed.match(/^(?:async\s+)?([a-zA-Z0-9_]+)\s*\([^)]*\)\s*(?::\s*[^;{]+)?\s*\{/);
  // Exclude private / constructor / helpers and JS control-flow statements
  if (methodMatch) {
    const methodName = methodMatch[1];
    const JS_KEYWORDS = new Set(["if", "while", "for", "switch", "catch"]);
    if (
      methodName !== "constructor" &&
      !methodName.startsWith("#") &&
      !methodName.startsWith("_") &&
      !JS_KEYWORDS.has(methodName)
    ) {
      checkedMethods++;
      if (!currentJsDoc || !currentJsDoc.includes("@throws")) {
        missingThrows.push(methodName);
      }
      currentJsDoc = "";
    }
  } else if (!trimmed.startsWith("*") && !trimmed.startsWith("//") && trimmed.length > 0) {
    // If line is not a comment and not a method, reset JSDoc buffer unless it's an attribute
    if (!trimmed.startsWith("@") && !trimmed.includes("=>")) {
      // keep currentJsDoc if directly preceding method
    }
  }
}

if (missingThrows.length > 0) {
  console.error(
    `lint-jsdoc: Found ${missingThrows.length} public methods missing JSDoc @throws annotations:\n  - ` +
      missingThrows.join("\n  - "),
  );
  process.exit(1);
}

console.log(`lint-jsdoc: Verified ${checkedMethods} public methods in FluxapayClient have @throws JSDoc annotations.`);
process.exit(0);
