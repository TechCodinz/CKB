# CKB — Code Knowledge Base

> **Architectural intelligence for AI-era development.** CKB scans your codebase, builds a dependency graph, detects architectural drift, and exposes everything through the Model Context Protocol so AI coding assistants like Cursor, Claude Desktop, and Continue.dev understand your architecture in real time.

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
| **Drift detection** | Finds forbidden deps, circular deps, god objects, layer violations |
| **Impact analysis** | Shows what breaks when you change a file or function |
| **MCP Server** | Exposes architecture data to any MCP-compatible AI assistant |
| **CI/CD integration** | SARIF, JUnit, and JSON output for GitHub Actions, GitLab CI, etc. |
| **VS Code extension** | Real-time diagnostics and status bar integration |
| **Web Dashboard** | React-based UI with charts, graph visualization, and violation tracking |
## User & Integration Guide

📖 **Complete User Guide**: For detailed setup instructions across IDEs, AI models, CLI, Cloud APIs, and CI/CD pipelines, see [USER_GUIDE.md](USER_GUIDE.md).

- ⚡ **[VS Code Extension](USER_GUIDE.md#1-vs-code-extension-setup--usage)**: Set API Key via Command Palette (`CKB: Set API Key`) and visualize real-time AST graphs.
- 🤖 **[AI Integration](USER_GUIDE.md#2-ai-assistant-integration-claude-code-codex-cursor-windsurf)**: Connect Claude Code, OpenAI Codex, Cursor, and Windsurf via MCP 1.0 (`ckb-mcp-server --stdio`).
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

# Start the MCP server for AI integration
ckb serve --port 3000 --cors
```

### Connect AI Assistants

Add to your Cursor / Claude Desktop MCP config:

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

```
ckb/
├── core/          # Rust engine — parsing, graph, analysis, storage
├── cli/           # CLI binary with 12 commands
├── mcp-server/    # MCP + REST server for AI integration
├── web/           # React dashboard with charts & graph view
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

# Run the MCP server
cargo run --bin ckb-mcp-server
```

## License

Dual-licensed under [MIT](LICENSE) and [Apache 2.0](LICENSE).

Copyright © 2026 CKB Contributors
