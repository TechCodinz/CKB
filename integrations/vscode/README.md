# CKB for VS Code

> Real-time architectural intelligence inside Visual Studio Code.

## Features

- **🔍 Scan** — Run a full codebase scan via the status bar or command palette
- **⚠️ Inline Diagnostics** — Architecture violations appear as squiggly-line errors/warnings directly in your files
- **⚡ Impact Analysis** — Right-click any line → "CKB: Analyze Impact" to see what would break
- **🛡️ Architecture Check** — Instant pass/fail check for CI-style workflow
- **🤖 MCP Integration** — Start the MCP server from inside VS Code for Claude / Cursor / GitHub Copilot

## Requirements

1. **CKB CLI installed** — `curl -fsSL https://ckb.dev/install.sh | sh` (or `cargo install ckb-cli`)
2. **CKB MCP Server** — Start with `ckb serve` or click "Start MCP Server" in the command palette

## Usage

### Command Palette (`Ctrl+Shift+P`)

| Command | Description |
|---------|-------------|
| `CKB: Scan Project` | Full architectural scan |
| `CKB: Check Architecture` | Pass/fail violation check |
| `CKB: Analyze Impact` | Impact analysis at cursor |
| `CKB: Show Status` | Quick action picker |
| `CKB: Start MCP Server` | Start server for AI integration |

### Status Bar
Click the **$(shield) CKB** item in the bottom status bar to open the action picker.

### Automatic Behavior
- Scans automatically when a workspace is opened
- Re-checks architecture when source files change (debounced)
- Violations appear as problems in the Problems panel (`Ctrl+Shift+M`)

## Config

Add to your `.vscode/settings.json`:
```json
{
  "ckb.serverUrl": "http://localhost:3000",
  "ckb.autoScanOnOpen": true,
  "ckb.showDiagnostics": true
}
```

## Extension Settings

| Setting | Default | Description |
|---------|---------|-------------|
| `ckb.serverUrl` | `http://localhost:3000` | CKB MCP server URL |
| `ckb.autoScanOnOpen` | `true` | Auto-scan when workspace opens |
| `ckb.showDiagnostics` | `true` | Show inline error/warning squiggles |
