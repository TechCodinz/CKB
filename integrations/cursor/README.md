# CKB for Cursor — Setup Guide

CKB supercharges Cursor by giving it real-time awareness of your architecture via the **Model Context Protocol (MCP)**.

## Quick Setup (2 minutes)

### Step 1: Start CKB MCP Server
```bash
# After installing CKB CLI:
ckb serve --cors --port 3000
```

### Step 2: Add to Cursor MCP Config
Edit `~/.cursor/mcp.json` (create if it doesn't exist):

```json
{
  "mcpServers": {
    "ckb": {
      "command": "ckb-mcp-server",
      "args": ["--port", "3000", "--cors"],
      "env": {
        "CKB_PROJECT_PATH": "${workspaceFolder}"
      }
    }
  }
}
```

### Step 3: Copy .cursorrules to Your Project
```bash
cp integrations/cursor/.cursorrules /your/project/.cursorrules
```

### Step 4: Scan Your Project
```bash
ckb scan /your/project
```

Now Cursor will **automatically query CKB** before generating code, so it:
- Knows your layer boundaries
- Avoids creating circular dependencies
- Understands which modules are stable vs. unstable
- Warns you if a suggestion would violate architecture rules

## What CKB Exposes to Cursor

| Tool | What Cursor gets |
|------|-----------------|
| `ckb_scan` | Full dependency graph and violations |
| `ckb_check_boundaries` | Current architectural boundaries |
| `ckb_analyze_impact` | What a change would break |
| `ckb_get_patterns` | Detected architectural patterns (MVC, DDD, etc.) |
| `ckb_suggest` | Architectural suggestions for current file |

## Example Cursor Prompts (after CKB integration)

```
"Add authentication to this service" 
→ Cursor queries CKB → sees auth should go in /src/domain layer → generates compliant code

"Refactor the UserService"
→ Cursor queries CKB → sees UserService has 12 direct dependents → warns you first

"Fix circular dependency between parser.ts and ast.ts"
→ CKB shows the exact cycle → Cursor suggests the correct extraction
```
