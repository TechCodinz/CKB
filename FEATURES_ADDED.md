# Features Added — Feature-Building Session

> **Update (marketplace-readiness pass):** a further session fixed real bugs
> in the VS Code and JetBrains extensions themselves, plus three pre-existing
> CI/release workflows that had never been reviewed (one was a hard-blocking
> bug that would fail every release build; two were empty stub files). See
> the dedicated section near the end of this document, and
> `OPEN_SOURCE_STRATEGY.md` for the public/private repo split plan.

Built on top of the hardened codebase from `AUDIT_REPORT.md`. Same caveat
applies: **nothing in this session was compiled either** (no Rust toolchain,
no `npm install` with registry access in the build sandbox). Everything below
was written carefully and then manually re-traced against real type
definitions and call sites in a dedicated compile-correctness pass — that
pass caught and fixed two real bugs (documented below) — but manual tracing
is not a compiler. Run the checklist in `BUILD_INSTRUCTIONS.md` before
deploying.

## 1. Real cross-repo dependency detection

`ScanReport` gained two new fields, populated during every scan:
- `package_identity: Option<String>` — the repo's own declared name, read
  from `package.json`/`Cargo.toml`/`go.mod`/`pyproject.toml` at the scan root.
- `external_dependencies: Vec<String>` — the deduplicated set of real,
  parsed, non-relative import sources found across every file (already-parsed
  data — nothing new to compute, just aggregated).

`federation::FederatedGraphEngine::federate()` now matches repo A's
`external_dependencies` against repo B's `package_identity` as the primary
signal for a cross-repo edge — a genuine, verifiable "A imports a package B
publishes" — falling back to the old text-mention heuristic only when that
finds nothing. This replaces the fabricated-then-heuristic cross-repo
detection flagged in the original audit with something actually grounded in
parsed imports.

**Limitation, stated plainly:** this still can't catch relationships with no
shared package (a plain HTTP call to a service, or an internal package whose
import name doesn't match its declared name). That needs contract-file
matching (OpenAPI/protobuf) or config-driven service topology — a real
follow-up feature, not implemented here.

## 2. Multi-repo federation actually works now

Previously `get_org_analytics`/`get_intelligence_metrics` were hardcoded to a
single fake `"ckb-core-platform"` entry — there was no way to register more
than one repo's scan. Now:
- `POST /api/v1/scan` accepts an optional `repo_name` field; when present,
  the scan is also stored in a real multi-repo registry (`AppState.federated_reports`).
- `GET /api/v1/federation/repos` lists everything currently registered.
- `get_org_analytics`/`get_intelligence_metrics` use the real registry,
  falling back to a `"default"` single-entry map (from the last unscoped
  scan) for single-project setups that never pass `repo_name` — so this is
  backward compatible, not a breaking change.

## 3. Session-level blast-radius aggregation

New `CkbEngine::analyze_session_impact(changes: &[SessionChange])` — runs
impact analysis for every change in one call, then merges: deduplicated
affected nodes/files, highest/average risk score, and — cross-referenced
against the real `TestCoverageAnalyzer` — which affected nodes have zero test
coverage. Exposed as:
- `POST /api/v1/session-impact`
- MCP tool `ckb_analyze_session_impact`
- SDK: `client.analyzeSessionImpact(changes)` (Node), `client.analyze_session_impact(changes)` (Python)

Built specifically for reviewing a multi-file AI-agent editing session in one
pass instead of reading N separate impact reports.

## 4. GitHub Action + PR bot

`.github/actions/ckb-scan/` — a composite action that builds `ckb-cli` from
source, runs `ckb-cli check`, and posts (or updates, on re-push — not
spammed) a single PR comment with a severity-sorted violation table. Fails
the workflow when `--strict` is set and violations meet the `--fail-on`
threshold. `.github/workflows/ckb-pr-check.yml` dogfoods it on this repo;
`.github/actions/ckb-scan/README.md` documents adoption in other repos.

**Real bug found and fixed while building this:** `ckb check --strict
--fail-on` were parsed CLI arguments that were **never actually used** —
`ckb check` always exited 0 regardless of violations found, which would have
made this whole Action pointless (a CI gate that can never fail the build).
Fixed in `cli/src/main.rs`'s `check_command` — it now actually compares
violation severities against the threshold and calls `std::process::exit(1)`
when `--strict` is set and something meets or exceeds it.

**Also fixed while building the Action's exit-code capture:** GitHub Actions
bash steps run with `set -e` by default, so the original script would have
silently aborted (never writing the captured exit code) the moment `ckb-cli
check` returned non-zero. Rewritten with an explicit `set +e` / capture /
`set -e` sequence.

## 5. Explain + Fix (Claude API)

New `mcp-server/src/explain.rs` — takes a `DriftViolation` and asks Claude for
a plain-language explanation plus a concrete suggested fix. Requires
`ANTHROPIC_API_KEY`. Exposed as:
- `POST /api/v1/violations/explain`
- MCP tool `ckb_explain_violation`
- SDK: `client.explainViolation(violation)` / `client.explain_violation(violation)`

Model is configurable via `CKB_EXPLAIN_MODEL` (defaults to a fast/cheap
model) rather than hardcoding a dated snapshot that will eventually be
deprecated.

## 6. Usage-based billing foundation

This was the biggest architectural change this session. Previously the
Node backend's `ApiKeyService` existed (create/list/revoke keys) but **there
was no middleware anywhere that actually validated an API key for
authentication** — it was dead infrastructure. Now:

- **Real per-user MCP auth**: the MCP server's `require_api_key` middleware
  has two modes. If `CKB_BACKEND_URL` + `CKB_INTERNAL_SECRET` are set, it
  validates the presented key against the backend's real `ApiKeyService` (via
  a new internal endpoint, see below) instead of a single shared
  `CKB_API_KEY` — this is what makes usage meterable per-user instead of
  per-deployment. Falls back to the original shared-key mode if unset, so
  existing single-tenant deployments aren't broken.
- **New backend internal endpoints** (protected by `INTERNAL_API_SECRET`,
  same pattern as the audit's `grant-access` endpoint):
  - `POST /api/v1/internal/validate-key` — validates a raw key, returns
    `{valid, key_id, user_id, plan, permissions}`.
  - `POST /api/v1/internal/record-usage` — logs one call. Called
    fire-and-forget (via `tokio::spawn`, never blocks the actual request) by
    the MCP server after every successfully authenticated call.
- **New Prisma model** `ApiKeyUsage` — one row per authenticated call
  (apiKeyId, userId, toolName, timestamp), replacing the previous
  single-timestamp `lastUsedAt` field that couldn't answer "how many calls
  this month."
- **New user-facing endpoint** `GET /api/v1/api-keys/usage` — real usage
  summary (total calls, grouped by tool and by key) for the dashboard.

**Real bug found and fixed in this area during the compile-review pass**:
`ApiKeyService.validateKey()`'s early-return branches returned `{ valid: false
}` (no `key` field at all), while the success branch returned `{ valid: true,
key }`. The caller in `server.ts` checks `if (!result.valid || !result.key)
return; ... result.key.userId`, and accessing `.key` on a union where one
member doesn't have that property at all is the kind of thing TypeScript
often rejects even outside strict mode. Fixed by making every branch's shape
uniform — `key: null` on the failure branches — so the property is always
present (just nullable), which is the well-supported narrowing pattern.

## 7. Violation feedback loop (real accuracy tracking)

New Prisma model `ViolationFeedback` and `backend/src/feedback.ts`:
- `POST /api/v1/violations/feedback` — confirm or dismiss a specific
  violation, with an optional note.
- `GET /api/v1/violations/accuracy` — real confirmation-rate /
  false-positive-rate metrics, computed from accumulated feedback. Returns
  `null` (not a fabricated percentage) until there's at least
  `minSampleSize` (default 10) data points — a 100% rate from 2 samples isn't
  a number worth showing anyone.

Violations don't have a stable ID across scans (each scan generates a fresh
UUID), so feedback is correlated via a content fingerprint (sha256 of
kind|from|to|boundary) — see `computeFingerprint` in `feedback.ts`.

This is the honest, real-data-backed replacement for the
"impact_prediction_accuracy_percent"/"false_positive_rate_percent" fields
that were **removed** (not fixed — actually deleted) from
`IntelligenceBenchmarkMetrics` in the original audit because they were
hardcoded constants with zero real data behind them. This feature is what
those fields need to exist for real, and it isn't wired back into the Rust
struct — it's a separate backend-side metric for now, deliberately, since the
core Rust crate doesn't have a database connection.

## 8. Natural-language codebase Q&A (MVP, explicitly scoped)

New `mcp-server/src/ask.rs`. **Read the scope note at the top of that file
before assuming this is more than it is**: this is keyword-overlap retrieval,
not real embeddings-based semantic search. It scores scanned nodes and
violations by literal token overlap with the question, takes the top ~25
nodes / ~10 violations, and asks Claude to answer using only that context.
Good enough for "which service handles X" style questions where vocabulary
roughly matches; will miss real semantic matches that use different words
for the same concept. A real version needs an embeddings pipeline + vector
store, which is a genuine scope item for later, not something this pass
attempted.

Exposed as `POST /api/v1/ask`, MCP tool `ckb_ask`, and `client.ask(question)`
on both SDKs. Requires `ANTHROPIC_API_KEY` and at least one prior scan on the
server.

## Bonus fix: CLI watch mode

`ckb watch` already existed (wasn't a stub) but had two real bugs, both fixed:
1. **It never actually displayed violations.** The code printed "Found N
   violations" and then, only if N > 3, printed "...and N-3 more" — without
   ever printing the first 3. Watch mode showed a count and nothing else.
   Now reuses the same table renderer `ckb check` uses.
2. **A single failed `--exec` hook crashed the entire watch loop** (via an
   unhandled `?` on the command's output), killing what's supposed to be a
   long-running continuous monitor over one bad command. Now logged as a
   warning and the loop continues. Same fix applied to scan failures — a
   transient scan error no longer kills the watch process either.

Also added a running summary ("N new violations since last change" / "N
resolved") between change detections.

## New environment variables (added to `.env.example`)

| Variable | Used by | Purpose |
|---|---|---|
| `ANTHROPIC_API_KEY` | mcp-server | Powers Explain+Fix and Ask |
| `CKB_EXPLAIN_MODEL` | mcp-server | Optional override for which Claude model to use |
| `CKB_BACKEND_URL` | mcp-server | Enables per-user API key mode (usage billing) |
| `CKB_INTERNAL_SECRET` | mcp-server | Must match backend's `INTERNAL_API_SECRET` |
| `INTERNAL_API_SECRET` | backend | Protects `/api/v1/internal/*` routes |

## Required before deploying: new Prisma migration

Two new models (`ApiKeyUsage`, `ViolationFeedback`) plus a new relation field
on `ApiKey` and `User`. Run:
```bash
cd backend
npx prisma generate
npx prisma migrate dev --name add_usage_and_feedback_tracking
```

## 9. Per-tenant session isolation (concurrency correctness fix)

Raised in a follow-up review: the MCP server held **one shared `CkbEngine`
with one shared internal graph** across every request. That's fine for a
single local user, but under real concurrent multi-tenant usage it was a
correctness bug, not just a performance one — user A scanning repo X and
user B scanning repo Y at the same time could overwrite each other's graph
and `latest_report`; A's impact-analysis call could silently run against B's
data.

Fixed with a new `SessionState` (isolated `engine` + `latest_report` pair),
keyed by the `repo_name` you already had to pass for federation. Every
handler that depends on "the graph populated by a prior scan" —
`scan`, `impact`, `search`, `session-impact`, `ask`, `otlp`, `test-gaps`,
`rules`, `report` — now resolves its own isolated session when `repo_name`
(or, for GET endpoints, `?repo=`) is provided, and falls back to the
server's original single shared session when it isn't. **This is fully
backward compatible** — existing single-tenant deployments that never pass
`repo_name` see no behavior change at all.

`detect_clones` and `explain_violation` were left untouched deliberately —
neither depends on the shared graph (clone detection re-reads files from
disk per call; explain-fix takes a violation object directly), so they were
never actually affected by this bug. `get_drift_timeline` was also left
alone — it reads git history from a hardcoded `"."` path unrelated to
per-repo session state, a separate pre-existing design question, not
bundled into this fix.

Both SDKs (Node, Python) were updated to accept an optional `repoName`/
`repo_name` on every affected method.

## 10. Rust test suite (previously: zero tests anywhere in the project)

Added `#[cfg(test)]` modules covering the highest-risk, most-recently-changed
logic:
- **`core/src/analysis/test_coverage.rs`** — direct regression tests for the
  `is_test_path` fix from the original audit (`latest.rs`/`contest.py`/
  `attestation.go` must NOT be misclassified as test files; real test files
  in various conventions must be recognized).
- **`core/src/federation/mod.rs`** — tests for the real package-identity
  cross-repo matching from this session: a genuine import→package match
  produces exactly one edge; unrelated repos produce zero edges (the direct
  regression test for the audit's "fabricated edge for every repo pair"
  bug); a repo can't create a self-edge; benchmark metrics reflect real scan
  data, not fabricated constants.
- **`core/src/parser/rust.rs`** — parses real Rust snippets and checks the
  actual extracted symbols/imports; tests empty and syntactically-broken
  input don't panic; a 50-iteration repeated-parse test on one shared parser
  instance, exercising the same lock-acquisition path repeatedly (doesn't
  literally simulate the poison-recovery scenario, which is awkward to do
  safely in a unit test, but does confirm the lock cycles correctly under
  repeated use).
- **`core/src/lib.rs`** — tests for `detect_package_identity` (against real
  temp-directory `package.json`/`Cargo.toml`/`go.mod` files) and
  `collect_external_dependencies` (deduplication, relative-import exclusion,
  scoped/deep-path normalization) — the two new functions this session's
  cross-repo detection is built on.

This is not comprehensive coverage — `core/graph`, `core/analysis/drift.rs`,
`core/analysis/boundaries.rs`, `core/analysis/clone_detector.rs`, the other
four language parsers, the CLI, and the entire TypeScript backend still have
zero automated tests. This is a start on the highest-risk code, not a
finished test suite. Run `cargo test --workspace` once the project builds to
see these actually execute (still unverified in this sandbox — no `cargo`
available here either).

## New environment variable / API surface from this session

- REST: `repo_name` (POST body) / `?repo=` (GET query) now accepted on
  `/api/v1/scan`, `/api/v1/report`, `/api/v1/impact`, `/api/v1/search`,
  `/api/v1/session-impact`, `/api/v1/ask`, `/api/v1/otlp`,
  `/api/v1/test-gaps`, `/api/v1/rules`.
- SDKs: `repoName` (Node) / `repo_name` (Python) parameter added to the
  corresponding client methods.

## 11. Marketplace-readiness pass — real bugs found in code I hadn't read yet

Prompted by "are we ready to launch on the IDE marketplace" — reading the
extensions and CI workflows closely for the first time surfaced real,
previously-unknown bugs, not just polish items.

### VS Code extension (`integrations/vscode/`)
- **No API key sent, ever.** Any CKB server with auth enabled (the
  recommended config after the security audit) would silently 401 every
  request from this extension.
- **Broken remote fallback.** Defaulted to a hardcoded external domain
  (`https://ckb-mcp-server.onrender.com`) and used it as a silent fallback
  that sent the user's *local* workspace path over HTTP — a remote server
  can't read a path that only exists on the user's machine. This was the
  extension's primary code path for anyone without the CLI installed, i.e.
  almost every first-time marketplace user.
- **A regression from this project's own earlier CLI fix**: `ckb check
  --strict` now correctly exits non-zero when it finds violations (fixed in
  the audit's GitHub Action work) — but Node's `child_process.exec` treats
  any non-zero exit as an error, and this extension's naive try/catch
  treated "found real violations" identically to "CLI not installed,"
  always taking the broken remote-fallback path whenever there was anything
  to actually report. Fixed with a helper (`runCliJson`) that recovers valid
  JSON from `error.stdout` on a non-zero exit instead of discarding it.
- **Violations on non-file nodes were silently dropped.** Path extraction
  only handled the `"::file"` suffix (file-level nodes); function/class/
  method-level violations have a different suffix (the symbol name) and
  were left as an unparseable "path::symbolName" string, producing an
  invalid `Uri` that silently failed to show a diagnostic.
- **The documented "re-checks on file change (debounced)" feature was a
  no-op** — it spun the status bar icon for two seconds and did nothing
  else. Now performs a real debounced re-scan, with a dedicated
  `ckb.rescanOnSave` setting (previously would have needed to overload
  `autoScanOnOpen`, which controls a different, unrelated behavior).
- **`checkArchitecture` reported "✅ Architecture compliant" on literally
  any error** — including ones that meant the check never actually ran, so
  a broken environment looked identical to a clean one.
- **The shipped README instructed `curl -fsSL https://ckb.dev/install.sh |
  sh`** — a domain with no evidence it exists or was ever set up. This would
  have been the very first thing a new user tried. Rewritten with real
  instructions (GitHub Releases download or `cargo build`/`cargo install`)
  and reconciled against the actual default config (the README and the
  shipped `package.json` default had also drifted out of sync with each
  other).
- **Packaging hygiene**: added `.vscodeignore` (a stale committed `.vsix`
  file had no ignore rules protecting against being bundled into itself),
  added a `LICENSE` file inside the extension's own directory (the root
  one isn't found by `vsce package` for a monorepo-nested extension), added
  `CHANGELOG.md`, removed the stale `.vsix`, bumped `*.vsix`/`*.jar` into
  root `.gitignore`, version bumped to 1.1.0.

### JetBrains plugin (`integrations/jetbrains/`)
- **Same missing-API-key bug** as VS Code — fixed with a `ckb.apiKey`
  setting and an OkHttp request-builder extension that adds the header.
- **A worse, crash-causing bug**: `DriftViolation.from`/`to` were typed as
  `Map<String, String>`, but the server actually serializes them as plain
  strings (Rust's `NodeId(String)` is a serde newtype — it serializes
  transparently as a JSON string, not an object). Gson would throw a
  `JsonSyntaxException` trying to deserialize a JSON string into a Map,
  which means `getReport()` would have crashed on **any** response
  containing a real violation — not degraded gracefully, crashed. Fixed by
  correcting the field types to `String`.

### Three pre-existing CI/release workflows, never reviewed until now
- **`.github/workflows/publish-crates.yml`** (real, pre-existing) — would
  have failed to publish `ckb-cli` or `ckb-mcp-server` at all:
  `ckb-core = { path = "../core" }` had no `version` field, and crates.io
  refuses to publish any crate with an un-versioned path dependency. Fixed
  by adding `version = "0.1.0"` alongside the path in both `cli/Cargo.toml`
  and `mcp-server/Cargo.toml`. Also: the workflow never published `ckb-cli`
  at all (only `ckb-core` and `ckb-mcp-server`), which would have made the
  README's `cargo install ckb-cli` instruction false. Added the missing
  publish step.
- **`.github/workflows/release.yml`** (real, pre-existing) — used
  `cargo build ... -p cli`, but `-p` takes the Cargo.toml `[package].name`
  value, which is `ckb-cli`, not the directory name `cli`. This would have
  failed **every single matrix build** immediately with "package `cli` not
  found." Also added the missing linker configuration for the
  `aarch64-unknown-linux-gnu` cross-compile target (installing the
  cross-gcc alone isn't enough; cargo needs to be told to use it), and an
  explicit `permissions: contents: write` block (release creation can
  silently 403 without it, depending on the repo's default token
  settings).
- **`.github/workflows/build.yml` and `docs.yml`** (pre-existing) — both
  were empty stub files containing only a comment (e.g. `# build.yml`),
  not valid GitHub Actions workflows at all. This means **there was no CI
  build/test verification anywhere in this project** — every change across
  every prior session was written and merged without any automated
  compile check, which is exactly the gap this whole multi-session audit
  has been working around manually. Built a real `build.yml` covering the
  Rust workspace (build + test + clippy), the WASM binding, both Node
  backends (backend/, web/), the VS Code extension compile step, and SDK
  syntax checks. Removed `docs.yml` (an invalid empty workflow file shows
  as a permanently broken/red entry in the repo's Actions tab — worse than
  not having it; a real docs pipeline is a fine follow-up, not urgent).

## 12. Finishing pass — everything else doable without a compiler

### More real bugs found while writing tests (not looking for bugs, writing tests surfaced them anyway)
- **TypeScript parser**: every top-level `class`/`function` declaration was
  marked `exported: true, public: true` **unconditionally** — including
  ones with no `export` keyword at all. Only declarations reached via the
  `export_statement` tree-sitter node were supposed to get that flag; the
  bare-declaration branch copy-pasted the same `true`/`true` instead of
  `false`/`false`. This meant nothing downstream (boundary inference,
  future API-surface analysis) could actually tell a module's real public
  surface from its private internals — every symbol looked exported. Fixed,
  with a direct regression test (`non_exported_function_is_not_marked_exported`).

### JetBrains plugin — three more real bugs, one severe
- **`plugin.xml` registered `externalAnnotator` extension points for
  Java/Kotlin/Python/TypeScript pointing at `dev.ckb.annotator.CkbAnnotator`
  — a class that doesn't exist anywhere in the plugin's source.** This is
  the most severe finding in this whole marketplace-readiness pass: it
  would throw `ClassNotFoundException` at plugin load time, which typically
  disables the *entire plugin*, not just the missing feature. Removed the
  dangling registration rather than write a real `ExternalAnnotator`
  implementation blind — that API's threading model (PSI access is
  forbidden off the EDT) is easy to get subtly wrong, and with no IntelliJ
  Platform SDK or compiler available here to verify against, a wrong
  implementation could easily be worse than an honest gap. The tool window
  still shows violations; inline annotations for JetBrains IDEs remain a
  real, undone follow-up.
- **`ImpactAnalysis`'s Kotlin fields didn't match the server's actual JSON**
  (`directly_affected`/`transitively_affected: List<String>` vs. the real
  `direct_impacts`/`indirect_impacts: List<ImpactedNode>`). Gson doesn't
  error on unmatched field names — it silently leaves the Kotlin defaults
  in place, so "Analyze Impact" would have always shown empty
  directly/transitively-affected lists, regardless of the real impact.
  Fixed the field names, added a proper `ImpactedNode` data class, and
  updated the display code that had been treating each item as a raw
  string.
- **`ScanProjectAction` set an `error` message when the server wasn't
  reachable, but never displayed it** — `onSuccess()` just returned
  silently on a null report, so a user with no server running saw
  absolutely nothing happen, not even an error.
- **Violation list sorted by severity as a raw string comparison**
  (`sortedByDescending { it.severity }`) — alphabetically, "Warning" sorts
  above "Critical" (W > C), giving a close-to-inverted "most severe first"
  order. Fixed with an explicit severity rank.
- Added `ckb.apiKey` setting + wired into every request (same gap as VS
  Code). Added `LICENSE`, version bumped to 1.0.1.

### Rust test coverage extended
Added `#[cfg(test)]` modules to the four language parsers that had zero
coverage: `python.rs`, `java.rs`, `go.rs`, `typescript.rs` — extraction of
real functions/classes/exports, empty-input and malformed-syntax handling
(tree-sitter's error tolerance means these should never panic), and for
Java specifically, public-vs-package-private detection. Combined with the
existing `rust.rs`/`test_coverage.rs`/`federation/mod.rs` tests from
earlier, 5 of 5 language parsers now have real tests; `core/graph`,
`drift.rs`, `boundaries.rs`, `clone_detector.rs`, `otlp.rs`, `git_drift.rs`
still don't.

### `render.yaml` corrected and split
The single existing `render.yaml` defined both the public `ckb-mcp-server`
and private `ckb-backend-api` services together, and had real gaps in both:
`ckb-mcp-server`'s env vars never mentioned `CKB_API_KEY`,
`CKB_BACKEND_URL`, `CKB_INTERNAL_SECRET`, or `ANTHROPIC_API_KEY` (all
required for features built in earlier sessions), had a stray unused
`DATABASE_URL`, and was missing `CKB_BIND_ALL=1` — without it the service
binds `127.0.0.1` by default (the audit's safe-by-default fix) and Render's
proxy literally can't reach it, so a deploy would look fine and then fail
health checks. `ckb-backend-api`'s block never included
`INTERNAL_API_SECRET` at all, despite the Coinbase webhook and MCP-server
per-user auth depending on it. Both fixed; split into `render.yaml`
(public, stays here) and `render-backend-private-repo.yaml` (delivered
alongside this document — move into the private repo once it exists).

### Confirmed, not just claimed: TypeScript compiles clean modulo missing `npm install`
Ran `tsc --noEmit` against the VS Code extension in this sandbox (a real
`tsc` binary happens to be available, even without network for `npm
install`). Every reported error was a missing-declaration error
(`@types/vscode`/`@types/node` aren't installed here) — filtering those out
left **zero** genuine structural/type errors. This isn't a substitute for a
real `npm install && tsc --noEmit` run, but it's a stronger signal than
manual tracing alone, and it's the first time in this whole effort that an
actual type-checker (not just my own reasoning) touched any of this code.

## Not built (from the original roadmap, still outstanding)

- **VS Code/JetBrains live diagnostics UI** — the CLI watch mode and REST
  endpoints this session built are the right foundation for it, but the
  actual editor-side diagnostics rendering wasn't touched.
- **Real embeddings-based Q&A** — see the scope note on feature 8 above.
- **Contract-file-based cross-repo detection** (OpenAPI/protobuf matching) —
  the deeper version of feature 1.
