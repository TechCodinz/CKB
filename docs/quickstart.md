# CKB Quick Start Guide
## Get architectural intelligence in 5 minutes

### 1. Install CKB

**macOS / Linux:**
```bash
curl -fsSL https://ckb.dev/install.sh | sh
```

**Windows:**
```powershell
iwr https://ckb.dev/install.ps1 -useb | iex
```

**Or with Cargo:**
```bash
cargo install ckb-cli
```

### 2. Scan Your First Project
```bash
cd your-project
ckb scan .
```

You'll see output like:
```text
✅ Scan complete!
- Files: 127
- Nodes: 1,432
- Edges: 3,891
- Patterns detected: 2 (Layered Architecture, Modular)
- Violations: 3
```

### 3. Install VS Code Extension

1. Open VS Code
2. Go to Extensions (Ctrl+Shift+X)
3. Search for "CKB"
4. Click Install

Now you'll see real-time architectural feedback as you code.

### 4. Connect Your AI Assistant

**For Cursor:**
Add to `~/.cursor/mcp.json`:
```json
{
  "mcpServers": {
    "ckb": {
      "command": "ckb",
      "args": ["serve"]
    }
  }
}
```

**For Claude Desktop:**
Add to Claude config:
```json
{
  "mcpServers": {
    "ckb": {
      "command": "ckb",
      "args": ["serve"]
    }
  }
}
```

**For Continue.dev:**
Add to `~/.continue/config.json`:
```json
{
  "experimental": {
    "mcpServers": {
      "ckb": {
        "command": "ckb",
        "args": ["serve"]
      }
    }
  }
}
```

### 5. View Your Dashboard

Open https://app.ckb.dev and log in.

You'll see:
- Project health scores
- Violation trends
- Team activity
- Recommended actions

### 6. Set Up CI/CD

**GitHub Actions:**
```yaml
name: CKB Check
on: [pull_request]
jobs:
  ckb-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Install CKB
        run: curl -fsSL https://ckb.dev/install.sh | sh
      - name: Check architecture
        run: ckb check . --strict
```

**GitLab CI:**
```yaml
ckb-check:
  stage: test
  script:
    - curl -fsSL https://ckb.dev/install.sh | sh
    - ckb check . --strict
```

### Next Steps

- Configure custom rules
- Invite your team
- Integrate with Slack
- Schedule architectural review

Questions? Join our Discord or email support@ckb.dev
