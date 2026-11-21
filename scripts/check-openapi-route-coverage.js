#!/usr/bin/env node
/**
 * Issue #1265: Verifies that `docs/openapi.yaml` covers every REST route the
 * indexer registers, and that every operation it documents for the indexer is
 * still registered.
 *
 * The check is deliberately static: it parses the Express registrations out of
 * the indexer sources instead of importing them, so it needs no database, no
 * `npm install`, and no build step — it runs in a few milliseconds in CI.
 *
 * Two directions are enforced:
 *   1. every registered route has a matching path + method in the spec, and
 *   2. every documented operation is backed by a registered route — unless it
 *      is listed in `docs/openapi-coverage-exceptions.json` with a reason
 *      (routes served by the separate `backend/` API, and documented endpoints
 *      that have no HTTP implementation yet).
 *
 * Known gaps are not silently ignored: an exception that is no longer needed
 * (because the operation is now registered, or is no longer documented) fails
 * the check, so the exception list can only shrink.
 *
 * Path matching rules (see docs/openapi-coverage.md):
 *   - `:id` (Express) and `{id}` (OpenAPI) are both parameter segments, and the
 *     parameter *name* is ignored: `/v1/invoices/:id` == `/v1/invoices/{invoice_id}`.
 *   - Trailing slashes and repeated slashes are insignificant.
 *   - An unversioned path and its `/v1` twin are aliases, so documenting either
 *     `/events/stream` or `/v1/events/stream` covers both registrations.
 *
 * Exits non-zero (fails CI) if the spec or a route source cannot be found, a
 * route registration uses a non-literal path (it cannot be verified), or the
 * two sides disagree.
 *
 * Usage: node scripts/check-openapi-route-coverage.js [--spec <path>]
 *          [--exceptions <path>] [--verbose] [--help]
 */
"use strict";

const fs = require("fs");
const path = require("path");

const repoRoot = path.join(__dirname, "..");

const DEFAULT_SPEC = "docs/openapi.yaml";
const DEFAULT_EXCEPTIONS = "docs/openapi-coverage-exceptions.json";

/**
 * Files that register HTTP routes on the indexer's Express app. Add a new route
 * module here when it is mounted — a module that is not listed is invisible to
 * this check.
 *
 * `indexer/src/routes/transactions.ts` is deliberately absent: it builds a
 * router that nothing mounts (and imports a `../db` module that does not
 * exist), so `GET /v1/transactions` is not part of the served surface.
 */
const ROUTE_SOURCES = [
  "indexer/src/server.ts",
  "indexer/src/webhooks/routes.ts",
];

/** Express receivers whose `.<method>("<path>", ...)` registrations we collect. */
const DEFAULT_RECEIVERS = ["app", "router"];

const ROUTE_METHODS = ["get", "post", "put", "patch", "delete", "head", "options", "all"];

/** OpenAPI Path Item fields that are not operations. */
const NON_OPERATION_KEYS = new Set([
  "$ref",
  "summary",
  "description",
  "servers",
  "parameters",
]);

const VALID_EXCEPTION_OWNERS = new Set(["backend", "unimplemented"]);

// ---------------------------------------------------------------------------
// Source scanning
// ---------------------------------------------------------------------------

/**
 * Removes `//` and `/* *\/` comments without touching the contents of string,
 * template, or regular-expression literals. Commented-out route registrations
 * must not count as live routes.
 */
function stripComments(source) {
  let out = "";
  let i = 0;
  while (i < source.length) {
    const char = source[i];
    const next = source[i + 1];

    if (char === "/" && next === "/") {
      while (i < source.length && source[i] !== "\n") i += 1;
      continue;
    }

    if (char === "/" && next === "*") {
      i += 2;
      while (i < source.length && !(source[i] === "*" && source[i + 1] === "/")) {
        // Keep newlines so reported line numbers stay accurate.
        if (source[i] === "\n") out += "\n";
        i += 1;
      }
      i += 2;
      continue;
    }

    if (char === '"' || char === "'" || char === "`") {
      const quote = char;
      out += char;
      i += 1;
      while (i < source.length) {
        const inner = source[i];
        out += inner;
        i += 1;
        if (inner === "\\") {
          if (i < source.length) {
            out += source[i];
            i += 1;
          }
          continue;
        }
        if (inner === quote) break;
      }
      continue;
    }

    out += char;
    i += 1;
  }
  return out;
}

function lineStarts(source) {
  const starts = [0];
  for (let i = 0; i < source.length; i += 1) {
    if (source[i] === "\n") starts.push(i + 1);
  }
  return starts;
}

function lineAt(starts, index) {
  let low = 0;
  let high = starts.length - 1;
  while (low < high) {
    const mid = Math.ceil((low + high) / 2);
    if (starts[mid] <= index) low = mid;
    else high = mid - 1;
  }
  return low + 1;
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Collects `receivers.<method>("<literal path>", ...)` registrations.
 *
 * `sources` is an array of `{ path, source }` objects so tests can pass
 * fixtures without touching disk. Returns
 * `{ routes: [{ method, path, file, line }], unsupported: [{ reason, file, line }] }`.
 * `unsupported` holds registrations whose path is not a plain string literal:
 * a dynamic path silently disappearing from the comparison would make the
 * check lie, so callers turn these into failures.
 */
function extractRoutes(sources, options = {}) {
  const receivers = options.receivers || DEFAULT_RECEIVERS;
  const routes = [];
  const unsupported = [];

  for (const { path: file, source } of sources) {
    const code = stripComments(source);
    const starts = lineStarts(code);

    const callRe = new RegExp(
      `(^|[^\\w$.])(?:${receivers.map(escapeRegExp).join("|")})\\s*\\.\\s*(${ROUTE_METHODS.join(
        "|",
      )})\\s*\\(`,
      "g",
    );

    let match;
    while ((match = callRe.exec(code)) !== null) {
      const method = match[2].toLowerCase();
      // Report the line the receiver itself is on, not the boundary character
      // (usually the previous line's newline) that the guard consumed.
      const line = lineAt(starts, match.index + match[1].length);
      const argStart = match.index + match[0].length;

      const literal = /^\s*(["'`])((?:\\.|(?!\1)[\s\S])*?)\1/.exec(code.slice(argStart));
      if (!literal) {
        unsupported.push({
          reason: `${method.toUpperCase()} registration does not start with a string literal path`,
          file,
          line,
        });
        continue;
      }

      const routePath = literal[2];
      if (!routePath.startsWith("/")) {
        unsupported.push({
          reason: `${method.toUpperCase()} registration uses "${routePath}", which is not an absolute path`,
          file,
          line,
        });
        continue;
      }
      if (routePath.includes("${")) {
        unsupported.push({
          reason: `${method.toUpperCase()} registration builds its path at runtime (${routePath})`,
          file,
          line,
        });
        continue;
      }

      routes.push({ method, path: routePath, file, line });
    }
  }

  return { routes, unsupported };
}

// ---------------------------------------------------------------------------
// Path normalisation and aliases
// ---------------------------------------------------------------------------

function isParameterSegment(segment) {
  if (segment.startsWith(":")) return true; // Express: :id, :id(\\d+)?
  if (/^\{[^}]*\}$/.test(segment)) return true; // OpenAPI: {id}
  return segment === "*";
}

/**
 * Canonical key for a path: parameter names are dropped so the spec and the
 * code agree regardless of what each side calls the parameter.
 * `/v1/payments/:paymentId` -> `/v1/payments/:`
 */
function pathKey(rawPath) {
  const segments = String(rawPath)
    .split("/")
    .filter((segment) => segment.length > 0)
    .map((segment) => {
      const bare = segment.replace(/\(.*\)$/, "");
      return isParameterSegment(bare) ? ":" : bare;
    });
  return `/${segments.join("/")}`;
}

/**
 * Canonical keys a path may match under: itself, plus its versioned or
 * unversioned twin (the indexer keeps legacy unversioned aliases for several
 * `/v1/*` routes).
 */
function aliasKeys(rawPath) {
  const keys = new Set([pathKey(rawPath)]);
  const value = String(rawPath);
  if (value === "/v1" || value.startsWith("/v1/")) {
    const unversioned = value.slice("/v1".length) || "/";
    keys.add(pathKey(unversioned));
  } else {
    keys.add(pathKey(`/v1${value}`));
  }
  return keys;
}

/** Human-readable form of a registered path, in OpenAPI's `{param}` style. */
function displayPath(rawPath) {
  const segments = String(rawPath).split("/");
  const rendered = segments.map((segment) => {
    const bare = segment.replace(/\(.*\)$/, "");
    if (!isParameterSegment(bare)) return segment;
    return bare === "*" ? "{wildcard}" : `{${bare.replace(/^:|^\{|\}$/g, "")}}`;
  });
  return rendered.join("/") || "/";
}

// ---------------------------------------------------------------------------
// OpenAPI spec scanning
// ---------------------------------------------------------------------------

/**
 * Extracts every path + method pair from the `paths:` block of an OpenAPI
 * document. Only the block mapping is parsed (this is not a general YAML
 * parser); anything unexpected — flow-style `paths: {}`, a key we cannot read,
 * a path with no operations — throws, so the check fails loudly instead of
 * silently comparing against a partial spec.
 */
function parseSpecOperations(specSource, specLabel = "spec") {
  const lines = specSource.split(/\r?\n/);
  const start = lines.findIndex((line) => /^paths:\s*(#.*)?$/.test(line));
  if (start < 0) {
    throw new Error(
      `Could not find a top-level "paths:" block in ${specLabel} (flow-style "paths: {...}" is not supported)`,
    );
  }

  const operations = [];
  let currentPath = null;
  let currentLine = 0;
  let currentMethods = 0;

  const closePath = () => {
    if (currentPath !== null && currentMethods === 0) {
      throw new Error(`${specLabel}:${currentLine}: path "${currentPath}" has no operations`);
    }
  };

  for (let i = start + 1; i < lines.length; i += 1) {
    const line = lines[i];
    if (/^\S/.test(line) && !/^\s*$/.test(line)) break; // next top-level key
    if (/^\s*#/.test(line) || /^\s*$/.test(line)) continue;

    const pathMatch = /^ {2}(?:'([^']+)'|"([^"]+)"|(\S+?))\s*:\s*(?:#.*)?$/.exec(line);
    if (pathMatch) {
      closePath();
      currentPath = pathMatch[1] || pathMatch[2] || pathMatch[3];
      currentLine = i + 1;
      currentMethods = 0;
      if (!currentPath.startsWith("/")) {
        throw new Error(`${specLabel}:${i + 1}: "${currentPath}" is not an absolute path`);
      }
      continue;
    }

    const operationMatch = /^ {4}([A-Za-z$][\w$-]*)\s*:\s*(?:#.*)?$/.exec(line);
    if (operationMatch && currentPath !== null) {
      const key = operationMatch[1].toLowerCase();
      if (NON_OPERATION_KEYS.has(key) || key.startsWith("x-")) continue;
      if (!ROUTE_METHODS.includes(key)) {
        throw new Error(
          `${specLabel}:${i + 1}: unsupported path item field "${operationMatch[1]}" on ${currentPath}`,
        );
      }
      operations.push({
        method: key,
        path: currentPath,
        file: specLabel,
        line: i + 1,
      });
      currentMethods += 1;
      continue;
    }

    // Deeper indentation belongs to operation fields (responses, schemas, ...)
    // and is irrelevant here. Content at path/path-item indentation that did
    // not match above is a construct we refuse to guess about.
    if (/^ {2}\S/.test(line) || /^ {4}\S/.test(line)) {
      throw new Error(
        `${specLabel}:${i + 1}: could not interpret "${line.trim()}" inside the paths block`,
      );
    }
  }
  closePath();

  if (operations.length === 0) {
    throw new Error(`${specLabel}: the "paths:" block documents no operations`);
  }
  return operations;
}

// ---------------------------------------------------------------------------
// Exceptions
// ---------------------------------------------------------------------------

function parseExceptions(document, label = "exceptions") {
  const entries = Array.isArray(document) ? document : document && document.exceptions;
  if (!Array.isArray(entries)) {
    throw new Error(`${label}: expected an object with an "exceptions" array`);
  }
  return entries;
}

function parseOperationString(value) {
  const match = /^([A-Za-z]+)\s+(\/\S*)$/.exec(String(value || "").trim());
  if (!match) return null;
  return { method: match[1].toLowerCase(), path: match[2] };
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

/**
 * Compares registered routes with documented operations.
 * Returns `{ problems, stats }`; `problems` is empty when the two sides agree.
 */
function compareCoverage({ routes, operations, exceptions = [] }) {
  const problems = [];

  // The same handler is sometimes registered twice (e.g. `/v1/webhooks/public-key`
  // in both server.ts and webhooks/routes.ts). Report each operation once.
  const seenRoutes = new Set();
  routes = routes.filter((route) => {
    const key = `${route.method} ${pathKey(route.path)}`;
    if (seenRoutes.has(key)) return false;
    seenRoutes.add(key);
    return true;
  });
  const operationsByKey = new Map();
  for (const operation of operations) {
    const key = `${operation.method} ${pathKey(operation.path)}`;
    if (operationsByKey.has(key)) {
      problems.push({
        kind: "duplicate-operation",
        message: `${operation.method.toUpperCase()} ${operation.path} is documented twice in ${operation.file} (lines ${operationsByKey.get(key).line} and ${operation.line})`,
      });
      continue;
    }
    operationsByKey.set(key, operation);
  }

  const routeKeys = new Set();
  for (const route of routes) {
    for (const key of aliasKeys(route.path)) routeKeys.add(`${route.method} ${key}`);
  }

  // Exceptions are validated first so a stale one is reported even when the
  // rest of the spec is clean.
  const exceptionByKey = new Map();
  for (const entry of exceptions) {
    const parsed = parseOperationString(entry.operation);
    if (!parsed) {
      problems.push({
        kind: "invalid-exception",
        message: `exception "${entry.operation}" must look like "GET /v1/path"`,
      });
      continue;
    }
    if (!VALID_EXCEPTION_OWNERS.has(entry.owner)) {
      problems.push({
        kind: "invalid-exception",
        message: `exception "${entry.operation}" has owner "${entry.owner}" (expected one of: ${[
          ...VALID_EXCEPTION_OWNERS,
        ].join(", ")})`,
      });
      continue;
    }
    if (!entry.reason || String(entry.reason).trim().length === 0) {
      problems.push({
        kind: "invalid-exception",
        message: `exception "${entry.operation}" needs a "reason" explaining why the indexer does not serve it`,
      });
      continue;
    }

    const key = `${parsed.method} ${pathKey(parsed.path)}`;
    if (exceptionByKey.has(key)) {
      problems.push({
        kind: "invalid-exception",
        message: `exception "${entry.operation}" is listed twice`,
      });
      continue;
    }
    exceptionByKey.set(key, entry);

    const documented = operations.some(
      (operation) =>
        operation.method === parsed.method && aliasKeys(operation.path).has(pathKey(parsed.path)),
    );
    if (!documented) {
      problems.push({
        kind: "stale-exception",
        message: `exception "${entry.operation}" is not documented in the spec; remove it from the exception list`,
      });
    }
    if (routeKeys.has(key)) {
      problems.push({
        kind: "obsolete-exception",
        message: `exception "${entry.operation}" is now registered by the indexer; remove it from the exception list`,
      });
    }
  }

  const documentedOpsForRoute = (route) =>
    operations.find(
      (operation) =>
        operation.method === route.method && aliasKeys(route.path).has(pathKey(operation.path)),
    );

  const missingFromSpec = [];
  for (const route of routes) {
    if (route.method === "all") continue; // `app.all` has no single OpenAPI method
    if (documentedOpsForRoute(route)) continue;
    missingFromSpec.push(route);
  }

  for (const route of missingFromSpec) {
    problems.push({
      kind: "missing-from-spec",
      message: `${route.method.toUpperCase()} ${displayPath(route.path)} is registered at ${route.file}:${route.line} but not documented`,
    });
  }

  const unregistered = [];
  let exempted = 0;
  const exemptedByOwner = new Map();
  for (const operation of operations) {
    const key = `${operation.method} ${pathKey(operation.path)}`;
    const covered = routes.some(
      (route) => route.method === operation.method && aliasKeys(route.path).has(pathKey(operation.path)),
    );
    if (covered) continue;

    const exception = exceptionByKey.get(key);
    if (exception) {
      exempted += 1;
      exemptedByOwner.set(exception.owner, (exemptedByOwner.get(exception.owner) || 0) + 1);
      continue;
    }
    unregistered.push(operation);
  }

  for (const operation of unregistered) {
    problems.push({
      kind: "documented-not-registered",
      message: `${operation.method.toUpperCase()} ${operation.path} is documented at ${operation.file}:${operation.line} but the indexer no longer registers it`,
    });
  }

  return {
    problems,
    stats: {
      routes: routes.length,
      documentedOperations: operations.length,
      missingFromSpec: missingFromSpec.length,
      unregistered: unregistered.length,
      exempted,
      exemptedByOwner,
      exceptions: exceptionByKey.size,
    },
  };
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

function readFileOrFail(relativeOrAbsolute, label) {
  const target = path.isAbsolute(relativeOrAbsolute)
    ? relativeOrAbsolute
    : path.join(repoRoot, relativeOrAbsolute);
  try {
    return { target, content: fs.readFileSync(target, "utf8") };
  } catch (error) {
    throw new Error(`expected ${label} not found: ${target}\n  ${error.message}`);
  }
}

function runCheck(options = {}) {
  const specPath = options.spec || DEFAULT_SPEC;
  const exceptionsPath = options.exceptions || DEFAULT_EXCEPTIONS;
  const sourcePaths = options.sources || ROUTE_SOURCES;
  const receivers = options.receivers || DEFAULT_RECEIVERS;

  const spec = readFileOrFail(specPath, "OpenAPI spec");
  const exceptionsFile = readFileOrFail(exceptionsPath, "exception list");

  const sources = sourcePaths.map((sourcePath) => {
    const file = readFileOrFail(sourcePath, "route source");
    return { path: sourcePath, source: file.content };
  });

  const { routes, unsupported } = extractRoutes(sources, { receivers });
  const operations = parseSpecOperations(spec.content, specPath);

  const exceptions = parseExceptions(
    JSON.parse(exceptionsFile.content),
    exceptionsPath,
  );

  const { problems, stats } = compareCoverage({ routes, operations, exceptions });
  for (const item of unsupported) {
    problems.push({
      kind: "unsupported-registration",
      message: `${item.file}:${item.line}: ${item.reason}; the coverage check cannot verify it`,
    });
  }

  return { problems, stats, routes, operations };
}

function printReport({ problems, stats }, { verbose } = {}) {
  if (verbose) {
    console.log(
      `Checked ${stats.routes} registered route(s) against ${stats.documentedOperations} documented operation(s).`,
    );
    const byOwner = [...stats.exemptedByOwner.entries()]
      .map(([owner, count]) => `${count} ${owner}`)
      .join(", ");
    console.log(
      `Exempt documented operations: ${stats.exempted}${byOwner ? ` (${byOwner})` : ""}.`,
    );
  }

  if (problems.length === 0) {
    console.log(
      `OpenAPI route coverage OK: ${stats.routes} registered indexer route(s) are all documented, ` +
        `and ${stats.documentedOperations - stats.exempted} documented operation(s) are backed by a registered route` +
        (stats.exempted > 0 ? ` (${stats.exempted} documented exception(s) skipped).` : "."),
    );
    return;
  }

  console.error("OpenAPI route coverage check failed:\n");
  for (const problem of problems) {
    console.error(`  - ${problem.message}`);
  }
  console.error(
    "\nFix the drift between the indexer routes and docs/openapi.yaml, or record a justified" +
      " exception in docs/openapi-coverage-exceptions.json. See docs/openapi-coverage.md.",
  );
}

function parseArgs(argv) {
  const options = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--help" || arg === "-h") {
      options.help = true;
    } else if (arg === "--verbose" || arg === "-v") {
      options.verbose = true;
    } else if (arg === "--spec") {
      options.spec = argv[++i];
    } else if (arg === "--exceptions") {
      options.exceptions = argv[++i];
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  return options;
}

function usage() {
  return [
    "Usage: node scripts/check-openapi-route-coverage.js [options]",
    "",
    "Options:",
    `  --spec <path>        OpenAPI document (default ${DEFAULT_SPEC})`,
    `  --exceptions <path>  documented-but-not-indexer operations (default ${DEFAULT_EXCEPTIONS})`,
    "  --verbose, -v        print the operation counts",
    "  --help, -h           show this message",
    "",
    "Exit codes: 0 = coverage in sync, 1 = drift or unreadable input.",
  ].join("\n");
}

function main(argv = process.argv.slice(2)) {
  let options;
  try {
    options = parseArgs(argv);
  } catch (error) {
    console.error(`${error.message}\n\n${usage()}`);
    return 1;
  }

  if (options.help) {
    console.log(usage());
    return 0;
  }

  let result;
  try {
    result = runCheck(options);
  } catch (error) {
    console.error(`check-openapi-route-coverage: ${error.message}`);
    return 1;
  }

  printReport(result, options);
  return result.problems.length === 0 ? 0 : 1;
}

module.exports = {
  DEFAULT_EXCEPTIONS,
  DEFAULT_RECEIVERS,
  DEFAULT_SPEC,
  ROUTE_METHODS,
  ROUTE_SOURCES,
  aliasKeys,
  compareCoverage,
  displayPath,
  extractRoutes,
  main,
  parseExceptions,
  parseSpecOperations,
  pathKey,
  runCheck,
  stripComments,
};

if (require.main === module) {
  process.exit(main());
}
