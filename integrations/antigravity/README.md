# CKB for Antigravity (Google DeepMind) — Integration Guide

## How it Works

**Antigravity already supports CKB natively via MCP** — no extension install needed.

Antigravity is an MCP-native AI coding assistant. When CKB's MCP server is running,
Antigravity automatically discovers and uses these tools during any coding session.

## Setup (1 minute)

### Step 1: Start CKB MCP Server
```bash
ckb serve --cors --port 3000
```

### Step 2: Connect to Antigravity
Add to your Antigravity workspace MCP config:
```json
{
  "mcpServers": {
    "ckb": {
      "command": "ckb-mcp-server",
      "args": ["--port", "3000", "--cors"]
    }
  }
}
```

### Step 3: Scan Your Project
```bash
ckb scan /your/project
```

## Available MCP Tools (Auto-discovered by Antigravity)

| Tool | Description |
|------|-------------|
| `ckb_scan_project` | Scan codebase and build knowledge graph |
| `ckb_check_drift` | Current drift, circular-dependency and layer-boundary violations |
| `ckb_analyze_impact` | Analyze impact of a change at file:line |
| `ckb_generate_ai_rules` | AI rules generated from the patterns and conventions detected in the codebase |
| `ckb_get_prompt_context` | Bounded, token-optimized architecture context for a query |

## Example — Antigravity with CKB Context

After connecting, Antigravity will:

1. **Before modifying code** — automatically call `ckb_analyze_impact` to assess risk
2. **Before adding features** — call `ckb_check_drift` to place code correctly
3. **When you ask "why is X failing"** — query the violation graph for root causes
4. **During refactoring** — track impact propagation across the whole codebase

## ANTIGRAVITY.md (Optional)

Place this file at your project root. Antigravity reads it at session start:

```markdown
# CKB Integration Active

This project uses CKB for architectural intelligence.
MCP Server: http://localhost:3000

Before modifying any file:
- Call ckb_analyze_impact and report risk_score
- If risk_score > 0.7: warn before proceeding

Before adding features:
- Call ckb_check_drift to place code in the correct layer
- Call ckb_generate_ai_rules / ckb_get_prompt_context to follow existing conventions
```
