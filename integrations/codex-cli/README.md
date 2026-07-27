# CKB for OpenAI Codex CLI — Setup Guide

The OpenAI Codex CLI supports MCP, so CKB plugs in as a tool provider.

## Setup

### Step 1: Start CKB Server
```bash
ckb serve --cors --port 3000
```

### Step 2: Add CKB to Codex MCP Config
Edit `~/.codex/config.toml`:
```toml
[mcp]
servers = [
  { name = "ckb", command = "ckb-mcp-server", args = ["--port", "3000", "--cors"] }
]
```

Or via CLI:
```bash
codex mcp add ckb --command "ckb-mcp-server --port 3000 --cors"
```

### Step 3: Add AGENTS.md
```bash
cp integrations/codex/AGENTS.md /your/project/AGENTS.md
```
Codex CLI reads `AGENTS.md` automatically.

### Step 4: Scan First
```bash
ckb scan .
```

## Usage

```bash
# Codex automatically uses CKB for context
codex "Add a rate limiter to the API layer"
# → Queries CKB → knows your API layer location
# → Generates architecture-compliant code

codex "What would break if I delete src/auth/token.ts?"
# → Calls ckb_analyze_impact → shows full impact tree
```

## AGENTS.md Example
```markdown
# Project Agents

## CKB Architectural Assistant
- Server: ckb (MCP)
- Use ckb_check_boundaries before adding features
- Use ckb_analyze_impact before modifying existing files
- Risk score > 0.7: warn user before proceeding
```
