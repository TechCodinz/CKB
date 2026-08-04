# CKB Audit Report

> **Update (feature-building session):** after the security/reliability audit
> below, a second working session added eight new features on top of the
> hardened codebase — see `FEATURES_ADDED.md` for what they are, what's real
> vs. MVP-scoped, and the new environment variables / Prisma migration they
> require. `BUILD_INSTRUCTIONS.md` has the updated, ordered build/verify
> checklist covering both sessions. Everything below this line is the
> original audit; it's kept as-is since it's still accurate for the files it
> covers.



Static read-through audit (no `cargo`/`npm install` available in this environment, so
nothing here was compiled or run — treat this as a thorough code review, and run your
own build/test pass before deploying). Scope: `core/`, `cli/`, `mcp-server/`, `backend/`,
`web/`, `web/api/`, `integrations/`.

## Fixed in this pass

### Money — payments never actually unlocked the product
- **Coinbase Commerce webhook had zero signature verification** (the code even said
  so in a comment) and the "grant access" step was commented out entirely — a paying
  customer got nothing, and anyone who found the URL could forge a fake
  `charge:confirmed` event to grant themselves a paid plan. Now verifies the HMAC
  signature and calls a new internal endpoint to actually upgrade the account.
- **Flutterwave provisioning never touched `User.plan`** — it created a `Subscription`
  row but every plan-gated check in the app reads `User.plan` directly, so paying
  Flutterwave customers were never unlocked either. Fixed, and the signature check is
  now constant-time and requires a real secret in production (the fallback default is
  public since it's in this repo).
- **Cancelled/failed Stripe subscriptions never downgraded the account** — `plan`
  stayed `'pro'` forever. Fixed in both the deletion and the update webhook handlers.
- **The real Stripe/Flutterwave router (`routes/pricing.ts`) was never mounted** in
  `server.ts` at all, and internally used a mocked `authenticate` that always
  resolved to a fake `user_123` and a mocked Prisma client. Wired it into the app with
  the real shared auth middleware.
- **The Stripe webhook would have failed signature verification anyway** —
  `express.json()` was applied globally *before* the route-specific `express.raw()`,
  so by the time Stripe's SDK tried to verify the signature, the raw bytes it needs
  were already gone. Fixed the middleware order.

You're running three payment providers in parallel (Coinbase, Stripe, Flutterwave).
They all work now, but I'd pick one primary provider — three webhook-security
surfaces and three reconciliation paths is a lot for a single-founder SaaS.

### Auth — real login failures were silently swallowed
- `ckbApi.login()` / `ckbApi.register()` in `web/src/services/api.ts`, **and**
  `Login.tsx` / `Signup.tsx`'s own `catch` blocks, caught *any* error from the backend
  — including a genuine 401 for a wrong password or 409 for a duplicate email — and
  silently handed back a client-side mock token, logging the user straight into a
  fake session instead of telling them the login failed. Fixed: real 4xx responses
  now surface as an actual error message; the fallback only fires when there's no
  backend response at all. The explicit "Launch Instant Demo" button is left alone —
  that's presumably the intended trial-access feature.
- `JWT_SECRET` had a hardcoded fallback (`'ckb-dev-secret-change-in-production'`).
  The backend now refuses to start in production without a real one set.
- No brute-force protection specific to `/auth/login` — it shared the generic
  100-req/15-min limiter with everything else. Added a dedicated 10-req/15-min
  limiter on the auth routes.

### Server-side authorization gaps
- No ownership check existed for fetching/deleting a single project — added
  `GET/DELETE /api/v1/projects/:id`, scoped to `req.user.id` (previously the only
  scoped route was the list endpoint).
- `ApiKeyService` and `AuditService` were fully built in `security/` but never
  mounted anywhere in the app. Wired up `GET/POST/DELETE /api/v1/api-keys`, and added
  audit-log calls on register/login.

### The MCP REST server was a fully open, unauthenticated, arbitrary-path scanner
`mcp-server/src/main.rs` bound to `0.0.0.0`, allowed CORS from `Any` origin, and had
no auth on any route — including `/api/v1/scan`, which scans whatever filesystem path
is in the POST body. Anyone who could reach the port could trigger scans of arbitrary
paths on the host. Added an optional `CKB_API_KEY` header check (bearer or
`X-API-Key`) on all `/api/v1/*` routes, made CORS origin configurable via
`CKB_ALLOWED_ORIGIN`, and changed the default bind to `127.0.0.1` (opt in to wider
exposure with `CKB_BIND_ALL=1`). `--stdio` mode (used for local MCP client
integration) is unchanged — that's a trusted local pipe, not a network listener.

### Reliability — panics with cascading blast radius
- Every language parser (`go.rs`, `java.rs`, `python.rs`, `rust.rs`,
  `typescript.rs`) called `self.parser.lock().unwrap()`. If *any* file ever panicked
  mid-parse, the mutex becomes **poisoned**, permanently breaking that parser for
  every subsequent scan on that engine instance — one bad file in one user's repo
  could take down parsing for the whole process. Fixed to recover from poison instead
  of unwrapping it.
- `child.utf8_text(...).unwrap()` (20+ call sites across all five parsers) could
  panic on any file. Now falls back to an empty string instead of crashing the scan.
- `CkbEngine::new()` unconditionally called `.build_global().unwrap()` on rayon's
  thread pool, which can only be initialized once per process — a second
  `CkbEngine::new()` call (plausible from the Node/WASM bindings, or any long-running
  host that creates more than one engine) would panic. Now tolerates "already
  initialized" and reuses the existing pool.
- A few `.partial_cmp(...).unwrap()` calls (sorting by a `f64` confidence/risk score)
  would panic on `NaN`. Given `NaN` scores are plausible from a division-by-zero in
  the analysis code, these are now `unwrap_or(Ordering::Equal)`.
- CLI's `path.to_str().unwrap()` (9 call sites) panicked on non-UTF-8 paths. Switched
  to `.to_string_lossy()`, matching a pattern already used elsewhere in the same file.

### Repo hygiene
- `integrations/jetbrains/.gradle/` (a local Gradle build cache — checksums, locks,
  file-hash caches) was committed to the repo. Removed it and added `.gradle/` to
  `.gitignore` so it doesn't come back. Worth also double-checking
  `integrations/vscode/ckb-vscode-1.0.0.vsix` — I left it since a prebuilt package
  might be intentionally shipped for direct install, but if that's not deliberate it
  should be a CI release artifact, not a committed binary.

## New environment variables (documented in `.env.example`)
- `JWT_SECRET` — now required in production.
- `INTERNAL_API_SECRET` / `BACKEND_URL` — used by the Coinbase webhook (a separate
  Vercel deployment) to call the backend's new `/api/v1/internal/grant-access` route.
- `CKB_API_KEY` / `CKB_ALLOWED_ORIGIN` / `CKB_BIND_ALL` — MCP server auth/exposure
  config.
- `FLUTTERWAVE_SECRET_HASH` reference added (was used in code but undocumented).

## Fourth pass — compile-correctness review (no compiler available, so this is a manual type/logic re-read of every edited file)

Since I still can't actually compile anything in this environment, I went back
through every file this audit touched and manually traced types, imports, and
control flow the way a compiler would, specifically hunting for mistakes my own
earlier edits might have introduced. Found and fixed two real ones:

1. **A middleware-ordering bug I introduced in pass one**: mounting
   `routes/pricing.ts`'s router *before* the global `express.json()` call (to
   preserve the raw body Stripe's webhook needs) also meant every *other* route
   in that router — `create-checkout`, `subscription/update`,
   `subscription/cancel`, `flutterwave/initialize` — never got a parsed
   `req.body` at all, since none of them apply their own body parser and were
   relying on the global one running first. Fixed by moving the JSON-parsing
   decision earlier: a single conditional middleware now parses JSON for every
   path except the exact Stripe webhook path, and the pricing router mounts
   after that (and after the rate limiter, so billing routes are still rate
   limited) — so the webhook still gets raw bytes, and everything else gets a
   normal parsed body.
2. **A type mismatch**: `req.headers['user-agent']` is typed
   `string | string[] | undefined` in Express, but `AuditLogEntry.userAgent`
   expects `string | undefined` — assigning the former to the latter is a type
   error regardless of `strict` mode (this project has `strict: false`, but
   TypeScript still checks structural assignability). Fixed by switching to
   `req.get('user-agent')`, which is correctly typed `string | undefined`.

Everything else — the Rust struct changes (`ScanReport.duration_ms`, the
restructured `IntelligenceBenchmarkMetrics`, the mutex-poison recovery pattern
repeated across 5 parser files, the `federation/mod.rs` rewrite, the new
`detect_semantic_clones_at`/`/api/v1/clones` route, the Node/Python SDK
methods), and the TypeScript backend changes (Prisma field usage against the
actual schema, `apiKeyService`/`paymentService` method signatures against their
real implementations) — were traced by hand against their actual definitions
and call sites and look correct. That's still not the same as a compiler
confirming it, though — see `BUILD_INSTRUCTIONS.md` for the exact commands to
run to close that gap, and treat `cargo build --workspace` as the first thing
to run, since it's the one most likely to surface something this manual review
missed.

## Third pass — fabricated data and non-functional "advanced" features

This pass found the most serious issue in the whole audit, so it gets called out
first: **parts of the org-intelligence/federation feature were presenting made-up
numbers as real measurements.**

### `federation/mod.rs` was generating fake analytics, not real ones
Three separate problems in `core/src/federation/mod.rs`, all now fixed:

1. **`IntelligenceBenchmarkMetrics::default()` hardcoded constants** — "14,200
   files/sec", "96.8% impact prediction accuracy", "1.2% false positive rate",
   "98.4% blast radius precision", "48.2MB memory usage" — that never changed
   regardless of what was actually scanned, served directly by the
   `/api/v1/metrics/intelligence` REST endpoint. This is the one I'd flag as
   genuinely risky to ship to paying customers: it's not a bug so much as fabricated
   data presented as fact. **Removed** the fake fields entirely and replaced them
   with only things that can honestly be computed: total files indexed, total
   violations detected, and average indexing speed — the last of which required
   adding *real* timing instrumentation (`ScanReport.duration_ms`, measured with
   `std::time::Instant` in `scan_codebase`/`scan_incremental`, which didn't track
   timing at all before). Fields like "prediction accuracy" and "false positive
   rate" need accumulated ground-truth (users confirming/dismissing violations over
   time) that doesn't exist yet in this codebase — that's a real feature to build,
   not something a single scan can produce, so I left it out rather than fabricate
   a number for it.

2. **Cross-repo dependency edges were fabricated for every single repo pair**,
   unconditionally, regardless of whether the repos had any actual relationship —
   `federate()` would report `N × (N-1)` "cross-service API calls" for any set of N
   federated repos every time, which is fiction for any org with more than a
   couple of services. Changed to only report an edge when there's real textual
   evidence (one repo's detected patterns or drift violations actually mention the
   other repo's name) — a conservative heuristic, not a full fix (see the limitation
   noted in the code: real cross-repo resolution needs each repo's full import
   graph, which `ScanReport`'s aggregate counts don't carry — worth a follow-up if
   you want to lean on this feature for real).

3. **"Technical debt %" and "architectural violations" were pure formulas from
   node/edge counts** (`edges * 0.05`, `nodes / 10 + 1`) with no connection to
   real detected problems — even though the same `ScanReport` already carries a
   real `drift: Vec<DriftViolation>` list from the actual drift detector. Now
   computed from real violations, weighted by severity.

### Two "advanced" analysis features were silently no-ops
- **Test coverage gaps** — covered in the second pass above (substring-matching
  false positives hid real gaps).
- **Semantic clone detection** (`ckb_detect_semantic_clones` MCP tool) — the
  handler passed an **empty `HashMap`** as the file contents to analyze, every
  single time, so the tool always reported "0 clones found" no matter what was in
  the repo, despite being advertised as a real capability in the tool list. Added
  `CkbEngine::detect_semantic_clones_at(path)`, which actually discovers and reads
  files from disk (same approach `scan_codebase` uses), wired the MCP tool to call
  it with a required `repo_path` argument, and added a matching REST route
  (`POST /api/v1/clones`) plus SDK methods (`detectClones`/`detect_clones`) so
  it's reachable the same way the other advanced-analysis features are.

## Fourth-pass status: what's real now vs. still a known gap

To be direct about where things stand for a "deploy for mass usage" bar:

**Now backed by real computation** (this audit): payments actually grant/revoke
access; auth failures actually surface as errors; the MCP server requires a key;
parsers don't cascade-fail from one bad file; test coverage gaps use real path
matching; org-intelligence benchmarks are computed, not hardcoded; cross-repo edges
require textual evidence instead of being fabricated for every pair; semantic clone
detection reads real files; Node/Python SDKs are real, working HTTP clients.

**Still a known limitation, not fixed in this pass** (documented in-code where
relevant, listed here for visibility):
- Cross-repo dependency detection is evidence-based but shallow — it can't do real
  import-graph resolution across repos without a larger data-plumbing change (each
  repo's full `DependencyGraph`, not just its `ScanReport` summary, would need to
  be exchanged).
- "Impact prediction accuracy" / "false positive rate" style metrics need an
  accumulated feedback loop (users confirming/dismissing violations over time) that
  doesn't exist yet — there's no field, storage, or endpoint for it. If you want
  this, it's a real feature to design (probably: a `violation_id` + `feedback:
  confirmed|dismissed` table, aggregated over time).
- `core/src/analysis/clone_detector.rs`'s hash isn't actually a rolling hash despite
  its doc comment (see second-pass notes) — correct output, just not the O(n)
  algorithm it claims to be. Fine at typical repo sizes.
- Federation/org-analytics currently only ever gets called with a single repo (the
  MCP server's `get_org_analytics` handler hardcodes a one-entry map keyed
  `"ckb-core-platform"` from its own `latest_report`). True multi-repo federation
  would need a route that accepts scan reports from multiple registered
  repos/services — right now there's no server-side mechanism to register or store
  more than one repo's report at a time.

## Second pass — additional fixes

### Test Coverage Gap Analysis had a false-positive bug that hid real gaps
`core/src/analysis/test_coverage.rs` classified files as "test files" (and callers
as "this is tested") using `path.contains("test")` / `.contains("spec")` — plain
substring matching. That also matches `latest.rs`, `contest.py`, `attestation.go`,
`protestor.ts`, `respected.js`, etc. Any function whose only caller happened to live
in a file like `latest_handler.rs` would be silently marked "covered" and dropped
from the untested-hotpaths report — quietly hiding real coverage gaps in exactly the
feature meant to surface them. Replaced with `is_test_path()`, which checks path
*segments* (`test/`, `tests/`, `__tests__/`, `spec/`) and filename conventions
(`test_foo.py`, `foo_test.go`, `foo.spec.ts`, `FooTest.java`, etc.) instead of raw
substrings.

### Node.js and Python "bindings" were placeholder garbage, not code
Every file under `bindings/node/` and `bindings/python/` literally contained nothing
but a comment with its own filename — e.g. `bindings/node/index.js` was the single
line `# index.js`, which isn't even valid JavaScript (`require()`-ing it would throw
a `SyntaxError` immediately). Same for `package.json`, `setup.py`, `__init__.py`,
`core.py`. Good news: none of this was referenced in the main `README.md`, so it
wasn't actively misleading anyone browsing the repo — it was just dead scaffolding.

Replaced both with real, working thin HTTP clients against the MCP REST API (no
native compilation/N-API/PyO3 needed):
- **`bindings/node/`** — `@ckb/sdk`, using Node 18+'s built-in `fetch`, zero
  dependencies. `index.js` + `index.d.ts` + `README.md`. Syntax-checked with
  `node --check` and a smoke-instantiation in this environment.
- **`bindings/python/`** — `ckb-sdk`, stdlib-only (`urllib`), zero dependencies.
  `ckb/core.py`, `ckb/__init__.py`, `setup.py`, `README.md`. Syntax-checked with
  `python3 -m py_compile` in this environment.

Both mirror the same method set (`scan`, `get_report`/`getReport`, `analyze_impact`/
`analyzeImpact`, `search`, drift timeline, test gaps, rule generation, org analytics,
intelligence metrics) and support the `CKB_API_KEY` header auth added in the MCP
server hardening above.

### The WASM binding's `scan()` was a fake success response
`bindings/wasm/src/lib.rs`'s doc comment said it "delegates to the MCP server via
fetch", but the function body never called `fetch` at all — it just returned a
hardcoded `{"status": "scan_delegated", ...}` string unconditionally. Every call
"succeeded" instantly regardless of whether a server was even running. Rewrote it to
actually perform a browser `fetch()` (via `web-sys`/`wasm-bindgen-futures`,
added as new Cargo dependencies) to `POST /api/v1/scan` and `GET /api/v1/report`,
including optional `X-API-Key` header support, and to propagate real HTTP errors
back to the caller instead of always claiming success. **I could not build this**
(no `wasm-pack`/`wasm32` target or network access for crates in this environment) —
the code follows the standard `web-sys` fetch pattern, but please run
`wasm-pack build` yourself before relying on it.

### Repo hygiene
- `integrations/jetbrains/.gradle/` (a local Gradle build cache) was committed.
  Removed and gitignored. `integrations/vscode/ckb-vscode-1.0.0.vsix` left in place —
  worth confirming that's an intentional shipped artifact and not an accident.

### Reviewed, no changes needed
- `core/src/graph/mod.rs`, `core/src/analysis/{drift,boundaries}.rs`,
  `core/src/storage/mod.rs` — structurally sound on read-through; boundary
  inferencers and drift rules didn't show obvious correctness bugs.
- `core/src/analysis/clone_detector.rs` — functionally correct, but worth knowing:
  its doc comment calls it a "rolling hash", but it fully recomputes the hash (and
  re-allocates/re-joins the token-normalized string) for every 8-line window instead
  of incrementally updating between adjacent windows. Not wrong, just O(n · window)
  instead of the O(n) a true rolling hash would give you — likely fine at typical
  file sizes, worth revisiting if clone detection gets slow on very large files.
- `App.tsx`, `Dashboard.tsx`, `ProjectView.tsx`, `GraphView.tsx`, `Navbar.tsx` — no
  `dangerouslySetInnerHTML`/`eval`, auth gating is a simple (and appropriately so,
  given it's paired with real server-side checks) "is there a token" check.

## Not yet covered
- `core/src/telemetry/otlp.rs` (OTLP span ingestion) was read and is structurally
  sound and real (not fabricated) — but note the mapping from an incoming span's
  `name` field directly to a graph `NodeId` only works if whatever's emitting the
  spans names them to exactly match CKB's internal `"{file_path}::{function_name}"`
  convention. Most APM/OTLP setups name spans after routes or handler names (e.g.
  `"GET /api/users"`), not that convention, so in practice this correlation will
  rarely match up without either the instrumented app adopting CKB's naming scheme
  or CKB adding a name-resolution/fuzzy-matching layer. Didn't change this — it's a
  design gap in how the feature would need to integrate with a real APM pipeline,
  not a bug in the ingestion code itself.
- Automated pre-deploy checklist — nothing in this audit was compiled (`cargo
  check`/`build`), type-checked (`tsc`), or `wasm-pack build`'d, since this
  environment has no network access for dependency resolution. **Before deploying,
  please run, in order:**
  1. `cargo build --workspace` (core, cli, mcp-server) — this is the one most
     likely to surface an issue, since several edits touched shared structs
     (`ScanReport` gained a field, `IntelligenceBenchmarkMetrics` was restructured).
  2. `cd backend && npm install && npm run build` (or `tsc --noEmit`)
  3. `cd web && npm install && npm run build`
  4. `cd bindings/wasm && wasm-pack build` if you plan to use the WASM binding
  5. A real end-to-end smoke test of the payment flows (Stripe test mode +
     Flutterwave test mode + Coinbase Commerce test webhook) — these had the
     highest-stakes bugs in this audit and deserve to be exercised for real, not
     just read.
