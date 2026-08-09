# CKB — Code Knowledge Base

> **Architectural intelligence for AI-era development.** CKB scans your codebase, builds a dependency graph, detects architectural drift, and exposes evidence-backed software memory through the Model Context Protocol so AI coding assistants can understand architecture without repeatedly rediscovering the repository.

[![Build Status](https://github.com/TechCodinz/CKB/workflows/build/badge.svg)](https://github.com/TechCodinz/CKB/actions)
[![VS Code Marketplace](https://img.shields.io/vscode-marketplace/v/TechCodinz.ckb-vscode?color=blue&logo=visualstudiocode)](https://marketplace.visualstudio.com/items?itemName=TechCodinz.ckb-vscode)
[![Open VSX](https://img.shields.io/open-vsx/v/TechCodinz/ckb-vscode?color=purple)](https://open-vsx.org/extension/TechCodinz/ckb-vscode)
[![MCP 1.0](https://img.shields.io/badge/MCP-1.0-emerald?logo=anthropic)](https://modelcontextprotocol.io)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange)](https://www.rust-lang.org/)

---

## What CKB Does

| Feature | Description |
|---|---|
| **Multi-language parsing** | TypeScript, JavaScript, Python, Go, Rust, Java via tree-sitter |
| **Dependency graph** | Builds a full graph of imports, exports, calls, type relationships |
| **Architecture Memory** | Persists the software graph across model sessions and retrieves bounded evidence-backed context |
| **Freshness + Memory Delta** | Detects new commits and uncommitted worktree changes, then reports exact graph changes after refresh |
| **Causal reasoning** | Explains proven architecture paths and transitive change/failure cones without inventing runtime execution |
| **Code DNA** | Explainable graph/runtime-derived health and structural-risk metrics |
| **Drift detection** | Finds forbidden deps, circular deps, god objects, layer violations |
| **Impact analysis** | Shows what can be affected when you change a file or function |
| **Runtime intelligence** | Separates static AST evidence from observed OpenTelemetry execution paths |
| **MCP Server** | Exposes architecture data and durable memory to MCP-compatible AI assistants |
| **CI/CD integration** | SARIF, JUnit, and JSON output for GitHub Actions, GitLab CI, etc. |
| **VS Code extension** | Real-time diagnostics and status bar integration |
| **Web Dashboard** | React-based Code X-Ray, Living Graph, Time Machine and evidence views |

## User & Integration Guide

📖 **Complete User Guide**: For detailed setup instructions across IDEs, AI models, CLI, Cloud APIs, and CI/CD pipelines, see [USER_GUIDE.md](USER_GUIDE.md).

- ⚡ **[VS Code Extension](USER_GUIDE.md#1-vs-code-extension-setup--usage)**: Set API Key via Command Palette (`CKB: Set API Key`) and visualize real-time AST graphs.
- 🤖 **[AI Integration](USER_GUIDE.md#2-ai-assistant-integration-claude-code-codex-cursor-windsurf)**: Connect Claude Code, OpenAI Codex, Cursor, and Windsurf via MCP.
- 💻 **[CLI Reference](USER_GUIDE.md#3-cli-reference--commands)**: Run `ckb scan`, `ckb check`, `ckb impact`, and `ckb export`.
- ☁️ **[Cloud Dashboard & API](USER_GUIDE.md#4-cloud-web-dashboard--rest-api-usage)**: Generate API Keys and query architectural data via REST API.
- ⚙️ **[CI/CD Pipelines](USER_GUIDE.md#5-cicd-pipeline-integration-github-actions)**: Automate drift detection in GitHub Actions workflows.

---

## Quick Start

### Install the CLI

```bash
# macOS / Linux
curl -fsSL https://ckb.dev/install.sh | sh

# Or build from source
cargo install --path cli
```

### Scan a Project

```bash
# Full scan with table output
ckb scan ./my-project

# Check for architectural drift (CI-friendly)
ckb check ./my-project --strict --report-format sarif

# Analyze change impact
ckb impact ./my-project src/auth/login.ts 42

# Export dependency graph
ckb export ./my-project --format mermaid --output graph.mmd

# Start the REST/MCP server
ckb serve --port 3000 --cors
```

## Durable Architecture Memory for AI Models

CKB also ships a dedicated stdio MCP binary whose job is to **remember the software architecture across AI/model sessions** rather than forcing each new assistant session to rediscover the codebase.

Build it:

```bash
cargo build --release -p ckb-mcp-server --bin architecture_memory_mcp
```

Configure an MCP-compatible client to launch it:

```json
{
  "mcpServers": {
    "ckb-memory": {
      "command": "/absolute/path/to/architecture_memory_mcp"
    }
  }
}
```

The memory server exposes tools including:

```text
ckb_memory_scan        build + persist software memory
ckb_memory_resume      resume it in a future model session
ckb_memory_status      verify commit + worktree freshness
ckb_memory_refresh     rescan stale source and return exact graph delta
ckb_memory_delta       retrieve the last architecture mutation set
ckb_memory_query       bounded natural-language/symbol architecture retrieval
ckb_symbol_memory      focused memory before editing one symbol
ckb_context_capsule    compact model-ready context under a character budget
ckb_code_dna           explainable architecture health/risk
ckb_causal_path        prove why A depends on B
ckb_failure_cone       transitive upstream change/failure exposure
```

A model can therefore resume yesterday's architecture knowledge, detect that the repository changed—even when the edits are **uncommitted**—refresh the graph, see exactly which symbols/relationships appeared or disappeared, and retrieve only the relevant context for the next edit.

CKB keeps evidence types separate: a static dependency is not called runtime execution; runtime claims require observed telemetry; simulation remains prediction.

### Connect AI Assistants to the standard server

For clients using the standard CKB MCP/REST process rather than the dedicated memory process:

```json
{
  "mcpServers": {
    "ckb": {
      "command": "ckb-mcp-server",
      "args": ["--port", "3000"]
    }
  }
}
```

## Architecture

```text
ckb/
├── core/          # Rust engine — parsing, graph, analysis, storage
├── cli/           # CLI binary
├── mcp-server/    # MCP + Reality REST servers + durable Architecture Memory
├── web/           # React dashboard with X-Ray and graph visualization
├── integrations/  # VS Code, JetBrains, Cursor extensions
├── bindings/      # Node.js, Python, WASM bindings
├── backend/       # TypeScript API (billing, auth, security)
├── landing/       # Marketing landing page
└── deploy/        # Docker + Kubernetes configs
```

## Pricing

| Plan | Price | Features |
|---|---|---|
| **Free** | $0 | 1 project, 1,000 nodes, CLI only |
| **Pro** | $29/mo | Unlimited projects, dashboard, priority support |
| **Team** | $99/mo | 10 seats, SSO, audit logs, shared dashboards |
| **Enterprise** | Custom | On-prem, SLA, dedicated support |

## Development

```bash
# Build all workspace crates
cargo build --workspace

# Run tests
cargo test --workspace

# Run the CLI
cargo run --bin ckb-cli -- scan ./my-project

# Run the standard MCP server
cargo run --bin ckb-mcp-server

# Run the durable Architecture Memory MCP server
cargo run -p ckb-mcp-server --bin architecture_memory_mcp
```

## License

Dual-licensed under [MIT](LICENSE) and [Apache 2.0](LICENSE).

Copyright © 2026 CKB Contributors
