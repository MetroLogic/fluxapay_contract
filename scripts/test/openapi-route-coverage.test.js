/**
 * Unit tests for scripts/check-openapi-route-coverage.js (Issue #1265).
 *
 * The checker is what stops an indexer route from shipping without a matching
 * entry in docs/openapi.yaml (and the reverse). These tests pin the two things
 * that make that promise meaningful:
 *   - static extraction sees every registration style used in the indexer, and
 *   - path comparison is deterministic for parameter names and for the
 *     versioned/unversioned aliases the indexer keeps.
 */

const assert = require("node:assert");
const path = require("node:path");
const os = require("node:os");
const fs = require("node:fs");
const { test } = require("node:test");

const {
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
} = require("../check-openapi-route-coverage");

const SOURCE = "indexer/src/server.ts";

function spec(paths) {
  return ["openapi: 3.1.0", "info:", "  title: Test", "  version: 1.0.0", "paths:", paths].join("\n");
}

function routesFrom(source, receivers) {
  return extractRoutes([{ path: SOURCE, source }], receivers ? { receivers } : {});
}

test("stripComments removes line and block comments but keeps strings intact", () => {
  const source = [
    'const url = "http://example.test//x";',
    "// app.get('/commented-out', handler);",
    "/* app.post('/also-commented', handler); */",
    "app.get('/live', handler);",
  ].join("\n");

  const stripped = stripComments(source);
  assert.ok(stripped.includes('"http://example.test//x"'), "string contents must be preserved");
  assert.ok(!stripped.includes("commented-out"));
  assert.ok(!stripped.includes("also-commented"));
  assert.ok(stripped.includes("app.get('/live'"));
});

test("extractRoutes collects app/router registrations, including multi-line ones", () => {
  const source = [
    'app.get("/health", handler);',
    "app.post(",
    '  "/v1/payments",',
    "  middleware,",
    "  handler,",
    ");",
    "router.get('/v1/transactions', handler);",
    "app.use('/webhooks', requireScope('manage:webhooks'));",
    '// app.delete("/v1/invoices/:id", handler);',
    'app.put(`/v1/merchant/settings/fee-recipient`, handler);',
  ].join("\n");

  const { routes, unsupported } = routesFrom(source);
  assert.deepStrictEqual(
    routes.map((route) => `${route.method.toUpperCase()} ${route.path}`),
    ["GET /health", "POST /v1/payments", "GET /v1/transactions", "PUT /v1/merchant/settings/fee-recipient"],
  );
  assert.deepStrictEqual(unsupported, []);
  assert.strictEqual(routes[1].line, 2, "multi-line registrations report the line of the call");
  assert.strictEqual(routes[0].file, SOURCE);
});

test("extractRoutes ignores other receivers and reports paths it cannot verify", () => {
  const source = [
    'client.get("/v1/remote", handler);',
    "app.get(dynamicPath, handler);",
    "app.get('relative/path', handler);",
    "app.get(`/v1/dynamic/${id}`, handler);",
  ].join("\n");

  const { routes, unsupported } = routesFrom(source);
  assert.deepStrictEqual(routes, [], "only app/router literals become routes");
  assert.strictEqual(unsupported.length, 3);
  assert.ok(unsupported.every((item) => item.file === SOURCE));
});

test("extractRoutes can be pointed at other receiver names", () => {
  const { routes } = routesFrom('apiRouter.get("/v1/things", handler);', ["apiRouter"]);
  assert.deepStrictEqual(
    routes.map((route) => route.path),
    ["/v1/things"],
  );
});

test("pathKey ignores parameter names and trailing slashes", () => {
  assert.strictEqual(pathKey("/v1/payments/:paymentId"), pathKey("/v1/payments/{id}"));
  assert.strictEqual(pathKey("/v1/invoices/:id/"), pathKey("/v1/invoices/{invoice_id}"));
  assert.strictEqual(pathKey("//v1/merchants//leaderboard"), pathKey("/v1/merchants/leaderboard"));
  assert.notStrictEqual(pathKey("/v1/payments/:id"), pathKey("/v1/payments/:id/refunds"));
});

test("aliasKeys pairs unversioned paths with their /v1 twin", () => {
  assert.ok(aliasKeys("/events/stream").has(pathKey("/v1/events/stream")));
  assert.ok(aliasKeys("/v1/events/stream").has(pathKey("/events/stream")));
  assert.ok(aliasKeys("/payments/:paymentId").has(pathKey("/v1/payments/{id}")));
  assert.ok(!aliasKeys("/admin/replay-dlq").has(pathKey("/v1/admin/replay")));
});

test("displayPath renders Express parameters in OpenAPI style", () => {
  assert.strictEqual(displayPath("/v1/invoices/:id/pdf"), "/v1/invoices/{id}/pdf");
  assert.strictEqual(displayPath("/health"), "/health");
});

test("parseSpecOperations reads path + method pairs and skips path-item metadata", () => {
  const document = spec(
    [
      "  /health:",
      "    get:",
      "      summary: Health probe",
      "      responses: {}",
      "  /v1/payments/{id}:",
      "    parameters:",
      "      - $ref: '#/components/parameters/Id'",
      "    get:",
      "      responses: {}",
      "    delete:",
      "      responses: {}",
      "components:",
      "  schemas: {}",
    ].join("\n"),
  );

  const operations = parseSpecOperations(document, "test.yaml");
  assert.deepStrictEqual(
    operations.map((operation) => `${operation.method.toUpperCase()} ${operation.path}`),
    ["GET /health", "GET /v1/payments/{id}", "DELETE /v1/payments/{id}"],
  );
  assert.strictEqual(operations[0].line, 7, "reports the line of the operation key");
});

test("parseSpecOperations fails loudly on a spec it cannot read completely", () => {
  assert.throws(() => parseSpecOperations("info:\n  title: x\n"), /paths:/);
  assert.throws(() => parseSpecOperations(spec("  /health: {get: {}}"), "flow.yaml"), /could not interpret/);
  assert.throws(() => parseSpecOperations(spec("  /health:\n    summary: no operations"), "empty.yaml"), /no operations/);
  assert.throws(
    () => parseSpecOperations(spec("  /health:\n    fetch:\n      responses: {}"), "odd.yaml"),
    /unsupported path item field "fetch"/,
  );
});

test("compareCoverage passes when both sides agree", () => {
  const { routes } = routesFrom(
    ['app.get("/v1/payments", handler);', 'app.get("/health", handler);'].join("\n"),
  );
  const operations = parseSpecOperations(
    spec(["  /v1/payments:", "    get:", "      responses: {}", "  /health:", "    get:", "      responses: {}"].join("\n")),
    "test.yaml",
  );

  const { problems, stats } = compareCoverage({ routes, operations });
  assert.deepStrictEqual(problems, []);
  assert.strictEqual(stats.routes, 2);
  assert.strictEqual(stats.exempted, 0);
});

test("compareCoverage matches parameterised paths and aliases in both directions", () => {
  const { routes } = routesFrom(
    [
      'app.get("/payments/:paymentId", handler);',
      'app.get("/events/stream", handler);',
    ].join("\n"),
  );
  const operations = parseSpecOperations(
    spec(
      [
        "  /v1/payments/{id}:",
        "    get:",
        "      responses: {}",
        "  /v1/events/stream:",
        "    get:",
        "      responses: {}",
      ].join("\n"),
    ),
    "test.yaml",
  );

  const { problems } = compareCoverage({ routes, operations });
  assert.deepStrictEqual(problems, []);
});

test("compareCoverage reports routes that are missing from the spec", () => {
  const { routes } = routesFrom(
    ["app.get('/v1/payments', handler);", "app.post('/v1/merchants/leaderboard', handler);"].join("\n"),
  );
  const operations = parseSpecOperations(
    spec(["  /v1/payments:", "    get:", "      responses: {}"].join("\n")),
    "test.yaml",
  );

  const { problems } = compareCoverage({ routes, operations });
  assert.strictEqual(problems.length, 1);
  assert.match(problems[0].message, /POST \/v1\/merchants\/leaderboard/);
  assert.match(problems[0].message, /indexer\/src\/server\.ts:2/);
  assert.match(problems[0].message, /not documented/);
});

test("compareCoverage reports documented operations the indexer no longer registers", () => {
  const { routes } = routesFrom('app.get("/health", handler);');
  const operations = parseSpecOperations(
    spec(
      [
        "  /health:",
        "    get:",
        "      responses: {}",
        "  /v1/refunds:",
        "    post:",
        "      responses: {}",
      ].join("\n"),
    ),
    "test.yaml",
  );

  const { problems } = compareCoverage({ routes, operations });
  assert.strictEqual(problems.length, 1);
  assert.match(problems[0].message, /POST \/v1\/refunds/);
  assert.match(problems[0].message, /no longer registers it/);
});

test("a justified exception suppresses the failure and is counted", () => {
  const { routes } = routesFrom('app.get("/health", handler);');
  const operations = parseSpecOperations(
    spec(["  /health:", "    get:", "      responses: {}", "  /v1/refunds:", "    post:", "      responses: {}"].join("\n")),
    "test.yaml",
  );
  const exceptions = [
    { operation: "POST /v1/refunds", owner: "unimplemented", reason: "No HTTP handler yet." },
  ];

  const { problems, stats } = compareCoverage({ routes, operations, exceptions });
  assert.deepStrictEqual(problems, []);
  assert.strictEqual(stats.exempted, 1);
  assert.strictEqual(stats.exemptedByOwner.get("unimplemented"), 1);
});

test("exceptions are rejected when stale, obsolete, duplicated, or malformed", () => {
  const { routes } = routesFrom('app.get("/health", handler);');
  const operations = parseSpecOperations(
    spec(["  /health:", "    get:", "      responses: {}", "  /v1/refunds:", "    post:", "      responses: {}"].join("\n")),
    "test.yaml",
  );

  const stale = compareCoverage({
    routes,
    operations,
    exceptions: [{ operation: "GET /v1/ghost", owner: "backend", reason: "Gone." }],
  });
  assert.ok(stale.problems.some((problem) => /not documented in the spec/.test(problem.message)));

  const obsolete = compareCoverage({
    routes,
    operations,
    exceptions: [{ operation: "GET /health", owner: "backend", reason: "Served elsewhere." }],
  });
  assert.ok(obsolete.problems.some((problem) => /now registered by the indexer/.test(problem.message)));

  const missingReason = compareCoverage({
    routes,
    operations,
    exceptions: [{ operation: "POST /v1/refunds", owner: "backend" }],
  });
  assert.ok(missingReason.problems.some((problem) => /needs a "reason"/.test(problem.message)));

  const badOwner = compareCoverage({
    routes,
    operations,
    exceptions: [{ operation: "POST /v1/refunds", owner: "somewhere", reason: "x" }],
  });
  assert.ok(badOwner.problems.some((problem) => /has owner "somewhere"/.test(problem.message)));

  const duplicate = compareCoverage({
    routes,
    operations,
    exceptions: [
      { operation: "POST /v1/refunds", owner: "backend", reason: "x" },
      { operation: "POST /v1/refunds", owner: "backend", reason: "y" },
    ],
  });
  assert.ok(duplicate.problems.some((problem) => /listed twice/.test(problem.message)));

  const malformed = compareCoverage({
    routes,
    operations,
    exceptions: [{ operation: "refunds", owner: "backend", reason: "x" }],
  });
  assert.ok(malformed.problems.some((problem) => /must look like/.test(problem.message)));
});

test("parseExceptions accepts both a bare array and an { exceptions: [] } document", () => {
  assert.deepStrictEqual(parseExceptions([]), []);
  assert.deepStrictEqual(parseExceptions({ exceptions: [{ operation: "GET /x" }] }), [
    { operation: "GET /x" },
  ]);
  assert.throws(() => parseExceptions({ nope: true }), /expected an object with an "exceptions" array/);
});

test("runCheck fails with the missing path when the spec does not exist", () => {
  assert.throws(
    () => runCheck({ spec: path.join(os.tmpdir(), "definitely-missing-openapi.yaml") }),
    /expected OpenAPI spec not found/,
  );
});

test("runCheck and the CLI pass on this repository's real spec and routes", () => {
  const { problems, stats } = runCheck();
  assert.deepStrictEqual(
    problems.map((problem) => problem.message),
    [],
    "indexer routes and docs/openapi.yaml must stay in sync",
  );
  assert.ok(stats.routes > 30, `expected the indexer surface to be scanned (got ${stats.routes})`);
  assert.ok(stats.documentedOperations > 20);

  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "openapi-coverage-"));
  try {
    assert.strictEqual(main(["--verbose"]), 0);
    assert.strictEqual(main(["--help"]), 0);
    assert.strictEqual(main(["--nope"]), 1);
    assert.strictEqual(main(["--spec", path.join(tempDir, "missing.yaml")]), 1);
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
  }
});
