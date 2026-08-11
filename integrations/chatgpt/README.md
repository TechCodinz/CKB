# CKB for ChatGPT (Remote MCP)

CKB exposes a production remote Model Context Protocol endpoint for ChatGPT at:

```text
https://<your-ckb-host>/mcp
```

For the current Render service this is expected to be:

```text
https://ckb-mcp-server.onrender.com/mcp
```

## What ChatGPT can ask CKB

The remote MCP surface is intentionally architecture-focused and repository-safe. It can:

- scan a public GitHub repository into an isolated CKB project
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

The MCP tools do **not** modify the target GitHub repository.

## Authentication

Set a long random `CKB_API_KEY` in the production environment. The MCP endpoint accepts it through either:

```text
Authorization: Bearer <CKB_API_KEY>
```

or:

```text
X-API-Key: <CKB_API_KEY>
```

Do not publish the key or place it in prompts. Configure it through the client/app authentication UI or secret storage.

## ChatGPT setup

1. Use ChatGPT on the web.
2. Open **Settings → Apps** and enable the custom-app/developer flow available for your account/workspace.
3. Create a custom MCP app.
4. Name it **CKB Architecture Intelligence**.
5. Set the MCP URL to `https://ckb-mcp-server.onrender.com/mcp` (or the deployed CKB host).
6. Configure bearer/API-key authentication with the production `CKB_API_KEY`.
7. Scan the server tools and save the app.
8. Add/mention CKB in a new chat and begin with a repository scan.

Example prompt:

```text
Use CKB to scan https://github.com/TechCodinz/CKB as project TechCodinz-CKB. Then show me its architecture graph, highest-risk change areas, test gaps, and the failure cone for the most central component.
```

## Protocol

The endpoint implements stateless MCP Streamable HTTP over `POST /mcp` and negotiates current/common MCP protocol revisions. `GET /mcp` returns `405 Method Not Allowed` because CKB does not currently open a server-to-client SSE stream; all CKB tools are request/response analysis operations.

The tool results include both MCP text content and `structuredContent` so ChatGPT can reason over CKB's native JSON evidence.

## Security model

- `/health` stays public for deployment health checks.
- `/mcp` fails closed unless CKB authentication is configured.
- Repository scanning is limited to the existing Reality v5 GitHub scan surface; local filesystem scanning remains disabled in production.
- Existing CKB API traffic continues through the hardened Reality gateway and its scan concurrency controls.
- MCP tool annotations identify analysis operations as repository-read-only.

## Deployment topology

```text
ChatGPT / MCP client
        |
        v
chatgpt_edge : /mcp + public /health
        |
        v
reality_gateway : auth + scan gate + trace persistence
        |
        v
reality_server_v5 : architecture intelligence engine
        |
        v
CKB Core : graph / AST / impact / memory / telemetry / history
```

The extra edge keeps the existing production gateway unchanged while adding ChatGPT-compatible MCP transport. It can later be folded directly into `reality_gateway` without changing the public `/mcp` contract.
