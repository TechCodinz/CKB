# CKB Quick Start Guide
## Get architectural intelligence in 5 minutes

### 1. Install CKB

**CLI — build from source (all platforms):**
```bash
git clone https://github.com/TechCodinz/CKB.git
cd CKB
cargo install --path cli
```
Prebuilt CLI binaries are attached to
[GitHub Releases](https://github.com/TechCodinz/CKB/releases).

**VS Code / VSCodium extension (macOS / Linux):**
```bash
curl -fsSL https://ckb-nu.vercel.app/install.sh | sh
```
That script installs the CKB editor extension from Open VSX — it is
not a CLI installer. (There is no `ckb.dev/install.ps1`; Windows users
install the extension from the Open VSX / Marketplace listing.)

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

Open https://ckb-nu.vercel.app and log in.

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
      # This repo ships a composite action that builds ckb-cli from
      # source and runs the check (posting a PR summary comment):
      - name: CKB architecture check
        uses: TechCodinz/CKB/.github/actions/ckb-scan@main
```

**GitLab CI:**
```yaml
ckb-check:
  stage: test
  script:
    # Install ckb-cli first: build from source (cargo install --path cli
    # in a clone of https://github.com/TechCodinz/CKB) or download a
    # prebuilt binary from https://github.com/TechCodinz/CKB/releases
    - ckb check . --strict
```

### Next Steps

- Configure custom rules
- Invite your team
- Integrate with Slack
- Schedule architectural review

Questions? Open an issue at https://github.com/TechCodinz/CKB/issues
