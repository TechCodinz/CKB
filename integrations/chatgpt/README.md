# CKB for ChatGPT and Codex (Remote MCP)

CKB exposes a production remote Model Context Protocol endpoint at:

```text
https://ckb-mcp-server.onrender.com/mcp
```

The same MCP server can back a private/custom ChatGPT app during development and, after OpenAI review, a CKB plugin distributed through the Plugins Directory.

## What ChatGPT can ask CKB

The remote MCP surface is architecture-focused and repository-safe. It can:

- scan a public GitHub repository into the caller's isolated CKB project namespace
- return the persisted architecture graph
- calculate file/line blast radius
- inspect observed OpenTelemetry runtime evidence
- inspect Git-backed architecture history/drift
- identify graph-aware test gaps
- trace causal architecture paths
- trace downstream failure cones
- query Architecture Memory
- return Code DNA
- list and diff architecture snapshots
- generate AI architecture guardrails without writing them into the repository

The ChatGPT MCP tools do **not** modify the target GitHub repository.

## Authentication model

ChatGPT users authenticate with **OAuth 2.1 authorization code + PKCE** through CKB Cloud. Do not give ChatGPT a shared `CKB_API_KEY`.

CKB uses these scopes:

- `architecture:read` — architecture graph, impact, runtime, history, test gaps, causal paths, failure cones, Architecture Memory, Code DNA, snapshots and generated guardrails
- `repository:scan` — fetch and analyze a public GitHub repository
- `offline_access` — permits refresh-token issuance so an approved client can remain connected without storing a CKB password

The public MCP resource server publishes:

```text
https://ckb-mcp-server.onrender.com/.well-known/oauth-protected-resource
```

The CKB Cloud authorization server publishes:

```text
https://ckb-backend-api.onrender.com/.well-known/oauth-authorization-server
```

ChatGPT/Codex discovers the authorization server from protected-resource metadata, dynamically registers a public PKCE client when supported, sends the MCP `resource` through the OAuth flow, and receives a short-lived scoped access token.

The Rust MCP service does not receive CKB's OAuth signing secret. It validates access tokens through CKB Cloud's protected `/oauth/introspect` endpoint over the existing `CKB_INTERNAL_SECRET` / `INTERNAL_API_SECRET` trust boundary.

## User isolation

OAuth callers never address the raw Reality project namespace directly. A logical project such as:

```text
TechCodinz-CKB
```

is internally scoped to the authenticated CKB user before it reaches Reality. This prevents two ChatGPT users who choose the same project name from sharing graph, snapshot or architecture-memory state.

Trusted internal/operator credentials retain the existing raw-project behavior for infrastructure and controlled diagnostics.

## Production deployment order

The integration spans two repositories and should be deployed in this order:

1. Deploy `TechCodinz/ckb-cloud` with the MCP OAuth authorization server and database migration.
2. Verify CKB Cloud health and OAuth discovery.
3. Deploy `TechCodinz/CKB` with `chatgpt_edge` as the public process.
4. Verify protected-resource discovery, MCP initialization and tool discovery.
5. Complete an OAuth authorization-code + PKCE test and invoke a read tool.
6. Scan a public GitHub repository and verify the resulting project can be read only by the same OAuth identity.
7. Connect the MCP server in an eligible ChatGPT/Codex development surface.
8. Package and submit the CKB app/plugin for OpenAI review when the integration is ready for public distribution.

Do not reverse steps 1 and 3: the Rust MCP resource server depends on CKB Cloud token introspection for OAuth callers.

## Required production configuration

### `TechCodinz/ckb-cloud`

```text
BACKEND_URL=https://ckb-backend-api.onrender.com
CKB_MCP_RESOURCE=https://ckb-mcp-server.onrender.com
INTERNAL_API_SECRET=<shared-random-secret>
JWT_SECRET=<existing-strong-secret>
MCP_OAUTH_JWT_SECRET=<recommended-dedicated-strong-secret>
DATABASE_URL=<production-postgres-url>
```

`MCP_OAUTH_JWT_SECRET` is recommended so MCP access-token signing can be rotated independently of the existing dashboard JWT secret.

### `TechCodinz/CKB`

```text
CKB_BACKEND_URL=https://ckb-backend-api.onrender.com
CKB_MCP_RESOURCE=https://ckb-mcp-server.onrender.com
CKB_INTERNAL_SECRET=<same value as Cloud INTERNAL_API_SECRET>
CKB_API_KEY=<production gateway/operator key>
CKB_ALLOW_LOCAL_SCAN=0
```

`CKB_API_KEY` remains a production gateway/operator credential for the current Reality gateway path. It is **not** an end-user ChatGPT authentication mechanism and must never be exposed in prompts, plugin metadata or client-side code.

## MCP protocol behavior

The endpoint implements stateless MCP Streamable HTTP over `POST /mcp`.

Public before account linking:

- `initialize`
- `ping`
- `tools/list`
- `/.well-known/oauth-protected-resource`

Authenticated per tool call:

- every CKB analysis tool declares an OAuth `securitySchemes` entry
- `ckb_scan_repository` requires `repository:scan`
- the other architecture-intelligence tools require `architecture:read`
- when authorization is absent/insufficient, the MCP result includes `_meta["mcp/www_authenticate"]` pointing at the protected-resource metadata URL

Tool results include readable MCP text plus `structuredContent`, preserving CKB's native JSON evidence for model reasoning and future interactive UI.

`GET /mcp` currently returns `405 Method Not Allowed`; CKB's current tool set is request/response and does not require a server-to-client SSE stream.

## Security model

- OAuth authorization code flow requires PKCE `S256`.
- Authorization codes are random, hashed at rest, expire after 10 minutes and are atomically single-use.
- Refresh tokens are opaque, hashed at rest, expire and rotate on each use.
- Access tokens are short-lived and bound to the CKB MCP resource/audience and granted scopes.
- The resource server validates the token for every protected tool call.
- OAuth project state is isolated per CKB user.
- `/health`, MCP initialization/discovery and OAuth metadata remain public; analysis execution is authenticated.
- Public GitHub repository scanning remains inside the existing Reality scan gate and concurrency controls.
- Local filesystem scanning stays disabled in production.
- Tool annotations mark the exposed operations as repository-read-only and non-destructive.

## Deployment topology

```text
ChatGPT / Codex
      |
      | OAuth 2.1 + PKCE
      v
CKB Cloud OAuth edge
      |
      | scoped access token
      v
chatgpt_edge : /mcp + OAuth protected-resource metadata
      |
      | token introspection + per-user project scoping
      v
reality_gateway : auth + scan gate + trace persistence
      |
      v
reality_server_v5 : architecture intelligence engine
      |
      v
CKB Core : graph / AST / impact / memory / telemetry / history
```

## Example conversation

```text
Use CKB to scan https://github.com/TechCodinz/CKB as project TechCodinz-CKB.
Then show me the architecture graph, highest-risk change areas, graph-aware test gaps,
and the failure cone for the most central component. Cite CKB evidence in the explanation.
```

## ChatGPT / Codex availability

OpenAI changes app/plugin eligibility and developer-mode controls independently of CKB. Do not hard-code a ChatGPT subscription tier into CKB. During development, use the custom MCP/app testing surface available to the account or workspace. For public distribution, package the MCP-backed CKB app as a plugin and follow the current OpenAI plugin submission/review process.

As of July 2026, OpenAI's Plugins Directory is the primary discovery surface for workflow capabilities across ChatGPT and Codex; the CKB integration should therefore be prepared as both a standards-compliant MCP app and a publishable plugin package.
