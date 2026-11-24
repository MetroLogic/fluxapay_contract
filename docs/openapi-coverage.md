# OpenAPI route coverage (issue #1265)

[`docs/openapi.yaml`](openapi.yaml) is the machine-readable contract for the FluxaPay
HTTP API. Before this check existed, nothing verified that the document actually
covered the routes the services register: a new indexer endpoint could ship with no
spec entry, and a deleted endpoint could stay documented forever.

`scripts/check-openapi-route-coverage.js` closes that gap for the indexer's HTTP
surface. It compares the Express registrations in the indexer against the `paths`
and methods in the spec **in both directions** and fails CI with a per-route list
when they disagree.

## What runs

```bash
node scripts/check-openapi-route-coverage.js --verbose
node --test scripts/test/openapi-route-coverage.test.js   # the checker's own tests
bash docs/openapi-check.sh                                # Redocly lint
```

CI runs all three in the `OpenAPI Docs` job
([`.github/workflows/openapi-coverage.yml`](../.github/workflows/openapi-coverage.yml))
on every push to `main` and every pull request. The job needs no `npm install`:
the coverage checker is plain Node with no dependencies, and the lint step uses
`npx --yes @redocly/cli@1.25.0` exactly like `docs/openapi-check.sh` does locally.

Exit codes: `0` in sync, `1` drift or an unreadable input (the failing path is
printed).

## What is compared

Routes are read statically from the files listed in `ROUTE_SOURCES` at the top of
`scripts/check-openapi-route-coverage.js`:

| File | Registers |
| --- | --- |
| `indexer/src/server.ts` | the main API (payments, invoices, disputes, merchant settings, SSE, admin) |
| `indexer/src/webhooks/routes.ts` | webhook public key, test delivery, delivery history |

Registration styles handled: `app.get|post|put|patch|delete|head|options|all("...")`
and the same on `router`, single- or multi-line, with single, double, or backtick
quotes. `app.use(...)` middleware is not a route and is ignored; commented-out
registrations are ignored.

`indexer/src/routes/transactions.ts` defines a router that nothing mounts (it also
imports a `../db` module that does not exist), so `GET /v1/transactions` is not part
of the served surface and the file is not listed. If you mount it, add the file to
`ROUTE_SOURCES` and document the route in the same pull request.

The spec side is read from the top-level `paths:` mapping: each `  /path:` key and
each `    <method>:` below it becomes one operation. Path-item fields (`parameters`,
`summary`, `servers`, `$ref`) are not operations.

## Matching rules

The comparison is deterministic, and these are the only relaxations:

- **Parameter names are ignored.** Express `:id` and OpenAPI `{id}` both normalise to
  one parameter segment, so `/v1/invoices/:id` matches `/v1/invoices/{invoiceId}`.
- **Trailing and repeated slashes are insignificant.** `/v1/payments/` == `/v1/payments`.
- **An unversioned path and its `/v1` twin are aliases.** The indexer keeps legacy
  paths (`/events/stream`, `/webhooks/public-key`, `/payments/:paymentId`,
  `/admin/replay`) alongside their versioned equivalents. Documenting either form
  covers both registrations, and you do not need duplicate spec entries.
- **Each operation is reported once.** The webhook public-key handler is registered in
  both `server.ts` and `webhooks/routes.ts`; that shows up as one operation.

Everything else must match exactly — including the method and the case of literal
segments.

## Adding or changing an indexer route

1. Register the route in `indexer/src` (and add the file to `ROUTE_SOURCES` if it is a
   new module that `server.ts` mounts).
2. Add the matching `path` + method to `docs/openapi.yaml`, documenting the path the
   code serves. Use OpenAPI's `{param}` form for parameters, declare every path
   parameter, give the operation an `operationId`, and set `security` to
   `apiKeyAuth` or `[]` when the route is not SEP-10 JWT authenticated.
3. Run the checker and the Redocly lint from the commands above.
4. If the route replaces a documented one, delete the old spec entry in the same
   pull request — the check fails when a documented operation is no longer registered.

## Documented operations the indexer does not serve

Two kinds of documented operations are legitimately not in the indexer:

- **Served by `backend/`** — the separate Express API service, for example
  `POST /v1/payments` and `POST /v1/invoices`.
- **Not implemented yet** — operations from the original spec (PR #990) that have no
  HTTP handler in either service, for example `POST /v1/fx/convert`.

Both are recorded in
[`docs/openapi-coverage-exceptions.json`](openapi-coverage-exceptions.json) with an
`owner` (`backend` or `unimplemented`) and a `reason`. The list is ratcheted: the
check fails if

- an exception is no longer documented in the spec (delete the entry),
- an exception is now registered by the indexer (delete the entry), or
- an exception is duplicated, missing a reason, or has an unknown owner.

So the list can only shrink, and every entry carries its justification for review.
Do **not** add an exception for a route that exists but is simply undocumented —
document the route instead.

## Limits of the check

- It is static, not runtime: it cannot prove a route is reachable, only that its
  registration is documented. Middleware-mounted prefixes (`app.use("/x", router)`)
  are handled by listing the router's module and writing the full mounted path in the
  spec, as `webhooks/routes.ts` does.
- A registration whose path is dynamic (a variable or an interpolated template
  literal) cannot be verified, so the checker fails and names the file and line
  instead of skipping it silently.
- The spec reader understands block-style `paths:` mappings only. Flow-style
  `paths: {...}`, YAML anchors, or a path item without operations make the checker
  fail loudly rather than compare against a partial document.
- The aliasing rule means a `/v1`-prefixed spec entry also covers its unversioned
  twin. Document the path the code actually serves when the two differ in meaning.
