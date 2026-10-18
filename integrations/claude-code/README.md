# CKB for Claude Code (Anthropic CLI) — Setup Guide

Claude Code (`claude`) supports MCP natively. CKB integrates in minutes.

## Setup

### Step 1: Start CKB MCP Server
```bash
ckb serve --cors --port 3000
```

### Step 2: Add CKB to Claude's MCP Config
```bash
# Creates/updates ~/.claude.json
claude mcp add ckb --command ckb-mcp-server --args "--port 3000 --cors"
```

Or manually edit `~/.claude.json`:
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

### Step 3: Add CLAUDE.md to Your Project
```bash
cp integrations/claude-code/CLAUDE.md /your/project/CLAUDE.md
```
Claude Code reads `CLAUDE.md` automatically at session start.

### Step 4: Scan Your Project
```bash
ckb scan /your/project
```

## Usage in Claude Code Sessions

```bash
# Claude will automatically use CKB tools
claude "Add OAuth2 authentication to this service"
# → Claude calls ckb_check_drift → sees auth layer boundary violations
# → Generates code that fits your architecture

claude "Refactor the UserRepository class"  
# → Claude calls ckb_analyze_impact on UserRepository
# → Reports risk score and affected files before proceeding
```

## Available MCP Tools

| Tool | Description |
|------|-------------|
| `ckb_scan_project` | Scan codebase and build knowledge graph |
| `ckb_check_drift` | Current drift, circular-dependency and layer-boundary violations |
| `ckb_analyze_impact` | Impact analysis for a file:line change |
| `ckb_generate_ai_rules` | AI rules generated from the patterns and conventions detected in the codebase |
| `ckb_get_prompt_context` | Bounded, token-optimized architecture context for a query |
