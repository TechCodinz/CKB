# CKB — Build & Verification Instructions

This project went through two sessions with an AI assistant (Claude), both
**without a compiler, network access, or test runner**: first a
security/reliability audit (`AUDIT_REPORT.md`), then a feature-building
session that added eight new features (`FEATURES_ADDED.md`). Every change in
both sessions was written by careful static reading and manual type-tracing,
never verified by an actual build. This document is the exact, ordered
checklist to close that gap.

Read `AUDIT_REPORT.md` and `FEATURES_ADDED.md` first for *what* changed and
*why*. This file is about *verifying it actually works*.

## 0. Prerequisites

- Rust toolchain (stable, 1.75+) with `cargo`
- Node.js 18+ and `npm`
- (Optional, only for the WASM binding) `wasm-pack` and the
  `wasm32-unknown-unknown` target: `rustup target add wasm32-unknown-unknown`
- A Postgres database (or Supabase project) for the backend's Prisma schema
- An Anthropic API key if you want Explain+Fix / Ask working
  (`ANTHROPIC_API_KEY`)
- Network access to crates.io / npm registry (this sandbox had neither —
  that's why none of this was ever compiled)

## 1. Rust workspace — highest priority, most likely to need a fix

```bash
cargo build --workspace 2>&1 | tee build.log
```

This is the single most important command to run before anything else. Several
edits touched shared types used across crates:

- `core::ScanReport` gained a new field, `duration_ms: f64` — if `cargo build`
  reports a missing-field error anywhere, it means there's a `ScanReport { ... }`
  construction site this audit didn't find. Search: `grep -rn "ScanReport {" --include="*.rs" .`
- `core::federation::IntelligenceBenchmarkMetrics` was restructured (fields
  removed, `Default` impl removed, replaced with `from_reports(reports, elapsed)`).
  If anything still calls `.default()` on it or references the removed fields
  (`repository_indexing_speed_files_per_sec`, `query_latency_ms`,
  `impact_prediction_accuracy_percent`, `false_positive_rate_percent`,
  `blast_radius_precision_percent`, `memory_usage_mb`), that's a compile error
  to fix by updating the caller to the new field names, or by re-adding a field
  if you decide you actually want it (see "Known gaps" in `AUDIT_REPORT.md`
  before doing that — don't just hardcode a number back in).
- `core::ScanReport` also gained `package_identity: Option<String>` and
  `external_dependencies: Vec<String>` in the feature session (see
  `FEATURES_ADDED.md` #1) — same "check every construction site" advice
  applies if `cargo build` reports a missing-field error.
- `CkbEngine::detect_semantic_clones` kept its old signature; a new
  `detect_semantic_clones_at(path: &str)` was added alongside it. Both should
  coexist fine, but double check nothing else called the old one expecting it
  to do file discovery.
- New in the feature session: `AppState` in `mcp-server/src/main.rs` gained
  `federated_reports`, `backend`, and `http_client` fields, and
  `require_api_key` was rewritten with a second auth mode. If you're
  resuming mid-build, this is the single riskiest file to check first.

**If `cargo build --workspace` succeeds:** run `cargo clippy --workspace` too —
several fixes (mutex-poison recovery, `partial_cmp` NaN handling) are exactly
the kind of thing clippy has opinions about, and it's a good second pass.

**If it fails:** the error message + file:line from `cargo` is authoritative.
Fix forward — these are mechanical fixes (type mismatches, missing imports),
not architectural problems, based on how contained the individual edits were.

### 1a. WASM binding (optional, only if you plan to use it)

```bash
cd bindings/wasm
wasm-pack build --target web
```

This is the **riskiest unverified code in the whole audit** — `bindings/wasm/src/lib.rs`
was rewritten to do a real `fetch()` call using `web-sys`/`wasm-bindgen-futures`
(previously it was a hardcoded fake success string that never actually called
the server). The general shape of the code follows the standard wasm-bindgen
"fetch from Rust" pattern, but `web-sys`'s exact API for `RequestInit` setters
(`set_method` vs `method`, consuming vs `&self`) has changed across versions,
and this was written from memory of the current API shape, not verified against
a specific pinned version. If this doesn't compile, the fix is almost certainly
in the setter calls (`opts.set_method(...)`, `opts.set_mode(...)`,
`opts.set_headers(...)`, `opts.set_body(...)`) — check whatever `web-sys`
version Cargo resolves to (`cargo tree -p web-sys`) against its docs on
docs.rs for the `RequestInit` struct.

## 2. Backend (Node/TypeScript)

```bash
cd backend
npm install
npx tsc --noEmit          # type-check without emitting, fastest signal
npm run build              # actual build, if the above passes
```

Set up `.env` first (copy `.env.example`, fill in at minimum `JWT_SECRET` and
`DATABASE_URL` — the server now refuses to start in production without a real
`JWT_SECRET`, by design, see `AUDIT_REPORT.md`).

Then run the Prisma migration/generate step if you haven't:
```bash
npx prisma generate
npx prisma migrate dev   # or `migrate deploy` in production
```

**If resuming after the feature session**, the schema also gained two new
models (`ApiKeyUsage`, `ViolationFeedback`) and two new relation fields
(`ApiKey.usage`, `User.violationFeedback`) — make sure your migration picks
those up (`npx prisma migrate dev --name add_usage_and_feedback_tracking` if
you haven't already migrated since then).

**Specific things to smoke-test, not just type-check** (these had the
highest-stakes fixes in the whole audit):
- Register a user, log in with the **wrong password**, confirm you get a real
  401 error message — not a silent successful login. (This was broken before;
  it's the fix I'm least able to verify without running it.)
- Run the app in Stripe test mode: create a checkout session, complete it with
  Stripe's test card, confirm the webhook fires and `User.plan` actually
  updates in the database. Then cancel the subscription and confirm `plan`
  reverts to `'free'`.
- Same for Flutterwave test mode if you use it: confirm `User.plan` updates on
  a successful test charge.
- If you use Coinbase Commerce: trigger a test webhook event from Coinbase's
  dashboard, confirm the signature check passes (it will reject anything
  without a valid `X-CC-Webhook-Signature` computed from
  `COINBASE_COMMERCE_WEBHOOK_SECRET`) and that
  `POST /api/v1/internal/grant-access` gets called with the right
  `INTERNAL_API_SECRET`.

## 3. Frontend (React dashboard)

```bash
cd web
npm install
npx tsc --noEmit
npm run build
```

Smoke-test manually:
- Sign up with a new account, log in, log out, log back in.
- Try logging in with a wrong password — you should see an actual error
  message on the page, not get silently taken to the dashboard.
- The "Launch Instant Demo" button should still work (that's intentional, not
  a bug) — it's specifically *wrong credentials on the real form* that should
  now fail visibly.

## 4. MCP server

```bash
cargo run --bin ckb-mcp-server
```

By default it now binds to `127.0.0.1:3000` (changed from `0.0.0.0` — see
`AUDIT_REPORT.md`) and runs without auth if `CKB_API_KEY` isn't set (with a
warning logged). To actually test the auth:

```bash
CKB_API_KEY=test123 cargo run --bin ckb-mcp-server &
curl -i http://127.0.0.1:3000/api/v1/report                       # expect 401
curl -i -H "X-API-Key: test123" http://127.0.0.1:3000/api/v1/report  # expect 200 or 404-no-scan-yet
```

Also smoke-test the newly-wired clone detection route:
```bash
curl -X POST http://127.0.0.1:3000/api/v1/clones \
  -H "Content-Type: application/json" -H "X-API-Key: test123" \
  -d '{"path": "."}'
```

## 4a. New features from the feature-building session

Once the server is running with `ANTHROPIC_API_KEY` set:
```bash
# Explain + Fix (needs a real violation object — grab one from a scan first)
curl -X POST http://127.0.0.1:3000/api/v1/violations/explain \
  -H "Content-Type: application/json" -H "X-API-Key: test123" \
  -d '{"violation": <paste a violation object from /api/v1/report>}'

# Ask (needs a prior scan on the server)
curl -X POST http://127.0.0.1:3000/api/v1/ask \
  -H "Content-Type: application/json" -H "X-API-Key: test123" \
  -d '{"question": "what does this codebase do"}'

# Session impact
curl -X POST http://127.0.0.1:3000/api/v1/session-impact \
  -H "Content-Type: application/json" -H "X-API-Key: test123" \
  -d '{"changes": [{"file": "src/lib.rs", "line": 1, "change_type": "modify"}]}'
```

For the **usage-based billing / per-user auth mode**, set `CKB_BACKEND_URL`
and `CKB_INTERNAL_SECRET` (matching the backend's `INTERNAL_API_SECRET`) on
the MCP server, create a real API key via the backend
(`POST /api/v1/api-keys`, JWT-authenticated), and use *that* key's raw value
(returned once at creation) as `X-API-Key` against the MCP server instead of
`CKB_API_KEY`. Then check `GET /api/v1/api-keys/usage` on the backend to
confirm the call got recorded.

For the **feedback loop**: `POST /api/v1/violations/feedback` (JWT-auth, on
the backend) with a violation + status, then check
`GET /api/v1/violations/accuracy` — expect `hasEnoughData: false` and null
rates until you've submitted at least 10.

For the **GitHub Action**, open a test PR against this repo (or a fork with
the workflow) and confirm a comment appears/updates — see
`.github/actions/ckb-scan/README.md`.

## 5. Node/Python SDKs

These are new, plain HTTP clients with no build step — they were syntax-checked
(`node --check`, `python3 -m py_compile`) but never run against a live server.

```bash
# Node — from bindings/node/
node -e "
const { CkbClient } = require('./index.js');
const c = new CkbClient({ baseUrl: 'http://127.0.0.1:3000', apiKey: 'test123' });
c.health().then(console.log).catch(console.error);
"

# Python — from bindings/python/
python3 -c "
from ckb import CkbClient
c = CkbClient(base_url='http://127.0.0.1:3000', api_key='test123')
print(c.health())
"
```

## 6. Things intentionally left as documented gaps, not bugs to "fix"

Don't spend time trying to patch these — they're architecture-level decisions
noted in `AUDIT_REPORT.md`, and a real fix means designing a feature, not
finding a typo:

- **Cross-repo dependency detection** — UPDATE: this was substantially
  improved in the feature session (real package-identity matching, see
  `FEATURES_ADDED.md` #1), but still can't catch relationships with no shared
  package (plain HTTP calls, internal packages with mismatched names). That
  deeper version still needs contract-file matching or service topology
  config — a real follow-up, not done here.
- **"Impact prediction accuracy" / "false positive rate" metrics** — UPDATE:
  the real version of this was built in the feature session (the violation
  feedback loop, `FEATURES_ADDED.md` #7) — `GET /api/v1/violations/accuracy`
  on the backend now returns real numbers once there's enough accumulated
  feedback. It's on the backend (Node/Prisma) rather than wired back into the
  Rust `IntelligenceBenchmarkMetrics` struct, deliberately, since the core
  Rust crate has no database connection.
- **OTLP span-to-node correlation** (`core/src/telemetry/otlp.rs`) assumes
  incoming span names exactly match CKB's internal `"path::function"` node ID
  format, which most real APM setups won't produce naturally. Fixing this for
  real means either the instrumented app adopting that naming convention, or
  CKB adding a fuzzy-matching/mapping layer. Not addressed in either session.
- **Real embeddings-based Q&A** — the feature session added a keyword-overlap
  MVP (`FEATURES_ADDED.md` #8), explicitly not real semantic search. A real
  version needs an embeddings pipeline + vector store.

## 7. Suggested order if you're an AI agent resuming this task

1. `cargo build --workspace`, fix any errors, repeat until clean.
2. `cargo clippy --workspace --all-targets`, address anything that looks like
   a real bug (not just style).
3. `backend`: `npx tsc --noEmit`, fix errors, then get Prisma set up and run
   the payment-flow smoke tests in section 2 — these are the highest-stakes
   fixes in the whole audit and deserve real verification, not just a
   type-check passing.
4. `web`: `npx tsc --noEmit`, fix errors, manually click through auth flows.
5. Only then bother with the WASM binding (section 1a) — it's the most
   speculative piece of code here and lowest-priority to get working.
6. Re-read the "Not yet covered" section of `AUDIT_REPORT.md` before adding new
   features — some things that look like small bugs (e.g. "cross-repo edges
   seem sparse") are actually the intended, honest behavior after this audit,
   not something to "fix" back to how it was.

## 8. One verification gap I couldn't fully close

A crude automated brace-balance check flagged `cli/src/main.rs` as off by one
`{`/`}` pair. I manually walked through both functions edited in this repo
across both sessions (`check_command`, `watch_command`) line-by-line and both
are correctly balanced — the discrepancy is most likely the checking
heuristic mishandling one of the file's many emoji/unicode-containing
`println!` format strings elsewhere in this large, otherwise-untouched file,
not a real syntax error. But I want to be honest that I couldn't fully
resolve the ambiguity without an actual compiler. If `cargo build` fails on
this file, this is the first place to suspect — though given how targeted
and verified the actual edits were, I'd bet on it being a false alarm.

