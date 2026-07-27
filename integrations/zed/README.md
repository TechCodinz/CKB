# CKB for Zed Editor — Setup Guide

Zed supports MCP and extensions. CKB integrates via both.

## Method 1: MCP Context Server (Recommended)

Edit `~/.config/zed/settings.json`:

```json
{
  "context_servers": {
    "ckb": {
      "command": {
        "path": "ckb-mcp-server",
        "args": ["--port", "3000", "--cors"]
      }
    }
  }
}
```

Then start the CKB server:
```bash
ckb serve --cors --port 3000
```

Zed's AI Assistant (powered by Claude) will automatically have access to your architectural graph.

## Method 2: Tasks (Run CKB from Zed)

Add to `.zed/tasks.json` in your project:

```json
[
  {
    "label": "CKB: Scan Project",
    "command": "ckb scan ${ZED_WORKTREE_ROOT}",
    "reveal": "always",
    "hide": "never",
    "env": {}
  },
  {
    "label": "CKB: Check Architecture",
    "command": "ckb check ${ZED_WORKTREE_ROOT}",
    "reveal": "always"
  },
  {
    "label": "CKB: Start MCP Server",
    "command": "ckb serve --cors",
    "reveal": "always",
    "hide": "on_success"
  }
]
```

Run tasks via: `Cmd+Shift+P` → "task: spawn" → select CKB task

## Using with Zed AI Assistant

Once CKB is configured as a context server:

1. Open the AI Assistant panel (`Cmd+?`)
2. Type: `@ckb` to invoke CKB tools directly
3. Ask: "Check if adding a database call here would violate our architecture"

Zed's Claude integration will automatically call `ckb_analyze_impact` and show results inline.
