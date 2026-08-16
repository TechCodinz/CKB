# CKB Public V13.2 Consolidation — Claude Code Brief

## Repository boundary — READ THIS FIRST

CKB is intentionally split across two repositories. This public repository is **not** the Cloud/commercial application repository.

### This repository: PUBLIC / OPEN SOURCE

- Repository: `TechCodinz/CKB`
- Visibility: **PUBLIC / OPEN SOURCE**
- Branch: `agent/v13-2-complete-consolidation`
- Expected sibling private repository: `../ckb-cloud`

This public repository owns reusable architecture-intelligence technology:

- Rust core / Reality / causality engines
- CLI
- public MCP/Reality server
- Universal Model Gateway and provider-neutral tool adapters
- protocol/tool schemas
- SDKs/bindings/runtime agents
- VS Code / JetBrains / public IDE integrations
- Open VSX/public distribution assets
- public integration docs/examples/smoke tests

**Do not copy into this public repository any private `ckb-cloud` implementation, private billing/auth logic, customer/account data models beyond public protocol requirements, private admin/business workflows, commercial secrets, database credentials, provider credentials, deployment secrets, or other closed-source infrastructure.**

### Sibling repository: PRIVATE / CLOSED SOURCE

- Repository: `TechCodinz/ckb-cloud`
- Visibility: **PRIVATE / CLOSED SOURCE**
- Branch: `agent/v13-2-complete-consolidation`

The private repo owns Cloud/backend/web/commercial infrastructure, including users/accounts, OAuth authorization persistence, Neon/Prisma Cloud data, entitlements, plans, Flutterwave, private operations/admin behavior and the Vite product UI.

Do not duplicate private Cloud implementation here merely to make a public MCP feature easier. Integrate through explicit HTTP/MCP/schema contracts.

Before editing, verify both repositories and branches:

```bash
# public/open repo (this repo)
pwd
git remote -v
git branch --show-current
# expected: TechCodinz/CKB
# expected branch: agent/v13-2-complete-consolidation

# private/closed sibling
cd ../ckb-cloud
git remote -v
git branch --show-current
# expected: TechCodinz/ckb-cloud
# expected branch: agent/v13-2-complete-consolidation
```

If the private checkout is elsewhere, locate `TechCodinz/ckb-cloud`; do not recreate its logic inside this repository.

Cross-repo features are complete only when both halves agree on the same contracts. In particular, Cloud OAuth and this public MCP resource server must match on issuer, resource, audience, scopes, introspection behavior and error semantics.

---

This branch was created fresh from current `main`. Do not merge historical branches wholesale.

Primary historical source: PR #7 / `agent/chatgpt-mcp` — Universal Model Gateway + production remote MCP.

Forward-port only capabilities still missing from current main while preserving all newer Reality, Render, JetBrains, Open VSX, security and deployment fixes.

Required capabilities to inspect/port:

- stateless Streamable HTTP MCP endpoint `/mcp`
- one canonical scoped CKB Reality tool registry
- `GET /llm/capabilities`
- `GET /llm/tools?provider=<provider>`
- `POST /llm/call`
- provider-shaped tool schemas generated from the canonical registry
- adapter calls routed back through canonical MCP authorization/handler
- RFC 9728 protected-resource metadata
- OAuth scope metadata: `architecture:read`, `repository:scan`, `offline_access`
- Cloud token introspection integration
- per-user Reality project isolation
- structured MCP tool outputs
- smoke suites for canonical MCP + Universal Model Gateway
- integration packaging that remains valid on current main

Security invariants:

- local filesystem scan stays disabled in production
- exposed target-repository operations stay read-only
- provider adapters never implement a second architecture engine
- `/llm/call` cannot bypass OAuth/tool scopes
- `CKB_INTERNAL_SECRET` remains the server-to-server trust boundary
- no user/model credential is exposed to the browser
- current Render startup/port/liveness fixes remain intact
- current shared-VPS PR #13 stays separate; do not fold it into this consolidation
- no private Cloud/commercial implementation is copied into this public repository
- public engine/protocol logic should remain here rather than being duplicated into `ckb-cloud`

Coordinate with the Cloud consolidation branch `TechCodinz/ckb-cloud:agent/v13-2-complete-consolidation`, whose OAuth server must agree on issuer/resource/audience/scopes/introspection behavior.

Mandatory validation:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

Also build the actual release binaries used by the current Render service and adapt/run the PR #7 MCP/gateway smoke tests.

Before declaring completion, verify the public/private boundary:

- no private `.env` or credential material is tracked
- no Cloud billing/payment implementation has been copied here
- no private admin/customer/business workflow has been copied here
- the Cloud OAuth/API-key half is integrated only through explicit documented contracts
- every cross-repo feature is identified in the Cloud `V13_2_CONSOLIDATION_REPORT.md` with a public half and private half

Push only to this branch. Open a PR to `main` after validation. Do not merge automatically.
