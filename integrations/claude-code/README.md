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
# → Claude calls ckb_check_boundaries → sees auth layer
# → Generates code that fits your architecture

claude "Refactor the UserRepository class"  
# → Claude calls ckb_analyze_impact on UserRepository
# → Reports risk score and affected files before proceeding
```

## Available MCP Tools

| Tool | Description |
|------|-------------|
| `ckb_scan` | Scan codebase and build knowledge graph |
| `ckb_get_report` | Get latest scan results |
| `ckb_analyze_impact` | Impact analysis for a file:line change |
| `ckb_check_boundaries` | Get architectural layer boundaries |
| `ckb_get_patterns` | Detect architectural patterns |
