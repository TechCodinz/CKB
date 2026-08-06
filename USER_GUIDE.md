# CKB Complete Integration & User Guide

Welcome to the **Code Knowledge Base (CKB)** User Guide! This guide provides comprehensive, step-by-step instructions for integrating and running CKB across your IDEs, AI coding assistants (Claude Code, OpenAI Codex, Cursor, Windsurf), CLI tools, cloud APIs, and CI/CD pipelines.

---

## Table of Contents

1. [VS Code Extension Setup & Usage](#1-vs-code-extension-setup--usage)
2. [AI Assistant Integration (Claude Code, Codex, Cursor, Windsurf)](#2-ai-assistant-integration-claude-code-codex-cursor-windsurf)
3. [CLI Reference & Commands](#3-cli-reference--commands)
4. [Cloud Web Dashboard & REST API Usage](#4-cloud-web-dashboard--rest-api-usage)
5. [CI/CD Pipeline Integration (GitHub Actions)](#5-cicd-pipeline-integration-github-actions)

---

## 1. VS Code Extension Setup & Usage

The CKB VS Code extension provides real-time architectural diagnostics, dependency graph visualizations, and instant drift detection directly inside your editor.

### Installation
1. Open VS Code.
2. Go to **Extensions** (`Ctrl+Shift+X` or `Cmd+Shift+X`).
3. Search for **`CKB`** or **`ckb-vscode`** and click **Install**.

### Setting Your API Key
1. Open the VS Code Command Palette: `Ctrl+Shift+P` (Windows/Linux) or `Cmd+Shift+P` (macOS).
2. Type **`CKB: Set API Key`** and press `Enter`.
3. Paste your CKB API Key (generated from the [CKB Dashboard API Keys Page](https://ckb-nu.vercel.app/api-keys)).
4. Press `Enter` to save.

### Key Commands
- **`CKB: Scan Current Workspace`**: Runs an instant AST architecture scan of your active project.
- **`CKB: Show Dependency Graph`**: Opens an interactive dependency visualizer in a side tab.
- **`CKB: Analyze Change Impact`**: Analyzes the active file and highlights upstream/downstream breaking changes.

---

## 2. AI Assistant Integration (Claude Code, Codex, Cursor, Windsurf)

CKB implements the **Model Context Protocol (MCP 1.0)**, giving AI coding models full awareness of your codebase structure, types, and architectural constraints.

### A. Claude Code & Claude Desktop

Add CKB to your `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "ckb": {
      "command": "ckb-mcp-server",
      "args": ["--stdio"],
      "env": {
        "CKB_API_KEY": "your_ckb_api_key_here"
      }
    }
  }
}
```

### B. Cursor & Windsurf IDEs

Add CKB to `.cursor/mcp.json` in your workspace or global settings:

```json
{
  "mcpServers": {
    "ckb": {
      "command": "ckb-mcp-server",
      "args": ["--stdio"]
    }
  }
}
```

### C. OpenAI Codex & Custom LLM Scripts

You can query the CKB REST MCP server directly over HTTP:

```bash
# Start the local server
ckb-mcp-server --port 3000 --cors
```

Make JSON-RPC 2.0 requests:

```bash
curl -X POST http://localhost:3000/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "tools/call",
    "params": {
      "name": "analyze_impact",
      "arguments": { "path": "./src/auth/service.ts" }
    }
  }'
```

---

## 3. CLI Reference & Commands

Install the CKB CLI globally:

```bash
# Cargo install
cargo install --path cli

# Or binary curl script (Linux / macOS)
curl -fsSL https://ckb.dev/install.sh | sh
```

### Core CLI Commands

| Command | Usage | Description |
|---|---|---|
| `ckb scan <path>` | `ckb scan ./` | Scans codebase and generates dependency graph & drift report |
| `ckb check <path>` | `ckb check ./ --strict` | Evaluates architectural rules; exits with code 1 on violations |
| `ckb impact <path> <file>` | `ckb impact ./ src/db.ts` | Calculates blast radius of modifying a target file |
| `ckb export <path>` | `ckb export ./ --format mermaid` | Exports graph as Mermaid diagrams or JSON |
| `ckb serve` | `ckb serve --port 3000` | Launches local MCP JSON-RPC & REST server |

---

## 4. Cloud Web Dashboard & REST API Usage

### Dashboard Access
Access your cloud workspace at: **[https://ckb-nu.vercel.app](https://ckb-nu.vercel.app)**

### Generating API Keys
1. Log in to the CKB Dashboard.
2. Click **API Keys** in the navigation header.
3. Click **+ Generate New API Key**.
4. Copy your secret key (`ckb_live_...`).

### REST API Examples

#### 1. Submit a Codebase Scan
```bash
curl -X POST https://ckb-backend-api.onrender.com/api/v1/scans \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{ "projectPath": "./" }'
```

#### 2. Fetch Latest Architectural Drift Report
```bash
curl -X GET https://ckb-backend-api.onrender.com/api/v1/scans/latest \
  -H "Authorization: Bearer YOUR_API_KEY"
```

---

## 5. CI/CD Pipeline Integration (GitHub Actions)

Add CKB architectural validation to your GitHub repository to catch circular dependencies, forbidden imports, and architectural drift before pull requests are merged.

Create `.github/workflows/ckb-check.yml`:

```yaml
name: CKB Architecture Check

on:
  push:
    branches: [ main ]
  pull_request:
    branches: [ main ]

jobs:
  architecture-check:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout Code
        uses: actions/checkout@v4

      - name: Install Rust Toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Install CKB CLI
        run: cargo install --path cli

      - name: Run Architectural Drift Check
        run: ckb check ./ --strict --report-format sarif > ckb-report.sarif

      - name: Upload SARIF Security & Drift Report
        uses: github/codeql-action/upload-sarif@v3
        with:
          sarif_file: ckb-report.sarif
```

---

*Need help? Reach out on GitHub Issues at [github.com/TechCodinz/CKB/issues](https://github.com/TechCodinz/CKB/issues).*
