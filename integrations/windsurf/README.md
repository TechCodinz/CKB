# CKB for Windsurf (Codeium) — Setup Guide

Windsurf supports the **Model Context Protocol (MCP)**, so CKB plugs in directly.

## Setup (2 minutes)

### Step 1: Start CKB Server
```bash
ckb serve --cors --port 3000
```

### Step 2: Add CKB to Windsurf MCP Config
Edit `~/.codeium/windsurf/mcp_config.json`:

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

### Step 3: Add .windsurfrules to Your Project
```bash
cp integrations/windsurf/.windsurfrules /your/project/.windsurfrules
```

### Step 4: Scan Your Project
```bash
ckb scan /your/project
```

## What Cascade (Windsurf AI) Gets

- Real dependency graph of your codebase
- All architectural violations with severity
- Impact analysis before modifying any file
- Boundary detection (Domain, Application, Infrastructure layers)
- Architectural pattern recognition (MVC, DDD, Clean Architecture, etc.)

## Windsurf-Specific Tips

- Cascade will automatically query CKB when you ask it to "add a feature" or "refactor"
- Use **Cascade Flow** with CKB for multi-step architectural refactoring
- CKB violations appear in Cascade's context — it won't generate code that breaks your rules
