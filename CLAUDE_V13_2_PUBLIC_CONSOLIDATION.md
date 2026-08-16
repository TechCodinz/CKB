# CKB Public V13.2 Consolidation — Claude Code Brief

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

Coordinate with the Cloud consolidation branch `TechCodinz/ckb-cloud:agent/v13-2-complete-consolidation`, whose OAuth server must agree on issuer/resource/audience/scopes/introspection behavior.

Mandatory validation:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

Also build the actual release binaries used by the current Render service and adapt/run the PR #7 MCP/gateway smoke tests.

Push only to this branch. Open a PR to `main` after validation. Do not merge automatically.
